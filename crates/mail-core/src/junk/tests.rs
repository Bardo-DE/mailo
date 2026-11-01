use super::*;
use crate::support_tests::{account, at, caps};
use mail_domain::testing::message;
use mail_domain::{Address, Change, ChangeId, Patch};
use mail_logic::junk::{ENOUGH, Heed, JUNK_LABEL, Treat};

/// The policy the person's settings give by default: the filter switched on.
fn on() -> Policy {
    Policy {
        filtering: Filtering::On,
        ..Policy::default()
    }
}

fn stored(store: &SqliteStore, mut message: Message, raw: &[u8]) -> Message {
    let blob = store.blobs().put(raw).unwrap();
    if let Body::Present { raw, .. } = &mut message.body {
        *raw = blob;
    }
    store
        .apply(
            account(),
            &Patch {
                id: ChangeId::generate(),
                changes: vec![Change::MessageUpsert(Box::new(message.clone()))],
            },
        )
        .unwrap();
    message
}

const SPAMMY: &str = "winner claim your free prize now lottery cash bonus";
const HAMMY: &str = "minutes from the meeting about the quarterly budget review";

/// A store whose filter has learned enough: spam about prizes, good mail about meetings.
fn trained() -> SqliteStore {
    trained_as(Some("Ada Lovelace"))
}

/// [`trained`], for a user whose identity has `name`.
fn trained_as(name: Option<&str>) -> SqliteStore {
    let store = mail_store::testing::in_memory();
    mail_store::testing::seed_account(&store, account(), "ada@example.test");
    mail_store::testing::seed_default_identity(&store, account(), "ada@example.test", name);
    for n in 0..ENOUGH as u128 {
        let spam = stored(
            &store,
            message(n, "promo@prizes.example", "You won", SPAMMY),
            b"",
        );
        let ham = stored(
            &store,
            message(100 + n, "bob@work.example", "Minutes", HAMMY),
            b"",
        );
        learn(&store, &[spam], Judged::Spam).unwrap();
        learn(&store, &[ham], Judged::Ham).unwrap();
    }
    store
}

#[test]
fn learning_a_message_twice_counts_it_once_and_changing_ones_mind_moves_it() {
    let store = trained();
    let counts = |store: &SqliteStore| store.junk_counts(account(), &[]).unwrap();
    let before = counts(&store);
    let again = message(0, "promo@prizes.example", "You won", SPAMMY);
    assert_eq!(
        learn(&store, std::slice::from_ref(&again), Judged::Spam).unwrap(),
        0
    );
    assert_eq!(counts(&store), before);
    learn(&store, &[again], Judged::Ham).unwrap();
    let after = counts(&store);
    assert_eq!((after.spam, after.ham), (before.spam - 1, before.ham + 1));
}

/// New mail in the inbox: junk moves to Spam, or is marked; a sender in Contacts, one written
/// to, and mail to the user's full name are spared; the server's own word is taken; mail the
/// server filed in Junk is learned from; nothing is judged with the filter off.
#[test]
fn new_mail_is_judged_as_the_policy_says() {
    struct Case {
        name: &'static str,
        policy: Policy,
        from: &'static str,
        to_name: Option<&'static str>,
        text: &'static str,
        raw: &'static [u8],
        arrives_in: MailboxRole,
        contact: Option<mail_store::Origin>,
        want_junk: bool,
        want_mailbox: MailboxRole,
        want_label: bool,
    }
    let base = || Case {
        name: "",
        policy: on(),
        from: "promo@prizes.example",
        to_name: None,
        text: SPAMMY,
        raw: b"Subject: You won\r\n\r\n",
        arrives_in: MailboxRole::Inbox,
        contact: None,
        want_junk: true,
        want_mailbox: MailboxRole::Spam,
        want_label: false,
    };
    let cases = vec![
        Case {
            name: "junk moves to Spam",
            ..base()
        },
        Case {
            name: "good mail stays",
            from: "carol@work.example",
            text: HAMMY,
            want_junk: false,
            want_mailbox: MailboxRole::Inbox,
            ..base()
        },
        Case {
            name: "marked, not moved",
            policy: Policy {
                treat: Treat::Mark,
                ..on()
            },
            want_mailbox: MailboxRole::Inbox,
            want_label: true,
            ..base()
        },
        Case {
            name: "a sender in Contacts is spared",
            contact: Some(mail_store::Origin::Manual),
            want_junk: false,
            want_mailbox: MailboxRole::Inbox,
            ..base()
        },
        Case {
            name: "unless Contacts are not heeded",
            policy: Policy {
                contacts: Heed::No,
                ..on()
            },
            contact: Some(mail_store::Origin::Manual),
            ..base()
        },
        Case {
            name: "mail to the user's full name is spared",
            to_name: Some("Ada Lovelace"),
            want_junk: false,
            want_mailbox: MailboxRole::Inbox,
            ..base()
        },
        Case {
            name: "the server's word is taken",
            from: "carol@work.example",
            text: HAMMY,
            raw: b"X-Spam-Flag: YES\r\n\r\n",
            ..base()
        },
        Case {
            name: "unless it is not heeded",
            policy: Policy {
                server_says: Heed::No,
                ..on()
            },
            from: "carol@work.example",
            text: HAMMY,
            raw: b"X-Spam-Flag: YES\r\n\r\n",
            want_junk: false,
            want_mailbox: MailboxRole::Inbox,
            ..base()
        },
        Case {
            name: "the server's Junk is learned from, not judged",
            arrives_in: MailboxRole::Spam,
            want_junk: false,
            want_mailbox: MailboxRole::Spam,
            ..base()
        },
        Case {
            name: "off judges nothing",
            policy: Policy {
                filtering: Filtering::Off,
                ..on()
            },
            want_junk: false,
            want_mailbox: MailboxRole::Inbox,
            ..base()
        },
    ];
    for case in cases {
        let store = trained();
        if let Some(origin) = &case.contact {
            store.put_contact(case.from, None, origin).unwrap();
        }
        let mut arriving = message(500, case.from, "Hello", case.text);
        arriving.mailbox = case.arrives_in;
        if let Some(name) = case.to_name {
            arriving.to = vec![Address::named(name, "ada@example.test")];
        } else {
            arriving.to = vec![Address::new("ada@example.test")];
        }
        let arriving = stored(&store, arriving, case.raw);
        let sifted = sift(
            &store,
            account(),
            &caps(),
            &[arriving.id],
            &case.policy,
            at(1000),
        )
        .unwrap();
        assert_eq!(
            sifted.junk.contains(&arriving.id),
            case.want_junk,
            "{}",
            case.name
        );
        let now = store.message(arriving.id).unwrap();
        assert_eq!(now.mailbox, case.want_mailbox, "{}", case.name);
        let junk_label = store
            .labels(account())
            .unwrap()
            .into_iter()
            .find(|label| label.name == JUNK_LABEL)
            .map(|label| label.id);
        assert_eq!(
            junk_label.is_some_and(|label| now.labels.contains(&label)),
            case.want_label,
            "{}",
            case.name
        );
    }
}

