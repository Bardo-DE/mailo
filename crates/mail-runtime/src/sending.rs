//! Sending apart from receiving.
//!
//! Thunderbird's rule, and the one here: an account's outgoing side is not its incoming one, and
//! a message that could not be sent waits in the outbox, saying why, to go again later. So a
//! failure to send — the sending credential missing or refused, the submission server refusing
//! the protocol, a wait it asked for — is the outbox row's (its `last_error` and its next
//! attempt) and the draft's, never the account's: it does not ask the person to sign in to
//! receive, does not hold the account's next pass, and does not stop the pass that met it.
//! Failures in receiving are what they always were.
//!
//! Two places meet sending trouble, and both ask here: the outbox's drain, for a submission
//! that failed ([`settles`], [`tells_the_pass`]), and a pass that makes sure of a Graph token
//! before it starts ([`sending_alone`]).

mod settle;

pub use settle::{Login, Mark, Outcome, Settled, Then, settle};

use crate::RuntimeError;
use chrono::{DateTime, Utc};
use mail_domain::{AccountPlan, DraftId, Incoming, Outgoing, ProtoOp, Retry, Retryable, SendState};
use std::time::Duration;

/// How long a message waits when the server will not take mail from the account over the
/// protocol at all, before it is tried again: a day, as a refused credential's does.
pub const PROTOCOL_REFUSED_WAIT: Duration = Duration::from_secs(24 * 60 * 60);

/// Who files the copy of a message an account submits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SentCopy {
    /// The server does, and a sync reads it back.
    Server,
    /// This client does, in the local Sent: the account has no mailbox for it.
    Here,
    /// This client uploads it to the server's Sent mailbox at this path.
    Upload(String),
}

/// Who files the copy of a message `plan`'s account submits, given the path of the Sent mailbox
/// its server named, if it named one.
///
/// SMTP says only that a message was accepted (RFC 6409), so whether a Sent copy exists
/// afterwards depends on the pairing of servers, and each has one answer:
///
/// - Graph's `sendMail`, JMAP's `EmailSubmission` and a Gmail or Microsoft SMTP server file
///   it themselves, and a sync reads it back from Sent. Keeping one here too would list the
///   message twice.
/// - An IMAP account on any other SMTP server has a Sent mailbox and nobody filing into it,
///   so the client uploads the copy there (`APPEND`, RFC 3501 §6.3.11) when the server's
///   `SPECIAL-USE` listing names one (RFC 6154), or, with none, a folder at the top has a
///   name every client takes for Sent (`Sent`, `Sent Items`: `mail_proto`'s folder roles).
///   With neither, uploading to a guessed path is how a message lands somewhere nobody
///   looks, so the copy is kept here.
/// - A POP3 or local account has no Sent mailbox at all, whichever server sends for it, so
///   the copy is kept here: Graph's lands in a Sent Items a POP3 sync never reads.
pub fn sent_copy(plan: &AccountPlan, sent_folder: Option<String>) -> SentCopy {
    match (&plan.incoming, &plan.outgoing) {
        (_, Outgoing::Jmap | Outgoing::Nowhere) | (Incoming::Jmap { .. } | Incoming::Graph, _) => {
            SentCopy::Server
        }
        (Incoming::Pop3 { .. } | Incoming::Local, _) => SentCopy::Here,
        (Incoming::Imap { .. }, Outgoing::Graph) => SentCopy::Server,
        (Incoming::Imap { .. }, Outgoing::Smtp { host, .. }) => {
            if mail_domain::presets::files_sent_itself(host) {
                SentCopy::Server
            } else {
                match sent_folder {
                    Some(path) => SentCopy::Upload(path),
                    None => SentCopy::Here,
                }
            }
        }
    }
}

/// Record where a draft got to, without letting that record fail the drain.
///
/// A missing draft is not an error worth propagating: the user may have deleted it while the
/// outbox was backed off, and the message either was or was not accepted regardless. Failing
/// here would leave the outbox entry settled and the pass reporting an error about something
/// nobody is waiting on.
pub fn mark_draft(
    store: &dyn mail_store::Store,
    draft: DraftId,
    state: SendState,
    now: DateTime<Utc>,
) {
    crate::pass::note_state(store.set_send_state(draft, &state, now));
}

