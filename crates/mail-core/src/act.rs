//! What happens to a conversation when it is archived, starred, labelled, snoozed or moved, and
//! how that is taken back.
//!
//! One operation is applied locally and, when the account has a server, queued for it; what it
//! returns is the [`Undo`] that reverses both. The window (`mail_ui::ui`) and the intents
//! provider (`mail_intents`) are two front-ends over this, so a conversation archived by
//! either is archived the same way: the same patch, the same queued work for the server, the
//! same undo.

use crate::error::Logged;
use chrono::Utc;
use mail_domain::*;
use mail_logic::accounts::Accounts;
use mail_logic::act::{Act, ActSnapshot, TakeBackSnapshot, Waiting};
use mail_logic::undo::Undo;
use mail_store::{SqliteStore, Store};
use porter_core::AccountId;

/// Take back one operation: its inverse, written like any other patch, and the reverse queued
/// for the server when the operation had told it anything.
///
/// `false` when it could not be taken back, and then nothing changed: a reverse the server
/// cannot be told is not taken back here either, or the two would disagree until the next sync
/// put the operation back without a word.
///
/// Decided over what is waiting in the outbox ([`mail_logic::act::take_back`]) and written whole:
/// the inverse, the reverse, and a move or copy not yet sent taken out of the outbox by the name
/// of the act that queued it.
///
/// Nothing marks an entry as taken by a pass that is running, so there is a window this does not
/// close for a move a sync is already sending, the window's own or `mailo watch`'s: it still
/// reaches the server, and its confirmation then settles an entry that is no longer here. The
/// message is back here and filed there, and the next sync shows it filed.
pub fn take_back(store: &SqliteStore, entry: &Undo) -> bool {
    take_back_with(store, &crate::gather::acting_accounts(store), entry)
}

/// [`take_back`], over accounts already read.
pub fn take_back_with(store: &SqliteStore, accounts: &Accounts, entry: &Undo) -> bool {
    store
        .within(|| {
            // The whole queue, not only what is due: a later operation waiting out a retry or a
            // credential is later all the same. One that cannot be read counts as none waiting.
            let waiting: Vec<Waiting> = store
                .outbox_due(entry.account.clone(), queue_horizon())
                .or_log("the outbox could not be read to take an operation back")
                .unwrap_or_default()
                .into_iter()
                .map(|queued| Waiting {
                    id: queued.id,
                    op: queued.op,
                    undo: queued.undo,
                    forward: queued.forward,
                    attempts: queued.attempts,
                })
                .collect();
            let snapshot = TakeBackSnapshot {
                accounts,
                waiting: &waiting,
                junk_label: crate::gather::junk_label(store, &entry.account),
            };
            let commit = mail_logic::act::take_back(&snapshot, entry, Utc::now())?;
            store
                .commit(&commit)
                .or_log("an undo could not be applied")
                .map(|_| ())
        })
        .or_log("an undo could not be applied")
        .flatten()
        .is_some()
}

/// A moment after every queued entry's next attempt, so that `outbox_due` lists the whole queue.
///
/// Year 9999 rather than `DateTime::MAX_UTC`: the store compares times as fixed-width text, and
/// a signed six-digit year sorts before every real one.
fn queue_horizon() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(253_402_300_799, 0).unwrap_or_else(chrono::Utc::now)
}

/// What the server was last seen to support, or, before the first sync, nothing: a change is
/// then made here alone and the server hears nothing, rather than hearing a guess.
pub fn caps_here(
    store: &SqliteStore,
    account: AccountId,
    now: chrono::DateTime<chrono::Utc>,
) -> AccountCaps {
    crate::sync::caps_of(store, account).unwrap_or_else(|| mail_logic::act::local_caps(now))
}

/// Apply one resolved operation: locally, and to the server when it has a server half.
///
/// Returns what it takes to undo it, which the caller keeps to take it back.
pub fn perform(store: &SqliteStore, thread: ThreadId, op: Op) -> Option<Undo> {
    perform_on(store, thread, Target::Threads(vec![thread]), op)
}

