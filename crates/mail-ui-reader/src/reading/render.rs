//! The renderings the reader sends off the thread that draws (`mail_ui_kit::cache` keeps what they
//! land in).

use dioxus::prelude::*;
use mail_client::Client;
use mail_client::message::{Frames, Key, Sent};
use mail_domain::Message;
use mail_reader::SanitizePolicy;
use std::sync::Arc;

/// Render `later` on a blocking thread, then move `landed` so the reader draws them.
pub(super) fn render_later(
    frames: Arc<Frames>,
    client: Client,
    later: Vec<(Message, SanitizePolicy)>,
    sent: std::rc::Rc<std::cell::RefCell<Sent>>,
    mut landed: Signal<u64>,
) {
    let keys: Vec<Key> = later
        .iter()
        .filter_map(|(message, policy)| frames.keyed(message, *policy))
        .collect();
    sent.borrow_mut().start(&keys);
    spawn(async move {
        let _ = tokio::task::spawn_blocking(move || {
            for (message, policy) in &later {
                let _ = client.framing(&frames).rendered(message, *policy);
            }
        })
        .await;
        sent.borrow_mut().finish(keys);
        landed += 1;
    });
}
