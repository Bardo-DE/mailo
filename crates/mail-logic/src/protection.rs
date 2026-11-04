//! What a message's protection came to, as values: which scheme it carries, what was checked of
//! its signature, whether it could be opened, and the passphrase seam a locked key is asked
//! through. Checking and opening are `mail_core::pgp` and `mail_core::smime`'s.
//!
//! The answers are typed: which protection a message carries, what was checked, the body it was
//! opened to. The words for them are the front-end's.

use mail_domain::{
    Encryption, Fingerprint, PgpKey, SmimeCert, SmimeEncryption, SmimeVerification, Verification,
};
use mail_mime::Parsed;

/// Asked for the passphrase of the user's key with this fingerprint, when a protected key has
/// to sign or decrypt. `None` is "not given": the send is refused or the message stays locked.
///
/// The command line asks on the terminal; the window asks in the window; a test
/// answers from a table. Nothing is asked for a key that has no passphrase.
pub type Ask<'a> = &'a dyn Fn(Fingerprint) -> Option<String>;

/// The [`Ask`] that never has an answer: for callers with nobody to ask.
pub fn no_passphrase(_: Fingerprint) -> Option<String> {
    None
}

/// Whether deleting a key with a secret half was confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WithSecret {
    /// Delete only a key the keyring holds no secret for.
    Refuse,
    /// `--with-secret`: the secret half goes too.
    Confirmed,
}

/// Which protection a message carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    OpenPgp,
    Smime,
}

/// Whether a passphrase was given, when a key stayed locked. `ask` is only called for a
/// protected key, so a key still locked after one was given was given the wrong one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tried {
    /// None was given: it needs one.
    Nothing,
    /// One was given, and it did not unlock the key.
    Wrong,
}

/// What was checked about a message, by the protection it carries. The opened body and the bytes
/// it was parsed from are not in here: they are [`Sealed::shown`], once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Protection {
    OpenPgp(Box<pgp::Protected>),
    Smime(Box<smime::Protected>),
}

/// A message opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
    pub protection: Protection,
    /// The decrypted or unwrapped message, parsed. `None` when there is nothing to show but what
    /// is stored: it could not be read.
    pub shown: Option<Box<Parsed>>,
}

/// What opening a message found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opening {
    /// It carries no OpenPGP or S/MIME, or only its headers are here.
    Plain,
    Opened(Box<Sealed>),
    /// Encrypted to the user's OpenPGP key `key`, whose passphrase is needed.
    Locked {
        key: Fingerprint,
        tried: Tried,
    },
    /// Opening it failed: `why` is the error as the protocol's module words it.
    Failed {
        scheme: Scheme,
        why: String,
    },
}

/// OpenPGP's reading of a message.
pub mod pgp {
    use super::*;

    /// A message opened for reading.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Protected {
        pub encryption: Encryption,
        pub verification: Verification,
        /// The key that made a good signature, as the store holds it: its addresses and trust.
        pub signer: Option<PgpKey>,
        /// The message as the reader shows it — decrypted, protected headers applied, signature
        /// part taken off — parsed the way every message is. `None` when it could not be opened
        /// ([`Encryption::CannotDecrypt`], [`Encryption::Locked`], [`Encryption::Unreadable`]):
        /// the reader shows what it already has.
        pub shown: Option<Parsed>,
        /// The bytes `shown` was parsed from. In memory only; not to be stored.
        pub raw: Option<Vec<u8>>,
    }
}

/// S/MIME's reading of a message.
pub mod smime {
    use super::*;

    /// A message opened for reading. The same shape as [`pgp::Protected`], so one badge can
    /// show either.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Protected {
        pub encryption: SmimeEncryption,
        pub verification: SmimeVerification,
        /// The certificate that made the signature, as the store holds it (or would, for one it has
        /// not kept): its subject, issuer, dates and addresses. `None` when there was no signature
        /// or no certificate for it.
        pub signer: Option<SmimeCert>,
        /// The message as the reader shows it — decrypted, signature part taken off — parsed the way
        /// every message is. `None` when it could not be opened.
        pub shown: Option<Parsed>,
        /// The bytes `shown` was parsed from. In memory only; not to be stored.
        pub raw: Option<Vec<u8>>,
    }
}
