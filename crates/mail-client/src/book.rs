//! What a window is told of the address book's groups and people, as the client says it: who a group
//! stands for, how each of its members reads, what adding a sender came to, and why a change was
//! not made.
//!
//! Each is what a call on the client returns in place of the core's value of the same name, which
//! also holds the core's own errors and so is not a caller's to name.

use crate::CallError;
use mail_editor::Person;
use mail_store::{GroupId, PersonCard};

/// Who a group stands for now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Expanded {
    /// Everyone it names, in its order, each address once.
    pub people: Vec<Person>,
    /// Its members that name nobody known here, as written.
    pub unresolved: Vec<String>,
}

/// A group the composer offers, expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub id: GroupId,
    pub name: String,
    pub expanded: Expanded,
}

impl From<mail_core::contacts::groups::Offer> for Offer {
    fn from(offer: mail_core::contacts::groups::Offer) -> Self {
        Offer {
            id: offer.id,
            name: offer.name,
            expanded: Expanded {
                people: offer.expanded.people,
                unresolved: offer.expanded.unresolved,
            },
        }
    }
}

/// How one member of a group reads: who it names, or itself when it names nobody known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Labelled {
    /// The member names nobody known.
    NotFound,
    /// It names one person, with a name of their own.
    Named(Person),
    /// It names these addresses (one nameless person, or several).
    Addresses(Vec<String>),
}

impl From<mail_core::contacts::groups::Labelled> for Labelled {
    fn from(labelled: mail_core::contacts::groups::Labelled) -> Self {
        use mail_core::contacts::groups::Labelled as Core;
        match labelled {
            Core::NotFound => Labelled::NotFound,
            Core::Named(person) => Labelled::Named(person),
            Core::Addresses(addresses) => Labelled::Addresses(addresses),
        }
    }
}

/// Why a change to a group was not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupError {
    /// A group needs a name.
    NoName,
    /// There is no such group any more.
    Gone,
    /// A synced group is its address book's to delete.
    InBook { name: String },
    /// Anything else.
    Other(CallError),
}

impl From<mail_core::contacts::groups::GroupError> for GroupError {
    fn from(error: mail_core::contacts::groups::GroupError) -> Self {
        use mail_core::contacts::groups::GroupError as Core;
        match error {
            Core::NoName => GroupError::NoName,
            Core::Gone => GroupError::Gone,
            Core::InBook { name } => GroupError::InBook { name },
            Core::Core(other) => GroupError::Other(other.into()),
        }
    }
}

/// Why a change to a card was not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonError {
    /// A card needs a name or an address.
    Empty,
    /// What was typed for an address is not one.
    BadAddress(String),
    /// There is no such card any more.
    Gone,
    /// A synced card is its address book's to delete.
    InBook { name: String },
    /// Anything else.
    Other(CallError),
}

impl From<mail_core::contacts::people::PersonError> for PersonError {
    fn from(error: mail_core::contacts::people::PersonError) -> Self {
        use mail_core::contacts::people::PersonError as Core;
        match error {
            Core::Empty => PersonError::Empty,
            Core::BadAddress(typed) => PersonError::BadAddress(typed),
            Core::Gone => PersonError::Gone,
            Core::InBook { name } => PersonError::InBook { name },
            Core::Core(other) => PersonError::Other(other.into()),
        }
    }
}

/// What Add to Contacts came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    /// A new card, for a sender nobody's card listed.
    New(PersonCard),
    /// The card that already lists the sender, left as it was.
    Already(PersonCard),
}

impl From<mail_core::contacts::people::Added> for Added {
    fn from(added: mail_core::contacts::people::Added) -> Self {
        use mail_core::contacts::people::Added as Core;
        match added {
            Core::New(card) => Added::New(card),
            Core::Already(card) => Added::Already(card),
        }
    }
}
