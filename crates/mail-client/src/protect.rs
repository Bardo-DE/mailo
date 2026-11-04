//! Why a protected message could not be sent or a key could not be kept, as the client says it: the
//! few refusals a window answers in its own way each have a variant, and anything else is said in
//! its own words ([`CallError`]).
//!
//! Each is what a call on the client returns in place of the core's error of the same name, which
//! also holds the store's and the keyring's failures and so is not a caller's to name.

use crate::{CallError, Fault};
use mail_domain::Fingerprint;
use mail_logic::remedy::Remedy;

/// `said` as a failure of no kind in particular, mended by `remedy` when there is one.
fn told(said: String, remedy: Option<Remedy>) -> CallError {
    let failure = CallError::new(Fault::Other, said);
    match remedy {
        Some(remedy) => failure.mended_by(remedy),
        None => failure,
    }
}

/// Why an OpenPGP step could not be done.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PgpError {
    /// Encryption was asked for and these recipients have no key to encrypt to.
    #[error(
        "no OpenPGP key for {}; nothing was sent. Find or import one, or send without encrypting",
        .0.join(", ")
    )]
    NoKeyFor(Vec<String>),
    /// Encryption was asked for with blind recipients, whose key ids every recipient would see.
    #[error(
        "an encrypted message cannot have Bcc recipients ({}): every recipient would see the \
         blind ones' key ids. Send them a separate message",
        .0.join(", ")
    )]
    BlindRecipients(Vec<String>),
    /// Signing or encrypting was asked for from an identity with no key of its own.
    #[error("{0} has no OpenPGP key")]
    NoOwnKey(String),
    /// A protected key's passphrase was not given, or was wrong.
    #[error("the OpenPGP key {0} needs its passphrase; none was given, or it was wrong")]
    Locked(Fingerprint),
    /// Anything else.
    #[error("{0}")]
    Other(CallError),
}

impl From<mail_core::pgp::PgpError> for PgpError {
    fn from(error: mail_core::pgp::PgpError) -> Self {
        use mail_core::pgp::PgpError as Core;
        match error {
            Core::NoKeyFor(addresses) => PgpError::NoKeyFor(addresses),
            Core::BlindRecipients(addresses) => PgpError::BlindRecipients(addresses),
            Core::NoOwnKey(address) => PgpError::NoOwnKey(address),
            Core::Locked(fingerprint) => PgpError::Locked(fingerprint),
            other => PgpError::Other(other.into()),
        }
    }
}

impl From<PgpError> for CallError {
    fn from(error: PgpError) -> Self {
        let remedy = match &error {
            PgpError::NoKeyFor(_) => Some(Remedy::FindPgpKey),
            PgpError::NoOwnKey(address) => Some(Remedy::MakePgpKey {
                address: address.clone(),
            }),
            _ => None,
        };
        match error {
            PgpError::Other(other) => other,
            typed => told(typed.to_string(), remedy),
        }
    }
}

impl From<PgpError> for String {
    fn from(error: PgpError) -> Self {
        error.to_string()
    }
}

/// Why an S/MIME step could not be done.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SmimeError {
    /// Encryption was asked for and these recipients have no certificate to encrypt to.
    #[error(
        "no S/MIME certificate to encrypt to for {}; nothing was sent. A signed message from \
         them brings theirs, or import one; or send without encrypting",
        .0.join(", ")
    )]
    NoCertFor(Vec<String>),
    /// Encryption was asked for with blind recipients, whose certificates every recipient would
    /// see named.
    #[error(
        "an encrypted message cannot have Bcc recipients ({}): every recipient would see the \
         blind ones' certificates named. Send them a separate message",
        .0.join(", ")
    )]
    BlindRecipients(Vec<String>),
    /// Signing or encrypting was asked for from an identity with no current certificate.
    #[error("{0} has no current S/MIME certificate of its own; import your identity")]
    NoOwnCert(String),
    /// The user's own certificate has a key mail cannot be encrypted to here.
    #[error(
        "your S/MIME certificate for {0} cannot be encrypted to (only RSA certificates for mail \
         can), so the sent copy could not be read; send without --encrypt"
    )]
    OwnCertCannotEncrypt(String),
    /// A PKCS#12 file was given without its password.
    #[error(
        "that PKCS#12 file needs its password: set MAILO_SMIME_PASSWORD, or run from a terminal \
         to be asked"
    )]
    NoPassword,
    /// Anything else.
    #[error("{0}")]
    Other(CallError),
}

