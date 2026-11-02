//! What happens to a conversation when it is archived, starred, labelled, snoozed or moved, and
//! how that is taken back: decisions over values, ending in a [`Commit`] for the store to write.
//!
//! [`act`] wraps `Op::apply` with what a commit needs besides the patch: whether the account has a
//! server to tell, what its server was last seen to support, and what the junk filter is to learn.
//! [`take_back`] reads the outbox as a list of what is waiting and decides what puts an act back,
//! and what leaves the outbox with it.

use crate::accounts::Accounts;
use crate::undo::{Undo, reverse_intent};
use chrono::{DateTime, Utc};
use mail_domain::{
    AccountCaps, ArchiveMeans, Change, Commit, Condstore, ExpungeMeans, FolderRoles, Judged,
    LabelId, MailboxRole, Membership, Message, MessageId, MoveExt, Op, OutboxId, Patch, ProtoOp,
    Queue, RemoteIntent, Resolve, Row, ServerLabels, ServerThreads, Supported, Target, Thread,
    ThreadId, Unplaced, WatchMode,
};
use porter_core::AccountId;

/// What the server was last seen to support, or, before the first sync, nothing: a change is then
/// made here alone and the server hears nothing, rather than hearing a guess.
pub fn local_caps(now: DateTime<Utc>) -> AccountCaps {
    AccountCaps {
        labels: ServerLabels::LocalOnly,
        threads: ServerThreads::Jwz,
        watch: WatchMode::Poll {
            every: std::time::Duration::from_secs(300),
        },
        archive: ArchiveMeans::LocalOnly,
        folders: FolderRoles::default(),
        condstore: Condstore::Absent,
        move_ext: MoveExt::Absent,
        expunge: ExpungeMeans::Forbidden,
        top: Supported::Absent,
        pipelining: Supported::Absent,
        connections: Default::default(),
        observed_at: now,
    }
}

/// `account`'s capabilities as an act sees them: what its server was last seen to support, and
/// [`local_caps`] where nothing has connected.
pub fn caps_here(accounts: &Accounts, account: &AccountId, now: DateTime<Utc>) -> AccountCaps {
    accounts
        .caps_of(account)
        .cloned()
        .unwrap_or_else(|| local_caps(now))
}

/// A conversation and every message in it.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadView {
    pub thread: Thread,
    pub messages: Vec<Message>,
}

/// What acts are decided over.
#[derive(Debug, Clone, Copy)]
pub struct ActSnapshot<'a> {
    pub threads: &'a [ThreadView],
    pub accounts: &'a Accounts,
    /// The label that marks junk on each account that has one, for taking it off to say Not Spam.
    pub junk_labels: &'a [(AccountId, LabelId)],
}

impl ActSnapshot<'_> {
    fn thread(&self, id: ThreadId) -> Option<&ThreadView> {
        self.threads
            .iter()
            .find(|view| view.thread.summary.id == id)
    }

    fn junk_label(&self, account: &AccountId) -> Option<LabelId> {
        self.junk_labels
            .iter()
            .find(|(of, _)| of == account)
            .map(|(_, label)| *label)
    }
}

/// One operation on a conversation, or some of its messages.
#[derive(Debug, Clone, PartialEq)]
pub struct Act {
    pub thread: ThreadId,
    pub target: Target,
    pub op: Op,
}

/// What an act came to: what to write, and what takes it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Acted {
    pub commit: Commit,
    pub undo: Undo,
}

/// Decide each act, in order. An act on a conversation the snapshot does not hold, or that holds
/// no message, is left out.
///
/// The commit is the operation's patch, its server half when the account has a server, and what
/// the junk filter learns: Spam and Not Spam teach it, as Mail's Junk and Not Junk do, from the
/// messages as they were, since what is learned is what they said and not where they went.
pub fn act(snapshot: &ActSnapshot<'_>, acts: &[Act], now: DateTime<Utc>) -> Vec<Acted> {
    acts.iter()
        .filter_map(|act| decide(snapshot, act, now))
        .collect()
}

