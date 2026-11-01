//! A JMAP account's arrivals meet the user's rules, muted conversations and rule replies, as an
//! IMAP account's do.
//!
//! The JMAP pass used to call only the junk filter between its bodies and its outbox, so on a
//! JMAP account a filing rule, a muting rule and a rule's reply never ran, though the engine's
//! documentation said they did. Each test here runs a whole pass (`sync::run`'s) against the fake
//! JMAP server the runtime's own tests use.

// The runtime's tests use more of the fake than these do.
#[allow(dead_code)]
#[path = "../../../mail-runtime/tests/jmap_fake/mod.rs"]
mod jmap_fake;

use crate::support::account;
use chrono::{DateTime, TimeZone, Utc};
use jmap_fake::{Fake, PASSWORD, USER};
use mail_core::sync::report::{Hooks, PassEnd};
use mail_domain::*;
use mail_secrets::{AccountSecrets, ClientRegistry};
use mail_store::{SqliteStore, Store};
use porter_core::{Credential, SecretKey, SecretPurpose, SecretText};
use porter_secrets::MemorySecrets;
use std::sync::Arc;

/// Just after the mail below was dated, so it is fresh enough for a rule to answer.
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap()
}

/// The fake, served from a runtime of its own so that the pass under test can run its own.
fn start_fake() -> Fake {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            tx.send(jmap_fake::start().await).unwrap();
            std::future::pending::<()>().await;
        });
    });
    rx.recv().unwrap()
}

struct Setup {
    fake: Fake,
    store: Arc<SqliteStore>,
    secrets: Arc<MemorySecrets>,
}

fn setup() -> Setup {
    let fake = start_fake();
    let store = Arc::new(mail_store::testing::in_memory());
    let preset = presets::jmap(USER, &fake.session_url(), HttpAuth::Basic);
    mail_store::testing::seed_account_plan(&store, account(), USER, &preset.plan, Some(now()));
    mail_store::testing::seed_caps(&store, account(), &crate::support::caps(), now()).unwrap();
    mail_store::testing::seed_default_identity(&store, account(), USER, None);
    let secrets = Arc::new(MemorySecrets::default());
    for purpose in [
        SecretPurpose::IncomingPassword,
        SecretPurpose::OutgoingPassword,
    ] {
        mail_secrets::block_on(secrets.put(
            &SecretKey {
                account: account(),
                purpose,
            },
            &Credential::Password(SecretText::new(PASSWORD.to_owned())),
        ))
        .unwrap();
    }
    Setup {
        fake,
        store,
        secrets,
    }
}

impl Setup {
    /// One pass over the account, as the program runs it.
    fn pass(&self) -> mail_core::sync::report::AccountReport {
        let secrets: Arc<dyn AccountSecrets> = self.secrets.clone();
        let ends = crate::blocking::run_with(
            self.store.clone(),
            secrets,
            &ClientRegistry::default(),
            now(),
            Hooks::default(),
        )
        .expect("the run");
        let [PassEnd::Finished(report)] = ends.as_slice() else {
            panic!("one account, one finished pass: {ends:?}");
        };
        assert!(
            report.trouble.is_empty(),
            "a clean pass: {:?}",
            report.trouble
        );
        report.clone()
    }

    fn held(&self, n: u32) -> Message {
        let id = self
            .store
            .message_by_rfc_id(account(), &format!("m{n}@example.test"))
            .unwrap()
            .unwrap_or_else(|| panic!("message {n} is not here"));
        self.store.message(id).unwrap()
    }

    /// The mailboxes the server holds email `id` in.
    fn on_server(&self, id: &str) -> Vec<String> {
        self.fake
            .state
            .lock()
            .unwrap()
            .email(id)
            .expect("the server holds it")
            .mailboxes
            .clone()
    }

    fn rule(&self, name: &str, filter: Filter, actions: Vec<RuleAction>) {
        self.store
            .put_rule(&Rule {
                id: RuleId::generate(),
                account: account(),
                name: name.to_owned(),
                position: 1,
                state: RuleState::Enabled,
                filter,
                actions,
                after: AfterMatch::Continue,
                made: now() - chrono::TimeDelta::days(30),
                reply_days: 4,
            })
            .unwrap();
    }
}

