//! Judging new mail by what the user has called spam, over the store.
//!
//! What the words say, whether a sender is spared, the policy and the server's verdict are
//! `mail_logic::junk`. This reads the contacts that spare a sender ([`spared`]), keeps what the
//! user teaches ([`learn`]), and acts on a verdict ([`sift`]). The counts live in the store, per
//! account (`junk_words`); [`learn`] is what the window's Spam and Not Spam call
//! ([`crate::act::perform`]).

use crate::error::{CoreError, Logged};
use chrono::{DateTime, Utc};
use mail_domain::{
    AccountCaps, Body, LabelOrigin, MailboxRole, Membership, Message, MessageId, Op,
};
use mail_logic::junk::{
    Acquaintance, Filtering, Heed, JUNK_LABEL, Policy, THRESHOLD, Treat, score, server_says_spam,
    words,
};
use mail_store::junk::{Judged, Learned};
use mail_store::{SqliteStore, Store};
use porter_core::AccountId;

/// Whether `message` is spared judging, by whom it is from or to: `names` are the user's full
/// names, from their identities. The sender is looked up in the contacts here; the rest is
/// `mail_logic::junk::spared`.
pub fn spared(store: &SqliteStore, message: &Message, policy: &Policy, names: &[String]) -> bool {
    let contact = store
        .contact(&message.from.email)
        .or_log("a sender could not be looked up")
        .flatten();
    let from = Acquaintance {
        in_contacts: contact
            .as_ref()
            .is_some_and(|contact| contact.origin != mail_store::Origin::History),
        written_to: contact
            .as_ref()
            .is_some_and(|contact| contact.written.count > 0),
    };
    mail_logic::junk::spared(from, message, policy, names)
}

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

/// What sifting a pass's arrivals did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sifted {
    /// Judged junk and moved or marked.
    pub junk: Vec<MessageId>,
}

/// Judge what a pass stored for the first time on `account`, as `policy` says.
///
/// Mail the server put in its own Junk is learned as spam rather than judged: that is the
/// server's filter speaking. Only mail in the inbox is judged, so a rule that filed it, or the
/// user who moved it already, has the last word.
pub fn sift(
    store: &SqliteStore,
    account: AccountId,
    caps: &AccountCaps,
    arrived: &[MessageId],
    policy: &Policy,
    now: DateTime<Utc>,
) -> Result<Sifted, CoreError> {
    let mut sifted = Sifted::default();
    if policy.filtering == Filtering::Off || arrived.is_empty() {
        return Ok(sifted);
    }
    let names: Vec<String> = store
        .identities(account.clone())?
        .into_iter()
        .filter_map(|identity| identity.from.name)
        .filter(|name| !name.trim().is_empty())
        .collect();
    for id in arrived {
        let Ok(message) = store.message(*id) else {
            // Gone since the pass stored it: nothing left to judge.
            continue;
        };
        if message.account != account {
            continue;
        }
        match message.mailbox {
            MailboxRole::Spam => {
                learn(store, std::slice::from_ref(&message), Judged::Spam)?;
                continue;
            }
            MailboxRole::Inbox => {}
            _ => continue,
        }
        // Only its headers are here yet, and a subject alone is too little to call it junk.
        if matches!(message.body, Body::Absent) {
            continue;
        }
        if spared(store, &message, policy, &names) {
            continue;
        }
        let server = policy.server_says == Heed::Yes
            && match &message.body {
                Body::Present { raw, .. } => store
                    .blobs()
                    .get(*raw)
                    .is_ok_and(|bytes| server_says_spam(&bytes)),
                Body::Absent => false,
            };
        let judged = server || {
            let counts = store.junk_counts(account.clone(), &words(&message))?;
            score(&counts).is_some_and(|p| p >= THRESHOLD)
        };
        if !judged {
            continue;
        }
        let op = match policy.treat {
            Treat::Move => Op::Spam,
            Treat::Mark => Op::Label(
                mail_store::rules::label_named(
                    store,
                    account.clone(),
                    JUNK_LABEL,
                    LabelOrigin::User,
                )?,
                Membership::In,
            ),
        };
        mail_store::rules::perform(store, caps, &op, &message, now)?;
        sifted.junk.push(message.id);
    }
    Ok(sifted)
}

#[cfg(test)]
mod tests;