fn decide(snapshot: &ActSnapshot<'_>, act: &Act, now: DateTime<Utc>) -> Option<Acted> {
    let view = snapshot.thread(act.thread)?;
    let account = view.messages.first().map(|m| m.account.clone())?;
    // What the server actually supports, which is the only thing that decides whether an
    // operation gets a `RemoteIntent` at all: `Op::remote_intent` is the sole consumer of `caps`,
    // and under `ArchiveMeans::LocalOnly` it returns `None` for Archive and Trash. Defaults in
    // place of the stored capabilities would change nothing on the server, and the conversation
    // would come back on the next full sync.
    let caps = caps_here(snapshot.accounts, &account, now);
    let applied = act
        .op
        .apply(&act.target, &view.thread, &view.messages, &caps, now);
    let taught = taught_by(&act.op, &view.messages, snapshot.junk_label(&account));
    let undo = Undo {
        thread: Some(act.thread),
        account: account.clone(),
        forward: applied.forward.clone(),
        inverse: applied.inverse.clone(),
        remote: applied.remote.clone(),
        resend: None,
    };
    // Local folders have no server half: their outbox is never drained, so nothing is put in it.
    let mut commit =
        applied.into_commit(account.clone(), now, snapshot.accounts.has_server(&account));
    // A deletion is addressed before it is written, the rest after (`into_commit`); a message
    // that names nothing the server holds is made here alone.
    debug_assert!(
        commit
            .queue
            .iter()
            .all(|queue| queue.unplaced == Unplaced::Skip)
    );
    commit
        .rows
        .extend(taught.into_iter().flat_map(|(judged, messages)| {
            messages.into_iter().map(move |message| Row::JunkTaught {
                account: message.account.clone(),
                message: message.id,
                words: crate::junk::words(message),
                judged,
            })
        }));
    Some(Acted { commit, undo })
}

/// The messages `op` teaches the junk filter about, and what it teaches.
fn taught_by<'a>(
    op: &Op,
    messages: &'a [Message],
    junk_label: Option<LabelId>,
) -> Option<(Judged, Vec<&'a Message>)> {
    let picked: Vec<&Message> = match op {
        Op::Spam => return Some((Judged::Spam, messages.iter().collect())),
        Op::Restore => messages
            .iter()
            .filter(|m| m.mailbox == MailboxRole::Spam)
            .collect(),
        // Taking the Junk label off is the way to say Not Spam for mail that was marked and not
        // moved ([`crate::junk::Treat::Mark`]).
        Op::Label(label, Membership::Out) if junk_label == Some(*label) => messages
            .iter()
            .filter(|m| m.labels.contains(label))
            .collect(),
        _ => return None,
    };
    (!picked.is_empty()).then_some((Judged::Ham, picked))
}

// ---------------------------------------------------------------------------------------
// Taking an act back
// ---------------------------------------------------------------------------------------

/// Work waiting in the outbox, as taking an act back sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Waiting {
    pub id: OutboxId,
    pub op: ProtoOp,
    /// What it puts back should the server refuse it.
    pub undo: Patch,
    /// The act that queued it, if one is named.
    pub forward: Option<mail_domain::UndoId>,
    pub attempts: u32,
}

/// What taking an act back is decided over.
#[derive(Debug, Clone, Copy)]
pub struct TakeBackSnapshot<'a> {
    pub accounts: &'a Accounts,
    /// Everything in the outbox, not only what is due: a later operation waiting out a retry or a
    /// credential is later all the same. In the order it was queued.
    pub waiting: &'a [Waiting],
    /// The label that marks junk on the act's account, if it has one.
    pub junk_label: Option<LabelId>,
}

