//! What a front end's tests arrange a client's world with: the store a test builds and reads, the
//! rows it seeds, the keyrings and OAuth clients it swaps in, and the operations on a store that
//! set a scene up (a draft made, a key imported, a message filed) before the window under test
//! is drawn.
//!
//! Behind the `testing` feature, which a crate enables for its tests and the program never does.
//! A window's own code does not reach any of this: it goes through the [`crate::Client`].

pub use mail_core::testing::*;
pub use mail_core::{SqliteStore, Store};
/// Seeding a store without writing SQL.
pub use mail_store::testing as seed;

/// The core's operations on a store, for a test to do what the program would have done before the
/// window was opened. A window never names these: its calls are the client's.
pub mod core {
    pub use mail_core::{
        Arrival, ClientRegistry, Environment, NoKeyring, Pushed, RuleOps, SigningStore, Storage,
        SyncOps, account, act, assemble, attach, compose, contacts, conversation, discover, erase,
        export, fetch, folder, formats, history, html_document, identity, import, mail, message,
        notify, offline, pgp, profile, query, receipt, rules, schedule, search, signature, single,
        smime, snooze, sync, template, unsubscribe, vip,
    };
}

/// What the runtime below the core keeps, read back by a test that checks it was written.
pub mod runtime {
    pub use mail_runtime::pgp;
}

/// A source of mail at `path` in the format `archive`, as a window is handed one after looking.
pub fn source(
    archive: &'static dyn core::formats::Archive,
    path: std::path::PathBuf,
) -> crate::files::Source {
    crate::files::Source(core::import::Source::new(archive, path))
}
