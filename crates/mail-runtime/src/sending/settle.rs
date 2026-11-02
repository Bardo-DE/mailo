//! How an attempt on a queued operation settles: the row, the draft, the pass, and what is
//! left to do, as one value.
//!
//! The drain of every protocol (SMTP and Graph in `mail-engine`, JMAP in [`crate::jmap`]) makes
//! the attempt, then asks here what it came to. The answer is a [`Settled`], a function of the
//! operation, whose credential it used and what the server said, with no store, clock or
//! network in it; each drain carries it out in the order it lists, with its own means of keeping
//! a copy. So the cases a message can end in (sent, waiting, refused for good, refused for good
//! after part of it was done) are decided in one place, and a table of them is a test.
//!
//! A sent message is never sent twice and a row is never dropped unsaid, whatever the
//! outcomes: a row is settled [`Settle::Ok`] only for an attempt the server accepted, and only a
//! refusal for good drops it, saying in the draft why ([`Mark::Failed`]) first. The property
//! tests of `tests/runtime/settling.rs` hold the drains to that.

use crate::RuntimeError;
use chrono::{DateTime, Utc};
use mail_domain::{DraftId, MessageId, ProtoOp, Retry, Retryable, SendState};
use mail_store::Settle;

/// How a submission signs in.
///
/// What its failure asks for follows: a sending password of its own that the server refuses is
/// the row's trouble ([`super::tells_the_pass`]); the account's own, as JMAP's, is the pass's
/// too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Login {
    /// With a credential of its own: SMTP's password, Graph's sending token.
    Apart,
    /// With the account's: JMAP signs in once for receiving and sending.
    Shared,
}

/// What came of the attempt.
#[derive(Debug)]
pub enum Outcome<'a> {
    /// The server did what was asked.
    Accepted,
    /// It did not.
    Refused {
        error: &'a RuntimeError,
        /// For an operation sent in parts, the messages the server had already carried out
        /// before it refused the rest; none for the rest.
        done: Vec<MessageId>,
        /// What to say of a refusal that came after `done`.
        said: String,
    },
}

/// What a draft is to be marked as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// Accepted by the server, at this time. Its stored copy is filled in by the drain that kept
    /// one.
    Sent {
        at: DateTime<Utc>,
    },
    Failed {
        reason: String,
        retry: Retry,
    },
}

impl Mark {
    /// The state it stands for, `kept` being the copy of what was sent that the drain stored.
    pub fn state(self, kept: Option<MessageId>) -> SendState {
        match self {
            Mark::Sent { at } => SendState::Sent { at, message: kept },
            Mark::Failed { reason, retry } => SendState::Failed { reason, retry },
        }
    }
}

/// What the drain does, in the order [`Settled::then`] lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Then {
    /// Settle the row so. After what a refusal records of the draft, so
    /// a drain cut off between the two leaves a row to try again and not a draft that says
    /// queued over a row that is gone; before what an acceptance leaves, so that one cut off
    /// there is not sent twice.
    SettleRow(Settle),
    /// Keep the message an upload put on the server.
    KeepUpload,
    /// Keep the copy of what was sent, where the account keeps it; its message goes in the mark
    /// that follows.
    KeepSentCopy,
    /// Record where the draft got to.
    Mark(DraftId, Mark),
    /// Queue the removal of the draft's copy on the server.
    DropDraftCopies(DraftId),
}

/// An attempt, settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    /// What is done, in order.
    pub then: Vec<Then>,
    /// What the pass hears of: a sign-in asked for, a wait.
    pub heard: Option<Retry>,
    /// What is said to the person, in order.
    pub notes: Vec<String>,
    /// A message was handed to the server and accepted.
    pub submitted: bool,
    /// The row is done with: counted as settled.
    pub settled: bool,
    /// The drain stops here, so nothing behind this overtakes it.
    pub halt: bool,
}

