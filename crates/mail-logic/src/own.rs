//! The addresses a person writes from, for telling their own mail from everyone else's.

/// The user's own addresses, across every account and identity, compared without case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Own(Vec<String>);

impl Own {
    pub fn new<S: AsRef<str>>(addresses: impl IntoIterator<Item = S>) -> Self {
        let mut all: Vec<String> = addresses
            .into_iter()
            .map(|a| a.as_ref().trim().to_ascii_lowercase())
            .filter(|a| !a.is_empty())
            .collect();
        all.sort();
        all.dedup();
        Own(all)
    }

    pub fn includes(&self, email: &str) -> bool {
        self.0
            .binary_search(&email.trim().to_ascii_lowercase())
            .is_ok()
    }
}
