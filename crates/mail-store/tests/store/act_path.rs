//! What an operation does to a store, written two ways and held to the same answer.
//!
//! A person's act is `Op::apply`, then its patch written, then the server's half queued, and the
//! patch taken back if the queue refuses. The SQLite store and the memory store must come out of
//! any run of those alike, and an operation followed by its inverse must leave the conversation as
//! it was. These tests describe the store as it is, before one `commit` does the whole of it.

use crate::support::*;
use mail_domain::*;
use mail_store::{OutboxEntry, Store};

use proptest::prelude::*;

const LABEL_A: LabelId = LabelId::from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000d1"));
const LABEL_B: LabelId = LabelId::from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000d2"));
const LABEL_FOLDER: LabelId =
    LabelId::from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000d3"));

/// A server that files into folders and takes labels: every operation has a server half.
pub fn server_caps() -> AccountCaps {
    AccountCaps {
        labels: ServerLabels::Supported,
        threads: ServerThreads::Jwz,
        watch: WatchMode::Idle,
        archive: ArchiveMeans::MoveToFolder("Archive".to_owned()),
        folders: FolderRoles::default(),
        condstore: Condstore::Absent,
        move_ext: MoveExt::Supported,
        expunge: ExpungeMeans::Forbidden,
        top: Supported::Absent,
        pipelining: Supported::Absent,
        connections: ConnectionBudget::default(),
        observed_at: at(0),
    }
}

/// Year 9999, which lists the whole queue.
pub fn horizon() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(253_402_300_799, 0).unwrap()
}

#[derive(Debug, Clone)]
pub struct Spec {
    /// Which of the two conversations it is in.
    pub thread: bool,
    pub read: bool,
    pub star: bool,
    pub mailbox: u8,
    pub labels: (bool, bool),
    /// Whether the server holds it: a message with no address has nothing to tell the server.
    pub on_server: bool,
}

pub fn specs() -> impl Strategy<Value = Vec<Spec>> {
    prop::collection::vec(
        (
            any::<bool>(),
            any::<bool>(),
            any::<bool>(),
            0u8..6,
            any::<(bool, bool)>(),
            prop::bool::weighted(0.8),
        )
            .prop_map(|(thread, read, star, mailbox, labels, on_server)| Spec {
                thread,
                read,
                star,
                mailbox,
                labels,
                on_server,
            }),
        1..5,
    )
}

#[derive(Debug, Clone)]
pub struct Step {
    pub thread: bool,
    /// Which of the conversation's messages the target names, if it names messages.
    pub only: Option<Vec<bool>>,
    pub op: Op,
    /// Whether the account has a server to tell.
    pub server: bool,
}

pub fn steps() -> impl Strategy<Value = Vec<Step>> {
    let label = prop_oneof![Just(LABEL_A), Just(LABEL_B)];
    let op = prop_oneof![
        Just(Op::Archive),
        Just(Op::Trash),
        Just(Op::Restore),
        Just(Op::Spam),
        any::<bool>().prop_map(|b| Op::SetRead(if b {
            ReadState::Read
        } else {
            ReadState::Unread
        })),
        any::<bool>().prop_map(|b| Op::SetStar(if b { Star::Starred } else { Star::Unstarred })),
        (label.clone(), any::<bool>())
            .prop_map(|(l, i)| Op::Label(l, if i { Membership::In } else { Membership::Out })),
        Just(Op::File(LABEL_FOLDER)),
        Just(Op::Copy(LABEL_FOLDER)),
        Just(Op::Flag(Hue::Orange)),
        Just(Op::SetPin(Pin::Rank(1))),
        Just(Op::SetMute(Mute::Muted)),
        Just(Op::SetSnooze(Snooze::Until(at(900)))),
        Just(Op::Destroy),
    ];
    prop::collection::vec(
        (
            any::<bool>(),
            prop::option::of(prop::collection::vec(any::<bool>(), 4)),
            op,
            prop::bool::weighted(0.8),
        )
            .prop_map(|(thread, only, op, server)| Step {
                thread,
                only,
                op,
                server,
            }),
        1..5,
    )
}

