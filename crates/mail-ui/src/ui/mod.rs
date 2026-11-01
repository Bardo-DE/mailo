//! The Dioxus shell.
//!
//! Thin on purpose: every decision lives in [`mail_ui_kit::view`], which is tested without a window.
//! What is here is layout, event wiring, and the one thing a UI can get dangerously wrong —
//! rendering a stranger's HTML.

pub mod ask;
pub mod handoff;
pub mod tidy;

mod add_account;
mod app;
mod command;
pub use mail_ui_compose::compose::discarding_draft;
pub use mail_ui_settings::default_mail;
mod erase;
mod follow_up;
pub(in crate::ui) use mail_ui_kit::frame;
#[cfg(test)]
pub(in crate::ui) use mail_ui_list::row;
pub(in crate::ui) use mail_ui_list::{list, organise, views};
pub(in crate::ui) use mail_ui_settings::{contacts, rules};
mod launch;
mod launcher_count;
mod no_account;
mod original_frame;
mod reauth;
mod settings;
mod sidebar;
mod signatures;
mod space_menu;
mod style;
mod switch;
mod window;
mod window_core;

#[cfg(test)]
pub(in crate::ui) mod compose_tests;
#[cfg(test)]
mod fixtures;
#[cfg(test)]
pub(in crate::ui) mod kit_tests;
#[cfg(test)]
mod list_tests;
#[cfg(test)]
pub(in crate::ui) mod reader_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod shell_tests;
#[cfg(test)]
mod signatures_tests;
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
    pub use super::follow_up::Notices;
    pub use super::launch::DesktopSettings;
    pub use super::launch::gate::{Found, found as damaged_files};
    pub use super::launch::native::{contexts, contexts_passing, live_root, root};
    pub use super::original_frame::OriginalFrame;
    pub use super::settings::settings_root;
    pub use super::window::{MessageOpen, message_root};
    pub use crate::handoff::{ActivationToken, Requests};
    pub use mail_ui_compose::compose::Dictionaries;
    pub use mail_ui_kit::provider_chip::read as read_icons;
    pub use mail_ui_list::server_search::{Search, ServerSearcher};
}
