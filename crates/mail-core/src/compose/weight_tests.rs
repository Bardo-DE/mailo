use super::{Measure, Said, SendLimit};

const MB: u64 = 1024 * 1024;

#[test]
fn an_accounts_limit_is_what_its_server_said_or_graphs_or_the_usual() {
    use mail_domain::Outgoing;
    use mail_domain::presets::{self, Manual};

    let store = mail_store::testing::in_memory();
    let now = mail_domain::testing::now();
    let preset = presets::manual(
        "ada@example.test",
        &Manual {
            imap_host: "imap.example.test".to_owned(),
            imap_port: 993,
            smtp_host: "smtp.example.test".to_owned(),
            smtp_port: 465,
            login: None,
        },
        now,
    );
    let seed = |address: &str, outgoing: Outgoing, said: Option<u64>| {
        let id = mail_domain::id::new_account_id();
        let plan = mail_domain::AccountPlan {
            outgoing,
            ..preset.plan.clone()
        };
        mail_store::testing::seed_account_plan(&store, id.clone(), address, &plan, None);
        mail_store::testing::seed_caps(&store, id.clone(), &preset.expected_caps, now)
            .unwrap_or_else(|why| panic!("caps: {why}"));
        store
            .put_send_limit(id.clone(), said)
            .unwrap_or_else(|why| panic!("limit: {why}"));
        id
    };
    let smtp = || Outgoing::Smtp {
        host: "smtp.example.test".to_owned(),
        port: 465,
        tls: mail_domain::Tls::Implicit,
    };
    let server = |octets| SendLimit {
        octets,
        measure: Measure::Wire,
        said: Said::Server,
    };
    let graph = SendLimit {
        octets: mail_runtime::graph::MAX_MESSAGE,
        measure: Measure::Content,
        said: Said::Graph,
    };
    let cases = [
        (
            "smtp that said SIZE",
            seed("a@x.test", smtp(), Some(10 * MB)),
            server(10 * MB),
        ),
        (
            "smtp not yet heard",
            seed("b@x.test", smtp(), None),
            SendLimit::USUAL,
        ),
        (
            "jmap that said",
            seed("c@x.test", Outgoing::Jmap, Some(50 * MB)),
            server(50 * MB),
        ),
        // Graph says nothing, and what a stray number says is not what Graph takes.
        ("graph", seed("d@x.test", Outgoing::Graph, Some(MB)), graph),
        (
            "local",
            seed("e@x.test", Outgoing::Nowhere, None),
            SendLimit::USUAL,
        ),
        (
            "an account not here",
            mail_domain::id::new_account_id(),
            SendLimit::USUAL,
        ),
    ];
    for (case, account, expected) in cases {
        assert_eq!(super::send_limit(&store, &account), expected, "{case}");
    }
}

/// Graph's `sendMail` cannot ask for delivery reports, so the composer offers the choice for every
/// account but a Graph one.
#[test]
fn delivery_reports_are_offered_for_every_account_but_graph() {
    use mail_domain::Outgoing;
    use mail_domain::presets::{self, Manual};

    let store = mail_store::testing::in_memory();
    let now = mail_domain::testing::now();
    let preset = presets::manual(
        "ada@example.test",
        &Manual {
            imap_host: "imap.example.test".to_owned(),
            imap_port: 993,
            smtp_host: "smtp.example.test".to_owned(),
            smtp_port: 465,
            login: None,
        },
        now,
    );
    let seed = |address: &str, outgoing: Outgoing| {
        let id = mail_domain::id::new_account_id();
        let plan = mail_domain::AccountPlan {
            outgoing,
            ..preset.plan.clone()
        };
        mail_store::testing::seed_account_plan(&store, id.clone(), address, &plan, None);
        id
    };
    let graph = seed("g@x.test", Outgoing::Graph);
    let jmap = seed("j@x.test", Outgoing::Jmap);
    let smtp = seed("s@x.test", preset.plan.outgoing.clone());
    assert!(!super::can_ask_delivery_report(&store, &graph));
    assert!(super::can_ask_delivery_report(&store, &jmap));
    assert!(super::can_ask_delivery_report(&store, &smtp));
}

