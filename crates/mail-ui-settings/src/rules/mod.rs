//! The Rules page of Settings: a pane of three levels. Its root lists the accounts
//! ([`crate::settings::account_list`]); an account pushes its rules, in the order they run; a
//! rule, or a new one, pushes its page over those. And the Vacation Reply page, Settings' next:
//! the accounts, each one's reply a page of its own pushed over them ([`vacation`]).
//!
//! The search bar's "Rules Settings" opens Settings on it. Everything it writes goes through the
//! same rows `mailo rules` and `mailo vacation` use — [`work`] and [`away`] are those questions
//! and writes as functions; the parts only draw them. One account at a time, because a rule acts
//! on one account's labels and folders, and a server runs one account's script.
//!
//! Server work is never a step the person takes: every save (a rule kept, switched, moved or
//! deleted, the vacation reply kept) puts the account's script on its server as `mailo sieve
//! push` does ([`server`]), and only a failure is said, at the head of the page. An account
//! whose server runs no rules keeps them here, and nothing is said.
//!
//! The pages are quire's `PaneStack`; which are shown is the window's `Shell` (`rules_pane`).

pub mod apply;
pub mod away;
pub mod conditions;
pub mod editor;
pub mod list;
pub mod server;
pub mod vacation;
pub mod when;
pub mod work;

use chrono::Utc;
use dioxus::prelude::*;
use ds::prelude::*;
use ds::root::common::Common;

use crate::settings::account_list::{self, AccountList};
use editor::RulePage;
use list::RulesList;
use mail_client::rules::server::AwayServer;
use mail_domain::Rule;
use mail_ui_kit::common::Told;
use mail_ui_kit::data::AccountRow;
use mail_ui_kit::files::{Phase, run};
use mail_ui_kit::view::{RulesAt, Shell};
use mail_ui_model::copy;
use porter_core::AccountId;
use server::{AwaySetter, Pusher, put, reach, reach_away, set_away};
pub use vacation::{VacationPage, back as vacation_back};

/// What the last act on the page came to, when it failed: said in a dialog until it is closed.
pub type Said = Signal<Option<Result<String, String>>>;

/// The stack's element id: its back buttons are `rules-pane-back-root`, `-account` and `-detail`.
const STACK: &str = "rules-pane";

/// The element id of the row that opens `at`, where the keyboard goes back to.
pub fn row_id(at: &RulesAt) -> Option<String> {
    match at {
        RulesAt::List => None,
        RulesAt::Account(account) => Some(account_list::row_id(STACK, account)),
        RulesAt::Rule(_, Some(id)) => Some(format!("rules-pane-open-{id}")),
        RulesAt::Rule(_, None) => Some("rules-pane-new".to_owned()),
    }
}

/// Push `at` over the page shown.
pub fn open(mut shell: Signal<Shell>, at: RulesAt) {
    let pushed = shell.peek().rules_pane.pushed(at);
    shell.write().rules_pane = pushed;
}

/// Push `account`'s rules over the list of accounts, whatever was shown.
pub fn open_account(mut shell: Signal<Shell>, account: AccountId) {
    shell.write().rules_pane = PanePath::new(RulesAt::List).pushed(RulesAt::Account(account));
}

/// Back one page.
pub fn back(mut shell: Signal<Shell>) {
    let back = shell.peek().rules_pane.popped();
    shell.write().rules_pane = back;
}

