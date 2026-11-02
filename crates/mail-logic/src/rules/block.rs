//! "Block sender" as a decision: the condition a block is, which address a conversation's block
//! means, and whether a rule already is one. Writing the rule is `mail_core::rules::block`.

use crate::own::Own;
use chrono::{DateTime, Utc};
use mail_domain::{Filter, Rule, RuleAction, TextMatch};

/// The condition a block of `email` is: its From is exactly that address.
pub fn condition(email: &str) -> Filter {
    Filter::From(TextMatch::Exact(email.trim().to_owned()))
}

/// The address of the newest of `senders` (dated, then the address) that `own` does not include.
pub fn newest_outside(senders: Vec<(DateTime<Utc>, String)>, own: &Own) -> Option<String> {
    senders
        .into_iter()
        .filter(|(_, email)| !own.includes(email))
        .max_by_key(|(date, _)| *date)
        .map(|(_, email)| email)
}

/// Whether `rule` is a block of `email`: its condition, and Spam among its actions.
pub fn blocks(rule: &Rule, email: &str) -> bool {
    let same = match &rule.filter {
        Filter::From(TextMatch::Exact(had)) => had.eq_ignore_ascii_case(email.trim()),
        _ => false,
    };
    same && rule.actions.contains(&RuleAction::Spam)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 24, hour, 0, 0).unwrap()
    }

    #[test]
    fn the_newest_sender_who_is_not_the_user_is_the_one_to_block() {
        let own = Own::new(["Me@Example.test", "alias@example.test"]);
        let senders = vec![
            (at(9), "pest@spam.example".to_owned()),
            (at(11), "ME@example.test".to_owned()),
            (at(10), "other@spam.example".to_owned()),
            (at(12), "alias@example.test".to_owned()),
        ];
        assert_eq!(
            newest_outside(senders, &own).as_deref(),
            Some("other@spam.example"),
            "the last word is the user's own, so the newest sender besides them"
        );
    }

    #[test]
    fn a_conversation_the_user_wrote_alone_has_no_one_to_block() {
        let own = Own::new(["me@example.test"]);
        let senders = vec![(at(9), "Me@example.test".to_owned())];
        assert_eq!(newest_outside(senders, &own), None);
        assert_eq!(newest_outside(Vec::new(), &own), None);
    }
}
