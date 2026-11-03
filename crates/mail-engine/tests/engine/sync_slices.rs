//! A first sync lands as a list that keeps growing, not one jump at the end.
//!
//! The engine asks for a band's headers in slices and keeps each before asking for the next. The
//! backend here is scripted, and records what the store held when each slice was asked for: that is
//! what a person watching the list would have seen. The store's own commits are counted too.

use crate::fixture::{acct_account, inbox, now};
use mail_domain::*;
use mail_engine::AccountEngine;
use mail_proto::{Backend, IoReady, Progress, ProtoOutcome};
use mail_store::{SqliteStore, Store};
use porter_secrets::MemorySecrets;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::watch;

fn remote(uid: u32) -> RemoteRef {
    RemoteRef::Imap {
        mailbox: "INBOX".to_owned(),
        uidvalidity: 1,
        uid,
    }
}

fn raw(uid: u32) -> Vec<u8> {
    format!(
        "From: Ada <ada@example.test>\r\nTo: me@example.test\r\nSubject: Mail {uid}\r\nMessage-ID: <m{uid}@example.test>\r\nDate: Tue, 14 Nov 2023 22:13:20 +0000\r\n\r\nHello\r\n"
    )
    .into_bytes()
}

/// One entry per `FetchHeaders`: the uids asked for, and how many messages were stored by then.
type Asked = Arc<Mutex<Vec<(Vec<u32>, usize)>>>;

struct Scripted {
    caps: AccountCaps,
    count: u32,
    asked: Asked,
    store: Arc<OnceLock<Arc<SqliteStore>>>,
    /// Stop the pass once this many slices have been asked for.
    stop_after: Option<(usize, watch::Sender<bool>)>,
}

impl Backend for Scripted {
    fn begin(&mut self, op: ProtoOp) -> Progress<ProtoOutcome> {
        match op {
            ProtoOp::FetchEnvelopes { mailbox, .. } => {
                Progress::Done(ProtoOutcome::Ingested(Box::new(Ingest {
                    mailbox,
                    validity: UidValidity::Same,
                    cursor: Some(SyncCursor::Imap {
                        uidvalidity: 1,
                        uidnext: self.count + 1,
                        modseq: None,
                    }),
                    messages: Vec::new(),
                    flags: Vec::new(),
                    flag_colors: Vec::new(),
                    labels: Vec::new(),
                    label_names: Vec::new(),
                    provider_threads: Vec::new(),
                    gone: Vec::new(),
                })))
            }
            ProtoOp::FetchHeaders { remotes } => {
                let uids: Vec<u32> = remotes
                    .iter()
                    .map(|r| match r {
                        RemoteRef::Imap { uid, .. } => *uid,
                        other => panic!("{other:?}"),
                    })
                    .collect();
                let stored = mail_store::testing::message_ids(self.store.get().unwrap()).len();
                let mut asked = self.asked.lock().unwrap();
                asked.push((uids.clone(), stored));
                if let Some((n, tx)) = &self.stop_after
                    && asked.len() >= *n
                {
                    tx.send(true).unwrap();
                }
                Progress::Done(ProtoOutcome::Fetched {
                    items: uids.iter().map(|u| (remote(*u), raw(*u))).collect(),
                    flags: Vec::new(),
                    flag_colors: Vec::new(),
                    label_names: Vec::new(),
                })
            }
            other => panic!("not scripted: {other:?}"),
        }
    }

    fn feed(&mut self, _: IoReady) -> Progress<ProtoOutcome> {
        unreachable!("every op above is answered without I/O")
    }

    fn caps(&self) -> &AccountCaps {
        &self.caps
    }

    /// The listing is in server order, oldest first.
    fn surveyed(&self) -> Vec<(RemoteRef, u64)> {
        (1..=self.count).map(|u| (remote(u), 1_000)).collect()
    }
}

/// Somewhere for the engine to connect before it asks the backend anything. Nothing is read or
/// written: the backend answers every op without I/O.
fn somewhere_to_connect() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for sock in listener.incoming().flatten() {
            held.push(sock);
        }
    });
    port
}

struct Fixture {
    engine: AccountEngine<Scripted>,
    store: Arc<SqliteStore>,
    asked: Asked,
    _dir: tempfile::TempDir,
}