impl From<mail_core::smime::SmimeError> for SmimeError {
    fn from(error: mail_core::smime::SmimeError) -> Self {
        use mail_core::smime::SmimeError as Core;
        match error {
            Core::NoCertFor(addresses) => SmimeError::NoCertFor(addresses),
            Core::BlindRecipients(addresses) => SmimeError::BlindRecipients(addresses),
            Core::NoOwnCert(address) => SmimeError::NoOwnCert(address),
            Core::OwnCertCannotEncrypt(address) => SmimeError::OwnCertCannotEncrypt(address),
            Core::NoPassword => SmimeError::NoPassword,
            other => SmimeError::Other(other.into()),
        }
    }
}

impl From<SmimeError> for CallError {
    fn from(error: SmimeError) -> Self {
        let remedy = match &error {
            SmimeError::NoCertFor(_) => Some(Remedy::ImportCertificate),
            SmimeError::NoOwnCert(_) => Some(Remedy::ImportOwnCertificate),
            _ => None,
        };
        match error {
            SmimeError::Other(other) => other,
            typed => told(typed.to_string(), remedy),
        }
    }
}

impl From<SmimeError> for String {
    fn from(error: SmimeError) -> Self {
        error.to_string()
    }
}

/// Why a send was refused, typed so a caller can act on it: an OpenPGP key that needs its
/// passphrase ([`SendError::locked`]) is asked for and the send tried again.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    #[error("{0}")]
    Pgp(PgpError),
    #[error("{0}")]
    Smime(SmimeError),
    /// Anything else: the draft already sent, no server to send from, the store.
    #[error("{0}")]
    Other(CallError),
}

impl From<mail_core::compose::SendError> for SendError {
    fn from(error: mail_core::compose::SendError) -> Self {
        use mail_core::compose::SendError as Core;
        match error {
            Core::Pgp(e) => SendError::Pgp(e.into()),
            Core::Smime(e) => SendError::Smime(e.into()),
            Core::Other(e) => SendError::Other(e.into()),
        }
    }
}

impl From<SendError> for CallError {
    fn from(error: SendError) -> Self {
        match error {
            SendError::Pgp(e) => e.into(),
            SendError::Smime(e) => e.into(),
            SendError::Other(e) => e,
        }
    }
}

impl From<SendError> for String {
    fn from(error: SendError) -> Self {
        error.to_string()
    }
}

impl SendError {
    /// The OpenPGP key whose passphrase was not given, or was wrong, when that is why.
    pub fn locked(&self) -> Option<Fingerprint> {
        match self {
            SendError::Pgp(PgpError::Locked(fingerprint)) => Some(*fingerprint),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_core::pgp::PgpError as CorePgp;
    use mail_core::smime::SmimeError as CoreSmime;

    /// The words are the core's, whichever way the error is held.
    #[test]
    fn the_words_are_the_cores() {
        let fingerprint = Fingerprint::V4([7; 20]);
        let addresses = vec!["ada@example.test".to_owned(), "bob@example.test".to_owned()];
        let pgp = [
            CorePgp::NoKeyFor(addresses.clone()),
            CorePgp::BlindRecipients(addresses.clone()),
            CorePgp::NoOwnKey("ada@example.test".to_owned()),
            CorePgp::Locked(fingerprint),
            CorePgp::NoKey("x".to_owned()),
        ];
        for core in pgp {
            let said = core.to_string();
            assert_eq!(PgpError::from(core).to_string(), said);
        }
        let smime = [
            CoreSmime::NoCertFor(addresses.clone()),
            CoreSmime::BlindRecipients(addresses),
            CoreSmime::NoOwnCert("ada@example.test".to_owned()),
            CoreSmime::OwnCertCannotEncrypt("ada@example.test".to_owned()),
            CoreSmime::NoPassword,
            CoreSmime::NoCert("x".to_owned()),
        ];
        for core in smime {
            let said = core.to_string();
            assert_eq!(SmimeError::from(core).to_string(), said);
        }
    }

    #[test]
    fn a_locked_key_is_told_through_a_send() {
        let fingerprint = Fingerprint::V4([7; 20]);
        let refused = SendError::from(mail_core::compose::SendError::Pgp(CorePgp::Locked(
            fingerprint,
        )));
        assert_eq!(refused.locked(), Some(fingerprint));
        assert_eq!(
            SendError::Smime(SmimeError::NoPassword).locked(),
            None,
            "only a locked OpenPGP key is one to unlock"
        );
    }
}
