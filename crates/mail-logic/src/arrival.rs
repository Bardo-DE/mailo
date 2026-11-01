//! What mail that has just arrived is made to do: the user's rules, muted conversations and the
//! junk filter, decided as one fold over the arrived messages and ending in a [`Commit`].
//!
//! The fold keeps the conversations in memory and applies each operation to them as it decides
//! it, so a later rule, a muted conversation or the junk filter sees what the earlier ones left,
//! exactly as the store's re-reads did when each operation was written before the next was
//! decided. Every operation is an [`Op`] a person would apply by hand (through
//! [`crate::act`]), so it changes the message here and tells the server in the same words.
//!
//! What a rule asks that only a running client can do (send mail, say a message arrived) is not
//! done here: it comes back as an [`ArrivalEffect`] for the caller to carry out after the commit.
//!
//! The commit is one value for the whole arrival: the labels the rules made, then every
//! operation's patch in the order it was decided, then each operation's server half, then what
//! the junk filter learned. A message that is gone, or on another account, is left out.

use crate::act::{Act, ThreadView, applied};
use crate::junk::{
    self, Acquaintance, Filtering, Heed, JUNK_LABEL, Order, Policy, THRESHOLD, Treat,
};
use chrono::{DateTime, Utc};
use mail_domain::filter::{MatchCtx, Placed};
use mail_domain::rule::{self, one_message};
use mail_domain::{
    AccountCaps, AfterMatch, Body, Change, ChangeId, Commit, Judged, JunkCounts, Label, LabelId,
    LabelOrigin, MailboxRef, MailboxRole, Membership, Message, MessageId, Mute, Op, Patch, Placing,
    Queue, ReadState, RemoteIntent, Rule, RuleAction, RuleId, Star, Target,
};
use porter_core::AccountId;

/// The fresh identifiers a decision needs, handed in so that none is drawn inside it.
pub trait Fresh {
    /// The id of a label a rule makes.
    fn label(&mut self) -> LabelId;
    /// The id of the patch the whole arrival is written as.
    fn change(&mut self) -> ChangeId;
}

/// What the junk filter needs to know of one arrived message besides the message.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Standing {
    /// What the records say of its sender.
    pub acquaintance: Acquaintance,
    /// Whether the server that delivered it said it is spam.
    pub server_spam: bool,
    /// What the account's filter knows about the message's words.
    pub counts: JunkCounts,
}

/// What is known of one arrived message beyond the conversation it is in.
#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub message: MessageId,
    pub standing: Standing,
    /// Where the server holds the message and what is queued to move it, which is what a rule
    /// about a folder asks.
    pub places: Placing,
}

/// What an arrival is decided over.
#[derive(Debug, Clone, Copy)]
pub struct ArrivalSnapshot<'a> {
    pub account: &'a AccountId,
    /// Whether the account has a server to tell. A local folder's operations are made here
    /// alone: its outbox is never drained, so nothing is put in it.
    pub has_server: bool,
    /// What the server was seen to support in the pass that brought the mail, which is not
    /// always what the accounts were read with.
    pub caps: &'a AccountCaps,
    /// The account's rules, in any order; [`rule::ordered`] orders the enabled ones.
    pub rules: &'a [Rule],
    pub labels: &'a [Label],
    /// The addresses of the VIPs, as a rule asking `is:vip` sees them.
    pub vips: &'a [String],
    /// The user's full names, from their identities.
    pub names: &'a [String],
    pub policy: Policy,
    /// The conversations the messages are in, each with every message in it.
    pub threads: &'a [ThreadView],
    pub evidence: &'a [Evidence],
}

/// Whether a run of rules carries out the actions that send mail or raise a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaking {
    /// Mail arriving, or messages the user picked: what such an action is for.
    Yes,
    /// The mail already here, in bulk: a forward of every old invoice, or a notification for
    /// each, is never what "apply it to what's here" meant.
    No,
}

