//! Pressing send as a decision: what a draft freezes into, and everything that is written for it.
//!
//! A send is three steps, each of which can fail without changing anything:
//!
//! 1. [`crate::gather::sending`] reads the draft, the identity it sends as, the message it answers
//!    and its attachments' bytes into a [`Snapshot`]. Reads only.
//! 2. [`keys`] finds the keys the draft asks for and unlocks the one that signs: the keyring and
//!    the passphrase prompt are used here and nowhere else.
//! 3. [`freeze`] builds the bytes, protects them and says what to write as a [`Commit`]. A
//!    function of its arguments: the time and the ids are handed in.
//!
//! The caller then puts the bytes in the blob store (content-addressed, so a send that is tried
//! again leaves nothing new behind) and writes the commit, which is refused as stale if the draft
//! was edited, queued, or settled since it was read. Everything the send changes is in that one
//! commit: the prior submission pulled back, the new one queued, the draft's state, and the
//! reminder held for it. A failure at any step before it leaves the draft and its earlier
//! submission as they were.

use super::Leaves;
use crate::error::CoreError;
use crate::pgp::{Ask, OpenPgp};
use crate::protect::{Keyed, Seal};
use crate::smime::Smime;
use chrono::{DateTime, Utc};
use mail_domain::{
    BlobId, Change, ChangeId, Commit, Draft, Expect, Identity, Message, Patch, Queue, RemoteIntent,
    Resolve, Row, SendState, ThreadId, Unplaced,
};
use mail_mime::{Posting, posting};
use mail_secrets::SigningStore;
use mail_store::SqliteStore;

/// What a send is decided over.
#[derive(Debug)]
pub struct Snapshot {
    /// The draft as it was read, whatever state it was in.
    pub draft: Draft,
    /// The identity it sends as.
    pub identity: Identity,
    /// The message it answers, when that is still held.
    pub parent: Option<Message>,
    /// Each attachment's bytes, with pictures already made as small as the person asked.
    pub parts: Vec<(BlobId, Vec<u8>)>,
}

/// The ids a send draws: it names the pull-back of the earlier submission and the new one's
/// (empty) undo.
#[derive(Debug, Clone, Copy)]
pub struct Ids {
    pub pulled_back: ChangeId,
    pub queued: ChangeId,
}

impl Ids {
    /// Fresh ones. Drawn at the edge, not in a decision.
    pub fn generate() -> Self {
        Self {
            pulled_back: ChangeId::generate(),
            queued: ChangeId::generate(),
        }
    }
}

/// Whether a draft in this state may be sent at all, and from an account that has (or has not) a
/// server. The first thing the send decides, before anything else is read, so a draft that cannot
/// be sent says so before its identity or attachments are looked for.
pub fn admits(draft: &Draft, has_server: bool) -> Result<(), CoreError> {
    match draft.state {
        SendState::Sent { at, .. } => return Err(CoreError::DraftSentAt { at }),
        SendState::Sending => return Err(CoreError::AlreadySending),
        SendState::Queued
        | SendState::Scheduled { .. }
        | SendState::Failed { .. }
        | SendState::Editing => {}
    }
    // Refused before anything is built or queued: an entry in the outbox of an account with no
    // server would sit there for ever, and the draft would say "queued" the whole time.
    if !has_server {
        return Err(mail_runtime::RuntimeError::NoServer("send from").into());
    }
    Ok(())
}

/// The protection the draft asks, found and unlocked.
#[derive(Debug)]
pub struct Keys {
    pgp: Keyed<OpenPgp>,
    smime: Keyed<Smime>,
}

impl Keys {
    /// Protecting nothing and adding nothing: for a draft that asks for nothing from a scheme and
    /// an identity with no key to speak of.
    pub fn idle() -> Self {
        Keys {
            pgp: Keyed::idle(),
            smime: Keyed::idle(),
        }
    }
}

