//! What time it is, asked through a seam so that a test can stand still. Reading the wall clock
//! is `mail_core::mail`'s.

use chrono::{DateTime, Utc};

/// What time it is. The clock a [`Mail`] reads, so a test can stand still.
pub trait Clock: Send + Sync {
    /// Now.
    fn now(&self) -> DateTime<Utc>;
}
