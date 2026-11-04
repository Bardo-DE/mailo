//! The quick filter in the list pane: the toolbar's Filter button, and the row of toggles it
//! shows under the toolbar. What the toggles mean is [`mail_ui_kit::quick`]: operators of the
//! search line, which this draws as pressed or not and writes into the line when they turn.

use dioxus::prelude::*;
use ds::components::controls::button_model::{Bezel, ImagePosition};
use ds::components::controls::chip::{Chip, ChipVariant};
use ds::prelude::*;
use ds::style::tokens::control_size::ControlSize;
use mail_ui_kit::press::on_primary;
use mail_ui_kit::quick::{self, Row, Toggle};
use mail_ui_kit::view::Shell;
use mail_ui_model::copy;

/// Turn `toggle`'s operator on or off in the search, from the first page of what it leaves.
/// Turned on, it shows the row, so what narrows the list is always on screen.
pub fn toggle(mut shell: Signal<Shell>, mut pages: Signal<u32>, toggle: Toggle) {
    if !shell.peek().filterable() {
        return;
    }
    let line = toggle.flip(&shell.peek().search);
    let on = toggle.is_on(&line);
    {
        let mut write = shell.write();
        write.search = line;
        if on {
            write.filter_row = Row::Shown;
        }
    }
    pages.set(1);
}

/// Every toggle's operator out of the search, the rest of the line left: the whole list again,
/// from its first page.
pub fn clear(mut shell: Signal<Shell>, mut pages: Signal<u32>) {
    let line = quick::cleared(&shell.peek().search);
    shell.write().search = line;
    pages.set(1);
}

/// The toolbar's Filter button: shows the row of toggles, or hides it and takes the toggles out
/// of the search. Pressed in while a toggle narrows the list. Not drawn where the list holds no
/// conversations.
#[component]
pub fn FilterButton(shell: Signal<Shell>, mut pages: Signal<u32>) -> Element {
    if !shell.read().filterable() {
        return rsx! {};
    }
    let (row, active) = {
        let current = shell.read();
        (current.filter_row, quick::is_active(&current.search))
    };
    rsx! {
        Button {
            bezel: Bezel::Toolbar,
            size: ControlSize::Large,
            image: ImagePosition::Only,
            label: copy::aria::FILTER,
            icon: Some(IconSource::Glyph(Icon::Switches)),
            title: Some(copy::tooltips::FILTER.to_owned()),
            value: Some(if active { Check::On } else { Check::Off }),
            shown: Some(if row == Row::Shown { Shown::Visible } else { Shown::Hidden }),
            onclick: on_primary(move || {
                if row == Row::Shown {
                    clear(shell, pages);
                }
                shell.write().filter_row = match row {
                    Row::Shown => Row::Hidden,
                    Row::Hidden => Row::Shown,
                };
            }),
        }
    }
}

/// The row of toggles under the toolbar while the Filter button shows it: Unread, Starred and
/// Has Attachments, each pressed in while its operator is in the search, and Clear Filter while
/// any is.
#[component]
pub fn QuickRow(shell: Signal<Shell>, pages: Signal<u32>) -> Element {
    let (line, row) = {
        let current = shell.read();
        (current.search.clone(), current.filter_row)
    };
    if row == Row::Hidden || !shell.read().filterable() {
        return rsx! {};
    }
    rsx! {
        div { class: "quick-filter", role: "group", aria_label: copy::aria::QUICK_FILTER,
            for one in Toggle::ALL {
                Chip {
                    key: "{one.term()}",
                    variant: ChipVariant::Neutral,
                    text: one.label().to_owned(),
                    pressed: Some(if one.is_on(&line) { Check::On } else { Check::Off }),
                    onpress: Some(EventHandler::new(on_primary(move || toggle(shell, pages, one)))),
                }
            }
            if quick::is_active(&line) {
                Button {
                    bezel: Bezel::Inline,
                    label: copy::controls::CLEAR_FILTER,
                    onclick: on_primary(move || clear(shell, pages)),
                }
            }
        }
    }
}
