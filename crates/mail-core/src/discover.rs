//! Looking an address up over the network, for the servers the built-in table does not know.
//!
//! What a domain's servers are, and where to look, is `porter-discover`'s; what the answer comes
//! to, the table of addresses that need no lookup, and the provider files are
//! `mail_logic::discover`'s. The lookup runs over mailo's own resolver and HTTP client
//! (`mail_net::lookup`, which implements the two seams `porter-discover` asks through). Saying
//! what was found, and getting a yes before any of it is used, is the front-end's: at the
//! terminal, `mail_cli::discover`.

mod jmap;
mod offer;
pub use jmap::find as find_jmap;
pub use offer::{Looked, resolve, settle, typed, typed_domain};

use crate::error::CoreError;
use chrono::{DateTime, Utc};
use mail_domain::Retry;
use mail_logic::discover::providers;
use mail_logic::discover::{Failed, Found, Gap, classify, map};
use porter_discover::{Dns, NotFound, Outcome, SearchOptions};
use porter_http::Http;

/// Record the clients that decide whether a personal Microsoft account (`outlook.com`,
/// `hotmail.com`) can be signed in: it can when they hold a Microsoft client registered for
/// personal accounts (`mail_secrets::clients::personal_microsoft_ready`). They are asked at each
/// address, so a client set up while the program runs counts. None, as in every test, refuses a
/// personal address and says why.
pub fn judge_personal_microsoft_by(clients: Option<crate::OAuthClients>) {
    mail_logic::discover::judge_personal_microsoft_by(clients.map(|clients| {
        std::sync::Arc::new(move || clients.takes_personal_microsoft())
            as std::sync::Arc<dyn Fn() -> bool + Send + Sync>
    }));
}

impl crate::mail::DiscoverOps<'_> {
    /// Look the address up, over the network, at the handle's time.
    pub async fn lookup(&self, address: &str) -> Result<Found, CoreError> {
        lookup(address, self.0.now()).await
    }

    /// [`lookup`], with the reason a miss was left typed.
    pub async fn search(&self, address: &str) -> Result<Found, Failed> {
        search(address, self.0.now()).await
    }

    /// Follow a JMAP session `url` over the network, without a credential.
    pub async fn find_jmap(&self, url: &str) -> Result<String, CoreError> {
        jmap::find(url).await
    }
}

/// Look the address up, over the network.
pub async fn lookup(address: &str, now: DateTime<Utc>) -> Result<Found, CoreError> {
    Ok(search(address, now).await?)
}

/// [`lookup`], with the reason a miss was left typed.
pub async fn search(address: &str, now: DateTime<Utc>) -> Result<Found, Failed> {
    let http = mail_net::lookup::http();
    let dns = mail_net::lookup::SystemDns::new().map_err(|e| Failed::Broken(e.to_string()))?;
    match tokio::time::timeout(mail_net::lookup::TOTAL, run(&http, &dns, address, now)).await {
        Ok(found) => found,
        Err(_) => Err(Failed::Unreachable {
            address: address.to_owned(),
            retry: Retry::Now,
            why: format!("no answer in {} seconds", mail_net::lookup::TOTAL.as_secs()),
        }),
    }
}

/// The search as mailo runs it: every source, a document that is usable only in part decided the
/// way mailo always did ([`SearchOptions`]), the answer mapped onto a plan. Over the two seams, so
/// a test runs it against tables.
///
/// - A server that needs STARTTLS is a miss and the next source is tried; a domain whose every
///   source ends so is told how to configure the account by hand ([`Gap::StartTlsOnly`]).
/// - A document that offers OAuth2 only, on a host that is no issuer's, is dropped as a miss and
///   the search goes on, never a password plan.
/// - A document's POP3 server serves when it names no IMAP one.
pub async fn run<H: Http, D: Dns>(
    http: &H,
    dns: &D,
    address: &str,
    now: DateTime<Utc>,
) -> Result<Found, Failed> {
    const TRY_NEXT: SearchOptions = SearchOptions {
        starttls_only: true,
        report_pop3: true,
        oauth_only_ok: true,
    };
    match find(http, dns, address, &TRY_NEXT).await {
        Ok(outcome) => map(outcome, address, now),
        Err(why) => {
            let failed = classify(address, &why);
            if why.offline() {
                return Err(failed);
            }
            // Nothing usable: was a STARTTLS-only document the reason? Asked again with the rule
            // off, only to say so (and to take an OAuth preset a STARTTLS document leads to).
            let accepting = SearchOptions {
                starttls_only: false,
                ..TRY_NEXT
            };
            match find(http, dns, address, &accepting).await {
                Ok(outcome) => match map(outcome, address, now) {
                    Ok(found) => Ok(found),
                    Err(Failed::NoServers {
                        gap: Gap::StartTlsOnly,
                        address,
                        tried,
                    }) => Err(Failed::NoServers {
                        address,
                        gap: Gap::StartTlsOnly,
                        tried,
                    }),
                    Err(_) => Err(failed),
                },
                Err(_) => Err(failed),
            }
        }
    }
}

