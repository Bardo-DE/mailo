//! What arriving mail taught about keys, kept under the rules for keeping it.
//!
//! The decision is made against what is held at the moment of writing, so it is made inside the
//! transaction the message is written in, by the one function here, over whichever store holds
//! the rows ([`Held`]).

use crate::StoreError;
use mail_domain::autocrypt;
use mail_domain::{AutocryptPeer, CertFingerprint, Fingerprint, PgpKey, SmimeCert, Taught};

/// The key rows a lesson reads and writes.
pub(crate) trait Held {
    fn autocrypt_peer(&mut self, address: &str) -> Result<Option<AutocryptPeer>, StoreError>;
    fn put_autocrypt_peer(&mut self, peer: &AutocryptPeer) -> Result<(), StoreError>;
    fn pgp_key(&mut self, fingerprint: Fingerprint) -> Result<Option<PgpKey>, StoreError>;
    fn put_pgp_key(&mut self, key: PgpKey) -> Result<PgpKey, StoreError>;
    fn smime_cert(&mut self, fingerprint: CertFingerprint)
    -> Result<Option<SmimeCert>, StoreError>;
    fn put_smime_cert(&mut self, cert: SmimeCert) -> Result<SmimeCert, StoreError>;
}

/// Keep what `taught` says, and whether that changed the keys or certificates held: a key not
/// held before, or one held with other bytes or addresses. Seeing a key again that is already
/// held, byte for byte, changes nothing.
pub(crate) fn learn(held: &mut impl Held, taught: &Taught) -> Result<bool, StoreError> {
    match taught {
        Taught::Autocrypt { sighting, key } => {
            let before = held.autocrypt_peer(&sighting.address)?;
            // A stranger who sent no header leaves no state.
            if before.is_none() && sighting.header.is_none() {
                return Ok(false);
            }
            let after = autocrypt::update(before.clone(), sighting);
            let mut changed = false;
            // The key is kept only when the rules took this header as the newest.
            if let (Some(key), Some(peer), Some((fingerprint, _))) = (key, &after, &sighting.header)
                && peer.autocrypt_timestamp == Some(sighting.at)
                && peer.key == Some(*fingerprint)
            {
                let was = held.pgp_key(key.fingerprint)?;
                let now = held.put_pgp_key(key.clone())?;
                changed = was.is_none_or(|was| was.key != now.key || was.emails != now.emails);
            }
            if let Some(peer) = after
                && Some(&peer) != before.as_ref()
            {
                held.put_autocrypt_peer(&peer)?;
            }
            Ok(changed)
        }
        Taught::Signer(cert) => {
            let was = held.smime_cert(cert.fingerprint)?;
            let now = held.put_smime_cert(cert.clone())?;
            Ok(was.is_none_or(|was| was.chain != now.chain || was.emails != now.emails))
        }
    }
}
