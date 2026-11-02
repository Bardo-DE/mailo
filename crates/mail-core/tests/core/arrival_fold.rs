//! The arrival fold, held to the way arrival was done before it: one operation at a time, each
//! written to the store before the next was decided, every question asked of the store again.
//!
//! [`sequentially`] is that way, kept here as the oracle: the store's own rules path as it stood
//! (re-read the message after each operation, ask the store where a message is listed, make a
//! label when a rule names one that is not there, queue and write each operation on its own).
//! Random rules over random arrivals, some in muted conversations, are run through it and through
//! [`mail_core::arrival::arrive`] on identical stores, and everything either could be seen to do
//! is compared.

use super::arrival::*;
use crate::support::account;
use chrono::{DateTime, Utc};
use mail_domain::rule::{self, one_message};
use mail_domain::*;
use mail_store::{SqliteStore, Store, StoreError};
use proptest::prelude::*;

// ---------------------------------------------------------------------------------------
// The oracle
// ---------------------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq)]
struct Ran {
    acted: Vec<(MessageId, Vec<String>)>,
    queued: usize,
    examined: usize,
    muted: Vec<MessageId>,
    left: Vec<(MessageId, RuleAction)>,
}

fn sequentially(
    store: &SqliteStore,
    caps: &AccountCaps,
    arrived: &[MessageId],
    now: DateTime<Utc>,
) -> Ran {
    let rules = store.rules(account()).unwrap();
    let ordered = rule::ordered(&rules);
    let mut ran = Ran::default();
    for id in arrived {
        let message = match store.message(*id) {
            Ok(message) => message,
            Err(StoreError::NoMessage(_)) => continue,
            Err(e) => panic!("{e}"),
        };
        if message.account != account() || message.mailbox != MailboxRole::Inbox {
            continue;
        }
        if !ordered.is_empty() {
            rules_over(store, caps, &ordered, message, now, &mut ran);
        }
        muted(store, caps, *id, now, &mut ran);
    }
    ran
}

fn muted(
    store: &SqliteStore,
    caps: &AccountCaps,
    id: MessageId,
    now: DateTime<Utc>,
    ran: &mut Ran,
) {
    let Ok(message) = store.message(id) else {
        return;
    };
    if store.thread(message.thread).unwrap().summary.mute != Mute::Muted {
        return;
    }
    ran.queued += perform(store, caps, &Op::SetRead(ReadState::Read), &message, now);
    let message = store.message(id).unwrap();
    if message.mailbox == MailboxRole::Inbox {
        ran.queued += perform(store, caps, &Op::Archive, &message, now);
    }
    ran.muted.push(id);
}

fn rules_over(
    store: &SqliteStore,
    caps: &AccountCaps,
    rules: &[&Rule],
    mut message: Message,
    now: DateTime<Utc>,
    ran: &mut Ran,
) {
    ran.examined += 1;
    let mut fired = Vec::new();
    let vips: Vec<String> = store
        .vips()
        .unwrap()
        .into_iter()
        .map(|vip| vip.address)
        .collect();
    for rule in rules {
        let summary = one_message(&message);
        let corpus = match &message.body {
            Body::Present { text, .. } => text.as_deref(),
            Body::Absent => None,
        };
        let folders = store.placed(message.id).unwrap();
        let sizes = [mail_domain::filter::message_size(&message)];
        let ctx = MatchCtx {
            summary: &summary,
            corpus,
            folders: &folders,
            sizes: &sizes,
            cc: &message.cc,
            vips: &vips,
            now,
        };
        if !rule.fits(&ctx) {
            continue;
        }
        fired.push(rule.name.clone());
        for action in &rule.actions {
            if action.speaks() {
                // A message a rule asked to hear of is said once (`speak::carry_out` did that
                // when it was handed the lot; the fold hands over each once).
                let again = *action == RuleAction::Notify
                    && ran.left.contains(&(message.id, RuleAction::Notify));
                if !again {
                    ran.left.push((message.id, action.clone()));
                }
                continue;
            }
            let op = match action {
                RuleAction::Label(name) => {
                    Op::Label(label_named(store, name, LabelOrigin::User), Membership::In)
                }
                RuleAction::File(path) => Op::File(label_named(store, path, LabelOrigin::Provider)),
                RuleAction::Archive => Op::Archive,
                RuleAction::Trash => Op::Trash,
                RuleAction::Spam => Op::Spam,
                RuleAction::MarkRead => Op::SetRead(ReadState::Read),
                RuleAction::Star => Op::SetStar(Star::Starred),
                RuleAction::MarkUnread => Op::SetRead(ReadState::Unread),
                _ => continue,
            };
            ran.queued += perform(store, caps, &op, &message, now);
            message = store.message(message.id).unwrap();
        }
        if rule.after == AfterMatch::Stop {
            break;
        }
    }
    if !fired.is_empty() {
        ran.acted.push((message.id, fired));
    }
}

