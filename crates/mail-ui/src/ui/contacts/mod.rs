//! Contacts in the window: the book the composer suggests from (`mail_ui_kit::book`), and the book
//! in and out as vCard from ⌘K, and Settings › Contacts ([`page`]), which only draws and says what
//! the book answers. A person is edited from their name in mail, which is the reader's
//! (`mail_ui_reader::sender`). CardDAV sync stays on the command line: it needs a URL and a
//! password.

mod cards;
pub(in crate::ui) mod page;

pub(in crate::ui) use cards::{export_cards, import_cards};

#[cfg(test)]
pub(in crate::ui) mod tests;
