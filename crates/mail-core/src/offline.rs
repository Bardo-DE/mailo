//! How each account is synced: whether all its mail is kept on this computer, large attachments
//! included, how often a timed pass checks it, how far back its mail is downloaded, and, for
//! POP3, when the server's copy of a message is removed.
//!
//! A sync fetches every message, but a large IMAP message only as its text: its attachments stay
//! on the server until someone opens one. An account kept offline in full has
//! them fetched too, a few each pass, after the bodies and largest last. Off unless asked for.
//!
//! The other three are the account page's in Settings, beside Download All Mail, and are kept the
//! same way: [`Every`] is what the window's scheduler and `mailo watch` time passes by
//! ([`crate::sync::due`]), [`Window`] how old a message may be and still have its body fetched by
//! a pass, and [`Remove`] when a POP3 pass sends `DELE` for what it holds here. Each account that
//! has never been set is the default, which is how every account synced before these existed.
//!
//! Preferences, so they live beside the others in the config directory (`offline.json`), not in
//! the mail database and not in the account's plan, which is a frozen domain type. Keyed by
//! account id, as `spaces.json` keys whose mail a Space shows; an id the database no longer has
//! is simply never asked about.

use crate::config::{json_damage, read_json, write_json};
use crate::error::CoreError;
use mail_store::{Offline, Store};
use porter_core::AccountId;
use std::path::Path;

pub use mail_logic::offline::{Every, Keep, Kept, Remove, Window};

/// The file the setting is kept in, in the config directory.
pub const FILE_NAME: &str = "offline.json";

/// The stored setting, or every account off when there is none or it cannot be read. The window
/// and the command line ask about a file that cannot be read ([`damage`]) before anything runs
/// over it.
pub fn load(dir: &Path) -> Kept {
    read_json(dir, FILE_NAME)
}

/// Why `offline.json` in `dir` cannot be read: `None` when it is missing or reads.
pub fn damage(dir: &Path) -> Option<String> {
    json_damage::<Kept>(dir, FILE_NAME)
}

/// Set what `account` keeps, leaving every other account as it was. A file that cannot be read is
/// left as it is, and nothing is set: writing would put the defaults in place of what it held.
pub fn save(dir: &Path, account: AccountId, keep: Keep) -> Result<(), CoreError> {
    change(dir, |kept| kept.with(account, keep))
}

/// Write the setting in `dir` as `edit` leaves it, every other account as it was; [`save`]'s
/// rule for a file that cannot be read holds for every choice.
pub fn change(dir: &Path, edit: impl FnOnce(Kept) -> Kept) -> Result<(), CoreError> {
    if let Some(why) = damage(dir) {
        return Err(CoreError::Damaged {
            path: dir.join(FILE_NAME),
            why,
        });
    }
    write_json(dir, FILE_NAME, &edit(load(dir)))
}

/// "1,204 of 1,310 messages offline", and what still waits on the server when anything does.
pub fn said(offline: &Offline) -> String {
    let mut out = format!(
        "{} of {} messages offline",
        grouped(offline.held),
        grouped(offline.messages)
    );
    if offline.parts_remote > 0 {
        out.push_str(&format!(
            "; {} {} ({}) on the server",
            grouped(offline.parts_remote),
            if offline.parts_remote == 1 {
                "attachment"
            } else {
                "attachments"
            },
            crate::attach::human_size(offline.remote_bytes)
        ));
    }
    out
}

/// `1310` as `1,310`.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Where one account stands: what it keeps, and how much of it is here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    pub address: String,
    pub keep: Keep,
    pub counted: Offline,
}

