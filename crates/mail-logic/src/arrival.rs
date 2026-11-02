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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::local_caps;
    use mail_domain::testing::{account, at, message};
    use mail_domain::{Address, FollowUp, Pin, Snooze, ThreadSummary};

    /// Ids counted up from one, so a test can say what was made.
    #[derive(Default)]
    struct Counted(u128);

    impl Fresh for Counted {
        fn label(&mut self) -> LabelId {
            self.0 += 1;
            LabelId::from_uuid(uuid::Uuid::from_u128(0xA000 + self.0))
        }
        fn change(&mut self) -> ChangeId {
            self.0 += 1;
            ChangeId::from_uuid(uuid::Uuid::from_u128(0xC000 + self.0))
        }
    }

    fn view(messages: Vec<Message>, mute: Mute) -> ThreadView {
        let id = messages[0].thread;
        ThreadView {
            thread: mail_domain::Thread {
                summary: ThreadSummary::derive(
                    id,
                    &messages,
                    Snooze::Inactive,
                    Pin::Unpinned,
                    mute,
                    FollowUp::Inactive,
                ),
                messages: messages.iter().map(|m| m.id).collect(),
            },
            messages,
        }
    }

    fn rule(
        name: &str,
        position: u32,
        filter: mail_domain::Filter,
        actions: Vec<RuleAction>,
    ) -> Rule {
        Rule {
            id: RuleId::from_uuid(uuid::Uuid::from_u128(0x5000 + u128::from(position))),
            account: account(),
            name: name.to_owned(),
            position,
            state: mail_domain::RuleState::Enabled,
            filter,
            actions,
            after: AfterMatch::Continue,
            made: at(0),
            reply_days: 4,
        }
    }

    fn from(who: &str) -> mail_domain::Filter {
        mail_domain::Filter::From(mail_domain::TextMatch::Contains(who.to_owned()))
    }

    struct World {
        account: AccountId,
        caps: AccountCaps,
        rules: Vec<Rule>,
        labels: Vec<Label>,
        threads: Vec<ThreadView>,
        policy: Policy,
    }

    impl World {
        fn new(threads: Vec<ThreadView>, rules: Vec<Rule>) -> World {
            World {
                account: account(),
                caps: AccountCaps {
                    archive: mail_domain::ArchiveMeans::DropInbox,
                    labels: mail_domain::ServerLabels::Supported,
                    ..local_caps(at(0))
                },
                rules,
                labels: Vec::new(),
                threads,
                policy: Policy::default(),
            }
        }

        fn arrive(&self, arrived: &[MessageId]) -> Arrival {
            let snapshot = ArrivalSnapshot {
                account: &self.account,
                has_server: true,
                caps: &self.caps,
                rules: &self.rules,
                labels: &self.labels,
                vips: &[],
                names: &[],
                policy: self.policy,
                threads: &self.threads,
                evidence: &[],
            };
            on_arrival(&snapshot, arrived, at(10), &mut Counted::default())
        }
    }

    fn arriving(n: u128, sender: &str) -> Message {
        let mut m = message(n, sender, "Hello", "body");
        m.from = Address::new(sender);
        m
    }

    #[test]
    fn a_rule_that_names_a_label_that_is_not_there_makes_it_first_and_once() {
        let m = arriving(1, "ada@example.test");
        let id = m.id;
        let world = World::new(
            vec![view(vec![m], Mute::Unmuted)],
            vec![rule(
                "Bills",
                1,
                from("ada"),
                vec![
                    RuleAction::Label("Bills".to_owned()),
                    RuleAction::Label("bills".to_owned()),
                    RuleAction::Archive,
                ],
            )],
        );
        let arrival = world.arrive(&[id]);
        let changes = &arrival.commit.forward.changes;
        assert!(matches!(&changes[0], Change::LabelUpsert(l) if l.name == "Bills"));
        assert_eq!(
            changes
                .iter()
                .filter(|c| matches!(c, Change::LabelUpsert(_)))
                .count(),
            1,
            "{changes:?}"
        );
        assert_eq!(arrival.acted, vec![(id, vec!["Bills".to_owned()])]);
        // The label, then the archive: two pieces of work for the server.
        assert_eq!(arrival.commit.queue.len(), 2);
    }

    #[test]
    fn a_later_rule_sees_what_an_earlier_one_did() {
        let m = arriving(1, "ada@example.test");
        let id = m.id;
        let world = World::new(
            vec![view(vec![m], Mute::Unmuted)],
            vec![
                rule("read", 1, from("ada"), vec![RuleAction::MarkRead]),
                rule(
                    "star the read",
                    2,
                    mail_domain::Filter::Read(ReadState::Read),
                    vec![RuleAction::Star],
                ),
            ],
        );
        let arrival = world.arrive(&[id]);
        assert_eq!(
            arrival.acted,
            vec![(id, vec!["read".to_owned(), "star the read".to_owned()])]
        );
    }

    #[test]
    fn a_reply_in_a_muted_conversation_is_read_and_archived_unless_a_rule_filed_it() {
        let first = arriving(1, "ada@example.test");
        let reply = {
            let mut m = arriving(2, "ada@example.test");
            m.thread = first.thread;
            m
        };
        let junked = {
            let mut m = arriving(3, "news@junk.example");
            m.thread = first.thread;
            m
        };
        let (reply_id, junked_id) = (reply.id, junked.id);
        let world = World::new(
            vec![view(vec![first, reply, junked], Mute::Muted)],
            vec![rule(
                "junk",
                1,
                from("junk.example"),
                vec![RuleAction::Spam],
            )],
        );
        let arrival = world.arrive(&[reply_id, junked_id]);
        assert_eq!(arrival.muted, vec![reply_id, junked_id]);
        let mailboxes: Vec<(MessageId, MailboxRole)> = arrival
            .commit
            .forward
            .changes
            .iter()
            .filter_map(|c| match c {
                Change::MessageMailbox(m, role) => Some((*m, *role)),
                _ => None,
            })
            .collect();
        assert_eq!(
            mailboxes,
            vec![
                (reply_id, MailboxRole::Archive),
                (junked_id, MailboxRole::Spam)
            ],
            "the filed one is not archived over"
        );
    }

    #[test]
    fn what_a_rule_says_or_sends_comes_back_in_order_and_a_notice_once() {
        let m = arriving(1, "boss@example.test");
        let id = m.id;
        let world = World::new(
            vec![view(vec![m], Mute::Unmuted)],
            vec![
                rule(
                    "one",
                    1,
                    from("boss"),
                    vec![
                        RuleAction::Notify,
                        RuleAction::Forward("me@elsewhere.example".to_owned()),
                    ],
                ),
                rule("two", 2, from("boss"), vec![RuleAction::Notify]),
            ],
        );
        let arrival = world.arrive(&[id]);
        let effects: Vec<String> = arrival
            .effects
            .iter()
            .map(|e| match e {
                ArrivalEffect::Notice(m) => format!("notice {}", m == &id),
                ArrivalEffect::RuleSend(r) => format!("send {:?}", r.action),
            })
            .collect();
        assert_eq!(
            effects,
            ["notice true", "send Forward(\"me@elsewhere.example\")"]
        );
    }

    #[test]
    fn rules_asked_for_over_old_mail_say_and_send_nothing() {
        let m = arriving(1, "boss@example.test");
        let id = m.id;
        let world = World::new(
            vec![view(vec![m], Mute::Unmuted)],
            vec![rule(
                "one",
                1,
                from("boss"),
                vec![RuleAction::Notify, RuleAction::Star],
            )],
        );
        let snapshot = ArrivalSnapshot {
            account: &world.account,
            has_server: true,
            caps: &world.caps,
            rules: &world.rules,
            labels: &[],
            vips: &[],
            names: &[],
            policy: Policy::default(),
            threads: &world.threads,
            evidence: &[],
        };
        let quiet = apply_rules(
            &snapshot,
            &[id],
            Speaking::No,
            at(10),
            &mut Counted::default(),
        );
        assert!(quiet.effects.is_empty());
        assert_eq!(quiet.acted.len(), 1, "but it still acts");
        let loud = apply_rules(
            &snapshot,
            &[id],
            Speaking::Yes,
            at(10),
            &mut Counted::default(),
        );
        assert_eq!(loud.effects, vec![ArrivalEffect::Notice(id)]);
    }

    #[test]
    fn an_account_without_a_server_tells_no_one() {
        let m = arriving(1, "ada@example.test");
        let id = m.id;
        let mut world = World::new(
            vec![view(vec![m], Mute::Unmuted)],
            vec![rule("a", 1, from("ada"), vec![RuleAction::Archive])],
        );
        let snapshot = |world: &World, has_server| {
            let snapshot = ArrivalSnapshot {
                account: &world.account,
                has_server,
                caps: &world.caps,
                rules: &world.rules,
                labels: &[],
                vips: &[],
                names: &[],
                policy: Policy::default(),
                threads: &world.threads,
                evidence: &[],
            };
            on_arrival(&snapshot, &[id], at(10), &mut Counted::default())
        };
        assert_eq!(snapshot(&world, true).commit.queue.len(), 1);
        let local = snapshot(&world, false);
        assert!(local.commit.queue.is_empty());
        assert!(
            !local.commit.forward.changes.is_empty(),
            "but it is done here"
        );
        world.rules.clear();
        assert!(
            !snapshot(&world, true).writes(),
            "no rules, nothing to write"
        );
    }

    #[test]
    fn junk_is_judged_before_or_after_the_rules_as_the_policy_says() {
        use crate::junk::{ENOUGH, Filtering, Order};
        let m = {
            let mut m = arriving(1, "promo@prizes.example");
            m.subject = "You won".to_owned();
            m
        };
        let id = m.id;
        let mut world = World::new(
            vec![view(vec![m.clone()], Mute::Unmuted)],
            vec![rule(
                "star",
                1,
                mail_domain::Filter::All,
                vec![RuleAction::Star],
            )],
        );
        world.policy = Policy {
            filtering: Filtering::On,
            ..Policy::default()
        };
        // A filter that is sure: every word of it is spam's.
        let words = junk::words(&m);
        let counts = JunkCounts {
            spam: ENOUGH,
            ham: ENOUGH,
            words: words.iter().map(|w| (w.clone(), ENOUGH - 1, 0)).collect(),
        };
        let evidence = [Evidence {
            message: id,
            standing: Standing {
                counts,
                ..Standing::default()
            },
            places: Placing::default(),
        }];
        let run = |order| {
            let snapshot = ArrivalSnapshot {
                account: &world.account,
                has_server: true,
                caps: &world.caps,
                rules: &world.rules,
                labels: &[],
                vips: &[],
                names: &[],
                policy: Policy {
                    order,
                    ..world.policy
                },
                threads: &world.threads,
                evidence: &evidence,
            };
            on_arrival(&snapshot, &[id], at(10), &mut Counted::default())
        };
        let before = run(Order::BeforeRules);
        assert_eq!(before.junk, vec![id]);
        assert!(before.acted.is_empty(), "the rule never saw it");
        let after = run(Order::AfterRules);
        assert_eq!(after.junk, vec![id]);
        assert_eq!(after.acted.len(), 1, "the rule saw it first");
    }
}