/// `discover_mail_with`, dropping a finding that is OAuth2 only on a host that is no issuer's:
/// the search is made again with those servers a miss, which is the document skipped and every
/// later source tried.
async fn find<H: Http, D: Dns>(
    http: &H,
    dns: &D,
    address: &str,
    options: &SearchOptions,
) -> Result<Outcome, NotFound> {
    let providers = providers::set();
    let outcome = porter_discover::discover_mail_with(http, dns, providers, address, options).await;
    match outcome {
        Ok(Outcome::Servers(servers))
            if options.oauth_only_ok
                && servers.oauth.is_some()
                && mail_logic::discover::oauth_issuer(&servers).is_none() =>
        {
            let without = SearchOptions {
                oauth_only_ok: false,
                ..*options
            };
            porter_discover::discover_mail_with(http, dns, providers, address, &without).await
        }
        other => other,
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::presets;
    use mail_domain::{AuthPlan, Incoming, Outgoing, SaslMech, Tls, Username};
    use mail_logic::discover::Source;
    use porter_discover::{DnsFault, MxRecord, SrvRecord};
    use porter_http::http::{Request, Response, StatusCode};
    use porter_http::{Http, HttpError};
    use porter_provider::{DomainName, Issuer};
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-24T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn name(text: &str) -> DomainName {
        DomainName::parse(text).unwrap()
    }

    /// Both seams of discovery, answering from tables: no test asks a real resolver or server.
    #[derive(Default)]
    struct Net {
        /// URL to document; any other URL is a 404.
        documents: HashMap<String, String>,
        mx: HashMap<String, Vec<MxRecord>>,
        asked: Mutex<Vec<String>>,
    }

    impl Http for Net {
        async fn send(&self, request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, HttpError> {
            let url = request.uri().to_string();
            self.asked.lock().unwrap().push(url.clone());
            let (status, body) = match self.documents.get(&url) {
                Some(body) => (200, body.clone()),
                None => (404, String::new()),
            };
            Ok(Response::builder()
                .status(StatusCode::from_u16(status).expect("a status"))
                .body(body.into_bytes())
                .expect("a response"))
        }
    }

    impl Dns for Net {
        async fn srv(&self, _: &str) -> Result<Vec<SrvRecord>, DnsFault> {
            Err(DnsFault::NoRecords)
        }

        async fn mx(&self, domain: &DomainName) -> Result<Vec<MxRecord>, DnsFault> {
            self.mx
                .get(domain.as_str())
                .cloned()
                .ok_or(DnsFault::NoRecords)
        }
    }

    impl Net {
        fn with_mx(domain: &str, host: &str) -> Net {
            let mut net = Net::default();
            net.mx.insert(
                domain.to_owned(),
                vec![MxRecord {
                    preference: 10,
                    host: name(host),
                }],
            );
            net
        }

        /// The discovery search for `address`, mapped as `search` maps it.
        async fn found(&self, address: &str) -> Result<Found, Failed> {
            run(self, self, address, now()).await
        }
    }

    fn document(servers: &str) -> String {
        format!(
            r#"<?xml version="1.0"?>
    <clientConfig version="1.1"><emailProvider id="example.test">{servers}</emailProvider></clientConfig>"#
        )
    }

    fn server(
        side: &str,
        kind: &str,
        (host, port, socket): (&str, u16, &str),
        user: &str,
        auth: &[&str],
    ) -> String {
        let auth: String = auth
            .iter()
            .map(|a| format!("<authentication>{a}</authentication>"))
            .collect();
        format!(
            "<{side}Server type=\"{kind}\"><hostname>{host}</hostname><port>{port}</port>\
             <socketType>{socket}</socketType><username>{user}</username>{auth}</{side}Server>"
        )
    }

    fn imap_smtp(
        imap: (&str, u16, &str),
        smtp: (&str, u16, &str),
        user: &str,
        auth: &[&str],
    ) -> String {
        document(&format!(
            "{}{}",
            server("incoming", "imap", imap, user, auth),
            server("outgoing", "smtp", smtp, user, auth)
        ))
    }

    fn imap_host(found: &Found) -> &str {
        match &found.preset.plan.incoming {
            Incoming::Imap { host, .. } => host,
            other => panic!("{other:?}"),
        }
    }

    /// A Fastmail, iCloud, Yahoo or GMX address resolves to its file with no request at all, and the
    /// plan is a password at the hosts the file names. The hosts are the providers' published ones,
    /// never checked against the servers (`tests/live_presets.rs`).
    #[tokio::test]
    async fn an_address_at_a_known_provider_resolves_to_its_file_without_asking_the_network() {
        use Tls::{Implicit, StartTlsRequired};
        type Row = (
            &'static str,
            Option<&'static str>,
            &'static str,
            (&'static str, u16),
            (&'static str, u16, Tls),
        );
        const CASES: &[Row] = &[
            (
                "me@fastmail.com",
                None,
                "Fastmail",
                ("imap.fastmail.com", 993),
                ("smtp.fastmail.com", 465, Implicit),
            ),
            (
                "me@fastmail.fm",
                None,
                "Fastmail",
                ("imap.fastmail.com", 993),
                ("smtp.fastmail.com", 465, Implicit),
            ),
            (
                "me@icloud.com",
                None,
                "iCloud",
                ("imap.mail.me.com", 993),
                ("smtp.mail.me.com", 587, StartTlsRequired),
            ),
            (
                "me@me.com",
                None,
                "iCloud",
                ("imap.mail.me.com", 993),
                ("smtp.mail.me.com", 587, StartTlsRequired),
            ),
            (
                "me@mac.com",
                None,
                "iCloud",
                ("imap.mail.me.com", 993),
                ("smtp.mail.me.com", 587, StartTlsRequired),
            ),
            (
                "me@yahoo.com",
                None,
                "Yahoo Mail",
                ("imap.mail.yahoo.com", 993),
                ("smtp.mail.yahoo.com", 465, Implicit),
            ),
            (
                "me@ymail.com",
                None,
                "Yahoo Mail",
                ("imap.mail.yahoo.com", 993),
                ("smtp.mail.yahoo.com", 465, Implicit),
            ),
            (
                "me@gmx.de",
                None,
                "GMX",
                ("imap.gmx.com", 993),
                ("mail.gmx.com", 465, Implicit),
            ),
            (
                "me@gmx.net",
                None,
                "GMX",
                ("imap.gmx.com", 993),
                ("mail.gmx.com", 465, Implicit),
            ),
            // A domain of their own, found by its MX.
            (
                "me@firm.example",
                Some("in1-smtp.messagingengine.com"),
                "Fastmail",
                ("imap.fastmail.com", 993),
                ("smtp.fastmail.com", 465, Implicit),
            ),
            (
                "me@firm.example",
                Some("mx01.mail.icloud.com"),
                "iCloud",
                ("imap.mail.me.com", 993),
                ("smtp.mail.me.com", 587, StartTlsRequired),
            ),
            (
                "me@firm.example",
                Some("mta5.am0.yahoodns.net"),
                "Yahoo Mail",
                ("imap.mail.yahoo.com", 993),
                ("smtp.mail.yahoo.com", 465, Implicit),
            ),
            (
                "me@firm.example",
                Some("mx00.gmx.net"),
                "GMX",
                ("imap.gmx.com", 993),
                ("mail.gmx.com", 465, Implicit),
            ),
        ];
        for (address, mx, label, imap, smtp) in CASES {
            let net = match mx {
                Some(host) => Net::with_mx("firm.example", host),
                None => Net::default(),
            };
            let found = net
                .found(address)
                .await
                .unwrap_or_else(|e| panic!("{address}: {e:?}"));
            let plan = &found.preset.plan;
            assert_eq!(
                plan.incoming,
                Incoming::Imap {
                    host: imap.0.to_owned(),
                    port: imap.1,
                    tls: Implicit
                },
                "{address} via {mx:?}"
            );
            assert_eq!(
                plan.outgoing,
                Outgoing::Smtp {
                    host: smtp.0.to_owned(),
                    port: smtp.1,
                    tls: smtp.2
                },
                "{address} via {mx:?}"
            );
            assert!(
                matches!(&plan.auth, AuthPlan::Password { username: Username::SameAsAddress, sasl } if sasl == &[SaslMech::Plain]),
                "{address}: {:?}",
                plan.auth
            );
            assert_eq!(plan.address, *address);
            assert!(
                found.source.to_string().contains(label),
                "{address}: {}",
                found.source
            );
            let via_mx = matches!(found.source, Source::ProviderMx { .. });
            assert_eq!(via_mx, mx.is_some(), "{address}");
            // A listed domain answers before any fetch; an MX is read after the documents.
            assert_eq!(
                net.asked.lock().unwrap().is_empty(),
                mx.is_none(),
                "{address}"
            );
        }
    }

    /// A custom domain whose mail exchanger is in Google's or Microsoft's own domain is that
    /// provider's, and signs in the way the provider's preset does.
    #[tokio::test]
    async fn a_mail_exchanger_in_a_providers_domain_gives_that_providers_preset() {
        let google = Net::with_mx("firm.example", "aspmx.l.google.com")
            .found("me@firm.example")
            .await
            .expect("google.com is known");
        assert!(matches!(
            google.preset.plan.auth,
            AuthPlan::OAuth {
                issuer: Issuer::Google,
                ..
            }
        ));
        assert_eq!(google.preset.plan.address, "me@firm.example");
        // How the command line words it is `cli::discover::describe`'s; here, what it is.
        let source = google.source.to_string();
        assert!(
            source.contains("Google (mail for the domain goes to aspmx.l.google.com)"),
            "{source}"
        );
        let incoming = format!("{:?}", google.preset.plan.incoming);
        assert!(
            incoming.contains("imap.gmail.com") && incoming.contains("993"),
            "{incoming}"
        );

        for host in ["firm-example.mail.protection.outlook.com", "mx.outlook.com"] {
            let microsoft = Net::with_mx("firm.example", host)
                .found("me@firm.example")
                .await
                .expect(host);
            assert!(
                matches!(
                    microsoft.preset.plan.auth,
                    AuthPlan::OAuth {
                        issuer: Issuer::Microsoft,
                        ..
                    }
                ),
                "{host}"
            );
        }

        // A lookalike is nobody's: the search goes on to the ISPDB, finds nothing, and says so.
        for host in [
            "notgoogle.com",
            "google.com.example.test",
            "evil-outlook.com",
        ] {
            let net = Net::with_mx("firm.example", host);
            let failed = net.found("me@firm.example").await.unwrap_err();
            assert!(
                matches!(failed, Failed::NoServers { .. }),
                "{host}: {failed:?}"
            );
        }
    }

    /// A personal Microsoft mailbox no longer takes a password and recent ones cannot send even with
    /// OAuth, so it is told why instead of being configured; a tenant's mailbox is not.
    #[tokio::test]
    async fn a_personal_microsoft_mailbox_is_refused_and_a_tenants_is_not() {
        for address in [
            "me@outlook.com",
            "me@Hotmail.com",
            "me@live.com",
            "me@msn.com",
        ] {
            let failed = Net::default().found(address).await.unwrap_err();
            let Failed::NoServers { gap, .. } = &failed else {
                panic!("{address}: {failed:?}")
            };
            assert_eq!(*gap, Gap::PersonalMicrosoft, "{address}");
        }
        let tenant = Net::default()
            .found("me@contoso.onmicrosoft.com")
            .await
            .expect("a tenant");
        assert!(matches!(
            tenant.preset.plan.auth,
            AuthPlan::OAuth {
                issuer: Issuer::Microsoft,
                ..
            }
        ));
        assert_eq!(
            tenant.source,
            Source::Provider {
                label: "Microsoft".to_owned()
            }
        );
    }

    /// With a client that signs personal accounts in, a personal address is set up as a personal
    /// account instead of refused; a tenant's is unchanged.
    #[tokio::test]
    async fn a_personal_microsoft_mailbox_is_set_up_once_a_client_takes_it() {
        let mut client = mail_secrets::clients::entry(Issuer::Microsoft, "client-id", None);
        client.endpoints = Some(porter_provider::IssuerEndpoints {
            authorize: porter_core::EndpointUrl::parse("https://login.example/common/authorize")
                .unwrap(),
            token: porter_core::EndpointUrl::parse("https://login.example/common/token").unwrap(),
            revoke: None,
        });
        judge_personal_microsoft_by(Some(crate::OAuthClients::Given(
            mail_secrets::clients::registry_of(vec![client]),
        )));
        let found = Net::default().found("me@outlook.com").await;
        let tenant = Net::default().found("me@contoso.onmicrosoft.com").await;
        judge_personal_microsoft_by(None);
        let found = found.expect("a personal mailbox");
        assert_eq!(
            found.preset.plan.auth,
            presets::microsoft_for(presets::MicrosoftAccount::Personal, "me@outlook.com", now())
                .plan
                .auth
        );
        assert_ne!(
            found.preset.plan.incoming,
            presets::microsoft_for(
                presets::MicrosoftAccount::Organisation,
                "me@outlook.com",
                now()
            )
            .plan
            .incoming
        );
        assert!(tenant.is_ok());
    }

    #[tokio::test]
    async fn the_domains_own_document_is_asked_before_the_ispdb_and_carries_the_address() {
        let mut net = Net::default();
        net.documents.insert(
            "https://autoconfig.example.test/mail/config-v1.1.xml?emailaddress=me%40example.test"
                .to_owned(),
            imap_smtp(
                ("imap.example.test", 993, "SSL"),
                ("smtp.example.test", 465, "SSL"),
                "%EMAILLOCALPART%",
                &["password-cleartext"],
            ),
        );
        let found = net.found("me@example.test").await.expect("found");
        assert_eq!(found.source, Source::Autoconfig);
        assert_eq!(imap_host(&found), "imap.example.test");
        assert!(matches!(
            &found.preset.plan.auth,
            AuthPlan::Password {
                username: Username::LocalPart,
                ..
            }
        ));
        assert_eq!(net.asked.lock().unwrap().len(), 1);
    }

    /// A document whose servers take OAuth2 and no password, at no issuer's host, is
    /// dropped and the search goes on: here to the ISPDB's document for the domain, which has a
    /// password. It is never a password plan at the OAuth-only servers.
    #[tokio::test]
    async fn an_oauth_only_document_at_no_issuer_is_dropped_and_the_search_goes_on() {
        let oauth_only = imap_smtp(
            ("imap.example.test", 993, "SSL"),
            ("smtp.example.test", 465, "SSL"),
            "",
            &["OAuth2"],
        );
        let mut net = Net::default();
        net.documents.insert(
            "https://autoconfig.example.test/mail/config-v1.1.xml?emailaddress=me%40example.test"
                .to_owned(),
            oauth_only.clone(),
        );
        let failed = net.found("me@example.test").await.unwrap_err();
        assert!(
            matches!(
                failed,
                Failed::NoServers {
                    gap: Gap::Nothing,
                    ..
                }
            ),
            "{failed:?}"
        );

        net.documents.insert(
            "https://autoconfig.thunderbird.net/v1.1/example.test".to_owned(),
            imap_smtp(
                ("imap.isp.example.test", 993, "SSL"),
                ("smtp.isp.example.test", 465, "SSL"),
                "%EMAILADDRESS%",
                &["password-cleartext"],
            ),
        );
        let found = net.found("me@example.test").await.expect("the ISPDB's");
        assert_eq!(imap_host(&found), "imap.isp.example.test");
        assert!(matches!(found.preset.plan.auth, AuthPlan::Password { .. }));
    }

    /// OAuth2 through the issuer a document names is taken even when that is the only sign-in
    /// it lists, through the whole search. A document that offers OAuth2 at an issuer's host
    /// with submission on STARTTLS (Microsoft's own) is a miss under the rule, and is taken
    /// after the later sources fail, as the issuer's preset whole.
    #[tokio::test]
    async fn an_oauth_only_document_at_a_known_issuer_is_that_issuers_oauth_plan() {
        let cases = [
            (
                ("imap.gmail.com", 993, "SSL"),
                ("smtp.gmail.com", 465, "SSL"),
                Issuer::Google,
                Some(Source::Autoconfig),
            ),
            (
                ("outlook.office365.com", 993, "SSL"),
                ("smtp.office365.com", 587, "STARTTLS"),
                Issuer::Microsoft,
                None,
            ),
        ];
        for (incoming, outgoing, want, source) in cases {
            let mut net = Net::default();
            net.documents.insert(
                "https://autoconfig.example.test/mail/config-v1.1.xml?emailaddress=me%40example.test"
                    .to_owned(),
                imap_smtp(incoming, outgoing, "", &["OAuth2"]),
            );
            let found = net.found("me@example.test").await.expect("found");
            assert!(
                matches!(
                    found.preset.plan.auth,
                    AuthPlan::OAuth { issuer, .. } if issuer == want
                ),
                "{want:?}"
            );
            if let Some(source) = source {
                assert_eq!(found.source, source);
            }
        }
    }

    /// Through the whole search, a POP3-only domain is found by its document.
    #[tokio::test]
    async fn a_pop3_only_domain_is_found_through_the_search() {
        let mut net = Net::default();
        net.documents.insert(
            "https://autoconfig.example.test/mail/config-v1.1.xml?emailaddress=me%40example.test"
                .to_owned(),
            document(
                &[
                    server(
                        "incoming",
                        "pop3",
                        ("pop.example.test", 995, "SSL"),
                        "",
                        &[],
                    ),
                    server(
                        "outgoing",
                        "smtp",
                        ("smtp.example.test", 465, "SSL"),
                        "",
                        &[],
                    ),
                ]
                .concat(),
            ),
        );
        let found = net.found("me@example.test").await.expect("found");
        assert!(matches!(
            found.preset.plan.incoming,
            Incoming::Pop3 { port: 995, .. }
        ));
    }

    /// A STARTTLS-only document is a miss, the later sources are still asked, and a
    /// domain with nothing else is told why and how to configure it by hand.
    #[tokio::test]
    async fn a_starttls_only_document_is_skipped_and_the_search_goes_on() {
        let starttls = imap_smtp(
            ("imap.example.test", 143, "STARTTLS"),
            ("smtp.example.test", 587, "STARTTLS"),
            "%EMAILADDRESS%",
            &["password-cleartext"],
        );
        let mut net = Net::with_mx("example.test", "mx.isp.example.net");
        net.documents.insert(
            "https://autoconfig.example.test/mail/config-v1.1.xml?emailaddress=me%40example.test"
                .to_owned(),
            starttls,
        );
        let failed = net.found("me@example.test").await.unwrap_err();
        let Failed::NoServers { gap, tried, .. } = &failed else {
            panic!("{failed:?}")
        };
        assert_eq!(*gap, Gap::StartTlsOnly);
        assert!(tried.contains("STARTTLS"), "{tried}");
        let asked = net.asked.lock().unwrap().clone();
        // The ISPDB, and the MX's operator, were still asked.
        assert!(
            asked
                .iter()
                .any(|url| url.contains("autoconfig.thunderbird.net/v1.1/example.test")),
            "{asked:?}"
        );
        assert!(
            asked.iter().any(
                |url| url.contains("autoconfig.thunderbird.net/v1.1/isp.example.net")
                    || url.contains("autoconfig.thunderbird.net/v1.1/example.net")
            ),
            "{asked:?}"
        );

        // A later source with implicit TLS is the one used.
        net.documents.insert(
            "https://autoconfig.thunderbird.net/v1.1/example.test".to_owned(),
            imap_smtp(
                ("imap.isp.example.test", 993, "SSL"),
                ("smtp.isp.example.test", 465, "SSL"),
                "%EMAILADDRESS%",
                &["password-cleartext"],
            ),
        );
        let found = net.found("me@example.test").await.expect("the ISPDB's");
        assert_eq!(imap_host(&found), "imap.isp.example.test");
    }
}
