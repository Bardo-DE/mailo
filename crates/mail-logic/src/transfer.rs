//! What Import and Export do, as values: where imported mail goes, what an import and an export
//! did, why a typed path cannot be imported, where mail can be uploaded and the head of a
//! message's source. Doing it, against the store and the disk, is `mail_core::import`,
//! `mail_core::export` and `mail_core::transfer`.

use porter_core::AccountId;
use std::io::ErrorKind;

/// Where imported mail goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// The local-only account.
    Local,
    /// A mailbox on an IMAP account, by the account's address and the folder's path.
    Mailbox { account: String, folder: String },
}

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Imported {
    /// Messages read out of the source.
    pub read: usize,
    /// New here: kept locally, or queued for upload.
    pub added: usize,
    /// Already held (or already queued), by identity.
    pub already: usize,
    /// Bytes that were not a message.
    pub unreadable: usize,
    /// Read from Apple Mail's `.partial.emlx` files: imported without the attachments stored
    /// beside them.
    pub partial: usize,
    /// Thunderbird messages of which only a part of the body was kept, and so left out.
    pub cut_off: usize,
}

/// What an export did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exported {
    pub written: usize,
    /// Only headers are here yet: the body was never downloaded.
    pub absent: usize,
    /// The body is here, but it was rebuilt from its parts with some attachment left on the
    /// server, so the stored bytes are not the message as sent.
    pub partial: usize,
}

/// The first bytes of a message's source, and how long the whole of it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawHead {
    /// At most the `limit` asked for.
    pub bytes: Vec<u8>,
    /// The size of the whole source.
    pub whole: u64,
}

/// Why the operating system would not do something with a path: its kind, and its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoCause {
    pub kind: ErrorKind,
    pub text: String,
}

impl IoCause {
    pub fn of(error: &std::io::Error) -> Self {
        Self {
            kind: error.kind(),
            text: error.to_string(),
        }
    }
}

/// Why a typed path cannot be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// There is nothing at the path.
    NothingThere,
    /// Something is there, and it cannot be looked at.
    CannotReach(IoCause),
    /// A file that cannot be opened.
    CannotOpen(IoCause),
    /// A file with nothing in it.
    Empty,
    /// It is not mail, or not mail that can be read: the import's own words.
    Unreadable(String),
}

/// An IMAP account's folder that imported mail can be uploaded into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub account: AccountId,
    pub address: String,
    pub folder: String,
}
