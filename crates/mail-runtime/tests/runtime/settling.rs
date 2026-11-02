//! A message is never sent twice and a row is never lost, whatever the server says and in
//! whatever order.
//!
//! The drains of every protocol carry out [`mail_runtime::sending::settle`]'s answer step by
//! step. Here a drain is carried out the same way over a real outbox, against a script of
//! outcomes (the server accepted, asked to wait, refused the password, refused for good, or the
//! person closed the app), pass after pass, and what the outbox and the drafts hold is checked
//! after each.

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use mail_domain::*;
use mail_proto::{ProtoError, Refusal};
use mail_runtime::RuntimeError;
use mail_runtime::sending::{Login, Outcome, Then, mark_draft, settle};
use mail_store::{SqliteStore, Store};
use porter_core::AccountId;
use proptest::prelude::*;
use std::collections::HashMap;

fn account() -> AccountId {
    mail_domain::id::account_id_from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000c9"))
}

fn start() -> DateTime<Utc> {
    Utc.timestamp_opt(1_790_000_000, 0).unwrap()
}

#[derive(Debug, Clone, Copy)]
enum Said {
    Accepted,
    Wait,
    Password,
    Refused,
    Closed,
}

fn said() -> impl Strategy<Value = Said> {
    prop_oneof![
        4 => Just(Said::Accepted),
        2 => Just(Said::Wait),
        1 => Just(Said::Password),
        2 => Just(Said::Refused),
        1 => Just(Said::Closed),
    ]
}

fn error(said: Said) -> Option<RuntimeError> {
    match said {
        Said::Accepted => None,
        Said::Wait => Some(RuntimeError::Proto(ProtoError::Refused {
            kind: Refusal::Transient,
            text: "451 later".to_owned(),
        })),
        Said::Password => Some(RuntimeError::Proto(ProtoError::AuthRejected(
            "535".to_owned(),
        ))),
        Said::Refused => Some(RuntimeError::Proto(ProtoError::Refused {
            kind: Refusal::Permanent,
            text: "550 no".to_owned(),
        })),
        Said::Closed => Some(RuntimeError::Cancelled),
    }
}

/// `count` drafts, each queued to be sent, and the store they are in.
fn queued(count: usize) -> (tempfile::TempDir, SqliteStore, Vec<DraftId>) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::in_memory(dir.path()).unwrap();
    mail_store::testing::seed_account(&store, account(), "me@example.test");
    let identity = IdentityId::generate();
    mail_store::testing::seed_identity_for(&store, identity, account(), "me@example.test", None);
    let mut drafts = Vec::new();
    for n in 0..count {
        let draft = Draft {
            id: DraftId::generate(),
            account: account(),
            identity,
            to: vec![Address::new("bea@example.test")],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: format!("message {n}"),
            in_reply_to: None,
            forward_of: None,
            text: "hello\r\n".to_owned(),
            html: None,
            attachments: Vec::new(),
            receipt: ReceiptRequest::Unrequested,
            openpgp: None,
            smime: None,
            priority: Priority::Normal,
            delivery: DeliveryReport::Unrequested,
            automatic: false,
            state: SendState::Editing,
            updated: start(),
        };
        store
            .apply(
                account(),
                &Patch {
                    id: ChangeId::generate(),
                    changes: vec![Change::DraftUpsert(Box::new(draft.clone()))],
                },
                chrono::Utc::now(),
            )
            .unwrap();
        let raw = store
            .blobs()
            .put(format!("Subject: {n}\r\n\r\nhi").as_bytes())
            .unwrap();
        store
            .enqueue(
                account(),
                RemoteIntent::Send {
                    draft: draft.id,
                    raw,
                    mail_from: "me@example.test".to_owned(),
                    rcpt_to: vec!["bea@example.test".to_owned()],
                    delivery: DeliveryReport::Unrequested,
                },
                &Patch {
                    id: ChangeId::generate(),
                    changes: Vec::new(),
                },
                start(),
            )
            .unwrap()
            .expect("a submission is always queued");
        store
            .set_send_state(draft.id, &SendState::Queued, start())
            .unwrap();
        drafts.push(draft.id);
    }
    (dir, store, drafts)
}

