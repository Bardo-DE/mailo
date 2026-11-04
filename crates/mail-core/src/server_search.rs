//! Searching the server from a search typed here: which accounts can, whether it runs by itself,
//! and the typed line as the filter a server is asked for.
//!
//! A search runs over the store. When it is shown, the list's end offers to search each account
//! in view on its server (`ui::server_search`), which finds what the store never held. It is
//! asked, not automatic, unless the user turned "Search the server automatically" on: a server
//! search is a connection and a query against someone else's machine, and a keystroke should
//! not make one. The switch is the window's (`search.server_automatically` in mail-app's
//! `mailo/settings.toml`; `server-search.json` before it), and off unless turned on.
//!
//! What a server is asked is the line exactly as the store is asked it, parsed by the same
//! [`mail_logic::query::parse_with`], except that `label:` resolves against the one account being
//! searched — a label is a name on one server — and a `re:/…/` pattern, which no server
//! matches, is said rather than dropped. Each protocol's translation is `mail_proto::search`'s;
//! the engines are `mail_runtime`'s ([`crate::SyncOps::search_server`]).

use crate::config::read_json;

pub use mail_logic::server_search::{Automatic, filter_of, needs_server};
use crate::error::CoreError;
use mail_domain::LabelId;
use mail_runtime::Searched;
use porter_core::AccountId;
use std::path::Path;

const FILE_NAME: &str = "server-search.json";

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Stored {
    #[serde(default)]
    automatic: Automatic,
}

/// What `server-search.json` said, or off when there is none or it cannot be read. Never
/// written: mail-app keeps the switch (`search.server_automatically`) and read this once.
pub fn load(dir: &Path) -> Automatic {
    read_json::<Stored>(dir, FILE_NAME).automatic
}

/// Search `account`'s server for the line `input`, and keep what it finds.
pub async fn search(
    mail: &crate::Mail,
    account: AccountId,
    input: &str,
) -> Result<Searched, CoreError> {
    let named: Vec<(String, LabelId)> = mail
        .store()
        .labels(account.clone())?
        .into_iter()
        .map(|l| (l.name, l.id))
        .collect();
    let accounts = crate::query::known_accounts(mail.store());
    let filter = match filter_of(
        input,
        &named,
        &accounts,
        &crate::vip::read(mail.store().as_ref()),
    ) {
        Ok(filter) => filter,
        Err(unsaid) => return Ok(Searched::Unsaid(unsaid)),
    };
    let labels: Vec<(LabelId, String)> = named.into_iter().map(|(n, id)| (id, n)).collect();
    mail.sync().search_server(account, &filter, &labels).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_until_turned_on() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Automatic::Off);
        std::fs::write(dir.path().join(FILE_NAME), r#"{"automatic":"on"}"#).unwrap();
        assert_eq!(load(dir.path()), Automatic::On);
    }

}
