//! A person in ⌘K. Tab on a person's row opens them in the panel: what can be done with them
//! (`mail_ui_kit::person`, the list the reader's names offer too) and the conversations they are
//! in, newest first, the field narrowing both. New Message to… lists the book's people instead,
//! and picking one writes to them; Tab opens that person too. Escape in an empty field goes back
//! to the search, which neither listing touched.

use super::items::RECENT;
use super::recent::{thread_key, thread_of};
use chrono::Utc;
use dioxus::prelude::*;
use mail_client::Client;
use mail_client::search::match_list;
use mail_domain::{Filter, TextMatch, ThreadSummary};
use mail_ui_kit::avatar::avatar_color;
use mail_ui_kit::menu::{MenuItem, Run, Tile, Tone};
use mail_ui_kit::person::{Act, Person};
use mail_ui_kit::view::{Bar, BarListing, Shell};
use mail_ui_model::copy::headers;

/// How many of their conversations an opened person lists.
const THREADS: usize = 5;

/// The prefix of a person's row key, in the search's People and in New Message to….
const PERSON: &str = "person:";

/// The person a row keyed `key` is, named as `names` (the search's answer) has them.
pub(super) fn of_key(
    key: &str,
    names: &std::collections::HashMap<String, String>,
) -> Option<Person> {
    let address = key.strip_prefix(PERSON)?;
    Some(Person {
        address: address.to_owned(),
        name: names.get(address).cloned().unwrap_or_default(),
    })
}

/// Open `person` in the panel, the first row lit.
pub(super) fn open(mut shell: Signal<Shell>, person: Person) {
    if let Bar::Open(open) = &mut shell.write().bar {
        open.listing = BarListing::Person {
            person,
            typed: String::new(),
        };
        open.active = 0;
        open.moved = false;
    }
}

/// The threads `person` is in, within `shell`'s Space: what they sent, and what went to them.
fn with(shell: &Shell, person: &Person) -> Filter {
    shell.scope.over(Filter::Or(vec![
        Filter::FromAddress(person.address.clone()),
        Filter::To(TextMatch::Contains(person.address.clone())),
    ]))
}

/// The panel's rows for `person` opened: their acts under their name, then `threads` under
/// Conversations, each narrowed to what `typed` names, its matched characters marked.
pub(super) fn rows(
    client: &Client,
    shell: &Shell,
    person: &Person,
    typed: &str,
    threads: &[ThreadSummary],
) -> Vec<MenuItem> {
    let acts = mail_ui_kit::person::rows(person, Some(client), Some(shell))
        .into_iter()
        .map(|row| MenuItem {
            group: Some(person.shown().to_owned()),
            ..row
        });
    let threads = threads.iter().take(THREADS).map(|summary| {
        let who = summary.from.shown();
        MenuItem {
            tile: Tile::Avatar {
                letter: initial(who),
                color: avatar_color(&summary.from.email),
            },
            group: Some(headers::WITH_THEM.to_owned()),
            help: Some(who.to_owned()),
            ..MenuItem::plain(
                thread_key(summary.id),
                mail_domain::subject_or_none(&summary.subject).into_owned(),
            )
        }
    });
    narrowed(acts.chain(threads).collect(), typed)
}

/// New Message to…'s rows: the book's people for `typed`, best first, the top of the book when
/// nothing is typed.
pub(super) fn recipients(client: &Client, typed: &str) -> Vec<MenuItem> {
    mail_ui_kit::book::suggest(client, typed.trim())
        .into_iter()
        .map(|found| {
            let person = Person {
                address: found.address.to_ascii_lowercase(),
                name: found.name,
            };
            recipient_row(&person, typed)
        })
        .collect()
}

fn recipient_row(person: &Person, typed: &str) -> MenuItem {
    let shown = person.shown().to_owned();
    let marks = match_list(typed.trim(), &[shown.as_str()])
        .into_iter()
        .next()
        .map(|hit| hit.indices)
        .unwrap_or_default();
    MenuItem {
        tile: Tile::Avatar {
            letter: initial(&shown),
            color: avatar_color(&person.address),
        },
        group: Some(headers::WRITE_TO.to_owned()),
        marks,
        detail: vec![Run {
            text: person.address.clone(),
            marks: Vec::new(),
            tone: Tone::Plain,
        }],
        ..MenuItem::plain(format!("{PERSON}{}", person.address), shown)
    }
}

