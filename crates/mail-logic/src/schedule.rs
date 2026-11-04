//! What the scheduler of fetching is told and says, as plain values: who schedules, how long it
//! waits, whether the background daemon holds the accounts, and why a watch was lost. The
//! scheduler itself, which runs passes and watches, is `mail-engine`'s.

use mail_domain::Retry;
use std::time::Duration;

/// Who decides when mail is fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// This process: timers, polls and pushes are its own.
    Window,
    /// A `mailo watch`: the window fetches only when a person asks.
    Watch,
}

/// Whether the background daemon holds the lock that makes it the one account watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Daemon {
    Holding,
    Absent,
}

/// Why the watch is over, when nobody asked it to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lost {
    /// What to do about it: wait, ask for a sign-in, or give up.
    pub retry: Retry,
    /// What to tell a person.
    pub why: String,
}

/// How soon a freshly opened window starts fetching, so that opening it is not also a network
/// round trip competing with the first paint.
pub const BEAT: Duration = Duration::from_secs(2);

/// How soon after being wanted a live watch connects, so that opening a window is not also a
/// second connection racing the first pass.
pub const FIRST: Duration = Duration::from_secs(4);

/// When the scheduler waits, as the host sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How soon the accounts already known first poll.
    pub beat: Duration,
    /// How soon after being wanted a live watch connects.
    pub first: Duration,
    /// The floor under the wait between one lost connection and the next attempt.
    pub retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            beat: BEAT,
            first: FIRST,
            retry: crate::fetch::RETRY_NOW,
        }
    }
}
