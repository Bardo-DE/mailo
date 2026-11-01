//! Driving a JMAP account: one pass of [`mail_runtime::JmapEngine`].
//!
//! The pass itself is the engine's — the mailbox list, changes, headers, bodies, the outbox — so
//! all that is here is saying where it began and typing what comes back. Waiting between passes
//! is the scheduler's, over [`super::live`]'s event source.

use super::report::{Done, Emit, Failure, Progress};
use super::{Configured, arrival, say};
use mail_runtime::JmapEngine;

/// One pass, with the arrivals acted on between its fetches and its outbox, as an IMAP pass's
/// are: the engine stops short of the outbox ([`JmapEngine::pass_until_outbox`]), the account's
/// rules and junk filter run over what arrived ([`super::arrival`]), and the outbox then drains
/// what they queued.
pub(super) async fn drive(
    engine: &mut JmapEngine,
    account: &Configured,
    cancel: &mut mail_runtime::Cancel,
    now: chrono::DateTime<chrono::Utc>,
    emit: Emit<'_>,
) -> Result<Done, Failure> {
    // One request round covers the account, so there is nothing finer to say than that it began.
    say(emit, Progress::Connecting);
    let midway = engine
        .pass_until_outbox(cancel, now)
        .await
        .map_err(|e| Failure::of("", &e))?;
    let mut acted = Done::default();
    if let Some(caps) = &midway.caps
        && !midway.stopped
    {
        arrival(
            engine.store(),
            account,
            caps,
            &midway.report.arrived,
            now,
            &mut acted,
        );
    }
    let report = engine.outbox(cancel, now, midway).await;
    let mut done = Done::from_report(report);
    done.counts.ruled.extend(acted.counts.ruled);
    done.counts.noted.extend(acted.counts.noted);
    done.trouble.extend(acted.trouble);
    Ok(done)
}