/// How a submission's failure settles its outbox row.
///
/// As the error says, except a server that refuses the protocol itself
/// ([`RuntimeError::RefusedFresh`]: SMTP AUTH switched off for the mailbox), which as `Fatal`
/// would drop the message. It is kept and waits ([`PROTOCOL_REFUSED_WAIT`]), saying why, like a
/// message whose credential was refused: the person may yet send it another way.
pub fn settles(error: &RuntimeError) -> Retry {
    match error {
        RuntimeError::RefusedFresh(_) => Retry::After(PROTOCOL_REFUSED_WAIT),
        _ => error.retry(),
    }
}

/// What a redirect says when the server will not send it, as Graph's refusal up front does.
pub const REDIRECT_REFUSED_FROM: &str = "the server will not send a message as someone who is not \
    on this account, and a redirect keeps its sender. Forward it instead";

/// Servers that refuse a message sent as another person's address say so as SMTP's enhanced
/// status 5.2.252 or 5.7.60 (Exchange Online), or JMAP's `forbiddenFrom` (RFC 8621 §7.5). Where
/// `resent` says the message was a redirect, that refusal is the redirect's, and the person is
/// told to forward it instead; anything else is as the server said.
pub fn redirect_refused(error: RuntimeError, resent: bool) -> RuntimeError {
    use mail_proto::{ProtoError, Refusal};
    let RuntimeError::Proto(ProtoError::Refused {
        kind: Refusal::Permanent,
        text,
    }) = &error
    else {
        return error;
    };
    let from_refused = ["5.2.252", "5.7.60", "forbiddenFrom"]
        .iter()
        .any(|marker| text.contains(marker));
    if resent && from_refused {
        return RuntimeError::Proto(ProtoError::Refused {
            kind: Refusal::Permanent,
            text: format!("{REDIRECT_REFUSED_FROM} ({text})"),
        });
    }
    error
}

/// Whether the account's pass hears of what `op`'s failure asks for (a sign-in, a wait): every
/// operation's but a submission's, which is the outbox row's alone.
///
/// A message refused for good, or refused the protocol for, is still said, as a note: the
/// person has to hear it somewhere. A note holds nothing and stops nothing.
pub fn tells_the_pass(op: &ProtoOp) -> bool {
    !matches!(op, ProtoOp::Submit { .. })
}

/// Whether the Graph token `plan` makes sure of before a pass is for sending alone, so that a
/// failure to get one is sending's: an account that reads over IMAP or POP3 and sends through
/// Graph. One that reads through Graph presents the same token to receive, and its failure is
/// the pass's.
pub fn sending_alone(plan: &AccountPlan) -> bool {
    plan.outgoing == Outgoing::Graph && plan.incoming != Incoming::Graph
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redirect_a_server_will_not_send_as_another_person_says_to_forward_it() {
        use mail_proto::{ProtoError, Refusal};
        let refused = |text: &str| {
            RuntimeError::Proto(ProtoError::Refused {
                kind: Refusal::Permanent,
                text: text.to_owned(),
            })
        };
        let said = |error: RuntimeError| match error {
            RuntimeError::Proto(ProtoError::Refused { text, .. }) => text,
            other => panic!("not a refusal: {other:?}"),
        };
        for server in [
            "550 5.7.60 SMTP; Client does not have permissions to send as this sender",
            "550 5.2.252 SendAsDenied",
            "submitting the message: forbiddenFrom",
        ] {
            let told = said(redirect_refused(refused(server), true));
            assert!(told.contains("Forward it instead"), "{told}");
            assert!(told.contains(server), "what the server said stays: {told}");
            // The same refusal of a message that is not a redirect is the server's own words.
            assert_eq!(said(redirect_refused(refused(server), false)), server);
        }
        assert_eq!(
            said(redirect_refused(refused("550 5.1.1 no such user"), true)),
            "550 5.1.1 no such user"
        );
    }

    #[test]
    fn a_refused_protocol_keeps_the_message_and_anything_else_says_what_it_says() {
        let refused = RuntimeError::RefusedFresh(Box::new(RuntimeError::NoServer("send from")));
        assert_eq!(settles(&refused), Retry::After(PROTOCOL_REFUSED_WAIT));
        let gone = RuntimeError::NoServer("send from");
        assert_eq!(settles(&gone), gone.retry());
    }
}