/// `rows` whose names `typed` matches, in their order, each with its matched characters marked;
/// every row when nothing is typed.
fn narrowed(rows: Vec<MenuItem>, typed: &str) -> Vec<MenuItem> {
    let typed = typed.trim();
    if typed.is_empty() {
        return rows;
    }
    let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    let mut hits: Vec<(usize, Vec<u32>)> = match_list(typed, &names)
        .into_iter()
        .map(|hit| (hit.index, hit.indices))
        .collect();
    hits.sort_by_key(|(index, _)| *index);
    hits.into_iter()
        .filter_map(|(index, marks)| {
            rows.get(index)
                .cloned()
                .map(|row| MenuItem { marks, ..row })
        })
        .collect()
}

/// What picking the row keyed `key` does while `person` is open: an act runs, and a
/// conversation opens. The panel closes either way.
pub(super) fn run(shell: Signal<Shell>, pages: Signal<u32>, person: &Person, key: &str) {
    if let Some(act) = Act::of_key(key) {
        super::close(shell);
        mail_ui_kit::person::run(act, person, shell, Some(pages));
        return;
    }
    if let Some(id) = key
        .split_once(':')
        .filter(|(kind, _)| *kind == RECENT)
        .and_then(|(_, rest)| thread_of(rest))
    {
        let mut shell = shell;
        shell.write().open(id);
        super::close(shell);
    }
}

/// Write to the person the row keyed `key` is, picked in New Message to….
pub(super) fn write(shell: Signal<Shell>, key: &str) {
    let Some(address) = key.strip_prefix(PERSON) else {
        return;
    };
    // The name the book gives them, which is what the row showed.
    let client = mail_ui_kit::live::client();
    let name = client
        .stored()
        .contact(address)
        .ok()
        .flatten()
        .and_then(|contact| contact.name)
        .unwrap_or_default();
    let person = Person {
        address: address.to_owned(),
        name,
    };
    super::close(shell);
    mail_ui_kit::person::run(Act::Write, &person, shell, None);
}

fn initial(name: &str) -> char {
    name.chars().next().unwrap_or('·').to_ascii_uppercase()
}

/// The conversations of the person open in the panel, read off the thread that draws each time
/// another person is opened, and dropped when the panel has moved on to someone else.
#[component]
pub(super) fn Threads(shell: Signal<Shell>, threads: Signal<Vec<ThreadSummary>>) -> Element {
    let opened = use_memo(move || match &shell.read().bar {
        Bar::Open(open) => match &open.listing {
            BarListing::Person { person, .. } => {
                Some((person.clone(), with(&shell.read(), person)))
            }
            _ => None,
        },
        Bar::Closed => None,
    });
    let _fetch = use_resource(move || {
        let opened = opened();
        let client = mail_ui_kit::live::client();
        async move {
            let mut threads = threads;
            let Some((person, filter)) = opened else {
                threads.set(Vec::new());
                return;
            };
            threads.set(Vec::new());
            let found = tokio::task::spawn_blocking(move || {
                use mail_client::search::Source as _;
                client
                    .source()
                    .listed(&filter, mail_client::search::first(THREADS), Utc::now())
            })
            .await
            .unwrap_or_default();
            if opened_now(shell) == Some(person) {
                threads.set(found);
            }
        }
    });
    rsx! {}
}

/// The person open in the panel now.
fn opened_now(shell: Signal<Shell>) -> Option<Person> {
    match &shell.peek().bar {
        Bar::Open(open) => match &open.listing {
            BarListing::Person { person, .. } => Some(person.clone()),
            _ => None,
        },
        Bar::Closed => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Person, narrowed, recipient_row};
    use mail_ui_kit::menu::MenuItem;

    #[test]
    fn typing_keeps_the_rows_it_names_in_their_order() {
        let rows: Vec<MenuItem> = ["New Message", "Copy Address", "Mail From Them"]
            .into_iter()
            .map(|name| MenuItem::plain(name.to_owned(), name.to_owned()))
            .collect();
        let kept: Vec<String> = narrowed(rows.clone(), "m")
            .into_iter()
            .map(|row| row.name)
            .collect();
        assert_eq!(kept, ["New Message", "Mail From Them"]);
        assert_eq!(narrowed(rows.clone(), " ").len(), 3);
        let copy = narrowed(rows, "copy");
        assert_eq!(copy[0].marks, [0, 1, 2, 3]);
    }

    #[test]
    fn a_recipient_row_is_keyed_by_address_and_named_by_name() {
        let row = recipient_row(
            &Person {
                address: "ada@example.test".to_owned(),
                name: "Ada Lovelace".to_owned(),
            },
            "lov",
        );
        assert_eq!(row.key, "person:ada@example.test");
        assert_eq!(row.name, "Ada Lovelace");
        assert_eq!(row.marks, [4, 5, 6]);
        assert_eq!(
            super::of_key(&row.key, &Default::default()).map(|person| person.address),
            Some("ada@example.test".to_owned())
        );
    }
}
