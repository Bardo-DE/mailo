//! How large a message is, against the largest its account's server takes: said in the composer
//! before Send, rather than learnt from a refusal after it.
//!
//! The limit is what the sending server said, where it said one: SMTP's `SIZE` in the `EHLO` of
//! the last submission, or a JMAP session's upload limits, both kept beside the account's
//! capabilities as the runtime hears them. Microsoft Graph says nothing, and takes what its
//! upload sessions take. Anything else is measured against [`USUAL_LIMIT`].
//!
//! The two kinds of limit count different things. SMTP and JMAP count the message as it goes,
//! attachments in base64; Graph and the usual limit count what the person attached. A [`Weight`]
//! holds both, and [`Weight::against`] reads the one a limit means.

use mail_domain::{AccountPlan, Draft};
use mail_store::SqliteStore;
use porter_core::AccountId;

pub use mail_logic::compose::weight::{
    Fit, Measure, Picture, Said, SendLimit, USUAL_LIMIT, Weight, fit,
};

use super::pictures;
use super::submit;
use crate::error::Logged;

/// The plan `account` sends by, if there is one and it reads.
fn plan_of(store: &SqliteStore, account: &AccountId) -> Option<AccountPlan> {
    store
        .account(account.clone())
        .or_log("the sending account could not be read")
        .flatten()
        // A plan that does not read says nothing of how it sends: the callers take it for SMTP's.
        .and_then(|stored| stored.plan.ok())
}

/// The largest message `account` sends: what its server said, where it said one.
pub fn send_limit(store: &SqliteStore, account: &AccountId) -> SendLimit {
    // A plan that does not read still sends through the server that said what it takes.
    let way =
        plan_of(store, account).map_or_else(submit::unread, |plan| submit::of(&plan.outgoing));
    let stored = store
        .send_limit(account.clone())
        .or_log("the sending server's size limit could not be read")
        .flatten();
    way.limit(stored)
}

/// Whether `account` can ask for delivery reports (RFC 3461). Graph's `sendMail` has no way to,
/// so the composer does not offer the choice there; every other way of sending takes it, and a
/// plan that does not read leaves the choice offered.
pub fn can_ask_delivery_report(store: &SqliteStore, account: &AccountId) -> bool {
    plan_of(store, account)
        .map_or_else(submit::unread, |plan| submit::of(&plan.outgoing))
        .asks_delivery_reports()
}

/// How large `draft` is, its stored files read and its pictures measured at every size. Reads
/// and decodes every picture, so it is for a thread that does not draw.
///
/// A file the store no longer has counts as nothing: it cannot be sent either, and Send says so.
pub fn weigh(store: &SqliteStore, draft: &Draft) -> Weight {
    let body = draft.text.len() + draft.html.as_ref().map_or(0, String::len);
    let files: Vec<([u64; 4], Picture)> = draft
        .attachments
        .iter()
        .filter_map(|attachment| {
            store
                .blobs()
                .get(attachment.blob)
                .or_log("an attachment could not be read to weigh it")
        })
        .map(|bytes| {
            let picture = if pictures::resizable(&bytes) {
                Picture::Yes
            } else {
                Picture::No
            };
            (pictures::sizes(&bytes), picture)
        })
        .collect();
    Weight::of(body as u64, &files)
}

#[cfg(test)]
#[path = "weight_tests.rs"]
mod tests;
