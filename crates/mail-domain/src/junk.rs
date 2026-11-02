//! What the local junk filter has learned about some words.

/// What an account's filter knows about some words.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JunkCounts {
    /// Messages learned as spam, and as not spam.
    pub spam: u64,
    pub ham: u64,
    /// Each word asked about that has been seen: in how many spam and how many good messages.
    pub words: Vec<(String, u64, u64)>,
}