/// What puts `entry` back, or `None` when it cannot be taken back and then nothing is to change:
/// a reverse the server cannot be told is not taken back here either, or the two would disagree
/// until the next sync put the operation back without a word.
///
/// The commit is the inverse written like any other patch, the reverse queued for the server when
/// the operation had told it anything, a move or copy not yet sent taken out of the outbox, and
/// what the act taught the junk filter taken back.
pub fn take_back(
    snapshot: &TakeBackSnapshot<'_>,
    entry: &Undo,
    now: DateTime<Utc>,
) -> Option<Commit> {
    // A send to queue again is the window's to undo (`Undo::resend`).
    if entry.resend.is_some() {
        return None;
    }
    let has_server = snapshot.accounts.has_server(&entry.account);
    // A copy the server has made stays made (`reverse_intent`): taking the copy out of its folder
    // here would put it back at the next sync, so once it has left the outbox there is nothing to
    // take back.
    if matches!(entry.remote, Some(RemoteIntent::Copy { .. }))
        && has_server
        && queued_filing(snapshot.waiting, entry).is_none()
    {
        return None;
    }
    let reverse = entry
        .remote
        .as_ref()
        .filter(|_| has_server)
        .and_then(|remote| reverse_intent(remote, &entry.inverse));
    let mut rows = Vec::new();
    if withdrawable(snapshot.waiting, entry) {
        rows.push(Row::Withdrawn {
            forward: entry.forward.id,
            inverse: entry.inverse.clone(),
        });
    }
    rows.extend(
        forgotten_by(entry, snapshot.junk_label)
            .into_iter()
            .map(|(judged, message)| Row::JunkForgotten {
                account: entry.account.clone(),
                message,
                judged,
            }),
    );
    Some(Commit {
        account: entry.account.clone(),
        at: now,
        forward: entry.inverse.clone(),
        queue: reverse
            .into_iter()
            .map(|intent| Queue {
                intent,
                undo: entry.forward.clone(),
                id: None,
                resolve: Resolve::AfterForward,
                unplaced: Unplaced::Skip,
            })
            .collect(),
        rows,
    })
}

/// What a gesture taught the junk filter, from the patch it applied: Spam taught that the messages
/// it moved into Spam are junk, and Not Spam, or taking the Junk label off, that the messages it
/// moved out of Spam or off that label are not.
fn forgotten_by(entry: &Undo, junk_label: Option<LabelId>) -> Vec<(Judged, MessageId)> {
    let was_spam: Vec<MessageId> = entry
        .inverse
        .changes
        .iter()
        .filter_map(|change| match change {
            Change::MessageMailbox(m, MailboxRole::Spam) => Some(*m),
            _ => None,
        })
        .collect();
    entry
        .forward
        .changes
        .iter()
        .filter_map(|change| match change {
            Change::MessageMailbox(m, MailboxRole::Spam) => Some((Judged::Spam, *m)),
            Change::MessageMailbox(m, _) if was_spam.contains(m) => Some((Judged::Ham, *m)),
            Change::MessageLabel(m, label, Membership::Out) if Some(*label) == junk_label => {
                Some((Judged::Ham, *m))
            }
            _ => None,
        })
        .collect()
}

/// Whether `waiting` is the work `entry`'s act queued: named by the act's patch, or, for work
/// queued before acts were named, by its undo being the act's inverse.
fn queued_by(waiting: &Waiting, entry: &Undo) -> bool {
    match waiting.forward {
        Some(named) => named == entry.forward.id,
        None => waiting.undo == entry.inverse,
    }
}

/// The queued move or copy `entry` made, while the server has not been tried with it. `None` once
/// it has been sent, has been tried and is waiting to retry, or was settled.
fn queued_filing<'a>(waiting: &'a [Waiting], entry: &Undo) -> Option<&'a Waiting> {
    let queued_as: fn(&ProtoOp) -> bool = match &entry.remote {
        Some(RemoteIntent::File { .. }) => |op| matches!(op, ProtoOp::File { .. }),
        Some(RemoteIntent::Copy { .. }) => |op| matches!(op, ProtoOp::Copy { .. }),
        _ => return None,
    };
    waiting.iter().find(|waiting| {
        queued_as(&waiting.op) && waiting.attempts == 0 && queued_by(waiting, entry)
    })
}

