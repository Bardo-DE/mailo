//! What a rule means, apart from the store that keeps it and the client that carries it out: when
//! an automatic send is allowed ([`guard`]), what a "block sender" rule is ([`block`]), how a
//! typed condition is read ([`condition`]), what a rule being written holds ([`manage`]) and where
//! an away reply is kept ([`server`]).

pub mod block;
pub mod condition;
pub mod guard;
pub mod manage;
pub mod server;

use mail_domain::Rule;

/// One rule of a listing, with its condition written back in the search language.
#[derive(Debug, Clone, PartialEq)]
pub struct ListedRule {
    pub rule: Rule,
    pub condition: String,
}
