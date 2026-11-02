//! What a condition typed for a rule can mean: the search language, read strictly.
//!
//! A search box reads a word it does not know as text; a rule runs unseen on every message that
//! arrives, so it must not.

use chrono::TimeZone;
use mail_domain::{Filter, LabelId};

/// Why a condition cannot be a rule's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionError {
    /// Nothing was typed.
    Blank,
    /// `field:` with nothing after the colon.
    NoValue { field: String },
    /// `before:`/`after:` with something that is not a date.
    NotADate { word: String, field: String },
    /// `label:` naming a label the account does not have.
    NoLabel { value: String },
    /// `is:` with a word it does not take.
    BadIs,
    /// `in:` with a word it does not take.
    BadIn,
    /// `has:` with a word it does not take.
    BadHas,
    /// A `word:` the search language has no term for.
    UnknownField { field: String },
}

/// The condition `text` means, or why a rule cannot use it.
///
/// A search box reads a word it does not know as text, so as not to refuse what is being
/// typed. A rule is different: it runs unseen on every message that arrives, and `frm:bank` read
/// as the text "frm:bank" is a rule that silently never fires. So every `word:` term is read on
/// its own through the same parser, and one the parser did not take as that term is refused here
/// before it is kept.
///
/// `is:vip` is read as the clause it is, which asks the VIP list whenever the rule runs: a rule
/// follows the list, and does not keep the people on it when the rule was saved.
pub fn read_condition<Tz: TimeZone>(
    text: &str,
    labels: &[(String, LabelId)],
    zone: &Tz,
) -> Result<Filter, ConditionError> {
    if text.trim().is_empty() {
        return Err(ConditionError::Blank);
    }
    let named = crate::query::named(labels);
    for word in text.split_whitespace() {
        let bare = word.strip_prefix('-').unwrap_or(word);
        if bare.starts_with('"') {
            continue;
        }
        let Some((field, value)) = bare.split_once(':') else {
            continue;
        };
        let alone = crate::query::parse_with(bare, zone, &named);
        // Gmail's category is the server's to say, and a message here is never in one: a rule on
        // it would never fire, and the script pushed to the server cannot say it.
        if matches!(alone, Filter::Category(_)) {
            return Err(ConditionError::UnknownField {
                field: field.to_ascii_lowercase(),
            });
        }
        if !matches!(alone, Filter::Text(_)) {
            continue;
        }
        return Err(refused(bare, field.to_ascii_lowercase(), value));
    }
    Ok(crate::query::parse_with(text, zone, &named))
}

/// Why `word`, whose field is `field`, is not a term.
fn refused(word: &str, field: String, value: &str) -> ConditionError {
    if value.is_empty() {
        return ConditionError::NoValue { field };
    }
    match field.as_str() {
        "before" | "after" => ConditionError::NotADate {
            word: word.to_owned(),
            field,
        },
        "label" => ConditionError::NoLabel {
            value: value.to_owned(),
        },
        "is" => ConditionError::BadIs,
        "in" => ConditionError::BadIn,
        "has" => ConditionError::BadHas,
        _ => ConditionError::UnknownField { field },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn a_condition_the_rules_cannot_read_is_refused_by_kind() {
        use ConditionError::*;
        let cases = [
            (
                "frm:bank.example",
                UnknownField {
                    field: "frm".to_owned(),
                },
            ),
            (
                "from:bank.example before:yesterday",
                NotADate {
                    word: "before:yesterday".to_owned(),
                    field: "before".to_owned(),
                },
            ),
            (
                "label:nowhere",
                NoLabel {
                    value: "nowhere".to_owned(),
                },
            ),
            (
                "category:promotions",
                UnknownField {
                    field: "category".to_owned(),
                },
            ),
            ("is:important", BadIs),
            ("in:elsewhere", BadIn),
            ("has:wings", BadHas),
            (
                "subject:",
                NoValue {
                    field: "subject".to_owned(),
                },
            ),
            ("   ", Blank),
        ];
        for (typed, expect) in cases {
            assert_eq!(read_condition(typed, &[], &Utc), Err(expect), "{typed:?}");
        }
        // What a search reads as text but a rule reads fine: a phrase, a negation, a plain word.
        for fine in [
            "\"weekly report\"",
            "-from:boss@example.com invoice",
            "subject:\"re: lunch\"",
        ] {
            assert!(
                read_condition(fine, &[], &Utc).is_ok(),
                "{fine:?} was refused"
            );
        }
    }
}
