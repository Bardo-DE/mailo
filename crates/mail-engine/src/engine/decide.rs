//! What a sync pass decides, as functions of what it has read.
//!
//! [`AccountEngine::sync`](super::AccountEngine::sync) and the body pass read the store and the
//! backend, ask the functions here what to do, and do it. None of these reads a store, draws a
//! clock or touches a connection: each is given the state it decides over, so a table can test
//! it, and the loop that calls them is left with the reads and the writes.

use super::{UNREADABLE_RETRY, by_band};
use chrono::{DateTime, Utc};
use mail_domain::{FetchSince, RemoteRef, SyncCursor, ThreadId, UidValidity};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// What the store holds of one mailbox, and what a pass decides its survey from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MailboxState {
    /// The state up to which everything the server listed is held (`Store::held_through`).
    pub held_through: Option<SyncCursor>,
    /// The cursor the last write left (`Store::cursor`).
    pub cursor: Option<SyncCursor>,
    /// When the whole listing for deletions was last made (`Swept::expunges`).
    pub expunges_swept: Option<DateTime<Utc>>,
}

/// How a survey of a mailbox is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Survey {
    /// The whole listing.
    Whole,
    /// Only what arrived after this state.
    From(SyncCursor),
}

impl Survey {
    /// The request that makes this survey.
    pub fn since(&self) -> FetchSince {
        match self {
            Survey::From(cursor) => FetchSince::After {
                cursor: cursor.clone(),
            },
            Survey::Whole => FetchSince::Beginning,
        }
    }

    /// Whether this survey began part-way.
    pub fn resumed(&self) -> bool {
        matches!(self, Survey::From(_))
    }

    /// Whether the server's answer (`fresh`) says the survey has to be made again, whole: it
    /// numbers the mailbox afresh, so where this one resumed from means nothing in it and every
    /// UID held is a different message.
    pub fn redo(&self, fresh: Option<&SyncCursor>) -> bool {
        match (self, fresh) {
            (Survey::From(from), Some(fresh)) => {
                UidValidity::between(Some(from), fresh) == UidValidity::Reset
            }
            _ => false,
        }
    }
}

/// Where the next survey of a mailbox may start from, rather than at the beginning: the state up
/// to which everything the server listed is held.
///
/// Nothing when there is no such state, when the server numbered the mailbox afresh since, or
/// when the listing for deletions is due: a whole listing is what gives the pass a full picture
/// of what the server holds, so one is made on that schedule and not on every pass.
pub(super) fn resume_point(
    state: &MailboxState,
    expunges_every: Duration,
    now: DateTime<Utc>,
) -> Option<SyncCursor> {
    let held @ SyncCursor::Imap {
        uidvalidity,
        uidnext,
        ..
    } = state.held_through.as_ref()?
    else {
        return None;
    };
    // A zero is the server not saying; resuming from it would name nothing.
    if *uidvalidity == 0 || *uidnext == 0 {
        return None;
    }
    if UidValidity::between(state.cursor.as_ref(), held) == UidValidity::Reset {
        return None;
    }
    if due(state.expunges_swept, expunges_every, now) {
        return None;
    }
    Some(held.clone())
}

/// How to survey a mailbox in this state.
pub(super) fn survey_kind(
    state: &MailboxState,
    expunges_every: Duration,
    now: DateTime<Utc>,
) -> Survey {
    resume_point(state, expunges_every, now).map_or(Survey::Whole, Survey::From)
}

/// Whether a survey that began part-way and left the cursor where it was may have found nothing
/// new, which only the addresses it listed can say ([`all_held`]). The server answers a request
/// for what is past the last UID with the last message it has, held already, so "nothing" is not
/// an empty answer.
pub(super) fn may_have_found_nothing(
    survey: &Survey,
    fresh: &Option<SyncCursor>,
    held_before: &Option<SyncCursor>,
) -> bool {
    survey.resumed() && fresh == held_before
}

/// Whether every address a survey listed is held already.
pub(super) fn all_held(listed: &[(RemoteRef, u64)], held: &HashSet<RemoteRef>) -> bool {
    listed.iter().all(|(remote, _)| held.contains(remote))
}

/// What to fetch headers for, in the order to fetch them: the survey's listing, less what is
/// already mapped and less what could not be read when last asked for, newest first, and the
/// smaller bands ahead of the larger.
///
/// The survey is *everything on the server*, which is the right answer to "what exists" and the
/// wrong one to "what should I fetch". It is in server order, oldest first: POP3 message numbers
/// and IMAP UIDs both count up as mail arrives. Nothing else is known before the headers are.
pub(super) fn headers_wanted(
    mut listed: Vec<(RemoteRef, u64)>,
    held: &HashSet<RemoteRef>,
    resting: &HashSet<RemoteRef>,
) -> Vec<(RemoteRef, u64)> {
    listed.retain(|(remote, _)| !held.contains(remote) && !resting.contains(remote));
    listed.reverse();
    by_band(&mut listed);
    listed
}

