//! "Block sender": a rule that sends one address's mail to Spam, and stops the rules after it.
//!
//! An ordinary [`Rule`], written through the same rows the window's rules page and `mailo rules` use,
//! so it is listed, edited, switched off and put on a Sieve server like any other. Its condition
//! is the whole address (`TextMatch::Exact`), which the Sieve compiler writes as an `address
//! :all :is "from"` test; a substring would also block every address that contains this one.
//!
//! It runs where rules run: on mail as it arrives. Mail already here stays where it is.

use crate::error::CoreError;
use crate::notify::own_addresses;
use chrono::Utc;
use mail_domain::{AfterMatch, Rule, RuleAction, RuleId, RuleState, ThreadId};
use mail_logic::rules::block::{blocks, condition, newest_outside};
use mail_store::{SqliteStore, Store};
use porter_core::AccountId;

/// What blocking did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocked {
    /// A rule was made.
    Made(Rule),
    /// The address was already blocked by this rule; nothing was written.
    Already(Rule),
}

/// The address to block for a conversation: the sender of its newest message that is not one of
/// the user's own addresses (an account's, or an identity's), since a conversation's last word is
/// often their own and blocking that would send their own mail to Spam. `None` when every
/// message is theirs, and then there is no one to block.
pub fn who_to_block(store: &SqliteStore, thread: ThreadId) -> Result<Option<String>, CoreError> {
    let own = own_addresses(store)?;
    let mut senders = Vec::new();
    for id in store.thread(thread)?.messages {
        let message = store.message(id)?;
        senders.push((message.date, message.from.email));
    }
    Ok(newest_outside(senders, &own))
}

/// Block `email` on `account`: one rule, first in the order so no earlier rule files the mail
/// somewhere else or stops the rules before it is reached. Blocking an address already blocked
/// writes nothing.
pub fn block(store: &SqliteStore, account: AccountId, email: &str) -> Result<Blocked, CoreError> {
    let email = email.trim();
    if email.is_empty() || !email.contains('@') {
        return Err(CoreError::NotAnAddressToBlock(email.to_owned()));
    }
    let existing = store.rules(account.clone())?;
    if let Some(rule) = existing.iter().find(|rule| blocks(rule, email)) {
        return Ok(Blocked::Already(rule.clone()));
    }
    // Names are unique on an account; a rule the user already called this keeps its name.
    let base = format!("Block {email}");
    let name = std::iter::once(base.clone())
        .chain((2..).map(|n| format!("{base} ({n})")))
        .find(|name| existing.iter().all(|rule| &rule.name != name))
        .unwrap_or(base); // `find` over an endless chain only ends by finding one.
    // Before every other rule. Positions start at 1 from the window and the command line, so
    // this is 0; blocks share it and run in name order, which between blocks does not matter.
    let position = existing
        .iter()
        .map(|rule| rule.position)
        .min()
        .map_or(1, |first| first.saturating_sub(1));
    let rule = Rule {
        id: RuleId::generate(),
        account,
        name,
        position,
        state: RuleState::Enabled,
        filter: condition(email),
        actions: vec![RuleAction::Spam],
        after: AfterMatch::Stop,
        made: Utc::now(),
        reply_days: mail_domain::rule::DEFAULT_REPLY_DAYS,
    };
    store.put_rule(&rule)?;
    Ok(Blocked::Made(rule))
}
