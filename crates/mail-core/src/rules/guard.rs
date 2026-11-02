//! When a rule may send mail on its own: the one question every automatic send asks, over the
//! store.
//!
//! What may be sent for (the headers, the sender, how fresh the mail is) is
//! `mail_logic::rules::guard`; this reads the message's bytes, and remembers whom a Reply has
//! answered already. Reply and Forward both ask [`guard`] before they make a draft; nothing else
//! decides.

use crate::error::CoreError;
use chrono::{DateTime, TimeDelta, Utc};
use mail_domain::Message;
use mail_logic::arrival::DraftRequest;
use mail_logic::own::Own;
use mail_logic::rules::guard::{Occasion, Refusal, Sends, headers_allow, timely};
use mail_store::SqliteStore;

/// Whether the rule behind `left` may send for `message`, and, for a Reply, that it has now
/// answered the sender. The one function every automatic send goes through.
pub fn guard(
    store: &SqliteStore,
    left: &DraftRequest,
    message: &Message,
    own: &Own,
    occasion: Occasion,
    now: DateTime<Utc>,
) -> Result<(), Refusal> {
    let Some(sends) = Sends::of(&left.action) else {
        // Saying it arrived sends nothing.
        return Ok(());
    };
    timely(occasion, left.made, message, now)?;
    let raw = crate::export::raw_message(store, message.id)
        .or_else_none("a rule could not read a message");
    headers_allow(raw.as_deref(), &message.from.email, own, sends)?;
    if sends == Sends::Reply {
        let within = TimeDelta::days(i64::from(left.reply_days));
        let free = store
            .claim_reply(left.rule, &message.from.email, now, within)
            .unwrap_or(false);
        if !free {
            return Err(Refusal::Recently);
        }
    }
    Ok(())
}

trait OrElseNone {
    fn or_else_none(self, what: &str) -> Option<Vec<u8>>;
}

impl OrElseNone for Result<Option<Vec<u8>>, CoreError> {
    fn or_else_none(self, what: &str) -> Option<Vec<u8>> {
        match self {
            Ok(raw) => raw,
            Err(why) => {
                log::warn!("{what}: {why}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::{ReadState, RuleAction};

    #[test]
    fn mail_a_rule_may_send_for_but_whose_bytes_are_not_here_is_not_sent_for() {
        use crate::support_tests::{at, message};
        use mail_domain::Star;
        let store = mail_store::testing::in_memory();
        let now = at(10 * 86_400);
        let left = DraftRequest {
            message: mail_domain::MessageId::generate(),
            action: RuleAction::Forward("friend@example.test".to_owned()),
            rule: mail_domain::RuleId::generate(),
            made: at(0),
            reply_days: 7,
        };
        let message = message(1, "Hello", "Body", 86_400, ReadState::Read, Star::Unstarred);
        // Asked for by the person: old and read mail is theirs to send for. It goes on to read
        // the message's bytes, which this store lacks.
        assert_eq!(
            guard(
                &store,
                &left,
                &message,
                &Own::default(),
                Occasion::Asked,
                now
            ),
            Err(Refusal::NotHere)
        );
    }
}
