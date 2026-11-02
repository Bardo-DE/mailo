//! What one decision asks the store to write, as one value.
//!
//! A decision (what an act does to a conversation, what taking it back does) is a pure function
//! that ends in a [`Commit`]; `Store::commit` is the one place that writes it, in one
//! transaction, so a decision either happened whole or not at all.

use crate::id::{ChangeId, MessageId, OutboxId};
use crate::junk::Judged;
use crate::op::{Applied, Patch, RemoteIntent};
use chrono::{DateTime, Utc};
use porter_core::AccountId;

/// Names the changes one act made, so taking it back finds the queued work that act left: the id
/// of the patch it applied.
pub type UndoId = ChangeId;

/// What to write, in one transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub account: AccountId,
    /// When it was decided: the time queued work is stamped with.
    pub at: DateTime<Utc>,
    /// The local change.
    pub forward: Patch,
    /// The server's half: work to queue, each with where in the commit it is resolved.
    pub queue: Vec<Queue>,
    /// Other writes made in the same transaction.
    pub rows: Vec<Row>,
}

/// One piece of work to queue for the server.
#[derive(Debug, Clone, PartialEq)]
pub struct Queue {
    pub intent: RemoteIntent,
    /// What puts the local change back when the server refuses the work for good.
    pub undo: Patch,
    /// What names the queued work to whoever takes the act back: the id of the patch it was
    /// queued for.
    pub id: Option<UndoId>,
    pub resolve: Resolve,
    pub unplaced: Unplaced,
}

/// Where in a commit work is addressed to the server's messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolve {
    /// Before the local change is written. A deletion needs this: removing a message takes its
    /// server addresses with it, and they are kept for the queued work as it is queued.
    BeforeForward,
    /// After it. Everything else.
    AfterForward,
}

/// What to do with work that names nothing the server holds: every message it is about is local
/// to this computer, which is normal for mail composed here and not yet sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unplaced {
    /// Queue nothing and write the rest.
    Skip,
    /// Write nothing at all: the commit is refused.
    Rollback,
}

/// A write other than a patch or queued work, made inside the commit's transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// The junk filter learns `message` as `judged`, from the `words` it says.
    JunkTaught {
        account: AccountId,
        message: MessageId,
        words: Vec<String>,
        judged: Judged,
    },
    /// The junk filter forgets what `message` taught it as `judged`.
    JunkForgotten {
        account: AccountId,
        message: MessageId,
        judged: Judged,
    },
    /// Queued work the server has not been tried with is taken out of the outbox. It is the
    /// work queued for the act whose patch was `forward`; for work queued before acts were named,
    /// the entry whose undo is `inverse`. Work already tried, or not there, is left.
    Withdrawn { forward: UndoId, inverse: Patch },
}

/// What a commit wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Each [`Queue`] in order: the outbox entry made, or `None` where it named nothing the
    /// server holds and was skipped.
    pub queued: Vec<Option<OutboxId>>,
}

impl Applied {
    /// The commit of an operation on `account`: its patch, and its server half when
    /// `tells_server`.
    ///
    /// A deletion is addressed before its patch is written; everything else after. Work that names
    /// nothing the server holds is skipped, and the change is made here alone.
    pub fn into_commit(self, account: AccountId, at: DateTime<Utc>, tells_server: bool) -> Commit {
        let queue = self
            .remote
            .filter(|_| tells_server)
            .map(|intent| Queue {
                resolve: match intent {
                    RemoteIntent::Destroy { .. } => Resolve::BeforeForward,
                    _ => Resolve::AfterForward,
                },
                intent,
                undo: self.inverse.clone(),
                id: Some(self.forward.id),
                unplaced: Unplaced::Skip,
            })
            .into_iter()
            .collect();
        Commit {
            account,
            at,
            forward: self.forward,
            queue,
            rows: Vec::new(),
        }
    }
}
