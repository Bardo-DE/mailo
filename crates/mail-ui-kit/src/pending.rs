//! How the window shows work that has not finished, in the three ways it does: [`Outline`] where
//! content is on its way (rows in outline), [`Pending`] where the wait has no count (a small
//! spinner leading the words), [`Meter`] where it has one (the words above a full-width bar).
//! Only ds's own pieces are used and no word is written here: each caller brings its own.

use dioxus::prelude::*;
use ds::components::content::label::{LabelRole, LabelStyle};
use ds::components::controls::progress::model::{Progress, ProgressStyle};
use ds::components::controls::progress::view::ProgressIndicator;
use ds::motion::detail::operation::{Operation, PendingToken};
use ds::prelude::*;
use ds::style::tokens::control_size::ControlSize;

/// Rows in outline where content will be: two lines each, the third row's title short. The same
/// rows whichever pane waits, so only how many differs.
#[component]
pub fn Outline(rows: usize) -> Element {
    rsx! {
        div { class: "outline", "aria-busy": "true",
            for n in 0..rows {
                SkeletonRow {
                    key: "{n}",
                    lines: if n % 3 == 2 { SkeletonLines::One } else { SkeletonLines::Two },
                }
            }
        }
    }
}

/// A wait with no count: one small spinner, turning while this is drawn, then `label`.
#[component]
pub fn Pending(label: String) -> Element {
    let running = use_hook(|| Operation::Running(PendingToken::start()));
    rsx! {
        div { class: "pending", role: "status",
            ProgressIndicator {
                style: ProgressStyle::Spinner,
                progress: Progress::Unknown(running),
                size: ControlSize::Small,
            }
            Label { text: label, role: LabelRole::Secondary }
        }
    }
}

/// Known progress: `label` above a bar across the width, filled `thousandths` of the way.
#[component]
pub fn Meter(label: String, thousandths: u16) -> Element {
    rsx! {
        div { class: "meter", role: "status",
            Label { text: label.clone(), role: LabelRole::Secondary, style: LabelStyle::Footnote }
            ProgressIndicator {
                style: ProgressStyle::Bar,
                progress: Progress::Known(Fraction(thousandths.min(1000))),
                common: Common { aria_label: Some(label), ..Common::default() },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(root: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(root);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn a_wait_with_no_count_is_a_small_spinner_then_its_words() {
        let html = drawn(|| rsx! { Pending { label: "Sending…".to_owned() } });
        assert!(html.contains(r#"class="pending""#), "{html}");
        assert!(html.contains(r#"data-style="spinner""#), "{html}");
        assert!(html.contains(r#"data-size="small""#), "{html}");
        assert!(
            html.find("progressbar").unwrap() < html.find("Sending…").unwrap(),
            "{html}"
        );
    }

    #[test]
    fn known_progress_is_its_words_above_a_full_bar() {
        let html =
            drawn(|| rsx! { Meter { label: "Importing… 3 of 9".to_owned(), thousandths: 333 } });
        assert!(html.contains(r#"class="meter""#), "{html}");
        assert!(html.contains(r#"data-style="bar""#), "{html}");
        assert!(html.contains(r#"aria-valuemax"#), "{html}");
        assert!(
            html.find("Importing… 3 of 9").unwrap() < html.find("progressbar").unwrap(),
            "{html}"
        );
    }

    #[test]
    fn the_outline_is_as_many_rows_as_asked_and_busy() {
        let html = drawn(|| rsx! { Outline { rows: 5 } });
        assert!(html.contains(r#"aria-busy="true""#), "{html}");
        assert_eq!(html.matches(r#"data-lines="#).count(), 5, "{html}");
    }
}
