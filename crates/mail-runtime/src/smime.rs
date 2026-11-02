//! S/MIME's effects: private keys in and out of the OS keyring, the operating system's trust
//! anchors, and correspondents' certificates learnt from their signed mail.
//!
//! The cryptography is `mail_crypto::smime`'s and is pure. What happens here is what it cannot do:
//! read and write the keyring, read the system's certificate bundle, and write to the store what
//! arriving mail taught. Nothing here decrypts anything — mail is decrypted when the reader opens
//! it, never as it arrives.

use crate::RuntimeError;
use chrono::{DateTime, Utc};
use mail_crypto::smime::{self, Cert, Keys, PrivateKey};
use mail_domain::signing::{SigningKeyId, SigningKeyRef, SigningSecret};
use mail_domain::{CertFingerprint, CertProblem, CertSource, SmimeVerification};
use mail_net::Failure;
use mail_secrets::SigningStore;
use mail_store::Store;
use porter_core::AccountId;
use std::sync::OnceLock;

fn entry(account: AccountId, fingerprint: CertFingerprint) -> SigningKeyRef {
    SigningKeyRef {
        account,
        key: SigningKeyId::Smime(fingerprint),
    }
}

/// The private key of the certificate with `fingerprint`, from the keyring.
pub fn private_key(
    secrets: &dyn SigningStore,
    account: AccountId,
    fingerprint: CertFingerprint,
) -> Result<PrivateKey, RuntimeError> {
    match secrets.get(&entry(account, fingerprint))? {
        SigningSecret::SmimeKey(pem) => {
            let pem = zeroize::Zeroizing::new(pem);
            Ok(PrivateKey::from_pkcs8_pem(&pem)?)
        }
        SigningSecret::OpenPgp(_) | SigningSecret::Relay(_) => {
            Err(RuntimeError::Secrets(Failure::said(format!(
                "the keyring entry for S/MIME certificate {fingerprint} holds something else"
            ))))
        }
    }
}

/// Keep the private key of the certificate with `fingerprint` in the keyring.
pub fn keep_private_key(
    secrets: &dyn SigningStore,
    account: AccountId,
    fingerprint: CertFingerprint,
    key: &PrivateKey,
) -> Result<(), RuntimeError> {
    let pem = key.to_pkcs8_pem()?;
    Ok(secrets.put(
        &entry(account, fingerprint),
        &SigningSecret::SmimeKey(pem.as_str().to_owned()),
    )?)
}

/// Remove the private key of `fingerprint` from the keyring. Already gone is success.
pub fn forget_private_key(
    secrets: &dyn SigningStore,
    account: AccountId,
    fingerprint: CertFingerprint,
) -> Result<(), RuntimeError> {
    Ok(secrets.forget(&entry(account, fingerprint))?)
}

/// The operating system's certificate authorities, read once per process.
///
/// What `rustls-native-certs` finds: on Linux the bundle OpenSSL would read (or the one
/// `SSL_CERT_FILE` names), the system store on macOS and Windows. The bundle does not say which
/// authorities are trusted for mail as opposed to web servers, so every one is taken as an
/// anchor for both. A certificate that cannot be parsed is skipped; a machine with no bundle has
/// no system anchors, and only certificates the user trusts are believed.
pub fn system_anchors() -> &'static [Cert] {
    static ANCHORS: OnceLock<Vec<Cert>> = OnceLock::new();
    ANCHORS.get_or_init(|| {
        mail_net::roots::system()
            .iter()
            .filter_map(|der| Cert::from_der(der.as_ref()).ok())
            .collect()
    })
}

/// The certificate a signed message from `from` was signed with, as a row to keep, when its
/// signature holds and the certificate names `from`; `arrived` is when it was seen.
///
/// Whether the certificate is to be believed is not decided here — that is asked each time it
/// is used, against the trust anchors of that moment — but a certificate is kept for an address
/// only when mail from that address proved it holds the key. Reads only what arrives in the
/// clear: a signature inside an encrypted message is learnt when the reader opens it. Pure: it
/// reads the bytes and writes nothing.
pub fn signer_of(raw: &[u8], from: &str, arrived: DateTime<Utc>) -> Option<mail_domain::SmimeCert> {
    let opened = smime::open(
        raw,
        &Keys {
            identities: &[],
            certs: &[],
            anchors: &[],
            now: arrived,
        },
    )?;
    proven_signer(&opened, from, arrived)
}

/// Keep what an opened message's signature taught, under the rule [`signer_of`] states.
pub fn keep_signer(
    store: &dyn Store,
    opened: &smime::Opened,
    from: &str,
    seen: DateTime<Utc>,
) -> Result<(), RuntimeError> {
    let Some(cert) = proven_signer(opened, from, seen) else {
        return Ok(());
    };
    let fingerprint = cert.fingerprint;
    let before = store.smime_cert(fingerprint)?;
    let after = store.put_smime_cert(cert)?;
    if before.is_none_or(|b| b.chain != after.chain || b.emails != after.emails) {
        crate::epoch::keys_changed();
    }
    Ok(())
}

/// The signer's certificate as a row, when the signature holds and the certificate is for `from`.
fn proven_signer(
    opened: &smime::Opened,
    from: &str,
    seen: DateTime<Utc>,
) -> Option<mail_domain::SmimeCert> {
    let proved = match &opened.verification {
        SmimeVerification::Good { .. } => true,
        SmimeVerification::Doubtful { problems, .. } => !problems
            .iter()
            .any(|p| matches!(p, CertProblem::NotFrom { .. })),
        SmimeVerification::NoSignature
        | SmimeVerification::Bad { .. }
        | SmimeVerification::UnknownSigner { .. } => false,
    };
    let signer = opened.signer.as_ref()?;
    (proved && signer.cert.is_for(from)).then(|| {
        signer
            .cert
            .record(&signer.chain, CertSource::Received, seen)
    })
}
