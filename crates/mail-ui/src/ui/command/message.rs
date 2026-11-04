//! The card in its message mode: ⌘F's find in the open message, as the field of the card ⌘K
//! brings up. The field's text is the find (`Shell::find`), the list's search is left alone, and
//! the card lists no rows; under the field a chip names the scope and another says where the
//! find is. The reader marks the matches; the card steps through them.
//!
//! Its keys are the card's own: Return and ⌘G go to the next match, Shift+Return and ⇧⌘G to the
//! previous; Backspace on the empty field takes the scope's chip away, and the card is the
//! ordinary search again. Escape is the card's, as in every mode (`super::keys`): the text, then
//! the card. Leaving the card leaves the marks, so ⌘G goes on stepping.

use chordkit::StandardAction;
use dioxus::prelude::*;
use ds::prelude::*;
use mail_client::search::Step;
use mail_ui_kit::actions::{self, Heard, Own};
use mail_ui_kit::summon::FIELD;
use mail_ui_kit::view::{Bar, BarListing, Finding, Shell};
use mail_ui_model::copy;

/// Whether the card is listing the message's find.
pub(super) fn is_on(shell: &Shell) -> bool {
    matches!(
        &shell.bar,
        Bar::Open(open) if open.listing == BarListing::Message
    )
}

/// What the field holds in this mode: the find's text.
pub(super) fn text(shell: &Shell) -> String {
    shell
        .find
        .as_ref()
        .map(|finding| finding.find.query.clone())
        .unwrap_or_default()
}

/// The chips under the field: the scope, and where the find is while there is something to say.
pub(super) fn chips(shell: &Shell) -> Vec<String> {
    let mut chips = vec![copy::fields::IN_THIS_MESSAGE.to_owned()];
    chips.extend(shell.find.as_ref().and_then(count));
    chips
}

/// What is said of the find: the match it is on, or that there is none; nothing while nothing is
/// typed or it is still counting.
fn count(finding: &Finding) -> Option<String> {
    if finding.find.query.trim().is_empty() || !finding.counted {
        return None;
    }
    Some(match finding.at() {
        Some(at) => copy::status::match_of(at + 1, finding.total),
        None => copy::status::NO_MATCHES.to_owned(),
    })
}

/// The field now holds `typed`: found again from the first match.
pub(super) fn retype(shell: &mut Shell, typed: String) {
    shell
        .find
        .get_or_insert_with(Finding::default)
        .retype(typed);
}

/// The card is the ordinary search again, the list's search back as it was before the card.
pub(super) fn to_search(mut shell: Signal<Shell>, mut pages: Signal<u32>) {
    if shell.write().message_to_search() {
        pages.set(1);
    }
}

/// A key the find answers in any mode of the card, or Return and Backspace in this one. Whether
/// the card took the key.
pub(super) fn on_key(
    mut shell: Signal<Shell>,
    pages: Signal<u32>,
    keys: Keys,
    event: &KeyboardEvent,
) -> bool {
    match actions::heard(keys, event, true) {
        Some(Heard::Standard(StandardAction::Find)) => {
            // Over the open message, the card finds in it; else it is the search, selected.
            if !mail_ui_kit::summon::summon_message(shell) {
                mail_ui_kit::host::Host::focus_all(FIELD);
            }
            return taken(event);
        }
        Some(Heard::Standard(StandardAction::FindNext)) => {
            shell.write().step_find(Step::Next);
            return taken(event);
        }
        Some(Heard::Standard(StandardAction::FindPrevious)) => {
            shell.write().step_find(Step::Previous);
            return taken(event);
        }
        Some(Heard::Own(Own::Search)) => {
            // ⌘K is the search: from the find it is the ordinary one, the marks staying.
            if is_on(&shell.peek()) {
                to_search(shell, pages);
            }
            mail_ui_kit::host::Host::focus_all(FIELD);
            return taken(event);
        }
        _ => {}
    }
    if !is_on(&shell.peek()) {
        return false;
    }
    let modifiers = event.modifiers();
    match event.key() {
        Key::Enter => {
            let back = modifiers.contains(Modifiers::SHIFT);
            shell
                .write()
                .step_find(if back { Step::Previous } else { Step::Next });
            taken(event)
        }
        Key::Backspace if modifiers.is_empty() && text(&shell.peek()).is_empty() => {
            to_search(shell, pages);
            taken(event)
        }
        _ => false,
    }
}

/// `true`, once the key is kept from the window and the field.
fn taken(event: &KeyboardEvent) -> bool {
    event.prevent_default();
    event.stop_propagation();
    true
}
