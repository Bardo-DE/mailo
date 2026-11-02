//! What the list holds while an account's first mail is on its way: rows in outline, so the pane
//! reads as "coming" and not as "empty". The counted download's bar across the list's
//! top says how far it has got.

use dioxus::prelude::*;
use mail_ui_kit::pending::Outline;

/// How many placeholder rows fill the pane: more than a pane shows.
const ROWS: usize = 8;

#[component]
pub fn FirstSyncRows() -> Element {
    rsx! { Outline { rows: ROWS } }
}
