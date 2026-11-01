//! [`Store::commit`]: one decision, written in one transaction.

use super::SqliteStore;
use crate::StoreError;
use mail_domain::commit::Written;
use mail_domain::{Commit, OutboxId, Resolve, Row, Unplaced};
use rusqlite::{Transaction, TransactionBehavior};

impl SqliteStore {
    pub(super) fn commit_whole(&self, commit: &Commit) -> Result<Written, StoreError> {
        let db = self.connection();
        if db.is_autocommit() {
            let tx = Transaction::new_unchecked(&db, TransactionBehavior::Immediate)?;
            let written = self.commit_steps(commit)?;
            tx.commit()?;
            return Ok(written);
        }
        // Inside [`Self::within`], whose transaction is not this commit's to end: what this one
        // wrote is taken back alone should it fail.
        db.execute_batch("SAVEPOINT act_commit")?;
        match self.commit_steps(commit) {
            Ok(written) => {
                db.execute_batch("RELEASE act_commit")?;
                Ok(written)
            }
            Err(error) => {
                db.execute_batch("ROLLBACK TO act_commit; RELEASE act_commit")
                    .ok();
                Err(error)
            }
        }
    }

    /// Make a decision, then write it, with nothing else held off: all the reads of
    /// `decide` and the writes of the commit it ends in see no write between them.
    ///
    /// The writer is held, which keeps out every other writer in this process, and the database
    /// is locked for writing from the first read, which keeps out another process's. A commit
    /// made inside is part of this transaction, taken back alone should it fail.
    pub fn within<T>(&self, decide: impl FnOnce() -> T) -> Result<T, StoreError> {
        let db = self.connection();
        if !db.is_autocommit() {
            return Ok(decide());
        }
        let tx = Transaction::new_unchecked(&db, TransactionBehavior::Immediate)?;
        let out = decide();
        tx.commit()?;
        Ok(out)
    }

    fn commit_steps(&self, commit: &Commit) -> Result<Written, StoreError> {
        let mut queued: Vec<Option<OutboxId>> = vec![None; commit.queue.len()];
        // A deletion is addressed while the store still knows where its messages are.
        self.queue_all(commit, Resolve::BeforeForward, &mut queued)?;
        self.write_patch_in(&commit.forward)?;
        self.queue_all(commit, Resolve::AfterForward, &mut queued)?;
        for row in &commit.rows {
            match row {
                Row::JunkTaught {
                    account,
                    message,
                    words,
                    judged,
                } => {
                    self.junk_learn_in(account.clone(), *message, words, *judged)?;
                }
                Row::JunkForgotten {
                    account,
                    message,
                    judged,
                } => {
                    self.junk_forget_in(account.clone(), *message, *judged)?;
                }
                Row::Withdrawn { forward, inverse } => {
                    self.withdraw_in(*forward, inverse, commit.at)?;
                }
            }
        }
        Ok(Written { queued })
    }

    fn queue_all(
        &self,
        commit: &Commit,
        when: Resolve,
        queued: &mut [Option<OutboxId>],
    ) -> Result<(), StoreError> {
        for (at, queue) in commit.queue.iter().enumerate() {
            if queue.resolve != when {
                continue;
            }
            let id = self.queue_in(
                commit.account.clone(),
                queue.intent.clone(),
                &queue.undo,
                queue.id,
                commit.at,
            )?;
            if id.is_none() && queue.unplaced == Unplaced::Rollback {
                return Err(StoreError::NotQueued);
            }
            queued[at] = id;
        }
        Ok(())
    }
}
