//! Brand logos (BIMI): whether the user's switch is on, and the domain a sender's address names.
//! Reading the switch's file, the roots a logo's certificate must chain to and asking for the logo
//! are `mail_core::bimi`'s.

/// Whether brand logos are shown. Off unless someone turned it on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Setting {
    On,
    #[default]
    Off,
}

/// The domain of `address`, when it has one: after the last `@`.
pub fn domain_of(address: &str) -> Option<&str> {
    let (_, domain) = address.trim().rsplit_once('@')?;
    (!domain.is_empty()).then_some(domain)
}
