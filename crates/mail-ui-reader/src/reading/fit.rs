//! An earlier message's frame as tall as what it holds, as Mail draws every message of a
//! conversation in full: the page scrolls, never a box inside it.
//!
//! The frame's document is read through the frame's own node on Blitz (`NodeHandle`): its root
//! element's box or its content's extent, whichever is taller, once laid out. A frame starts
//! short (`.html[data-fit]`), so a page that fills its viewport (`html { height: 100% }`) does not
//! measure as tall as a guess; it is read again while its pictures may still be arriving, and grows
//! with them.
//!
//! Every message's page, the last one's too, is also kept no wider than its frame: one a sender
//! laid out wider than the pane is drawn smaller to fit it ([`shrink`]), as Mail does, rather
//! than cut at its right edge. It is drawn at the reader's text size (`reading.text_zoom`, ⌘+ and
//! ⌘−) by the same zoom, so a larger size makes a page larger only until it fills its frame.
//!
//! The zoom is set on the document already in the frame ([`draw_at`]), which restyles it, rather
//! than written into a new one, which Blitz would parse, lay out and load the pictures of again.
//! What a page was fitted to is kept for the next time it opens ([`remembered`]), so it opens at
//! that and is laid out once.

use dioxus::core::Task;
use dioxus::prelude::*;
use dioxus_native_dom::NodeHandle;
use ds::base::time::clock::sleep;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::Duration;

/// How often a frame is read again, and for how long after it mounts: long enough for the
/// pictures a person allowed to arrive.
const EVERY: Duration = Duration::from_millis(120);
const READS: usize = 80;

/// The frame's content height in CSS pixels, once its document is laid out.
fn content_height(frame: &MountedData) -> Option<f32> {
    let handle = frame.downcast::<NodeHandle>()?;
    let doc = handle.try_doc()?;
    let sub = doc.subdoc(handle.node_id())?;
    let inner = sub.inner();
    let root = inner.root_element();
    // The content's extent leaves out the body's own margins; the root's box holds them. Short of
    // either, the frame keeps a few pixels of range of its own, and a wheel burst that starts over
    // it stays with it instead of moving the reader.
    let layout = root.final_layout();
    let height = layout
        .scrollable_overflow_rect
        .bottom
        .max(layout.size.height);
    (height > 0.0).then_some(height.ceil())
}

/// How many pages' fits are kept: a few bytes each, the oldest let go first.
const KEPT: usize = 500;

/// A page (its document's fingerprint) at a text size (in thousandths).
type Fit = (u64, u32);

/// The fits pages were last drawn at, the oldest first.
#[derive(Default)]
struct Fitted {
    drawn: HashMap<Fit, Drawn>,
    order: VecDeque<Fit>,
}

impl Fitted {
    fn get(&self, fit: Fit) -> Option<Drawn> {
        self.drawn.get(&fit).copied()
    }

    fn keep(&mut self, fit: Fit, drawn: Drawn) {
        if self.drawn.insert(fit, drawn).is_none() {
            self.order.push_back(fit);
        }
        while self.order.len() > KEPT {
            if let Some(oldest) = self.order.pop_front() {
                self.drawn.remove(&oldest);
            }
        }
    }
}

thread_local! {
    static FITTED: RefCell<Fitted> = RefCell::default();
}

fn fit_of(page: u64, text: f32) -> Fit {
    // A text size is a step of a few, never negative.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (page, (text * 1000.0).round() as u32)
}

/// How `page` was last drawn at the text size `text`, if it has been fitted since mailo opened.
pub(super) fn remembered(page: u64, text: f32) -> Option<Drawn> {
    FITTED.with(|fitted| fitted.borrow().get(fit_of(page, text)))
}

fn remember(page: u64, text: f32, drawn: Drawn) {
    FITTED.with(|fitted| fitted.borrow_mut().keep(fit_of(page, text), drawn));
}

/// Draw `frame`'s document at `zoom` where it stands, by its root element's own style, which a
/// new document from [`zoomed`] has none of until it is set again here. Whether it is at `zoom`
/// or set to be: not while the document is not styled yet.
fn draw_at(frame: &MountedData, zoom: f32) -> bool {
    let Some(handle) = frame.downcast::<NodeHandle>() else {
        return false;
    };
    // Free now, and nothing runs before it is taken: no await between.
    if handle.try_doc().is_none() {
        return false;
    }
    let mut doc = handle.doc_mut();
    let Some(sub) = doc.subdoc_mut(handle.node_id()) else {
        return false;
    };
    let mut inner = sub.inner_mut();
    let root = inner.root_element();
    let Some(now) = root
        .primary_styles()
        .map(|style| style.effective_zoom.value())
    else {
        return false;
    };
    if (now - zoom).abs() >= 0.001 {
        let root = root.id;
        inner.set_style_property(root, "zoom", &format!("{zoom:.3}"));
        inner.shell_provider.request_redraw();
    }
    true
}

