//! Where a message stands on read receipts, for the reader to show. Answering one is
//! `mail_core::receipt`'s.

use mail_domain::ReceiptAnswer;
use mail_mime::ReceiptAsk;

/// Where a message stands on read receipts, for the reader to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptState {
    /// It does not ask, or it is our own message, or it is a receipt itself.
    NotAsked,
    /// Only its headers are here; whether it asks is known once the body arrives.
    Unknown,
    /// It asks, and nobody has answered. Show `ask.to`, and warn when `ask.return_path` is
    /// [`mail_mime::ReturnPath::Differs`].
    Pending(ReceiptAsk),
    /// Answered on this machine.
    Answered(ReceiptAnswer),
}
