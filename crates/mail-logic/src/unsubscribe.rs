//! Leaving a mailing list: a message's way out with what is needed to take it, and what taking it
//! did. Reading the list headers from the stored message and taking the way out are
//! `mail_core::unsubscribe`'s.

use mail_domain::{Address, DraftId, MessageId};
use mail_mime::ListHeaders;
use porter_core::AccountId;

/// A message's way out of its list, with what is needed to take it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The message the headers were read from.
    pub message: MessageId,
    /// The account it came to, which is the one a `mailto:` unsubscribe is sent from.
    pub account: AccountId,
    /// Everyone it was addressed to, for choosing which of the account's identities is on the list.
    pub addressed: Vec<Address>,
    pub list: ListHeaders,
}

/// What taking the way out did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The list's server accepted the one-click `POST`.
    Unsubscribed { url: String },
    /// An unsubscribe message is queued, and leaves on the next sync.
    Queued { draft: DraftId },
    /// Only a page is offered, and this client does not open pages. Here is its address.
    Page { url: String },
}
