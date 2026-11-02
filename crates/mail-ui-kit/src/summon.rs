//! Bringing up the search panel: ⌘K, or a press on the list's search.

use crate::view::{Bar, BarOpen, Shell};
use dioxus::prelude::*;

/// The panel's field, for the host to put the keyboard in.
pub const FIELD: &str = ".ds-search-card input";

/// ⌘K, or a press on the toolbar's search: the panel up over the search there is, and the
/// keyboard in its field with the text selected, so the first key typed replaces it.
pub fn summon(mut shell: Signal<Shell>) {
    if shell.peek().bar == Bar::Closed {
        let search = shell.peek().search.clone();
        shell.write().bar = Bar::Open(BarOpen::over(search));
    }
    crate::host::Host::focus_all(FIELD);
}
