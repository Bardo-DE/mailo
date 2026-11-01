//! Fetching and keeping each provider's icon. Which provider an account is on, and what it is
//! called, is `mail_logic::provider`.

#[path = "provider/icon/mod.rs"]
pub mod icon;

#[cfg(test)]
mod tests {
    use mail_domain::{
        AccountPlan, AuthPlan, HttpAuth, Incoming, Outgoing, SaslMech, Tls, Username,
    };
    use mail_logic::provider::{Provider, provider};

    /// A JMAP session URL is read once, so a port, a user part, an upper-case name or an IPv6
    /// address gives the provider, the sieve server and the receiver the same host.
    #[test]
    fn a_jmap_session_url_has_one_host_for_every_reader() {
        use mail_logic::trust::registered;
        use mail_mime::auth::Receiver;
        use mail_proto::sieve::{NoSieve, endpoint};
        use porter_provider::Issuer;

        let base = AccountPlan {
            address: "a@example.test".to_owned(),
            incoming: Incoming::Imap {
                host: "mail.example.test".to_owned(),
                port: 993,
                tls: Tls::Implicit,
            },
            outgoing: Outgoing::Smtp {
                host: "smtp.example".to_owned(),
                port: 465,
                tls: Tls::Implicit,
            },
            auth: AuthPlan::Password {
                username: Username::SameAsAddress,
                sasl: vec![SaslMech::Plain],
            },
            identities: Vec::new(),
        };
        // The session, the host it names, and the provider that host belongs to.
        let cases = [
            (
                "https://mail.example.test/.well-known/jmap",
                "mail.example.test",
                Provider::Imap,
            ),
            (
                "https://ada:pw@mail.example.test:8443/jmap",
                "mail.example.test",
                Provider::Imap,
            ),
            (
                "https://Mail.Example.Test:443/jmap?x=1#top",
                "mail.example.test",
                Provider::Imap,
            ),
            (
                "https://[2001:db8::1]:8443/jmap",
                "2001:db8::1",
                Provider::Imap,
            ),
            (
                "https://ada@imap.gmail.com:8443/jmap",
                "imap.gmail.com",
                Provider::Google,
            ),
        ];
        for (session, host, expect) in cases {
            let plan = AccountPlan {
                incoming: Incoming::Jmap {
                    session: session.to_owned(),
                    auth: HttpAuth::Basic,
                },
                ..base.clone()
            };
            assert_eq!(
                plan.incoming.served_from().as_deref(),
                Some(host),
                "{session}"
            );
            assert_eq!(provider(&plan), expect, "{session}");
            match endpoint(&plan) {
                Ok(found) => assert_eq!(found.host, host, "{session}"),
                Err(why) => {
                    assert_eq!(why, NoSieve::Provider(Issuer::Google), "{session}");
                    assert_eq!(expect, Provider::Google, "{session}");
                }
            }
            let believed = match registered(host) {
                Some(domain) if domain == "gmail.com" => {
                    Receiver::Domains(vec![domain, "google.com".to_owned()])
                }
                Some(domain) => Receiver::Domains(vec![domain]),
                None => Receiver::Topmost,
            };
            assert_eq!(crate::auth::receiver(&plan), believed, "{session}");
        }
    }
}