fn label_named(store: &SqliteStore, name: &str, origin: LabelOrigin) -> LabelId {
    let labels = store.labels(account()).unwrap();
    let found = labels
        .iter()
        .find(|l| l.name == name)
        .or_else(|| labels.iter().find(|l| l.name.eq_ignore_ascii_case(name)));
    if let Some(label) = found {
        return label.id;
    }
    let label = Label {
        id: LabelId::generate(),
        account: account(),
        name: name.to_owned(),
        color: None,
        origin,
    };
    store
        .apply(
            account(),
            &Patch {
                id: ChangeId::generate(),
                changes: vec![Change::LabelUpsert(label.clone())],
            },
            chrono::Utc::now(),
        )
        .unwrap();
    label.id
}

fn perform(
    store: &SqliteStore,
    caps: &AccountCaps,
    op: &Op,
    message: &Message,
    now: DateTime<Utc>,
) -> usize {
    let thread = store.thread(message.thread).unwrap();
    let messages: Vec<Message> = thread
        .messages
        .iter()
        .map(|id| store.message(*id).unwrap())
        .collect();
    let applied = op.apply(
        &Target::Messages(vec![message.id]),
        &thread,
        &messages,
        caps,
        now,
    );
    if applied.forward.changes.is_empty() {
        return 0;
    }
    let commit = applied.into_commit(message.account.clone(), now, true);
    store
        .commit(&commit)
        .unwrap()
        .queued
        .iter()
        .flatten()
        .count()
}

// ---------------------------------------------------------------------------------------
// The scenarios
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RuleSpec {
    filter: usize,
    actions: Vec<usize>,
    stop: bool,
}

#[derive(Debug, Clone)]
struct MessageSpec {
    sender: usize,
    thread: usize,
    sent: bool,
}

#[derive(Debug, Clone)]
struct Scenario {
    rules: Vec<RuleSpec>,
    messages: Vec<MessageSpec>,
    muted: [bool; 2],
    gmail: bool,
}

const SENDERS: [&str; 3] = ["ada@example.test", "bob@example.test", "news@lists.example"];

fn filter_of(n: usize) -> Filter {
    let at = |path: &str| MailboxRef {
        account: account(),
        path: path.to_owned(),
    };
    match n % 9 {
        0 => Filter::All,
        1 => from("ada"),
        2 => from("lists"),
        3 => Filter::Read(ReadState::Read),
        4 => Filter::Read(ReadState::Unread),
        5 => Filter::Not(Box::new(from("ada"))),
        6 => Filter::InFolder(at("INBOX")),
        7 => Filter::InFolder(at("Projects")),
        _ => Filter::Starred(Star::Starred),
    }
}

fn action_of(n: usize) -> RuleAction {
    match n % 12 {
        0 => RuleAction::MarkRead,
        1 => RuleAction::MarkUnread,
        2 => RuleAction::Star,
        3 => RuleAction::Archive,
        4 => RuleAction::Trash,
        5 => RuleAction::Spam,
        6 => RuleAction::Label("Bills".to_owned()),
        7 => RuleAction::Label("bills".to_owned()),
        8 => RuleAction::File("Projects".to_owned()),
        9 => RuleAction::File("Later".to_owned()),
        10 => RuleAction::Notify,
        _ => RuleAction::Forward("friend@elsewhere.example".to_owned()),
    }
}

fn scenario() -> impl Strategy<Value = Scenario> {
    let rule = (
        0..9usize,
        prop::collection::vec(0..12usize, 1..4),
        any::<bool>(),
    )
        .prop_map(|(filter, actions, stop)| RuleSpec {
            filter,
            actions,
            stop,
        });
    let message =
        (0..3usize, 0..2usize, prop::bool::weighted(0.15)).prop_map(|(sender, thread, sent)| {
            MessageSpec {
                sender,
                thread,
                sent,
            }
        });
    (
        prop::collection::vec(rule, 0..4),
        prop::collection::vec(message, 1..5),
        [any::<bool>(), any::<bool>()],
        any::<bool>(),
    )
        .prop_map(|(rules, messages, muted, gmail)| Scenario {
            rules,
            messages,
            muted,
            gmail,
        })
}

/// What either way of arriving could be seen to do.
#[derive(Debug, PartialEq)]
struct Seen {
    ran: Ran,
    states: Vec<State>,
    ops: Vec<ProtoOp>,
    labels: Vec<(String, LabelOrigin)>,
    muted: Vec<Mute>,
}

