//! A draft's copy in the server's Drafts mailbox: what is queued to replace it, and to remove it.
//!
//! Apple Mail and Thunderbird keep the draft being written in Drafts on the server, replace it as
//! it changes and remove it once it is sent or thrown away, so a reply started on one device can
//! be finished on another. The copy is the server's alone: it is never brought in here as a
//! message (Drafts is not followed by a sync), and the draft this client holds stays the
//! document. Every copy carries the draft's `Message-ID` ([`mail_mime::draft_message_id`]), which
//! is how each operation finds the ones before it.

use crate::RuntimeError;
use chrono::{DateTime, Utc};
use mail_domain::{
    AccountCaps, ArchiveMeans, ChangeId, Draft, DraftId, MailboxRef, Patch, RemoteIntent, SendState,
};
use mail_store::{SqliteStore, Store, StoreError};

/// Whether an account with these capabilities has its drafts kept on its server at all.
///
/// Not on Gmail (an archive that drops the inbox label): its IMAP `EXPUNGE` in `[Gmail]/Drafts`
/// may only take the Drafts label off an old copy and leave it in All Mail, so every autosave
/// could leave one more copy there. Until that is checked on a real account, a Gmail draft is
/// this client's alone, as it was before copies were kept.
pub fn keeps_copies(caps: &AccountCaps) -> bool {
    !matches!(caps.archive, ArchiveMeans::DropInbox)
}

/// The path of the server's Drafts mailbox where an account on `incoming`, with these
/// capabilities, has its drafts kept: the one the server named, on a protocol whose server keeps
/// the copies a client uploads ([`mail_domain::Caps::server_drafts`]: IMAP and JMAP, not POP3,
/// local folders or Graph, whose draft has an id of its own) and not on Gmail ([`keeps_copies`]).
///
/// The one rule of whether a draft is kept on the server at all, which keeping it, replacing it
/// and dropping it once sent all ask. A JMAP account's mailbox is read from the live list rather
/// than from `caps`, and asks only the first half.
pub fn drafts_folder(incoming: &mail_domain::Incoming, caps: &AccountCaps) -> Option<String> {
    if !incoming.caps().server_drafts || !keeps_copies(caps) {
        return None;
    }
    caps.folders
        .path(mail_domain::MailboxRole::Drafts)
        .map(str::to_owned)
}

/// The `Message-ID` every copy of `draft` carries, without angle brackets; `None` when the
/// identity it is written from is gone, and with it the domain the id was made with.
pub fn message_id(store: &SqliteStore, draft: &Draft) -> Result<Option<String>, RuntimeError> {
    Ok(store
        .identity(draft.identity)?
        .map(|identity| mail_mime::draft_message_id(draft, &identity)))
}

