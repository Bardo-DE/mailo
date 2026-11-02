//! The local junk filter's memory: word counts per account, learned from mail the user called
//! spam or not spam (migration 0047). What the counts mean is `mail_core::junk`'s; this keeps
//! them, and keeps a message from being counted twice.

pub use mail_domain::Judged;

/// What learning one message did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Learned {
    /// It was not learned before.
    New,
    /// It was learned as this already: nothing changed.
    Unchanged,
    /// It was learned the other way, which was taken back first.
    Turned,
}
