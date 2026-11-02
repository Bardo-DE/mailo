//! What a sync pass found, kept as data.
//!
//! Each account's pass ends as a [`PassEnd`], and what the caller needs to *act* on is in it as
//! values: which account is waiting on a sign-in, which is backing off and for how long, and
//! which folder is at fault. None of that can be read back out of a sentence, so no sentence is
//! made here; the window and the command line each word a [`PassEnd`] for themselves.

use std::collections::BTreeMap;
use std::time::Duration;

use mail_domain::{MessageId, Retry, Retryable};
use mail_net::NetError;
use mail_proto::ProtoError;
use mail_runtime::pass::Verdict;
use mail_runtime::{RuntimeError, SyncReport};
use porter_core::AccountId;
use tokio::sync::watch;

use crate::fetch::Pause;
pub use crate::fetch::Trouble;

/// What one account's pass fetched and settled. [`SyncReport`] without its trouble, which is
/// kept as [`Trouble`] so that each piece of it carries its decision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub headers_fetched: usize,
    pub bodies_fetched: usize,
    /// Attachments kept offline by this pass.
    pub parts_fetched: usize,
    pub outbox_settled: usize,
    /// Messages handed to the submission server and accepted.
    pub submitted: usize,
    /// Messages uploaded into a mailbox by the outbox.
    pub appended: usize,
    /// Still queued when the pass ended, waiting on a retry.
    pub still_queued: usize,
    /// Messages this pass stored for the first time, in order.
    pub arrived: Vec<MessageId>,
    /// Arrived messages a rule acted on, with the rules that did.
    pub ruled: Vec<(MessageId, Vec<String>)>,
    /// Arrived messages a rule asked to be told of, however quiet they would otherwise be.
    pub noted: Vec<MessageId>,
}

/// What a pass found on one account that reached the server and ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountReport {
    pub account: AccountId,
    pub address: String,
    pub counts: Counts,
    pub trouble: Vec<Trouble>,
}

/// How one account's pass ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassEnd {
    /// It ran. Some of it may still have gone wrong: see [`AccountReport::trouble`].
    Finished(AccountReport),
    /// It could not run at all: no credential, no connection, a refused sign-in.
    Failed {
        account: AccountId,
        address: String,
        retry: Retry,
        why: String,
        /// Why the server is to be left alone, when `retry` says to wait.
        pause: Pause,
        /// What the person can do about it that neither waiting nor signing in again would.
        offer: Option<Offer>,
    },
    /// The caller's cancel signal ended it.
    Cancelled { account: AccountId, address: String },
}

/// What a person can do about a pass that failed, when it is neither waiting nor signing in
/// again: the one action the window offers beside the failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Set the account up again to read and send through Microsoft Graph: Microsoft refused it
    /// mail over IMAP (`account::switch_to_graph`).
    UseGraph,
}

/// The server will not serve the account over the protocol it was reached by: it refused a
/// sign-in its issuer stands by (`RuntimeError::RefusedFresh`), or listed no inbox
/// (`RuntimeError::NoInbox`). Nothing about the credential or the network, so no retry and no
/// new sign-in changes it; what the account is set up with might.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProtocolRefused;

/// A pass's failure to run, before it knows which account it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    pub retry: Retry,
    pub why: String,
    /// What kind of wait `retry` asks for. Said only where the error is classified: the delay
    /// alone cannot tell a rate limit from a dropped connection.
    pub pause: Pause,
    /// Whether it was the protocol, not the account, that was refused. Kept while the error is
    /// still typed, so the account's [`Offer`] can be decided once the account is known.
    pub refused: Option<ProtocolRefused>,
}

/// An error that knows why a wait after it is a wait.
pub(crate) trait Pauses {
    /// What the server did, as far as this error can tell.
    fn pause(&self) -> Pause;

    /// Whether the server refused the protocol itself ([`ProtocolRefused`]).
    fn refused(&self) -> Option<ProtocolRefused> {
        None
    }
}

