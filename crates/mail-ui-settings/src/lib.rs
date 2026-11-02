//! Settings: the window of pages (General, Accounts, Contacts, Spaces, Composing, Viewing,
//! Notifications, Privacy, Junk Mail, Rules, Vacation Reply and Keyboard), and the parts only it
//! draws: the rules and the vacation reply, the address book's page, the keyboard's page and the
//! default mail program's row and question.
//!
//! It sits beside the window's other parts and below the window: it names `mail-ui-kit` and the
//! words of `mail-ui-model`, and no other part. What it needs the window to hand it (the
//! Signatures page, which is written in the composer's body, and the window's stylesheet) it is
//! given as props ([`settings::Slots`]). Like `mail-ui` it names no `mail-core`, `mail-store` or
//! `mail-runtime`.

pub mod contacts;
pub mod default_mail;
pub mod keyboard;
pub mod rules;
pub mod settings;

/// The Contacts page's rules, and the sender's popover's. Layout only; the window lays its own
/// sheet out around it (`mail-ui`'s `style`).
pub const CONTACTS_STYLE: &str = include_str!("style/contacts.css");

/// The Settings window's rules and the Rules page's. Layout only.
pub const STYLE: &str = concat!(
    include_str!("style/settings.css"),
    include_str!("style/rules.css"),
);