/// Draw `frame`'s document at `zoom` once it is styled: a page never shrunk ([`shrink`]), a
/// plain-text message, is at the text size all the same.
pub(super) fn hold(frame: Rc<MountedData>, zoom: Signal<f32>) -> Task {
    spawn(async move {
        for _ in 0..READS {
            if draw_at(&frame, *zoom.peek()) {
                return;
            }
            sleep(EVERY).await;
        }
    })
}

/// The least a message is shrunk to: past this its words are too small to read, and the
/// frame's right edge cuts what is left over instead.
const LEAST: f32 = 0.4;

/// How a frame's document is drawn: at `zoom`, with its lone table cells let go of their `width`
/// when `relaxed` ([`RELAXED`]).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Drawn {
    pub zoom: f32,
    pub relaxed: bool,
}

/// What a browser does with a cell that is alone in its row and wider than the room its table
/// has: its `width` is a wish, so the table is as wide as the room, not as the wish. Blitz holds
/// the column to it, and a `<td width="960">` round a 480 px message pushes the message off
/// centre and past the frame's edge. Written into a document ([`zoomed`]) only when such widths
/// are what makes it wider than its frame ([`decide`]), so a table a sender sized to a wide pane
/// with a cell is not narrowed to what that cell holds.
const RELAXED: &str = "td:only-child,th:only-child{width:auto}";

/// How `frame`'s document is drawn for a person who asked for its text at `text` (the reader's
/// text size, 1 for its own), given how it is `now`: at `text`, unless the document would then be
/// wider than the frame (a newsletter laid out for 800 px, its tables and pictures fixed at that
/// width, in a narrower pane), when it is the frame's width over the document's own, so a larger
/// text size grows a page only until it fills the frame, and a pane widened while the page is
/// still read lets it grow back. When the width is a cell's wish rather than what the page holds,
/// the cells let go of it instead ([`RELAXED`]) and the page is read again. The document's width
/// is read in its own pixels. `None` when it is not laid out, or not yet at `now`'s zoom.
fn fitting(frame: &MountedData, text: f32, now: Drawn) -> Option<Drawn> {
    let handle = frame.downcast::<NodeHandle>()?;
    let doc = handle.try_doc()?;
    let width = doc.get_node(handle.node_id())?.final_layout().size.width;
    let sub = doc.subdoc(handle.node_id())?;
    let inner = sub.inner();
    let (boxes, held) = laid_width(&inner)?;
    // Read only at the zoom it is judged at: a page as wide as its frame is that wide at every
    // zoom, so read at another it would ask for that one back.
    let drawn = inner.root_element().primary_styles()?.effective_zoom;
    if width <= 0.0 || (drawn.value() - now.zoom).abs() >= 0.001 {
        return None;
    }
    let unzoom = |px: f32| drawn.unzoom(px);
    Some(decide(unzoom(boxes), unzoom(held), width, text, now))
}

/// [`fitting`]'s choice, from the widest box of the page (`boxes`) and the widest of what it holds
/// (`held`, which leaves out the cells and tables sized by a wish), both in the page's own pixels.
fn decide(boxes: f32, held: f32, width: f32, text: f32, now: Drawn) -> Drawn {
    if !now.relaxed && boxes * text > width + 1.0 && held < boxes - 1.0 {
        // Lay it out again with the cells let go before judging its width.
        return Drawn {
            zoom: text,
            relaxed: true,
        };
    }
    Drawn {
        zoom: fitted(boxes, width, text),
        relaxed: now.relaxed,
    }
}

/// The zoom a document `own` pixels wide is drawn at in a frame `width` wide, its text asked for
/// at `text`: `text` while that fits, else what fits, but never under [`LEAST`].
fn fitted(own: f32, width: f32, text: f32) -> f32 {
    if own * text > width + 1.0 {
        (width / own).max(LEAST).min(text)
    } else {
        text
    }
}

