use dioxus::prelude::*;

/// The Original frame as the reader draws it, in mailo's stylesheet, and nothing else: for the
/// guarantee tests on Blitz (`tests/app/native_frame.rs`), which put markup in it that the sanitizer
/// would never have let through. A test binary of its own, because a Blitz document replaces the
/// process's event converter, which the `VirtualDom` fixtures in this crate's unit tests rely on.
#[component]
pub fn OriginalFrame(html: String) -> Element {
    rsx! {
        style { {ds::stylesheet()} }
        style { {*crate::ui::style::STYLE} }
        div { class: "reader-body",
            article { class: "frame",
                mail_ui_reader::reading::blocks::Sandbox { html }
            }
        }
    }
}