/// How `op`'s `outcome` settles, signed in as `login` at `now`.
pub fn settle(op: &ProtoOp, login: Login, outcome: Outcome<'_>, now: DateTime<Utc>) -> Settled {
    let draft = match op {
        ProtoOp::Submit { draft, .. } => Some(*draft),
        _ => None,
    };
    match outcome {
        Outcome::Accepted => Settled {
            then: match (op, draft) {
                (_, Some(draft)) => vec![
                    Then::SettleRow(Settle::Ok),
                    Then::KeepSentCopy,
                    Then::Mark(draft, Mark::Sent { at: now }),
                    Then::DropDraftCopies(draft),
                ],
                (ProtoOp::Append { .. }, None) => {
                    vec![Then::SettleRow(Settle::Ok), Then::KeepUpload]
                }
                _ => vec![Then::SettleRow(Settle::Ok)],
            },
            heard: None,
            notes: Vec::new(),
            submitted: draft.is_some(),
            settled: true,
            halt: false,
        },
        // Not a refusal: the person closed the app or switched accounts. Nothing is settled, and
        // the row goes again; `Retry::Fatal("cancelled")` would drop it.
        Outcome::Refused {
            error: RuntimeError::Cancelled,
            ..
        } => Settled {
            then: Vec::new(),
            heard: None,
            notes: Vec::new(),
            submitted: false,
            settled: false,
            halt: true,
        },
        Outcome::Refused { error, done, said } => {
            let tells = login == Login::Shared || super::tells_the_pass(op);
            let retry = match tells {
                true => error.retry(),
                false => super::settles(error),
            };
            let heard = tells.then(|| retry.clone());
            // Refused for good after part of it was done: only the rest is undone.
            let done = match retry {
                Retry::Fatal(_) => done,
                _ => Vec::new(),
            };
            if !done.is_empty() {
                return Settled {
                    then: vec![Then::SettleRow(Settle::InPart { done })],
                    heard,
                    notes: vec![said],
                    submitted: false,
                    settled: false,
                    halt: true,
                };
            }
            // Said as the error is, whatever the row does with it: a send the server refused
            // for good, or refused the protocol for, is worth a note.
            let notes =
                if (tells && retry.needs_person()) || matches!(error.retry(), Retry::Fatal(_)) {
                    vec![error.to_string()]
                } else {
                    Vec::new()
                };
            Settled {
                then: draft
                    .map(|draft| {
                        // Carries the retry, so the composer can say "retrying" rather than
                        // "failed" for something the outbox has not given up on.
                        Then::Mark(
                            draft,
                            Mark::Failed {
                                reason: error.to_string(),
                                retry: retry.clone(),
                            },
                        )
                    })
                    .into_iter()
                    .chain([Then::SettleRow(Settle::Failed {
                        reason: error.to_string(),
                        retry,
                    })])
                    .collect(),
                heard,
                notes,
                submitted: false,
                settled: false,
                halt: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sending::PROTOCOL_REFUSED_WAIT;
    use mail_domain::{BlobId, DeliveryReport, ProtoOp, RemoteRef};
    use mail_proto::{ProtoError, Refusal};
    use std::time::Duration;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_000_000, 0).unwrap()
    }

    fn submit() -> (ProtoOp, DraftId) {
        let draft = DraftId::generate();
        let op = ProtoOp::Submit {
            draft,
            raw: BlobId::generate(),
            mail_from: "me@example.test".to_owned(),
            rcpt_to: vec!["bea@example.test".to_owned()],
            delivery: DeliveryReport::Unrequested,
        };
        (op, draft)
    }

    fn append() -> ProtoOp {
        ProtoOp::Append {
            mailbox: mail_domain::MailboxRef {
                account: porter_core::AccountId::parse("acct-a").unwrap(),
                path: "Archive".to_owned(),
            },
            flags: Vec::new(),
            date: None,
            raw: BlobId::generate(),
        }
    }

    fn flags() -> ProtoOp {
        ProtoOp::SetFlags {
            remotes: Vec::<RemoteRef>::new(),
            read: None,
            star: None,
        }
    }

    fn refused(error: &RuntimeError) -> Outcome<'_> {
        Outcome::Refused {
            error,
            done: Vec::new(),
            said: String::new(),
        }
    }

    fn proto(kind: Refusal, text: &str) -> RuntimeError {
        RuntimeError::Proto(ProtoError::Refused {
            kind,
            text: text.to_owned(),
        })
    }

    fn row(settled: &Settled) -> &Settle {
        settled
            .then
            .iter()
            .find_map(|step| match step {
                Then::SettleRow(row) => Some(row),
                _ => None,
            })
            .expect("a settled attempt settles its row")
    }

    #[test]
    fn what_the_server_accepted_settles_its_row_once_and_then_keeps_what_follows_from_it() {
        let (op, draft) = submit();
        let sent = settle(&op, Login::Apart, Outcome::Accepted, now());
        assert_eq!(
            sent.then,
            [
                Then::SettleRow(Settle::Ok),
                Then::KeepSentCopy,
                Then::Mark(draft, Mark::Sent { at: now() }),
                Then::DropDraftCopies(draft),
            ]
        );
        assert!(sent.submitted && sent.settled && !sent.halt);
        assert_eq!((sent.heard, sent.notes), (None, Vec::new()));

        let uploaded = settle(&append(), Login::Apart, Outcome::Accepted, now());
        assert_eq!(
            uploaded.then,
            [Then::SettleRow(Settle::Ok), Then::KeepUpload]
        );
        assert!(!uploaded.submitted && uploaded.settled);

        let other = settle(&flags(), Login::Apart, Outcome::Accepted, now());
        assert_eq!(other.then, [Then::SettleRow(Settle::Ok)]);
    }

    /// A refusal records the draft before it settles the row, so a drain cut off between the two
    /// leaves a row to try again rather than a draft that says queued over a row that is gone.
    #[test]
    fn a_refused_submission_marks_its_draft_and_then_settles_its_row_and_stops_the_drain() {
        let (op, draft) = submit();
        let wait = proto(Refusal::Transient, "451 try later");
        let settled = settle(&op, Login::Apart, refused(&wait), now());
        let retry = Retry::After(Duration::from_secs(60));
        assert_eq!(
            settled.then,
            [
                Then::Mark(
                    draft,
                    Mark::Failed {
                        reason: wait.to_string(),
                        retry: retry.clone()
                    }
                ),
                Then::SettleRow(Settle::Failed {
                    reason: wait.to_string(),
                    retry
                }),
            ]
        );
        assert!(settled.halt && !settled.submitted && !settled.settled);
        // A wait on the submission's own credential is the row's, not the pass's; on the
        // account's it is both.
        assert_eq!((&settled.heard, &settled.notes), (&None, &Vec::new()));
        let shared = settle(&op, Login::Shared, refused(&wait), now());
        assert_eq!(shared.heard, Some(Retry::After(Duration::from_secs(60))));
        assert!(shared.notes.is_empty());
    }

    #[test]
    fn a_refused_sending_password_is_the_rows_and_a_refused_account_password_is_the_passes() {
        let (op, _) = submit();
        let error = RuntimeError::Proto(ProtoError::AuthRejected("535".to_owned()));
        let apart = settle(&op, Login::Apart, refused(&error), now());
        assert!(matches!(
            row(&apart),
            Settle::Failed {
                retry: Retry::NeedsReauth,
                ..
            }
        ));
        assert_eq!((apart.heard, apart.notes), (None, Vec::new()));
        let shared = settle(&op, Login::Shared, refused(&error), now());
        assert_eq!(shared.heard, Some(Retry::NeedsReauth));
        assert_eq!(shared.notes, [error.to_string()]);
        // Anything but a submission is the pass's whichever way the send signs in.
        let flagged = settle(&flags(), Login::Apart, refused(&error), now());
        assert_eq!(flagged.heard, Some(Retry::NeedsReauth));
        assert_eq!(flagged.then.len(), 1, "no draft to mark");
    }

    #[test]
    fn a_refusal_for_good_drops_the_row_with_its_reason_and_says_it() {
        let (op, draft) = submit();
        let error = proto(Refusal::Permanent, "550 no");
        for login in [Login::Apart, Login::Shared] {
            let settled = settle(&op, login, refused(&error), now());
            let Settle::Failed {
                retry: Retry::Fatal(_),
                ..
            } = row(&settled)
            else {
                panic!("{settled:?}");
            };
            assert_eq!(settled.notes, [error.to_string()]);
            assert!(
                matches!(&settled.then[0], Then::Mark(d, Mark::Failed { retry: Retry::Fatal(_), .. }) if *d == draft)
            );
            assert_eq!(settled.heard.is_some(), login == Login::Shared);
        }
    }

    /// A server that refuses the protocol itself keeps the message and waits, but says so.
    #[test]
    fn a_server_that_refuses_the_protocol_keeps_the_message_and_says_it() {
        let (op, _) = submit();
        let error = RuntimeError::RefusedFresh(Box::new(RuntimeError::NoServer("send from")));
        let settled = settle(&op, Login::Apart, refused(&error), now());
        assert!(matches!(
            row(&settled),
            Settle::Failed { retry: Retry::After(wait), .. } if *wait == PROTOCOL_REFUSED_WAIT
        ));
        assert_eq!(settled.notes, [error.to_string()]);
    }

    #[test]
    fn an_operation_refused_after_part_of_it_was_done_undoes_only_the_rest() {
        let op = flags();
        let error = proto(Refusal::Permanent, "NO");
        let kept = vec![mail_domain::MessageId::generate()];
        let part = |error| Outcome::Refused {
            error,
            done: kept.clone(),
            said: "done in part".to_owned(),
        };
        let settled = settle(&op, Login::Apart, part(&error), now());
        assert_eq!(
            settled.then,
            [Then::SettleRow(Settle::InPart { done: kept.clone() })]
        );
        assert_eq!(settled.notes, ["done in part"]);
        assert!(settled.halt);
        // A wait is not a refusal: what was done is the server's and the rest goes again.
        let wait = proto(Refusal::Transient, "try later");
        let settled = settle(&op, Login::Apart, part(&wait), now());
        assert!(matches!(row(&settled), Settle::Failed { .. }));
        assert!(settled.notes.is_empty());
    }

    /// The person closing the app is not a refusal: `Retry::Fatal("cancelled")` would drop a
    /// message that was never sent.
    #[test]
    fn a_cancelled_attempt_settles_nothing() {
        let (op, _) = submit();
        let settled = settle(&op, Login::Apart, refused(&RuntimeError::Cancelled), now());
        assert!(settled.then.is_empty() && settled.halt && settled.notes.is_empty());
    }
}