/// The keys `snapshot`'s draft asks for, with the signing secret unlocked: `ask` is asked for the
/// passphrase of one that has it.
///
/// Every refusal is here, before the draft is touched: both schemes asked at once, no key of the
/// sender's own, a recipient with nothing to be encrypted to, blind recipients under encryption.
pub fn keys(
    store: &SqliteStore,
    secrets: &dyn SigningStore,
    ask: Ask<'_>,
    snapshot: &Snapshot,
    now: DateTime<Utc>,
) -> Result<Keys, CoreError> {
    let Snapshot {
        draft, identity, ..
    } = snapshot;
    // S/MIME first, as asking both is refused by it before OpenPGP seals anything.
    let smime = Smime::keyed(store, secrets, ask, draft, identity, now)?;
    let pgp = OpenPgp::keyed(store, secrets, ask, draft, identity, now)?;
    Ok(Keys { pgp, smime })
}

/// What a draft is frozen into.
#[derive(Debug)]
pub struct Frozen {
    /// The bytes and the envelope.
    pub post: Posting,
    /// The draft as it is once queued.
    pub draft: Draft,
    seen: Expect,
    /// The draft as it comes back when an earlier submission of it is taken back first.
    pulled_back: Option<Draft>,
    ids: Ids,
    state: SendState,
    leaves: DateTime<Utc>,
    now: DateTime<Utc>,
    held: Option<Held>,
}

/// A reminder to hold until the message has left.
#[derive(Debug)]
struct Held {
    message_id: String,
    thread: Option<ThreadId>,
    due: DateTime<Utc>,
}

/// `draft` frozen: the bytes of the message it is, protected as it asks with `keys`, and the
/// commit that queues them to leave as `leaves` says.
///
/// A draft already queued, scheduled or failing is taken back in the same commit and queued
/// afresh, so pressing Send twice, or changing the time of a scheduled send, leaves one
/// submission in the outbox rather than two, which the recipient would have received twice.
/// `remind` is held as the message's follow-up reminder until it has left; with none, one an
/// earlier send of the draft held goes.
pub fn freeze(
    snapshot: &Snapshot,
    keys: Keys,
    leaves: Leaves,
    remind: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    ids: Ids,
) -> Result<Frozen, CoreError> {
    let Snapshot {
        draft: read,
        identity,
        parent,
        parts,
    } = snapshot;
    let seen = Expect::Draft {
        draft: read.id,
        updated: read.updated,
        state: read.state.clone(),
    };
    let pulled = matches!(
        read.state,
        SendState::Queued | SendState::Scheduled { .. } | SendState::Failed { .. }
    );
    let draft = match pulled {
        true => Draft {
            state: SendState::Editing,
            updated: now,
            ..read.clone()
        },
        false => read.clone(),
    };
    let mut post = posting(&draft, identity, parent.as_ref(), parts)?;
    post.message = keys.pgp.seal(post.message, now)?;
    post.message = keys.smime.seal(post.message, now)?;
    let (state, leaves) = match leaves {
        Leaves::Now => (SendState::Queued, now),
        Leaves::At(at) => (SendState::Scheduled { at }, at),
    };
    let held = remind.and_then(|due| {
        // A message with no `Message-ID` cannot be waited on, and the send goes without.
        let message_id = mail_mime::parse(&post.message).ok()?.rfc_message_id?;
        Some(Held {
            message_id,
            thread: parent.as_ref().map(|parent| parent.thread),
            due,
        })
    });
    Ok(Frozen {
        post,
        draft: Draft {
            state: state.clone(),
            updated: now,
            ..draft.clone()
        },
        seen,
        pulled_back: pulled.then_some(draft),
        ids,
        state,
        leaves,
        now,
        held,
    })
}

