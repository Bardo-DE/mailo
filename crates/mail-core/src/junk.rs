//! What the user teaches the junk filter, over the store.
//!
//! What the words say, whether a sender is spared, the policy and the server's verdict are
//! `mail_logic::junk`, and judging a pass's arrivals is one step of the arrival fold
//! (`mail_logic::arrival`, run by [`crate::arrival`]). This keeps what the user teaches
//! ([`learn`]). The counts live in the store, per account (`junk_words`); [`learn`] is what the
//! window's Spam and Not Spam call ([`crate::act::perform`]).

use crate::error::CoreError;
use mail_domain::Message;
use mail_logic::junk::words;
use mail_store::SqliteStore;
use mail_store::junk::{Judged, Learned};

/// Learn `messages` as junk or not: the window's Spam and Not Spam. Each is counted once, and
/// one learned the other way before is taken back first.
pub fn learn(
    store: &SqliteStore,
    messages: &[Message],
    judged: Judged,
) -> Result<usize, CoreError> {
    let mut learned = 0;
    for message in messages {
        let said =
            store.junk_learn(message.account.clone(), message.id, &words(message), judged)?;
        if said != Learned::Unchanged {
            learned += 1;
        }
    }
    Ok(learned)
}

#[cfg(test)]
mod tests;
