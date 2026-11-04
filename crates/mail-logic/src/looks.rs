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

/// Which lookup, for `reads`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// A sender's checks, read from the stored raw message.
    Checks,
    /// A message's invitation.
    Invite,
    /// Whether a message asks for a read receipt.
    Receipt,
    /// A thread's way out of its list.
    List,
    /// A message opened as OpenPGP or S/MIME.
    Seal,
    /// A message's one-time code.
    Code,
    /// Whether a thread's newest message names its list's address (`List-Post`).
    Post,
}