/// A forward or a reply a rule asks for, with what the rule says about making it. Not a built
/// message: the caller drafts and queues it after the commit, through the guard that every
/// automatic send goes through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftRequest {
    pub message: MessageId,
    pub action: RuleAction,
    pub rule: RuleId,
    /// When the rule was made: it sends only for mail dated after.
    pub made: DateTime<Utc>,
    /// For a Reply, the days it leaves between two answers to one sender.
    pub reply_days: u32,
}

/// What an arrival asks of the caller once its commit is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArrivalEffect {
    /// A rule asked to hear of this message, once.
    Notice(MessageId),
    /// A rule asked for a forward or a reply.
    RuleSend(DraftRequest),
}

/// What an arrival came to.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    pub commit: Commit,
    pub effects: Vec<ArrivalEffect>,
    /// Messages a rule acted on, each with the names of the rules that did, in order.
    pub acted: Vec<(MessageId, Vec<String>)>,
    /// Messages a rule was run over.
    pub examined: usize,
    /// Messages that arrived in a muted conversation and were kept out of the inbox.
    pub muted: Vec<MessageId>,
    /// Messages the junk filter judged junk and moved or marked.
    pub junk: Vec<MessageId>,
}

impl Arrival {
    /// Whether there is anything to write.
    pub fn writes(&self) -> bool {
        !(self.commit.forward.changes.is_empty()
            && self.commit.queue.is_empty()
            && self.commit.rows.is_empty())
    }
}

/// Mail that has just arrived: the junk filter and the rules, in the order the policy says, and a
/// muted conversation's new mail kept out of the way.
///
/// `arrived` is what a sync pass stored for the first time, so a message is acted on once, when it
/// first appears. Only mail that arrived in the inbox meets the rules: that is delivery, which is
/// what a rule is about and all a server's Sieve script ever sees. Mute comes after the rules, so
/// a rule still sees a muted conversation's mail as it arrived and may file it, label it or
/// throw it away; whatever of it is still in the inbox afterwards is archived, and all of it is
/// marked read.
pub fn on_arrival(
    snapshot: &ArrivalSnapshot<'_>,
    arrived: &[MessageId],
    now: DateTime<Utc>,
    fresh: &mut dyn Fresh,
) -> Arrival {
    let ordered = rule::ordered(snapshot.rules);
    let mut fold = Fold::new(*snapshot, now, fresh);
    if snapshot.policy.order == Order::BeforeRules {
        fold.sift(arrived);
    }
    for id in arrived {
        let Some(message) = fold.message(*id) else {
            continue;
        };
        if message.account != *snapshot.account || message.mailbox != MailboxRole::Inbox {
            continue;
        }
        if !ordered.is_empty() {
            fold.rules_over(*id, &ordered, Speaking::Yes);
        }
        fold.muted(*id);
    }
    if snapshot.policy.order == Order::AfterRules {
        fold.sift(arrived);
    }
    fold.finish()
}

/// The account's enabled rules, in order, over messages the user picked, or the one rule the user
/// asked to run over the mail already here (the snapshot then holds that rule alone): every
/// action is carried out, those that send or notify included unless `speaking` is
/// [`Speaking::No`]. A message on another account, or gone, is left out.
pub fn apply_rules(
    snapshot: &ArrivalSnapshot<'_>,
    picked: &[MessageId],
    speaking: Speaking,
    now: DateTime<Utc>,
    fresh: &mut dyn Fresh,
) -> Arrival {
    let ordered = rule::ordered(snapshot.rules);
    let mut fold = Fold::new(*snapshot, now, fresh);
    if ordered.is_empty() {
        return fold.finish();
    }
    for id in picked {
        let Some(message) = fold.message(*id) else {
            continue;
        };
        if message.account != *snapshot.account {
            continue;
        }
        fold.rules_over(*id, &ordered, speaking);
    }
    fold.finish()
}

