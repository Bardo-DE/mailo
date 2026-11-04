//! A search line split into operators, finished words, and the word still being typed.
//!
//! Operators come from [`crate::query::parse_with`], so `from:dana` means the same thing here as
//! it does in the rest of the app. The free words stay separate because prefix expansion
//! completes only the last one, and only while it is still being typed.

use chrono::TimeZone;
use mail_domain::{Filter, TextMatch};
use std::ops::Range;

/// What one search line is, before it becomes a [`Filter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// Operator filters, in the order they were typed.
    pub operators: Vec<Filter>,
    /// Those operators as typed (`from:dana`), for the menu.
    pub operator_tokens: Vec<String>,
    /// Where each of `operator_tokens` lies in the input, as byte ranges: one range per token it
    /// was read from, so an either-or reads from three (`from:ada`, `OR`, `from:bob`).
    pub operator_spans: Vec<Vec<Range<usize>>>,
    /// Where every token that is not part of an operator lies: the free words, the phrases and
    /// whatever else the parser read as nothing, in the order typed.
    pub rest_spans: Vec<Range<usize>>,
    /// Free words whose trailing whitespace has already been typed.
    pub words: Vec<String>,
    /// Quoted phrases, without the quote marks.
    pub phrases: Vec<String>,
    /// The last free word, when the input does not end in whitespace.
    pub typing: Option<String>,
}

impl Parsed {
    /// The free words, including the one still being typed.
    pub fn query_words(&self) -> Vec<String> {
        let mut words = self.words.clone();
        if let Some(word) = &self.typing {
            words.push(word.clone());
        }
        words
    }

    /// Whether nothing was typed: no operators, no words, no phrase.
    pub fn is_empty(&self) -> bool {
        self.operators.is_empty()
            && self.words.is_empty()
            && self.phrases.is_empty()
            && self.typing.is_none()
    }
}

/// Split `input` with [`crate::query::parse_with`].
///
/// `zone` resolves `before:` and `after:`. `names` resolves `label:`, `account:` and the
/// relative dates ([`crate::query::Names`]). A caller with no index passes
/// `&crate::query::unnamed` and `label:travel` becomes text, which is the parser's own rule.
///
/// Terms joined by `OR` or `|` are one operator, typed as they were joined: an either-or of
/// words is not a word to complete.
pub fn parse<Tz: TimeZone>(input: &str, zone: &Tz, label: &dyn crate::query::Names) -> Parsed {
    let ended_with_space = input.ends_with(char::is_whitespace);
    let mut operators = Vec::new();
    let mut operator_tokens = Vec::new();
    let mut operator_spans = Vec::new();
    let mut rest_spans = Vec::new();
    let mut words = Vec::new();
    let mut phrases = Vec::new();

    let tokens = raw_spans(input)
        .into_iter()
        .map(|span| (input[span.clone()].to_owned(), vec![span]))
        .collect();
    for (token, spans) in either_joined(tokens) {
        match crate::query::parse_with(&token, zone, label) {
            Filter::All => rest_spans.extend(spans),
            Filter::Text(TextMatch::Contains(word)) => {
                words.push(word);
                rest_spans.extend(spans);
            }
            Filter::Text(TextMatch::Exact(phrase)) => {
                phrases.push(phrase);
                rest_spans.extend(spans);
            }
            other => {
                operators.push(other);
                operator_tokens.push(token);
                operator_spans.push(spans);
            }
        }
    }

    // The last free word is still being typed unless whitespace already closed it. A phrase
    // has its closing quote, so it is finished even at the end of the line.
    let typing = if ended_with_space { None } else { words.pop() };

    Parsed {
        operators,
        operator_tokens,
        operator_spans,
        rest_spans,
        words,
        phrases,
        typing,
    }
}

/// `tokens` with each run joined by `OR` or `|` made one token, `a OR b`, so the query parser
/// reads it as one either-or term; each carries the ranges of the tokens it was made from,
/// joiners included. A joiner with nothing on one side stays a token of its own, which parses to
/// nothing.
fn either_joined(tokens: Vec<(String, Vec<Range<usize>>)>) -> Vec<(String, Vec<Range<usize>>)> {
    let joiner = crate::query::joins;
    let mut out: Vec<(String, Vec<Range<usize>>)> = Vec::new();
    let mut joining = false;
    for (token, spans) in tokens {
        if joiner(&token) {
            joining = out.last().is_some_and(|(last, _)| !joiner(last));
            if joining {
                if let Some((_, held)) = out.last_mut() {
                    held.extend(spans);
                }
            } else {
                out.push((token, spans));
            }
            continue;
        }
        match out.last_mut() {
            Some((last, held)) if joining => {
                last.push_str(" OR ");
                last.push_str(&token);
                held.extend(spans);
            }
            _ => out.push((token, spans)),
        }
        joining = false;
    }
    out
}

/// Where each whitespace-separated token of `input` lies, as byte ranges, keeping a quoted run
/// — including `subject:"lunch on friday"` — as one token. A range covers the quotes and a
/// leading `-`, because [`parse`] hands each token to `parse_with` whole.
fn raw_spans(input: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((from, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        let mut to = from + c.len_utf8();
        if c == '"' {
            close_quote(&mut chars, &mut to);
        } else {
            while let Some(&(at, next)) = chars.peek() {
                if next.is_whitespace() {
                    break;
                }
                chars.next();
                to = at + next.len_utf8();
                if next == '"' && input[from..to].ends_with(":\"") {
                    close_quote(&mut chars, &mut to);
                    break;
                }
            }
        }
        out.push(from..to);
    }
    out
}

/// Move `to` past the next closing quote (or the end), taking its characters from `chars`.
fn close_quote(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>, to: &mut usize) {
    for (at, quote) in chars.by_ref() {
        *to = at + quote.len_utf8();
        if quote == '"' {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    /// What each operator's spans cut out of `input`, one string per token it was read from.
    fn cut(input: &str) -> Vec<Vec<&str>> {
        parse(input, &Utc, &crate::query::unnamed)
            .operator_spans
            .iter()
            .map(|spans| spans.iter().map(|span| &input[span.clone()]).collect())
            .collect()
    }

    #[test]
    fn an_operator_knows_the_tokens_it_was_read_from() {
        let cases: &[(&str, &[&[&str]])] = &[
            ("lunch", &[]),
            ("from:ada  lunch", &[&["from:ada"]]),
            (
                "is:unread subject:\"lunch plans\" ada",
                &[&["is:unread"], &["subject:\"lunch plans\""]],
            ),
            // A word that holds an operator's text is a word.
            ("xfrom:ada from:ada", &[&["from:ada"]]),
            ("a from:ada | from:bob b", &[&["from:ada", "|", "from:bob"]]),
            (
                "from:ada OR from:bob lunch",
                &[&["from:ada", "OR", "from:bob"]],
            ),
            ("héllo from:ada", &[&["from:ada"]]),
        ];
        for (input, want) in cases {
            assert_eq!(cut(input), *want, "{input}");
        }
    }

    #[test]
    fn what_is_not_an_operator_is_the_rest() {
        let rest = |input: &str| -> Vec<String> {
            parse(input, &Utc, &crate::query::unnamed)
                .rest_spans
                .iter()
                .map(|span| input[span.clone()].to_owned())
                .collect()
        };
        assert_eq!(rest("a from:ada \"b c\" d"), ["a", "\"b c\"", "d"]);
        assert_eq!(rest("from:ada | from:bob"), Vec::<String>::new());
        assert_eq!(rest("xfrom:ada from:ada"), ["xfrom:ada"]);
        assert_eq!(rest("a |"), ["a", "|"]);
    }
}
