//! Mail that has just arrived, acted on: the user's rules, the muted conversations and the junk
//! filter, in the order the person chose.
//!
//! These are the recorded scenarios the arrival fold must agree with. Each is run through
//! [`arrive`] (and [`run_now`], [`apply_to`] for rules asked for), and what is asserted is the
//! state the store is left in, the work queued for the server and what the rules left to the
//! caller. The first revision of this file drove the store's own rules (`mail_store::rules`) and
//! the junk filter in sequence, as a sync pass did; the expectations have not changed since.

use crate::support::{account, store};
use chrono::{DateTime, TimeZone, Utc};
use mail_domain::id::account_id_from_uuid;
use mail_domain::*;
use mail_logic::arrival::{ArrivalEffect, Speaking};
use mail_logic::junk::{ENOUGH, Filtering, Order, Policy, Treat};
use mail_store::SqliteStore;
use mail_store::Store;
use porter_core::AccountId;

pub(crate) fn other() -> AccountId {
    account_id_from_uuid(uuid::uuid!("00000000-0000-4000-8000-0000000000a2"))
}

pub(crate) fn at(n: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + n, 0).unwrap()
}

pub(crate) fn caps(labels: ServerLabels, archive: ArchiveMeans) -> AccountCaps {
    AccountCaps {
        labels,
        threads: ServerThreads::ProviderId,
        watch: WatchMode::Idle,
        archive,
        folders: FolderRoles::default(),
        condstore: Condstore::Supported,
        move_ext: MoveExt::Supported,
        expunge: ExpungeMeans::Forbidden,
        top: Supported::Absent,
        pipelining: Supported::Absent,
        connections: ConnectionBudget::default(),
        observed_at: at(0),
    }
}

pub(crate) fn gmail() -> AccountCaps {
    caps(ServerLabels::Supported, ArchiveMeans::DropInbox)
}

pub(crate) fn folders() -> AccountCaps {
    caps(
        ServerLabels::LocalOnly,
        ArchiveMeans::MoveToFolder("Archive".to_owned()),
    )
}

pub(crate) fn rule(position: u32, name: &str, filter: Filter, actions: Vec<RuleAction>) -> Rule {
    Rule {
        made: Default::default(),
        reply_days: 4,
        id: RuleId::generate(),
        account: account(),
        name: name.to_owned(),
        position,
        state: RuleState::Enabled,
        filter,
        actions,
        after: AfterMatch::Continue,
    }
}

pub(crate) fn from(s: &str) -> Filter {
    Filter::From(TextMatch::Contains(s.to_owned()))
}

/// One message from `sender`, in its own thread, as the server would hand it over.
pub(crate) fn arriving(n: u32, sender: &str, mailbox: MailboxRole, raw: BlobId) -> Fetched {
    let key = format!("m{n}@example.test");
    let message = Message {
        id: MessageId::generate(),
        thread: ThreadId::generate(),
        key: MessageKey::Rfc(key.clone()),
        date: at(i64::from(n)),
        from: Address {
            name: None,
            email: sender.to_owned(),
        },
        to: vec![Address {
            name: None,
            email: "me@example.test".to_owned(),
        }],
        subject: format!("Message {n}"),
        rfc_message_id: Some(key.clone()),
        mailbox,
        body: Body::Present {
            text: Some(format!("body of message {n}")),
            raw,
        },
        account: account(),
        ..mail_domain::testing::message(0, sender, "", "")
    };
    Fetched {
        remote: RemoteRef::Imap {
            mailbox: "INBOX".to_owned(),
            uidvalidity: 1,
            uid: n,
        },
        key: MessageKey::Rfc(key),
        raw,
        message,
    }
}

pub(crate) fn ingest(messages: Vec<Fetched>) -> Ingest {
    Ingest {
        mailbox: MailboxRef {
            account: account(),
            path: "INBOX".to_owned(),
        },
        validity: UidValidity::Same,
        cursor: None,
        messages,
        flags: vec![],
        flag_colors: Vec::new(),
        labels: vec![],
        label_names: Vec::new(),
        provider_threads: Vec::new(),
        gone: vec![],
    }
}

/// What a sync pass reports as arrived: the messages the ingest stored for the first time.
pub(crate) fn arrived(patch: &Patch) -> Vec<MessageId> {
    patch
        .changes
        .iter()
        .filter_map(|c| match c {
            Change::MessageUpsert(m) => Some(m.id),
            _ => None,
        })
        .collect()
}

