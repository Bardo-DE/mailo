//! Which accounts the lists of accounts hold, and what each list does with an account's
//! preferences. Pinned before the accounts became a value read once: the answers here are the
//! ones `sync::configured` gave, one store read at a time.

use crate::support::{caps, now};
use mail_core::offline::{Every, Kept};
use mail_core::sync::{self, due, live};
use mail_domain::id::account_id_from_uuid;
use mail_domain::*;
use mail_store::{SqliteStore, Syncing};
use porter_core::AccountId;
use std::time::Duration;

fn id(n: u128) -> AccountId {
    account_id_from_uuid(uuid::Uuid::from_u128(0x4000_8000_0000_0000_0000 + n))
}

fn plan(address: &str, incoming: Incoming) -> AccountPlan {
    AccountPlan {
        address: address.to_owned(),
        incoming,
        outgoing: Outgoing::Smtp {
            host: "127.0.0.1".to_owned(),
            port: 1,
            tls: Tls::Plaintext,
        },
        auth: AuthPlan::Password {
            username: Username::SameAsAddress,
            sasl: vec![SaslMech::Plain],
        },
        identities: Vec::new(),
    }
}

fn imap() -> Incoming {
    Incoming::Imap {
        host: "127.0.0.1".to_owned(),
        port: 1,
        tls: Tls::Plaintext,
    }
}

fn seed(
    store: &SqliteStore,
    n: u128,
    address: &str,
    incoming: Incoming,
    caps: Option<AccountCaps>,
) {
    let at = now() + chrono::TimeDelta::seconds(n as i64);
    mail_store::testing::seed_account_plan(
        store,
        id(n),
        address,
        &plan(address, incoming),
        Some(at),
    );
    if let Some(caps) = caps {
        mail_store::testing::seed_caps(store, id(n), &caps, now()).unwrap();
    }
}

fn idle() -> AccountCaps {
    AccountCaps {
        watch: WatchMode::Idle,
        ..caps()
    }
}

/// 1 polls, 2 idles, 3 keeps its mail here, 4 has never connected, 5's plan does not read.
fn five() -> SqliteStore {
    let store = mail_store::testing::in_memory();
    seed(&store, 1, "one@example.test", imap(), Some(caps()));
    seed(&store, 2, "two@example.test", imap(), Some(idle()));
    seed(
        &store,
        3,
        "three@example.test",
        Incoming::Local,
        Some(caps()),
    );
    seed(&store, 4, "four@example.test", imap(), None);
    mail_store::testing::seed_account(&store, id(5), "five@example.test");
    store
}

#[test]
fn an_account_that_never_connected_or_whose_plan_does_not_read_is_in_no_list() {
    let store = five();
    let addresses: Vec<String> = sync::addresses(&store)
        .into_iter()
        .map(|(_, a)| a)
        .collect();
    assert_eq!(
        addresses,
        ["one@example.test", "two@example.test", "three@example.test"]
    );
    assert_eq!(sync::local_accounts(&store), [id(3)]);
    let signs_in = sync::auth_by_account(&store).unwrap();
    let mut keys: Vec<_> = signs_in.keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["one@example.test", "three@example.test", "two@example.test"]
    );
}

#[test]
fn only_accounts_with_a_server_are_given_an_interval_and_the_person_s_choice_is_used() {
    let store = five();
    let secs = Duration::from_secs;
    assert_eq!(
        due::intervals_with(&store, &Kept::default()),
        [(id(1), secs(300)), (id(2), due::IDLE)]
    );
    let kept = Kept::default()
        .with_every(id(1), Every::Minutes(15))
        .with_every(id(2), Every::Manually);
    assert_eq!(
        due::intervals_with(&store, &kept),
        [(id(1), secs(900)), (id(2), due::MANUALLY)]
    );
}

#[test]
fn a_paused_account_is_in_every_list_of_accounts_but_none_of_what_to_sync() {
    let store = five();
    store.set_account_syncing(id(1), Syncing::Paused).unwrap();
    assert!(sync::paused(&store, id(1)));
    assert_eq!(sync::addresses(&store).len(), 3);
    assert_eq!(
        due::intervals_with(&store, &Kept::default()),
        [(id(2), due::IDLE)]
    );
    // The shortest interval of those that remain.
    assert_eq!(sync::poll_interval(&store), due::IDLE);
}

#[test]
fn the_poll_interval_is_the_shortest_any_unpaused_account_asks_for() {
    let store = five();
    assert_eq!(sync::poll_interval(&store), Duration::from_secs(300));
    let empty = mail_store::testing::in_memory();
    assert_eq!(sync::poll_interval(&empty), Duration::from_secs(300));
}

#[test]
fn an_idling_account_is_waited_on_unless_the_person_chose_an_interval_or_paused_it() {
    let store = five();
    assert_eq!(live::pushing_with(&store, &Kept::default()), [id(2)]);
    let kept = Kept::default().with_every(id(2), Every::Minutes(5));
    assert!(live::pushing_with(&store, &kept).is_empty());
    store.set_account_syncing(id(2), Syncing::Paused).unwrap();
    assert!(live::pushing_with(&store, &Kept::default()).is_empty());
}
