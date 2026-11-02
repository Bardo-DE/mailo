//! What a pass says, as what the link hears.
//!
//! The pass reports in its own vocabulary ([`Progress`], [`PassEnd`]) because it is written
//! against the engines; the link reads [`Event`]s because it is written against the screen.
//! Everything that turns one into the other is here, where a table can test it, and the window's
//! runner only moves the results.

use super::{PassEnd, Progress, hold, person_asked};
use crate::fetch::{Count, Event, Pause, Step, Trouble};
use mail_domain::Retry;
use porter_core::AccountId;
use std::time::Duration;

impl Progress {
    /// Where a pass has got to, as the link records it.
    pub fn event(self) -> Event {
        stepped(self)
    }
}

impl PassEnd {
    /// How the pass ended, as the one event the link hears.
    ///
    /// A pass that ran can still have been refused, or told to slow down, by the folder it was
    /// in the middle of; those are the two outcomes that must not read as "up to date". The
    /// first is a credential the server has already rejected, which polling again would try on
    /// it every interval, and the second is a wait the server named. They become failures, the
    /// refusal ranking first.
    pub fn event(self) -> Event {
        ended(self)
    }
}

fn stepped(progress: Progress) -> Event {
    let counted = |done: u32, of: Option<u32>| of.map(|of| Count { done, of });
    let (step, count) = match progress {
        Progress::Connecting => (Step::Connecting, None),
        Progress::Folders => (Step::Folders, None),
        Progress::Headers { mailbox, done, of } => (Step::Headers { mailbox }, counted(done, of)),
        Progress::Flags { .. } => (Step::Flags, None),
        Progress::Bodies { done, of } => (Step::Bodies, counted(done, of)),
        Progress::Sending => (Step::Sending, None),
    };
    Event::Stepped(step, count)
}

fn ended(end: PassEnd) -> Event {
    match end {
        PassEnd::Finished(report) => {
            if let Some(asked) = person_asked(&report.trouble) {
                return Event::Failed {
                    retry: asked.retry.clone(),
                    why: asked.said(),
                    pause: Pause::ServerBusy,
                };
            }
            match hold(&report.trouble) {
                Some(wait) => Event::Failed {
                    retry: Retry::After(wait),
                    why: "The server asked to be left alone for a while".to_owned(),
                    pause: Pause::Throttled,
                },
                None => Event::Finished {
                    trouble: report.trouble.into_iter().map(Trouble::worded).collect(),
                },
            }
        }
        PassEnd::Failed {
            retry, why, pause, ..
        } => Event::Failed { retry, why, pause },
        PassEnd::Cancelled { .. } => Event::Cancel,
    }
}

/// What a whole run did to `account`: its own end, or the run's failure to happen at all.
///
/// A run that could not start (no registry, a task that panicked) is worth trying again, as it
/// always was: a laptop lid is the usual cause. It waits the interval, doubling, like any
/// failure, and says nothing of a server because none was reached.
pub fn outcome<E: std::fmt::Display>(done: Result<Vec<PassEnd>, E>, account: AccountId) -> Event {
    match done {
        Ok(ends) => ends
            .into_iter()
            .find(|end| end.account() == account)
            .map_or(
                Event::Finished {
                    trouble: Vec::new(),
                },
                ended,
            ),
        Err(why) => Event::Failed {
            retry: Retry::After(Duration::ZERO),
            why: why.to_string(),
            pause: Pause::ServerBusy,
        },
    }
}
