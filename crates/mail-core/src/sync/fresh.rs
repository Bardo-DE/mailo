//! Whether an account's mail is as current as its schedule promises, judged by the store's record
//! of the passes. The judging is `mail_logic::fetch::fresh`'s.

use crate::error::Logged;
use mail_domain::WatchMode;
use mail_store::SqliteStore;
use porter_core::AccountId;

pub use mail_logic::fetch::fresh::*;

/// `account`'s pace as the store has it: its recorded finish, and the allowance its stored
/// capabilities give. An account with none yet is taken to poll at the default interval.
pub fn pace_of(store: &SqliteStore, account: AccountId) -> Pace {
    let recorded = store
        .passes(account.clone())
        .or_log("how the account's last pass ended could not be read")
        .and_then(|passes| passes.finished);
    let watch = store
        .account_caps(account)
        .or_log("what the account's server can do could not be read")
        .flatten()
        .map_or(
            WatchMode::Poll {
                every: mail_logic::accounts::IDLE,
            },
            |caps| caps.watch,
        );
    Pace {
        recorded,
        ..Pace::of_watch(&watch)
    }
}
