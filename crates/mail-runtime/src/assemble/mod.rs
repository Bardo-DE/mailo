//! Raw bytes to a stored [`Message`].
//!
//! This is the seam the protocol crates deliberately cannot cross. A `Message` needs a
//! `MessageId`, a `ThreadId` and a stored `BlobId`, and none of the three is anything the wire
//! supplies — so `mail-proto` hands back `ProtoOutcome::Fetched { remote, raw }` and the work
//! of turning that into something the store will accept happens here, where the blob store,
//! `mail-mime` and the id space all exist.
//!
//! Three stages, so that the middle one can be tested with the store left out:
//!
//! 1. **Parse** ([`parse`]): bytes to [`parse::Parsed`]. Pure.
//! 2. **Resolve** ([`resolve`]): a batch of parses, the answers the store gave about the
//!    conversations it names ([`resolve::ThreadIndex`]) and the ids it is handed
//!    ([`resolve::Ids`]) to an [`Ingest`] and the rows that go with it. Pure.
//! 3. **Commit** ([`commit`]): the blobs are put first (idempotent), then the ingest is written
//!    in one transaction with the rows, so what a message taught about its sender's keys is kept
//!    or lost with the message.
//!
//! The batch is whatever the caller hands in (a fetch band of a few hundred), and its threads
//! are resolved together: what the store is asked is bounded by the batch, never the mailbox.

mod commit;
mod parse;
mod resolve;

use crate::RuntimeError;
use crate::error::Logged;
use chrono::{DateTime, Utc};
use commit::Gather;
use mail_domain::*;
use mail_store::{SqliteStore, Store};
use parse::{Provenance, Reading, learns_from};
use porter_core::AccountId;
use resolve::{Entry, Frame, Ids, Tiebreak, message_key};

/// What arrived, and where from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    pub remote: RemoteRef,
    pub raw: Vec<u8>,
}

/// A batch resolved and ready to write: the ingest, and the rows that are written with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembly {
    pub ingest: Ingest,
    /// What the messages taught about their senders' keys, kept in the transaction the messages
    /// are written in.
    pub rows: Vec<Row>,
}

impl Assembly {
    /// Write it: the commit stage. `now` stamps what the store records itself.
    pub fn write(
        self,
        store: &dyn Store,
        account: AccountId,
        now: DateTime<Utc>,
    ) -> Result<Patch, RuntimeError> {
        commit::write(store, account, self.ingest, &self.rows, now).map_err(RuntimeError::Store)
    }
}

/// Turn fetched bytes into an [`Assembly`] the store can absorb.
///
/// `fallback_date` is the server's own idea of when a message arrived, used when the `Date`
/// header is missing or unparseable. That is common enough in real mail that treating it as an
/// error would reject ordinary messages; it is a fact to record, not a failure.
pub fn assemble(
    store: &SqliteStore,
    account: AccountId,
    mailbox: MailboxRef,
    role: MailboxRole,
    cursor: Option<SyncCursor>,
    arrivals: Vec<Arrival>,
    fallback_date: DateTime<Utc>,
) -> Result<Assembly, RuntimeError> {
    assemble_as(
        store,
        account,
        Destination { mailbox, role },
        cursor,
        arrivals,
        Provenance::Wire,
        fallback_date,
    )
    .map(|assembled| assembled.assembly)
}

/// What [`assemble_as`] made of a batch: the assembly, and which arrivals were not messages.
struct Assembled {
    assembly: Assembly,
    /// The addresses of the arrivals that could not be read.
    unreadable: Vec<RemoteRef>,
}