/// Whether a move or a copy to a folder, taken back before the server heard of it, is taken out of
/// the outbox too.
///
/// A filing has no reverse the server can be sent (`reverse_intent`), so once it has gone the
/// server keeps it filed. Until it has been tried, though, it is only a queued intention, and
/// leaving it queued would move the message on the next sync after the user said not to, and hold
/// its row out of the folder it is back in meanwhile, because a queued move is a message leaving.
/// Settled as refused, which drops the entry and puts back the same values the undo just wrote.
///
/// Not when anything queued after it acts on the same messages. Settling it refused writes its
/// undo, the state from before it, and drops its pending changes, and a later operation on those
/// messages was made on top of it: that operation's own undo and pending changes assume the move
/// is there. Rather than work out which of them survive it, the move is left queued and the server
/// keeps it filed.
fn withdrawable(waiting: &[Waiting], entry: &Undo) -> bool {
    let messages = match &entry.remote {
        Some(RemoteIntent::File { messages, .. }) | Some(RemoteIntent::Copy { messages, .. }) => {
            messages
        }
        _ => return false,
    };
    let Some(filing) = queued_filing(waiting, entry) else {
        return false;
    };
    let mut ours = messages.clone();
    ours.extend(messages_in(&filing.undo));
    !waiting
        .iter()
        .filter(|later| later.id > filing.id)
        .any(|later| touches(later, &ours, &filing.op))
}

/// The messages a patch writes to.
fn messages_in(patch: &Patch) -> Vec<MessageId> {
    patch
        .changes
        .iter()
        .filter_map(|change| match change {
            Change::MessageRead(m, _)
            | Change::MessageStar(m, _)
            | Change::MessageMailbox(m, _)
            | Change::MessageLabel(m, _, _)
            | Change::MessageDelete(m) => Some(*m),
            Change::MessageUpsert(message) => Some(message.id),
            _ => None,
        })
        .collect()
}