#[test]
fn a_handle_nobody_set_up_does_not_judge() {
    assert_eq!(Policy::default().filtering, Filtering::Off);
    let store = trained();
    let arriving = stored(
        &store,
        message(500, "promo@prizes.example", "Hello", SPAMMY),
        b"",
    );
    let sifted = sift(
        &store,
        account(),
        &caps(),
        &[arriving.id],
        &Policy::default(),
        at(1000),
    )
    .unwrap();
    assert!(sifted.junk.is_empty());
    assert_eq!(
        store.message(arriving.id).unwrap().mailbox,
        MailboxRole::Inbox
    );
}

#[test]
fn an_identity_with_an_empty_name_spares_nobody() {
    let store = trained_as(Some(""));
    let mut arriving = message(500, "promo@prizes.example", "Hello", SPAMMY);
    arriving.to = vec![Address::named("", "ada@example.test")];
    let arriving = stored(&store, arriving, b"");
    let sifted = sift(&store, account(), &caps(), &[arriving.id], &on(), at(1000)).unwrap();
    assert!(
        sifted.junk.contains(&arriving.id),
        "spared by an empty name"
    );
}

#[test]
fn mail_whose_body_has_not_come_is_not_judged() {
    let store = trained();
    let mut arriving = message(500, "promo@prizes.example", "winner claim prize", SPAMMY);
    arriving.body = Body::Absent;
    store
        .apply(
            account(),
            &Patch {
                id: ChangeId::generate(),
                changes: vec![Change::MessageUpsert(Box::new(arriving.clone()))],
            },
        )
        .unwrap();
    let sifted = sift(&store, account(), &caps(), &[arriving.id], &on(), at(1000)).unwrap();
    assert!(sifted.junk.is_empty());
}

#[test]
fn undoing_spam_and_not_spam_leaves_the_counts_as_they_were() {
    let store = trained();
    let counts = || {
        let c = store.junk_counts(account(), &[]).unwrap();
        (c.spam, c.ham)
    };
    let before = counts();
    let arriving = stored(
        &store,
        message(500, "promo@prizes.example", "Hello", SPAMMY),
        b"",
    );
    let spam = crate::act::perform(&store, arriving.thread, Op::Spam).unwrap();
    assert_eq!(counts(), (before.0 + 1, before.1));
    assert!(crate::act::take_back(&store, &spam));
    assert_eq!(counts(), before, "Spam, undone");

    // Not Spam: from the Spam folder back out.
    crate::act::perform(&store, arriving.thread, Op::Spam).unwrap();
    let restored = crate::act::perform(&store, arriving.thread, Op::Restore).unwrap();
    // Spam taught spam, then Not Spam turned it to ham.
    assert_eq!(counts(), (before.0, before.1 + 1));
    assert!(crate::act::take_back(&store, &restored));
    assert_eq!(
        counts(),
        (before.0, before.1),
        "ham forgotten, the spam it turned stays gone"
    );
}

#[test]
fn taking_the_junk_label_off_teaches_not_spam() {
    let store = trained();
    let counts = || store.junk_counts(account(), &[]).unwrap().ham;
    let arriving = stored(
        &store,
        message(500, "promo@prizes.example", "Hello", SPAMMY),
        b"",
    );
    let label = mail_store::rules::label_named(
        &store,
        account(),
        JUNK_LABEL,
        mail_domain::LabelOrigin::User,
    )
    .unwrap();
    crate::act::perform(&store, arriving.thread, Op::Label(label, Membership::In)).unwrap();
    let before = counts();
    let off =
        crate::act::perform(&store, arriving.thread, Op::Label(label, Membership::Out)).unwrap();
    assert_eq!(counts(), before + 1);
    assert!(crate::act::take_back(&store, &off));
    assert_eq!(counts(), before);
}
