//! What the window keeps of the rendered bodies of `mail_client::message`: the cache it owns and
//! the warming that keeps the top of the list rendered before anyone opens it.
//!
//! The cache is one [`Frames`] for the whole app, a root context (`ui::launch::contexts`); a
//! reader drawn with none around it (a test's) gets one of its own. So is the [`Looks`], the
//! other thing the reader keeps per message: the checks, the invitation, the receipt, the list's
//! way out and what OpenPGP or S/MIME opened. Opened bodies are the `Looks`', handed to the
//! `Frames` as the one thing it asks of them.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use dioxus::prelude::*;
use mail_client::message::{FIRST_PAGE, Frames, Looks, ahead};
use mail_client::query::Conversation;
use mail_domain::{Message, ThreadId, ThreadSummary};
use mail_reader::SanitizePolicy;

use crate::view::Shell;

/// A new, empty set of the reader's per-message lookups.
pub fn looks() -> Arc<Looks> {
    Arc::new(Looks::new())
}

/// The app's lookups: the root context, or a set of this component's own where there is none.
pub fn use_looks() -> Arc<Looks> {
    use_hook(|| try_consume_context::<Arc<Looks>>().unwrap_or_else(looks))
}

/// A new cache of rendered bodies, asking `looks` which messages it has opened.
pub fn frames(looks: Arc<Looks>) -> Arc<Frames> {
    Arc::new(Frames::new(Arc::new(move |message: &Message| {
        looks.opened_body(message).map(|parsed| *parsed)
    })))
}

/// The app's cache of rendered bodies: the root context, or a cache of this component's own where
/// there is none.
pub fn use_frames() -> Arc<Frames> {
    let looks = use_looks();
    use_hook(|| try_consume_context::<Arc<Frames>>().unwrap_or_else(|| frames(looks)))
}

/// Keep what the person is about to open rendered: the top of the list once it is drawn, and
/// the neighbours of the open conversation whenever it changes. Their bodies, where they are not
/// here yet, are what the next body pass fetches first (`mail_client::wanted`).
///
/// Warmed under the frame's policy, which is the one a conversation opens with: showing remote
/// images is asked for per conversation, and opening another one clears it.
///
/// The open conversation is kept read by the client too, so it draws at once rather than a frame
/// after; the neighbours are only rendered, not kept read.
pub fn use_warming(shell: Signal<Shell>, threads: Memo<Vec<ThreadSummary>>) {
    let frames = use_frames();
    let client = crate::live::use_client();
    let kept = use_hook(|| Rc::new(RefCell::new(Vec::new())));
    // A memo, so that only a change of the open conversation counts, not every keystroke the
    // shell sees.
    let open = use_memo(move || shell.read().open);
    // Warmed as the reader will look them up: with the body the settings prefer, and again when
    // that changes.
    let settings = use_hook(try_consume_context::<Signal<mail_ui_model::settings::MailSettings>>);
    use_effect(move || {
        if let Some(settings) = settings {
            frames.prefer(settings.read().reading.prefer_plain_text.into());
        }
        let list: Vec<ThreadId> = threads.read().iter().map(|thread| thread.id).collect();
        let order = ahead(open(), &list, FIRST_PAGE);
        // The same order is what a body pass fetches first, with the open conversation ahead of
        // it: a message without its body yet cannot be rendered ahead, only fetched ahead.
        let bodies: Vec<ThreadId> = open().into_iter().chain(order.iter().copied()).collect();
        mail_client::wanted::ask_first(&bodies);
        // Only the open conversation is kept read as a whole: a value that every commit reads
        // again, body by body. The neighbours are rendered by `warm` below, which reads each
        // once on its own thread and keeps nothing of it. Asked for before the last one is let
        // go, so the same conversation is not read again.
        let next = client.prime(open().into_iter().map(Conversation));
        *kept.borrow_mut() = next;
        client.framing(&frames).warm(order, SanitizePolicy::FRAME);
    });
}
