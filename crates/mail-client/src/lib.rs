//! Mail as values that are already read, and kept read.
//!
//! A caller asks a [`Client`] for what it needs as a [`Live`] value, and never waits for it: the
//! client reads it on threads of its own, and again whenever the store says a commit touched what
//! it reads ([`mail_store::changes`]), whoever made the commit — the caller, a sync, a watch in
//! another process. So a caller that must not wait never does, and none has to remember to read
//! again after a write.
//!
//! Logic only: what the values mean to a person, and how they are shown, is the caller's.
//! `docs/mail-client.md` says where this is going.

pub mod act;
mod book;
mod caches;
pub mod calls;
mod client;
mod clock;
mod error;
mod external;
mod fault;
pub mod fetch;
mod files;
mod keys;
mod live;
mod net;
mod protect;
mod pure;
pub mod query;
mod saved;
mod stored;
#[cfg(feature = "testing")]
pub mod testing;

pub use act::{Act, Acted, Destroy, Perform, TakeBack, Ticket};
pub use caches::{Framing, Looking};
pub use client::{Client, ClientConfig, FetchRun, WeakClient};
pub use error::ClientError;
pub use external::on_write;
pub use fault::{CallError, Fault};
pub use keys::SigningKeys;
pub use live::{Live, Query, Snapshot, bucket_of};
pub use mail_store::{Generation, Topics};
pub use net::{Net, Proof, Reach, SignIn};
pub use pure::*;
pub use stored::Stored;
