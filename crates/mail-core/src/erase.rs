//! Erasing Trash and Spam by themselves: after a day, a week or a month there, and, under
//! "Delete deletes it now", as soon as a deleted message has reached the server's Trash.
//!
//! Erasing is Delete forever ([`crate::act::destroy_messages`]), so the rule that keeps it safe
//! holds here too: only mail in Trash or Spam is ever erased, and [`mail_domain::Op::Destroy`]
//! refuses anything else. Delete itself still moves to Trash, with its Undo, under either
//! setting; "deletes it now" marks what it moved ([`mark_deleted`]), and the next sweep after the
//! move has reached the server erases it. A move still on its way, or one the server has not yet
//! said where it put, waits: a deletion sent to where the message was would find nothing, and the
//! message would come back.
//!
//! How long a message has been there is counted from when a sweep first found it there, not from
//! its date ([`mail_store::binned`]): a message from years ago deleted today has been in Trash for
//! no time at all. A message restored and deleted again starts again.

pub use mail_logic::erase::{Delete, Keep, Policy};

use crate::error::Logged;
use chrono::{DateTime, TimeDelta, Utc};
use mail_domain::{Change, MailboxRole, MessageId, ThreadId};
use mail_logic::undo::Undo;
use mail_store::SqliteStore;
use mail_store::binned::{Binned, Settled, Soon};
use porter_core::AccountId;
use std::collections::BTreeMap;

/// How long a message Delete sent to Trash to be erased at once is kept first, so Undo can still
/// bring it back.
pub const UNDO_GRACE: TimeDelta = TimeDelta::seconds(30);

/// Whether `binned` is erased now under `policy`. Pure, so the whole table is a test.
///
/// Only a message whose place the server has confirmed is erased, or one wholly here
/// (`local`: its account has no server). A message marked by Delete is erased at once only
/// while Delete still means "now" and only from Trash; otherwise it waits like any other.
pub fn due(policy: &Policy, binned: &Binned, local: bool, now: DateTime<Utc>) -> bool {
    let settled = match binned.settled {
        Settled::Yes => true,
        Settled::Unplaced => local,
        Settled::Pending => false,
    };
    if !settled {
        return false;
    }
    let keep = match binned.mailbox {
        MailboxRole::Trash => {
            if binned.soon == Soon::Asked && policy.delete == Delete::Now {
                // Not before the Undo the Delete offered has run out: erased at once, it would
                // leave Undo nothing to bring back.
                return now.signed_duration_since(binned.since) >= UNDO_GRACE;
            }
            policy.trash
        }
        MailboxRole::Spam => policy.spam,
        _ => return false,
    };
    keep.span()
        .is_some_and(|span| now.signed_duration_since(binned.since) >= span)
}

/// Erase what `policy` says is due, here and on the server (the deletion leaves with the next
/// pass). Returns the messages erased. Nothing is read or written when the policy is
/// [`Policy::idle`].
pub fn sweep(
    store: &SqliteStore,
    accounts: &mail_logic::accounts::Accounts,
    policy: &Policy,
    now: DateTime<Utc>,
) -> Vec<MessageId> {
    if policy.idle() {
        return Vec::new();
    }
    let Some(binned) = store
        .binned(now)
        .or_log("what is in Trash and Spam could not be read")
    else {
        return Vec::new();
    };
    let local: Vec<AccountId> = accounts.local();
    let mut by_thread: BTreeMap<ThreadId, Vec<MessageId>> = BTreeMap::new();
    for one in binned
        .iter()
        .filter(|one| due(policy, one, local.contains(&one.account), now))
    {
        by_thread.entry(one.thread).or_default().push(one.message);
    }
    by_thread
        .into_iter()
        .filter_map(|(thread, messages)| crate::act::destroy_messages(store, thread, messages))
        .flatten()
        .collect()
}

/// Mark what `undos` moved to Trash to be erased as soon as the server holds it there: Delete,
/// under [`Delete::Now`]. Anything else they did is left alone.
pub fn mark_deleted(store: &SqliteStore, undos: &[Undo], now: DateTime<Utc>) {
    let trashed: Vec<MessageId> = undos
        .iter()
        .flat_map(|undo| &undo.forward.changes)
        .filter_map(|change| match change {
            Change::MessageMailbox(message, MailboxRole::Trash) => Some(*message),
            _ => None,
        })
        .collect();
    if trashed.is_empty() {
        return;
    }
    store
        .erase_soon(&trashed, now)
        .or_log("what Delete moved could not be marked to erase");
}

#[cfg(test)]
mod tests;
