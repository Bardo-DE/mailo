//! The root every per-account page of Settings shares (Accounts, Signatures, Rules, Vacation
//! Reply), as System Settings lists its Internet Accounts: a grouped list of the accounts, each by
//! its provider's mark and its address, a chevron row that pushes that account's page for the
//! tab into the tab's own pane. Each tab keeps its pages in its own `PaneStack` (the path is in
//! the window's `Shell`), and gives [`AccountList`] its accounts and what to do when one is
//! opened; the list shows even with one account, so every tab is click first, then edit.
//!
//! A tab's own list actions (Accounts' Add Account… and Other Keys) stand in the `under` slot,
//! below the accounts.
//!
//! With no account at all there is nothing to open: [`NoAccounts`] stands in for the list, saying
//! so with an Add Account… to start from, as the Notifications switches say it.

use dioxus::prelude::*;
use ds::components::fields::field_row::FieldRow;
use ds::components::lists::list::model::ListStyle;
use ds::components::lists::row::size::RowSize;
use ds::host::measure::MountedRef;
use ds::prelude::*;
use ds::root::common::Common;
use ds::style::tokens::control_size::ControlSize;
use mail_ui_kit::add_account::Join;
use mail_ui_kit::data::AccountRow;
use mail_ui_kit::press::on_primary;
use mail_ui_model::copy;
use porter_core::AccountId;

/// The element id of the row that opens `account` on the page whose pane is `pane`, where the
/// keyboard goes back to.
pub fn row_id(pane: &str, account: &AccountId) -> String {
    format!("{pane}-open-{account}")
}

/// The accounts `rows` as a grouped list, a chevron each, in a form with `under` below it.
/// `pane` names the page for its elements' ids; `onopen` hears the one pressed. `statuses` is
/// what a row says at its end in place of the chevron, for the accounts it has one for.
#[component]
pub fn AccountList(
    pane: &'static str,
    label: &'static str,
    rows: Vec<AccountRow>,
    onopen: EventHandler<AccountId>,
    #[props(default)] statuses: Vec<(AccountId, String)>,
    #[props(default)] under: Option<Element>,
) -> Element {
    let items: Vec<ListItem<String>> = rows
        .iter()
        .map(|row| {
            let shown = row.shown();
            // The address under a name the person gave: where the name is the address, once is
            // enough.
            let detail = (shown != row.address && !row.is_local())
                .then(|| TextLine::from(row.address.clone()));
            let accessory = statuses
                .iter()
                .find(|(id, _)| *id == row.id)
                .map_or(Accessory::Chevron, |(_, status)| {
                    Accessory::Text(status.clone())
                });
            let id = row.id.clone();
            ListItem::row(
                row.id.to_string(),
                shown.clone(),
                rsx! {
                    Row {
                        leading: mail_ui_kit::provider_chip::account_mark(row),
                        title: shown.clone(),
                        detail,
                        size: RowSize::Settings,
                        accessory,
                        onclick: move |_| onopen.call(id.clone()),
                        common: Common {
                            id: Some(row_id(pane, &row.id)),
                            aria_label: Some(copy::aria::details_for(&shown)),
                            ..Common::default()
                        },
                    }
                },
            )
        })
        .collect();
    rsx! {
        Form {
            FormSection {
                if rows.is_empty() {
                    NoAccounts {}
                } else {
                    List::<String> {
                        label,
                        items,
                        style: ListStyle::Grouped,
                    }
                }
            }
            if let Some(under) = under {
                {under}
            }
        }
    }
}

/// The Add Account… the pages that need an account offer: its press, the menu of the providers
/// that sign in straight in the browser while it is open, and the element it hangs from.
#[derive(Clone, Copy)]
pub struct Adding {
    offered: Signal<Option<Vec<(porter_provider::Issuer, String)>>>,
    anchor: Signal<Option<MountedRef>>,
}

impl std::fmt::Debug for Adding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Adding(..)")
    }
}

/// An [`Adding`] for the component that calls it; draw its [`Adding::menu`] beside the element
/// that is given [`Adding::mounted`] and starts it.
pub fn use_adding() -> Adding {
    Adding {
        offered: use_signal(|| None),
        anchor: use_signal(|| None),
    }
}

impl Adding {
    /// Start adding an account that joins no Space: Settings is every account, in no Space.
    pub fn start(mut self) {
        mail_ui_kit::add_account::direct::add_from(Join::Nowhere, move |offered| {
            self.offered.set(Some(offered))
        });
    }

    /// The `mounted` of the element the menu hangs from.
    pub fn mounted(mut self) -> EventHandler<MountedEvent> {
        EventHandler::new(move |event: MountedEvent| {
            self.anchor.set(Some(MountedRef(event.data())));
        })
    }

    /// The menu while it is open.
    pub fn menu(mut self) -> Element {
        let Some(redirects) = (self.offered)() else {
            return rsx! {};
        };
        rsx! {
            mail_ui_kit::add_account::direct::AddMenu {
                anchor: (self.anchor)(),
                redirects,
                join: Join::Nowhere,
                on_close: move |()| self.offered.set(None),
            }
        }
    }
}

/// The row that says there is no account yet, with Add Account… beside it: for a section of a
/// page that has no account to show.
#[component]
pub fn NoAccounts() -> Element {
    let adding = use_adding();
    rsx! {
        FieldRow { label: copy::empty::ADD_AN_ACCOUNT_FIRST,
            Button {
                label: copy::controls::ADD_ACCOUNT,
                size: ControlSize::Small,
                common: Common {
                    aria_label: Some(copy::aria::ADD_ACCOUNT.to_owned()),
                    mounted: Some(adding.mounted()),
                    ..Common::default()
                },
                onclick: on_primary(move || adding.start()),
            }
        }
        {adding.menu()}
    }
}
