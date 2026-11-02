//! The signing keyring a caller holds, as a handle of the client's own.
//!
//! Signing keys are kept by a crate below the client; a caller that holds the keyring (a window
//! given one by its root, a test given a fake) holds it as this, and hands it back to the calls
//! that need it.

use mail_core::{NoKeyring, SigningStore};
use mail_domain::signing::{SigningKeyRef, SigningSecret};
use mail_secrets::CredentialError;
use std::sync::Arc;

/// Where signing keys are kept.
#[derive(Clone)]
pub struct SigningKeys(Arc<dyn SigningStore>);

impl SigningKeys {
    /// The keyring `store` is.
    pub fn new(store: Arc<dyn SigningStore>) -> Self {
        SigningKeys(store)
    }

    /// A keyring that holds nothing: what a window gets from a root that handed none.
    pub fn none() -> Self {
        SigningKeys(Arc::new(NoKeyring))
    }
}

impl std::fmt::Debug for SigningKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SigningKeys")
    }
}

impl SigningStore for SigningKeys {
    fn get(&self, key: &SigningKeyRef) -> Result<SigningSecret, CredentialError> {
        self.0.get(key)
    }

    fn put(&self, key: &SigningKeyRef, value: &SigningSecret) -> Result<(), CredentialError> {
        self.0.put(key, value)
    }

    fn forget(&self, key: &SigningKeyRef) -> Result<(), CredentialError> {
        self.0.forget(key)
    }
}
