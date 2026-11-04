//! Removing an account from this computer: its saved sign-in, then everything the database keeps
//! of it.
//!
//! The secrets go first, every one of the account's in one `Secrets::delete_account`. A sign-in
//! that cannot be forgotten stops the removal before the database is touched, so the account and
//! its mail are still there to try again; the other order would leave a password in the keyring
//! that no account names any more, which nothing would ever find to clean up. A store that
//! refuses part-way may have forgotten some already, and those stay forgotten: the account then
//! asks to sign in again, which is what [`RemoveError::Keyring`] says.
//! The database goes last ([`SqliteStore::remove_account`]): every row that names the account,
//! and the stored messages and attachment parts nothing else uses. Nothing on the server is
//! touched, and keys and certificates stay: they are the user's, not the account's.

pub use mail_logic::account::RemoveError;

use mail_secrets::AccountSecrets;
use mail_store::{Freed, SqliteStore};
use porter_core::AccountId;

/// An account that was removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// The address it had, for the front-end to say what went.
    pub address: String,
    /// The stored messages and parts that went with it.
    pub freed: Freed,
}

/// Remove `account` and everything kept of it here. See the module's words for the order.
pub async fn remove(
    store: &SqliteStore,
    secrets: &dyn AccountSecrets,
    account: AccountId,
) -> Result<Removed, RemoveError> {
    let stored = store
        .account(account.clone())
        .map_err(|e| RemoveError::Store(e.to_string()))?
        .ok_or(RemoveError::Unknown)?;
    let address = stored.address;
    let plan = stored.plan.ok();
    // A plan that no longer reads is still an account with a server: only a readable `Local`
    // is refused.
    if plan
        .as_ref()
        .is_some_and(|plan| !plan.incoming.caps().has_server)
    {
        return Err(RemoveError::Local);
    }
    secrets
        .forget_account(&account)
        .await
        .map_err(|e| RemoveError::Keyring(e.to_string()))?;
    match store
        .remove_account(account)
        .map_err(|e| RemoveError::Store(e.to_string()))?
    {
        None => Err(RemoveError::Unknown),
        Some(freed) => Ok(Removed { address, freed }),
    }
}

#[cfg(test)]
#[path = "remove_tests.rs"]
mod tests;
