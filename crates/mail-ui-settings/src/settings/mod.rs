//! Settings: a window of its own, as the desktop's Settings app is and as a Mac app's settings
//! are, opened with ⌘, the gear in the sidebar's foot or the command menu, and raised if it is
//! already open. Laid out as detent lays out a program's page: a sidebar of pages, each a column
//! of titled groups of rows.
//!
//! Pages: General, Accounts, Contacts, Signatures, Spaces, Composing, Viewing, Notifications, Privacy, Junk Mail, Rules,
//! Vacation Reply and Keyboard. General, Composing, Viewing, Notifications, Privacy and Junk Mail draw
//! mailo's settings schema (`mail_ui_model::settings`) as groups of rows ([`pages`], each row a key's,
//! drawn by [`keys`]), the same keys detent draws on mailo's page and writes to the same
//! `mailo/settings.toml`. Accounts lists each account, whose row pushes its own page into the
//! pane ([`account`]): its mail kept here, its encryption keys, and its removal. Signatures lists
//! every address's signatures, each written in the composer's body on a page of its own
//! ([`signatures`]). Rules and Keyboard are drawn by their
//! modules (`rules`, `keyboard`). A page that has pages of its own pushes them, as System
//! Settings does, with quire's `PaneStack`; a sheet is only for making something or asking
//! before something goes. Spaces lists each Space, whose row pushes its page: its name, colour,
//! theme and accounts, and its deletion ([`spaces`]); the Space's menu, a right click on the
//! Space's name, is the quick way to the same, and its Edit Space… opens the Space's page here
//! ([`open_space`]). Contacts is the address book: every person's card and every group, each
//! on a page of its own. A person is edited from their name in mail.
//!
//! Every way into one of those pages from the main window (⌘K, the composer's bar) opens the
//! window on that page, or turns the open window to it ([`open_at`]), and the composer's way to
//! an account's keys opens it on that account's page ([`open_account`]): where it is asked to be
//! is put in [`SettingsAsked`], which every window shares, and the Settings window follows it.
//!
//! The window is quire's (`ds_blitz::open_window`), with the root contexts every window of the
//! app is given. It keeps its own `Shell` for its pages and the sheets it opens. What it changes
//! in the store every window hears from the one client; what it writes to the configuration files
//! it tells the other windows (`ui::configured`), which read the settings, the keymap and the
//! Spaces again, so a switch here is followed there without a watch on the file. The call goes
//! through [`SettingsWindows`] when the window was given one, which is how a test sees what would
//! open: quire's harness has no event loop to open a window on.

pub mod account;
pub mod account_split;
pub mod accounts;
pub mod calendar;
pub mod keys;
pub mod offline;
pub mod page;
pub mod pages;
pub mod push_relay;
pub mod root;
pub mod rows;
pub mod spaces;
pub mod syncing;
pub mod vips;

pub use account::Seams as AccountSeams;
pub use page::PAGES;
pub use root::{settings_root_with, settings_window_with};

use dioxus::prelude::*;
use ds::components::chrome::sidebar::Sidebar;
use ds::components::chrome::sidebar_model::SidebarSection;
use ds::prelude::*;
use ds_blitz::{WindowHandle, WindowSpec};
use mail_ui_kit::settings_at::{OpenSettings, SettingsAsked, SettingsAt, SettingsWindows};
use mail_ui_kit::view::{AccountStep, ContactsPage, PageId, RulesAt, Shell, SpacesPage};
use mail_ui_model::copy;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// What the window and its command are called.
pub const TITLE: &str = copy::titles::SETTINGS;

/// The size the window opens at, in logical pixels.
const SIZE: (u32, u32) = (960, 640);

/// What the window hands Settings that it cannot draw itself: the Signatures page, which is written
/// in the composer's body, and the window's stylesheet.
#[derive(Clone, Copy, PartialEq)]
pub struct Slots {
    /// The Signatures page, drawn over the window's `Shell`.
    pub signatures: Callback<Signal<Shell>, Element>,
    /// The window's stylesheet, laid inside the Settings window's frame.
    pub style: &'static str,
}

impl std::fmt::Debug for Slots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Slots")
    }
}

/// The Settings window this window opened, kept so that opening it again raises it.
#[derive(Clone, Default, Debug)]
pub struct SettingsOpened(Rc<RefCell<Option<WindowHandle>>>);

