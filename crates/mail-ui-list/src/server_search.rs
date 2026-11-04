//! "Search on Server", in the search shown in the list's header.
//!
//! A search lists what the client holds. Wherever a search narrows the list, the header offers
//! each account in view that has a server to search it (`mail_client::server_search`); pressed,
//! the account's server is asked off the drawing thread, what it finds is kept as headers, and
//! the conversations it named follow the list under "From the server", each with the chip that
//! says so. The header says that it is asking and then how it went. What the server could not be
//! asked is said instead of searched for.
//!
//! It runs by itself only when "Search the server automatically" is on, from the effect of
//! [`use_server_search`] that sees a search settle (an effect's task is polled, a render's is
//! not), and only once per line and account. What searches is a root context, [`ServerSearcher`],
//! so a test can answer for a server.

use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use ds::components::controls::button_model::{Bezel, ImagePosition};
use ds::host::measure::MountedRef;
use ds::prelude::*;
use ds::root::common::Common;
use ds::style::tokens::control_size::ControlSize;
use mail_client::Client;
use mail_domain::ThreadId;
use mail_proto::search::Searched;
use mail_ui_kit::menu::anchor_at;
use mail_ui_kit::press::on_primary;
use mail_ui_model::copy;
use porter_core::AccountId;
use std::sync::Arc;

/// Search one account's server for a typed line: the signature of [`mail_client::server_search::search`].
pub type Search =
    Arc<dyn Fn(Client, AccountId, &str, DateTime<Utc>) -> Result<Searched, String> + Send + Sync>;

/// What searches an account's server. The server itself unless a test provided its own.
#[derive(Clone)]
pub struct ServerSearcher(pub Search);

impl std::fmt::Debug for ServerSearcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServerSearcher")
    }
}

impl ServerSearcher {
    /// The server, through [`mail_client::server_search::search`].
    #[cfg(not(any(test, feature = "testing")))]
    fn server() -> Self {
        Self(Arc::new(|client, account, input, _now| {
            client
                .net()
                .search_server(account, input)
                .map_err(mail_ui_kit::remedy::told)
        }))
    }

    /// Not in a test: the real search reads the keyring and opens a socket, and a test that
    /// forgot to provide its own must fail in words rather than reach either.
    #[cfg(any(test, feature = "testing"))]
    fn server() -> Self {
        Self(Arc::new(|_, _, input, _| {
            Err(format!(
                "a test searched the server for {input:?} without providing a ServerSearcher"
            ))
        }))
    }
}

/// One account's server asked for one line, and how that went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub input: String,
    pub account: AccountId,
    pub answer: Answer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Running,
    /// The conversations the server named, newest first; how many of their messages this search
    /// brought; how many more the server matched.
    Found {
        threads: Vec<ThreadId>,
        fetched: usize,
        more: u64,
    },
    /// Not asked: what the server cannot be asked faithfully.
    Unsaid(String),
    Failed(String),
}

/// The answer for `input` on `account`, if it was asked.
pub fn answer_for<'a>(asked: &'a [Asked], input: &str, account: AccountId) -> Option<&'a Answer> {
    asked
        .iter()
        .find(|a| a.input == input && a.account == account)
        .map(|a| &a.answer)
}

/// Every conversation the servers named for `input`, newest first, less those in `listed`.
pub fn found_threads(asked: &[Asked], input: &str, listed: &[ThreadId]) -> Vec<ThreadId> {
    let mut out: Vec<ThreadId> = Vec::new();
    for a in asked.iter().filter(|a| a.input == input) {
        if let Answer::Found { threads, .. } = &a.answer {
            for thread in threads {
                if !listed.contains(thread) && !out.contains(thread) {
                    out.push(*thread);
                }
            }
        }
    }
    out
}

/// What the header says of an account's search once it has an answer.
pub fn said(address: &str, answer: &Answer) -> Option<String> {
    Some(match answer {
        Answer::Running => copy::status::server_searching(address),
        Answer::Found { threads, .. } if threads.is_empty() => {
            copy::status::server_found_nothing(address)
        }
        Answer::Found {
            threads,
            fetched,
            more,
        } => copy::status::server_found(address, threads.len(), *fetched, *more),
        Answer::Unsaid(what) => copy::status::server_not_searched(address, what),
        Answer::Failed(why) => why.clone(),
    })
}