/// What to write to send a message that is not a draft's: a redirect, an answer to an
/// invitation, a receipt, which have no draft behind them and nothing to take back. `id` is the
/// name the outbox keys the submission by, and `post` the bytes, already in the blob store as
/// `raw`. The message leaves at `leaves`, and a commit that places nothing is refused whole.
pub fn unattached(
    account: porter_core::AccountId,
    id: mail_domain::DraftId,
    (post, raw): (&Posting, BlobId),
    leaves: DateTime<Utc>,
    ids: Ids,
) -> Commit {
    Commit {
        account,
        at: leaves,
        forward: Patch {
            id: ids.pulled_back,
            changes: Vec::new(),
        },
        queue: vec![Queue {
            intent: RemoteIntent::Send {
                draft: id,
                raw,
                mail_from: post.mail_from.clone(),
                rcpt_to: post.rcpt_to.clone(),
                delivery: post.delivery,
            },
            // Nothing to undo: the outbox cannot take back bytes once they are on the wire.
            undo: Patch {
                id: ids.queued,
                changes: Vec::new(),
            },
            id: None,
            resolve: Resolve::AfterForward,
            unplaced: Unplaced::Rollback,
        }],
        rows: Vec::new(),
    }
}

impl Frozen {
    /// What to write once the bytes are in the blob store as `raw`.
    pub fn commit(&self, raw: BlobId) -> Commit {
        let draft = self.draft.id;
        let mut rows = vec![
            Row::Expect(self.seen.clone()),
            Row::FollowUpReleased { draft },
            Row::SendState {
                draft,
                state: self.state.clone(),
                at: self.now,
            },
        ];
        if let Some(Held {
            message_id,
            thread,
            due,
        }) = &self.held
        {
            rows.push(Row::FollowUpHeld {
                draft,
                account: self.draft.account.clone(),
                message_id: message_id.clone(),
                thread: *thread,
                due: *due,
                set: self.leaves,
            });
        }
        Commit {
            account: self.draft.account.clone(),
            // The entry's `next_attempt`, which is the whole of what holds a scheduled send:
            // the outbox does not hand out an entry before it.
            at: self.leaves,
            forward: Patch {
                id: self.ids.pulled_back,
                // The earlier submission goes with the draft that queued it, and the same draft
                // comes back as it was written: what returns is exactly what was sent.
                changes: self
                    .pulled_back
                    .iter()
                    .flat_map(|back| {
                        [
                            Change::DraftDelete(draft),
                            Change::DraftUpsert(Box::new(back.clone())),
                        ]
                    })
                    .collect(),
            },
            queue: vec![Queue {
                intent: RemoteIntent::Send {
                    draft,
                    raw,
                    mail_from: self.post.mail_from.clone(),
                    rcpt_to: self.post.rcpt_to.clone(),
                    delivery: self.post.delivery,
                },
                // No undo. Unsending is not something the outbox can offer once the bytes are
                // on the wire, and a patch that pretended otherwise would revert something real.
                undo: Patch {
                    id: self.ids.queued,
                    changes: Vec::new(),
                },
                id: None,
                // After the pull-back, which withdraws the earlier entry with its draft.
                resolve: Resolve::AfterForward,
                // A send that names nothing is refused whole: no draft saying queued over an
                // outbox with nothing in it.
                unplaced: Unplaced::Rollback,
            }],
            rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use mail_domain::{Address, DraftId, IdentityId, IsDefault, ReceiptRequest};

    fn at(n: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + n, 0).unwrap()
    }

    fn account() -> porter_core::AccountId {
        mail_domain::id::account_id_from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000a1"))
    }

    fn identity() -> Identity {
        Identity {
            id: IdentityId::from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000b1")),
            account: account(),
            from: Address {
                name: Some("Me".to_owned()),
                email: "me@example.test".to_owned(),
            },
            reply_to: None,
            signature: None,
            default: IsDefault::Default,
        }
    }

    /// A draft to Bea written at second 0, in `state`.
    fn draft(state: SendState) -> Draft {
        Draft {
            id: DraftId::from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000d1")),
            account: account(),
            identity: identity().id,
            to: vec![Address::new("bea@example.test")],
            cc: vec![],
            bcc: vec![Address::new("dee@example.test")],
            subject: "lunch on friday".to_owned(),
            in_reply_to: None,
            forward_of: None,
            text: "Shall we say one o'clock?".to_owned(),
            html: None,
            attachments: vec![],
            receipt: ReceiptRequest::Unrequested,
            openpgp: None,
            smime: None,
            priority: mail_domain::Priority::Normal,
            delivery: mail_domain::DeliveryReport::Unrequested,
            automatic: false,
            state,
            updated: at(0),
        }
    }

    fn snapshot(draft: Draft) -> Snapshot {
        Snapshot {
            draft,
            identity: identity(),
            parent: None,
            parts: vec![],
        }
    }

    fn ids() -> Ids {
        Ids {
            pulled_back: ChangeId::from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000e1")),
            queued: ChangeId::from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000e2")),
        }
    }

    fn raw() -> BlobId {
        BlobId::from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000f1"))
    }

    /// The message and its envelope, with fixed ids and times, are the same every time, and the
    /// blind recipient is in the envelope and nowhere else.
    #[test]
    fn a_draft_frozen_is_the_same_bytes_and_the_same_commit_every_time() {
        let snapshot = snapshot(draft(SendState::Editing));
        let once = freeze(&snapshot, Keys::idle(), Leaves::Now, None, at(10), ids()).unwrap();
        let again = freeze(&snapshot, Keys::idle(), Leaves::Now, None, at(10), ids()).unwrap();
        assert_eq!(once.post, again.post);
        assert_eq!(once.commit(raw()), again.commit(raw()));

        let text = String::from_utf8(once.post.message.clone()).unwrap();
        assert!(text.contains("Subject: lunch on friday\r\n"), "{text}");
        assert!(text.contains("Message-ID: <"), "{text}");
        assert!(
            !text.contains("dee@example.test"),
            "the blind one is hidden: {text}"
        );
        assert_eq!(
            once.post.rcpt_to,
            ["bea@example.test", "dee@example.test"],
            "and named in the envelope"
        );
        assert_eq!(once.post.mail_from, "me@example.test");
    }

    /// Everything a first send writes, and nothing else: the submission, to leave now, with no
    /// undo; the draft's state; the reminder released; and what it was decided over.
    #[test]
    fn a_first_send_queues_one_submission_and_sets_the_draft_queued() {
        let read = draft(SendState::Editing);
        let frozen = freeze(
            &snapshot(read.clone()),
            Keys::idle(),
            Leaves::Now,
            None,
            at(10),
            ids(),
        )
        .unwrap();
        let commit = frozen.commit(raw());
        assert_eq!(commit.at, at(10));
        assert!(commit.forward.changes.is_empty(), "nothing to take back");
        assert_eq!(
            commit.queue,
            [Queue {
                intent: RemoteIntent::Send {
                    draft: read.id,
                    raw: raw(),
                    mail_from: "me@example.test".to_owned(),
                    rcpt_to: vec!["bea@example.test".to_owned(), "dee@example.test".to_owned()],
                    delivery: mail_domain::DeliveryReport::Unrequested,
                },
                undo: Patch {
                    id: ids().queued,
                    changes: vec![]
                },
                id: None,
                resolve: Resolve::AfterForward,
                unplaced: Unplaced::Rollback,
            }]
        );
        assert_eq!(
            commit.rows,
            [
                Row::Expect(Expect::Draft {
                    draft: read.id,
                    updated: at(0),
                    state: SendState::Editing
                }),
                Row::FollowUpReleased { draft: read.id },
                Row::SendState {
                    draft: read.id,
                    state: SendState::Queued,
                    at: at(10)
                },
            ]
        );
        assert_eq!(frozen.draft.state, SendState::Queued);
        assert_eq!(frozen.draft.updated, at(10));
    }

    /// A send held for later is queued to leave then, and the draft says when.
    #[test]
    fn a_scheduled_send_leaves_at_its_time_and_the_draft_says_when() {
        let frozen = freeze(
            &snapshot(draft(SendState::Editing)),
            Keys::idle(),
            Leaves::At(at(3_600)),
            None,
            at(10),
            ids(),
        )
        .unwrap();
        let commit = frozen.commit(raw());
        assert_eq!(commit.at, at(3_600));
        assert!(commit.rows.contains(&Row::SendState {
            draft: frozen.draft.id,
            state: SendState::Scheduled { at: at(3_600) },
            at: at(10),
        }));
    }

    /// A draft already queued, scheduled or failing is taken back in the same commit, and is
    /// written as it was before it was sent; the message is dated as it is written again.
    #[test]
    fn a_draft_sent_again_is_taken_back_in_the_same_commit() {
        for state in [
            SendState::Queued,
            SendState::Scheduled { at: at(500) },
            SendState::Failed {
                reason: "no".to_owned(),
                retry: mail_domain::Retry::Now,
            },
        ] {
            let read = draft(state.clone());
            let frozen = freeze(
                &snapshot(read.clone()),
                Keys::idle(),
                Leaves::Now,
                None,
                at(10),
                ids(),
            )
            .unwrap();
            let commit = frozen.commit(raw());
            assert_eq!(commit.forward.id, ids().pulled_back);
            assert_eq!(
                commit.forward.changes,
                [
                    Change::DraftDelete(read.id),
                    Change::DraftUpsert(Box::new(Draft {
                        state: SendState::Editing,
                        updated: at(10),
                        ..read.clone()
                    })),
                ],
                "{state:?}"
            );
            assert_eq!(
                commit.rows[0],
                Row::Expect(Expect::Draft {
                    draft: read.id,
                    updated: at(0),
                    state
                }),
                "decided over the draft as it was read"
            );
        }
    }

    #[test]
    fn a_reminder_is_held_with_the_message_id_it_goes_by() {
        let read = draft(SendState::Editing);
        let mut snap = snapshot(read.clone());
        let parent = {
            let mut parent = mail_domain::testing::message(7, "ada@example.test", "Hi", "x");
            parent.account = account();
            parent
        };
        let thread = parent.thread;
        snap.draft.in_reply_to = Some(parent.id);
        snap.parent = Some(parent);
        let frozen = freeze(
            &snap,
            Keys::idle(),
            Leaves::At(at(3_600)),
            Some(at(86_400)),
            at(10),
            ids(),
        )
        .unwrap();
        let sent = String::from_utf8(frozen.post.message.clone()).unwrap();
        let id = sent
            .lines()
            .find_map(|line| line.strip_prefix("Message-ID: <"))
            .and_then(|rest| rest.strip_suffix('>'))
            .unwrap();
        let commit = frozen.commit(raw());
        assert!(
            commit.rows.contains(&Row::FollowUpHeld {
                draft: read.id,
                account: account(),
                message_id: id.to_owned(),
                thread: Some(thread),
                due: at(86_400),
                set: at(3_600),
            }),
            "{:?}",
            commit.rows
        );
    }

    #[test]
    fn a_draft_that_cannot_be_sent_says_so_before_anything_is_read_for_it() {
        let sent = SendState::Sent {
            at: at(5),
            message: None,
        };
        assert!(matches!(
            admits(&draft(sent), true),
            Err(CoreError::DraftSentAt { .. })
        ));
        assert!(matches!(
            admits(&draft(SendState::Sending), true),
            Err(CoreError::AlreadySending)
        ));
        assert!(
            admits(&draft(SendState::Editing), false).is_err(),
            "no server"
        );
        for state in [SendState::Editing, SendState::Queued] {
            assert!(admits(&draft(state), true).is_ok());
        }
    }
}