fn assemble_as(
    store: &SqliteStore,
    account: AccountId,
    into: Destination,
    cursor: Option<SyncCursor>,
    arrivals: Vec<Arrival>,
    provenance: Provenance,
    fallback_date: DateTime<Utc>,
) -> Result<Assembled, RuntimeError> {
    let how = Reading {
        provenance,
        learns: learns_from(into.role),
        at: fallback_date,
    };
    // 1. Parse.
    let mut read = Vec::new();
    let mut unreadable = Vec::new();
    for arrival in arrivals {
        // A message we cannot parse is skipped, not fatal. One malformed message in a maildrop
        // of two thousand must not stop the other 1999 arriving. It is counted and said, though:
        // the pass's cursor moves past it, so nothing will fetch it again.
        match parse::parse(&arrival.raw, how) {
            Ok(parsed) => read.push((arrival, parsed)),
            Err(why) => {
                log::warn!(
                    "a message in {} could not be read and was skipped: {why}",
                    into.mailbox.path
                );
                unreadable.push(arrival.remote);
            }
        }
    }
    // Blobs, and what the store knows of the conversations the batch names.
    let gathered: Vec<Gather<'_>> = read
        .iter()
        .map(|(arrival, parsed)| Gather {
            raw: &arrival.raw,
            parsed,
            remote: Some(&arrival.remote),
        })
        .collect();
    let (blobs, index) = commit::gather(store, &account, &gathered)?;
    // 2. Resolve.
    let frame = Frame {
        account: &account,
        fallback: fallback_date,
    };
    let batch: Vec<(&RemoteRef, &parse::Parsed)> = read
        .iter()
        .map(|(arrival, parsed)| (&arrival.remote, parsed))
        .collect();
    let resolved = resolve::resolve(
        &frame,
        into,
        cursor,
        &index,
        &batch,
        &mut Ids {
            message: &mut MessageId::generate,
            thread: &mut ThreadId::generate,
            blob: &|digest| blobs[digest],
        },
    );
    Ok(Assembled {
        assembly: Assembly {
            ingest: resolved.ingest,
            rows: resolved.rows,
        },
        unreadable,
    })
}

/// One message to keep with no server address, and where the file it came from put it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keep {
    pub raw: Vec<u8>,
    pub placement: mail_archive::Placement,
}

/// What [`keep`] did with a batch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kept {
    /// Messages new to the account.
    pub added: usize,
    /// Messages it already held, by identity; only their labels may have grown.
    pub already: usize,
    /// Bytes that were not a message at all.
    pub unreadable: usize,
}

/// Keep a batch of messages that no server holds, through the same parse, identity and
/// threading as a sync — imported mail, or an upload whose server did not say where it went.
///
/// The identity of a message with no `Message-ID` is a digest of its fields and its bytes, so
/// importing the same file again finds the same identity and keeps nothing twice.
pub fn keep(
    store: &SqliteStore,
    account: AccountId,
    batch: Vec<Keep>,
    fallback_date: DateTime<Utc>,
) -> Result<Kept, RuntimeError> {
    let (kept, unreadable) = prepare(store, account.clone(), batch, fallback_date)?;
    let offered = kept.len();
    let patch = store
        .import(account, Import { messages: kept }, fallback_date)
        .map_err(RuntimeError::Store)?;
    let added = patch
        .changes
        .iter()
        .filter(|c| matches!(c, Change::MessageUpsert(_)))
        .count();
    Ok(Kept {
        added,
        already: offered - added,
        unreadable,
    })
}

/// Keep the copy of a message this client has just submitted, in Sent, for an account whose
/// server files none (POP3): the only copy of what the user sent is the one kept here.
///
/// `raw` is the bytes the submission server was handed, so the copy is what went, not what the
/// draft says now. Read, since the user wrote it. Threaded like any arrival, so a reply joins
/// the conversation it answers. Kept with no server address, so no sync can take it away.
///
/// The stored message, or `None` for bytes that do not parse as a message, which a submission
/// this client built never is.
pub fn sent(
    store: &SqliteStore,
    account: AccountId,
    raw: Vec<u8>,
    at: DateTime<Utc>,
) -> Result<Option<MessageId>, RuntimeError> {
    let placement =
        mail_archive::Placement::new(MailboxRole::Sent, vec![SystemFlag::Seen], Vec::new());
    let (kept, _) = prepare(store, account.clone(), vec![Keep { raw, placement }], at)?;
    let Some(first) = kept.first() else {
        return Ok(None);
    };
    let (fresh, key) = (first.message.id, first.key.clone());
    let patch = store
        .import(account.clone(), Import { messages: kept }, at)
        .map_err(RuntimeError::Store)?;
    let added = patch
        .changes
        .iter()
        .any(|c| matches!(c, Change::MessageUpsert(m) if m.id == fresh));
    if added {
        return Ok(Some(fresh));
    }
    // Already held: the same submission settled twice, or a copy of it already fetched.
    let MessageKey::Rfc(rfc_id) = key else {
        return Ok(None);
    };
    Ok(store
        .message_by_rfc_id(account, &rfc_id)
        .or_log("a message could not be looked up by its Message-ID")
        .flatten())
}