/// [`perform`], on `target` within `thread`: the conversation whole, or some of its messages,
/// as opening one message of a list of single messages reads that message and not the rest.
pub fn perform_on(store: &SqliteStore, thread: ThreadId, target: Target, op: Op) -> Option<Undo> {
    perform_with(
        store,
        &crate::gather::acting_accounts(store),
        thread,
        target,
        op,
    )
}

/// [`perform_on`], over accounts already read: for many operations in a row.
///
/// The conversation is read, the operation decided ([`mail_logic::act::act`]) and written in one
/// go, so a message that arrives meanwhile is not missed. The server's half is queued with it:
/// an operation the server cannot be told is not done here either, and nothing is left half
/// written, as a folder that cannot be queued is not made (`folder::apply`).
pub fn perform_with(
    store: &SqliteStore,
    accounts: &Accounts,
    thread: ThreadId,
    target: Target,
    op: Op,
) -> Option<Undo> {
    store
        .within(|| {
            let acted = decide(store, accounts, thread, target, op)?;
            store
                .commit(&acted.commit)
                .or_log("an operation could not be applied")?;
            Some(acted.undo)
        })
        .or_log("an operation could not be applied")
        .flatten()
}

/// One operation on `thread`, decided over the store as it is now.
fn decide(
    store: &SqliteStore,
    accounts: &Accounts,
    thread: ThreadId,
    target: Target,
    op: Op,
) -> Option<mail_logic::act::Acted> {
    let view = crate::gather::thread_view(store, thread)?;
    let account = view.messages.first().map(|m| m.account.clone())?;
    let junk_labels: Vec<_> = crate::gather::junk_label(store, &account)
        .map(|label| (account, label))
        .into_iter()
        .collect();
    let views = [view];
    let snapshot = ActSnapshot {
        threads: &views,
        accounts,
        junk_labels: &junk_labels,
    };
    mail_logic::act::act(&snapshot, &[Act { thread, target, op }], Utc::now()).pop()
}

/// Delete forever what `thread` holds in Trash or Spam, here and on the server. Returns the
/// messages it removed; `None` when there were none.
///
/// The deletion is queued while the store still knows where each message is, and the messages
/// removed in the same commit (`Resolve::BeforeForward`); nothing is returned to undo it.
pub fn destroy(store: &SqliteStore, thread: ThreadId) -> Option<Vec<MessageId>> {
    destroy_target(store, thread, Target::Threads(vec![thread]))
}

/// [`destroy`], for `messages` of `thread` alone: those of them in Trash or Spam are deleted
/// forever, and the rest of the conversation stays. What erasing Trash and Spam by themselves
/// uses ([`crate::erase`]), which picks messages by how long each has been there.
pub fn destroy_messages(
    store: &SqliteStore,
    thread: ThreadId,
    messages: Vec<MessageId>,
) -> Option<Vec<MessageId>> {
    destroy_target(store, thread, Target::Messages(messages))
}

/// [`destroy`] and [`destroy_messages`]: Delete forever on `target` within `thread`.
fn destroy_target(store: &SqliteStore, thread: ThreadId, target: Target) -> Option<Vec<MessageId>> {
    let accounts = crate::gather::acting_accounts(store);
    store
        .within(|| {
            let acted = decide(store, &accounts, thread, target, Op::Destroy)?;
            let gone: Vec<MessageId> = acted
                .commit
                .forward
                .changes
                .iter()
                .filter_map(|change| match change {
                    Change::MessageDelete(m) => Some(*m),
                    _ => None,
                })
                .collect();
            if gone.is_empty() {
                return None;
            }
            // Not removed here either when the server cannot be told: a message gone here and
            // left there comes back with the next sync, and meanwhile says it was deleted.
            store
                .commit(&acted.commit)
                .or_log("a deletion could not be applied")?;
            Some(gone)
        })
        .or_log("a deletion could not be applied")
        .flatten()
}

#[cfg(test)]
mod tests;