impl Pauses for RuntimeError {
    fn pause(&self) -> Pause {
        match self {
            RuntimeError::Connect(_)
            | RuntimeError::Io(_)
            | RuntimeError::Stalled { .. }
            | RuntimeError::Tls(_)
            | RuntimeError::Proto(ProtoError::UnexpectedEof) => Pause::Unreachable,
            RuntimeError::Proto(ProtoError::Throttled { .. }) => Pause::Throttled,
            // A transient refusal is the server saying "not now". Everything else that waits is
            // a server that answered, and answered badly. Graph folds a `429` in with `503`
            // (`graph::refused`), so the two cannot be told apart here and are not guessed at.
            _ => Pause::ServerBusy,
        }
    }

    fn refused(&self) -> Option<ProtocolRefused> {
        matches!(self, RuntimeError::RefusedFresh(_) | RuntimeError::NoInbox)
            .then_some(ProtocolRefused)
    }
}

impl Pauses for mail_secrets::CredentialError {
    fn pause(&self) -> Pause {
        match self {
            mail_secrets::CredentialError::Io(_) => Pause::Unreachable,
            mail_secrets::CredentialError::Net(
                NetError::Connect(_)
                | NetError::Io(_)
                | NetError::Stalled { .. }
                | NetError::Tls(_),
            ) => Pause::Unreachable,
            _ => Pause::ServerBusy,
        }
    }
}

/// A store failure is ours, not the server's, and none of the three says so. A wait after one is
/// the same wait, so it takes the mildest.
impl Pauses for mail_store::StoreError {
    fn pause(&self) -> Pause {
        Pause::ServerBusy
    }
}

/// Where a pass is, for a window to show. Emitted as each step starts and as it finishes; a
/// step the server gives no total for has `of: None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Connecting,
    Folders,
    Headers {
        mailbox: String,
        done: u32,
        of: Option<u32>,
    },
    Flags {
        mailbox: String,
    },
    Bodies {
        done: u32,
        of: Option<u32>,
    },
    Sending,
}

/// Where one account's pass reports progress.
pub(crate) type Emit<'a> = Option<&'a dyn Fn(Progress)>;

/// What a watch reports as it goes, for its caller to say.
///
/// A watch has no end to return a report from, so each thing it would have said is handed over
/// as it happens. The caller words them; none of them is a sentence yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watched {
    /// One account's pass ended.
    Pass(PassEnd),
    /// Waiting on the server failed. The watch connects again, further apart each time.
    WaitFailed { address: String, why: String },
    /// A pass could not start at all, for a reason that is not an account's (no client registry,
    /// a task that stopped). It is tried again at the account's interval, doubling.
    RunFailed { why: String },
    /// What a pass fetched could not be announced on the desktop.
    AnnounceFailed { address: String, why: String },
    /// The follow-up reminders could not be swept.
    RemindersFailed { why: String },
}

/// What a caller can hand a pass beyond the accounts it is to run.
#[derive(Default)]
pub struct Hooks<'a> {
    /// Told of each account's progress as the pass goes.
    pub progress: Option<&'a dyn Fn(AccountId, Progress)>,
    /// A signal per account that ends its pass. An account with none runs to the end.
    pub cancel: BTreeMap<AccountId, watch::Receiver<bool>>,
    /// What the window is about to read and fetching itself, which the pass's body fetches go
    /// first and around. Nothing, for a pass nobody's window is watching.
    pub wanted: mail_runtime::wanted::Wanted,
    /// When the pass began, as its caller counts it; the handle's clock when not said.
    pub now: Option<chrono::DateTime<chrono::Utc>>,
}

impl std::fmt::Debug for Hooks<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hooks")
            .field("progress", &self.progress.map(|_| "a sink"))
            .field("cancel", &self.cancel.keys().collect::<Vec<_>>())
            .field("now", &self.now)
            .finish()
    }
}