struct Fold<'a> {
    snapshot: ArrivalSnapshot<'a>,
    now: DateTime<Utc>,
    fresh: &'a mut dyn Fresh,
    /// The conversations as the operations decided so far leave them.
    views: Vec<ThreadView>,
    /// The account's labels, and those the rules made.
    labels: Vec<Label>,
    places: Vec<(MessageId, Placing)>,
    forward: Vec<Change>,
    queue: Vec<Queue>,
    rows: Vec<mail_domain::Row>,
    /// What the filter has taught itself so far this fold: the words of each message learned
    /// as spam, which the messages judged after see in their counts.
    taught: Vec<Vec<String>>,
    effects: Vec<ArrivalEffect>,
    acted: Vec<(MessageId, Vec<String>)>,
    examined: usize,
    muted: Vec<MessageId>,
    junk: Vec<MessageId>,
}

impl<'a> Fold<'a> {
    fn new(snapshot: ArrivalSnapshot<'a>, now: DateTime<Utc>, fresh: &'a mut dyn Fresh) -> Self {
        Fold {
            views: snapshot.threads.to_vec(),
            labels: snapshot.labels.to_vec(),
            places: snapshot
                .evidence
                .iter()
                .map(|e| (e.message, e.places.clone()))
                .collect(),
            snapshot,
            now,
            fresh,
            forward: Vec::new(),
            queue: Vec::new(),
            rows: Vec::new(),
            taught: Vec::new(),
            effects: Vec::new(),
            acted: Vec::new(),
            examined: 0,
            muted: Vec::new(),
            junk: Vec::new(),
        }
    }

    fn finish(self) -> Arrival {
        let id = self.fresh.change();
        Arrival {
            commit: Commit {
                account: self.snapshot.account.clone(),
                at: self.now,
                forward: Patch {
                    id,
                    changes: self.forward,
                },
                queue: self.queue,
                rows: self.rows,
            },
            effects: self.effects,
            acted: self.acted,
            examined: self.examined,
            muted: self.muted,
            junk: self.junk,
        }
    }

    fn message(&self, id: MessageId) -> Option<&Message> {
        self.views
            .iter()
            .flat_map(|view| &view.messages)
            .find(|m| m.id == id)
    }

    fn view_of(&self, id: MessageId) -> Option<&ThreadView> {
        self.views
            .iter()
            .find(|view| view.messages.iter().any(|m| m.id == id))
    }

    /// The label called `name` on the account, made with `origin` if there is none: the exact
    /// name first, then the same name in another case.
    fn label_named(&mut self, name: &str, origin: LabelOrigin) -> LabelId {
        let found = self.labels.iter().find(|l| l.name == name).or_else(|| {
            self.labels
                .iter()
                .find(|l| l.name.eq_ignore_ascii_case(name))
        });
        if let Some(label) = found {
            return label.id;
        }
        let label = Label {
            id: self.fresh.label(),
            account: self.snapshot.account.clone(),
            name: name.to_owned(),
            color: None,
            origin,
        };
        let id = label.id;
        self.forward.push(Change::LabelUpsert(label.clone()));
        self.labels.push(label);
        id
    }

    /// Apply `op` to one message, as the person's own hand would: here, and to the server when
    /// there is a server half to tell.
    fn perform(&mut self, op: Op, id: MessageId) {
        let Some(index) = self
            .views
            .iter()
            .position(|view| view.messages.iter().any(|m| m.id == id))
        else {
            return;
        };
        let thread = self.views[index].thread.summary.id;
        let act = Act {
            thread,
            target: Target::Messages(vec![id]),
            op,
        };
        let Some(acted) = applied(
            &self.views[index],
            &act,
            self.snapshot.caps,
            self.snapshot.has_server,
            self.now,
        ) else {
            return;
        };
        // Nothing changed, so there is nothing to tell the server either.
        if acted.commit.forward.changes.is_empty() {
            return;
        }
        self.views[index].apply(&acted.commit.forward);
        self.forward.extend(acted.commit.forward.changes);
        for queued in acted.commit.queue {
            self.note_queued(&queued.intent);
            self.queue.push(queued);
        }
    }

