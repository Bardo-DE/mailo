//! A sync hands over the flags of every message in a mailbox, nearly all of them what they were
//! last time. What has not changed is not written, not said, and does not rebuild its summary.

use crate::support::*;
use mail_domain::*;
use mail_store::Store;

/// `count` messages in the inbox, message `n` at UID `n`, each in a thread of its own.
fn delivered(store: &dyn Store, count: u128) -> Vec<Message> {
    (1..=count)
        .map(|n| {
            let m = message(n);
            deliver(store, &m, imap("INBOX", n as u32));
            m
        })
        .collect()
}

/// A sync of the inbox that says every message of `count` is unread and unstarred, except the
/// ones `read` lists, which are read.
fn flags(count: u32, read: &[u32]) -> Ingest {
    Ingest {
        flags: (1..=count)
            .map(|uid| {
                let state = if read.contains(&uid) {
                    ReadState::Read
                } else {
                    ReadState::Unread
                };
                (imap("INBOX", uid), state, Star::Unstarred)
            })
            .collect(),
        ..ingest("INBOX")
    }
}

#[test]
fn flags_the_store_already_holds_are_not_written_again() {
    let found = Both::new().agree(|store| {
        delivered(store, 5);
        let first = store
            .ingest(acct_account(), flags(5, &[2]), chrono::Utc::now())
            .unwrap();
        let second = store
            .ingest(acct_account(), flags(5, &[2]), chrono::Utc::now())
            .unwrap();
        (first.changes.len(), second.changes.len())
    });
    // The first says the one message that was read; the second has nothing to say.
    assert_eq!(found, (1, 0));
}

#[test]
fn a_sync_that_finds_nothing_new_commits_nothing() {
    let store = sqlite();
    delivered(&store, 5);
    store
        .ingest(acct_account(), flags(5, &[2]), chrono::Utc::now())
        .unwrap();
    let rx = store.subscribe();
    let before = store.raw_connection().total_changes();

    store
        .ingest(acct_account(), flags(5, &[2]), chrono::Utc::now())
        .unwrap();

    let said: Vec<_> = rx.try_iter().collect();
    assert!(said.is_empty(), "{said:?}");
    assert_eq!(store.raw_connection().total_changes(), before);
}

#[test]
fn one_flag_flipped_among_many_changes_that_message_and_its_conversation_only() {
    let found = Both::new().agree(|store| {
        let all = delivered(store, 6);
        store
            .ingest(acct_account(), flags(6, &[]), chrono::Utc::now())
            .unwrap();
        let patch = store
            .ingest(acct_account(), flags(6, &[4]), chrono::Utc::now())
            .unwrap();
        let reads: Vec<ReadState> = all
            .iter()
            .map(|m| store.thread(m.thread).unwrap().summary.read)
            .collect();
        (patch.changes, reads)
    });
    let (changes, reads) = found;
    assert_eq!(
        changes,
        vec![Change::MessageRead(message(4).id, ReadState::Read)]
    );
    let expected: Vec<ReadState> = (1..=6)
        .map(|n| {
            if n == 4 {
                ReadState::Read
            } else {
                ReadState::Unread
            }
        })
        .collect();
    assert_eq!(reads, expected);
}

#[test]
fn a_cursor_that_has_not_moved_is_not_written_again_within_the_minute() {
    let store = sqlite();
    let batch = || Ingest {
        cursor: Some(SyncCursor::Pop),
        ..ingest("INBOX")
    };
    store
        .ingest(acct_account(), batch(), chrono::Utc::now())
        .unwrap();
    let before = store.raw_connection().total_changes();

    store
        .ingest(acct_account(), batch(), chrono::Utc::now())
        .unwrap();

    assert_eq!(store.raw_connection().total_changes(), before);
}

#[test]
fn the_minute_a_cursor_is_left_alone_is_the_callers_and_not_the_machines() {
    let store = sqlite();
    let batch = || Ingest {
        cursor: Some(SyncCursor::Pop),
        ..ingest("INBOX")
    };
    store.ingest(acct_account(), batch(), at(1_000)).unwrap();
    assert_eq!(store.last_synced(acct_account()).unwrap(), Some(at(1_000)));

    // Half a minute on, by the caller's clock: nothing is written.
    let before = store.raw_connection().total_changes();
    store.ingest(acct_account(), batch(), at(1_030)).unwrap();
    assert_eq!(store.raw_connection().total_changes(), before);
    assert_eq!(store.last_synced(acct_account()).unwrap(), Some(at(1_000)));

    // Past the minute it says the pass ran, still as one row for the mailbox.
    store.ingest(acct_account(), batch(), at(1_061)).unwrap();
    assert_eq!(store.last_synced(acct_account()).unwrap(), Some(at(1_061)));
    let rows: i64 = store
        .raw_connection()
        .query_row("SELECT count(*) FROM sync_state", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn a_message_is_stamped_first_stored_at_the_callers_time_and_keeps_the_first() {
    let store = sqlite();
    let m = message(1);
    store
        .apply(
            acct_account(),
            &patch(vec![Change::MessageUpsert(Box::new(m.clone()))]),
            at(500),
        )
        .unwrap();
    store
        .apply(
            acct_account(),
            &patch(vec![Change::MessageUpsert(Box::new(m.clone()))]),
            at(900),
        )
        .unwrap();
    let stamped: String = store
        .raw_connection()
        .query_row(
            "SELECT stored_at FROM messages WHERE id = ?1",
            [m.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        stamped,
        at(500).to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
    );
}