pub(crate) fn outbox(store: &SqliteStore) -> Vec<ProtoOp> {
    store
        .outbox_due(account(), at(1_000_000))
        .unwrap()
        .into_iter()
        .map(|e| e.op)
        .collect()
}

/// A store holding both accounts, with a blob for the messages' raw bytes.
pub(crate) fn fixture() -> (SqliteStore, BlobId) {
    let store = store();
    mail_store::testing::seed_account(&store, other(), "also@example.test");
    let raw = store.blobs().put(b"raw").unwrap();
    (store, raw)
}

/// What an arrival, or a run of rules, did, as far as the caller can see.
#[derive(Debug, Clone, PartialEq, Default)]
struct Seen {
    /// The names of the rules that acted, message by message.
    acted: Vec<Vec<String>>,
    queued: usize,
    examined: usize,
    muted: usize,
    junk: Vec<MessageId>,
    /// What the rules left to the caller: sending and saying.
    left: Vec<(MessageId, RuleAction)>,
}

/// Where a message is, whether read, whether starred, and its labels by name.
pub(crate) type State = (MailboxRole, ReadState, Star, Vec<String>);

pub(crate) fn state(store: &SqliteStore, id: MessageId) -> State {
    let m = store.message(id).unwrap();
    let labels = store.labels(account()).unwrap();
    let mut named: Vec<String> = m
        .labels
        .iter()
        .filter_map(|l| labels.iter().find(|x| x.id == *l).map(|x| x.name.clone()))
        .collect();
    named.sort();
    (m.mailbox, m.read, m.star, named)
}

// ---------------------------------------------------------------------------------------
// The drivers: the one place this file says how arrival is run.
// ---------------------------------------------------------------------------------------

