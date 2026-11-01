//! [`Store::commit`]: one decision, written in one transaction.

use super::SqliteStore;
use crate::StoreError;
use mail_domain::commit::Written;
use mail_domain::{Commit, OutboxId, Resolve, Row, Unplaced};
use rusqlite::{Transaction, TransactionBehavior};

impl SqliteStore {
    pub(super) fn commit_whole(&self, commit: &Commit) -> Result<Written, StoreError> {
        let db = self.connection();
        let tx = Transaction::new_unchecked(&db, TransactionBehavior::Immediate)?;
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
        tx.commit()?;
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
