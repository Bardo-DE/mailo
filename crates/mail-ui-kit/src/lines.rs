//! The lines of a message's header, as the composer's page and the reader's head both set them: a
//! name small and secondary on the label column's edge, then the value, and what the recipient
//! lists are called.

use dioxus::prelude::*;
use ds::components::content::label::{LabelRole, LabelStyle};
use ds::prelude::*;

/// Which recipient list a chip or a typed address belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
    To,
    Cc,
    Bcc,
}

/// What a recipient list's row and field are called.
pub fn list_label(list: List) -> &'static str {
    match list {
        List::To => "To",
        List::Cc => "Cc",
        List::Bcc => "Bcc",
    }
}

/// One header line: its name, small and secondary, on the label column's leading edge, then its
/// value or control from the value column's edge, which every line shares. Its `data-row` is what
/// the page's tests and its layout find it by.
#[component]
pub fn Line(#[props(into)] label: String, row: &'static str, children: Element) -> Element {
    rsx! {
        div { class: "c-line", role: "group", aria_label: "{label}", "data-row": row,
            Label {
                text: label.clone(),
                role: LabelRole::Secondary,
                style: LabelStyle::Footnote,
                common: crate::common::classed("c-line-k"),
            }
            div { class: "c-line-v", {children} }
        }
    }
}
