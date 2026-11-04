//! New mail, said out loud.
//!
//! Three questions, kept apart. *Which* of the messages a pass stored deserve a word is a pure
//! function of each message and a few facts about the user ([`verdict`]). *How many* words a burst
//! gets is another ([`batch`]). *Where* the words go is a seam ([`Notifier`]): the desktop's
//! notification server in the binary, a recorder in the tests, which must never raise a real one.
//!
//! The glue between them, [`announce`], is the only part that reads the store, and it reads it
//! after the pass: the pass applies the server's flags and fetches bodies after it stores a
//! header, so "is this still unread, is it still in the inbox" is the store's current answer and
//! not the one the header fetch built.
//!
//! A notification is the ordinary new-mail banner, the sender and the subject, as every mail
//! app's is. A one-time code in the message is never read into it, never offered on it and never
//! copied on arrival: it shows in a message's open reader, and nowhere a locked screen can read.

pub mod floor;

pub use mail_logic::notify::{
    ARCHIVE_ACTION, Act, DEFAULT_ACTION, Heard, Interest, Notification, Notifier, ONE_BY_ONE, Opens,
    Places, Quiet, REPLY_ACTION, Setting, Sound, Verdict, Whom, act_for, batch, heard, verdict,
};

use crate::error::CoreError;
use crate::error::Logged;
use chrono::{DateTime, Utc};
use mail_domain::{Message, MessageId};
use mail_logic::own::Own;
use mail_store::{SqliteStore, Store as _};
use porter_core::AccountId;
use std::collections::HashSet;
use std::path::Path;

const FILE_NAME: &str = "notify.json";

#[derive(Debug, Default, serde::Deserialize)]
struct Stored {
    #[serde(default)]
    notifications: Setting,
}

/// What `notify.json` said, or on when there is none or it cannot be read. Never written.
pub fn load(dir: &Path) -> Setting {
    crate::config::read_json::<Stored>(dir, FILE_NAME).notifications
}

/// Whether a watch announces what it fetches.
#[derive(Clone, Copy)]
pub enum Announce<'a> {
    /// `mailo sync`, `mailo watch --no-notify`, or notifications turned off.
    Quietly,
    To {
        store: &'a SqliteStore,
        notifier: &'a dyn Notifier,
        /// Whose mail is said, asked as each pass ends: the person's setting changed meanwhile
        /// is followed from the next pass.
        whom: &'a dyn Fn() -> Whom,
    },
}

impl std::fmt::Debug for Announce<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Announce::Quietly => f.write_str("Quietly"),
            Announce::To { .. } => f.write_str("To(..)"),
        }
    }
}

/// Who `whom` means in `store` now: everyone, or the VIPs it holds.
pub fn heard_of(whom: Whom, store: &dyn mail_store::Store) -> Heard {
    match whom {
        Whom::Everyone => Heard::Everyone,
        Whom::Vips => Heard::Only(crate::vip::read(store)),
    }
}

/// Announce what one account's pass stored, returning how many notifications were raised.
///
/// `noted` are the arrivals a rule asked to be told of ([`RuleAction::Notify`]): those are said
/// wherever the rules filed them and whether or not they are read ([`heard`]).
///
/// Arms the account's floor on its first watched pass, whether or not anything arrived — the
/// pass that arms it is the start of "new", and everything dated before it is backfill.
///
/// `whom` is the person's "Notify for": under [`Whom::Vips`] only mail from a VIP is said, the
/// VIPs read from the store as the pass ends, so one marked a moment ago is heard at once.
///
/// [`RuleAction::Notify`]: mail_domain::RuleAction::Notify
pub fn announce(
    store: &SqliteStore,
    account: AccountId,
    arrived: &[MessageId],
    noted: &[MessageId],
    notifier: &dyn Notifier,
    whom: Whom,
    now: DateTime<Utc>,
) -> Result<usize, CoreError> {
    let floor = floor::armed(store, account.clone(), now)?;
    if arrived.is_empty() {
        return Ok(0);
    }
    let interest = notifier.interest();
    if !account_heard(store, &account, &interest)? {
        return Ok(0);
    }
    let own = own_addresses(store)?;
    let heard = heard_of(whom, store);
    let followed = match interest.places {
        Places::Inbox => HashSet::new(),
        Places::Followed => followed_messages(store, &account, arrived)?,
    };
    let mut held = Vec::with_capacity(arrived.len());
    for id in arrived {
        // Gone again already — expunged by the sweep of the same pass. Nothing to announce.
        let Ok(message) = store.message(*id) else {
            continue;
        };
        let snooze = store
            .thread(message.thread)
            .map(|t| t.summary.snooze)
            .or_log_default("a conversation's snooze could not be read");
        held.push((message, snooze));
    }
    let announced: Vec<&Message> = held
        .iter()
        .filter(|(message, snooze)| {
            let followed = followed.contains(&message.id);
            // A rule's Notify is the person's own ask for this message, so whose mail is heard
            // does not apply to it; the floor and the user's own mail still do.
            let said = if noted.contains(&message.id) {
                let asked = verdict(
                    message,
                    *snooze,
                    &own,
                    &Heard::Everyone,
                    floor,
                    now,
                    followed,
                );
                self::heard(asked)
            } else {
                verdict(message, *snooze, &own, &heard, floor, now, followed)
            };
            said == Verdict::Announce
        })
        .map(|(message, _)| message)
        .collect();
    let notifications = batch(account, &announced);
    for notification in &notifications {
        notifier.show(notification);
    }
    Ok(notifications.len())
}

