//! Which presses open or pick a row.

use super::click_of;
use dioxus::html::input_data::keyboard_types::Modifiers;

#[test]
fn only_the_primary_button_opens_or_picks_a_row() {
    use chordkit::{Desktop, Platform};
    use ds::base::press::{PointerButton, Press};
    use mail_ui_model::selection::Click;
    let ours = Platform::Linux {
        desktop: Desktop::Ours,
    };
    // The key that toggles is the platform's primary one: Command on a Mac and our desktop,
    // Ctrl on Windows and other Linux. The other is not.
    let cases = [
        (
            "a plain click",
            ours,
            PointerButton::Primary,
            Modifiers::empty(),
            Some(Click::Plain),
        ),
        (
            "shift",
            ours,
            PointerButton::Primary,
            Modifiers::SHIFT,
            Some(Click::Range),
        ),
        (
            "command on our desktop",
            ours,
            PointerButton::Primary,
            Modifiers::SUPER,
            Some(Click::Toggle),
        ),
        (
            "command, as a window may report it",
            ours,
            PointerButton::Primary,
            Modifiers::META,
            Some(Click::Toggle),
        ),
        (
            "ctrl is not the primary key on our desktop",
            ours,
            PointerButton::Primary,
            Modifiers::CONTROL,
            Some(Click::Plain),
        ),
        (
            "ctrl on Windows",
            Platform::Windows,
            PointerButton::Primary,
            Modifiers::CONTROL,
            Some(Click::Toggle),
        ),
        (
            "shift wins over the primary key",
            ours,
            PointerButton::Primary,
            Modifiers::SHIFT | Modifiers::SUPER,
            Some(Click::Range),
        ),
        (
            "a right click",
            ours,
            PointerButton::Secondary,
            Modifiers::empty(),
            None,
        ),
        (
            "a middle click",
            ours,
            PointerButton::Middle,
            Modifiers::empty(),
            None,
        ),
    ];
    for (case, platform, button, modifiers, want) in cases {
        let press = Press {
            button,
            modifiers,
            ..Press::primary()
        };
        assert_eq!(click_of(press, platform), want, "{case}");
    }
}
