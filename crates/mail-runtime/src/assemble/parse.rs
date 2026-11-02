//! Stage one of assembly: what a message's bytes say, and nothing about anything else.
//!
//! Pure. No store, no network, no clock but the one time it is told (`Reading::at`), no ids:
//! the same bytes read the same way always give the same [`Parsed`]. What the bytes cannot say
//! (which thread a message belongs to, which blob holds it, what it is called here) is for
//! [`super::resolve`], and writing any of it is for [`super::commit`].

use chrono::{DateTime, Utc};
use mail_crypto::openpgp::{AutocryptHeader, autocrypt_of};
use mail_domain::{MailboxRole, SmimeCert};
use mail_mime::MimeError;

/// Where a batch of bytes came from, which decides what in them may be believed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Provenance {
    /// Straight from a server: every byte is the sender's.
    Wire,
    /// Rebuilt by [`mail_mime::reconstruct`] from sections, with its own markers on the parts
    /// it left on the server. Only this runtime makes these, so only it says so.
    Rebuilt,
}

/// How to read a message.
#[derive(Debug, Clone, Copy)]
pub(super) struct Reading {
    pub provenance: Provenance,
    /// Whether to read what the message teaches about its sender's keys.
    pub learns: bool,
    /// When it arrived: the date of a signature check, and the latest a key's date may claim.
    pub at: DateTime<Utc>,
}

/// Whether mail filed under `role` teaches anything about its sender's keys. Not our own sent
/// or draft copies, whose header is ours.
pub(super) fn learns_from(role: MailboxRole) -> bool {
    !matches!(role, MailboxRole::Sent | MailboxRole::Drafts)
}

/// Everything one message's bytes say about it.
pub(super) struct Parsed {
    pub fields: mail_mime::Parsed,
    /// A digest of the whole message, which is what names its blob before the blob has an id.
    pub digest: blake3::Hash,
    /// Whether the bytes are the message as it came, so that its parts are inside them to be
    /// decoded when wanted and are not stored apart. A message rebuilt from sections is not: its
    /// parts were fetched one by one and the bytes held are not all of them.
    pub whole: bool,
    /// The same, for each attachment held apart from the message ([`mail_mime::ParsedPart::remote`]
    /// is `None` and the message is not [`Parsed::whole`]); `None` for one left on the server or
    /// inside the message's own bytes.
    pub part_digests: Vec<Option<blake3::Hash>>,
    /// The one valid Autocrypt header of a message from a sender, if it had one.
    pub autocrypt: Option<AutocryptHeader>,
    /// The certificate the message's signature proved its sender holds.
    pub signer: Option<SmimeCert>,
}

/// Read `raw`. An error is a message that cannot be read at all, which is rare: a missing or
/// broken header is a fact about the message and not an error.
pub(super) fn parse(raw: &[u8], how: Reading) -> Result<Parsed, MimeError> {
    let fields = match how.provenance {
        Provenance::Wire => mail_mime::parse(raw),
        Provenance::Rebuilt => mail_mime::parse_reconstructed(raw),
    }?;
    let (mut autocrypt, mut signer) = (None, None);
    // Autocrypt keys ride in on the header of mail their owners send, read from the header
    // alone, and an S/MIME signature carries its signer's certificate, which is what lets the
    // person answer encrypted. Nothing is decrypted.
    if how.learns
        && let Some(from) = &fields.from
    {
        let address = from.email.trim();
        if !address.is_empty() {
            autocrypt = autocrypt_of(raw, address);
        }
        signer = crate::smime::signer_of(raw, &from.email, how.at);
    }
    let whole = matches!(how.provenance, Provenance::Wire);
    let part_digests = fields
        .attachments
        .iter()
        .map(|part| (part.remote.is_none() && !whole).then(|| blake3::hash(&part.bytes)))
        .collect();
    Ok(Parsed {
        digest: blake3::hash(raw),
        whole,
        part_digests,
        fields,
        autocrypt,
        signer,
    })
}