fn role(n: u8) -> MailboxRole {
    MailboxRole::ALL[usize::from(n) % MailboxRole::ALL.len()]
}

/// Hold `specs` in both stores: two conversations, the messages with an address on the server
/// where they are `on_server`, and the three labels the operations name.
pub fn seed(stores: &[&dyn Store], specs: &[Spec]) -> [Vec<MessageId>; 2] {
    let mut threads = [Vec::new(), Vec::new()];
    for store in stores {
        let labels = [LABEL_A, LABEL_B, LABEL_FOLDER]
            .iter()
            .enumerate()
            .map(|(i, id)| {
                Change::LabelUpsert(Label {
                    id: *id,
                    account: acct_account(),
                    name: format!("label {i}"),
                    color: None,
                    origin: LabelOrigin::User,
                })
            })
            .collect();
        store.apply(acct_account(), &patch(labels)).unwrap();
    }
    for (n, spec) in specs.iter().enumerate() {
        let n = n as u128 + 1;
        let mut m = message(n);
        m.thread =
            ThreadId::from_uuid(uuid::Uuid::from_u128(if spec.thread { 2001 } else { 2000 }));
        m.read = if spec.read {
            ReadState::Read
        } else {
            ReadState::Unread
        };
        m.star = if spec.star {
            Star::Starred
        } else {
            Star::Unstarred
        };
        m.mailbox = role(spec.mailbox);
        m.labels = [(spec.labels.0, LABEL_A), (spec.labels.1, LABEL_B)]
            .into_iter()
            .filter_map(|(on, l)| on.then_some(l))
            .collect();
        for store in stores {
            if spec.on_server {
                deliver(*store, &m, imap("INBOX", n as u32));
                // `deliver` files it as the inbox does; the rest is a local change.
                store
                    .apply(
                        acct_account(),
                        &patch(vec![
                            Change::MessageMailbox(m.id, m.mailbox),
                            Change::MessageRead(m.id, m.read),
                            Change::MessageStar(m.id, m.star),
                        ]),
                    )
                    .unwrap();
                for l in &m.labels {
                    store
                        .apply(
                            acct_account(),
                            &patch(vec![Change::MessageLabel(m.id, *l, Membership::In)]),
                        )
                        .unwrap();
                }
            } else {
                store
                    .apply(
                        acct_account(),
                        &patch(vec![Change::MessageUpsert(Box::new(m.clone()))]),
                    )
                    .unwrap();
            }
        }
        threads[usize::from(spec.thread)].push(m.id);
    }
    threads
}

/// What a store holds of the two conversations and its queue: all that an operation can change.
#[derive(Debug, PartialEq)]
pub struct Held {
    pub threads: Vec<(Thread, Vec<Message>)>,
    pub queue: Vec<OutboxEntry>,
}

pub fn held(store: &dyn Store, threads: &[Vec<MessageId>; 2]) -> Held {
    let mut out = Vec::new();
    for ids in threads.iter().filter(|ids| !ids.is_empty()) {
        // A conversation whose every message was deleted is gone, which is also an answer.
        let Some(first) = ids.iter().find_map(|id| store.message(*id).ok()) else {
            continue;
        };
        let thread = store.thread(first.thread).unwrap();
        let messages = ids
            .iter()
            .filter_map(|id| store.message(*id).ok())
            .collect();
        out.push((thread, messages));
    }
    Held {
        threads: out,
        // Without the name of the act that queued it, which only a commit gives.
        queue: store
            .outbox_due(acct_account(), horizon())
            .unwrap()
            .into_iter()
            .map(|entry| OutboxEntry {
                forward: None,
                ..entry
            })
            .collect(),
    }
}

/// The patch written, its undo, and the server's half, as the operation decided them.
pub fn decide(store: &dyn Store, threads: &[Vec<MessageId>; 2], step: &Step) -> Option<Applied> {
    let ids = &threads[usize::from(step.thread)];
    let first = ids.iter().find_map(|id| store.message(*id).ok())?;
    let loaded = store.thread(first.thread).ok()?;
    let messages: Vec<Message> = loaded
        .messages
        .iter()
        .filter_map(|id| store.message(*id).ok())
        .collect();
    let target = match &step.only {
        None => Target::Threads(vec![first.thread]),
        Some(pick) => Target::Messages(
            loaded
                .messages
                .iter()
                .zip(pick.iter().cycle())
                .filter_map(|(id, on)| on.then_some(*id))
                .collect(),
        ),
    };
    Some(
        step.op
            .apply(&target, &loaded, &messages, &server_caps(), at(100)),
    )
}

