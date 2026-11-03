//! Keeping a watch told when the accounts it fetches have changed, for the watch in this
//! process (`mailo watch`) and for the one the app runs.

use mail_core::SqliteStore;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the accounts are looked at, beside each write to the store.
const ACCOUNTS_EVERY: Duration = Duration::from_secs(30);

/// How soon after one look at the accounts the next may be. A sync writes to the store without
/// stopping, each write saying so, and a look reads every account and its settings.
const ACCOUNTS_GAP: Duration = Duration::from_secs(2);

/// Wait until the accounts are worth looking at again: a write heard on `written`, or `every`
/// with none, but never sooner than `gap` after `looked`, when the last look was. Every write
/// heard by then is taken as one, so a burst of them is a single look, and the one that ends it
/// is not lost for having come early. A store that cannot be watched (its sender went with the
/// watch that was never made) is looked at every `every` all the same.
///
/// Not by `data_version`: a commit through the store's own connection, which is where an account
/// edited in this process is written, does not change it.
fn wait_to_look(
    written: &std::sync::mpsc::Receiver<()>,
    looked: Option<Instant>,
    every: Duration,
    gap: Duration,
) {
    if let Err(std::sync::mpsc::RecvTimeoutError::Disconnected) = written.recv_timeout(every) {
        std::thread::sleep(every);
    }
    if let Some(at) = looked {
        std::thread::sleep(gap.saturating_sub(at.elapsed()));
    }
    while written.try_recv().is_ok() {}
}

/// A receiver that moves whenever the accounts that have a server, how often each is fetched,
/// or when each last finished a pass have changed: read again after each write to the store, and every
/// [`ACCOUNTS_EVERY`], by a thread of its own.
pub fn follow_accounts(
    store: &Arc<SqliteStore>,
    config: Option<std::path::PathBuf>,
) -> mail_core::sync::watcher::Asked {
    let (moved, accounts) = tokio::sync::watch::channel(0u64);
    let store = Arc::clone(store);
    let started = std::thread::Builder::new()
        .name("mailo-accounts".to_owned())
        .spawn(move || {
            let (tx, written) = std::sync::mpsc::channel();
            // Gone only with this thread, which is not a write to say anything about.
            let _watching = store.file().and_then(|file| {
                mail_client::on_write(file, move || {
                    // Nobody left to hear it once this thread has ended.
                    let _ = tx.send(());
                })
            });
            let mail = mail_edge::mail(&store, config.as_deref());
            // How often each account is fetched, and when each last finished a pass: a pass
            // finished in a window that signed an account in is news to a watch that gave up on it.
            let intervals = || {
                let every = match mail.roster() {
                    Ok(accounts) => mail_core::sync::due::intervals(&accounts),
                    Err(why) => {
                        log::warn!("the accounts could not be read: {why}");
                        Vec::new()
                    }
                };
                (every, mail_core::sync::watcher::finished(&mail))
            };
            let mut known = intervals();
            let mut looked = Some(Instant::now());
            loop {
                wait_to_look(&written, looked, ACCOUNTS_EVERY, ACCOUNTS_GAP);
                looked = Some(Instant::now());
                let now = intervals();
                if now != known {
                    known = now;
                    moved.send_modify(|count| *count = count.wrapping_add(1));
                }
            }
        });
    if let Err(why) = started {
        log::warn!("an account added now is fetched from mailo's next start: {why}");
    }
    accounts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_of_writes_is_one_look_and_looks_keep_their_distance() {
        let (tx, written) = std::sync::mpsc::channel();
        let gap = Duration::from_millis(150);
        let every = Duration::from_secs(30);
        let start = Instant::now();
        let mut looked = None;
        let mut looks = 0;
        for _ in 0..3 {
            for _ in 0..50 {
                tx.send(()).unwrap();
            }
            wait_to_look(&written, looked, every, gap);
            looked = Some(Instant::now());
            looks += 1;
            assert!(written.try_recv().is_err(), "the burst was taken as one");
        }
        assert_eq!(looks, 3);
        // Three looks cannot be nearer than two gaps, however many writes asked for them.
        assert!(start.elapsed() >= gap * 2, "{:?}", start.elapsed());
    }
}
