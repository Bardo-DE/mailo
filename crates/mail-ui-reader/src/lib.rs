//! The reader: a conversation's messages as the window draws them, and what is drawn from one
//! (the print of it, a receipt, an invitation, the way out of a list, the sender's brand and
//! checks).
//!
//! It sits beside the window's other parts and below the window: it names `mail-ui-kit` and the
//! words of `mail-ui-model`, and no other part, and what it needs another part to do it is handed
//! in as a prop or a seam the kit holds. Like `mail-ui` it names no `mail-core`, `mail-store` or
//! `mail-runtime`.

pub mod brand;
pub mod checks;
pub mod invite;
pub mod print;
pub mod reading;
pub mod receipt;
pub mod sender;
pub mod unsubscribe;

/// The reader's rules, in the cascade's order: the reader's column, then the invitation card.
/// Layout only; the window lays its own sheet out around it (`mail-ui`'s `style`).
pub const STYLE: &str = concat!(
    include_str!("style/reader.css"),
    include_str!("style/invite.css")
);
