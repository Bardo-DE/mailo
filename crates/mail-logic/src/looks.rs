//! What the reader finds out about one message beyond its body, as values: what a thread's
//! answers are a function of, an invitation a message carries, and one message's standing on read
//! receipts. Finding them out, and keeping them, is `mail_core::message::Looks`.

use crate::receipt::ReceiptState;
use mail_domain::{BlobId, InviteAnswer, MessageId};
use mail_pim::Invite;

/// What a thread's answers are a function of: its messages and the bytes each one has. A body
/// that arrives, or a message that joins the thread, is a new key; nothing else changes it,
/// because a blob is content-addressed.
pub type Bodies = Vec<(MessageId, Option<BlobId>)>;

/// An invitation a message carries, with what the user last answered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invited {
    pub invite: Box<Invite>,
    pub answered: Option<InviteAnswer>,
}

/// One message's standing on read receipts, with the name the front-end calls its sender by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub message: MessageId,
    /// The sender's name, else their address.
    pub sender: String,
    pub state: ReceiptState,
}
