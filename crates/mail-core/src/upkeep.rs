//! The repairs the program makes once, after it opens the store and before anything reads it:
//! each finds what an older build left unfinished and finishes it, and finds nothing after.

use crate::CoreError;
use mail_store::{SqliteStore, Store};

/// Messages an older parser misread are read again from their stored bytes. How many were
/// rewritten. Usually an empty queue and one query.
pub fn reparse_queued(store: &SqliteStore) -> Result<usize, CoreError> {
    Ok(mail_runtime::reparse_queued(store)?)
}

/// OpenPGP keys kept before their dates were recorded have them read from their bytes. How many
/// were dated.
pub fn date_keys(store: &dyn Store) -> Result<usize, CoreError> {
    Ok(mail_runtime::pgp::date_keys(store)?)
}