/// What a person's act has done to the store so far: patch, then queue, then take the patch back
/// if the queue failed. The one way of writing an `Applied`, before `commit`.
pub fn write_by_hand(store: &dyn Store, applied: &Applied, server: bool) -> Result<(), String> {
    store
        .apply(acct_account(), &applied.forward)
        .map_err(|e| e.to_string())?;
    if let Some(intent) = applied.remote.clone().filter(|_| server)
        && store
            .enqueue(acct_account(), intent, &applied.inverse, at(100))
            .is_err()
    {
        store.apply(acct_account(), &applied.inverse).ok();
        return Err("not queued".to_owned());
    }
    Ok(())
}

/// A deletion is queued before it is applied, while the store still knows where its messages are.
pub fn write_in_order(store: &dyn Store, applied: &Applied, server: bool) -> Result<(), String> {
    if matches!(applied.remote, Some(RemoteIntent::Destroy { .. })) {
        if let Some(intent) = applied.remote.clone().filter(|_| server) {
            store
                .enqueue(acct_account(), intent, &applied.inverse, at(100))
                .map_err(|e| e.to_string())?;
        }
        return store
            .apply(acct_account(), &applied.forward)
            .map_err(|e| e.to_string());
    }
    write_by_hand(store, applied, server)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// Any run of operations leaves the SQLite store and the memory store holding the same
    /// conversations and the same queue.
    #[test]
    fn both_stores_come_out_of_a_run_of_operations_alike(specs in specs(), steps in steps()) {
        let both = Both::new();
        let stores = [&both.sqlite as &dyn Store, &both.memory];
        let threads = seed(&stores, &specs);
        prop_assert_eq!(held(&both.sqlite, &threads), held(&both.memory, &threads));
        for step in &steps {
            let Some(applied) = decide(&both.sqlite, &threads, step) else { continue };
            let sqlite = write_in_order(&both.sqlite, &applied, step.server);
            let memory = write_in_order(&both.memory, &applied, step.server);
            prop_assert_eq!(sqlite, memory);
            prop_assert_eq!(held(&both.sqlite, &threads), held(&both.memory, &threads));
        }
    }

    /// An operation and then its inverse give the conversation back as it was, in either store,
    /// whatever else was done in between to other messages. Deleting forever has no way back.
    #[test]
    fn an_operation_then_its_inverse_gives_the_conversation_back(
        specs in specs(),
        step in steps().prop_map(|mut s| s.remove(0)),
    ) {
        prop_assume!(step.op != Op::Destroy);
        let both = Both::new();
        let stores = [&both.sqlite as &dyn Store, &both.memory];
        let threads = seed(&stores, &specs);
        for (name, store) in both.each() {
            let before = held(store, &threads).threads;
            let Some(applied) = decide(store, &threads, &step) else { continue };
            write_in_order(store, &applied, step.server).unwrap();
            store.apply(acct_account(), &applied.inverse).unwrap();
            let mut after = held(store, &threads).threads;
            let mut before = before;
            for (_, messages) in before.iter_mut().chain(after.iter_mut()) {
                for m in messages {
                    m.labels.sort();
                }
            }
            prop_assert_eq!(&after, &before, "{} did not give it back", name);
        }
    }
}

