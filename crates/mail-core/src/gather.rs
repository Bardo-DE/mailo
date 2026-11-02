//! Reading what a decision is made over. Reads only: the stores and the settings files in, a
//! value out, so that what decides never reads either itself.

use crate::error::{CoreError, Logged};
use mail_logic::accounts::{Account, Accounts, Prefs, Settings};
use mail_store::SqliteStore;
use std::path::Path;

/// The settings the accounts are read with: the choices in `offline.json` under `config` (every
/// account at the defaults when there is no such directory or the file will not read) and the
/// junk policy the window set.
pub fn settings(config: Option<&Path>, junk: mail_logic::junk::Policy) -> Settings {
    Settings {
        kept: config.map(crate::offline::load).unwrap_or_default(),
        junk,
    }
}

/// Every account, in the order they were added, with its plan, what its server was last observed
/// to support and what `settings` say about syncing it.
///
/// An account whose plan or capabilities cannot be read is left out and logged, so that one bad
/// row does not take every other account's syncing with it. One nothing has connected for yet is
/// in, with no capabilities.
pub fn accounts(store: &SqliteStore, settings: &Settings) -> Result<Accounts, CoreError> {
    let mut out = Vec::new();
    for stored in store.list_accounts()? {
        match read_one(store, stored, settings) {
            Ok(account) => out.push(account),
            Err(error) => log::warn!("an account is left out: {error}"),
        }
    }
    Ok(Accounts::new(out))
}

/// One stored account as an [`Account`], or why it cannot be.
fn read_one(
    store: &SqliteStore,
    stored: mail_store::StoredAccount,
    settings: &Settings,
) -> Result<Account, CoreError> {
    let address = stored.address;
    let plan = stored.plan.map_err(|e| CoreError::PlanUnreadable {
        address: address.clone(),
        why: crate::sync::why(&e),
    })?;
    let caps = stored
        .caps
        .map(|caps| {
            caps.map_err(|e| CoreError::CapsUnreadable {
                address: address.clone(),
                why: crate::sync::why(&e),
            })
        })
        .transpose()?;
    let paused = store
        .account_prefs(stored.id.clone())
        .or_log("the account's preferences could not be read")
        .is_some_and(|prefs| prefs.syncing == mail_store::Syncing::Paused);
    Ok(Account {
        prefs: Prefs::of(settings, &stored.id),
        id: stored.id,
        address,
        plan,
        caps,
        paused,
    })
}