/// What one operation fetches headers for: the messages of `wanted` that fit in the band ending
/// at `edge`, in order, at most `left`.
pub(super) fn header_batch(wanted: &[(RemoteRef, u64)], edge: u64, left: usize) -> Vec<RemoteRef> {
    wanted
        .iter()
        .filter(|(_, size)| *size <= edge)
        .take(left)
        .map(|(remote, _)| remote.clone())
        .collect()
}

/// The state to remember as held through, once everything the survey listed is held: where the
/// cursor stands, without its modseq (this is a place to start listing from, and the sweep owns
/// that). Nothing when the cursor is not an IMAP one, or the state is remembered already.
pub(super) fn held_mark(
    cursor: &Option<SyncCursor>,
    held_through: &Option<SyncCursor>,
) -> Option<SyncCursor> {
    let Some(SyncCursor::Imap {
        uidvalidity,
        uidnext,
        ..
    }) = cursor
    else {
        return None;
    };
    let through = SyncCursor::Imap {
        uidvalidity: *uidvalidity,
        uidnext: *uidnext,
        modseq: None,
    };
    (held_through.as_ref() != Some(&through)).then_some(through)
}

/// What the store answered when asked which bodies are missing, in two lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Backlog {
    /// Each message of the conversations asked for first, beside its conversation, newest first
    /// within one (`Store::unfetched_in_threads`).
    pub hinted: Vec<(ThreadId, RemoteRef)>,
    /// The rest of what lacks a body (`Store::unfetched_in`).
    pub rest: Vec<RemoteRef>,
}

/// What the person is looking at: the conversations to fetch first, and the addresses of the
/// messages the window is fetching for itself, which the pass leaves out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Wanted {
    pub asked: Vec<ThreadId>,
    pub taken: Vec<RemoteRef>,
}

/// What a body pass fetches, in order, at most `limit`, and how many at the front are the
/// asked-for conversations', which the caller keeps in front.
///
/// The conversations asked for come first, less what the window is fetching itself, so the two
/// do not fetch one body twice. Those that could not be read when last asked for (`resting`) are
/// left out before the limit is applied: one that stays unread would otherwise keep a place in
/// every pass's allowance for ever, and a mailbox of them would fetch nothing. So the backlog was
/// asked for that many more than `limit` ([`backlog_asked`]).
pub(super) fn bodies_wanted(
    backlog: Backlog,
    wanted: &Wanted,
    resting: &HashSet<RemoteRef>,
    limit: u32,
) -> (Vec<RemoteRef>, usize) {
    let (mut order, mut first) = mail_runtime::wanted::ahead(
        &wanted.asked,
        backlog.hinted,
        backlog.rest,
        backlog_asked(limit, resting) as usize,
    );
    for gone in [
        &wanted.taken[..],
        &resting.iter().cloned().collect::<Vec<_>>()[..],
    ] {
        first -= order[..first].iter().filter(|r| gone.contains(r)).count();
        order.retain(|remote| !gone.contains(remote));
    }
    order.truncate(limit as usize);
    first = first.min(order.len());
    (order, first)
}

/// `order`, each with the size the survey gave it, `u64::MAX` where none did: "unknown", sorted
/// into the last band.
pub(super) fn sized(
    order: Vec<RemoteRef>,
    sizes: &HashMap<RemoteRef, u64>,
) -> Vec<(RemoteRef, u64)> {
    order
        .into_iter()
        .map(|remote| {
            let size = sizes.get(&remote).copied().unwrap_or(u64::MAX);
            (remote, size)
        })
        .collect()
}

/// How many the store is asked for to get `limit` bodies once `resting` are left out.
pub(super) fn backlog_asked(limit: u32, resting: &HashSet<RemoteRef>) -> u32 {
    limit.saturating_add(u32::try_from(resting.len()).unwrap_or(u32::MAX))
}

/// When a message that could not be read is asked for again.
pub(super) fn resting_since(now: DateTime<Utc>) -> DateTime<Utc> {
    now - UNREADABLE_RETRY
}

/// Whether a periodic task is due.
///
/// Never having run counts as due, so a fresh account sweeps once immediately rather than
/// waiting out a full interval before it can show a flag change.
pub(super) fn due(last: Option<DateTime<Utc>>, every: Duration, now: DateTime<Utc>) -> bool {
    let Some(last) = last else {
        return true;
    };
    match chrono::TimeDelta::from_std(every) {
        Ok(delta) => now - last >= delta,
        // An interval too large for TimeDelta is not a reason to sweep constantly.
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests;