/// The page: the accounts at its root, an account's rules over them, a rule's page over those.
#[component]
pub fn RulesPage(shell: Signal<Shell>) -> Element {
    let rows = mail_ui_kit::data::use_account_rows();
    let path = shell.read().rules_pane.clone();
    let said: Said = use_signal(|| None);
    // A rule or vacation reply kept here that its server did not take, said apart from a write
    // that did not happen at all.
    let unpushed: Said = use_signal(|| None);
    // A rule applied to the mail already here, as it runs: started as its page saves it and goes
    // back, so it is held here, and the list draws its bar.
    let phase = use_signal(|| Phase::Ready);
    let running = use_signal(String::new);
    let applied = use_callback(move |rule: Rule| apply(rule, phase, running));
    // Every save ends here: the account's script goes to its server, off the thread that draws,
    // and only a failure is said. Made here, so a push outlives the rule's page it was saved on.
    let pushed = use_callback(move |row: AccountRow| push(row, unpushed));
    let title = use_callback(move |at: RulesAt| {
        let named = |account: &AccountId| {
            rows.read()
                .iter()
                .find(|row| row.id == *account)
                .map_or_else(|| account.to_string(), AccountRow::shown)
        };
        match at {
            RulesAt::List => copy::titles::RULES.to_owned(),
            RulesAt::Account(account) => named(&account),
            RulesAt::Rule(_, None) => copy::titles::NEW_RULE.to_owned(),
            RulesAt::Rule(account, Some(id)) => {
                let client = mail_ui_kit::live::client();
                client
                    .stored()
                    .rules(account.clone())
                    .ok()
                    .and_then(|rules| rules.into_iter().find(|rule| rule.id == id))
                    .map_or_else(|| named(&account), |rule| rule.name)
            }
        }
    });
    let page = use_callback(move |at: RulesAt| {
        let of = |account: &AccountId| rows.read().iter().find(|row| row.id == *account).cloned();
        match at {
            RulesAt::List => rsx! { Accounts { shell } },
            RulesAt::Account(account) => match of(&account) {
                Some(row) => {
                    rsx! { RulesList { key: "{account}", shell, row, said, pushed, phase, running } }
                }
                None => rsx! {},
            },
            RulesAt::Rule(account, id) => match of(&account) {
                Some(row) => {
                    rsx! { RulePage { key: "{id:?}", shell, row, id, said, pushed, applied } }
                }
                None => rsx! {},
            },
        }
    });
    let opener = use_callback(|at: RulesAt| row_id(&at));
    rsx! {
        PaneStack::<RulesAt> {
            path,
            title,
            page,
            opener: Some(opener),
            on_back: move |()| back(shell),
            common: Common { id: Some(STACK.to_owned()), ..Common::default() },
        }
        Told { title: copy::titles::NOT_SAVED, said }
        Told { title: copy::titles::NOT_ON_SERVER, said: unpushed }
    }
}

/// The root: the accounts, each opening its rules.
#[component]
fn Accounts(shell: Signal<Shell>) -> Element {
    // Read again when an account comes or goes.
    let _ = mail_ui_kit::live::moved_for(mail_client::Topics::ACCOUNTS);
    let rows = mail_ui_kit::data::use_account_rows()();
    rsx! {
        AccountList {
            pane: STACK,
            label: copy::titles::RULES,
            rows,
            onopen: move |account: AccountId| open_account(shell, account),
        }
    }
}

/// Apply `rule` to the mail already here, as `mailo rules run` does, its bar in `phase`; the
/// window's lists follow once it ends.
fn apply(rule: Rule, phase: Signal<Phase>, mut running: Signal<String>) {
    let client = mail_ui_kit::live::client();
    let of = client
        .rules_manage()
        .conversations(rule.account.clone(), Utc::now());
    running.set(rule.name.clone());
    run(
        phase,
        of,
        move |report| work::run(&client, &rule, Utc::now(), report),
        // What it wrote draws the window again: nothing more to do after.
        || {},
    );
}

/// Put `row`'s rules and vacation reply on its server, when it has one that runs them: on a
/// blocking thread, saying only a failure, in `said`.
fn push(row: AccountRow, mut said: Said) {
    if let Err(why) = reach(&row.plan) {
        log::debug!("{}: no server to put the rules on: {why}", row.address);
        return;
    }
    let client = mail_ui_kit::live::client();
    let push = try_consume_context::<Pusher>()
        .unwrap_or_else(Pusher::server)
        .0;
    spawn(async move {
        let done = tokio::task::spawn_blocking(move || put(&client, &row, &push, Utc::now())).await;
        let failed = match done {
            Ok(Ok(_)) => return,
            Ok(Err(why)) => why,
            Err(error) => error.to_string(),
        };
        said.set(Some(Err(copy::errors::not_on_server(&failed))));
    });
}

/// Put `row`'s vacation reply where its server keeps one: with its rules in its Sieve script
/// ([`push`]), or set on its JMAP or Graph server. On a blocking thread, saying only a failure,
/// in `said`.
fn push_away(row: AccountRow, mut said: Said) {
    match reach_away(&row.plan) {
        Ok(AwayServer::Sieve) => return push(row, said),
        Ok(AwayServer::Jmap | AwayServer::Graph) => {}
        // Nowhere to put one: the page offers no switch for such an account.
        Err(why) => {
            log::debug!("{}: no reply to put on a server: {why}", row.address);
            return;
        }
    }
    let client = mail_ui_kit::live::client();
    let set = try_consume_context::<AwaySetter>()
        .unwrap_or_else(AwaySetter::server)
        .0;
    spawn(async move {
        let done =
            tokio::task::spawn_blocking(move || set_away(&client, &row, &set, Utc::now())).await;
        let failed = match done {
            Ok(Ok(())) => return,
            Ok(Err(why)) => why,
            Err(error) => error.to_string(),
        };
        said.set(Some(Err(copy::errors::not_on_server(&failed))));
    });
}
