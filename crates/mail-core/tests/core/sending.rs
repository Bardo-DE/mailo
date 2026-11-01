//! Pressing send: a draft's bytes are frozen and its submission put in the outbox
//! (`compose::queue`), pressed again, scheduled, or refused.
//!
//! The account is a POP3 one that sends over SMTP, in a store in memory. Nothing here reaches a
//! server: it is the outbox that is looked at.

use crate::support::account;
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use mail_core::CoreError;
use mail_core::compose::{self, Leaves};
use mail_domain::*;
use mail_store::{SqliteStore, Store};

const ME: IdentityId = IdentityId::from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000b1"));

fn at(n: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + n, 0).unwrap()
}

fn seeded() -> SqliteStore {
    let store = mail_store::testing::in_memory();
    let mut plan = mail_domain::presets::manual_pop3(
        "me@example.test",
        &mail_domain::presets::ManualPop3 {
            pop3_host: "pop.example.test".to_owned(),
            pop3_port: 995,
            smtp_host: "smtp.example.test".to_owned(),
            smtp_port: 465,
            login: None,
        },
        at(0),
    )
    .plan;
    plan.outgoing = Outgoing::Smtp {
        host: "smtp.example.test".to_owned(),
        port: 465,
        tls: Tls::Implicit,
    };
    mail_store::testing::seed_account_plan(&store, account(), "me@example.test", &plan, None);
    mail_store::testing::seed_identity_for(&store, ME, account(), "me@example.test", Some("Me"));
    store
}

/// A draft to Bea, saved.
fn written(store: &SqliteStore) -> Draft {
    let draft = compose::draft_new(
        store,
        account(),
        &[Address::new("bea@example.test")],
        "lunch on friday",
        "Shall we say one o'clock?",
        at(1),
    )
    .unwrap();
    compose::save(store, &draft).unwrap();
    draft
}

/// The submissions in the outbox, each with when it may leave.
fn submissions(store: &SqliteStore) -> Vec<(DraftId, BlobId, DateTime<Utc>)> {
    store
        .outbox_due(account(), at(1_000_000))
        .unwrap()
        .into_iter()
        .filter_map(|entry| match entry.op {
            ProtoOp::Submit { draft, raw, .. } => Some((draft, raw, entry.next_attempt)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_draft_sent_is_one_submission_held_for_the_grace_and_the_draft_says_queued() {
    let store = seeded();
    let draft = written(&store);
    let due = at(10) + TimeDelta::seconds(5);
    let (queued, post) = compose::queue(&store, draft.id, Leaves::Now, due).unwrap();

    assert_eq!(queued.state, SendState::Queued);
    assert_eq!(store.draft(draft.id).unwrap().state, SendState::Queued);
    assert_eq!(post.rcpt_to, ["bea@example.test"]);
    let held = submissions(&store);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].0, draft.id);
    assert_eq!(held[0].2, due);
    // What the outbox holds is the message `posting` made, frozen.
    assert_eq!(store.blobs().get(held[0].1).unwrap(), post.message);
}

#[test]
fn pressing_send_again_or_changing_its_time_leaves_one_submission() {
    let store = seeded();
    let draft = written(&store);
    compose::queue(&store, draft.id, Leaves::Now, at(10)).unwrap();
    compose::queue(&store, draft.id, Leaves::Now, at(12)).unwrap();
    assert_eq!(submissions(&store).len(), 1, "once, not twice");

    let later = at(3_600);
    let (scheduled, _) = compose::queue(&store, draft.id, Leaves::At(later), at(20)).unwrap();
    assert_eq!(scheduled.state, SendState::Scheduled { at: later });
    let held = submissions(&store);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].2, later, "held until the new time");
    assert_eq!(
        store.draft(draft.id).unwrap().state,
        SendState::Scheduled { at: later }
    );
}

#[test]
fn a_time_that_has_gone_leaves_an_existing_schedule_alone() {
    let store = seeded();
    let draft = written(&store);
    let later = at(3_600);
    compose::queue(&store, draft.id, Leaves::At(later), at(10)).unwrap();
    let refused = compose::queue(&store, draft.id, Leaves::At(at(5)), at(20));
    assert!(matches!(refused, Err(CoreError::SendTimePassed { .. })));
    assert_eq!(submissions(&store).len(), 1);
    assert_eq!(
        store.draft(draft.id).unwrap().state,
        SendState::Scheduled { at: later }
    );
}

#[test]
fn an_unsent_draft_is_editable_and_has_no_submission() {
    let store = seeded();
    let draft = written(&store);
    compose::queue(&store, draft.id, Leaves::Now, at(10)).unwrap();
    let back = compose::unsend(&store, draft.id, at(11)).unwrap();
    assert_eq!(back.state, SendState::Editing);
    assert!(submissions(&store).is_empty());
    assert_eq!(store.draft(draft.id).unwrap().state, SendState::Editing);
}

#[test]
fn a_sent_draft_cannot_be_sent_again() {
    let store = seeded();
    let draft = written(&store);
    compose::queue(&store, draft.id, Leaves::Now, at(10)).unwrap();
    store
        .set_send_state(draft.id, &SendState::Sending, at(11))
        .unwrap();
    assert!(matches!(
        compose::queue(&store, draft.id, Leaves::Now, at(12)),
        Err(CoreError::AlreadySending)
    ));
    assert_eq!(submissions(&store).len(), 1);
}

/// Asking, once queued, to be encrypted to someone there is no key for: the next press of send
/// is refused. Which of the earlier submission and the draft's state survives that is what
/// `a_send_refused_leaves_the_queued_one_where_it_was` says.
fn asked_to_encrypt(store: &SqliteStore, draft: DraftId) {
    let mut queued = store.draft(draft).unwrap();
    queued.openpgp = Some(Mode::Encrypt);
    store
        .apply(
            account(),
            &Patch {
                id: ChangeId::generate(),
                changes: vec![Change::DraftUpsert(Box::new(queued))],
            },
        )
        .unwrap();
}

/// Today the earlier submission is taken back before anything can refuse the new one, so the
/// refusal leaves nothing queued and the draft editable. Phase 4 makes the refusal leave it as it
/// was; this is the expectation it changes.
#[test]
fn a_send_refused_pulls_the_queued_one_back_today() {
    let store = seeded();
    let draft = written(&store);
    compose::queue(&store, draft.id, Leaves::Now, at(10)).unwrap();
    asked_to_encrypt(&store, draft.id);

    assert!(compose::queue(&store, draft.id, Leaves::Now, at(20)).is_err());
    assert!(submissions(&store).is_empty());
    assert_eq!(store.draft(draft.id).unwrap().state, SendState::Editing);
}
