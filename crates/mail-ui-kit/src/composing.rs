//! The composer's page, as the reader's column sets it: the reader is handed the page already
//! drawn, whether it answers the open conversation (it sits under that thread) or stands in for
//! the reader, and names nothing of the composer.

use dioxus::prelude::*;

/// The page being written, drawn by the composer.
#[derive(Debug, Clone)]
pub struct Composing {
    /// Whether it answers the open conversation, so it sits under it; else it stands in for the
    /// reader.
    pub reply: bool,
    /// The page, keyed by its draft.
    pub page: Element,
}