/// Whether a queued entry acts on any of `messages`, or on any server message `filing` names.
///
/// Its undo names the messages it changed here, and its operation the ones it will change there;
/// either is enough. An operation this cannot read counts as touching them, because guessing wrong
/// that way costs only a move the server is still sent.
fn touches(later: &Waiting, messages: &[MessageId], filing: &ProtoOp) -> bool {
    if messages_in(&later.undo)
        .iter()
        .any(|m| messages.contains(m))
    {
        return true;
    }
    let (ProtoOp::File { remotes: ours, .. } | ProtoOp::Copy { remotes: ours, .. }) = filing else {
        return true;
    };
    match &later.op {
        ProtoOp::SetFlags { remotes, .. }
        | ProtoOp::SetMailbox { remotes, .. }
        | ProtoOp::SetLabels { remotes, .. }
        | ProtoOp::File { remotes, .. }
        | ProtoOp::Copy { remotes, .. }
        | ProtoOp::AddKeyword { remotes, .. }
        | ProtoOp::Expunge { remotes }
        | ProtoOp::Destroy { remotes } => remotes.iter().any(|r| ours.contains(r)),
        // A new message, a sent one, a mailbox or a draft's copy on the server: none of them is a
        // message already held.
        ProtoOp::Append { .. }
        | ProtoOp::Submit { .. }
        | ProtoOp::Folder(_)
        | ProtoOp::KeepDraft { .. }
        | ProtoOp::DropDraft { .. } => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::Account;
    use mail_domain::testing::{account, at, message, now};
    use mail_domain::{
        AccountPlan, AuthPlan, Incoming, Outgoing, ReadState, SaslMech, Star, ThreadSummary, Tls,
        Username,
    };

    fn plan(incoming: Incoming) -> AccountPlan {
        AccountPlan {
            address: "me@example.test".to_owned(),
            incoming,
            outgoing: Outgoing::Nowhere,
            auth: AuthPlan::Password {
                username: Username::SameAsAddress,
                sasl: vec![SaslMech::Plain],
            },
            identities: Vec::new(),
        }
    }

    fn imap() -> Incoming {
        Incoming::Imap {
            host: "imap.example.test".to_owned(),
            port: 993,
            tls: Tls::Implicit,
        }
    }

    fn folders() -> AccountCaps {
        AccountCaps {
            archive: ArchiveMeans::MoveToFolder("Archive".to_owned()),
            ..local_caps(at(0))
        }
    }

    fn view(n: u128) -> ThreadView {
        let m = message(n, "a@example.test", "hello", "cheap pills");
        let messages = vec![m];
        ThreadView {
            thread: Thread {
                summary: ThreadSummary::derive(
                    messages[0].thread,
                    &messages,
                    mail_domain::Snooze::Inactive,
                    mail_domain::Pin::Unpinned,
                    mail_domain::Mute::Unmuted,
                    mail_domain::FollowUp::Inactive,
                ),
                messages: vec![messages[0].id],
            },
            messages,
        }
    }

    fn acts(views: &[ThreadView], op: Op) -> Vec<Act> {
        views
            .iter()
            .map(|v| Act {
                thread: v.thread.summary.id,
                target: Target::Threads(vec![v.thread.summary.id]),
                op: op.clone(),
            })
            .collect()
    }

    fn decide_all(accounts: &Accounts, views: &[ThreadView], op: Op) -> Vec<Acted> {
        let snapshot = ActSnapshot {
            threads: views,
            accounts,
            junk_labels: &[],
        };
        act(&snapshot, &acts(views, op), now())
    }

    /// An account that has a server and has been seen to file into folders queues the move; one
    /// that keeps its mail here, or has not connected yet, makes the change here alone.
    #[test]
    fn the_server_is_told_only_by_an_account_that_has_one_and_has_been_seen() {
        let views = [view(1)];
        let table = [
            ("a server seen filing", Some(imap()), Some(folders()), true),
            ("a server not yet seen", Some(imap()), None, false),
            ("no server", Some(Incoming::Local), Some(folders()), false),
        ];
        for (what, incoming, caps, queued) in table {
            let accounts = Accounts::fixture([Account::new(
                account(),
                "me@example.test",
                plan(incoming.unwrap()),
                caps,
            )]);
            let done = decide_all(&accounts, &views, Op::Archive);
            assert_eq!(done.len(), 1, "{what}");
            assert_eq!(done[0].commit.queue.len(), usize::from(queued), "{what}");
            assert!(!done[0].commit.forward.changes.is_empty(), "{what}");
            assert_eq!(done[0].undo.forward, done[0].commit.forward, "{what}");
        }
    }

    #[test]
    fn a_deletion_is_addressed_before_it_is_written_and_everything_else_after() {
        let mut view = view(1);
        view.messages[0].mailbox = MailboxRole::Trash;
        let accounts = Accounts::fixture([Account::new(
            account(),
            "me@example.test",
            plan(imap()),
            Some(folders()),
        )]);
        let views = [view];
        let destroyed = decide_all(&accounts, &views, Op::Destroy);
        assert_eq!(destroyed[0].commit.queue[0].resolve, Resolve::BeforeForward);
        let read = decide_all(&accounts, &views, Op::SetRead(ReadState::Read));
        assert_eq!(read[0].commit.queue[0].resolve, Resolve::AfterForward);
        // Work that names nothing the server holds is made here alone, for a person's act and a
        // rule's alike.
        assert!(
            read[0]
                .commit
                .queue
                .iter()
                .all(|queue| queue.unplaced == Unplaced::Skip)
        );
    }

    #[test]
    fn a_conversation_the_snapshot_lacks_is_left_out() {
        let views = [view(1)];
        let missing = [view(2)];
        let accounts = Accounts::default();
        let snapshot = ActSnapshot {
            threads: &views,
            accounts: &accounts,
            junk_labels: &[],
        };
        assert!(act(&snapshot, &acts(&missing, Op::Archive), now()).is_empty());
    }

    #[test]
    fn spam_teaches_the_filter_from_the_messages_as_they_were() {
        let mut spammy = view(1);
        spammy.messages[0].mailbox = MailboxRole::Spam;
        let accounts = Accounts::default();
        let junk = LabelId::generate();
        let mut marked = view(2);
        marked.messages[0].labels = vec![junk];
        let views = [view(3), spammy, marked];
        let snapshot = ActSnapshot {
            threads: &views,
            accounts: &accounts,
            junk_labels: &[(account(), junk)],
        };
        let taught = |op: Op, of: usize| {
            act(&snapshot, &acts(&views[of..=of], op), now())[0]
                .commit
                .rows
                .iter()
                .map(|row| match row {
                    Row::JunkTaught { judged, words, .. } => (*judged, words.clone()),
                    other => panic!("{other:?}"),
                })
                .collect::<Vec<_>>()
        };
        let words = crate::junk::words(&views[0].messages[0]);
        assert_eq!(taught(Op::Spam, 0), [(Judged::Spam, words)]);
        // Not Spam teaches only about what was in Spam.
        assert_eq!(taught(Op::Restore, 0), []);
        assert_eq!(taught(Op::Restore, 1).len(), 1);
        assert_eq!(taught(Op::Restore, 1)[0].0, Judged::Ham);
        // Taking the junk label off teaches Not Spam about what carried it; the label on
        // nothing, or another label, teaches nothing.
        assert_eq!(
            taught(Op::Label(junk, Membership::Out), 2)[0].0,
            Judged::Ham
        );
        assert_eq!(taught(Op::Label(junk, Membership::Out), 0), []);
        assert_eq!(taught(Op::Label(junk, Membership::In), 2), []);
        assert_eq!(taught(Op::Archive, 0), []);
    }

    // -- taking back --------------------------------------------------------------------

    fn waiting_for(acted: &Acted, id: i64, tried: u32, named: bool) -> Waiting {
        Waiting {
            id: OutboxId::from_i64(id),
            op: match acted.undo.remote.as_ref().unwrap() {
                RemoteIntent::File { .. } => ProtoOp::File {
                    remotes: Vec::new(),
                    folder: "Projects".to_owned(),
                },
                RemoteIntent::Copy { .. } => ProtoOp::Copy {
                    remotes: Vec::new(),
                    folder: "Projects".to_owned(),
                },
                _ => ProtoOp::Destroy {
                    remotes: Vec::new(),
                },
            },
            undo: acted.undo.inverse.clone(),
            forward: named.then_some(acted.undo.forward.id),
            attempts: tried,
        }
    }

    fn accounts_with_server() -> Accounts {
        Accounts::fixture([Account::new(
            account(),
            "me@example.test",
            plan(imap()),
            Some(folders()),
        )])
    }

    fn taken(accounts: &Accounts, waiting: &[Waiting], entry: &Undo) -> Option<Commit> {
        take_back(
            &TakeBackSnapshot {
                accounts,
                waiting,
                junk_label: None,
            },
            entry,
            now(),
        )
    }

    #[test]
    fn a_move_not_yet_sent_is_withdrawn_with_its_undo_and_a_sent_one_is_not() {
        let accounts = accounts_with_server();
        let label = LabelId::generate();
        let views = [view(1)];
        let moved = decide_all(&accounts, &views, Op::File(label)).remove(0);
        let entry = &moved.undo;
        // Waiting, named or queued before acts were: withdrawn.
        for named in [true, false] {
            let waiting = [waiting_for(&moved, 1, 0, named)];
            let commit = taken(&accounts, &waiting, entry).unwrap();
            assert_eq!(commit.forward, entry.inverse);
            assert_eq!(
                commit.rows,
                [Row::Withdrawn {
                    forward: entry.forward.id,
                    inverse: entry.inverse.clone()
                }],
                "named: {named}"
            );
        }
        // Tried, or gone: taken back here, and the server keeps it filed.
        for waiting in [vec![waiting_for(&moved, 1, 1, true)], vec![]] {
            let commit = taken(&accounts, &waiting, entry).unwrap();
            assert!(commit.rows.is_empty());
            // A filing has no reverse to tell the server.
            assert!(commit.queue.is_empty());
        }
        // Another act's entry is not this one's.
        let other = decide_all(&accounts, &views, Op::File(label)).remove(0);
        let waiting = [waiting_for(&other, 1, 0, true)];
        assert!(taken(&accounts, &waiting, entry).unwrap().rows.is_empty());
    }

    #[test]
    fn a_move_stays_queued_under_a_later_entry_that_acts_on_the_same_message() {
        let accounts = accounts_with_server();
        let label = LabelId::generate();
        let views = [view(1)];
        let moved = decide_all(&accounts, &views, Op::File(label)).remove(0);
        let read = decide_all(&accounts, &views, Op::SetRead(ReadState::Read)).remove(0);
        let mut later = waiting_for(&read, 2, 0, true);
        later.op = ProtoOp::SetFlags {
            remotes: Vec::new(),
            read: Some(ReadState::Read),
            star: None,
        };
        let waiting = [waiting_for(&moved, 1, 0, true), later.clone()];
        // The later entry's undo names the same message.
        assert!(
            taken(&accounts, &waiting, &moved.undo)
                .unwrap()
                .rows
                .is_empty()
        );
        // One that touches nothing of it does not hold it.
        later.undo = Patch {
            id: later.undo.id,
            changes: vec![Change::MessageStar(MessageId::generate(), Star::Starred)],
        };
        let waiting = [waiting_for(&moved, 1, 0, true), later];
        assert_eq!(
            taken(&accounts, &waiting, &moved.undo).unwrap().rows.len(),
            1
        );
    }

    #[test]
    fn a_copy_the_server_has_made_cannot_be_taken_back() {
        let accounts = accounts_with_server();
        let views = [view(1)];
        let copied = decide_all(&accounts, &views, Op::Copy(LabelId::generate())).remove(0);
        assert!(taken(&accounts, &[], &copied.undo).is_none());
        let waiting = [waiting_for(&copied, 1, 0, true)];
        assert_eq!(
            taken(&accounts, &waiting, &copied.undo).unwrap().rows.len(),
            1
        );
    }

    #[test]
    fn what_the_server_was_told_is_told_back_and_a_send_is_not_ours_to_take_back() {
        let accounts = accounts_with_server();
        let views = [view(1)];
        let read = decide_all(&accounts, &views, Op::SetRead(ReadState::Read)).remove(0);
        let commit = taken(&accounts, &[], &read.undo).unwrap();
        assert_eq!(commit.queue.len(), 1);
        assert_eq!(commit.queue[0].undo, read.undo.forward);
        assert_eq!(commit.queue[0].resolve, Resolve::AfterForward);
        let local = Accounts::fixture([Account::new(
            account(),
            "me@example.test",
            plan(Incoming::Local),
            None,
        )]);
        assert!(taken(&local, &[], &read.undo).unwrap().queue.is_empty());
        let mut resent = read.undo.clone();
        resent.resend = Some(crate::undo::Resend {
            draft: mail_domain::DraftId::generate(),
            at: now(),
            pictures: crate::picture::PictureSize::ALL[0],
        });
        assert!(taken(&accounts, &[], &resent).is_none());
    }

    #[test]
    fn taking_back_spam_or_not_spam_takes_back_what_it_taught() {
        let accounts = Accounts::default();
        let views = [view(1)];
        let spam = decide_all(&accounts, &views, Op::Spam).remove(0);
        let commit = taken(&accounts, &[], &spam.undo).unwrap();
        assert!(matches!(
            &commit.rows[..],
            [Row::JunkForgotten {
                judged: Judged::Spam,
                ..
            }]
        ));
    }
}
