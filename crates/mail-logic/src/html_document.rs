//! What a printout says in front of a reader, the document it comes to and where its consented
//! remote images come from. Making it from the stored mail and writing it to a file is
//! `mail_core::html_document`'s.

use mail_domain::MessageId;
use mail_reader::Labels;
use std::collections::BTreeMap;

/// What a printout says in front of a reader: the header rows, the counts, the notes where a
/// body or an image is missing. `mail-mime` writes the document and none of these words.
pub const LABELS: Labels<'static> = Labels {
    from: "From",
    to: "To",
    cc: "Cc",
    date: "Date",
    subject: "Subject",
    printed: "Printed {when}",
    no_subject: mail_domain::NO_SUBJECT,
    thread_count: "({n} messages)",
    image: "[image",
    image_alt: ": {alt}",
    image_host: ", from {host}",
    no_body: "The body of this message has not been downloaded, so only its headers are printed.",
    cut_off: "This message is longer than can be shown; the rest of it is not printed.",
    attachments: "Attachments ({n})",
    unnamed: "(unnamed)",
};

/// A printable document, and the subject it is named after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printed {
    /// The first message's subject: what a file of it is called.
    pub subject: String,
    /// The whole HTML document.
    pub html: String,
}

/// Where a printout's consented remote images come from.
///
/// A remote image is a read receipt, so this is only ever built from the reader's consent: which
/// messages it covers, and how to fetch what they show. [`mail_reader::remote_images`] lists what
/// the consented messages' printout would draw, and only that list is handed to `fetch`.
pub struct Pictures<'a> {
    /// Whether the reader's consent to remote images covers this message.
    pub consented: &'a dyn Fn(MessageId) -> bool,
    /// The images of `urls` that could be fetched, each as a `data:` URI of a raster image.
    /// Blocking.
    pub fetch: &'a dyn Fn(&[String]) -> BTreeMap<String, String>,
}

impl std::fmt::Debug for Pictures<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pictures").finish_non_exhaustive()
    }
}