/// The drafts whose submissions are in the outbox, however far off they are due.
fn waiting(store: &SqliteStore) -> Vec<DraftId> {
    store
        .outbox_due(account(), start() + TimeDelta::try_days(10_000).unwrap())
        .unwrap()
        .into_iter()
        .filter_map(|entry| match entry.op {
            ProtoOp::Submit { draft, .. } => Some(draft),
            _ => None,
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Whatever the server says, in whatever order, over as many passes as it takes:
    ///
    /// - a message the server accepted is sent once, and its row is gone for good;
    /// - a row the server has not accepted is either still in the outbox, its draft saying it is
    ///   queued, on its way, or failing; or it was refused for good and the draft says so;
    /// - no row is gone without one of those two.
    #[test]
    fn nothing_is_sent_twice_and_no_row_is_lost(
        count in 1usize..5,
        script in proptest::collection::vec((any::<Login2>(), said()), 1..40),
    ) {
        let (_dir, store, drafts) = queued(count);
        let mut sent: HashMap<DraftId, u32> = HashMap::new();
        let mut script = script.into_iter();
        let mut now = start();
        'passes: for _ in 0..40 {
            now += TimeDelta::try_days(1).unwrap();
            for entry in store.outbox_due(account(), now).unwrap() {
                let Some((login, said)) = script.next() else { break 'passes };
                let ProtoOp::Submit { draft, .. } = &entry.op else { unreachable!() };
                mark_draft(&store, *draft, SendState::Sending, now);
                let failure = error(said);
                let outcome = match &failure {
                    None => Outcome::Accepted,
                    Some(error) => Outcome::Refused { error, done: Vec::new(), said: String::new() },
                };
                let settled = settle(&entry.op, login.0, outcome, now);
                let mut kept = None;
                for step in &settled.then {
                    match step {
                        Then::SettleRow(row) => store.outbox_settle(entry.id, row.clone(), now).unwrap(),
                        Then::KeepSentCopy => kept = None,
                        Then::Mark(draft, mark) => {
                            mark_draft(&store, *draft, mark.clone().state(kept), now);
                        }
                        Then::KeepUpload | Then::DropDraftCopies(_) => {}
                    }
                }
                if settled.submitted {
                    *sent.entry(*draft).or_default() += 1;
                }
                if settled.halt {
                    break;
                }
            }
            check(&store, &drafts, &sent)?;
        }
        check(&store, &drafts, &sent)?;
    }
}

/// `Login`, which has no `Arbitrary` of its own.
#[derive(Debug, Clone, Copy)]
struct Login2(Login);

impl Arbitrary for Login2 {
    type Parameters = ();
    type Strategy = BoxedStrategy<Login2>;
    fn arbitrary_with((): ()) -> Self::Strategy {
        prop_oneof![Just(Login2(Login::Apart)), Just(Login2(Login::Shared))].boxed()
    }
}

fn check(
    store: &SqliteStore,
    drafts: &[DraftId],
    sent: &HashMap<DraftId, u32>,
) -> Result<(), TestCaseError> {
    let rows = waiting(store);
    for draft in drafts {
        let times = sent.get(draft).copied().unwrap_or(0);
        prop_assert!(times <= 1, "{draft} was sent {times} times");
        let in_box = rows.iter().filter(|row| *row == draft).count();
        prop_assert!(in_box <= 1, "{draft} is queued {in_box} times");
        let state = store.draft(*draft).unwrap().state;
        match (times, in_box, &state) {
            (1, 0, SendState::Sent { .. }) => {}
            (0, 1, SendState::Queued | SendState::Sending | SendState::Failed { .. }) => {
                if let SendState::Failed { retry, .. } = &state {
                    prop_assert!(
                        !matches!(retry, Retry::Fatal(_)),
                        "{draft} waits but is lost"
                    );
                }
            }
            (
                0,
                0,
                SendState::Failed {
                    retry: Retry::Fatal(_),
                    ..
                },
            ) => {}
            other => prop_assert!(
                false,
                "{draft}: sent {}, queued {}, {:?}",
                other.0,
                other.1,
                other.2
            ),
        }
    }
    Ok(())
}
