use super::*;
use crate::support_tests::{account, at, caps, message};
use mail_store::testing::{clear_outbox, in_memory, seed_caps};

/// An account that has a server, as the accounts an act is decided over need it to be.
fn seed_imap_account(store: &SqliteStore) {
    let plan = AccountPlan {
        address: "me@example.test".to_owned(),
        incoming: Incoming::Imap {
            host: "imap.example.test".to_owned(),
            port: 993,
            tls: Tls::Implicit,
        },
        outgoing: Outgoing::Nowhere,
        auth: AuthPlan::Password {
            username: Username::SameAsAddress,
            sasl: vec![SaslMech::Plain],
        },
        identities: Vec::new(),
    };
    mail_store::testing::seed_account_plan(store, account(), "me@example.test", &plan, None);
}

/// The messages of a loaded conversation.
fn messages_of(store: &SqliteStore, held: &Thread) -> Vec<Message> {
    held.messages
        .iter()
        .map(|id| store.message(*id).unwrap())
        .collect()
}

/// A store whose server files into named folders, holding one message in the inbox, and a label
/// the folder `Projects` is.
fn store_with_a_copy_target() -> (SqliteStore, ThreadId, LabelId) {
    let store = in_memory();
    seed_imap_account(&store);
    let caps = AccountCaps {
        archive: ArchiveMeans::MoveToFolder("Archive".to_owned()),
        ..caps()
    };
    seed_caps(&store, account(), &caps, at(0)).unwrap();
    let mut held = message(1, "Hello", "Body", 0, ReadState::Read, Star::Unstarred);
    // The message's bytes are a row of their own that it refers to.
    let raw = store.blobs().put(b"Body").unwrap();
    if let mail_domain::Body::Present { raw: slot, .. } = &mut held.body {
        *slot = raw;
    }
    let thread = held.thread;
    let message_id = held.id;
    store
        .apply(
            account(),
            &Patch {
                id: ChangeId::generate(),
                changes: vec![Change::MessageUpsert(Box::new(held))],
            },
        )
        .unwrap();
    // The server holds it, so work on it is queued for the server: a message with no address
    // there has nothing to tell the server about.
    mail_store::testing::seed_remote_uid(&store, account(), "INBOX", 1, message_id);
    let projects = store.create_label(account(), "Projects").unwrap();
    (store, thread, projects)
}

fn copies_queued(store: &SqliteStore) -> usize {
    mail_store::testing::outbox_ops(store)
        .iter()
        .filter(|op| matches!(op, ProtoOp::Copy { .. }))
        .count()
}

/// A copy still waiting for the server is withdrawn with its undo; one the server has made stays
/// made, and an undo that would only put it back at the next sync is refused, with nothing
/// changed here.
#[test]
fn a_copy_is_taken_back_only_while_it_is_still_waiting_for_the_server() {
    let (store, thread, projects) = store_with_a_copy_target();
    let in_projects = |store: &SqliteStore| {
        let held = store.thread(thread).unwrap();
        messages_of(store, &held)
            .iter()
            .all(|m| m.labels.contains(&projects))
    };

    let waiting = perform(&store, thread, Op::Copy(projects)).expect("copied");
    assert_eq!(copies_queued(&store), 1);
    assert!(in_projects(&store));
    assert!(take_back(&store, &waiting), "refused while it was waiting");
    assert!(!in_projects(&store));
    assert_eq!(copies_queued(&store), 0, "still queued after its undo");

    let sent = perform(&store, thread, Op::Copy(projects)).expect("copied again");
    // The server took it: nothing waits.
    clear_outbox(&store);
    assert!(
        !take_back(&store, &sent),
        "taken back after the server made it"
    );
    assert!(in_projects(&store), "the copy was taken out of its folder");
}

/// A store whose account keeps its mail on this computer alone, holding one message that the
/// store has an address for, as if it had once been on a server: the account decides, not the
/// address.
fn local_store() -> (SqliteStore, ThreadId) {
    let (store, thread, _) = store_with_a_copy_target();
    let plan = AccountPlan {
        address: "me@example.test".to_owned(),
        incoming: Incoming::Local,
        outgoing: Outgoing::Nowhere,
        auth: AuthPlan::Password {
            username: Username::SameAsAddress,
            sasl: vec![SaslMech::Plain],
        },
        identities: Vec::new(),
    };
    mail_store::testing::seed_account_plan(&store, account(), "me@example.test", &plan, None);
    (store, thread)
}

fn queued(store: &SqliteStore) -> usize {
    mail_store::testing::outbox_ops(store).len()
}

#[test]
fn an_act_on_an_account_with_a_server_queues_its_half_and_one_without_queues_none() {
    let (server, thread, _) = store_with_a_copy_target();
    perform(&server, thread, Op::Archive).expect("archived");
    assert_eq!(queued(&server), 1);

    let (local, thread) = local_store();
    let undo = perform(&local, thread, Op::Archive).expect("archived");
    assert_eq!(
        queued(&local),
        0,
        "a local account's outbox is never drained"
    );
    assert!(take_back(&local, &undo));
    assert_eq!(queued(&local), 0);
}

