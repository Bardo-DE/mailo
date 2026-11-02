//! The composer: a message being written, as the window draws it, and the draft's life from the
//! first key to the send, the later and the discard.
//!
//! It sits beside the window's other parts and below the window: it names `mail-ui-kit` and the
//! words of `mail-ui-model`, and no other part. What the window asks of it (a page for a reply, the
//! outbox pill, the park on a switch) it gives as functions and props, and what it needs the
//! window to do it asks through a seam the kit holds. Like `mail-ui` it names no `mail-core`,
//! `mail-store` or `mail-runtime`.

pub mod compose;
pub mod emoji;
pub mod spelling;

/// The composer's rules: its page, its rows and its floats. Layout only; the window lays its own
/// sheet out around it (`mail-ui`'s `style`).
pub const STYLE: &str = include_str!("style/composer.css");