/// An account with no server is told nothing, and a message with no address on one gives the
/// server nothing to be told: the change is made here and the queue stays empty.
#[test]
fn a_local_change_queues_nothing() {
    let both = Both::new();
    let stores = [&both.sqlite as &dyn Store, &both.memory];
    let threads = seed(
        &stores,
        &[Spec {
            thread: false,
            read: false,
            star: false,
            mailbox: 0,
            labels: (false, false),
            on_server: false,
        }],
    );
    let step = Step {
        thread: false,
        only: None,
        op: Op::SetRead(ReadState::Read),
        server: true,
    };
    let applied = decide(&both.sqlite, &threads, &step).unwrap();
    assert!(applied.remote.is_some(), "the op asks the server");
    for (name, store) in both.each() {
        write_in_order(store, &applied, true).unwrap();
        assert!(
            held(store, &threads).queue.is_empty(),
            "{name} queued work for a message the server does not hold"
        );
        assert_eq!(
            store.message(threads[0][0]).unwrap().read,
            ReadState::Read,
            "{name} did not make the change here"
        );
    }
}

/// A deletion is queued before it is applied: the queued work names the messages' addresses,
/// which the deletion takes with it.
#[test]
fn a_deletion_is_queued_with_the_addresses_it_takes() {
    let both = Both::new();
    let stores = [&both.sqlite as &dyn Store, &both.memory];
    let threads = seed(
        &stores,
        &[Spec {
            thread: false,
            read: true,
            star: false,
            mailbox: MailboxRole::ALL
                .iter()
                .position(|r| *r == MailboxRole::Trash)
                .unwrap() as u8,
            labels: (false, false),
            on_server: true,
        }],
    );
    let step = Step {
        thread: false,
        only: None,
        op: Op::Destroy,
        server: true,
    };
    let applied = decide(&both.sqlite, &threads, &step).unwrap();
    for (name, store) in both.each() {
        write_in_order(store, &applied, true).unwrap();
        let queue = held(store, &threads).queue;
        assert_eq!(queue.len(), 1, "{name}");
        assert!(
            matches!(&queue[0].op, ProtoOp::Destroy { remotes } if remotes.len() == 1),
            "{name} queued {:?}",
            queue[0].op
        );
        assert!(store.message(threads[0][0]).is_err(), "{name} kept it");
    }
}

/// A rule's action is the same operation, written the same way: queued for a message the server
/// holds and not for one it does not (which is every message of an account with no server), and
/// the change is made here either way.
#[test]
fn a_rule_queues_for_a_message_the_server_holds_and_for_no_other() {
    let both = Both::new();
    let stores = [&both.sqlite as &dyn Store, &both.memory];
    let threads = seed(
        &stores,
        &[
            Spec {
                thread: false,
                read: false,
                star: false,
                mailbox: 0,
                labels: (false, false),
                on_server: true,
            },
            Spec {
                thread: true,
                read: false,
                star: false,
                mailbox: 0,
                labels: (false, false),
                on_server: false,
            },
        ],
    );
    for (name, store) in both.each() {
        for (n, ids) in threads.iter().enumerate() {
            let message = store.message(ids[0]).unwrap();
            // As a rule applies it: to the one message, the server told whenever there is a
            // server half, and the message changed here either way.
            let thread = store.thread(message.thread).unwrap();
            let messages: Vec<Message> = thread
                .messages
                .iter()
                .map(|id| store.message(*id).unwrap())
                .collect();
            let applied = Op::SetRead(ReadState::Read).apply(
                &Target::Messages(vec![message.id]),
                &thread,
                &messages,
                &server_caps(),
                at(100),
            );
            let written = store
                .commit(&applied.into_commit(message.account.clone(), at(100), true))
                .unwrap();
            let queued = written.queued.iter().flatten().count();
            assert_eq!(queued, usize::from(n == 0), "{name} message {n}");
            assert_eq!(store.message(ids[0]).unwrap().read, ReadState::Read);
        }
        assert_eq!(held(store, &threads).queue.len(), 1, "{name}");
    }
}

// ---------------------------------------------------------------------------------------
// One commit
// ---------------------------------------------------------------------------------------