/// Ask `account`'s server for `input`, off the drawing thread, and put its answer in `asked`.
///
/// From a press, or from [`use_server_search`]'s effect: never from a render.
pub fn start(mut asked: Signal<Vec<Asked>>, input: String, account: AccountId) {
    if matches!(
        answer_for(&asked.peek(), &input, account.clone()),
        Some(Answer::Running)
    ) {
        return;
    }
    {
        let mut write = asked.write();
        write.retain(|a| !(a.input == input && a.account == account));
        write.push(Asked {
            input: input.clone(),
            account: account.clone(),
            answer: Answer::Running,
        });
    }
    let search = try_consume_context::<ServerSearcher>().unwrap_or_else(ServerSearcher::server);
    let client = mail_ui_kit::live::client();
    let searched_for = account.clone();
    spawn(async move {
        let line = input.clone();
        // `spawn_blocking`: the search waits on the application's runtime (`Client::net`), which
        // an async task must not.
        let done = tokio::task::spawn_blocking(move || {
            let searched = (search.0)(client.clone(), searched_for, &line, Utc::now());
            searched.map(|searched| match searched {
                Searched::Unsaid(unsaid) => Answer::Unsaid(unsaid.to_string()),
                Searched::Found(hits) => {
                    let mut threads: Vec<ThreadId> = Vec::new();
                    for message in &hits.messages {
                        if let Ok(m) = client.stored().message(*message)
                            && !threads.contains(&m.thread)
                        {
                            threads.push(m.thread);
                        }
                    }
                    Answer::Found {
                        threads,
                        fetched: hits.fetched,
                        more: hits.more,
                    }
                }
            })
        })
        .await;
        let answer = match done {
            Ok(Ok(answer)) => answer,
            Ok(Err(why)) => Answer::Failed(why),
            Err(e) => Answer::Failed(format!("Server search stopped: {e}")),
        };
        if let Some(entry) = asked
            .write()
            .iter_mut()
            .find(|a| a.input == input && a.account == account)
        {
            entry.answer = answer;
        }
    });
}

/// Automatically, where the user turned it on, or where the line names a category only the
/// server can say: ask each of `accounts` for `input`, once per line and account. Called by the
/// list whether or not the header shows the search, so a line settled under the panel is asked
/// as it is when shown.
pub fn use_server_search(input: &str, accounts: &[(AccountId, String)], asked: Signal<Vec<Asked>>) {
    let settings = mail_ui_kit::prefs::use_settings();
    let automatic = use_memo(move || {
        mail_client::server_search::Automatic::from(settings.read().search.server_automatically)
    });
    use_effect(use_reactive(
        (&input.to_owned(), &accounts.to_vec()),
        move |(wanted, everyone)| {
            // A category is only the server's to say, so a line that names one is asked at once.
            if automatic() != mail_client::server_search::Automatic::On
                && !mail_client::server_search::needs_server(&wanted)
            {
                return;
            }
            for (account, _) in &everyone {
                if answer_for(&asked.peek(), &wanted, account.clone()).is_none() {
                    start(asked, wanted.clone(), account.clone());
                }
            }
        },
    ));
}

