//! The Settings window as the shell hands it over: `mail-ui-settings` draws every page but one,
//! and the shell fills what it cannot draw itself, the Signatures page (written in the composer's
//! body) and the window's stylesheet.

use super::signatures::Signatures;
use super::style::STYLE;
use dioxus::prelude::*;
use mail_ui_settings::settings::{self, Slots};

pub(in crate::ui) use mail_ui_settings::settings::{AccountSeams, use_settings_opened};

fn use_slots() -> Slots {
    let signatures = use_callback(|shell| rsx! { Signatures { shell } });
    Slots {
        signatures,
        style: *STYLE,
    }
}

/// The window as quire opens it, with the desktop's settings and mailo's own watched.
pub(in crate::ui) fn settings_window() -> Element {
    settings::settings_window_with(use_slots())
}

/// The window as a test drives it, without the watches on the settings directory.
pub fn settings_root() -> Element {
    settings::settings_root_with(use_slots())
}
