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
        queue: store.outbox_due(acct_account(), horizon()).unwrap(),
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
            let queued = mail_store::rules::perform(
                store,
                &server_caps(),
                &Op::SetRead(ReadState::Read),
                &message,
                at(100),
            )
            .unwrap();
            assert_eq!(queued, usize::from(n == 0), "{name} message {n}");
            assert_eq!(store.message(ids[0]).unwrap().read, ReadState::Read);
        }
        assert_eq!(held(store, &threads).queue.len(), 1, "{name}");
    }
}
