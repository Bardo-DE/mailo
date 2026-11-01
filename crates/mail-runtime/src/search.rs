//! Searching the server: what a search found, and how it is kept.
//!
//! A search typed here runs over the store. What the store never held — mail older than a sync
//! reached, in folders nobody follows, on Gmail anywhere but the inbox and Sent — only the
//! server can find. Each engine asks its server in its own protocol
//! ([`mail_proto::search`] says how and what cannot be asked), and what comes back is kept the
//! way a sync keeps a new message: its headers fetched by the header path every sync uses, and
//! absorbed into the store, so it lists, opens and threads as any other message does. Its body
//! is not fetched for the search, and nothing remote in it is loaded: opening it later fetches
//! the body as opening any headers-only message does.
//!
//! What was held already is not fetched again, and a message new to this computer is marked
//! ([`mail_store::Store::mark_found`]) so the list can say where it came from.

pub use mail_proto::search::{Searched, ServerHits, Unsaid};

/// Most messages one search of the server brings here: the newest it found. The rest are
/// counted in [`ServerHits::more`], not fetched.
pub const SERVER_HITS: usize = 50;