/// Parse, store and thread a batch for [`keep`]: what to import, and how many were not
/// messages at all.
fn prepare(
    store: &SqliteStore,
    account: AccountId,
    batch: Vec<Keep>,
    fallback_date: DateTime<Utc>,
) -> Result<(Vec<mail_domain::Kept>, usize), RuntimeError> {
    // Imported mail teaches nothing about anyone's keys: it is not mail from them to us.
    let how = Reading {
        provenance: Provenance::Wire,
        learns: false,
        at: fallback_date,
    };
    let mut unreadable = 0;
    let mut read = Vec::new();
    for item in batch {
        let Ok(parsed) = parse::parse(&item.raw, how) else {
            unreadable += 1;
            continue;
        };
        read.push((item, parsed));
    }
    let gathered: Vec<Gather<'_>> = read
        .iter()
        .map(|(item, parsed)| Gather {
            raw: &item.raw,
            parsed,
            remote: None,
        })
        .collect();
    let (blobs, index) = commit::gather(store, &account, &gathered)?;
    let frame = Frame {
        account: &account,
        fallback: fallback_date,
    };
    let entries: Vec<Entry<'_>> = read
        .iter()
        .map(|(item, parsed)| Entry {
            parsed,
            key: message_key(&parsed.fields, Tiebreak::Bytes(&parsed.digest)),
            role: item.placement.role,
            provider: None,
        })
        .collect();
    let kept: Vec<mail_domain::Kept> = resolve::messages(
        &frame,
        &index,
        &entries,
        &mut Ids {
            message: &mut MessageId::generate,
            thread: &mut ThreadId::generate,
            blob: &|digest| blobs[digest],
        },
    )
    .into_iter()
    .zip(read)
    .map(|(mut message, (item, parsed))| {
        let placement = item.placement;
        message.read = placement.read();
        message.star = placement.star();
        mail_domain::Kept {
            key: message.key.clone(),
            raw: blobs[&parsed.digest],
            message,
            labels: placement.labels,
        }
    })
    .collect();
    Ok((kept, unreadable))
}

/// Keep a message just uploaded to `into`, at the address the server gave it (`APPENDUID`).
///
/// Recorded now rather than left for the next sync to find, so that importing the same file
/// again sees the message as already there and uploads nothing — and so the next sync, finding
/// this UID already mapped, does not fetch it back.
pub fn appended(
    store: &SqliteStore,
    account: AccountId,
    into: Destination,
    remote: RemoteRef,
    raw: Vec<u8>,
    flags: &[SystemFlag],
    fallback_date: DateTime<Utc>,
) -> Result<Patch, RuntimeError> {
    let Assembly { mut ingest, rows } = assemble_as(
        store,
        account.clone(),
        into,
        None,
        vec![Arrival { remote, raw }],
        Provenance::Wire,
        fallback_date,
    )?
    .assembly;
    for fetched in &mut ingest.messages {
        if flags.contains(&SystemFlag::Seen) {
            fetched.message.read = ReadState::Read;
        }
        if flags.contains(&SystemFlag::Flagged) {
            fetched.message.star = Star::Starred;
        }
    }
    Assembly { ingest, rows }.write(store, account, fallback_date)
}

/// Headers only, for the pass that must not mark anything read.
///
/// Identical to [`assemble`] except the body is [`Body::Absent`] — which is why that variant
/// exists. A message listed from its headers is a real message, not a broken one.
pub fn assemble_headers(
    store: &SqliteStore,
    account: AccountId,
    mailbox: MailboxRef,
    role: MailboxRole,
    cursor: Option<SyncCursor>,
    arrivals: Vec<Arrival>,
    fallback_date: DateTime<Utc>,
) -> Result<Assembly, RuntimeError> {
    assemble_headers_counted(
        store,
        account,
        Destination { mailbox, role },
        cursor,
        arrivals,
        fallback_date,
    )
    .map(|assembled| assembled.assembly)
}

/// [`assemble_headers`], counting what was not a message.
fn assemble_headers_counted(
    store: &SqliteStore,
    account: AccountId,
    into: Destination,
    cursor: Option<SyncCursor>,
    arrivals: Vec<Arrival>,
    fallback_date: DateTime<Utc>,
) -> Result<Assembled, RuntimeError> {
    let mut assembled = assemble_as(
        store,
        account,
        into,
        cursor,
        arrivals,
        Provenance::Wire,
        fallback_date,
    )?;
    for fetched in &mut assembled.assembly.ingest.messages {
        fetched.message.body = Body::Absent;
    }
    Ok(assembled)
}

