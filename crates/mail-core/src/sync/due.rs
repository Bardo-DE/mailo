//! Which accounts a timed pass in the window syncs.
//!
//! The window wakes at the shortest interval any account asks for, [`mail_logic::accounts::Accounts::poll_interval`]. A
//! wake syncs only the accounts whose own interval has passed: one Graph account polls every
//! fifteen seconds, and the IMAP accounts beside it want a pass every five minutes. A pass the user asks for is
//! not a wake and still syncs them all, through [`SyncOps::run`](crate::SyncOps).
//!
//! Each account's interval is the one its server asks for unless the person chose another on
//! the account's page ([`crate::offline::Every`]). One checked manually is still an account with
//! a server, so a person's Sync reaches it; its interval is [`MANUALLY`], which no timer reaches
//! and the scheduler starts no pass at, the first one included.

use crate::error::CoreError;
use std::time::Duration;

use crate::offline::Every;
use mail_domain::WatchMode;
use mail_logic::accounts::Accounts;
use porter_core::AccountId;

pub use mail_engine::schedule::MANUALLY;

pub use mail_logic::accounts::{IDLE, every};

/// How often one account is checked, given what the person chose: the server's own interval on
/// [`Every::Automatic`], the minutes chosen, or [`MANUALLY`].
pub fn chosen(watch: &WatchMode, chose: Every) -> Duration {
    match chose {
        Every::Automatic => every(watch),
        // Never under a minute, whatever a hand-edited file says: a zero would be a pass at every
        // turn of the loop.
        Every::Minutes(minutes) => Duration::from_secs(60 * u64::from(minutes.max(1))),
        Every::Manually => MANUALLY,
    }
}

/// Each account with a server, and how often it wants a pass, as the person chose.
///
/// An account that keeps its mail here is left out: it has nothing to fetch, so it is never due.
/// Nor is one whose syncing is paused, or one nothing has connected for yet.
pub fn intervals(accounts: &Accounts) -> Vec<(AccountId, Duration)> {
    accounts
        .syncing()
        .filter_map(|account| {
            let caps = account.caps.as_ref()?;
            Some((account.id.clone(), chosen(&caps.watch, account.prefs.every)))
        })
        .collect()
}

impl crate::mail::SyncOps<'_> {
    /// One pass over the accounts in `due` and no others, as [`SyncOps::run`](crate::SyncOps) does
    /// it for all of them.
    pub async fn run_due(
        &self,
        due: &[AccountId],
        hooks: super::report::Hooks<'_>,
    ) -> Result<Vec<super::report::PassEnd>, CoreError> {
        let mail = self.0;
        let registry = mail.clients()?;
        let accounts = mail.roster()?;
        super::run_all(
            mail.store().clone(),
            mail.secrets(),
            &registry,
            mail.now(),
            super::Mode::Once,
            &accounts,
            &|account| due.contains(&account),
            hooks,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::id::account_id_from_uuid;

    fn acct_imap() -> AccountId {
        account_id_from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000e2"))
    }
    fn acct_new() -> AccountId {
        account_id_from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000e4"))
    }

    /// What the account page offers, as the interval the scheduler is handed.
    #[test]
    fn each_choice_is_the_interval_it_says() {
        let poll = WatchMode::Poll { every: secs(60) };
        let table: [(&WatchMode, Every, Duration); 9] = [
            (&poll, Every::Automatic, secs(60)),
            (&WatchMode::Idle, Every::Automatic, IDLE),
            (&WatchMode::Idle, Every::Minutes(1), secs(60)),
            (&poll, Every::Minutes(5), secs(300)),
            (&poll, Every::Minutes(15), secs(900)),
            (&poll, Every::Minutes(30), secs(1800)),
            (&WatchMode::Idle, Every::Minutes(60), secs(3600)),
            (&poll, Every::Minutes(0), secs(60)),
            (&WatchMode::Idle, Every::Manually, MANUALLY),
        ];
        for (watch, chose, want) in table {
            assert_eq!(chosen(watch, chose), want, "{watch:?}, {chose:?}");
        }
    }

    #[test]
    fn an_account_that_offers_idle_is_polled_every_five_minutes() {
        assert_eq!(every(&WatchMode::Idle), secs(300));
    }

    #[test]
    fn a_paused_account_is_never_due() {
        let store = mail_store::testing::in_memory();
        let now = chrono::DateTime::UNIX_EPOCH;
        for (id, address) in [
            (acct_imap(), "ada@example.test"),
            (acct_new(), "bo@example.test"),
        ] {
            let preset = mail_domain::presets::manual_pop3(
                address,
                &mail_domain::presets::ManualPop3 {
                    pop3_host: "pop.example.test".to_owned(),
                    pop3_port: 995,
                    smtp_host: "smtp.example.test".to_owned(),
                    smtp_port: 465,
                    login: None,
                },
                now,
            );
            mail_store::testing::seed_account_plan(&store, id.clone(), address, &preset.plan, None);
            mail_store::testing::seed_caps(&store, id, &preset.expected_caps, now).unwrap();
        }
        store
            .set_account_syncing(acct_imap(), mail_store::Syncing::Paused)
            .unwrap();
        let at = |store: &mail_store::SqliteStore| {
            intervals(&crate::gather::accounts(store, &Default::default()).unwrap())
        };
        let listed: Vec<AccountId> = at(&store).into_iter().map(|(id, _)| id).collect();
        assert_eq!(
            listed,
            [acct_new()],
            "the paused account is not on the timetable"
        );
        store
            .set_account_syncing(acct_imap(), mail_store::Syncing::On)
            .unwrap();
        assert_eq!(at(&store).len(), 2, "and is again once it syncs");
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }
}