fn seen(ran: mail_core::arrival::Arrived) -> Seen {
    Seen {
        acted: ran.acted.iter().map(|(_, names)| names.clone()).collect(),
        queued: ran.queued,
        examined: ran.examined,
        muted: ran.muted.len(),
        junk: ran.junk,
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

/// What a pass does with `arrived`, as the policy orders it: the junk filter and the rules.
fn arrive(
    store: &SqliteStore,
    caps: &AccountCaps,
    arrived: &[MessageId],
    policy: Policy,
    now: DateTime<Utc>,
) -> Seen {
    seen(mail_core::arrival::arrive(store, &account(), true, caps, arrived, policy, now).unwrap())
}

/// [`arrive`] with the junk filter off.
fn arrive_rules(
    store: &SqliteStore,
    caps: &AccountCaps,
    arrived: &[MessageId],
    now: DateTime<Utc>,
) -> Seen {
    arrive(store, caps, arrived, Policy::default(), now)
}

fn run_now(
    store: &SqliteStore,
    caps: &AccountCaps,
    rule: &Rule,
    within: &Filter,
    batch: u32,
    now: DateTime<Utc>,
    progress: &mut dyn FnMut(usize),
) -> Seen {
    seen(
        mail_core::arrival::run_now(store, true, caps, rule, within, batch, now, &mut |ran| {
            progress(ran.examined);
        })
        .unwrap(),
    )
}

fn apply_to(
    store: &SqliteStore,
    account: AccountId,
    caps: &AccountCaps,
    picked: &[MessageId],
    now: DateTime<Utc>,
) -> Seen {
    seen(
        mail_core::arrival::apply_to(store, &account, true, caps, picked, Speaking::Yes, now)
            .unwrap(),
    )
}

// ---------------------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------------------

#[test]
fn a_rule_acts_once_on_arriving_mail_and_the_server_is_told_as_the_user_would_tell_it() {
    let (store, raw) = fixture();
    store
        .put_rule(&rule(
            1,
            "Bills",
            from("bank.example"),
            vec![
                RuleAction::MarkRead,
                RuleAction::Label("Bills".to_owned()),
                RuleAction::Archive,
            ],
        ))
        .unwrap();
    let bank = arriving(1, "statements@bank.example", MailboxRole::Inbox, raw);
    let friend = arriving(2, "ada@example.test", MailboxRole::Inbox, raw);
    let (bank_id, friend_id) = (bank.message.id, friend.message.id);
    let patch = store
        .ingest(
            account(),
            ingest(vec![bank.clone(), friend.clone()]),
            chrono::Utc::now(),
        )
        .unwrap();
    let new = arrived(&patch);
    assert_eq!(new.len(), 2, "both are new");

    let seen = arrive_rules(&store, &gmail(), &new, at(10));
    let ops = outbox(&store);

    assert_eq!(seen.acted, vec![vec!["Bills".to_owned()]]);
    assert_eq!(seen.queued, 3);
    assert_eq!(seen.examined, 2);
    assert_eq!(
        state(&store, bank_id),
        (
            MailboxRole::Archive,
            ReadState::Read,
            Star::Unstarred,
            vec!["Bills".to_owned()]
        )
    );
    assert_eq!(
        state(&store, friend_id),
        (
            MailboxRole::Inbox,
            ReadState::Unread,
            Star::Unstarred,
            vec![]
        )
    );
    let bank_remote = RemoteRef::Imap {
        mailbox: "INBOX".to_owned(),
        uidvalidity: 1,
        uid: 1,
    };
    assert_eq!(
        ops,
        vec![
            ProtoOp::SetFlags {
                remotes: vec![bank_remote.clone()],
                read: Some(ReadState::Read),
                star: None,
            },
            ProtoOp::SetLabels {
                remotes: vec![bank_remote.clone()],
                add: vec!["Bills".to_owned()],
                remove: vec![],
            },
            // Archive on Gmail: `SetMailbox` to Archive, which the backend sends as the Inbox
            // label removed.
            ProtoOp::SetMailbox {
                remotes: vec![bank_remote],
                role: MailboxRole::Archive,
            },
        ]
    );

    // The same messages again: stored already, so they did not arrive.
    let patch = store
        .ingest(account(), ingest(vec![bank, friend]), chrono::Utc::now())
        .unwrap();
    let again = arrive_rules(&store, &gmail(), &arrived(&patch), at(20));
    assert!(again.acted.is_empty(), "nothing arrived the second time");
    assert_eq!(outbox(&store).len(), 3, "and nothing more was queued");
}

#[test]
fn a_rule_that_labels_a_message_makes_the_label_even_when_it_is_already_there() {
    let (store, raw) = fixture();
    store
        .put_rule(&rule(
            1,
            "Twice",
            Filter::All,
            vec![
                RuleAction::Label("Bills".to_owned()),
                RuleAction::Label("bills".to_owned()),
            ],
        ))
        .unwrap();
    let m = arriving(1, "ada@example.test", MailboxRole::Inbox, raw);
    let id = m.message.id;
    let patch = store
        .ingest(account(), ingest(vec![m]), chrono::Utc::now())
        .unwrap();
    let seen = arrive_rules(&store, &gmail(), &arrived(&patch), at(10));
    // The second names the first's label in another case, and finds it: one label, one op.
    assert_eq!(seen.queued, 1);
    assert_eq!(state(&store, id).3, vec!["Bills".to_owned()]);
    let made: Vec<String> = store
        .labels(account())
        .unwrap()
        .into_iter()
        .map(|l| l.name)
        .collect();
    assert_eq!(made, vec!["Bills".to_owned()]);
}

/// Mail that was here before a rule existed is left alone at arrival, and `run_now` reaches it,
/// in batches.
#[test]
fn run_now_reaches_mail_that_was_already_here_in_batches() {
    let (store, raw) = fixture();
    let old: Vec<Fetched> = (1..=3)
        .map(|n| arriving(n, "news@lists.example", MailboxRole::Inbox, raw))
        .chain([arriving(4, "ada@example.test", MailboxRole::Inbox, raw)])
        // Sent by the user: a rule is about mail received.
        .chain([arriving(5, "news@lists.example", MailboxRole::Sent, raw)])
        .collect();
    store
        .ingest(account(), ingest(old), chrono::Utc::now())
        .unwrap();

    let news = rule(
        1,
        "News",
        from("lists.example"),
        vec![RuleAction::File("Newsletters".to_owned())],
    );
    store.put_rule(&news).unwrap();

    let fresh = arriving(6, "news@lists.example", MailboxRole::Inbox, raw);
    let patch = store
        .ingest(account(), ingest(vec![fresh]), chrono::Utc::now())
        .unwrap();
    let at_arrival = arrive_rules(&store, &folders(), &arrived(&patch), at(10));
    assert_eq!(
        at_arrival.acted,
        vec![vec!["News".to_owned()]],
        "only the new one"
    );

    let mut batches = Vec::new();
    let ran = run_now(
        &store,
        &folders(),
        &news,
        &Filter::All,
        2,
        at(20),
        &mut |examined| batches.push(examined),
    );
    // Six conversations, two a batch, newest first; the Sent one is looked past, not at.
    assert_eq!(batches, vec![1, 2, 2]);
    assert_eq!(ran.examined, 5);
    // Four newsletters match. The one filed at arrival is already where the rule puts it, so
    // only the three old ones had anything to queue.
    assert_eq!(ran.acted.len(), 4);
    assert_eq!(ran.queued, 3);
    let ops = outbox(&store);
    let filed: Vec<&ProtoOp> = ops
        .iter()
        .filter(|op| matches!(op, ProtoOp::File { folder, .. } if folder == "Newsletters"))
        .collect();
    assert_eq!(filed.len(), 4, "{ops:?}");
}

/// Rules run in order against the message as the ones before left it; `stop` ends it.
#[test]
fn a_later_rule_sees_what_an_earlier_one_did_and_stop_means_stop() {
    let (store, raw) = fixture();
    let mut first = rule(1, "read it", from("ada"), vec![RuleAction::MarkRead]);
    first.after = AfterMatch::Continue;
    let second = rule(
        2,
        "star what is read",
        Filter::Read(ReadState::Read),
        vec![RuleAction::Star],
    );
    let mut third = rule(3, "stop here", Filter::All, vec![]);
    third.after = AfterMatch::Stop;
    let never = rule(4, "never reached", Filter::All, vec![RuleAction::Trash]);
    let mut off = rule(0, "disabled", Filter::All, vec![RuleAction::Spam]);
    off.state = RuleState::Disabled;
    for r in [&never, &third, &second, &first, &off] {
        store.put_rule(r).unwrap();
    }
    let m = arriving(1, "ada@example.test", MailboxRole::Inbox, raw);
    let id = m.message.id;
    let patch = store
        .ingest(account(), ingest(vec![m]), chrono::Utc::now())
        .unwrap();
    let seen = arrive_rules(&store, &folders(), &arrived(&patch), at(10));
    assert_eq!(
        seen.acted,
        vec![vec![
            "read it".to_owned(),
            "star what is read".to_owned(),
            "stop here".to_owned()
        ]]
    );
    assert_eq!(
        state(&store, id),
        (MailboxRole::Inbox, ReadState::Read, Star::Starred, vec![])
    );
}

#[test]
fn filing_on_a_server_with_folders_queues_one_move_into_the_folder() {
    let (store, raw) = fixture();
    store
        .put_rule(&rule(
            1,
            "Projects",
            Filter::Subject(TextMatch::Contains("message".to_owned())),
            vec![RuleAction::File("Projects/2026".to_owned())],
        ))
        .unwrap();
    let m = arriving(3, "ada@example.test", MailboxRole::Inbox, raw);
    let id = m.message.id;
    let patch = store
        .ingest(account(), ingest(vec![m]), chrono::Utc::now())
        .unwrap();
    arrive_rules(&store, &folders(), &arrived(&patch), at(10));
    assert_eq!(
        outbox(&store),
        vec![ProtoOp::File {
            remotes: vec![RemoteRef::Imap {
                mailbox: "INBOX".to_owned(),
                uidvalidity: 1,
                uid: 3,
            }],
            folder: "Projects/2026".to_owned(),
        }]
    );
    assert_eq!(
        state(&store, id),
        (
            MailboxRole::Archive,
            ReadState::Unread,
            Star::Unstarred,
            vec!["Projects/2026".to_owned()]
        )
    );
    let label = store
        .labels(account())
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Projects/2026")
        .unwrap();
    assert_eq!(label.origin, LabelOrigin::Provider, "the server's folder");
}

/// A rule may name a server folder: the message's own addresses answer it.
#[test]
fn a_rule_about_a_folder_asks_the_messages_own_addresses() {
    let (store, raw) = fixture();
    let at_path = |path: &str| MailboxRef {
        account: account(),
        path: path.to_owned(),
    };
    store
        .put_rule(&rule(
            1,
            "from the inbox",
            Filter::InFolder(at_path("INBOX")),
            vec![RuleAction::Star],
        ))
        .unwrap();
    store
        .put_rule(&rule(
            2,
            "from a folder",
            Filter::InFolder(at_path("Projects")),
            vec![RuleAction::MarkRead],
        ))
        .unwrap();
    let m = arriving(1, "ada@example.test", MailboxRole::Inbox, raw);
    let patch = store
        .ingest(account(), ingest(vec![m]), chrono::Utc::now())
        .unwrap();
    let seen = arrive_rules(&store, &folders(), &arrived(&patch), at(10));
    assert_eq!(seen.acted, vec![vec!["from the inbox".to_owned()]]);
}

/// A rule that files a message takes it out of the folder it was in for the rules after it, as
/// a move queued for the server takes it out of a folder view.
#[test]
fn a_rule_that_files_a_message_takes_it_out_of_its_folder_for_the_rules_after() {
    let (store, raw) = fixture();
    let inbox = MailboxRef {
        account: account(),
        path: "INBOX".to_owned(),
    };
    store
        .put_rule(&rule(
            1,
            "file it",
            from("ada"),
            vec![RuleAction::File("Projects".to_owned())],
        ))
        .unwrap();
    store
        .put_rule(&rule(
            2,
            "still in the inbox",
            Filter::InFolder(inbox),
            vec![RuleAction::Star],
        ))
        .unwrap();
    let m = arriving(1, "ada@example.test", MailboxRole::Inbox, raw);
    let id = m.message.id;
    let patch = store
        .ingest(account(), ingest(vec![m]), chrono::Utc::now())
        .unwrap();
    let seen = arrive_rules(&store, &folders(), &arrived(&patch), at(10));
    assert_eq!(seen.acted, vec![vec!["file it".to_owned()]]);
    assert_eq!(state(&store, id).2, Star::Unstarred);
}

// ---------------------------------------------------------------------------------------
// Mute
// ---------------------------------------------------------------------------------------

/// A message from `sender`, as a reply in the conversation `into`.
pub(crate) fn reply_in(into: &Fetched, n: u32, sender: &str, raw: BlobId) -> Fetched {
    let mut reply = arriving(n, sender, MailboxRole::Inbox, raw);
    reply.message.thread = into.message.thread;
    reply.message.in_reply_to = into.message.rfc_message_id.clone();
    reply.message.references = into.message.rfc_message_id.iter().cloned().collect();
    reply
}

/// Mute or unmute `thread` as the window does, through `Op::apply`, returning its undo.
pub(crate) fn set_mute(store: &SqliteStore, thread: ThreadId, mute: Mute) -> Patch {
    let loaded = store.thread(thread).unwrap();
    let messages: Vec<Message> = loaded
        .messages
        .iter()
        .map(|id| store.message(*id).unwrap())
        .collect();
    let applied = Op::SetMute(mute).apply(
        &Target::Threads(vec![thread]),
        &loaded,
        &messages,
        &gmail(),
        at(5),
    );
    assert_eq!(applied.remote, None, "mute is this client's own state");
    store
        .apply(account(), &applied.forward, chrono::Utc::now())
        .unwrap();
    applied.inverse
}

/// A conversation is muted; its next reply arrives read and archived, another conversation's
/// arrives as it always has, and a reply a rule sent to Spam stays there, read. Unmuted again
/// (by the mute's own undo), the next reply lands in the inbox, unread.
#[test]
fn a_muted_conversations_reply_arrives_read_and_archived_and_an_unmuted_ones_does_not() {
    let (store, raw) = fixture();
    store
        .put_rule(&rule(
            1,
            "Junk",
            from("junk.example"),
            vec![RuleAction::Spam],
        ))
        .unwrap();
    let first = arriving(1, "ada@example.test", MailboxRole::Inbox, raw);
    let elsewhere_first = arriving(2, "bob@example.test", MailboxRole::Inbox, raw);
    store
        .ingest(
            account(),
            ingest(vec![first.clone(), elsewhere_first.clone()]),
            chrono::Utc::now(),
        )
        .unwrap();
    let thread = first.message.thread;
    let undo = set_mute(&store, thread, Mute::Muted);
    assert_eq!(store.thread(thread).unwrap().summary.mute, Mute::Muted);

    let reply = reply_in(&first, 3, "ada@example.test", raw);
    let elsewhere = reply_in(&elsewhere_first, 4, "bob@example.test", raw);
    let junk = reply_in(&first, 5, "news@junk.example", raw);
    let patch = store
        .ingest(
            account(),
            ingest(vec![reply.clone(), elsewhere.clone(), junk.clone()]),
            chrono::Utc::now(),
        )
        .unwrap();
    let seen = arrive_rules(&store, &gmail(), &arrived(&patch), at(10));
    let ops = outbox(&store);

    let unread_inbox = (
        MailboxRole::Inbox,
        ReadState::Unread,
        Star::Unstarred,
        vec![],
    );
    // Mail already here stays where it was: a mute is about what arrives.
    assert_eq!(state(&store, first.message.id), unread_inbox);
    assert_eq!(
        state(&store, reply.message.id),
        (
            MailboxRole::Archive,
            ReadState::Read,
            Star::Unstarred,
            vec![]
        )
    );
    assert_eq!(
        state(&store, elsewhere.message.id),
        unread_inbox,
        "another conversation's reply"
    );
    assert_eq!(
        state(&store, junk.message.id),
        (MailboxRole::Spam, ReadState::Read, Star::Unstarred, vec![]),
        "a rule's filing stands, and it is read"
    );
    assert_eq!(seen.muted, 2);
    let remote = |uid| RemoteRef::Imap {
        mailbox: "INBOX".to_owned(),
        uidvalidity: 1,
        uid,
    };
    // The server hears what the user's own hand would have said: read, then archived.
    assert_eq!(
        ops,
        vec![
            ProtoOp::SetFlags {
                remotes: vec![remote(3)],
                read: Some(ReadState::Read),
                star: None,
            },
            ProtoOp::SetMailbox {
                remotes: vec![remote(3)],
                role: MailboxRole::Archive,
            },
            ProtoOp::SetMailbox {
                remotes: vec![remote(5)],
                role: MailboxRole::Spam,
            },
            ProtoOp::SetFlags {
                remotes: vec![remote(5)],
                read: Some(ReadState::Read),
                star: None,
            },
        ]
    );

    store.apply(account(), &undo, chrono::Utc::now()).unwrap();
    assert_eq!(store.thread(thread).unwrap().summary.mute, Mute::Unmuted);
    let late = reply_in(&first, 6, "ada@example.test", raw);
    let patch = store
        .ingest(account(), ingest(vec![late.clone()]), chrono::Utc::now())
        .unwrap();
    arrive_rules(&store, &gmail(), &arrived(&patch), at(20));
    assert_eq!(
        state(&store, late.message.id),
        unread_inbox,
        "unmuted, mail arrives again"
    );
}

/// Two replies to one muted conversation arrive together: the second is judged in the
/// conversation as the first left it.
#[test]
fn two_replies_to_a_muted_conversation_are_both_kept_out() {
    let (store, raw) = fixture();
    let first = arriving(1, "ada@example.test", MailboxRole::Inbox, raw);
    store
        .ingest(account(), ingest(vec![first.clone()]), chrono::Utc::now())
        .unwrap();
    set_mute(&store, first.message.thread, Mute::Muted);
    let a = reply_in(&first, 2, "ada@example.test", raw);
    let b = reply_in(&first, 3, "ada@example.test", raw);
    let patch = store
        .ingest(
            account(),
            ingest(vec![a.clone(), b.clone()]),
            chrono::Utc::now(),
        )
        .unwrap();
    let seen = arrive_rules(&store, &folders(), &arrived(&patch), at(10));
    assert_eq!(seen.muted, 2);
    let archived = (
        MailboxRole::Archive,
        ReadState::Read,
        Star::Unstarred,
        vec![],
    );
    assert_eq!(state(&store, a.message.id), archived);
    assert_eq!(state(&store, b.message.id), archived);
    assert_eq!(seen.queued, 4);
}

// ---------------------------------------------------------------------------------------
// What a rule asks that only a running client can do
// ---------------------------------------------------------------------------------------

/// A rule that marks mail unread, notifies and forwards: the mark is the store's to make, the
/// rest are left to the caller, in order, as mail arrives and on messages the user picked, and
/// never over the mail already here.
#[test]
fn what_a_rule_says_or_sends_is_left_to_the_caller_and_never_done_over_the_backlog() {
    let (store, raw) = fixture();
    store
        .put_rule(&rule(
            1,
            "Boss",
            Filter::Or(vec![from("boss@example.test"), Filter::Larger(1_000_000)]),
            vec![
                RuleAction::MarkRead,
                RuleAction::MarkUnread,
                RuleAction::Notify,
                RuleAction::Forward("me@elsewhere.example".to_owned()),
            ],
        ))
        .unwrap();
    let boss = arriving(1, "boss@example.test", MailboxRole::Inbox, raw);
    let id = boss.message.id;
    let patch = store
        .ingest(account(), ingest(vec![boss]), chrono::Utc::now())
        .unwrap();
    let seen = arrive_rules(&store, &gmail(), &arrived(&patch), at(10));
    let spoken = vec![
        (id, RuleAction::Notify),
        (id, RuleAction::Forward("me@elsewhere.example".to_owned())),
    ];
    assert_eq!(seen.left, spoken);
    assert_eq!(state(&store, id).1, ReadState::Unread, "the last mark wins");

    let boss_rule = store.rules(account()).unwrap().remove(0);
    let backlog = run_now(
        &store,
        &gmail(),
        &boss_rule,
        &Filter::All,
        10,
        at(20),
        &mut |_| {},
    );
    assert_eq!(backlog.acted.len(), 1, "the backlog run matched it");
    assert!(backlog.left.is_empty(), "and sent nothing");

    let picked = apply_to(&store, account(), &gmail(), &[id], at(30));
    assert_eq!(picked.left, spoken);
    let elsewhere = apply_to(&store, other(), &gmail(), &[id], at(40));
    assert_eq!(
        elsewhere.acted.len(),
        0,
        "a message on another account is not that account's"
    );
}

// ---------------------------------------------------------------------------------------
// The junk filter, and its order against the rules
// ---------------------------------------------------------------------------------------

const SPAMMY: &str = "winner claim your free prize now lottery cash bonus";
const HAMMY: &str = "minutes from the meeting about the quarterly budget review";

fn on(order: Order) -> Policy {
    Policy {
        filtering: Filtering::On,
        order,
        ..Policy::default()
    }
}

/// A store whose filter has learned enough: spam about prizes, good mail about meetings.
fn trained() -> (SqliteStore, BlobId) {
    let (store, raw) = fixture();
    mail_store::testing::seed_default_identity(&store, account(), "me@example.test", None);
    for n in 0..ENOUGH as u128 {
        for (m, judged) in [
            (
                mail_domain::testing::message(5000 + n, "promo@prizes.example", "You won", SPAMMY),
                Judged::Spam,
            ),
            (
                mail_domain::testing::message(6000 + n, "bob@work.example", "Minutes", HAMMY),
                Judged::Ham,
            ),
        ] {
            let mut m = m;
            let blob = store.blobs().put(b"").unwrap();
            if let Body::Present { raw, .. } = &mut m.body {
                *raw = blob;
            }
            store
                .apply(
                    account(),
                    &Patch {
                        id: ChangeId::generate(),
                        changes: vec![Change::MessageUpsert(Box::new(m.clone()))],
                    },
                    chrono::Utc::now(),
                )
                .unwrap();
            mail_core::junk::learn(&store, &[m], judged).unwrap();
        }
    }
    (store, raw)
}

/// A message of junk, which arrives with its body, and one of good mail.
fn junk_and_good(store: &SqliteStore, _raw: BlobId) -> (Fetched, Fetched, Vec<MessageId>) {
    let mail = |from: &str, subject: &str, text: &str| {
        store
            .blobs()
            .put(format!("From: {from}\r\nSubject: {subject}\r\n\r\n{text}\r\n").as_bytes())
            .unwrap()
    };
    let junk_raw = mail("promo@prizes.example", "You won", SPAMMY);
    let good_raw = mail("carol@work.example", "Minutes", HAMMY);
    let mut junk = arriving(1, "promo@prizes.example", MailboxRole::Inbox, junk_raw);
    junk.message.subject = "You won".to_owned();
    let mut good = arriving(2, "carol@work.example", MailboxRole::Inbox, good_raw);
    good.message.subject = "Minutes".to_owned();
    let patch = store
        .ingest(
            account(),
            ingest(vec![junk.clone(), good.clone()]),
            chrono::Utc::now(),
        )
        .unwrap();
    let new = arrived(&patch);
    (junk, good, new)
}

#[test]
fn junk_before_the_rules_leaves_the_rules_only_what_the_filter_left_in_the_inbox() {
    let (store, raw) = trained();
    store
        .put_rule(&rule(1, "Star all", Filter::All, vec![RuleAction::Star]))
        .unwrap();
    let (junk, good, new) = junk_and_good(&store, raw);
    let seen = arrive(&store, &folders(), &new, on(Order::BeforeRules), at(10));
    assert_eq!(seen.junk, vec![junk.message.id]);
    assert_eq!(state(&store, junk.message.id).0, MailboxRole::Spam);
    assert_eq!(
        state(&store, junk.message.id).2,
        Star::Unstarred,
        "no rule saw it"
    );
    assert_eq!(state(&store, good.message.id).2, Star::Starred);
    assert_eq!(seen.acted.len(), 1);
}

#[test]
fn junk_after_the_rules_sees_only_what_the_rules_left_in_the_inbox() {
    let (store, raw) = trained();
    store
        .put_rule(&rule(1, "Star all", Filter::All, vec![RuleAction::Star]))
        .unwrap();
    let (junk, good, new) = junk_and_good(&store, raw);
    let seen = arrive(&store, &folders(), &new, on(Order::AfterRules), at(10));
    assert_eq!(seen.junk, vec![junk.message.id]);
    assert_eq!(state(&store, junk.message.id).0, MailboxRole::Spam);
    assert_eq!(
        state(&store, junk.message.id).2,
        Star::Starred,
        "the rule saw it first"
    );
    assert_eq!(state(&store, good.message.id).2, Star::Starred);
    assert_eq!(seen.acted.len(), 2);
}

#[test]
fn a_rule_that_files_junk_has_the_last_word_when_the_filter_goes_second() {
    let (store, raw) = trained();
    store
        .put_rule(&rule(
            1,
            "Prizes",
            from("prizes.example"),
            vec![RuleAction::Archive],
        ))
        .unwrap();
    let (junk, _, new) = junk_and_good(&store, raw);
    let seen = arrive(&store, &folders(), &new, on(Order::AfterRules), at(10));
    assert!(seen.junk.is_empty(), "it was not in the inbox to judge");
    assert_eq!(state(&store, junk.message.id).0, MailboxRole::Archive);
}

#[test]
fn junk_marked_rather_than_moved_gets_the_junk_label_and_stays_in_the_inbox() {
    let (store, raw) = trained();
    let (junk, _, new) = junk_and_good(&store, raw);
    let policy = Policy {
        treat: Treat::Mark,
        ..on(Order::BeforeRules)
    };
    let seen = arrive(&store, &gmail(), &new, policy, at(10));
    assert_eq!(seen.junk, vec![junk.message.id]);
    assert_eq!(
        state(&store, junk.message.id),
        (
            MailboxRole::Inbox,
            ReadState::Unread,
            Star::Unstarred,
            vec!["Junk".to_owned()]
        )
    );
    assert_eq!(
        outbox(&store),
        vec![ProtoOp::SetLabels {
            remotes: vec![RemoteRef::Imap {
                mailbox: "INBOX".to_owned(),
                uidvalidity: 1,
                uid: 1
            }],
            add: vec!["Junk".to_owned()],
            remove: vec![],
        }]
    );
}

#[test]
fn junk_moved_to_spam_by_the_filter_does_not_teach_it() {
    let (store, raw) = trained();
    let before = store.junk_counts(account(), &[]).unwrap();
    let (_, _, new) = junk_and_good(&store, raw);
    arrive(&store, &folders(), &new, on(Order::BeforeRules), at(10));
    let after = store.junk_counts(account(), &[]).unwrap();
    assert_eq!((after.spam, after.ham), (before.spam, before.ham));
}

#[test]
fn a_rule_that_sends_mail_to_spam_does_not_teach_the_filter() {
    let (store, raw) = trained();
    store
        .put_rule(&rule(
            1,
            "Junk",
            from("junk.example"),
            vec![RuleAction::Spam],
        ))
        .unwrap();
    let before = store.junk_counts(account(), &[]).unwrap();
    let m = arriving(1, "news@junk.example", MailboxRole::Inbox, raw);
    let patch = store
        .ingest(account(), ingest(vec![m]), chrono::Utc::now())
        .unwrap();
    arrive(
        &store,
        &folders(),
        &arrived(&patch),
        on(Order::BeforeRules),
        at(10),
    );
    let after = store.junk_counts(account(), &[]).unwrap();
    assert_eq!((after.spam, after.ham), (before.spam, before.ham));
}

#[test]
fn mail_the_server_filed_as_spam_teaches_the_filter() {
    let (store, raw) = trained();
    let before = store.junk_counts(account(), &[]).unwrap();
    let m = arriving(1, "promo@prizes.example", MailboxRole::Spam, raw);
    let patch = store
        .ingest(account(), ingest(vec![m]), chrono::Utc::now())
        .unwrap();
    arrive(
        &store,
        &folders(),
        &arrived(&patch),
        on(Order::BeforeRules),
        at(10),
    );
    let after = store.junk_counts(account(), &[]).unwrap();
    assert_eq!((after.spam, after.ham), (before.spam + 1, before.ham));
}
