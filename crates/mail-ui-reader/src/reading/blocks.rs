//! The sandboxed frame displaying the message body.

use chordkit::StandardAction;
use dioxus::core::Task;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use ds::prelude::use_keys;
use mail_domain::MessageId;
use std::rc::Rc;

use super::words;

/// The sandboxed frame. `tag` is the frame's `data-frame-tag` (`Holder::tag`): on Blitz the
/// network reads it to know which reader drew the frame and which message's consented images it
/// may fetch, and an untagged frame fetches none (`ui/original/net.rs`).
///
/// The frame's words are its own document's to select ([`words`]): ⌘A with the keyboard in it
/// selects them, and is not the window's ⌘A, which picks every conversation in the list. A right
/// click on them puts the selection the last press let go of back, and hands its words to `asked`
/// for the reader's Copy menu.
#[component]
pub fn Sandbox(
    html: String,
    #[props(default)] tag: Option<String>,
    /// A plain-text message's frame, which has no ground of its own (`FrameBody::Present`).
    #[props(default)]
    plain: bool,
    /// An earlier message's frame, as tall as what it holds (`super::fit`); the last message's
    /// takes the room left instead.
    #[props(default)]
    fit: bool,
    /// The message the frame shows, which a reply asks for its selection (`super::selection`) and
    /// a find looks through (`super::found`); a frame drawn with no reader around it is not.
    #[props(default)]
    message: Option<MessageId>,
    /// The words a right click on the frame was on, for the reader's Copy menu, which takes
    /// them; a frame drawn with no reader around it offers none.
    #[props(default)]
    asked: Option<Signal<Option<String>>>,
) -> Element {
    // The reader's text size (`reading.text_zoom`), read here so a change draws the frame again;
    // a frame with no window around it (a test's) is at its own size.
    let settings = use_hook(try_consume_context::<Signal<mail_ui_model::settings::MailSettings>>);
    let text = settings.map_or(1.0, |settings| settings.read().reading.text_zoom.factor());
    let height = use_signal(|| None::<f32>);
    let selections = use_hook(try_consume_context::<mail_ui_kit::frame_selection::Selections>);
    {
        let selections = selections.clone();
        use_drop(move || {
            if let (Some(selections), Some(message)) = (selections, message) {
                selections.let_go(message);
            }
        });
    }
    // The page is drawn at the text size, and a sender's page that would then be wider than the
    // frame smaller, to fit it (`super::fit::shrink`); a plain-text message wraps to the frame
    // and is never wider. It opens at what it was fitted to when it was last open, if it was.
    let page = use_memo(use_reactive!(|html| words::fingerprint(&html)));
    let kept = move |text: f32| super::fit::remembered(*page.peek(), text);
    let opened = use_hook(|| super::fit::opening(kept(text), text));
    let mut zoom = use_signal(|| opened.zoom);
    let mut relaxed = use_signal(|| opened.relaxed);
    // The frame's node, and what was selected in it when the last left press let go.
    let mut frame = use_signal(|| None::<Rc<MountedData>>);
    let mut reading = use_signal(Vec::<Task>::new);
    // Measure the frame again from the start, for the text size `text`: what was being measured
    // for another size stops, so it cannot set the zoom back.
    let mut measure = move |text: f32| {
        for task in reading.write().drain(..) {
            task.cancel();
        }
        let Some(frame) = frame.peek().clone() else {
            return;
        };
        let mut started = Vec::new();
        if fit {
            started.push(super::fit::follow(frame.clone(), height));
        }
        if plain {
            started.push(super::fit::hold(frame, zoom));
        } else {
            let page = *page.peek();
            let kept = super::fit::remembered(page, text);
            started.push(super::fit::shrink(frame, zoom, relaxed, text, page, kept));
        }
        reading.set(started);
    };
    // Start the page as it opens for `text`, and measure it again.
    let mut open = move |text: f32| {
        let opened = super::fit::opening(kept(text), text);
        zoom.set(opened.zoom);
        if *relaxed.peek() != opened.relaxed {
            relaxed.set(opened.relaxed);
        }
        measure(text);
    };
    // The size the frame was last measured for, and the page: a new one of either opens again.
    let mut measured = use_signal(|| (text, *page.peek()));
    use_effect(use_reactive!(|text| {
        let now = (text, page());
        let was = *measured.peek();
        if (was.0 - now.0).abs() >= 0.001 || was.1 != now.1 {
            measured.set(now);
            open(text);
        }
    }));
    // The frame's document: a new one only for other html or cells let go, as each is parsed and
    // laid out again; a zoom is set on the one there (`super::fit::shrink`). With which document
    // it is, for what was selected in it.
    let document = use_memo(use_reactive!(|html| {
        let (zoom, relaxed) = (*zoom.peek(), relaxed());
        let which = {
            use std::hash::{DefaultHasher, Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            (page(), zoom.to_bits(), relaxed).hash(&mut hasher);
            hasher.finish()
        };
        (super::fit::zoomed(&html, zoom, relaxed), which)
    }));
    let (html, key) = document();
    let found = use_hook(try_consume_context::<super::found::Frames>);
    // What a find marked is marked again, and its match brought back to the middle, in a new
    // document, or one drawn at another zoom, whose lines have moved.
    {
        let found = found.clone();
        use_effect(move || {
            let _ = (document.read().1, zoom());
            if let Some(found) = &found {
                found.changed();
            }
        });
    }
    {
        let found = found.clone();
        use_drop(move || {
            if let (Some(found), Some(message)) = (found, message) {
                found.gone(message);
            }
        });
    }
    let style = match (fit, height()) {
        (true, Some(px)) => format!("height: {px}px;"),
        _ => String::new(),
    };
    let keys = use_keys();
    // Kept with the document it was taken in (`key`): a frame given other html is a new
    // document, whose nodes and offsets are not these.
    let mut held = use_signal(|| None::<(u64, words::Held)>);
    rsx! {
        iframe {
            class: "html",
            "data-plain": plain,
            "data-fit": fit,
            style,
            onmounted: move |event: MountedEvent| {
                let mounted = event.data();
                frame.set(Some(mounted.clone()));
                if let (Some(found), Some(message)) = (&found, message) {
                    found.drawn(message, mounted.clone());
                }
                if let (Some(selections), Some(message)) = (&selections, message) {
                    selections.hold(message, &mounted);
                }
                measure(text);
            },
            onpointerup: move |event: PointerEvent| {
                if event.trigger_button() == Some(MouseButton::Primary) {
                    let Some(mounted) = frame.peek().clone() else {
                        return;
                    };
                    spawn(async move {
                        held.set(words::selected(&mounted).await.map(|words| (key, words)));
                    });
                }
            },
            onkeydown: move |event: KeyboardEvent| {
                let said = mail_ui_kit::actions::heard(keys, &event, false);
                if said == Some(mail_ui_kit::actions::Heard::Standard(StandardAction::SelectAll)) {
                    event.stop_propagation();
                    let Some(mounted) = frame.peek().clone() else {
                        return;
                    };
                    spawn(async move {
                        held.set(words::select_all(&mounted).await.map(|words| (key, words)));
                    });
                }
            },
            oncontextmenu: move |_| {
                let (Some(mut asked), Some(mounted), Some(words)) = (
                    asked,
                    frame.peek().clone(),
                    held.peek().clone().filter(|(of, _)| *of == key).map(|(_, words)| words),
                ) else {
                    return;
                };
                asked.set(Some(words.text.clone()));
                spawn(async move { words::reselect(&mounted, &words).await });
            },
            "data-frame-tag": tag,
            "sandbox": "",
            srcdoc: "{html}",
            title: "The message as the sender laid it out",
        }
    }
}

/// One message body, drawn as the sandboxed frame.
#[component]
pub(super) fn MessageView(
    message_id: MessageId,
    html: String,
    #[props(default)] holder: Option<mail_ui_kit::original::Holder>,
    #[props(default)] plain: bool,
    #[props(default)] fit: bool,
    /// The words a right click on the frame was on (`Sandbox`).
    #[props(default)]
    asked: Option<Signal<Option<String>>>,
) -> Element {
    rsx! {
        Sandbox {
            html,
            plain,
            fit,
            message: Some(message_id),
            tag: holder.map(|holder| holder.tag(message_id)),
            asked,
        }
    }
}
