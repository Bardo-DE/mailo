//! What a row is as tall as and what the search shown says, without a window.

use super::{Said, pitch, searched};
/// A row is as tall as what it draws: one of two lines is shorter than one of three, and a
/// heading is shorter still. Which a row draws is `row::listed`'s.
#[test]
fn a_row_is_as_tall_as_the_lines_it_draws() {
    use ds::components::app::thread_height::ThreadLines;
    let three = pitch(Some(ThreadLines::Three));
    let two = pitch(Some(ThreadLines::Two));
    let heading = pitch(None);
    assert!(three > two && two > heading, "{three} {two} {heading}");
}

/// What the end of the search shown in the header says: where the search looked, or why it
/// could not be run.
#[test]
fn the_search_shown_says_where_it_looked() {
    use mail_ui_kit::list_search::Scope;
    let broken = regex::Regex::new(&String::from("("))
        .expect_err("an unclosed group")
        .to_string();
    let ada = ["ada@example.test".to_owned()];
    let both = ["ada@example.test".to_owned(), "me@example.test".to_owned()];
    let said = |text: &str, bad: bool| Said {
        text: text.to_owned(),
        bad,
    };
    // A case's name, the list's answer, the accounts looked at, how many there are, and what is said.
    type Case<'a> = (&'a str, Scope, &'a [String], usize, Said);
    let cases: &[Case] = &[
        (
            "the only account",
            Scope::Searched,
            &ada,
            1,
            said("in all mail", false),
        ),
        (
            "one of two, its tile pressed",
            Scope::Searched,
            &ada,
            2,
            said("in ada@example.test", false),
        ),
        (
            "every account",
            Scope::Searched,
            &both,
            2,
            said("in all mail", false),
        ),
        (
            "not answered yet",
            Scope::Place,
            &both,
            2,
            said("in all mail", false),
        ),
        (
            "a bare pattern",
            Scope::OverPage,
            &both,
            2,
            said("on this page of Inbox", false),
        ),
        (
            "a broken pattern",
            Scope::Invalid(broken),
            &both,
            2,
            said("unclosed group", true),
        ),
    ];
    for (name, scope, looked, all, want) in cases {
        assert_eq!(searched(scope, "Inbox", looked, *all), *want, "{name}");
    }
}