/// The search header's server part: what each account's server said of the line (or that it is
/// being asked), then the button that asks, or a menu of the accounts when there are several.
#[component]
pub fn ServerStrip(
    /// The settled search line.
    input: String,
    /// The accounts in view that have a server, with their addresses.
    accounts: Vec<(AccountId, String)>,
    asked: Signal<Vec<Asked>>,
) -> Element {
    let mut open = use_signal(|| false);
    let mut at = use_signal(|| None::<MountedRef>);
    if accounts.is_empty() {
        return rsx! {};
    }
    let answers: Vec<(String, Answer)> = accounts
        .iter()
        .filter_map(|(account, address)| {
            answer_for(&asked.read(), &input, account.clone())
                .map(|answer| (address.clone(), answer.clone()))
        })
        .collect();
    let running = |account: &AccountId| {
        matches!(
            answer_for(&asked.read(), &input, account.clone()),
            Some(Answer::Running)
        )
    };
    // Offered for those accounts not being asked now; "again" once any has answered.
    let free: Vec<(AccountId, String)> = accounts
        .iter()
        .filter(|(account, _)| !running(account))
        .cloned()
        .collect();
    let again = !answers.is_empty();
    let label = if again {
        copy::aria::SEARCH_ON_SERVER_AGAIN
    } else {
        copy::aria::SEARCH_ON_SERVER
    };
    let several = accounts.len() > 1;
    let items: Vec<MenuItem<usize>> = if open() {
        free.iter()
            .enumerate()
            .map(|(n, (_, address))| {
                MenuItem::new(n, copy::menus::search_account_on_server(address))
            })
            .collect()
    } else {
        Vec::new()
    };
    let single = free.first().cloned();
    let pick = free.clone();
    rsx! {
        // The line the button searches for, which the list settles a beat after the typing: a
        // press is for this line, and its answer shows while the line is still this one.
        div { class: "search-server", "data-line": "{input}",
            for (address , answer) in answers {
                {
                    let said = said(&address, &answer).unwrap_or_default();
                    if matches!(answer, Answer::Running) {
                        rsx! {
                            span { class: "search-server-said", title: "{said}",
                                mail_ui_kit::pending::Pending { label: said.clone() }
                            }
                        }
                    } else {
                        let bad = matches!(answer, Answer::Failed(_));
                        rsx! {
                            span {
                                class: if bad { "search-server-said bad" } else { "search-server-said" },
                                role: "status",
                                title: "{said}",
                                "{said}"
                            }
                        }
                    }
                }
            }
            if !free.is_empty() {
                Button {
                    bezel: Bezel::Toolbar,
                    size: ControlSize::Large,
                    image: ImagePosition::Only,
                    label,
                    icon: Some(IconSource::Glyph(Icon::Globe)),
                    title: Some(label.to_owned()),
                    shown: if open() { Shown::Visible } else { Shown::Hidden },
                    common: Common {
                        aria_label: Some(label.to_owned()),
                        mounted: Some(EventHandler::new(move |event: MountedEvent| {
                            at.set(Some(MountedRef(event.data())));
                        })),
                        ..Common::default()
                    },
                    onclick: {
                        let line = input.clone();
                        on_primary(move || match (&single, several) {
                            (Some((account, _)), false) => start(asked, line.clone(), account.clone()),
                            _ => open.set(!open()),
                        })
                    },
                }
            }
            if open() {
                Menu::<usize> {
                    placement: MenuPlacement::Popup,
                    anchor: anchor_at(at()),
                    items,
                    common: Common {
                        aria_label: Some(copy::aria::SERVER_SEARCH_ACCOUNTS.to_owned()),
                        ..Common::default()
                    },
                    onpick: {
                        let line = input.clone();
                        move |n: usize| {
                            open.set(false);
                            if let Some((account, _)) = pick.get(n) {
                                start(asked, line.clone(), account.clone());
                            }
                        }
                    },
                    onclose: move |()| open.set(false),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::id::account_id_from_uuid;

    fn id(n: u128) -> ThreadId {
        ThreadId::from_uuid(uuid::Uuid::from_u128(n))
    }

    fn acct_account() -> AccountId {
        account_id_from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000a1"))
    }

    #[test]
    fn what_each_answer_says() {
        let found = |threads: Vec<ThreadId>, fetched, more| Answer::Found {
            threads,
            fetched,
            more,
        };
        let cases: Vec<(Answer, &str)> = vec![
            (Answer::Running, "Searching me@example.test on the server…"),
            (
                found(vec![], 0, 0),
                "Nothing found on me@example.test's server.",
            ),
            (
                found(vec![id(1)], 1, 0),
                "1 conversation found on me@example.test's server, 1 new here.",
            ),
            (
                found(vec![id(1), id(2)], 0, 48),
                "2 conversations found on me@example.test's server, 0 new here; 48 more there.",
            ),
            (
                Answer::Unsaid("is:pinned (kept on this computer)".to_owned()),
                "Not searched on me@example.test's server: is:pinned (kept on this computer).",
            ),
        ];
        for (answer, line) in cases {
            assert_eq!(said("me@example.test", &answer).as_deref(), Some(line));
        }
    }

    #[test]
    fn found_conversations_leave_out_what_is_listed_and_other_lines() {
        let asked = vec![
            Asked {
                input: "lunch".to_owned(),
                account: acct_account(),
                answer: Answer::Found {
                    threads: vec![id(1), id(2), id(3)],
                    fetched: 2,
                    more: 0,
                },
            },
            Asked {
                input: "budget".to_owned(),
                account: acct_account(),
                answer: Answer::Found {
                    threads: vec![id(9)],
                    fetched: 1,
                    more: 0,
                },
            },
        ];
        assert_eq!(found_threads(&asked, "lunch", &[id(2)]), vec![id(1), id(3)]);
        assert_eq!(found_threads(&asked, "lunc", &[]), Vec::<ThreadId>::new());
    }
}
