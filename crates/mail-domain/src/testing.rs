//! The values a test starts from, so it states only what it is about.
//!
//! A test of a list row or a filter needs a whole [`ThreadSummary`], and nearly every field of it
//! is beside the point. Each test used to spell all of them out; now it writes
//! `ThreadSummary { subject, read, ..summary() }`. The defaults are an unread, unstarred
//! conversation of one message in the Inbox, with every thread-level setting off.
//!
//! [`account`] and [`now`] are fixed rather than fresh, so two calls in one test name the same
//! account and the same instant.

use crate::account::Identity;
use crate::address::Address;
use crate::id::{IdentityId, ThreadId, account_id_from_uuid, new_account_id};
use crate::message::ThreadSummary;
use crate::state::{
    Attachments, FollowUp, IsDefault, MailboxRole, MailboxSet, Mute, Pin, ReadState, Snooze, Star,
};
use chrono::{DateTime, TimeZone, Utc};
use porter_core::AccountId;
use uuid::Uuid;

/// The one account a test's records belong to.
pub fn account() -> AccountId {
    account_id_from_uuid(Uuid::from_u128(0xa1))
}

/// The instant a test's clock reads: 2023-11-14 22:13:20 UTC.
pub fn now() -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap()
}

/// `secs` seconds after [`now`].
pub fn at(secs: i64) -> DateTime<Utc> {
    now() + chrono::TimeDelta::seconds(secs)
}

/// An unread, unstarred one-message conversation in the Inbox, from `a@example.test`, on an
/// account of its own.
pub fn summary() -> ThreadSummary {
    ThreadSummary {
        id: ThreadId::generate(),
        account: new_account_id(),
        subject: "s".into(),
        snippet: String::new(),
        from: Address {
            name: None,
            email: "a@example.test".into(),
        },
        participants: vec![],
        recipients: vec![],
        last_date: now(),
        message_count: 1,
        read: ReadState::Unread,
        star: Star::Unstarred,
        flag: crate::Hue::Red,
        mailboxes: MailboxSet::only(MailboxRole::Inbox),
        labels: vec![],
        attachments: Attachments::None,
        snooze: Snooze::Inactive,
        pin: Pin::Unpinned,
        mute: Mute::Unmuted,
        follow_up: FollowUp::Inactive,
    }
}

/// The default identity of an account of its own, `me@example.test`, with no signature.
pub fn identity() -> Identity {
    Identity {
        id: IdentityId::generate(),
        account: new_account_id(),
        from: Address {
            name: None,
            email: "me@example.test".into(),
        },
        reply_to: None,
        signature: None,
        default: IsDefault::Default,
    }
}

/// A conversation as a search test builds one: thread number `n` (`ThreadId::from_uuid(n)`), on
/// [`account`], from `email`, last written `secs` seconds after [`now`].
pub fn listed(
    n: u128,
    subject: &str,
    snippet: &str,
    email: &str,
    secs: i64,
    read: ReadState,
    star: Star,
) -> ThreadSummary {
    ThreadSummary {
        id: ThreadId::from_uuid(Uuid::from_u128(n)),
        account: account(),
        subject: subject.to_owned(),
        snippet: snippet.to_owned(),
        from: Address {
            name: None,
            email: email.to_owned(),
        },
        last_date: at(secs),
        read,
        star,
        ..summary()
    }
}

/// Message number `n` in the Inbox of [`account`], from `from`, unread, with a stored body of
/// `text`; its ids come from `n`, so two numbers are two messages and one number is one.
pub fn message(n: u128, from: &str, subject: &str, text: &str) -> crate::Message {
    use crate::{BlobId, Body, MailboxRole, Message, MessageId, MessageKey};
    Message {
        id: MessageId::from_uuid(Uuid::from_u128(0x9000 + n)),
        thread: ThreadId::from_uuid(Uuid::from_u128(0x7000 + n)),
        account: account(),
        key: MessageKey::Rfc(format!("m{n}@example.test")),
        date: at(n as i64),
        from: Address::new(from),
        reply_to: vec![],
        to: vec![Address::named("Ada Lovelace", "ada@example.test")],
        cc: vec![],
        bcc: vec![],
        subject: subject.to_owned(),
        in_reply_to: None,
        references: vec![],
        rfc_message_id: None,
        read: ReadState::Unread,
        star: Star::Unstarred,
        flag: crate::Hue::Red,
        mailbox: MailboxRole::Inbox,
        labels: vec![],
        body: Body::Present {
            text: Some(text.to_owned()),
            raw: BlobId::from_uuid(Uuid::from_u128(0xB000 + n)),
        },
        attachments: vec![],
    }
}