#[test]
fn a_deletion_on_an_account_without_a_server_tells_no_one() {
    let (store, thread) = local_store();
    let trashed = perform(&store, thread, Op::Trash).expect("trashed");
    assert!(trashed.remote.is_none() || queued(&store) == 0);
    let gone = destroy(&store, thread).expect("deleted");
    assert_eq!(gone.len(), 1);
    assert_eq!(queued(&store), 0);
    assert!(store.thread(thread).is_err());
}

#[test]
fn a_deletion_is_queued_with_the_addresses_it_removes() {
    let (store, thread, _) = store_with_a_copy_target();
    perform(&store, thread, Op::Trash).expect("trashed");
    mail_store::testing::clear_outbox(&store);
    let gone = destroy(&store, thread).expect("deleted");
    assert_eq!(gone.len(), 1);
    let ops = mail_store::testing::outbox_ops(&store);
    assert!(
        matches!(&ops[..], [ProtoOp::Destroy { remotes }] if remotes.len() == 1),
        "queued {ops:?}"
    );
}

/// What the system's own changes (a snooze waking, a follow-up) write: the change here, the
/// server told nothing, and nothing handed back to take it back with.
#[test]
fn a_change_the_system_makes_is_local_and_leaves_the_server_out() {
    let (store, thread, _) = store_with_a_copy_target();
    crate::snooze::apply(&store, thread, Op::Archive, at(5)).unwrap();
    assert_eq!(
        queued(&store),
        0,
        "archiving is not told to the server from here"
    );
    let held = store.thread(thread).unwrap();
    assert!(
        messages_of(&store, &held)
            .iter()
            .all(|m| m.mailbox == MailboxRole::Archive)
    );
    crate::snooze::apply(&store, thread, Op::SetSnooze(Snooze::Until(at(900))), at(5)).unwrap();
    assert_eq!(
        store.thread(thread).unwrap().summary.snooze,
        Snooze::Until(at(900))
    );
    assert_eq!(queued(&store), 0);
}

/// A filing taken back before the server heard of it is taken out of the outbox: exactly its
/// own entry, and not another message's.
#[test]
fn taking_back_a_move_withdraws_its_own_entry_and_no_other() {
    let (store, thread, projects) = store_with_a_copy_target();
    let other = {
        let mut held = message(2, "Other", "Body", 1, ReadState::Read, Star::Unstarred);
        let raw = store.blobs().put(b"Body").unwrap();
        if let mail_domain::Body::Present { raw: slot, .. } = &mut held.body {
            *slot = raw;
        }
        let (thread, id) = (held.thread, held.id);
        store
            .apply(
                account(),
                &Patch {
                    id: ChangeId::generate(),
                    changes: vec![Change::MessageUpsert(Box::new(held))],
                },
            )
            .unwrap();
        mail_store::testing::seed_remote_uid(&store, account(), "INBOX", 2, id);
        thread
    };
    let first = perform(&store, thread, Op::File(projects)).expect("filed");
    perform(&store, other, Op::File(projects)).expect("filed");
    assert_eq!(queued(&store), 2);
    assert!(take_back(&store, &first));
    let left = mail_store::testing::outbox_ops(&store);
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(matches!(&left[0], ProtoOp::File { .. }));
    let held = store.thread(other).unwrap();
    assert!(
        messages_of(&store, &held)
            .iter()
            .all(|m| m.mailbox == MailboxRole::Archive),
        "the other is still filed"
    );
}

/// A later operation on the same message was made on top of the move, so the move stays queued
/// and the server keeps it filed.
#[test]
fn a_move_stays_queued_when_a_later_operation_acts_on_the_same_message() {
    let (store, thread, projects) = store_with_a_copy_target();
    let filed = perform(&store, thread, Op::File(projects)).expect("filed");
    perform(&store, thread, Op::SetRead(ReadState::Unread)).expect("marked");
    assert_eq!(queued(&store), 2);
    assert!(take_back(&store, &filed));
    let ops = mail_store::testing::outbox_ops(&store);
    assert!(
        ops.iter().any(|op| matches!(op, ProtoOp::File { .. })),
        "the move was withdrawn from under the later work: {ops:?}"
    );
}

/// An entry the server has been tried with is not withdrawn.
#[test]
fn a_move_the_server_has_been_tried_with_is_not_withdrawn() {
    let (store, thread, projects) = store_with_a_copy_target();
    let filed = perform(&store, thread, Op::File(projects)).expect("filed");
    let entry = store
        .outbox_due(account(), queue_horizon())
        .unwrap()
        .remove(0);
    store
        .outbox_settle(
            entry.id,
            mail_store::Settle::Failed {
                reason: "offline".to_owned(),
                retry: Retry::Now,
            },
            at(1),
        )
        .unwrap();
    assert!(take_back(&store, &filed));
    let ops = mail_store::testing::outbox_ops(&store);
    assert!(
        ops.iter().any(|op| matches!(op, ProtoOp::File { .. })),
        "{ops:?}"
    );
}
