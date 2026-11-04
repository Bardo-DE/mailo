//! Looking an address up over the network, side by side with the domain's JMAP session, and
//! reading what that comes to ([`settle`]), which is `mail_logic::discover::offer`'s.

use super::{find_jmap, search};
use chrono::{DateTime, Utc};
use mail_domain::presets;

pub use mail_logic::discover::offer::{Looked, settle, typed, typed_domain};

/// Look `address` up over the network: the autoconfig search and the domain's `/.well-known/jmap`
/// side by side. Each is handed only what it needs, the address and the well-known URL. The
/// built-in table ([`super::known`]) is the caller's to ask first, since it needs no lookup.
pub async fn resolve(address: &str, now: DateTime<Utc>) -> Looked {
    let jmap = async {
        match presets::well_known(address) {
            Some(url) => find_jmap(&url).await.ok(),
            None => None,
        }
    };
    let (found, jmap) = tokio::join!(search(address, now), jmap);
    settle(found, jmap)
}
