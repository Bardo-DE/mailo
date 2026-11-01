//! The Space's menu: quire's (`SpaceMenu`), with mail's Accounts ▸ in its slot.
//!
//! Appearance, New Space and Delete are the kit's; Edit Space… opens the Space's page in Settings
//! (`settings_window::open_space`), where its name, colour, theme and accounts are. Accounts
//! lists every account with a check on the ones the Space shows; a pick puts it in or takes it
//! out and leaves the menu open, so several can be switched. Under them is Add Account: the
//! providers that sign in straight in the browser, then Other Account…, or with none of those
//! the window itself. Deleting a Space says the mail is not affected, and forgets the Space's
//! Today.

use crate::ui::add_account::{AddPick, Join, add_menu, offered_redirects, picked};
use dioxus::prelude::*;
use ds::components::app::spaces::SpaceMenu;
use ds::components::menus::item::item::{AfterPick, MenuItem};
use ds::prelude::{Availability, Check, Icon, MenuImage};
use mail_client::profile::ProfileId;
use mail_ui_kit::appearance::WindowDirs;
pub(in crate::ui) use mail_ui_kit::data::account_names as accounts;
use mail_ui_kit::data::use_account_rows;
pub(in crate::ui) use mail_ui_kit::space::toggled;
use mail_ui_kit::space::{Handle, Mail, Recall, SpaceId};
use mail_ui_kit::today::Today;
use mail_ui_model::copy;
use porter_core::AccountId;

/// A pick in Accounts ▸: an account put in or taken out, or a way to add one.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::ui) enum AccountPick {
    Toggle(AccountId),
    Add(AddPick),
}

/// Accounts ▸ for a Space showing `shown`: one row per account, checked when the Space shows
/// it, then Add Account. Any account may be put in or taken out: one may be in no Space.
/// `redirects` are the providers that sign in straight in the browser: with any, Add Account is
/// a submenu of them and Other Account…; with none, it opens the window.
pub(in crate::ui) fn accounts_menu(
    shown: &[AccountId],
    accounts: &[(AccountId, String)],
    redirects: &[(porter_provider::Issuer, String)],
) -> Vec<MenuItem<AccountPick>> {
    let mut rows: Vec<MenuItem<AccountPick>> = accounts
        .iter()
        .map(|(id, name)| {
            let on = shown.contains(id);
            MenuItem::new(AccountPick::Toggle(id.clone()), name.clone())
                .with_check(if on { Check::On } else { Check::Off })
                .with_after(AfterPick::KeepOpen)
        })
        .collect();
    if !rows.is_empty() {
        rows.push(MenuItem::Separator);
    }
    rows.push(if redirects.is_empty() {
        MenuItem::new(AccountPick::Add(AddPick::Other), copy::menus::ADD_ACCOUNT)
            .with_image(MenuImage::Icon(Icon::Plus))
    } else {
        MenuItem::Submenu {
            title: copy::menus::ADD_ACCOUNT_SUBMENU.to_owned(),
            image: Some(MenuImage::Icon(Icon::Plus)),
            availability: Availability::Enabled,
            children: add_menu(redirects)
                .into_iter()
                .map(|item| item.map(&AccountPick::Add))
                .collect(),
        }
    });
    vec![MenuItem::Submenu {
        title: copy::menus::ACCOUNTS.to_owned(),
        image: None,
        availability: Availability::Enabled,
        children: rows,
    }]
}

/// The Space's menu, wherever it was opened.
#[component]
pub(in crate::ui) fn MailSpaceMenu(handle: Handle, mut today: Signal<Today>) -> Element {
    let rows = use_account_rows();
    let every = accounts(&rows.read());
    let extra = handle
        .open()
        .and_then(|open| {
            let held = handle.spaces();
            let spaces = held.read();
            spaces
                .get(open.space)
                .map(|space| accounts_menu(&space.payload.accounts, &every, &offered_redirects()))
        })
        .unwrap_or_default();
    rsx! {
        SpaceMenu::<Mail, Recall, AccountPick> {
            handle,
            new_payload: move |()| Mail::new_in(&ProfileId::default()),
            extra,
            on_extra: move |(space, pick): (SpaceId, AccountPick)| match pick {
                AccountPick::Toggle(account) => {
                    handle.apply(space, |one| {
                        one.payload.accounts = toggled(&one.payload.accounts, account);
                    });
                }
                AccountPick::Add(pick) => picked(pick, Join::Into(space)),
            },
            kept: "Your mail".to_owned(),
            on_edit: move |space: SpaceId| mail_ui_kit::settings_at::open_space(space),
            on_deleted: move |space: SpaceId| {
                today.write().drop_space(space);
                if let Some(dirs) = try_consume_context::<WindowDirs>() {
                    let _ = mail_ui_kit::today::save(&dirs.state, &today.peek());
                }
            },
        }
    }
}

#[cfg(test)]
#[path = "space_menu/tests.rs"]
mod tests;
