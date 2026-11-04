//! Bringing up the search panel: ⌘K, or a press on the list's search; and ⌘F, which brings it up
//! over the open message.

use crate::view::Shell;
use dioxus::prelude::*;

/// The panel's field, for the host to put the keyboard in.
pub const FIELD: &str = ".ds-search-card input";

/// ⌘K, or a press on the toolbar's search: the panel up over the search there is, and the
/// keyboard in its field with the text selected, so the first key typed replaces it. A folder
/// or label picker that is open goes.
pub fn summon(mut shell: Signal<Shell>) {
    shell.write().open_bar();
    crate::host::Host::focus_all(FIELD);
}

/// ⌘F: the panel up over the open message, finding in it ([`Shell::find_in_message`]), and the
/// keyboard in its field with the text selected. With no message open, or a message being
/// written, nothing is done and it is `false`.
pub fn summon_message(mut shell: Signal<Shell>) -> bool {
    let taken = shell.write().find_in_message();
    if taken {
        crate::host::Host::focus_all(FIELD);
    }
    taken
}
