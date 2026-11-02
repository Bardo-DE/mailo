//! [`Store::commit`]: one decision, written in one transaction.

use super::SqliteStore;
use crate::StoreError;
use chrono::{DateTime, Utc};
use mail_domain::commit::Written;
use mail_domain::{Commit, Expect, OutboxId, Resolve, Row, Unplaced};
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
                    // The commit failed and that is the error returned; undoing it further is best effort.
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
        // Before anything is written: what it was decided over is still what is there.
        for row in &commit.rows {
            if let Row::Expect(expect) = row {
                self.expecting(expect)?;
            }
        }
        // A deletion is addressed while the store still knows where its messages are.
        self.queue_all(commit, Resolve::BeforeForward, &mut queued)?;
        self.write_patch_in(&commit.forward, commit.at)?;
        self.queue_all(commit, Resolve::AfterForward, &mut queued)?;
        let mut keys_changed = false;
        for row in &commit.rows {
            keys_changed |= self.write_row(row, commit.at)?;
        }
        Ok(Written {
            queued,
            keys_changed,
        })
    }

    /// Write one [`Row`] in the transaction the caller holds, at `at`. Whether it changed the
    /// keys or certificates held.
    pub(super) fn write_row(&self, row: &Row, at: DateTime<Utc>) -> Result<bool, StoreError> {
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
                self.withdraw_in(*forward, inverse, at)?;
            }
            Row::SendState { draft, state, at } => {
                self.set_send_state(*draft, state, *at)?;
            }
            Row::FollowUpHeld {
                draft,
                account,
                message_id,
                thread,
                due,
                set,
            } => {
                self.hold_follow_up(&crate::FollowUpHold {
                    draft: *draft,
                    account: account.clone(),
                    message_id: message_id.clone(),
                    thread: *thread,
                    at: *due,
                    set: *set,
                })?;
            }
            Row::FollowUpReleased { draft } => {
                self.release_follow_up(*draft)?;
            }
            Row::KeyLearned(taught) => {
                return crate::learned::learn(&mut Through(self), taught);
            }
            Row::Expect(_) => {}
        }
        Ok(false)
    }

    /// Whether `expect` still holds, or the commit is stale.
    fn expecting(&self, expect: &Expect) -> Result<(), StoreError> {
        match expect {
            Expect::Draft {
                draft,
                updated,
                state,
            } => match self.load_draft(*draft) {
                Ok(now) if now.updated == *updated && now.state == *state => Ok(()),
                Ok(_) | Err(StoreError::NoDraft(_)) => Err(StoreError::Stale),
                Err(other) => Err(other),
            },
        }
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

/// The key rows of a store, read and written inside the transaction the caller holds.
struct Through<'a>(&'a SqliteStore);

impl crate::learned::Held for Through<'_> {
    fn autocrypt_peer(
        &mut self,
        address: &str,
    ) -> Result<Option<mail_domain::AutocryptPeer>, StoreError> {
        self.0.load_autocrypt_peer(address)
    }

    fn put_autocrypt_peer(&mut self, peer: &mail_domain::AutocryptPeer) -> Result<(), StoreError> {
        self.0.write_autocrypt_peer(peer)
    }

    fn pgp_key(
        &mut self,
        fingerprint: mail_domain::Fingerprint,
    ) -> Result<Option<mail_domain::PgpKey>, StoreError> {
        self.0.load_pgp_key(fingerprint)
    }

    fn put_pgp_key(&mut self, key: mail_domain::PgpKey) -> Result<mail_domain::PgpKey, StoreError> {
        self.0.write_pgp_key_in(key)
    }

    fn smime_cert(
        &mut self,
        fingerprint: mail_domain::CertFingerprint,
    ) -> Result<Option<mail_domain::SmimeCert>, StoreError> {
        self.0.load_smime_cert(fingerprint)
    }

    fn put_smime_cert(
        &mut self,
        cert: mail_domain::SmimeCert,
    ) -> Result<mail_domain::SmimeCert, StoreError> {
        self.0.write_smime_cert_in(cert)
    }
}