/// Set what one account keeps (when `set` is given), then say where each stands: the account
/// whose address is `address` (any case), or every one when there is none.
///
/// `dir` is `None` when there is no home directory to keep the setting in; then only a request
/// to change it fails.
pub fn standing(
    dir: Option<&Path>,
    store: &dyn Store,
    accounts: &[(AccountId, String)],
    address: Option<&str>,
    set: Option<Keep>,
) -> Result<Vec<Standing>, CoreError> {
    let chosen: Vec<&(AccountId, String)> = match address {
        None => accounts.iter().collect(),
        Some(address) => {
            let found: Vec<_> = accounts
                .iter()
                .filter(|(_, a)| a.eq_ignore_ascii_case(address))
                .collect();
            if found.is_empty() {
                return Err(CoreError::UnknownAccount(address.to_owned()));
            }
            found
        }
    };
    if let Some(keep) = set {
        let Some(dir) = dir else {
            return Err(CoreError::NoConfigDir);
        };
        for (id, _) in &chosen {
            save(dir, id.clone(), keep)?;
        }
    }
    let kept = dir.map(load).unwrap_or_default();
    let mut out = Vec::new();
    for (id, address) in chosen {
        out.push(Standing {
            address: address.clone(),
            keep: kept.of(id.clone()),
            counted: store.offline(id.clone())?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::id::account_id_from_uuid;

    fn acct_ada() -> AccountId {
        account_id_from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000a1"))
    }
    fn acct_bea() -> AccountId {
        account_id_from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000a2"))
    }

    #[test]
    fn off_until_turned_on_and_one_account_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(dir.path()).of(acct_ada()),
            Keep::Bodies,
            "off by default"
        );

        save(dir.path(), acct_ada(), Keep::Everything).unwrap();
        let kept = load(dir.path());
        assert_eq!(kept.of(acct_ada()), Keep::Everything);
        assert_eq!(
            kept.of(acct_bea()),
            Keep::Bodies,
            "the other account is untouched"
        );

        save(dir.path(), acct_bea(), Keep::Everything).unwrap();
        save(dir.path(), acct_ada(), Keep::Bodies).unwrap();
        let kept = load(dir.path());
        assert_eq!(
            (kept.of(acct_ada()), kept.of(acct_bea())),
            (Keep::Bodies, Keep::Everything)
        );
    }

    /// Each account's choices are its own, a default is left out of the file, and a file written
    /// before they existed reads as every account at the defaults.
    #[test]
    fn each_choice_is_one_accounts_and_a_default_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        change(dir.path(), |kept| {
            kept.with_every(acct_ada(), Every::Minutes(15))
                .with_window(acct_ada(), Window::Days(90))
                .with_remove(acct_bea(), Remove::AfterDays(7))
        })
        .unwrap();
        let kept = load(dir.path());
        assert_eq!(
            (
                kept.every_of(&acct_ada()),
                kept.window_of(&acct_ada()),
                kept.remove_of(&acct_ada())
            ),
            (Every::Minutes(15), Window::Days(90), Remove::Never)
        );
        assert_eq!(
            (
                kept.every_of(&acct_bea()),
                kept.window_of(&acct_bea()),
                kept.remove_of(&acct_bea())
            ),
            (Every::Automatic, Window::All, Remove::AfterDays(7))
        );

        change(dir.path(), |kept| {
            kept.without(acct_ada()).without(acct_bea())
        })
        .unwrap();
        let written = std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();
        assert_eq!(
            written.trim(),
            r#"{"everything":[]}"#,
            "the defaults are not written"
        );

        std::fs::write(
            dir.path().join(FILE_NAME),
            r#"{"everything":["00000000-0000-4000-8000-0000000000a1"]}"#,
        )
        .unwrap();
        let older = load(dir.path());
        assert_eq!(older.of(acct_ada()), Keep::Everything);
        assert_eq!(older.every_of(&acct_ada()), Every::Automatic);
    }

    #[test]
    fn a_damaged_file_is_every_account_off() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), b"{not json").unwrap();
        assert_eq!(load(dir.path()).of(acct_ada()), Keep::Bodies);
    }

    #[test]
    fn a_damaged_file_is_said_and_never_written_over() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(damage(dir.path()), None, "no file is not damage");
        std::fs::write(dir.path().join(FILE_NAME), b"{not json").unwrap();
        assert!(damage(dir.path()).is_some());
        assert!(matches!(
            save(dir.path(), acct_ada(), Keep::Everything),
            Err(CoreError::Damaged { .. })
        ));
        assert_eq!(
            std::fs::read(dir.path().join(FILE_NAME)).unwrap(),
            b"{not json"
        );
    }

    #[test]
    fn the_count_reads_as_a_sentence() {
        const CASES: &[(Offline, &str)] = &[
            (
                Offline {
                    messages: 1310,
                    held: 1204,
                    parts_remote: 0,
                    remote_bytes: 0,
                },
                "1,204 of 1,310 messages offline",
            ),
            (
                Offline {
                    messages: 3,
                    held: 2,
                    parts_remote: 1,
                    remote_bytes: 2 * 1024 * 1024,
                },
                "2 of 3 messages offline; 1 attachment (2.0 MB) on the server",
            ),
        ];
        for (offline, want) in CASES {
            assert_eq!(said(offline), *want, "{offline:?}");
        }
    }
}