/// The identity [`keep`] gives a message: what an import asks the store about before it
/// uploads anything, so the answer matches what keeping it would have recorded.
pub fn identity(fields: &mail_mime::Parsed, raw: &[u8]) -> MessageKey {
    message_key(fields, Tiebreak::Bytes(&blake3::hash(raw)))
}

/// Absorb fetched bytes into the store, returning what changed.
pub fn absorb(
    store: &SqliteStore,
    account: AccountId,
    mailbox: MailboxRef,
    cursor: Option<SyncCursor>,
    arrivals: Vec<Arrival>,
    headers_only: bool,
    fallback_date: DateTime<Utc>,
) -> Result<Patch, RuntimeError> {
    absorb_into(
        store,
        account,
        Destination {
            mailbox,
            role: MailboxRole::Inbox,
        },
        cursor,
        arrivals,
        headers_only,
        fallback_date,
    )
}

/// Where a batch of mail is being absorbed: the mailbox, and what that mailbox is *for*.
///
/// One value rather than two arguments because they are one fact — a folder and its role are
/// never chosen independently — and because seven arguments is where this function was already.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub mailbox: MailboxRef,
    pub role: MailboxRole,
}

/// The same, told which role the mailbox serves.
///
/// Every absorbed message used to be `MailboxRole::Inbox` whatever folder it came from, which
/// was invisible while only `INBOX` was ever synced. The moment a second folder is fetched it
/// stops being invisible: mail from Sent would be listed in the inbox and archiving it would be
/// the only way to make it go away.
pub fn absorb_into(
    store: &SqliteStore,
    account: AccountId,
    into: Destination,
    cursor: Option<SyncCursor>,
    arrivals: Vec<Arrival>,
    headers_only: bool,
    fallback_date: DateTime<Utc>,
) -> Result<Patch, RuntimeError> {
    absorb_counted(
        store,
        account,
        into,
        cursor,
        arrivals,
        headers_only,
        fallback_date,
    )
    .map(|absorbed| absorbed.patch)
}

/// What [`absorb_counted`] did with a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Absorbed {
    /// What changed in the store.
    pub patch: Patch,
    /// Arrivals that could not be read as messages, and were skipped (and logged). A pass says
    /// how many, because its cursor has moved past them and nothing will fetch them again.
    pub unreadable: usize,
    /// Where each of them is on the server, so a pass can leave them be for a while rather than
    /// ask for the same bytes again.
    pub skipped: Vec<RemoteRef>,
}

/// [`absorb_into`], saying how many arrivals were not messages: what a pass absorbs through,
/// so the count reaches the person and not only the log.
pub fn absorb_counted(
    store: &SqliteStore,
    account: AccountId,
    into: Destination,
    cursor: Option<SyncCursor>,
    arrivals: Vec<Arrival>,
    headers_only: bool,
    fallback_date: DateTime<Utc>,
) -> Result<Absorbed, RuntimeError> {
    let Assembled {
        assembly,
        unreadable,
    } = if headers_only {
        assemble_headers_counted(
            store,
            account.clone(),
            into,
            cursor,
            arrivals,
            fallback_date,
        )?
    } else {
        assemble_as(
            store,
            account.clone(),
            into,
            cursor,
            arrivals,
            Provenance::Wire,
            fallback_date,
        )?
    };
    let patch = assembly.write(store, account, fallback_date)?;
    Ok(Absorbed {
        patch,
        unreadable: unreadable.len(),
        skipped: unreadable,
    })
}

/// Absorb messages rebuilt from their sections, reading which parts were left on the server.
///
/// A separate entry point rather than a flag on [`Arrival`], so that a caller holding bytes off
/// the wire has no way to ask for them to be believed.
pub fn absorb_rebuilt_into(
    store: &SqliteStore,
    account: AccountId,
    into: Destination,
    arrivals: Vec<Arrival>,
    fallback_date: DateTime<Utc>,
) -> Result<Patch, RuntimeError> {
    assemble_as(
        store,
        account.clone(),
        into,
        None,
        arrivals,
        Provenance::Rebuilt,
        fallback_date,
    )?
    .assembly
    .write(store, account, fallback_date)
}
