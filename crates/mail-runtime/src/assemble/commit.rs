//! Stage three of assembly, and the reads and puts that come before the middle one.
//!
//! The only stage that touches the store. [`gather`] puts the blobs (idempotent: the same bytes
//! are the same blob) and asks the store the questions threading will ask; [`write`] hands
//! the resolved batch to the store in one go, the key rows in the transaction of the messages
//! they were learnt from.

use super::parse::Parsed;
use super::resolve::ThreadIndex;
use crate::RuntimeError;
use crate::error::Logged;
use chrono::{DateTime, Utc};
use mail_domain::{BlobId, Ingest, Patch, RemoteRef, Row};
use mail_store::{SqliteStore, Store, StoreError};
use porter_core::AccountId;
use std::collections::HashMap;

/// The blobs of a batch, by the digest of their bytes.
pub(super) type Blobs = HashMap<blake3::Hash, BlobId>;

/// A message to gather for: its bytes, what they parse to, and where a server holds it.
pub(super) struct Gather<'a> {
    pub raw: &'a [u8],
    pub parsed: &'a Parsed,
    pub remote: Option<&'a RemoteRef>,
}

/// Store every blob the batch holds (each message, and each attachment held in it) and ask the
/// store what threading needs to know about the batch.
pub(super) fn gather(
    store: &SqliteStore,
    account: &AccountId,
    batch: &[Gather<'_>],
) -> Result<(Blobs, ThreadIndex), RuntimeError> {
    let mut blobs = Blobs::new();
    for item in batch {
        put(store, &mut blobs, item.parsed.digest, item.raw)?;
        for (part, digest) in item
            .parsed
            .fields
            .attachments
            .iter()
            .zip(&item.parsed.part_digests)
        {
            if let Some(digest) = digest {
                put(store, &mut blobs, *digest, &part.bytes)?;
            }
        }
    }
    Ok((blobs, thread_index(store, account, batch)))
}

fn put(
    store: &SqliteStore,
    blobs: &mut Blobs,
    digest: blake3::Hash,
    bytes: &[u8],
) -> Result<(), RuntimeError> {
    if !blobs.contains_key(&digest) {
        let id = store.blobs().put(bytes).map_err(RuntimeError::Store)?;
        blobs.insert(digest, id);
    }
    Ok(())
}

/// What the store holds that the threading of `batch` asks about: the conversation of each
/// `Message-ID` the batch names, of each server thread id it was given, and the thread id held
/// against each address.
fn thread_index(store: &SqliteStore, account: &AccountId, batch: &[Gather<'_>]) -> ThreadIndex {
    let mut index = ThreadIndex::default();
    for remote in batch.iter().filter_map(|item| item.remote) {
        if let Some(thrid) = store
            .provider_thread_of(account.clone(), remote)
            .or_log("the server's thread id for a message could not be read")
            .flatten()
        {
            index.provider_of.insert(remote.clone(), thrid);
        }
    }
    for id in ThreadIndex::rfc_ids_asked(batch.iter().map(|item| item.parsed)) {
        if let Some(thread) = store
            .thread_by_rfc_id(account.clone(), &id)
            .or_log("a thread could not be looked up by a Message-ID")
            .flatten()
        {
            index.by_rfc_id.insert(id, thread);
        }
    }
    let mut asked: Vec<&String> = index.provider_of.values().collect();
    asked.sort();
    asked.dedup();
    for thrid in asked {
        if let Some(thread) = store
            .thread_by_provider_thread(account.clone(), thrid)
            .or_log("a thread could not be looked up by the server's thread id")
            .flatten()
        {
            index.by_provider.insert(thrid.clone(), thread);
        }
    }
    index
}

/// Write a resolved batch: the ingest, and with its first chunk the rows. If the keys held
/// changed, say so ([`crate::epoch`]).
pub(super) fn write(
    store: &dyn Store,
    account: AccountId,
    ingest: Ingest,
    rows: &[Row],
    now: DateTime<Utc>,
) -> Result<Patch, StoreError> {
    match store.ingest_with(account, ingest, rows, now) {
        Ok(ingested) => {
            if ingested.keys_changed {
                crate::epoch::keys_changed();
            }
            Ok(ingested.patch)
        }
        Err(error) => {
            // Some of it was written, and with it perhaps a key.
            if matches!(error, StoreError::Partial { .. }) && !rows.is_empty() {
                crate::epoch::keys_changed();
            }
            Err(error)
        }
    }
}
