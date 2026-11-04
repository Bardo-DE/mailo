//! What Escape does in a search field, everywhere the app has one: with text in it, empty the
//! text and keep the field; with none, leave, which closes the popover, picker or panel the
//! field belongs to (a field that is a page's own has nothing to close and leaves it alone).
//!
//! The rule is [`escape`], pure; the ⌘K bar (`command/keys.rs`) decides its keys with it. A
//! field whose keys the caller owns hears them through [`clear_on_escape`], which does the
//! clearing and keeps the key from also closing what the field sits in.

use dioxus::prelude::*;

/// What Escape does to a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    /// Empty the field; the keyboard stays in it.
    Clear,
    /// Leave: close what the field belongs to.
    Leave,
}

/// What Escape does in a field holding `text`: clear it while it holds any, leave once empty.
pub fn escape(text: &str) -> Escape {
    escape_with(!text.is_empty())
}

/// [`escape`] for a caller that already knows whether the field holds any text.
pub fn escape_with(has_text: bool) -> Escape {
    if has_text {
        Escape::Clear
    } else {
        Escape::Leave
    }
}

/// Whether `name` (a DOM key name) is Escape.
pub fn is_escape(name: &str) -> bool {
    name == "Escape"
}

/// `event` in a field holding `text`: if it is Escape and there is text, `clear` runs and the
/// key stops here, so it does not also close the popover the field is in. Returns whether it
/// cleared; `false` leaves the key to do what it would, which for an empty field is to leave.
pub fn clear_on_escape(event: &KeyboardEvent, text: &str, clear: impl FnOnce()) -> bool {
    if !is_escape(&event.key().to_string()) || escape(text) == Escape::Leave {
        return false;
    }
    event.stop_propagation();
    event.prevent_default();
    clear();
    true
}

#[cfg(test)]
mod tests {
    use super::{Escape, escape};

    #[test]
    fn escape_clears_text_first_and_leaves_once_the_field_is_empty() {
        for (text, want) in [
            ("", Escape::Leave),
            ("a", Escape::Clear),
            (" ", Escape::Clear),
            ("inbox", Escape::Clear),
        ] {
            assert_eq!(escape(text), want, "{text:?}");
        }
    }
}