fn fixture(count: u32, stop_after: Option<(usize, watch::Sender<bool>)>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::in_memory(dir.path()).unwrap());
    mail_store::testing::seed_account(&store, acct_account(), "me@example.test");
    let caps = AccountCaps {
        labels: ServerLabels::LocalOnly,
        threads: ServerThreads::Jwz,
        watch: WatchMode::Idle,
        archive: ArchiveMeans::LocalOnly,
        folders: FolderRoles::default(),
        condstore: Condstore::Absent,
        move_ext: MoveExt::Absent,
        expunge: ExpungeMeans::Forbidden,
        top: Supported::Absent,
        pipelining: Supported::Absent,
        connections: ConnectionBudget { max: 1 },
        observed_at: now(),
    };
    let plan = AccountPlan {
        address: "me@example.test".to_owned(),
        incoming: Incoming::Imap {
            host: "127.0.0.1".to_owned(),
            port: somewhere_to_connect(),
            tls: Tls::Plaintext,
        },
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
    };
    let asked: Asked = Arc::default();
    let cell = Arc::new(OnceLock::new());
    cell.set(store.clone()).ok();
    let backend = Scripted {
        caps,
        count,
        asked: asked.clone(),
        store: cell,
        stop_after,
    };
    let engine = AccountEngine::new(
        acct_account(),
        plan,
        backend,
        store.clone(),
        Arc::new(MemorySecrets::default()),
    );
    Fixture {
        engine,
        store,
        asked,
        _dir: dir,
    }
}

fn commits(heard: &std::sync::mpsc::Receiver<mail_store::Committed>) -> usize {
    heard.try_iter().count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_mail_lands_in_slices_newest_first() {
    let mut it = fixture(120, None);
    let heard = it.store.subscribe();
    let (_tx, mut cancel) = watch::channel(false);

    let report = it
        .engine
        .sync(&inbox(), &mut cancel, now(), 200)
        .await
        .unwrap();
    assert_eq!(report.headers_fetched, 120, "{:?}", report.needs_attention);

    let asked = it.asked.lock().unwrap();
    let sizes: Vec<usize> = asked.iter().map(|(uids, _)| uids.len()).collect();
    assert_eq!(sizes, [25, 50, 45]);
    assert_eq!(
        asked[0].0,
        (96..=120).rev().collect::<Vec<u32>>(),
        "the first rows are the newest"
    );
    let stored_by_then: Vec<usize> = asked.iter().map(|(_, stored)| *stored).collect();
    assert_eq!(
        stored_by_then,
        [0, 25, 75],
        "each slice is kept before the next is asked for"
    );
    assert_eq!(mail_store::testing::message_ids(&it.store).len(), 120);
    assert!(
        commits(&heard) >= 3,
        "the store published a commit per slice, not one at the end"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_few_new_messages_are_still_one_fetch_and_one_commit() {
    let mut it = fixture(5, None);
    let (_tx, mut cancel) = watch::channel(false);
    // The survey's own commit is not what is counted.
    it.engine
        .sync(&inbox(), &mut cancel, now(), 200)
        .await
        .unwrap();
    let asked = it.asked.lock().unwrap();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0.len(), 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_pass_keeps_its_slices_and_the_next_one_finishes_the_rest() {
    let (tx, mut cancel) = watch::channel(false);
    let mut it = fixture(120, Some((1, tx)));

    it.engine
        .sync(&inbox(), &mut cancel, now(), 200)
        .await
        .unwrap();
    let kept = mail_store::testing::message_ids(&it.store).len();
    assert_eq!(
        kept, 25,
        "the slice asked for before the stop is kept, and no more"
    );
    assert_eq!(
        it.store.held_through(&inbox()).unwrap(),
        None,
        "the mailbox is not marked fully held"
    );

    // The next pass fetches only what is missing.
    let (_tx2, mut cancel) = watch::channel(false);
    it.engine
        .sync(&inbox(), &mut cancel, now(), 200)
        .await
        .unwrap();
    assert_eq!(mail_store::testing::message_ids(&it.store).len(), 120);
    let asked = it.asked.lock().unwrap();
    let again: usize = asked[1..].iter().map(|(uids, _)| uids.len()).sum();
    assert_eq!(again, 95, "nothing already stored is fetched again");
}
