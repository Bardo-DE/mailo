//! Reading: a message's S/MIME signature checked and its encryption opened, when the reader
//! opens it.
//!
//! As with OpenPGP (`crate::pgp::read`): never at sync time for encrypted mail, and decrypted
//! content lives in memory only — returned to the caller and kept in a small cache in this
//! process, never written to the store, the blob directory or the index. What a signature
//! teaches — the signer's certificate — is kept, under the rule `mail_runtime::smime` states.

pub use mail_logic::protection::smime::Protected;

use super::certs::identity_of;
use super::{SmimeError, epoch};
use crate::error::Logged;
use chrono::{DateTime, Utc};
use mail_crypto::smime::{self, Cert, Identity, Keys};
use mail_domain::*;
use mail_secrets::SigningStore;
use mail_store::{SqliteStore, Store};
use std::sync::Mutex;

/// How many opened messages to keep in memory.
const CACHE_SIZE: usize = 32;

/// Opened messages, newest last, with the certificate epoch they were opened under. Only those
/// that opened: one that could not be is asked about again, when an identity may have arrived.
static CACHE: Mutex<Vec<(BlobId, u64, Protected)>> = Mutex::new(Vec::new());

fn cached(raw: BlobId) -> Option<Protected> {
    let now = epoch();
    let cache = CACHE.lock().unwrap_or_else(|held| held.into_inner());
    cache
        .iter()
        .find(|(blob, at, _)| *blob == raw && *at == now)
        .map(|(_, _, protected)| protected.clone())
}

fn keep(raw: BlobId, protected: &Protected) {
    let mut cache = CACHE.lock().unwrap_or_else(|held| held.into_inner());
    cache.retain(|(blob, _, _)| *blob != raw);
    cache.push((raw, epoch(), protected.clone()));
    if cache.len() > CACHE_SIZE {
        cache.remove(0);
    }
}

/// Open `message` for reading: `Ok(None)` when it carries no S/MIME, or its body is not here
/// yet.
///
/// Decrypts with the user's private keys for the certificates the message is encrypted to, and
/// checks the signature against the system's trust anchors and the certificates the user trusts.
/// The signer's certificate is kept for the sender's address when the signature proves it.
pub fn open_message(
    store: &SqliteStore,
    secrets: &dyn SigningStore,
    message: &Message,
    now: DateTime<Utc>,
) -> Result<Option<Protected>, SmimeError> {
    let Some(raw_id) = message.body.raw() else {
        return Ok(None);
    };
    if let Some(hit) = cached(raw_id) {
        return Ok(Some(hit));
    }
    let raw = store.blobs().get(raw_id)?;
    let Some((protected, opened)) = opened(store, secrets, &raw, now)? else {
        return Ok(None);
    };
    // What a signature teaches is a convenience; failing to record it is no reason not to show
    // the message.
    mail_runtime::smime::keep_signer(store, &opened, &message.from.email, now)
        .or_log("the signer's certificate was not kept");
    if matches!(
        protected.encryption,
        SmimeEncryption::Decrypted
            | SmimeEncryption::DecryptedAuthenticated
            | SmimeEncryption::NotEncrypted
    ) {
        keep(raw_id, &protected);
    }
    Ok(Some(protected))
}

/// The same, on bytes the caller already holds. Nothing is cached and nothing is learnt.
pub fn open_bytes(
    store: &SqliteStore,
    secrets: &dyn SigningStore,
    raw: &[u8],
    now: DateTime<Utc>,
) -> Result<Option<Protected>, SmimeError> {
    Ok(opened(store, secrets, raw, now)?.map(|(protected, _)| protected))
}

fn opened(
    store: &SqliteStore,
    secrets: &dyn SigningStore,
    raw: &[u8],
    now: DateTime<Utc>,
) -> Result<Option<(Protected, smime::Opened)>, SmimeError> {
    let identities = identities_for(store, secrets, raw)?;
    let held = store.smime_certs()?;
    let mut certs: Vec<Cert> = Vec::new();
    let mut anchors: Vec<Cert> = mail_runtime::smime::system_anchors().to_vec();
    for record in &held {
        let Ok(cert) = Cert::from_der(&record.der) else {
            continue;
        };
        if record.trust == KeyTrust::Verified {
            anchors.push(cert.clone());
        }
        certs.push(cert);
        certs.extend(
            record
                .chain
                .iter()
                .filter_map(|der| Cert::from_der(der).ok()),
        );
    }
    let keys = Keys {
        identities: &identities,
        certs: &certs,
        anchors: &anchors,
        now,
    };
    let Some(opened) = smime::open(raw, &keys) else {
        return Ok(None);
    };
    let signer = match &opened.signer {
        Some(signer) => Some(
            store
                .smime_cert(signer.cert.fingerprint())?
                .unwrap_or_else(|| signer.cert.record(&signer.chain, CertSource::Received, now)),
        ),
        None => None,
    };
    let readable = matches!(
        opened.encryption,
        SmimeEncryption::Decrypted
            | SmimeEncryption::DecryptedAuthenticated
            | SmimeEncryption::NotEncrypted
    );
    let shown = if readable {
        mail_mime::parse(&opened.message).ok()
    } else {
        None
    };
    Ok(Some((
        Protected {
            encryption: opened.encryption.clone(),
            verification: opened.verification.clone(),
            signer,
            raw: shown.is_some().then(|| opened.message.clone()),
            shown,
        },
        opened,
    )))
}

/// The user's identities `raw` is encrypted to, their private keys read from the keyring. Empty
/// for a message not encrypted, or not to the user; the keyring is not touched then.
fn identities_for(
    store: &SqliteStore,
    secrets: &dyn SigningStore,
    raw: &[u8],
) -> Result<Vec<Identity>, SmimeError> {
    let Some(to) = smime::recipients(raw) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for record in store.smime_certs()? {
        if record.secret != SecretHeld::Held {
            continue;
        }
        let Ok(cert) = Cert::from_der(&record.der) else {
            continue;
        };
        if to.iter().any(|id| id.names(&cert)) {
            out.push(identity_of(store, secrets, &record)?);
        }
    }
    Ok(out)
}