/// Whether `interest` wants to hear about `account`'s mail.
fn account_heard(
    store: &SqliteStore,
    account: &AccountId,
    interest: &Interest,
) -> Result<bool, CoreError> {
    if interest.muted.is_empty() {
        return Ok(true);
    }
    Ok(store
        .list_accounts()?
        .iter()
        .find(|stored| stored.id == *account)
        .is_none_or(|stored| interest.hears(&stored.address)))
}

/// Which of `arrived`, a pass's new messages, are held in a mailbox of `account` that its server
/// lists as followed and that is not the Inbox or a place nobody is waiting on: a folder the
/// person made, or the server's own without a special use. Asked of the arrivals alone, so a
/// pass costs what it brought and not what the followed folders hold.
fn followed_messages(
    store: &SqliteStore,
    account: &AccountId,
    arrived: &[MessageId],
) -> Result<HashSet<MessageId>, CoreError> {
    let mut held = HashSet::new();
    for mailbox in store.followed_folders()? {
        if mailbox.account == *account {
            held.extend(store.addressed_in(&mailbox, arrived)?);
        }
    }
    Ok(held)
}

/// Every address the user sends from: each account's own, and each identity's.
pub fn own_addresses(store: &SqliteStore) -> Result<Own, CoreError> {
    let mut addresses: Vec<String> = Vec::new();
    for account in store.list_accounts()? {
        addresses.push(account.address);
        // An unreadable plan still has its address; the identities are a bonus.
        if let Ok(plan) = account.plan {
            addresses.extend(plan.identities.into_iter().map(|i| i.from.email));
        }
    }
    addresses.extend(store.identity_addresses()?);
    Ok(Own::new(addresses))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support_tests::account;
    use crate::vip::Vips;
    use chrono::TimeZone;
    use mail_domain::{MailboxRole, ReadState, Snooze, ThreadId};

    use mail_domain::{Address, Body, MessageKey, Star};

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 24, hour, 0, 0).unwrap()
    }

    fn message(n: u128, from: (&str, &str), subject: &str) -> Message {
        Message {
            id: MessageId::from_uuid(uuid::Uuid::from_u128(0x9000 + n)),
            thread: ThreadId::from_uuid(uuid::Uuid::from_u128(0x7000 + n)),
            account: account(),
            key: MessageKey::Rfc(format!("m{n}@example.test")),
            date: at(12),
            from: Address {
                name: (!from.0.is_empty()).then(|| from.0.to_owned()),
                email: from.1.to_owned(),
            },
            reply_to: Vec::new(),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: subject.to_owned(),
            in_reply_to: None,
            references: Vec::new(),
            rfc_message_id: None,
            read: ReadState::Unread,
            star: Star::Unstarred,
            flag: mail_domain::Hue::Red,
            mailbox: MailboxRole::Inbox,
            labels: Vec::new(),
            body: Body::Absent,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn the_old_file_is_read_leniently_and_is_on_without_it() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Setting::On, "on by default");
        std::fs::write(dir.path().join(FILE_NAME), r#"{"notifications":"off"}"#).unwrap();
        assert_eq!(load(dir.path()), Setting::Off);
        std::fs::write(dir.path().join(FILE_NAME), b"not json").unwrap();
        assert_eq!(load(dir.path()), Setting::On, "an unreadable file is on");
    }

    #[test]
    fn mail_in_a_followed_mailbox_is_announced_only_when_followed_mailboxes_are_asked_for() {
        let own = Own::default();
        let with = |role: MailboxRole| {
            let mut m = message(1, ("Ada", "ada@example.test"), "lunch");
            m.mailbox = role;
            m
        };
        let ask = |role: MailboxRole, followed: bool| {
            verdict(
                &with(role),
                Snooze::Inactive,
                &own,
                &Heard::Everyone,
                at(9),
                at(13),
                followed,
            )
        };
        let cases = [
            (
                "a followed folder, asked",
                MailboxRole::Archive,
                true,
                Verdict::Announce,
            ),
            (
                "a followed folder, not asked",
                MailboxRole::Archive,
                false,
                Verdict::Quiet(Quiet::Elsewhere(MailboxRole::Archive)),
            ),
            (
                "the inbox either way",
                MailboxRole::Inbox,
                false,
                Verdict::Announce,
            ),
            (
                "junk is never waited on",
                MailboxRole::Spam,
                true,
                Verdict::Quiet(Quiet::Elsewhere(MailboxRole::Spam)),
            ),
            (
                "nor trash",
                MailboxRole::Trash,
                true,
                Verdict::Quiet(Quiet::Elsewhere(MailboxRole::Trash)),
            ),
            (
                "nor sent",
                MailboxRole::Sent,
                true,
                Verdict::Quiet(Quiet::Elsewhere(MailboxRole::Sent)),
            ),
        ];
        for (name, role, followed, expect) in cases {
            assert_eq!(ask(role, followed), expect, "{name}");
        }
    }

    #[test]
    fn vips_only_hears_the_vips_the_store_holds() {
        assert_eq!(
            heard_of(Whom::Vips, &mail_store::MemoryStore::new()),
            Heard::Only(Vips::default()),
            "VIPs only with nobody marked hears nobody"
        );
    }
}
