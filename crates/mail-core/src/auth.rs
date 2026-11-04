//! What the server that received a message says it checked about the sender: SPF, DKIM and
//! DMARC, from the `Authentication-Results` field `mail_mime::auth` believes.
//!
//! Which field is believed follows from who received the mail, and that is the account's
//! server: its registered domain is the provider's, and only a field whose authserv-id lies
//! under it is read (`mail_mime::Receiver::Domains`). An account with no server name to go on (a
//! local one, Graph, an IP literal) reads only the topmost field.
//!
//! Read from the stored raw message each time, like the list headers (`crate::unsubscribe`):
//! the raw blob already holds the field, and a column would need a migration and a change to
//! the frozen `Message` to cache something shown for one message at a time.

pub use mail_logic::auth::{Standing, sentence, standing, words};

use crate::error::Logged;
use mail_domain::{AccountPlan, Message};
use mail_mime::{AuthResults, Receiver};
use mail_store::SqliteStore;
use porter_core::AccountId;

/// Providers whose receiving servers sign as a domain other than the one their mail is read
/// from, keyed by the registered domain of the account's server. Google's help pages show its
/// fields as `Authentication-Results: mx.google.com; …` for mail read at `imap.gmail.com`.
const ALSO_SIGNS_AS: &[(&str, &[&str])] = &[
    ("gmail.com", &["google.com"]),
    ("googlemail.com", &["google.com"]),
];

/// Whose `Authentication-Results` to believe for mail that came to an account with `plan`.
pub fn receiver(plan: &AccountPlan) -> Receiver {
    let host = plan.incoming.served_from();
    let Some(registered) = host.as_deref().and_then(mail_logic::trust::registered) else {
        return Receiver::Topmost;
    };
    let mut domains = vec![registered.clone()];
    if let Some((_, also)) = ALSO_SIGNS_AS.iter().find(|(d, _)| *d == registered) {
        domains.extend(also.iter().map(|d| (*d).to_owned()));
    }
    Receiver::Domains(domains)
}

/// The account's plan, when the store has one that reads.
fn plan_of(store: &SqliteStore, account: AccountId) -> Option<AccountPlan> {
    store
        .account(account)
        .or_log("an account could not be read")??
        .plan
        .or_log("an account's plan could not be read")
}

/// What the receiving server said about `message`'s sender, read from its stored bytes. `None`
/// when the body is not here (fetching it would be a POP3 `RETR`, which marks it read), or when
/// no field is believed. Reads a blob: call it off the thread that draws.
pub fn results_of(store: &SqliteStore, message: &Message) -> Option<AuthResults> {
    let raw = message.body.raw()?;
    let bytes = store
        .blobs()
        .get(raw)
        .or_log("a message's bytes could not be read")?;
    let receiver = plan_of(store, message.account.clone())
        .as_ref()
        .map_or(Receiver::Topmost, receiver);
    mail_mime::authentication_results(&bytes, &receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::{AuthPlan, Identity, Incoming, LeaveOnServer, Outgoing, Tls};

    fn plan(incoming: Incoming) -> AccountPlan {
        AccountPlan {
            address: "me@example.com".to_owned(),
            incoming,
            outgoing: Outgoing::Smtp {
                host: "smtp.example.com".to_owned(),
                port: 465,
                tls: Tls::Implicit,
            },
            auth: AuthPlan::Password {
                username: mail_domain::Username::SameAsAddress,
                sasl: Vec::new(),
            },
            identities: Vec::<Identity>::new(),
        }
    }

    fn imap(host: &str) -> Incoming {
        Incoming::Imap {
            host: host.to_owned(),
            port: 993,
            tls: Tls::Implicit,
        }
    }

    #[test]
    fn the_receiver_is_the_account_servers_registered_domain() {
        let domains =
            |list: &[&str]| Receiver::Domains(list.iter().map(|d| (*d).to_owned()).collect());
        let cases: Vec<(&str, Incoming, Receiver)> = vec![
            (
                "gmail signs as google",
                imap("imap.gmail.com"),
                domains(&["gmail.com", "google.com"]),
            ),
            (
                "a registered domain",
                imap("mail.example.co.uk"),
                domains(&["example.co.uk"]),
            ),
            (
                "pop3 too",
                Incoming::Pop3 {
                    host: "pop.example.com".to_owned(),
                    port: 995,
                    tls: Tls::Implicit,
                    leave: LeaveOnServer::Keep,
                },
                domains(&["example.com"]),
            ),
            ("an ip literal", imap("192.0.2.1"), Receiver::Topmost),
            ("local mail", Incoming::Local, Receiver::Topmost),
        ];
        for (name, incoming, want) in cases {
            assert_eq!(receiver(&plan(incoming)), want, "case: {name}");
        }
    }
}
