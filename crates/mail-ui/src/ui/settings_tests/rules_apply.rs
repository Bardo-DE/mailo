//! Applying rules to existing mail asks first when a rule would send.

use crate::ui::fixtures::{acct_account, seeded, thread_like};
use dioxus::prelude::*;
use mail_domain::ThreadId;
use mail_domain::{AfterMatch, Filter, Rule, RuleAction, RuleId, RuleState};
use mail_ui_kit::view::Shell;
use mail_ui_settings::rules::apply::apply_or_ask;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
struct Probe(Vec<ThreadId>, Arc<AtomicBool>);

#[component]
fn Asker() -> Element {
    let Probe(threads, asked) = consume_context::<Probe>();
    let shell = use_signal(Shell::default);
    use_hook(|| {
        apply_or_ask(&mail_ui_kit::live::client(), shell, threads.clone());
        asked.store(shell.peek().applying_rules.is_some(), Ordering::SeqCst);
    });
    rsx! {}
}

fn with_rule(actions: Vec<RuleAction>) -> bool {
    let (store, _dir) = seeded();
    let thread = thread_like(&store, "hi");
    mail_client::testing::Store::put_rule(
        &*store,
        &Rule {
            id: RuleId::generate(),
            account: acct_account(),
            name: "Rule".to_owned(),
            position: 1,
            state: RuleState::Enabled,
            filter: Filter::All,
            actions,
            after: AfterMatch::Continue,
            made: Default::default(),
            reply_days: 4,
        },
    )
    .unwrap();
    let asked = Arc::new(AtomicBool::new(false));
    let mut dom = VirtualDom::new(Asker)
        .with_root_context(crate::ui::fixtures::client(&store))
        .with_root_context(Probe(vec![thread], asked.clone()));
    dom.rebuild_in_place();
    asked.load(Ordering::SeqCst)
}

/// Rules that reply or forward are asked about before they send; the rest apply at once.
#[test]
fn apply_rules_asks_only_when_a_rule_would_send() {
    assert!(
        with_rule(vec![RuleAction::Reply("Away".to_owned())]),
        "a reply asks"
    );
    assert!(
        with_rule(vec![RuleAction::Forward("a@b.test".to_owned())]),
        "a forward asks"
    );
    assert!(!with_rule(vec![RuleAction::Archive]), "an archive does not");
    assert!(
        !with_rule(vec![RuleAction::Notify]),
        "a notification sends nothing"
    );
}
