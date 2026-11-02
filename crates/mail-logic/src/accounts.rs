//! The accounts as one immutable value: each account's plan, what its server was last observed to
//! support, and what the person chose for how it is synced.
//!
//! Gathered once (`mail-core`'s `gather::accounts`) and handed to whatever decides, so a decision
//! about an account never reads the store or a settings file itself, and two decisions made from
//! one value agree. Nothing here is defaulted: an account's [`Prefs`] are the person's choices or
//! the defaults the settings file gives, never a placeholder to be overlaid later.

use crate::{junk, offline};
use mail_domain::{AccountCaps, AccountPlan, AuthPlan, WatchMode};
use porter_core::AccountId;
use std::time::Duration;

/// What an account that offers IDLE is polled at, since no connection is held open for it.
pub const IDLE: Duration = Duration::from_secs(300);

/// How often an account whose server is `watch`ed wants a pass, before the person chooses.
pub fn every(watch: &WatchMode) -> Duration {
    match watch {
        WatchMode::Poll { every } => *every,
        WatchMode::Idle => IDLE,
    }
}

/// The settings the accounts are read with: the person's syncing choices and the junk policy.
/// Built at the edge, from the files under the program's roots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub kept: offline::Kept,
    pub junk: junk::Policy,
}

/// How an account is synced: the person's choices ([`offline`]) and the junk filter's policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Prefs {
    /// Whether a pass fetches the attachments a sync leaves on the server too.
    pub keep: offline::Keep,
    /// How often a timed pass checks it.
    pub every: offline::Every,
    /// How far back a pass downloads its bodies.
    pub window: offline::Window,
    /// When a POP3 pass removes the server's copies.
    pub remove: offline::Remove,
    /// How the junk filter judges what a pass brings.
    pub junk: junk::Policy,
}

impl Prefs {
    /// `account`'s, as `settings` has them.
    pub fn of(settings: &Settings, account: &AccountId) -> Prefs {
        Prefs {
            keep: settings.kept.of(account.clone()),
            every: settings.kept.every_of(account),
            window: settings.kept.window_of(account),
            remove: settings.kept.remove_of(account),
            junk: settings.junk,
        }
    }
}

/// One account.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub id: AccountId,
    pub address: String,
    pub plan: AccountPlan,
    /// What the server was last observed to support; `None` when nothing has connected yet. Each
    /// consumer decides what that means for it: the scheduler probes, an action takes effect here
    /// alone, and sending refuses with the usual remedy.
    pub caps: Option<AccountCaps>,
    pub prefs: Prefs,
    /// Whether the person paused its syncing.
    pub paused: bool,
}

impl Account {
    /// An account with the default [`Prefs`], not paused.
    pub fn new(
        id: AccountId,
        address: impl Into<String>,
        plan: AccountPlan,
        caps: Option<AccountCaps>,
    ) -> Account {
        Account {
            id,
            address: address.into(),
            plan,
            caps,
            prefs: Prefs::default(),
            paused: false,
        }
    }

    /// Whether the account has a server to fetch from and send through, rather than keeping its
    /// mail on this computer alone.
    pub fn has_server(&self) -> bool {
        self.plan.incoming.caps().has_server
    }

    /// Whether a timed pass, a pass asked for or a held connection reaches it: it has a server and
    /// is not paused.
    pub fn syncs(&self) -> bool {
        self.has_server() && !self.paused
    }

    /// How often its server asks for a pass; `None` before anything has connected.
    pub fn poll_interval(&self) -> Option<Duration> {
        self.caps.as_ref().map(|caps| every(&caps.watch))
    }
}

/// Every account, in the order they were added.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Accounts(Vec<Account>);

impl Accounts {
    pub fn new(accounts: Vec<Account>) -> Accounts {
        Accounts(accounts)
    }

