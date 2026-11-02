//! A send written as one commit: the submission queued, the earlier one taken back, the draft's
//! state set and a reminder held, or none of it. Both stores, compared.

use crate::support::*;
use mail_domain::*;
use mail_store::testing::{seed_account, seed_identity_for};
use mail_store::{SqliteStore, Store};

const IDENTITY: IdentityId =
    IdentityId::from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000b1"));

fn seed(sqlite: &SqliteStore) {
    seed_account(sqlite, acct_account(), "me@example.test");
    seed_identity_for(
        sqlite,
        IDENTITY,
        acct_account(),
        "me@example.test",
        Some("Me"),
    );
}

fn draft(state: SendState) -> Draft {
    Draft {
        id: DraftId::generate(),
        account: acct_account(),
        identity: IDENTITY,
        to: vec![Address::new("you@example.test")],
        cc: vec![],
        bcc: vec![],
        subject: "tomorrow morning".to_owned(),
        in_reply_to: None,
        forward_of: None,
        text: "hello".to_owned(),
        html: None,
        attachments: vec![],
        receipt: ReceiptRequest::Unrequested,
        openpgp: None,
        smime: None,
        priority: Priority::Normal,
        delivery: DeliveryReport::Unrequested,
        automatic: false,
        state,
        updated: at(0),
    }
}

fn save(store: &dyn Store, draft: &Draft) {
    store
        .apply(
            acct_account(),
            &patch(vec![Change::DraftUpsert(Box::new(draft.clone()))]),
        )
        .unwrap();
}

/// What pressing send writes for `draft`, which was read as `read`: the earlier submission taken
/// back when `pulls_back`, then this one queued, the state set, a reminder held.
fn send(draft: &Draft, read: &Draft, pulls_back: bool, leaves: i64) -> Commit {
    let back = Draft {
        state: SendState::Editing,
        updated: at(10),
        ..draft.clone()
    };
    let mut rows = vec![
        Row::Expect(Expect::Draft {
            draft: draft.id,
            updated: read.updated,
            state: read.state.clone(),
        }),
        Row::FollowUpReleased { draft: draft.id },
        Row::SendState {
            draft: draft.id,
            state: SendState::Queued,
            at: at(10),
        },
    ];
    rows.push(Row::FollowUpHeld {
        draft: draft.id,
        account: acct_account(),
        message_id: "m1@example.test".to_owned(),
        thread: None,
        due: at(86_400),
        set: at(leaves),
    });
    Commit {
        account: acct_account(),
        at: at(leaves),
        forward: patch(match pulls_back {
            true => vec![
                Change::DraftDelete(draft.id),
                Change::DraftUpsert(Box::new(back)),
            ],
            false => vec![],
        }),
        queue: vec![Queue {
            intent: RemoteIntent::Send {
                draft: draft.id,
                raw: BlobId::generate(),
                mail_from: "me@example.test".to_owned(),
                rcpt_to: vec!["you@example.test".to_owned()],
                delivery: DeliveryReport::Unrequested,
            },
            undo: patch(vec![]),
            id: None,
            resolve: Resolve::AfterForward,
            unplaced: Unplaced::Rollback,
        }],
        rows,
    }
}

fn both() -> Both {
    let b = Both::new();
    seed(&b.sqlite);
    b
}

fn sends(store: &dyn Store) -> Vec<(DraftId, chrono::DateTime<chrono::Utc>)> {
    store
        .outbox_due(acct_account(), at(10_000_000))
        .unwrap()
        .into_iter()
        .filter_map(|entry| match entry.op {
            ProtoOp::Submit { draft, .. } => Some((draft, entry.next_attempt)),
            _ => None,
        })
        .collect()
}

/// The submission is queued to leave at the commit's time, the draft says queued and when it
/// was last touched, and the reminder is held (SQLite is the store that lists them).
#[test]
fn a_send_queues_the_submission_sets_the_draft_and_holds_the_reminder() {
    let b = both();
    let draft = draft(SendState::Editing);
    for (name, store) in b.each() {
        save(store, &draft);
        store.commit(&send(&draft, &draft, false, 3_600)).unwrap();
        assert_eq!(sends(store), [(draft.id, at(3_600))], "{name}");
        let stored = store.draft(draft.id).unwrap();
        assert_eq!(stored.state, SendState::Queued, "{name}");
        assert_eq!(stored.updated, at(10), "{name}");
    }
    let held = b.sqlite.follow_up_holds().unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].message_id, "m1@example.test");
    assert_eq!((held[0].at, held[0].set), (at(86_400), at(3_600)));
}

/// An earlier submission of the same draft goes with it, in the same commit, and one is left.
#[test]
fn a_send_again_takes_the_earlier_submission_back_and_leaves_one() {
    let b = both();
    let draft = draft(SendState::Editing);
    for (name, store) in b.each() {
        save(store, &draft);
        store.commit(&send(&draft, &draft, false, 100)).unwrap();
        let read = store.draft(draft.id).unwrap();
        store.commit(&send(&draft, &read, true, 200)).unwrap();
        assert_eq!(sends(store), [(draft.id, at(200))], "{name}: one, not two");
        assert_eq!(
            store.draft(draft.id).unwrap().state,
            SendState::Queued,
            "{name}"
        );
    }
    assert_eq!(b.sqlite.follow_up_holds().unwrap().len(), 1, "held once");
}

/// A draft that changed since it was read is refused, and nothing is written, wherever in the
/// commit that would have been.
#[test]
fn a_send_decided_over_a_draft_that_has_changed_writes_nothing() {
    let b = both();
    let draft = draft(SendState::Editing);
    for (name, store) in b.each() {
        save(store, &draft);
        // Edited, or settled by the outbox, after it was read.
        store
            .set_send_state(draft.id, &SendState::Sending, at(5))
            .unwrap();
        let refused = store.commit(&send(&draft, &draft, false, 100));
        assert!(
            matches!(refused, Err(mail_store::StoreError::Stale)),
            "{name}: {refused:?}"
        );
        assert_eq!(sends(store), [], "{name}");
        let stored = store.draft(draft.id).unwrap();
        assert_eq!(
            (stored.state, stored.updated),
            (SendState::Sending, at(5)),
            "{name}"
        );
    }
    assert!(b.sqlite.follow_up_holds().unwrap().is_empty());
}

/// A draft deleted since it was read is as changed as one edited.
#[test]
fn a_send_decided_over_a_draft_that_is_gone_is_stale() {
    let b = both();
    let draft = draft(SendState::Editing);
    for (name, store) in b.each() {
        let refused = store.commit(&send(&draft, &draft, false, 100));
        assert!(
            matches!(refused, Err(mail_store::StoreError::Stale)),
            "{name}: {refused:?}"
        );
        assert_eq!(sends(store), [], "{name}");
    }
}
