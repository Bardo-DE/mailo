//! Settings' Vacation Reply: a pane of two levels, as Spaces is. Its root lists each account with
//! a server ([`crate::settings::account_list`]) by its provider's mark and its address, with
//! whether its reply is on; a row pushes that account's reply ([`AwayPage`]) as a page of its own,
//! its back button the way home. An account whose server cannot send a reply is listed all the
//! same, saying so, and its page's switch is greyed.

use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use ds::prelude::*;
use ds::root::common::Common;
use mail_client::Client;
use mail_logic::when::Clock;
use porter_core::AccountId;

use super::away::AwayPage;
use super::server::reach_away;
use super::{Said, push_away};
use crate::settings::account_list::{self, AccountList};
use mail_ui_kit::common::Told;
use mail_ui_kit::data::{AccountRow, use_account_rows};
use mail_ui_kit::menus::when_words;
use mail_ui_kit::view::{Shell, VacationAt};
use mail_ui_model::copy;

/// The stack's element id: its back buttons are `vacation-pane-back-root` and `-detail`.
const STACK: &str = "vacation-pane";

/// The element id of the row that pushes `at`, where the keyboard goes back to.
fn row_id(at: &VacationAt) -> Option<String> {
    match at {
        VacationAt::List => None,
        VacationAt::Account(id) => Some(account_list::row_id(STACK, id)),
    }
}

/// Back to the accounts.
pub fn back(mut shell: Signal<Shell>) {
    let back = shell.peek().vacation_pane.popped();
    shell.write().vacation_pane = back;
}

/// The accounts a reply can be looked at on: every one with a server, whether or not it can send
/// one.
fn replying(rows: &[AccountRow]) -> Vec<AccountRow> {
    rows.iter().filter(|row| !row.is_local()).cloned().collect()
}

/// The pane: the accounts at its root, one account's reply over them.
#[component]
pub fn VacationPage(shell: Signal<Shell>) -> Element {
    let all = use_account_rows();
    let unpushed: Said = use_signal(|| None);
    // Bumped by every reply kept, so the list says it again.
    let mut changed = use_signal(|| 0u64);
    let pushed = use_callback(move |row: AccountRow| {
        changed += 1;
        push_away(row, unpushed);
    });
    let path = shell.read().vacation_pane.clone();
    let title = use_callback(move |at: VacationAt| match at {
        VacationAt::List => copy::titles::VACATION_REPLY.to_owned(),
        VacationAt::Account(id) => replying(&all.read()).iter().find(|row| row.id == id).map_or_else(
            || copy::titles::VACATION_REPLY.to_owned(),
            AccountRow::shown,
        ),
    });
    let page = use_callback(move |at: VacationAt| match at {
        VacationAt::List => rsx! { VacationList { shell } },
        VacationAt::Account(id) => match replying(&all.read()).into_iter().find(|row| row.id == id) {
            // Read again for another account.
            Some(row) => rsx! { AwayPage { key: "{row.id}", row, pushed } },
            None => rsx! {},
        },
    });
    let opener = use_callback(|at: VacationAt| row_id(&at));
    rsx! {
        PaneStack::<VacationAt> {
            path,
            title,
            page,
            opener: Some(opener),
            on_back: move |()| back(shell),
            common: Common { id: Some(STACK.to_owned()), ..Common::default() },
        }
        Told { title: copy::titles::NOT_ON_SERVER, said: unpushed }
    }
}

/// The root: each account, and whether its reply is on.
#[component]
fn VacationList(shell: Signal<Shell>) -> Element {
    // A reply kept, here or anywhere, moves the store.
    let _ =
        mail_ui_kit::live::moved_for(mail_client::Topics::RULES | mail_client::Topics::ACCOUNTS);
    let rows = replying(&use_account_rows().read());
    let client = mail_ui_kit::live::client();
    let now = Utc::now();
    let clock = mail_ui_kit::clock::hours();
    let statuses: Vec<(AccountId, String)> = rows
        .iter()
        .map(|row| (row.id.clone(), status(&client, row, now, clock)))
        .collect();
    rsx! {
        AccountList {
            pane: STACK,
            label: copy::titles::VACATION_REPLY,
            rows,
            statuses,
            onopen: move |id: AccountId| {
                let mut shell = shell;
                shell.write().vacation_pane = PanePath::new(VacationAt::List).pushed(VacationAt::Account(id));
            },
        }
    }
}

/// What an account's row says of its reply: off, on, on until or from a time, ended, or that its
/// server cannot send one.
fn status(client: &Client, row: &AccountRow, now: DateTime<Utc>, clock: Clock) -> String {
    use copy::status::vacation as words;
    if reach_away(&row.plan).is_err() {
        return words::UNAVAILABLE.to_owned();
    }
    // A reply the client cannot read is not known to be off.
    let kept = match client.stored().vacation(row.id.clone()) {
        Ok(Some(kept)) => kept,
        Ok(None) => return words::OFF.to_owned(),
        Err(_) => return words::UNAVAILABLE.to_owned(),
    };
    let at = |t| when_words(t, now, &chrono::Local, clock);
    match (kept.during.from, kept.during.to) {
        (_, Some(end)) if end <= now => words::ENDED.to_owned(),
        (Some(start), _) if now < start => words::from(&at(start)),
        (_, Some(end)) => words::until(&at(end)),
        _ => words::ON.to_owned(),
    }
}