/// The bytes of the copy of `draft` for the server's Drafts mailbox, made from the draft as it
/// stands now: when an upload runs, not when it was queued, so the outbox holds no bytes for it
/// and an upload that waited sends the latest.
///
/// `None` when there is nothing to put there any more: the draft is gone, or on its way out
/// (its copy goes when it is sent), or now asks to be encrypted, which is never put on a server
/// in the clear, or its identity is gone. The upload then does nothing; whatever was queued
/// beside it (a removal) takes the copy away.
pub fn copy(store: &SqliteStore, draft: DraftId) -> Result<Option<Vec<u8>>, RuntimeError> {
    let draft = match store.draft(draft) {
        Ok(draft) => draft,
        Err(StoreError::NoDraft(_)) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if draft.state != SendState::Editing
        || draft.openpgp.is_some_and(mail_domain::Mode::encrypts)
        || draft.smime.is_some_and(mail_domain::Mode::encrypts)
    {
        return Ok(None);
    }
    let Some(identity) = store.identity(draft.identity)? else {
        return Ok(None);
    };
    // The message it answers, for its threading headers; a copy without them is still a copy.
    let parent = match draft.in_reply_to.map(|id| store.message(id)).transpose() {
        Ok(parent) => parent,
        Err(e) => {
            log::warn!("the message a draft answers could not be read: {e}");
            None
        }
    };
    let mut parts = Vec::with_capacity(draft.attachments.len());
    for attachment in &draft.attachments {
        parts.push((attachment.blob, store.blobs().get(attachment.blob)?));
    }
    Ok(Some(mail_mime::draft_copy(
        &draft,
        &identity,
        parent.as_ref(),
        &parts,
    )?))
}

/// Queue the replacement of `draft`'s copy in the Drafts mailbox at `mailbox` with one made from
/// it when the upload runs ([`copy`]). An upload of it still waiting goes (`Store::enqueue`).
pub fn queue_keep(
    store: &SqliteStore,
    draft: &Draft,
    mailbox: MailboxRef,
    now: DateTime<Utc>,
) -> Result<bool, RuntimeError> {
    let Some(message_id) = message_id(store, draft)? else {
        return Ok(false);
    };
    queue(
        store,
        draft,
        RemoteIntent::KeepDraft {
            draft: draft.id,
            mailbox,
            message_id,
        },
        now,
    )
}

/// Queue the removal of every copy of `draft` from the Drafts mailbox at `mailbox`: it was sent,
/// or discarded. An upload of it still waiting goes too.
pub fn queue_drop(
    store: &SqliteStore,
    draft: &Draft,
    mailbox: MailboxRef,
    now: DateTime<Utc>,
) -> Result<bool, RuntimeError> {
    let Some(message_id) = message_id(store, draft)? else {
        return Ok(false);
    };
    queue(
        store,
        draft,
        RemoteIntent::DropDraft {
            draft: draft.id,
            mailbox,
            message_id,
        },
        now,
    )
}

fn queue(
    store: &SqliteStore,
    draft: &Draft,
    intent: RemoteIntent,
    now: DateTime<Utc>,
) -> Result<bool, RuntimeError> {
    let queued = store.enqueue(
        draft.account.clone(),
        intent,
        // Nothing to undo: the copy changes nothing here, before or after it is made.
        &Patch {
            id: ChangeId::generate(),
            changes: Vec::new(),
        },
        now,
    )?;
    Ok(queued.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::{
        Condstore, ConnectionBudget, ExpungeMeans, FolderRoles, Incoming, MailboxRole, MoveExt,
        ServerLabels, ServerThreads, Supported, Tls, WatchMode,
    };

    fn caps(archive: ArchiveMeans) -> AccountCaps {
        AccountCaps {
            labels: ServerLabels::LocalOnly,
            threads: ServerThreads::Jwz,
            watch: WatchMode::Idle,
            archive,
            folders: FolderRoles(vec![("Drafts".to_owned(), MailboxRole::Drafts)]),
            condstore: Condstore::Absent,
            move_ext: MoveExt::Absent,
            expunge: ExpungeMeans::Forbidden,
            top: Supported::Absent,
            pipelining: Supported::Absent,
            connections: ConnectionBudget::default(),
            observed_at: chrono::DateTime::from_timestamp(1_790_000_000, 0).unwrap(),
        }
    }

    fn imap() -> Incoming {
        Incoming::Imap {
            host: "imap.example.test".to_owned(),
            port: 993,
            tls: Tls::Implicit,
        }
    }

    /// Whether a draft is kept on the server at all, for every protocol and every archive.
    #[test]
    fn a_draft_is_kept_where_the_server_keeps_copies_and_named_a_drafts_folder() {
        let folder = Some("Drafts".to_owned());
        let plain = caps(ArchiveMeans::MoveToFolder("Archive".to_owned()));
        assert_eq!(drafts_folder(&imap(), &plain), folder);
        // Gmail's EXPUNGE may only take a label off an old copy.
        assert_eq!(drafts_folder(&imap(), &caps(ArchiveMeans::DropInbox)), None);
        // No protocol but IMAP and JMAP has a server that keeps what is uploaded.
        for incoming in [Incoming::Local, Incoming::Graph] {
            assert_eq!(drafts_folder(&incoming, &plain), None, "{incoming:?}");
        }
        // No folder named, nothing to keep a copy in: a guessed path is how a copy is lost.
        let nameless = AccountCaps {
            folders: FolderRoles::default(),
            ..plain
        };
        assert_eq!(drafts_folder(&imap(), &nameless), None);
    }
}
