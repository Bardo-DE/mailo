//! What a rule means, apart from the store that keeps it and the client that carries it out: when
//! an automatic send is allowed ([`guard`]), what a "block sender" rule is ([`block`]), and how a
//! typed condition is read ([`condition`]).

pub mod block;
pub mod condition;
pub mod guard;