    /// What a move or copy queued now does to where its messages are listed.
    fn note_queued(&mut self, intent: &RemoteIntent) {
        let (messages, label, copy) = match intent {
            RemoteIntent::File {
                messages, label, ..
            } => (messages, label, false),
            RemoteIntent::Copy {
                messages, label, ..
            } => (messages, label, true),
            _ => return,
        };
        // The label's name is the folder's path; a label gone leaves nothing to file into.
        let Some(name) = self
            .labels
            .iter()
            .find(|l| l.id == *label)
            .map(|l| l.name.clone())
        else {
            return;
        };
        for message in messages {
            let Some((_, places)) = self.places.iter_mut().find(|(m, _)| m == message) else {
                continue;
            };
            // Queued only for a message the server holds, or will find.
            if places.addresses.is_empty() && !places.unplaced {
                continue;
            }
            if copy {
                places.copying.push(MailboxRef {
                    account: self.snapshot.account.clone(),
                    path: name.clone(),
                });
            } else {
                places.moving.push(name.clone());
            }
        }
    }

    /// Where `message` is listed, as a rule about a folder weighs it.
    fn placed(&self, message: &Message) -> Vec<Placed> {
        let Some((_, places)) = self.places.iter().find(|(m, _)| *m == message.id) else {
            return Vec::new();
        };
        let mut places = places.clone();
        places.copying.sort_by(|a, b| a.path.cmp(&b.path));
        places.copying.dedup();
        places.placed(message.mailbox, &self.snapshot.caps.folders)
    }

    /// Run `rules`, already in order, over one message.
    ///
    /// Each rule is tested against the message as the rules before it left it, as a Sieve script
    /// would: a rule that marks mail read is seen by a later rule asking `is:read`.
    fn rules_over(&mut self, id: MessageId, rules: &[&Rule], speaking: Speaking) {
        self.examined += 1;
        let mut fired = Vec::new();
        for rule in rules {
            let fits = {
                let Some(message) = self.message(id) else {
                    break;
                };
                let summary = one_message(message);
                let corpus = match &message.body {
                    Body::Present { text, .. } => text.as_deref(),
                    Body::Absent => None,
                };
                // Its own addresses, again after each rule: a rule that files it elsewhere takes
                // it out of `InFolder` for the rules after, as it takes it out of a folder view.
                let folders = self.placed(message);
                let sizes = [mail_domain::filter::message_size(message)];
                rule.fits(&MatchCtx {
                    summary: &summary,
                    corpus,
                    folders: &folders,
                    sizes: &sizes,
                    cc: &message.cc,
                    vips: self.snapshot.vips,
                    now: self.now,
                })
            };
            if !fits {
                continue;
            }
            fired.push(rule.name.clone());
            for action in &rule.actions {
                if action.speaks() {
                    if speaking == Speaking::Yes {
                        self.say(id, action, rule);
                    }
                    continue;
                }
                if let Some(op) = self.op_for(action) {
                    self.perform(op, id);
                }
            }
            if rule.after == AfterMatch::Stop {
                break;
            }
        }
        if !fired.is_empty() {
            self.acted.push((id, fired));
        }
    }

    fn say(&mut self, message: MessageId, action: &RuleAction, rule: &Rule) {
        match action {
            RuleAction::Notify => {
                let again = ArrivalEffect::Notice(message);
                if !self.effects.contains(&again) {
                    self.effects.push(again);
                }
            }
            RuleAction::Forward(_) | RuleAction::Reply(_) => {
                self.effects.push(ArrivalEffect::RuleSend(DraftRequest {
                    message,
                    action: action.clone(),
                    rule: rule.id,
                    made: rule.made,
                    reply_days: rule.reply_days,
                }));
            }
            _ => {}
        }
    }

    /// The operation an action means, with any label it names found or made; `None` for one that
    /// is not an operation on the message ([`RuleAction::speaks`]).
    fn op_for(&mut self, action: &RuleAction) -> Option<Op> {
        Some(match action {
            RuleAction::Label(name) => {
                Op::Label(self.label_named(name, LabelOrigin::User), Membership::In)
            }
            // The server's folder, so the server's origin: the user renames it there, not here.
            RuleAction::File(path) => Op::File(self.label_named(path, LabelOrigin::Provider)),
            RuleAction::Archive => Op::Archive,
            RuleAction::Trash => Op::Trash,
            RuleAction::Spam => Op::Spam,
            RuleAction::MarkRead => Op::SetRead(ReadState::Read),
            RuleAction::Star => Op::SetStar(Star::Starred),
            RuleAction::MarkUnread => Op::SetRead(ReadState::Unread),
            RuleAction::Notify | RuleAction::Forward(_) | RuleAction::Reply(_) => return None,
        })
    }