fn thread_id(n: usize) -> ThreadId {
    ThreadId::from_uuid(uuid::Uuid::from_u128(0x7100 + n as u128))
}

/// The scenario set up in a fresh store, and arrived: by `way`.
fn run(
    scenario: &Scenario,
    way: impl FnOnce(&SqliteStore, &AccountCaps, &[MessageId], DateTime<Utc>) -> Ran,
) -> Seen {
    let (store, raw) = fixture();
    for (n, spec) in scenario.rules.iter().enumerate() {
        let mut r = rule(
            n as u32 + 1,
            &format!("rule {n}"),
            filter_of(spec.filter),
            spec.actions.iter().map(|a| action_of(*a)).collect(),
        );
        r.id = RuleId::from_uuid(uuid::Uuid::from_u128(0x5100 + n as u128));
        if spec.stop {
            r.after = AfterMatch::Stop;
        }
        store.put_rule(&r).unwrap();
    }
    let caps = if scenario.gmail { gmail() } else { folders() };
    // The conversations exist before the mail under test: one earlier message in each.
    let older: Vec<Fetched> = (0..2)
        .map(|t| {
            let mut m = arriving(100 + t as u32, "ada@example.test", MailboxRole::Inbox, raw);
            m.message.id = MessageId::from_uuid(uuid::Uuid::from_u128(0x1100 + t as u128));
            m.message.thread = thread_id(t);
            m.message.date = at(-1000 + t as i64);
            m
        })
        .collect();
    store
        .ingest(account(), ingest(older), chrono::Utc::now())
        .unwrap();
    for (t, muted) in scenario.muted.iter().enumerate() {
        if *muted {
            set_mute(&store, thread_id(t), Mute::Muted);
        }
    }
    let fresh: Vec<Fetched> = scenario
        .messages
        .iter()
        .enumerate()
        .map(|(n, spec)| {
            let mailbox = if spec.sent {
                MailboxRole::Sent
            } else {
                MailboxRole::Inbox
            };
            let mut m = arriving(n as u32 + 1, SENDERS[spec.sender], mailbox, raw);
            m.message.id = MessageId::from_uuid(uuid::Uuid::from_u128(0x1000 + n as u128));
            m.message.thread = thread_id(spec.thread);
            m
        })
        .collect();
    let new = arrived(
        &store
            .ingest(account(), ingest(fresh), chrono::Utc::now())
            .unwrap(),
    );
    let ran = way(&store, &caps, &new, at(500));

    let mut ids: Vec<MessageId> = store_ids(&store);
    ids.sort();
    let mut labels: Vec<(String, LabelOrigin)> = store
        .labels(account())
        .unwrap()
        .into_iter()
        .map(|l| (l.name, l.origin))
        .collect();
    labels.sort_by(|a, b| a.0.cmp(&b.0));
    Seen {
        ran,
        states: ids.iter().map(|id| state(&store, *id)).collect(),
        ops: outbox(&store),
        labels,
        muted: (0..2)
            .map(|t| store.thread(thread_id(t)).unwrap().summary.mute)
            .collect(),
    }
}

fn store_ids(store: &SqliteStore) -> Vec<MessageId> {
    mail_store::testing::message_ids(store)
}

fn fold(store: &SqliteStore, caps: &AccountCaps, arrived: &[MessageId], now: DateTime<Utc>) -> Ran {
    use mail_logic::arrival::ArrivalEffect;
    let ran = mail_core::arrival::arrive(
        store,
        &account(),
        true,
        caps,
        arrived,
        mail_logic::junk::Policy::default(),
        now,
    )
    .unwrap();
    Ran {
        acted: ran.acted,
        queued: ran.queued,
        examined: ran.examined,
        muted: ran.muted,
        left: ran
            .effects
            .into_iter()
            .map(|effect| match effect {
                ArrivalEffect::Notice(message) => (message, RuleAction::Notify),
                ArrivalEffect::RuleSend(request) => (request.message, request.action),
            })
            .collect(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// Whatever the rules and whatever arrives, the fold leaves the store, the outbox and the
    /// caller as the sequential way did.
    #[test]
    fn the_fold_does_what_arriving_one_operation_at_a_time_did(scenario in scenario()) {
        let before = run(&scenario, sequentially);
        let after = run(&scenario, fold);
        prop_assert_eq!(&after.ran, &before.ran);
        prop_assert_eq!(&after.states, &before.states);
        prop_assert_eq!(&after.ops, &before.ops);
        prop_assert_eq!(&after.labels, &before.labels);
        prop_assert_eq!(&after.muted, &before.muted);
    }
}
