//! The Dioxus shell.
//!
//! Thin on purpose: every decision lives in [`mail_ui_kit::view`], which is tested without a window.
//! What is here is layout, event wiring, and the one thing a UI can get dangerously wrong —
//! rendering a stranger's HTML.

pub mod ask;
pub mod emoji;
pub mod handoff;
pub mod spelling;
pub mod tidy;

mod add_account;
mod app;
mod command;
mod compose;
pub use compose::discarding_draft;
mod contacts;
pub mod default_mail;
mod erase;
mod fetching;
mod folder_open;
mod follow_up;
mod frame;
mod keyboard;
mod launch;
mod launcher_count;
mod list;
mod marked;
mod no_account;
mod original_frame;
mod organise;
mod page;
mod provider_chip;
mod reauth;
mod row;
mod rules;
mod server_search;
mod settings_window;
mod sidebar;
mod space_menu;
mod style;
mod switch;
mod view_groups;
mod views;
mod window;
mod window_core;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
pub(in crate::ui) mod kit_tests;
#[cfg(test)]
pub(in crate::ui) mod reader_tests;
#[cfg(test)]
mod shell_tests;
#[cfg(test)]
mod tips_tests;

pub use ds_blitz::LaunchError;
pub use launch::{KeepBadge, Program, Resident, run};

/// The window on Blitz, for a test to drive through `ds_harness::Harness`.
pub mod native {
    pub use super::add_account::{
        AddAccountWindows, Ask as AddAccountAsk, Browse as AddAccountBrowse, DirectSignIns,
        OpenAddAccount, Opened as AddAccountOpened, SignInDirectly, Wiring as AddAccountWiring,
        add_account_root,
    };
    pub use super::compose::Dictionaries;
    pub use super::follow_up::Notices;
    pub use super::launch::DesktopSettings;
    pub use super::launch::gate::{Found, found as damaged_files};
    pub use super::launch::native::{contexts, contexts_passing, live_root, root};
    pub use super::provider_chip::read as read_icons;
    pub use super::original_frame::OriginalFrame;
    pub use super::server_search::{Search, ServerSearcher};
    pub use super::settings_window::settings_root;
    pub use super::window::{MessageOpen, message_root};
    pub use crate::handoff::{ActivationToken, Requests};
}
