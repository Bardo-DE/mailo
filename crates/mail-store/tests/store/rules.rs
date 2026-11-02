//! Rules and the vacation reply: kept in both stores alike.
//!
//! Running them is `mail_logic::arrival`'s, over what `mail_core::arrival` reads; its scenarios
//! are `mail-core`'s tests.

use crate::support::*;
use mail_domain::*;
use mail_store::{MemoryStore, Store, StoreError};

fn rule(position: u32, name: &str, filter: Filter, actions: Vec<RuleAction>) -> Rule {
    Rule {
        made: Default::default(),
        reply_days: 4,
        id: RuleId::generate(),
        account: acct_account(),
        name: name.to_owned(),
        position,
        state: RuleState::Enabled,
        filter,
        actions,
        after: AfterMatch::Continue,
    }
}

fn from(s: &str) -> Filter {
    Filter::From(TextMatch::Contains(s.to_owned()))
}

#[test]
fn rules_are_kept_in_order_with_one_name_each_per_account() {
    let sqlite = sqlite();
    let memory = MemoryStore::new();
    let stores: [&dyn Store; 2] = [&sqlite, &memory];
    for store in stores {
        let b = rule(2, "b", from("x"), vec![RuleAction::Star]);
        let a = rule(2, "a", Filter::All, vec![RuleAction::MarkRead]);
        let first = rule(1, "z first", Filter::Nothing, vec![]);
        for r in [&b, &a, &first] {
            store.put_rule(r).unwrap();
        }
        let listed: Vec<String> = store
            .rules(acct_account())
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(listed, ["z first", "a", "b"]);

        // A second rule by an existing name is refused; the same rule saved again is not.
        let clash = rule(3, "a", Filter::All, vec![]);
        assert!(matches!(
            store.put_rule(&clash),
            Err(StoreError::RuleNameTaken(name)) if name == "a"
        ));
        let edited = Rule {
            state: RuleState::Disabled,
            ..a.clone()
        };
        store.put_rule(&edited).unwrap();
        assert_eq!(
            store.rules(acct_account()).unwrap()[1].state,
            RuleState::Disabled
        );
        // Another account may use the name.
        store
            .put_rule(&Rule {
                account: acct_other(),
                ..clash
            })
            .unwrap();
        assert_eq!(store.rules(acct_other()).unwrap().len(), 1);

        store.delete_rule(b.id).unwrap();
        assert!(matches!(
            store.delete_rule(b.id),
            Err(StoreError::NoRule(_))
        ));
        assert_eq!(store.rules(acct_account()).unwrap().len(), 2);
    }
}

#[test]
fn a_vacation_reply_is_kept_replaced_and_cleared() {
    let sqlite = sqlite();
    let memory = MemoryStore::new();
    let stores: [&dyn Store; 2] = [&sqlite, &memory];
    for store in stores {
        assert_eq!(store.vacation(acct_account()).unwrap(), None);
        let away = Vacation {
            account: acct_account(),
            subject: "Away".to_owned(),
            body: "Back on Monday.".to_owned(),
            days: Vacation::DEFAULT_DAYS,
            addresses: vec!["me@example.test".to_owned()],
            from: None,
            during: DateRange {
                from: Some(at(0)),
                to: Some(at(86_400)),
            },
        };
        store
            .put_vacation(acct_account(), Some(&away), at(0))
            .unwrap();
        assert_eq!(store.vacation(acct_account()).unwrap(), Some(away.clone()));
        let longer = Vacation { days: 14, ..away };
        store
            .put_vacation(acct_account(), Some(&longer), at(1))
            .unwrap();
        assert_eq!(store.vacation(acct_account()).unwrap(), Some(longer));
        assert_eq!(store.vacation(acct_other()).unwrap(), None);
        store.put_vacation(acct_account(), None, at(2)).unwrap();
        assert_eq!(store.vacation(acct_account()).unwrap(), None);
    }
}

#[test]
fn a_rule_with_an_action_this_build_does_not_know_still_loads_without_it() {
    let store = sqlite();
    let kept = rule(1, "kept", Filter::All, vec![RuleAction::Star]);
    let newer = rule(2, "newer", Filter::All, vec![RuleAction::Archive]);
    store.put_rule(&kept).unwrap();
    store.put_rule(&newer).unwrap();
    store
        .raw_connection()
        .execute(
            "UPDATE rules SET actions = ?1 WHERE name = 'newer'",
            [r#"[{"kind":"from_the_future","v":"x"},{"kind":"archive"}]"#],
        )
        .unwrap();
    let listed = store.rules(acct_account()).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].actions, vec![RuleAction::Star]);
    assert_eq!(listed[1].actions, vec![RuleAction::Archive]);
}