    /// For a test: these accounts, as they are.
    pub fn fixture(accounts: impl IntoIterator<Item = Account>) -> Accounts {
        Accounts(accounts.into_iter().collect())
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Account> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn get(&self, id: &AccountId) -> Option<&Account> {
        self.0.iter().find(|account| &account.id == id)
    }

    pub fn by_address(&self, address: &str) -> Option<&Account> {
        self.0.iter().find(|account| account.address == address)
    }

    /// The accounts that keep their mail on this computer alone.
    pub fn local(&self) -> Vec<AccountId> {
        self.0
            .iter()
            .filter(|account| !account.has_server())
            .map(|account| account.id.clone())
            .collect()
    }

    /// What `account`'s server was last observed to support.
    pub fn caps_of(&self, account: &AccountId) -> Option<&AccountCaps> {
        self.get(account)?.caps.as_ref()
    }

    /// Every account's id and address.
    pub fn addresses(&self) -> Vec<(AccountId, String)> {
        self.0
            .iter()
            .map(|account| (account.id.clone(), account.address.clone()))
            .collect()
    }

    /// How `account` signs in.
    pub fn auth_of(&self, account: &AccountId) -> Option<&AuthPlan> {
        self.get(account).map(|account| &account.plan.auth)
    }

    /// Whether `account` has a server. An id nothing knows is taken to have one, as an account
    /// that keeps its mail here is only ever one that is known to.
    pub fn has_server(&self, account: &AccountId) -> bool {
        self.get(account).is_none_or(Account::has_server)
    }

    /// The accounts a pass can reach: those with a server that are not paused.
    pub fn syncing(&self) -> impl Iterator<Item = &Account> {
        self.0.iter().filter(|account| account.syncs())
    }

    /// The shortest interval any account that is not paused asks for. An account that wants
    /// IDLE-like freshness is not held to another's hourly poll; `None` when no account has
    /// connected.
    pub fn poll_interval(&self) -> Option<Duration> {
        self.0
            .iter()
            .filter(|account| !account.paused)
            .filter_map(Account::poll_interval)
            .min()
    }
}

impl<'a> IntoIterator for &'a Accounts {
    type Item = &'a Account;
    type IntoIter = std::slice::Iter<'a, Account>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::id::account_id_from_uuid;
    use mail_domain::{Incoming, Outgoing, SaslMech, Tls, Username};

    fn id(n: u128) -> AccountId {
        account_id_from_uuid(uuid::Uuid::from_u128(0x4000_8000_0000_0000_0000 + n))
    }

    fn plan(incoming: Incoming) -> AccountPlan {
        AccountPlan {
            address: "a@example.test".to_owned(),
            incoming,
            outgoing: Outgoing::Smtp {
                host: "smtp.example.test".to_owned(),
                port: 465,
                tls: Tls::Implicit,
            },
            auth: AuthPlan::Password {
                username: Username::SameAsAddress,
                sasl: vec![SaslMech::Plain],
            },
            identities: Vec::new(),
        }
    }

    fn imap() -> Incoming {
        Incoming::Imap {
            host: "imap.example.test".to_owned(),
            port: 993,
            tls: Tls::Implicit,
        }
    }

    #[test]
    fn local_is_the_accounts_without_a_server_connected_or_not() {
        let accounts = Accounts::fixture([
            Account::new(id(1), "one@example.test", plan(imap()), None),
            Account::new(id(2), "two@example.test", plan(Incoming::Local), None),
        ]);
        assert_eq!(accounts.local(), [id(2)]);
        assert!(accounts.has_server(&id(1)));
        assert!(!accounts.has_server(&id(2)));
        // An id nothing knows is not one that keeps its mail here.
        assert!(accounts.has_server(&id(9)));
    }

    #[test]
    fn a_paused_or_local_account_is_not_one_a_pass_reaches() {
        let mut paused = Account::new(id(1), "one@example.test", plan(imap()), None);
        paused.paused = true;
        let accounts = Accounts::fixture([
            paused,
            Account::new(id(2), "two@example.test", plan(Incoming::Local), None),
            Account::new(id(3), "three@example.test", plan(imap()), None),
        ]);
        let reached: Vec<_> = accounts.syncing().map(|a| a.id.clone()).collect();
        assert_eq!(reached, [id(3)]);
        assert_eq!(accounts.addresses().len(), 3);
    }
}