/// The decision as one value, for a person's act: the server told when `server` says so.
fn committed(applied: &Applied, server: bool) -> Commit {
    applied.clone().into_commit(acct_account(), at(100), server)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// One commit writes what the patch, the queue and the rollback by hand wrote, in either
    /// store, and both stores agree about it.
    #[test]
    fn a_commit_writes_what_writing_by_hand_wrote(specs in specs(), steps in steps()) {
        let by_hand = Both::new();
        let by_commit = Both::new();
        let hand = [&by_hand.sqlite as &dyn Store, &by_hand.memory];
        let commit = [&by_commit.sqlite as &dyn Store, &by_commit.memory];
        let threads = seed(&hand, &specs);
        seed(&commit, &specs);
        for step in &steps {
            let Some(applied) = decide(&by_hand.sqlite, &threads, step) else { continue };
            let wanted = write_in_order(&by_hand.sqlite, &applied, step.server);
            prop_assert_eq!(&wanted, &write_in_order(&by_hand.memory, &applied, step.server));
            for (name, store) in by_commit.each() {
                let written = store.commit(&committed(&applied, step.server));
                prop_assert_eq!(wanted.is_ok(), written.is_ok(), "{}", name);
            }
            let want = held(&by_hand.sqlite, &threads);
            prop_assert_eq!(&held(&by_commit.sqlite, &threads), &want);
            prop_assert_eq!(&held(&by_commit.memory, &threads), &want);
        }
    }
}

/// A commit that carries a piece of work for the server says which entry each made.
#[test]
fn a_commit_says_what_it_queued() {
    let both = Both::new();
    let stores = [&both.sqlite as &dyn Store, &both.memory];
    let on = |on_server| Spec {
        thread: false,
        read: false,
        star: false,
        mailbox: 0,
        labels: (false, false),
        on_server,
    };
    let threads = seed(&stores, &[on(true)]);
    let step = Step {
        thread: false,
        only: None,
        op: Op::SetRead(ReadState::Read),
        server: true,
    };
    let applied = decide(&both.sqlite, &threads, &step).unwrap();
    for (name, store) in both.each() {
        let written = store.commit(&committed(&applied, true)).unwrap();
        let queue = held(store, &threads).queue;
        assert_eq!(
            written.queued,
            queue.iter().map(|entry| Some(entry.id)).collect::<Vec<_>>(),
            "{name}"
        );
    }
}

/// What a commit that fails leaves behind is nothing: the conversation and the queue as they
/// were, wherever in the commit it failed.
mod atomicity {
    use super::*;

    /// A conversation in Trash that the server holds, and a deletion of it to commit.
    fn trashed() -> (Both, [Vec<MessageId>; 2], Commit) {
        let both = Both::new();
        let stores = [&both.sqlite as &dyn Store, &both.memory];
        let trash = MailboxRole::ALL
            .iter()
            .position(|r| *r == MailboxRole::Trash)
            .unwrap() as u8;
        let threads = seed(
            &stores,
            &[Spec {
                thread: false,
                read: true,
                star: false,
                mailbox: trash,
                labels: (false, false),
                on_server: true,
            }],
        );
        let step = Step {
            thread: false,
            only: None,
            op: Op::Destroy,
            server: true,
        };
        let applied = decide(&both.sqlite, &threads, &step).unwrap();
        let commit = committed(&applied, true);
        assert_eq!(commit.queue[0].resolve, Resolve::BeforeForward);
        (both, threads, commit)
    }

    fn unchanged(both: &Both, threads: &[Vec<MessageId>; 2], before: &Held) {
        for (name, store) in both.each() {
            assert_eq!(
                &held(store, threads),
                before,
                "{name} wrote part of a failed commit"
            );
        }
    }

    /// The local change is refused: the deletion already addressed, before it, is undone.
    #[test]
    fn a_change_that_is_refused_takes_the_queue_before_it_back() {
        let (both, threads, mut commit) = trashed();
        let before = held(&both.sqlite, &threads);
        commit.forward.changes.push(Change::MessageLabel(
            threads[0][0],
            LabelId::generate(),
            Membership::In,
        ));
        // The memory store has no foreign keys to refuse a label that is not there.
        assert!(both.sqlite.commit(&commit).is_err());
        assert_eq!(held(&both.sqlite, &threads), before);
    }

