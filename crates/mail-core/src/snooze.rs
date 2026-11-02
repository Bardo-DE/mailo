//! Putting a conversation off until later, and bringing it back.
//!
//! `op_for` returns `None` for `OpKind::Snooze` because `Op::SetSnooze` needs a payload, the
//! instant to come back at; this module supplies it.
//!
//! Nothing is written when a snooze expires. `SnoozeDue` is resolved against `now` at query time,
//! so a conversation returns to the inbox on the stroke whether the client was running or not;
//! a window that is open asks again at [`next_wake`], so its list shows it come back.

use crate::error::CoreError;
use crate::error::Logged;
use chrono::{DateTime, Local, Utc};
use mail_domain::{
    Filter, Message, Op, PageReq, Property, Query, Snooze, Sort, SortDir, Target, ThreadId,
};
use mail_logic::snooze::snooze_until;
use mail_store::{SqliteStore, Store};

/// Put a conversation off until `phrase` names, and say when that is.
pub fn snooze(
    store: &SqliteStore,
    thread: ThreadId,
    phrase: &str,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, CoreError> {
    let at = snooze_until(phrase, now, &Local)?;
    set(store, thread, Snooze::Until(at), now)?;
    Ok(at)
}

/// When the next snoozed conversation comes back: the earliest snooze still ahead of `now`, or
/// `None` with none waiting (or none readable, which is logged).
pub fn next_wake(store: &SqliteStore, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let filter = Filter::And(vec![
        Filter::Snoozed,
        Filter::Not(Box::new(Filter::SnoozeDue)),
    ]);
    let mut next = None;
    let mut after = None;
    loop {
        let query = Query {
            filter: filter.clone(),
            sort: Sort {
                property: Property::Date,
                dir: SortDir::Desc,
            },
            page: PageReq { after, limit: 500 },
        };
        let Some(page) = store
            .threads(&query, now)
            .or_log("the snoozed conversations could not be read")
        else {
            return next;
        };
        next = page
            .items
            .iter()
            .filter_map(|summary| match summary.snooze {
                Snooze::Until(at) if at > now => Some(at),
                _ => None,
            })
            .chain(next)
            .min();
        match page.next {
            Some(cursor) => after = Some(cursor),
            None => return next,
        }
    }
}

/// Bring one back now.
pub fn wake(store: &SqliteStore, thread: ThreadId, now: DateTime<Utc>) -> Result<(), CoreError> {
    set(store, thread, Snooze::Inactive, now)
}

/// Pin a conversation, or unpin it if it is already pinned. Says whether it is pinned now.
///
/// Here rather than in a module of its own because pin and snooze are the same shape: thread
/// level, local by construction, and needing a payload that `op_for` cannot supply.
pub fn pin(store: &SqliteStore, thread: ThreadId, now: DateTime<Utc>) -> Result<bool, CoreError> {
    let loaded = store.thread(thread)?;
    let op = mail_logic::mailbox_filter::pin_op(&loaded.summary, now);
    let pinned = matches!(op, Op::SetPin(mail_domain::Pin::Rank(_)));
    apply(store, thread, op, now)?;
    Ok(pinned)
}

/// Apply `Op::SetSnooze` to a thread.
fn set(
    store: &SqliteStore,
    thread: ThreadId,
    snooze: Snooze,
    now: DateTime<Utc>,
) -> Result<(), CoreError> {
    apply(store, thread, Op::SetSnooze(snooze), now)
}

/// Apply a thread-level op the way every other operation is applied ([`crate::act`]): decided by
/// [`mail_logic::act::act`] and written in one commit.
///
/// What this is for (a snooze waking, a reminder, a pin) has no server half, so no account's
/// capabilities are consulted: none are given, and the op is decided as local.
///
/// The undo that [`mail_logic::act::act`] makes is dropped here, and nothing says so to the
/// caller: neither a waking snooze nor a sweep of reminders is something the person did to be taken
/// back, and a pin is a toggle. A snooze the person chooses from the menu goes through
/// [`crate::act::perform_on`] and can be taken back.
pub(crate) fn apply(
    store: &SqliteStore,
    thread: ThreadId,
    op: Op,
    now: DateTime<Utc>,
) -> Result<(), CoreError> {
    let accounts = mail_logic::accounts::Accounts::default();
    store.within(|| {
        let view = store.thread(thread)?;
        let messages: Vec<Message> = view
            .messages
            .iter()
            .filter_map(|id| store.message(*id).or_log("a message could not be read"))
            .collect();
        if messages.is_empty() {
            return Err(CoreError::EmptyConversation);
        }
        let views = [mail_logic::act::ThreadView {
            thread: view,
            messages,
        }];
        let snapshot = mail_logic::act::ActSnapshot {
            threads: &views,
            accounts: &accounts,
            junk_labels: &[],
        };
        let acted = mail_logic::act::act(
            &snapshot,
            &[mail_logic::act::Act {
                thread,
                target: Target::Threads(vec![thread]),
                op,
            }],
            now,
        );
        for acted in acted {
            store.commit(&acted.commit)?;
        }
        Ok(())
    })?
}
