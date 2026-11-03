//! Accounts: a pane of two levels, as System Settings' Internet Accounts is. Its root lists each
//! account on this computer by its address and its provider's mark ([`super::account_list`]), a
//! chevron row that pushes the account's own page ([`super::account`]) into the same pane; then
//! Add Account…; then Other Keys, the keys and certificates no account here owns. No protocol is
//! named here: that is the account's page's.
//!
//! The pages are quire's `PaneStack`; which are shown is the window's `Shell`
//! (`accounts_pane.path`), so a window can be asked onto an account's page (`SettingsAt::Account`)
//! and a removal can go back to the list. The stack takes the back button, Escape and ⌘[, moves
//! the keyboard to the pushed page's back button and, on the way back, to the row that opened it.
//!
//! Which Space shows which account is the Space's menu's; this page is every account at once.

use super::account::{AccountPage, back};
use super::account_list::{AccountList, row_id, use_adding};
use super::rows::link_row;
use dioxus::prelude::*;
use ds::components::lists::list::model::ListStyle;
use ds::prelude::*;
use ds::root::common::Common;
use ds::style::icon::family::PlateFamily;
use mail_ui_kit::common::{add_mark, tile};
use mail_ui_kit::view::{AccountStep, AccountsPage, Shell};
use mail_ui_model::copy;
use porter_core::AccountId;

/// The key of the Add Account… row, which no account id can be.
const ADD: &str = " add";

/// The element id of the Other Keys row, where the keyboard goes back to.
const OTHER_KEYS_ROW: &str = "accounts-pane-open-other-keys";

/// The stack's element id: its back buttons are `accounts-pane-back-root` and `-detail`.
const STACK: &str = "accounts-pane";

/// The pane: the list at its root, an account's page over it.
#[component]
pub fn Accounts(shell: Signal<Shell>) -> Element {
    let path = shell.read().accounts_pane.path.clone();
    let named = mail_ui_kit::data::use_account_rows();
    let title = use_callback(move |page: AccountsPage| match page {
        AccountsPage::List => copy::titles::ACCOUNTS.to_owned(),
        AccountsPage::Account(account) => named
            .read()
            .iter()
            .find(|row| row.id == account)
            .map_or_else(|| account.to_string(), |row| row.shown()),
        AccountsPage::OtherKeys => copy::titles::OTHER_KEYS.to_owned(),
    });
    let page = use_callback(move |page: AccountsPage| match page {
        AccountsPage::List => rsx! { Root { shell } },
        AccountsPage::Account(account) => {
            rsx! { AccountPage { key: "{account}", shell, account } }
        }
        AccountsPage::OtherKeys => rsx! {
            Form {
                mail_ui_kit::pgp::keys::KeysPage { whose: mail_ui_kit::pgp::keys::Whose::Others }
            }
        },
    });
    let opener = use_callback(|page: AccountsPage| match page {
        AccountsPage::List => None,
        AccountsPage::Account(account) => Some(row_id(STACK, &account)),
        AccountsPage::OtherKeys => Some(OTHER_KEYS_ROW.to_owned()),
    });
    rsx! {
        PaneStack::<AccountsPage> {
            path,
            title,
            page,
            opener: Some(opener),
            on_back: move |()| back(shell),
            common: Common { id: Some(STACK.to_owned()), ..Common::default() },
        }
    }
}

/// The root: every account, Add Account…, and Other Keys.
#[component]
fn Root(shell: Signal<Shell>) -> Element {
    // Read again when an account comes or goes.
    let _ = mail_ui_kit::live::moved_for(mail_client::Topics::ACCOUNTS);
    let adding = use_adding();
    let rows = mail_ui_kit::data::use_account_rows()();
    // Add Account… is a list of its own under the accounts, as New Space is under the Spaces. With
    // no account the list says so beside an Add Account… of its own.
    let add = vec![link_row(
        ADD,
        copy::controls::ADD_ACCOUNT,
        add_mark(),
        move |_| adding.start(),
        Common {
            aria_label: Some(copy::controls::ADD_ACCOUNT.to_owned()),
            mounted: Some(adding.mounted()),
            ..Common::default()
        },
    )];
    let other_keys = vec![link_row(
        OTHER_KEYS_ROW,
        copy::titles::OTHER_KEYS,
        tile(Icon::Key, PlateFamily::Amber),
        move |_| other_keys(shell),
        Common {
            id: Some(OTHER_KEYS_ROW.to_owned()),
            ..Common::default()
        },
    )];
    let any = !rows.is_empty();
    let under = rsx! {
        if any {
            FormSection {
                List::<String> { label: copy::aria::ADD_ACCOUNT, items: add, style: ListStyle::Grouped }
            }
        }
        FormSection {
            List::<String> { label: copy::titles::OTHER_KEYS, items: other_keys, style: ListStyle::Grouped }
        }
    };
    rsx! {
        AccountList {
            pane: STACK,
            label: copy::titles::ACCOUNTS,
            rows,
            onopen: move |id: AccountId| push(shell, id),
            under: Some(under),
        }
        {adding.menu()}
    }
}

/// Push `account`'s page over the list. The pane is two levels deep, so it is the list and that
/// page whatever was shown.
pub fn push(mut shell: Signal<Shell>, account: AccountId) {
    let mut write = shell.write();
    write.accounts_pane.path =
        PanePath::new(AccountsPage::List).pushed(AccountsPage::Account(account));
    if write.accounts_pane.step != AccountStep::Removing {
        write.accounts_pane.step = AccountStep::Showing;
    }
}

/// Push the Other Keys page over the list.
fn other_keys(mut shell: Signal<Shell>) {
    shell.write().accounts_pane.path =
        PanePath::new(AccountsPage::List).pushed(AccountsPage::OtherKeys);
}
