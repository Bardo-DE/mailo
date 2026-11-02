//! The list: the pane of conversations, its rows, its pages, the search across its header and the
//! saved-view sheet, as the window draws them.
//!
//! It sits beside the window's other parts and below the window: it names `mail-ui-kit` and the
//! words of `mail-ui-model`, and no other part. What it needs the window to do (open Add Account,
//! run a search) it asks through the seams the kit holds. Like `mail-ui` it names no `mail-core`,
//! `mail-store` or `mail-runtime`.

pub mod bar;
pub mod list;
pub mod marked;
pub mod organise;
pub mod page;
pub mod row;
pub mod server_search;
pub mod view_groups;
pub mod views;

/// The list's rules. Layout only; the window lays its own sheet out around it (`mail-ui`'s
/// `style`).
pub const STYLE: &str = include_str!("style/list.css");