/// How wide `doc` is laid out, in its pixels as drawn, as its widest box and as the widest of
/// what it holds ([`is_sized`] leaves out the boxes a wish sizes): its widest box inside the frame's inset
/// (`mail_core::message`'s `mailo-inset`, the one element of `<body>`), and the insets of the
/// body and the inset either side; a document with no inset, its widest box under `<body>`. The
/// inset itself is a block as wide as the frame whatever it holds, so it is never the widest.
/// Not where boxes end: Blitz starts a centred box that overflows at an offset that does not
/// shrink with the zoom, where its width does. Read to a bound, as a page could be very large.
fn laid_width(doc: &blitz_dom::BaseDocument) -> Option<(f32, f32)> {
    const BOXES: usize = 20_000;
    let root = doc.root_element();
    let body = child_named(doc, root, "body")?;
    let inset = child_named(doc, body, "div").filter(|div| {
        div.attr(blitz_dom::LocalName::from("class"))
            .is_some_and(|class| {
                class
                    .split_ascii_whitespace()
                    .any(|name| name == "mailo-inset")
            })
    });
    let insets = insets_of(body, true) + inset.map_or(0.0, |inset| insets_of(inset, false));
    let mut widest = 0.0_f32;
    let mut held = 0.0_f32;
    let mut stack: Vec<_> = inset.unwrap_or(body).children.to_vec();
    let mut seen = 0;
    while let Some(id) = stack.pop() {
        seen += 1;
        if seen > BOXES {
            break;
        }
        // Text and comments have no box of their own.
        let Some(node) = doc
            .get_node(id)
            .filter(|node| node.is_element() || node.is_anonymous())
        else {
            continue;
        };
        let wide = node.final_layout().size.width;
        widest = widest.max(wide);
        if is_sized(node) {
            held = held.max(wide);
        }
        stack.extend(node.children.iter().copied());
    }
    Some((widest + insets, held + insets))
}

/// `parent`'s first child element named `name`.
fn child_named<'d>(
    doc: &'d blitz_dom::BaseDocument,
    parent: &blitz_dom::Node,
    name: &str,
) -> Option<&'d blitz_dom::Node> {
    parent
        .children
        .iter()
        .filter_map(|id| doc.get_node(*id))
        .find(|node| {
            node.element_data()
                .is_some_and(|data| &*data.name.local == name)
        })
}

/// Whether `node`'s own width says how wide the page is. A table's cells, rows and sections do not,
/// and neither does a table with no width of its own: a browser sizes such a table to what it holds
/// (at most the room it has) and takes a cell's `width` as a wish, not a floor, so a `<td
/// width="960">` round a 480 px message is no 960 px page. Blitz draws that cell at 960, and its
/// table with it, which is why they are left out here: what they hold is counted instead, and
/// a table with a width of its own (`<table width="800">`, or a stylesheet's) still is.
fn is_sized(node: &blitz_dom::Node) -> bool {
    let Some(name) = node.element_data().map(|data| &*data.name.local) else {
        return true;
    };
    match name {
        "td" | "th" | "tr" | "tbody" | "thead" | "tfoot" | "colgroup" | "col" => false,
        "table" => node
            .primary_styles()
            .is_none_or(|style| !style.get_position().width.is_auto()),
        _ => true,
    }
}

/// `node`'s padding, border and margin, left and right together; a margin the page left `auto`
/// (a body of `margin: 0 auto` and a `max-width`, centred in the frame) is room around it, not
/// width the page needs, so it is left out when `centred` allows.
fn insets_of(node: &blitz_dom::Node, centred: bool) -> f32 {
    let outer = node.final_layout();
    let auto = |left: bool| {
        centred
            && node.primary_styles().is_some_and(|style| {
                let margin = style.get_margin();
                if left {
                    margin.margin_left.is_auto()
                } else {
                    margin.margin_right.is_auto()
                }
            })
    };
    let margin = (if auto(true) { 0.0 } else { outer.margin.left })
        + (if auto(false) { 0.0 } else { outer.margin.right });
    outer.padding.left + outer.padding.right + outer.border.left + outer.border.right + margin
}

/// Keep `zoom` at the text size `text` where the page fits `frame`'s width, else at what fits,
/// as Mail shrinks a message laid out wider than its pane rather than cut its right edge, while
/// its pictures may still widen it. The document is drawn at `zoom` where it stands, and what it
/// came to is kept for `page`'s next opening.
pub(super) fn shrink(
    frame: Rc<MountedData>,
    mut zoom: Signal<f32>,
    mut relaxed: Signal<bool>,
    text: f32,
    page: u64,
) -> Task {
    spawn(async move {
        for _ in 0..READS {
            let now = Drawn {
                zoom: *zoom.peek(),
                relaxed: *relaxed.peek(),
            };
            let _ = draw_at(&frame, now.zoom);
            if let Some(next) = fitting(&frame, text, now)
                && (next.relaxed != now.relaxed || (next.zoom - now.zoom).abs() >= 0.01)
            {
                if next.relaxed != now.relaxed {
                    relaxed.set(next.relaxed);
                }
                zoom.set(next.zoom);
                remember(page, text, next);
                let _ = draw_at(&frame, next.zoom);
            }
            sleep(EVERY).await;
        }
    })
}