    /// Keep a message that arrived in a muted conversation out of the way: read, and archived if
    /// it is still in the inbox. Nothing for a conversation that is not muted.
    ///
    /// Both are the operations the user's own hand would apply, and not a local-only change: a
    /// message archived only here is still in the server's inbox and unread there, so the user's
    /// phone and webmail would show it as new, and once nothing is pending on it the next sync
    /// would bring the server's inbox back. Mute itself stays local (`Op::SetMute` has no remote
    /// intent), as snooze and pin do.
    fn muted(&mut self, id: MessageId) {
        let Some(view) = self.view_of(id) else {
            return;
        };
        if view.thread.summary.mute != Mute::Muted {
            return;
        }
        self.perform(Op::SetRead(ReadState::Read), id);
        // Still in the inbox: a rule before this may have filed it, trashed it or called it spam,
        // and that is where it stays.
        if self
            .message(id)
            .is_some_and(|m| m.mailbox == MailboxRole::Inbox)
        {
            self.perform(Op::Archive, id);
        }
        self.muted.push(id);
    }

    /// Judge what the pass stored for the first time, as the policy says.
    ///
    /// Mail the server put in its own Junk is learned as spam rather than judged: that is the
    /// server's filter speaking. Only mail in the inbox is judged, so a rule that filed it, or the
    /// user who moved it already, has the last word.
    fn sift(&mut self, arrived: &[MessageId]) {
        let policy = self.snapshot.policy;
        if policy.filtering == Filtering::Off || arrived.is_empty() {
            return;
        }
        for id in arrived {
            let Some(message) = self.message(*id) else {
                // Gone since the pass stored it: nothing left to judge.
                continue;
            };
            if message.account != *self.snapshot.account {
                continue;
            }
            match message.mailbox {
                MailboxRole::Spam => {
                    let words = junk::words(message);
                    self.rows.push(mail_domain::Row::JunkTaught {
                        account: message.account.clone(),
                        message: *id,
                        words: words.clone(),
                        judged: Judged::Spam,
                    });
                    self.taught.push(words);
                    continue;
                }
                MailboxRole::Inbox => {}
                _ => continue,
            }
            // Only its headers are here yet, and a subject alone is too little to call it junk.
            if matches!(message.body, Body::Absent) {
                continue;
            }
            let Some(standing) = self
                .snapshot
                .evidence
                .iter()
                .find(|e| e.message == *id)
                .map(|e| &e.standing)
            else {
                continue;
            };
            if junk::spared(standing.acquaintance, message, &policy, self.snapshot.names) {
                continue;
            }
            let judged = (policy.server_says == Heed::Yes && standing.server_spam)
                || junk::score(&self.counts_for(message, standing)).is_some_and(|p| p >= THRESHOLD);
            if !judged {
                continue;
            }
            let op = match policy.treat {
                Treat::Move => Op::Spam,
                Treat::Mark => Op::Label(
                    self.label_named(JUNK_LABEL, LabelOrigin::User),
                    Membership::In,
                ),
            };
            self.perform(op, *id);
            self.junk.push(*id);
        }
    }

    /// What the filter knows of `message`'s words: the counts the pass read, and what this fold
    /// has taught it since.
    fn counts_for(&self, message: &Message, standing: &Standing) -> JunkCounts {
        let mut counts = standing.counts.clone();
        counts.spam += self.taught.len() as u64;
        for word in junk::words(message) {
            let n = self
                .taught
                .iter()
                .filter(|taught| taught.contains(&word))
                .count() as u64;
            if n == 0 {
                continue;
            }
            match counts.words.iter_mut().find(|(w, _, _)| *w == word) {
                Some((_, spam, _)) => *spam += n,
                None => counts.words.push((word, n, 0)),
            }
        }
        counts
    }
}