    /// Work after the change that is refused takes the change, and the work before it, back.
    #[test]
    fn work_that_is_refused_takes_the_change_back() {
        let (both, threads, mut commit) = trashed();
        let before = held(&both.sqlite, &threads);
        let unknown = MessageId::generate();
        commit.queue.push(Queue {
            intent: RemoteIntent::SetFlags {
                messages: vec![unknown],
                read: Some(ReadState::Read),
                star: None,
            },
            undo: patch(Vec::new()),
            id: None,
            resolve: Resolve::AfterForward,
            unplaced: Unplaced::Rollback,
        });
        for (name, store) in both.each() {
            assert!(
                matches!(
                    store.commit(&commit),
                    Err(mail_store::StoreError::NotQueued)
                ),
                "{name}"
            );
        }
        unchanged(&both, &threads, &before);
    }

    /// The same work, left out where it names nothing, is skipped and the rest is written.
    #[test]
    fn work_that_names_nothing_is_skipped_where_that_is_said() {
        let (both, _, mut commit) = trashed();
        commit.queue.push(Queue {
            intent: RemoteIntent::SetFlags {
                messages: vec![MessageId::generate()],
                read: Some(ReadState::Read),
                star: None,
            },
            undo: patch(Vec::new()),
            id: None,
            resolve: Resolve::AfterForward,
            unplaced: Unplaced::Skip,
        });
        for (name, store) in both.each() {
            let written = store.commit(&commit).unwrap();
            assert!(
                written.queued[0].is_some() && written.queued[1].is_none(),
                "{name}"
            );
        }
    }

    /// The outbox refuses the first piece of work.
    #[test]
    fn a_queue_the_outbox_refuses_writes_nothing() {
        let (both, threads, commit) = trashed();
        let before = held(&both.sqlite, &threads);
        mail_store::testing::refuse_outbox_writes(&both.sqlite);
        assert!(both.sqlite.commit(&commit).is_err());
        assert_eq!(held(&both.sqlite, &threads), before);
    }

    /// A row refused after the change and the work were written takes both back.
    #[test]
    fn a_row_that_is_refused_takes_the_change_and_the_queue_back() {
        let (both, threads, mut commit) = trashed();
        let before = held(&both.sqlite, &threads);
        both.sqlite
            .raw_connection()
            .execute_batch(
                "CREATE TRIGGER refuse_junk BEFORE INSERT ON junk_learned \
                 BEGIN SELECT RAISE(ABORT, 'full'); END;",
            )
            .unwrap();
        commit.rows.push(Row::JunkTaught {
            account: acct_account(),
            message: threads[0][0],
            words: vec!["viagra".to_owned()],
            judged: Judged::Spam,
        });
        assert!(both.sqlite.commit(&commit).is_err());
        assert_eq!(held(&both.sqlite, &threads), before);
        assert_eq!(
            both.sqlite
                .junk_counts(acct_account(), &["viagra".to_owned()])
                .unwrap(),
            JunkCounts::default()
        );
    }
}

/// What the junk filter learns in a commit is the same in either store, and what is forgotten
/// is forgotten.
#[test]
fn a_commit_teaches_and_untaches_the_junk_filter_alike_in_both_stores() {
    let both = Both::new();
    let stores = [&both.sqlite as &dyn Store, &both.memory];
    let threads = seed(
        &stores,
        &[Spec {
            thread: false,
            read: true,
            star: false,
            mailbox: 0,
            labels: (false, false),
            on_server: false,
        }],
    );
    let message = threads[0][0];
    let words: Vec<String> = ["cheap", "pills"].map(str::to_owned).into();
    let teach = |judged| Commit {
        account: acct_account(),
        at: at(100),
        forward: patch(Vec::new()),
        queue: Vec::new(),
        rows: vec![Row::JunkTaught {
            account: acct_account(),
            message,
            words: words.clone(),
            judged,
        }],
    };
    let forget = |judged| Commit {
        rows: vec![Row::JunkForgotten {
            account: acct_account(),
            message,
            judged,
        }],
        ..teach(judged)
    };
    let counts = || {
        (
            both.sqlite.junk_counts(acct_account(), &words).unwrap(),
            both.memory.junk_counts(acct_account(), &words),
        )
    };
    for commit in [teach(Judged::Spam), teach(Judged::Spam), teach(Judged::Ham)] {
        both.sqlite.commit(&commit).unwrap();
        both.memory.commit(&commit).unwrap();
        let (sqlite, memory) = counts();
        assert_eq!(sqlite, memory);
    }
    let (sqlite, _) = counts();
    assert_eq!(
        (sqlite.spam, sqlite.ham),
        (0, 1),
        "turned, and counted once"
    );
    // Forgetting it as spam, which it no longer is, does nothing; as ham, it leaves nothing.
    for commit in [forget(Judged::Spam), forget(Judged::Ham)] {
        both.sqlite.commit(&commit).unwrap();
        both.memory.commit(&commit).unwrap();
        let (sqlite, memory) = counts();
        assert_eq!(sqlite, memory);
    }
    assert_eq!(counts().0.ham, 0);
}

