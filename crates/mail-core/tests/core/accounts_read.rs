//! Which accounts the lists of accounts hold, and what each list does with an account's
//! preferences. The accounts are one value read once (`gather::accounts`); what each list
//! of them holds was pinned before that, from `sync::configured` read one store call at a time.

use crate::support::{caps, now};
use mail_core::offline::{Every, Keep, Kept, Remove, Window};
use mail_core::sync::{self, due, live};
use mail_domain::id::account_id_from_uuid;
use mail_domain::*;
use mail_logic::accounts::{Accounts, Settings};
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

fn read(store: &SqliteStore, kept: Kept) -> Accounts {
    let settings = Settings {
        kept,
        ..Settings::default()
    };
    mail_core::gather::accounts(store, &settings).unwrap()
}

#[test]
fn an_account_whose_plan_does_not_read_is_in_no_list_and_one_that_never_connected_has_no_caps() {
    let store = five();
    let accounts = read(&store, Kept::default());
    let addresses: Vec<String> = accounts.addresses().into_iter().map(|(_, a)| a).collect();
    assert_eq!(
        addresses,
        [
            "one@example.test",
            "two@example.test",
            "three@example.test",
            "four@example.test"
        ]
    );
    assert_eq!(accounts.local(), [id(3)]);
    assert!(accounts.caps_of(&id(4)).is_none());
    assert!(accounts.caps_of(&id(1)).is_some());
    assert!(matches!(
        accounts.auth_of(&id(1)),
        Some(AuthPlan::Password { .. })
    ));
    assert!(accounts.auth_of(&id(5)).is_none());
}

#[test]
fn only_accounts_with_a_server_are_given_an_interval_and_the_person_s_choice_is_used() {
    let store = five();
    let secs = Duration::from_secs;
    assert_eq!(
        due::intervals(&read(&store, Kept::default())),
        [(id(1), secs(300)), (id(2), due::IDLE)]
    );
    let kept = Kept::default()
        .with_every(id(1), Every::Minutes(15))
        .with_every(id(2), Every::Manually);
    assert_eq!(
        due::intervals(&read(&store, kept)),
        [(id(1), secs(900)), (id(2), due::MANUALLY)]
    );
}

#[test]
fn a_paused_account_is_in_every_list_of_accounts_but_none_of_what_to_sync() {
    let store = five();
    store.set_account_syncing(id(1), Syncing::Paused).unwrap();
    let accounts = read(&store, Kept::default());
    assert!(accounts.get(&id(1)).unwrap().paused);
    assert_eq!(accounts.addresses().len(), 4);
    assert_eq!(due::intervals(&accounts), [(id(2), due::IDLE)]);
    // The shortest interval of those that remain.
    assert_eq!(sync::poll_interval(&accounts), due::IDLE);
}

#[test]
fn the_poll_interval_is_the_shortest_any_unpaused_account_asks_for() {
    let store = five();
    assert_eq!(
        sync::poll_interval(&read(&store, Kept::default())),
        Duration::from_secs(300)
    );
    let empty = mail_store::testing::in_memory();
    assert_eq!(
        sync::poll_interval(&read(&empty, Kept::default())),
        Duration::from_secs(300)
    );
}

#[test]
fn an_idling_account_is_waited_on_unless_the_person_chose_an_interval_or_paused_it() {
    let store = five();
    let pushing = |kept| live::pushing(&store, &read(&store, kept));
    assert_eq!(pushing(Kept::default()), [id(2)]);
    assert!(pushing(Kept::default().with_every(id(2), Every::Minutes(5))).is_empty());
    store.set_account_syncing(id(2), Syncing::Paused).unwrap();
    assert!(pushing(Kept::default()).is_empty());
}

#[test]
fn the_person_s_choices_are_on_each_account_not_overlaid_later() {
    let store = five();
    let kept = Kept::default()
        .with(id(1), Keep::Everything)
        .with_window(id(1), Window::Days(30))
        .with_remove(id(2), Remove::WhenDeleted);
    let accounts = read(&store, kept);
    let one = accounts.get(&id(1)).unwrap().prefs;
    assert_eq!(
        (one.keep, one.window, one.remove),
        (Keep::Everything, Window::Days(30), Remove::Never)
    );
    assert_eq!(
        accounts.get(&id(2)).unwrap().prefs.remove,
        Remove::WhenDeleted
    );
    // What a pass is run with is what the value says: a folder opened on demand fetches
    // attachments for an account the person keeps in full.
    assert_eq!(
        sync::Configured::of(accounts.get(&id(1)).unwrap())
            .unwrap()
            .keep,
        Keep::Everything
    );
}

#[test]
fn removing_an_account_drops_its_syncing_choices_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    mail_core::offline::change(dir.path(), |kept| {
        kept.with(id(1), Keep::Everything)
            .with_every(id(1), Every::Manually)
    })
    .unwrap();
    let store = std::sync::Arc::new(five());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mail = mail_core::Mail::new(store, runtime.handle().clone())
        .with_secrets(std::sync::Arc::new(porter_secrets::MemorySecrets::default()))
        .with_config(Some(dir.path().to_path_buf()));
    runtime.block_on(mail.accounts().remove(id(1))).unwrap();
    let after = mail_core::offline::load(dir.path());
    assert_eq!(after, Kept::default(), "{after:?}");
}

#[test]
fn an_account_that_never_connected_is_still_told_how_to_sign_in() {
    use mail_logic::remedy::{Remedy, SignInWith};
    let accounts = read(&five(), Kept::default());
    let Remedy::SignIn { with, .. } = sync::sign_in_remedy(&accounts, "four@example.test") else {
        panic!("a sign-in remedy was expected");
    };
    assert_ne!(with, SignInWith::Unknown);
    // An address nothing knows is asked to sign in without saying how.
    let Remedy::SignIn { with, .. } = sync::sign_in_remedy(&accounts, "nobody@example.test") else {
        panic!("a sign-in remedy was expected");
    };
    assert_eq!(with, SignInWith::Unknown);
}

/// A call for one account whose plan does not read says so, where it used to say there is no such
/// account; one nothing has connected for, and one nobody stored, are still gone.
#[test]
fn a_call_for_an_account_whose_plan_does_not_read_is_told_why() {
    let store = std::sync::Arc::new(five());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mail = mail_core::Mail::new(store, runtime.handle().clone())
        .with_secrets(std::sync::Arc::new(porter_secrets::MemorySecrets::default()));
    let sync = mail.sync();
    let broken = runtime.block_on(sync.quota(id(5)));
    assert!(
        matches!(&broken, Err(mail_core::CoreError::PlanUnreadable { address, .. })
            if address == "five@example.test"),
        "{broken:?}"
    );
    let broken = runtime.block_on(sync.drain(id(5)));
    assert!(
        matches!(broken, Err(mail_core::CoreError::PlanUnreadable { .. })),
        "{broken:?}"
    );
    // Not connected yet, and not there at all, are the accounts they were.
    for other in [id(4), id(9)] {
        let gone = runtime.block_on(sync.quota(other));
        assert!(matches!(gone, Err(mail_core::CoreError::AccountGone)), "{gone:?}");
    }
}
