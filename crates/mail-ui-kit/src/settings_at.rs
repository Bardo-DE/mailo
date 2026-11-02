//! Where the Settings window is asked to be, as the parts that open it see it: a page, an account's
//! page, a Space's, a person's card ([`SettingsAt`]); where it is asked to be next
//! ([`SettingsAsked`]); and what opens it ([`SettingsWindows`], a root context the program
//! gives, quire's event loop or a test's recorder). The window itself is `mail-ui`'s.

use crate::space::SpaceId;
use crate::view::{AccountStep, AccountsPage, AccountsPane, ContactsPage, PageId, SpacesPage};
use dioxus::prelude::*;
use ds::prelude::*;
use porter_core::AccountId;
use std::sync::Arc;
use tokio::sync::watch;

/// Where the Settings window is asked to be: a page, one account's page beside Accounts' list,
/// one Space's pushed over Spaces, or one person's card pushed over Contacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAt {
    Page(PageId),
    Account(AccountId),
    Space(SpaceId),
    Card(mail_client::contacts::CardId),
}

impl SettingsAt {
    /// The page of the sidebar it is on.
    pub fn page(&self) -> PageId {
        match self {
            SettingsAt::Page(page) => *page,
            SettingsAt::Account(_) => PageId::ACCOUNTS,
            SettingsAt::Space(_) => PageId::SPACES,
            SettingsAt::Card(_) => PageId::CONTACTS,
        }
    }

    /// The Accounts pane as it stands there: the first account, or the account asked for.
    pub fn pane(&self) -> AccountsPane {
        match self {
            SettingsAt::Page(_) | SettingsAt::Space(_) | SettingsAt::Card(_) => {
                AccountsPane::default()
            }
            SettingsAt::Account(account) => AccountsPane {
                shown: Some(AccountsPage::Account(account.clone())),
                step: AccountStep::Showing,
            },
        }
    }
}

impl SettingsAt {
    /// The Spaces pane as it stands there: the list, or the Space's page over it.
    pub fn spaces_pane(&self) -> PanePath<SpacesPage> {
        let list = PanePath::new(SpacesPage::List);
        match self {
            SettingsAt::Space(space) => list.pushed(SpacesPage::Space(*space)),
            SettingsAt::Page(_) | SettingsAt::Account(_) | SettingsAt::Card(_) => list,
        }
    }

    /// The Contacts pane as it stands there: the people and groups, or a card over them.
    pub fn contacts_pane(&self) -> PanePath<ContactsPage> {
        let list = PanePath::new(ContactsPage::List);
        match self {
            SettingsAt::Card(card) => list.pushed(ContactsPage::Person(card.clone())),
            SettingsAt::Page(_) | SettingsAt::Account(_) | SettingsAt::Space(_) => list,
        }
    }
}

impl Default for SettingsAt {
    fn default() -> Self {
        SettingsAt::Page(PageId::default())
    }
}

/// Whatever opens the Settings window: quire's event loop in the launched window, a recorder in
/// a test.
pub trait OpenSettings: Send + Sync + 'static {
    /// Open the Settings window, or raise it when it is open, turned to `at` when it is asked
    /// for and where it is otherwise.
    fn open(&self, at: Option<SettingsAt>);
}

/// Where the Settings window is next, as a root context every window shares: the main window puts
/// where it asks for here, a Settings window opening starts there, and an open one turns to it.
/// Without one (a test's single window) the window opens on General.
#[derive(Clone)]
pub struct SettingsAsked(Arc<watch::Sender<SettingsAt>>);

impl Default for SettingsAsked {
    fn default() -> Self {
        SettingsAsked(Arc::new(watch::channel(SettingsAt::default()).0))
    }
}

impl std::fmt::Debug for SettingsAsked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SettingsAsked")
            .field(&*self.0.borrow())
            .finish()
    }
}

impl SettingsAsked {
    /// Where it was asked to be last.
    pub fn at(&self) -> SettingsAt {
        self.0.borrow().clone()
    }

    /// Ask for `at`. Sent even when it is where it was asked to be last: the window may have been
    /// turned elsewhere since.
    pub fn ask(&self, at: SettingsAt) {
        self.0.send_replace(at);
    }

    /// What a Settings window hears each ask through.
    pub fn heard(&self) -> watch::Receiver<SettingsAt> {
        self.0.subscribe()
    }
}

/// Where ⌘, goes, as a root context: the program's (quire's `open_window`), or a test's recorder.
#[derive(Clone)]
pub struct SettingsWindows(pub Arc<dyn OpenSettings>);

impl std::fmt::Debug for SettingsWindows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SettingsWindows")
    }
}

/// Open Settings, or raise it on the page it shows.
pub fn open() {
    ask(None);
}

/// Open Settings on `page`, or raise it and turn it to `page`.
pub fn open_at(page: PageId) {
    ask(Some(SettingsAt::Page(page)));
}

/// Open Settings on `account`'s page, beside Accounts' list, or raise it and turn it there.
pub fn open_account(account: AccountId) {
    ask(Some(SettingsAt::Account(account)));
}

/// Open Settings on `space`'s page, pushed over Spaces, or raise it and turn it there: the
/// Space's menu's Edit Space….
pub fn open_space(space: SpaceId) {
    ask(Some(SettingsAt::Space(space)));
}

/// Open Settings on `card`'s page, pushed over Contacts, or raise it and turn it there: a
/// sender's Show Contact Card.
pub fn open_card(card: mail_client::contacts::CardId) {
    ask(Some(SettingsAt::Card(card)));
}

fn ask(at: Option<SettingsAt>) {
    if let Some(windows) = try_consume_context::<SettingsWindows>() {
        windows.0.open(at);
    }
}
