//! Operators of a search line as a set of switches: whether the line holds one, adding it, taking
//! it away, and whether the line is made of nothing else.
//!
//! The list's Filter row is three such switches over the search (`is:unread`, `is:starred`,
//! `has:attachment`). A term is found by what it means, as the parser reads it, not by its
//! spelling, and is cut out by the parser's own spans, so a word or a phrase that merely holds the
//! term's text is left alone.

use super::parse;
use crate::query::{parse_with, unnamed};
use chrono::Utc;
use mail_domain::Filter;
use std::ops::Range;

/// The filter `term` is, read as a search reads it.
fn meaning(term: &str) -> Filter {
    parse_with(term, &Utc, &unnamed)
}

/// The operators of `line` that mean `term`, as the byte ranges of the tokens they were read
/// from. An either-or that holds the term is a different operator, and is not the term.
fn spans_of(line: &str, term: &Filter) -> Vec<Range<usize>> {
    let parsed = parse(line, &Utc, &unnamed);
    parsed
        .operators
        .iter()
        .zip(parsed.operator_spans)
        .filter(|(operator, _)| *operator == term)
        .flat_map(|(_, spans)| spans)
        .collect()
}

/// Whether `line` holds the operator `term` means.
pub fn has_term(line: &str, term: &str) -> bool {
    !spans_of(line, &meaning(term)).is_empty()
}

/// `line` with `term` added at its end, or taken away wherever it stands, if it is there.
pub fn toggle_term(line: &str, term: &str) -> String {
    if has_term(line, term) {
        without_terms(line, &[term])
    } else {
        let line = line.trim_end();
        if line.is_empty() {
            term.to_owned()
        } else {
            format!("{line} {term}")
        }
    }
}

/// `line` without any operator that means one of `terms`, each taken out with the space after it
/// (or before it, at the end of the line), and the rest of the line exactly as typed.
pub fn without_terms(line: &str, terms: &[&str]) -> String {
    let mut cuts: Vec<Range<usize>> = terms
        .iter()
        .flat_map(|term| spans_of(line, &meaning(term)))
        .collect();
    cuts.sort_by_key(|cut| cut.start);
    cuts.dedup();
    let mut out = line.to_owned();
    for cut in cuts.into_iter().rev() {
        cut_out(&mut out, cut);
    }
    out.trim().to_owned()
}

/// `line` without the operator it holds at `at` (as the parser lists them, an either-or whole),
/// cut out as [`without_terms`] cuts one. `None` when there is no such operator.
pub fn without_operator(line: &str, at: usize) -> Option<String> {
    let parsed = parse(line, &Utc, &unnamed);
    let spans = parsed.operator_spans.get(at)?;
    let start = spans.iter().map(|span| span.start).min()?;
    let end = spans.iter().map(|span| span.end).max()?;
    let mut out = line.to_owned();
    cut_out(&mut out, start..end);
    Some(out.trim().to_owned())
}

/// `cut` taken out of `out` with the space after it, or before it at the end of the line.
fn cut_out(out: &mut String, cut: Range<usize>) {
    let after = out[cut.end..].len() - out[cut.end..].trim_start().len();
    let start = if cut.end + after == out.len() {
        // At the end of the line the space before goes instead.
        out[..cut.start].trim_end().len()
    } else {
        cut.start
    };
    out.replace_range(start..cut.end + after, "");
}

/// Whether `line` has something in it and all of it is operators that mean one of `terms`: a
/// line that narrows without searching for anything.
pub fn only_terms(line: &str, terms: &[&str]) -> bool {
    let parsed = parse(line, &Utc, &unnamed);
    let meant: Vec<Filter> = terms.iter().map(|term| meaning(term)).collect();
    !parsed.operators.is_empty()
        && parsed.rest_spans.is_empty()
        && parsed.operators.iter().all(|op| meant.contains(op))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNREAD: &str = "is:unread";
    const STARRED: &str = "is:starred";
    const ATTACHMENT: &str = "has:attachment";
    const ALL: [&str; 3] = [UNREAD, STARRED, ATTACHMENT];

    /// A term is added at the end and taken away from wherever it stands.
    #[test]
    fn a_term_toggles_in_the_line() {
        let cases = [
            ("", UNREAD, "is:unread"),
            ("   ", UNREAD, "is:unread"),
            ("lunch", UNREAD, "lunch is:unread"),
            ("lunch  ", UNREAD, "lunch is:unread"),
            ("is:unread", UNREAD, ""),
            ("lunch is:unread", UNREAD, "lunch"),
            ("is:unread lunch", UNREAD, "lunch"),
            ("a is:unread b", UNREAD, "a b"),
            ("a  is:unread  b", UNREAD, "a  b"),
            (
                "from:ada is:unread has:attachment",
                UNREAD,
                "from:ada has:attachment",
            ),
            ("from:ada", STARRED, "from:ada is:starred"),
            ("is:unread is:unread x", UNREAD, "x"),
            ("x is:unread is:unread", UNREAD, "x"),
        ];
        for (line, term, want) in cases {
            assert_eq!(toggle_term(line, term), want, "{line:?} {term}");
        }
    }

    /// A quoted phrase or a word holding the term's text is not the term, and is kept as typed.
    #[test]
    fn only_the_operator_is_cut() {
        let cases = [
            ("xis:unread", false),
            (r#"subject:"a is:unread b""#, false),
            ("-is:unread", false),
            ("is:unread OR is:starred", false),
            (r#"is:unread "a  b""#, true),
        ];
        for (line, has) in cases {
            assert_eq!(has_term(line, UNREAD), has, "{line}");
        }
        assert_eq!(
            without_terms(r#""a  b" is:unread subject:"x is:unread""#, &[UNREAD]),
            r#""a  b" subject:"x is:unread""#
        );
        // Adding to a line whose quote holds the text keeps the quote whole.
        assert_eq!(
            toggle_term(r#"subject:"is:unread""#, UNREAD),
            r#"subject:"is:unread" is:unread"#
        );
    }

    /// Clearing takes every term and leaves the rest of the line.
    #[test]
    fn clearing_leaves_the_rest() {
        assert_eq!(
            without_terms("is:unread lunch has:attachment from:ada is:starred", &ALL),
            "lunch from:ada"
        );
        assert_eq!(without_terms("is:unread is:starred", &ALL), "");
        assert_eq!(without_terms("lunch", &ALL), "lunch");
    }

    /// A line is only terms when nothing else is in it.
    #[test]
    fn a_line_of_only_terms() {
        let cases = [
            ("", false),
            ("is:unread", true),
            ("is:unread has:attachment", true),
            ("is:unread lunch", false),
            ("is:unread from:ada", false),
            ("lunch", false),
            ("is:read", false),
        ];
        for (line, want) in cases {
            assert_eq!(only_terms(line, &ALL), want, "{line:?}");
        }
    }
}
