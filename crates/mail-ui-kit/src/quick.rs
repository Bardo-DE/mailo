//! The list's quick filter: Unread, Starred and Has Attachments, toggled from a row under the
//! list's header, as Thunderbird's Quick Filter bar and Apple Mail's filter button offer them.
//!
//! The toggles are not state of their own: each is a search operator the window already reads
//! (`is:unread`, `is:starred`, `has:attachment`), and turning one on writes it into the search
//! line ([`Toggle::flip`]), where the header shows it as the chip it is. A line made of nothing
//! but toggles does not search: it narrows the place the list is in, so Unread in the Inbox is
//! the unread mail of the Inbox ([`narrows`], and [`crate::view::Shell::narrowing`]). Any other
//! word or operator in the line makes it a search of all mail, which the toggles then hold as
//! they hold any search.

use mail_domain::Filter;
use mail_ui_model::copy;

/// One toggle of the quick filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Unread,
    Starred,
    Attachments,
}

impl Toggle {
    /// Every toggle, in the order the row draws them.
    pub const ALL: [Toggle; 3] = [Toggle::Unread, Toggle::Starred, Toggle::Attachments];

    /// The search term the toggle is: what typing it into the search asks for.
    pub fn term(self) -> &'static str {
        match self {
            Toggle::Unread => "is:unread",
            Toggle::Starred => "is:starred",
            Toggle::Attachments => "has:attachment",
        }
    }

    /// What the toggle keeps: its term, read as a search reads it.
    pub fn filter(self) -> Filter {
        mail_logic::query::parse_with(self.term(), &chrono::Utc, &mail_logic::query::unnamed)
    }

    /// Its words on the row.
    pub fn label(self) -> &'static str {
        match self {
            Toggle::Unread => copy::controls::FILTER_UNREAD,
            Toggle::Starred => copy::controls::FILTER_STARRED,
            Toggle::Attachments => copy::controls::FILTER_ATTACHMENTS,
        }
    }

    /// Whether the search `line` holds the toggle's term.
    pub fn is_on(self, line: &str) -> bool {
        mail_logic::search::has_term(line, self.term())
    }

    /// `line` with the toggle's term added at its end, or taken out of wherever it stands.
    pub fn flip(self, line: &str) -> String {
        mail_logic::search::toggle_term(line, self.term())
    }
}

/// Every toggle's term.
fn terms() -> [&'static str; 3] {
    Toggle::ALL.map(Toggle::term)
}

/// The toggles the search `line` holds, in the row's order.
pub fn on(line: &str) -> Vec<Toggle> {
    Toggle::ALL
        .into_iter()
        .filter(|toggle| toggle.is_on(line))
        .collect()
}

/// Whether the search `line` holds any toggle: the list is narrowed.
pub fn is_active(line: &str) -> bool {
    Toggle::ALL.into_iter().any(|toggle| toggle.is_on(line))
}

/// `line` with every toggle's term taken out and the rest of it as typed.
pub fn cleared(line: &str) -> String {
    mail_logic::search::without_terms(line, &terms())
}

/// Whether the search `line` is made of toggles alone, and so narrows the place rather than
/// searching all mail.
pub fn narrows(line: &str) -> bool {
    mail_logic::search::only_terms(line, &terms())
}

/// The toggles `line` holds as one narrowing of their own. `None` while none is on.
pub fn filter(line: &str) -> Option<Filter> {
    let mut held: Vec<Filter> = on(line).into_iter().map(Toggle::filter).collect();
    match held.len() {
        0 => None,
        1 => held.pop(),
        _ => Some(Filter::And(held)),
    }
}

/// Whether the row of toggles is drawn under the list's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Row {
    #[default]
    Hidden,
    Shown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::{ReadState, Star};

    /// Each toggle is the term a search reads, and the filter that term is.
    #[test]
    fn each_toggle_is_its_search_term() {
        let cases = [
            (Toggle::Unread, "is:unread", Filter::Read(ReadState::Unread)),
            (
                Toggle::Starred,
                "is:starred",
                Filter::Starred(Star::Starred),
            ),
            (Toggle::Attachments, "has:attachment", Filter::HasAttachment),
        ];
        for (toggle, term, want) in cases {
            assert_eq!(toggle.term(), term, "{toggle:?}");
            assert_eq!(toggle.filter(), want, "{toggle:?}");
        }
    }

    /// The toggles a line holds are joined with `And` in the row's order however they were
    /// turned on; none leaves nothing to narrow by.
    #[test]
    fn the_toggles_in_a_line_are_one_narrowing() {
        let line = |toggles: &[Toggle]| {
            toggles
                .iter()
                .fold(String::new(), |line, toggle| toggle.flip(&line))
        };
        let unread = || Filter::Read(ReadState::Unread);
        let starred = || Filter::Starred(Star::Starred);
        let cases: [(&str, &[Toggle], Option<Filter>); 5] = [
            ("none", &[], None),
            ("unread", &[Toggle::Unread], Some(unread())),
            (
                "attachments then unread",
                &[Toggle::Attachments, Toggle::Unread],
                Some(Filter::And(vec![unread(), Filter::HasAttachment])),
            ),
            (
                "all three",
                &[Toggle::Starred, Toggle::Attachments, Toggle::Unread],
                Some(Filter::And(vec![
                    unread(),
                    starred(),
                    Filter::HasAttachment,
                ])),
            ),
            (
                "unread twice is off",
                &[Toggle::Unread, Toggle::Unread],
                None,
            ),
        ];
        for (name, toggles, want) in cases {
            let line = line(toggles);
            assert_eq!(filter(&line), want, "{name}: {line:?}");
            assert_eq!(is_active(&line), want.is_some(), "{name}");
        }
    }

    /// Clearing takes the toggles out and leaves the words and the other operators; only a line
    /// of toggles narrows a place.
    #[test]
    fn clearing_leaves_the_rest_and_only_toggles_narrow() {
        assert_eq!(
            cleared("is:unread lunch from:ada has:attachment"),
            "lunch from:ada"
        );
        assert_eq!(
            on("x is:starred y is:unread"),
            [Toggle::Unread, Toggle::Starred]
        );
        assert!(narrows("is:unread has:attachment"));
        assert!(!narrows("is:unread lunch"));
        assert!(!narrows(""));
    }
}