/// Taking an act back withdraws the work that act queued, and only that.
mod withdrawing {
    use super::*;

    fn on_server() -> Spec {
        Spec {
            thread: false,
            read: false,
            star: false,
            mailbox: 0,
            labels: (false, false),
            on_server: true,
        }
    }

    fn act(store: &dyn Store, threads: &[Vec<MessageId>; 2], op: Op) -> Applied {
        let step = Step {
            thread: false,
            only: None,
            op,
            server: true,
        };
        let applied = decide(store, threads, &step).unwrap();
        store.commit(&committed(&applied, true)).unwrap();
        applied
    }

    /// The commit that takes `applied` back: its inverse, and its work out of the outbox.
    fn take_back(applied: &Applied) -> Commit {
        Commit {
            account: acct_account(),
            at: at(200),
            forward: applied.inverse.clone(),
            queue: Vec::new(),
            rows: vec![Row::Withdrawn {
                forward: applied.forward.id,
                inverse: applied.inverse.clone(),
            }],
        }
    }

    #[test]
    fn an_act_takes_back_its_own_entry_and_no_other() {
        let both = Both::new();
        let stores = [&both.sqlite as &dyn Store, &both.memory];
        let threads = seed(&stores, &[on_server()]);
        for (name, store) in both.each() {
            let read = act(store, &threads, Op::SetRead(ReadState::Read));
            let starred = act(store, &threads, Op::SetStar(Star::Starred));
            assert_eq!(held(store, &threads).queue.len(), 2, "{name}");
            store.commit(&take_back(&starred)).unwrap();
            let queue = held(store, &threads).queue;
            assert_eq!(queue.len(), 1, "{name}");
            assert_eq!(
                queue[0].undo, read.inverse,
                "{name} withdrew the wrong entry"
            );
        }
    }

    #[test]
    fn an_entry_the_server_has_been_tried_with_stays() {
        let both = Both::new();
        let stores = [&both.sqlite as &dyn Store, &both.memory];
        let threads = seed(&stores, &[on_server()]);
        for (name, store) in both.each() {
            let read = act(store, &threads, Op::SetRead(ReadState::Read));
            let entry = held(store, &threads).queue.remove(0);
            store
                .outbox_settle(
                    entry.id,
                    mail_store::Settle::Failed {
                        reason: "offline".to_owned(),
                        retry: Retry::Now,
                    },
                    at(150),
                )
                .unwrap();
            store.commit(&take_back(&read)).unwrap();
            assert_eq!(held(store, &threads).queue.len(), 1, "{name}");
        }
    }

    /// Work queued before acts were named has no name to find it by, and is found by its undo.
    #[test]
    fn an_entry_queued_without_a_name_is_found_by_its_undo() {
        let both = Both::new();
        let stores = [&both.sqlite as &dyn Store, &both.memory];
        let threads = seed(&stores, &[on_server()]);
        let step = Step {
            thread: false,
            only: None,
            op: Op::SetRead(ReadState::Read),
            server: true,
        };
        let applied = decide(&both.sqlite, &threads, &step).unwrap();
        for (name, store) in both.each() {
            write_in_order(store, &applied, true).unwrap();
            assert_eq!(held(store, &threads).queue.len(), 1, "{name}");
            store.commit(&take_back(&applied)).unwrap();
            assert!(held(store, &threads).queue.is_empty(), "{name}");
        }
    }
}