/// `html`, the frame's document, drawn at `zoom`, its lone cells let go of their widths when
/// `relaxed`: unchanged at 1 and not relaxed.
pub(super) fn zoomed(html: &str, zoom: f32, relaxed: bool) -> String {
    let zoom = ((zoom - 1.0).abs() >= 0.001).then(|| format!("html {{ zoom: {zoom:.3}; }}"));
    let rules: Vec<&str> = zoom
        .as_deref()
        .into_iter()
        .chain(relaxed.then_some(RELAXED))
        .collect();
    if rules.is_empty() {
        return html.to_owned();
    }
    html.replacen(
        "</head>",
        &format!("<style>{}</style></head>", rules.join(" ")),
        1,
    )
}

/// Keep `height` at `frame`'s content height while it may still change.
pub(super) fn follow(frame: Rc<MountedData>, mut height: Signal<Option<f32>>) -> Task {
    spawn(async move {
        for _ in 0..READS {
            if let Some(read) = content_height(&frame)
                && height.peek().is_none_or(|was| (was - read).abs() >= 1.0)
            {
                height.set(Some(read));
            }
            sleep(EVERY).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{Drawn, Fitted, KEPT, LEAST, decide, fit_of, fitted, zoomed};

    const PAGE: &str =
        "<!DOCTYPE html><html><head><style>p {}</style></head><body><p>Hi</p></body></html>";

    #[test]
    fn a_page_that_fits_is_its_own_size_and_a_wider_one_is_zoomed_after_its_sheets() {
        assert_eq!(zoomed(PAGE, 1.0, false), PAGE);
        assert_eq!(
            zoomed(PAGE, 0.6814, false),
            "<!DOCTYPE html><html><head><style>p {}</style><style>html { zoom: 0.681; }</style>\
             </head><body><p>Hi</p></body></html>"
        );
        assert!(zoomed(PAGE, 1.25, false).contains("<style>html { zoom: 1.250; }</style></head>"));
    }

    /// The text size a person asked for, unless the page would then be wider than its frame:
    /// then what fits, never under the least.
    #[test]
    fn a_larger_text_grows_a_page_only_until_it_fills_its_frame() {
        // (case, the page's own width, the frame's, the text size, the zoom drawn)
        let cases = [
            ("fits at its size", 500.0, 600.0, 1.0, 1.0),
            ("fits larger", 400.0, 600.0, 1.5, 1.5),
            ("grows to the frame", 500.0, 600.0, 1.5, 1.2),
            ("wider at its size", 800.0, 600.0, 1.0, 0.75),
            ("wider, larger, still fits", 800.0, 600.0, 2.0, 0.75),
            ("smaller text fits", 800.0, 600.0, 0.5, 0.5),
            ("far too wide", 3000.0, 600.0, 1.0, LEAST),
        ];
        for (case, own, width, text, want) in cases {
            let got = fitted(own, width, text);
            assert!((got - want).abs() < 1e-4, "{case}: {got}");
        }
    }

    /// A cell's wish for width, not what the page holds, makes it wider than the frame: its cells
    /// let go of it, and the page is judged again; what the page holds that is wider is shrunk.
    #[test]
    fn a_cell_s_wish_is_let_go_before_the_page_is_shrunk() {
        let fresh = Drawn {
            zoom: 1.0,
            relaxed: false,
        };
        // 964 wide in boxes, 480 in what it holds, in a 700 frame.
        let first = decide(964.0, 480.0, 700.0, 1.0, fresh);
        assert_eq!((first.zoom, first.relaxed), (1.0, true));
        // Laid out again, it is 480: the text size.
        let then = decide(480.0, 480.0, 700.0, 1.0, first);
        assert_eq!((then.zoom, then.relaxed), (1.0, true));
        // Boxes as wide as what they hold (an 800 px table) are shrunk, not let go.
        let wide = decide(836.0, 836.0, 700.0, 1.0, fresh);
        assert!(!wide.relaxed && (wide.zoom - 700.0 / 836.0).abs() < 1e-4);
    }

    #[test]
    fn a_fit_is_kept_per_page_and_text_size_and_the_oldest_let_go() {
        let mut fitted = Fitted::default();
        let wide = Drawn {
            zoom: 0.8,
            relaxed: false,
        };
        fitted.keep(fit_of(1, 1.0), wide);
        assert_eq!(fitted.get(fit_of(1, 1.0)), Some(wide));
        assert_eq!(fitted.get(fit_of(1, 1.25)), None, "another text size");
        for page in 2..=KEPT as u64 {
            fitted.keep(fit_of(page, 1.0), wide);
        }
        assert_eq!(fitted.get(fit_of(1, 1.0)), Some(wide), "full, not over");
        fitted.keep(fit_of(1, 1.0), wide);
        assert_eq!(fitted.order.len(), KEPT, "kept again is not kept twice");
        fitted.keep(fit_of(9999, 1.0), wide);
        assert_eq!(fitted.get(fit_of(1, 1.0)), None, "the oldest goes");
        assert_eq!(fitted.drawn.len(), KEPT);
    }
}
