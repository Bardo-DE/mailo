//! What the person chose for each account's syncing, as a value: whether all its mail is kept
//! here, how often it is checked, how far back its mail is downloaded, and when a POP3 server's
//! copy is removed. [`Kept`] is the stored setting, and `mail-core` reads and writes the file it
//! lives in; the choices and what they mean are decided here.

use chrono::{DateTime, TimeDelta, Utc};
use porter_core::AccountId;
use std::collections::{BTreeMap, BTreeSet};

/// The file the setting is kept in, in the config directory.
pub const FILE_NAME: &str = "offline.json";

/// How much of an account a sync keeps here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Keep {
    /// Every message, with large attachments left on the server until opened.
    #[default]
    Bodies,
    /// Every message and every attachment.
    Everything,
}

/// How often a timed pass checks an account for new mail: the account page's Check for New Mail.
///
/// [`Every::Automatic`] is what every account did before this was a choice: a server that can
/// push is waited on, and anything else is polled at the interval its capabilities ask for
/// (`sync::due::every` in mail-core). A fixed interval is polled at that, and is never pushed to, as
/// Mail does: a person who asks for every fifteen minutes is not asking for a connection held
/// open. [`Every::Manually`] is neither: no timed pass and no connection, only a person's Sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Every {
    #[default]
    Automatic,
    /// Every this many minutes.
    Minutes(u16),
    Manually,
}

impl Every {
    /// What the account page offers, in its order.
    pub const CHOICES: [Every; 7] = [
        Every::Automatic,
        Every::Minutes(1),
        Every::Minutes(5),
        Every::Minutes(15),
        Every::Minutes(30),
        Every::Minutes(60),
        Every::Manually,
    ];
}

/// How far back a pass downloads an account's mail: the account page's Keep Mail From.
///
/// A message older than the window is still listed, from the headers every pass fetches, but its
/// body is not fetched until it is opened, or shown on the screen (`mail_runtime::wanted`).
/// Headers are what a POP3 server and a list need to say how old a message is at all, so they are
/// fetched whatever its age; it is the body, and its attachments, that make an account large.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Window {
    #[default]
    All,
    /// The last this many days.
    Days(u16),
}

impl Window {
    /// What the account page offers, in its order.
    pub const CHOICES: [Window; 4] = [
        Window::All,
        Window::Days(30),
        Window::Days(90),
        Window::Days(365),
    ];

    /// The oldest a message may be dated at `now` and still be downloaded; `None` for all of it.
    pub fn since(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Window::All => None,
            Window::Days(days) => Some(now - TimeDelta::days(i64::from(days))),
        }
    }
}

/// When a POP3 pass removes the server's copy of a message this computer holds in full: the
/// account page's Remove from Server.
///
/// [`Remove::Never`] by default, whatever the plan's `LeaveOnServer` says: nothing sent `DELE`
/// before this, so an account added with delete-after-fetch has always left its mail on the
/// server, and an upgrade must not start deleting it unasked. A message's age is its date, the
/// only time the store keeps of it. Only a message whose body is held here is ever removed, so
/// a first sync that stops halfway never leaves a message nowhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Remove {
    #[default]
    Never,
    /// Once the message is this many days old.
    AfterDays(u16),
    /// Once it is in the Trash here, or deleted forever.
    WhenDeleted,
}

impl Remove {
    /// What the account page offers, in its order.
    pub const CHOICES: [Remove; 5] = [
        Remove::Never,
        Remove::AfterDays(1),
        Remove::AfterDays(7),
        Remove::AfterDays(30),
        Remove::WhenDeleted,
    ];
}

/// The stored setting: the accounts kept offline in full, and each account's other choices where
/// it has made one. Every other account is [`Keep::Bodies`] and the defaults. An account set back
/// to a default is left out, so the file holds only what differs.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Kept {
    #[serde(default)]
    everything: BTreeSet<AccountId>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    every: BTreeMap<AccountId, Every>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    window: BTreeMap<AccountId, Window>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    remove: BTreeMap<AccountId, Remove>,
}

/// `map` with `account` set to `value`, or left out when that is the default.
fn chosen<T: Default + PartialEq>(map: &mut BTreeMap<AccountId, T>, account: AccountId, value: T) {
    if value == T::default() {
        map.remove(&account);
    } else {
        map.insert(account, value);
    }
}

impl Kept {
    /// What `account` keeps.
    pub fn of(&self, account: AccountId) -> Keep {
        if self.everything.contains(&account) {
            Keep::Everything
        } else {
            Keep::Bodies
        }
    }

    /// The same, with `account` set to `keep`.
    pub fn with(mut self, account: AccountId, keep: Keep) -> Self {
        match keep {
            Keep::Everything => self.everything.insert(account),
            Keep::Bodies => self.everything.remove(&account),
        };
        self
    }

    /// How often `account` is checked.
    pub fn every_of(&self, account: &AccountId) -> Every {
        // Never chosen is the default: what every account did before this was a choice.
        self.every.get(account).copied().unwrap_or_default()
    }

    /// How far back `account` is downloaded.
    pub fn window_of(&self, account: &AccountId) -> Window {
        // Never chosen is the default: what every account did before this was a choice.
        self.window.get(account).copied().unwrap_or_default()
    }

    /// When `account`'s server copies are removed.
    pub fn remove_of(&self, account: &AccountId) -> Remove {
        // Never chosen is the default: what every account did before this was a choice.
        self.remove.get(account).copied().unwrap_or_default()
    }

    /// The same, with `account` checked `every`.
    pub fn with_every(mut self, account: AccountId, every: Every) -> Self {
        chosen(&mut self.every, account, every);
        self
    }

    /// The same, with `account` downloaded as far back as `window`.
    pub fn with_window(mut self, account: AccountId, window: Window) -> Self {
        chosen(&mut self.window, account, window);
        self
    }

    /// The same, with `account`'s server copies removed by `remove`.
    pub fn with_remove(mut self, account: AccountId, remove: Remove) -> Self {
        chosen(&mut self.remove, account, remove);
        self
    }

    /// The same, with nothing kept of `account`: what its removal leaves.
    pub fn without(self, account: AccountId) -> Self {
        self.with(account.clone(), Keep::Bodies)
            .with_every(account.clone(), Every::default())
            .with_window(account.clone(), Window::default())
            .with_remove(account, Remove::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_reaches_back_its_days_and_all_of_it_reaches_back_for_ever() {
        let now = DateTime::UNIX_EPOCH + TimeDelta::days(1000);
        assert_eq!(Window::All.since(now), None);
        assert_eq!(Window::Days(30).since(now), Some(now - TimeDelta::days(30)));
    }
}
