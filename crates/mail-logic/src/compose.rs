//! What composing decides with no store: how large a message is against what its server takes, what
//! a reply answers and quotes, how a stored draft opens as a document, what a file is called by its
//! name. Keeping a draft, queueing a send and reading the account it leaves from are `mail-core`'s.

pub mod addresses;
pub mod doc;
pub mod weight;

use chrono::{DateTime, Utc};
use mail_domain::{DraftId, ReplyScope};
use porter_core::AccountId;

/// Where [`copy_myself`] puts the sender's own address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Myself {
    Cc,
    Bcc,
}

/// The most a single message may carry, before base64 expands it.
///
/// Encoding inflates by four bytes for every three, so this is about 27 MiB on the wire, which
/// is over the 25 MB Gmail and most of the rest refuse at. The check is here rather than at send
/// because the answer has to arrive while the file is being attached: a refusal at Send is one
/// that comes after the message is written, and there is nothing useful to do with it then.
pub const ATTACHMENT_BUDGET: u64 = 20 * 1024 * 1024;

/// What to call the content of a file, from its name.
///
/// A small table and `application/octet-stream` for everything else, rather than a dependency
/// that sniffs content. The receiving client re-sniffs anyway, the honest fallback is never
/// wrong — it says "bytes", which they are — and `mail_mime::build` replaces anything that is
/// not a well-formed media type regardless, so a wrong guess here cannot forge a header.
pub fn media_type_of(name: &str) -> &'static str {
    let extension = name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    match extension.to_ascii_lowercase().as_str() {
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "json" => "application/json",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        // A saved message. As `message/rfc822` it is shown by the recipient's client as the
        // message it is, and `mail_mime::build` sends it under an encoding RFC 2046 allows.
        "eml" => ENCLOSED,
        _ => "application/octet-stream",
    }
}

/// Whether a file named `name` is a picture the body can show (`attach_inline` takes it): by
/// its name, as everything here that types a file does.
pub fn shows_inline(name: &str) -> bool {
    mail_mime::embeddable(media_type_of(name)).is_some()
}

/// When a queued message may leave the outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaves {
    /// As soon as a pass drains the outbox.
    Now,
    /// Not before this instant. The draft shows [`SendState::Scheduled`] until it goes.
    At(DateTime<Utc>),
}

/// The media type an attached message goes as.
pub const ENCLOSED: &str = "message/rfc822";

/// Whom an answer goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// A reply to the sender, or to everyone, as [`Draft::reply_to`] addresses it.
    Scope(ReplyScope),
    /// A reply to the list the message came through, at the address its `List-Post` names
    /// (RFC 2369 §3.4), and to nobody else: the person who wrote to the list reads it there.
    List,
}

/// What an answer quotes beneath its attribution line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote<'a> {
    /// The original's whole text part.
    Whole,
    /// Only this, the text the person had selected in the original when they asked to reply.
    /// Blank is [`Quote::Whole`]: a selection of nothing but space is a click, not a choice.
    Selection(&'a str),
}

/// A redirect that is queued: whom it goes to, and what it takes to withdraw it
/// ([`withdraw_redirect`]) while its grace period runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirected {
    /// The addresses of the envelope, blind ones too.
    pub to: Vec<String>,
    pub account: AccountId,
    /// The id its outbox entry is keyed by. No draft has it.
    pub draft: DraftId,
    /// When it was redirected.
    pub at: DateTime<Utc>,
}

/// What a message's body was written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// It has an HTML part: formatting, with or without a plain copy beside it.
    Rich,
    /// Plain text alone.
    Plain,
}