/// An email as the server hands it over, from Ada, with a machine-free `Return-Path`.
fn raw(n: u32, subject: &str, extra: &str) -> Vec<u8> {
    format!(
        "Return-Path: <ada@example.test>\r\nFrom: Ada <ada@example.test>\r\nTo: {USER}\r\n\
         Subject: {subject}\r\nMessage-ID: <m{n}@example.test>\r\n{extra}\
         Date: Tue, 01 Sep 2026 09:0{n}:00 +0000\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\r\nabout pelicans\r\n"
    )
    .into_bytes()
}

fn ada() -> Filter {
    Filter::From(TextMatch::Contains("ada@example.test".to_owned()))
}

#[test]
#[ignore = "a bug until arrivals are folded in mail-core: the JMAP pass runs only the junk filter"]
fn a_filing_rule_runs_on_a_jmap_account_and_the_server_hears_of_it_in_the_same_pass() {
    let s = setup();
    s.rule(
        "Pelicans",
        ada(),
        vec![
            RuleAction::Label("Birds".to_owned()),
            RuleAction::MarkRead,
            RuleAction::Archive,
        ],
    );
    let id = s.fake.state.lock().unwrap().deliver(
        &raw(1, "Hello", ""),
        &["mbI"],
        &[],
        "2026-09-01T09:01:00Z",
    );

    let report = s.pass();

    let hello = s.held(1);
    assert_eq!(hello.mailbox, MailboxRole::Archive, "the rule filed it");
    assert_eq!(hello.read, ReadState::Read);
    assert_eq!(report.counts.ruled.len(), 1, "and the pass said so");
    let there = s.on_server(&id);
    assert!(
        !there.contains(&"mbI".to_owned()),
        "the server was told in the same pass: {there:?}"
    );
}

#[test]
#[ignore = "a bug until arrivals are folded in mail-core: the JMAP pass runs only the junk filter"]
fn a_muted_conversations_reply_arrives_read_and_archived_on_a_jmap_account() {
    let s = setup();
    let first = s.fake.state.lock().unwrap().deliver(
        &raw(1, "Hello", ""),
        &["mbI"],
        &[],
        "2026-09-01T09:01:00Z",
    );
    s.pass();
    let thread = s.held(1).thread;
    assert!(mail_core::act::perform(&s.store, thread, Op::SetMute(Mute::Muted)).is_some());

    let reply = s.fake.state.lock().unwrap().deliver(
        &raw(
            2,
            "Re: Hello",
            "In-Reply-To: <m1@example.test>\r\nReferences: <m1@example.test>\r\n",
        ),
        &["mbI"],
        &[],
        "2026-09-01T09:02:00Z",
    );
    s.pass();

    let (hello, replied) = (s.held(1), s.held(2));
    assert_eq!(replied.thread, thread, "it is a reply in the conversation");
    assert_eq!(hello.mailbox, MailboxRole::Inbox, "what was here stays");
    assert_eq!(replied.mailbox, MailboxRole::Archive);
    assert_eq!(replied.read, ReadState::Read);
    assert!(!s.on_server(&reply).contains(&"mbI".to_owned()));
    assert!(s.on_server(&first).contains(&"mbI".to_owned()));
}

#[test]
#[ignore = "a bug until arrivals are folded in mail-core: the JMAP pass runs only the junk filter"]
fn a_rules_reply_is_sent_by_a_jmap_account_in_the_pass_that_brought_the_mail() {
    let s = setup();
    let kept =
        mail_core::compose::draft_new(&s.store, account(), &[], "", "Back soon.", now()).unwrap();
    mail_core::template::save(&s.store, kept.id, "Away", now()).unwrap();
    s.rule("Away", ada(), vec![RuleAction::Reply("Away".to_owned())]);
    s.fake.state.lock().unwrap().deliver(
        &raw(1, "Hello", ""),
        &["mbI"],
        &[],
        "2026-09-01T09:01:00Z",
    );

    let report = s.pass();

    assert_eq!(
        report.counts.submitted, 1,
        "the reply left in the same pass"
    );
    let submitted = s.fake.state.lock().unwrap().submitted.clone();
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0].rcpt_to, vec!["ada@example.test".to_owned()]);
}