/// Keep the Settings window this one opens. Called once, from `App`.
pub fn use_settings_opened(root: fn() -> Element) {
    use_context_provider(SettingsOpened::default);
    // Where "Settings" goes when the program gave nothing else: quire's event loop.
    use_hook(|| {
        if try_consume_context::<SettingsWindows>().is_none() {
            provide_context(SettingsWindows(Arc::new(Quire(root))));
        }
    });
}

/// [`OpenSettings`] as the program has it: the window asked for, quire's event loop, raised if
/// it is already open.
struct Quire(fn() -> Element);

impl OpenSettings for Quire {
    fn open(&self, at: Option<SettingsAt>) {
        if let (Some(at), Some(asked)) = (at, try_consume_context::<SettingsAsked>()) {
            asked.ask(at);
        }
        quire(self.0);
    }
}

/// Show `page` in this Settings window, each per-account page on the account it last showed and
/// with what it pushed closed, as System Settings shows a pane chosen in its sidebar.
pub fn go(mut shell: Signal<Shell>, page: PageId) {
    let mut write = shell.write();
    write.settings = Some(page);
    if write.accounts_pane.step != AccountStep::Removing {
        write.accounts_pane.step = AccountStep::Showing;
    }
    write.rules.path = PanePath::new(RulesAt::List);
    write.spaces_pane = PanePath::new(SpacesPage::List);
    write.contacts_pane = PanePath::new(ContactsPage::List);
}

/// Turn this Settings window to `at`: a page, an account's page beside Accounts' list, or a
/// Space's over Spaces.
pub fn go_to(shell: Signal<Shell>, at: SettingsAt) {
    match at {
        SettingsAt::Page(page) => go(shell, page),
        SettingsAt::Account(account) => {
            go(shell, PageId::ACCOUNTS);
            accounts::push(shell, account);
        }
        SettingsAt::Space(space) => {
            go(shell, PageId::SPACES);
            spaces::push(shell, space);
        }
        SettingsAt::Card(card) => {
            go(shell, PageId::CONTACTS);
            crate::contacts::page::push(shell, ContactsPage::Person(card));
        }
    }
}

/// Ask quire's event loop for the window, or raise the one already open.
fn quire(root: fn() -> Element) {
    let opened = try_consume_context::<SettingsOpened>().unwrap_or_default();
    let existing = opened.0.borrow().clone();
    if let Some(handle) = existing.filter(|handle| mail_ui_kit::window::raise(Some(handle.life())))
    {
        handle.focus();
        return;
    }
    match ds_blitz::open_window(
        WindowSpec::new(TITLE, ds_blitz::WindowSize::new(SIZE.0, SIZE.1)),
        root,
    ) {
        Ok(handle) => {
            *opened.0.borrow_mut() = Some(handle);
        }
        Err(why) => {
            log::warn!("the Settings window would not open: {why}");
            mail_ui_kit::motion::tell(
                mail_ui_model::copy::toasts::would_not_open(TITLE),
                mail_ui_kit::motion::Follow::Nothing,
            );
        }
    }
}

/// The window's content: the sidebar of pages beside the page shown, which scrolls.
#[component]
pub fn SettingsView(shell: Signal<Shell>) -> Element {
    let current = shell.read().settings.unwrap_or_default();
    let items: Vec<ListItem<PageId>> = PAGES
        .iter()
        .copied()
        .map(|each| {
            let id = each.id();
            let selection = if id == current {
                Selection::Selected
            } else {
                Selection::Unselected
            };
            ListItem::row(
                id,
                each.name(),
                rsx! {
                    Row {
                        title: TextLine::from(each.name()),
                        leading: RowLeading::Icon(each.icon()),
                        state: ds::base::vocab::RowState { selection, ..Default::default() },
                        common: ds::root::common::Common {
                            aria_label: Some(each.name().to_owned()),
                            ..Default::default()
                        },
                        onclick: move |_| go(shell, id),
                    }
                },
            )
        })
        .collect();
    let shown = PAGES.iter().copied().find(|each| each.id() == current);
    rsx! {
        div { class: "settings",
            Sidebar::<PageId> {
                label: TITLE,
                sections: vec![SidebarSection::List(items)],
                cursor: Some(current),
                onselect: move |next: PageId| go(shell, next),
            }
            div {
                class: "settings-scroll settings-page",
                "data-page": shown.map(|each| each.name()),
                if let Some(page) = shown {
                    {page.body(shell)}
                }
            }
        }
    }
}