impl Failure {
    pub fn fatal(why: impl Into<String>) -> Self {
        let why = why.into();
        Failure {
            retry: Retry::Fatal(why.clone()),
            why,
            pause: Pause::ServerBusy,
            refused: None,
        }
    }

    /// A credential that is missing or refused: nothing to wait for, so the pause is not read.
    pub fn reauth(why: impl Into<String>) -> Self {
        Failure {
            retry: Retry::NeedsReauth,
            why: why.into(),
            pause: Pause::ServerBusy,
            refused: None,
        }
    }

    /// A typed error, with `prefix` before what it says.
    pub fn of<E: Retryable + Pauses + std::fmt::Display>(prefix: &str, e: &E) -> Self {
        Failure {
            retry: e.retry(),
            why: format!("{prefix}{e}"),
            pause: e.pause(),
            refused: e.refused(),
        }
    }
}

/// What the passes of one account's steps gathered: [`Counts`] and the trouble so far.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Done {
    pub counts: Counts,
    pub trouble: Vec<Trouble>,
}

impl Done {
    /// Record a failure, worded for a person.
    pub fn trouble(&mut self, mailbox: Option<&str>, retry: Retry, why: String) {
        self.trouble.push(Trouble {
            mailbox: mailbox.map(str::to_owned),
            retry,
            why: Some(why),
        });
    }

    /// Record a note an engine worded itself. It carries no classification of its own: it is an
    /// operation given up on, which is what the engine surfaces rather than retries.
    pub fn notes(&mut self, mailbox: Option<&str>, notes: Vec<String>) {
        for note in notes {
            self.trouble(mailbox, Retry::Fatal(note.clone()), note);
        }
    }

    /// Record the refusals an engine's own report carries as flags.
    pub fn flags(&mut self, report: &SyncReport) {
        for retry in report.verdict.retries() {
            self.trouble.push(Trouble {
                mailbox: None,
                retry,
                why: None,
            });
        }
    }

    /// An engine's whole report, as data.
    pub fn from_report(report: SyncReport) -> Self {
        let mut done = Done {
            counts: Counts {
                headers_fetched: report.headers_fetched,
                bodies_fetched: report.bodies_fetched,
                parts_fetched: report.parts_fetched,
                outbox_settled: report.outbox_settled,
                submitted: report.submitted,
                appended: report.appended,
                still_queued: report.still_queued,
                arrived: report.arrived.clone(),
                ruled: report.ruled.clone(),
                noted: Vec::new(),
            },
            trouble: Vec::new(),
        };
        done.flags(&report);
        done.notes(None, report.needs_attention);
        done
    }

    pub fn of(self, account: &super::Configured) -> AccountReport {
        AccountReport {
            account: account.id.clone(),
            address: account.address.clone(),
            counts: self.counts,
            trouble: self.trouble,
        }
    }
}

/// The first of `trouble` only the person can answer: a refused credential, or a grant gone.
pub(crate) fn person_asked(trouble: &[Trouble]) -> Option<&Trouble> {
    trouble.iter().find(|t| t.retry.needs_person())
}

/// Whether any of `trouble` is one only the person can answer.
pub(crate) fn needs_person(trouble: &[Trouble]) -> bool {
    person_asked(trouble).is_some()
}

/// The longest wait any of `trouble` asked for.
pub(crate) fn hold(trouble: &[Trouble]) -> Option<Duration> {
    Verdict::of(trouble.iter().map(|t| &t.retry)).hold
}

impl PassEnd {
    pub fn account(&self) -> AccountId {
        match self {
            PassEnd::Finished(report) => report.account.clone(),
            PassEnd::Failed { account, .. } | PassEnd::Cancelled { account, .. } => account.clone(),
        }
    }
}

mod event;
pub use event::outcome;
#[cfg(test)]
mod event_tests;
#[cfg(test)]
mod tests;
