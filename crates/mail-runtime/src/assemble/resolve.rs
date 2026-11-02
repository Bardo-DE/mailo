//! Stage two of assembly: what only a batch can say about its messages.
//!
//! A [`Parsed`] knows what its bytes say. It does not know which conversation it belongs to
//! (that depends on the rest of the batch and on what is stored), what it is called here, or which
//! blob holds it. This stage gives it all three, as a pure function of the batch, a
//! [`ThreadIndex`] (the answers the store gave to the questions this batch asks) and the ids it
//! is handed ([`Ids`]), and returns what to write: an [`Ingest`] and the rows that go with it.
//! No store, no clock, no random id is touched, so a batch can be assembled in a test with the
//! store left out.

use super::Destination;
use super::parse::{Parsed, learns_from};
use chrono::{DateTime, Utc};
use mail_domain::autocrypt::{Sighting, effective_date};
use mail_domain::threading::{self, PROVIDER_PREFIX};
use mail_domain::{
    Address, Attachment, BlobId, Body, Fetched, Ingest, KeySource, MailboxRole, Message, MessageId,
    MessageKey, PartContent, ReadState, RemoteRef, Row, Star, SyncCursor, Taught, ThreadId,
    UidValidity,
};
use porter_core::AccountId;
use std::collections::HashMap;

/// What the store already holds that a batch's threading asks about: the conversation a
/// `Message-ID` or a server's thread id belongs to, and the thread id the server gave an address
/// before. Gathered for the batch about to be resolved, from the ids that batch names, so it is
/// as big as the batch and never as big as the mailbox.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct ThreadIndex {
    /// Normalized `Message-ID` to the conversation holding it.
    pub by_rfc_id: HashMap<String, ThreadId>,
    /// A server's own thread id to the conversation holding it.
    pub by_provider: HashMap<String, ThreadId>,
    /// The server thread id already held against an address.
    pub provider_of: HashMap<RemoteRef, String>,
}

impl ThreadIndex {
    /// What threading asks: a `Message-ID`, or a server's thread id as [`threading::provider_key`]
    /// spells it.
    fn known(&self, id: &str) -> Option<ThreadId> {
        match id.strip_prefix(PROVIDER_PREFIX) {
            Some(thrid) => self.by_provider.get(thrid).copied(),
            None => self.by_rfc_id.get(id).copied(),
        }
    }

    /// Every `Message-ID` that threading may ask the store about, for these messages.
    pub fn rfc_ids_asked<'a>(parsed: impl Iterator<Item = &'a Parsed>) -> Vec<String> {
        let mut ids = Vec::new();
        for parsed in parsed {
            let fields = &parsed.fields;
            let named = fields
                .rfc_message_id
                .iter()
                .chain(&fields.in_reply_to)
                .chain(&fields.references);
            for raw in named {
                let id = threading::normalize_id(raw);
                if !id.is_empty() && !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        ids
    }
}

/// The ids a resolution is given, because it may not make them.
pub(super) struct Ids<'a> {
    /// The id of the next message.
    pub message: &'a mut dyn FnMut() -> MessageId,
    /// The id of the next conversation that is new.
    pub thread: &'a mut dyn FnMut() -> ThreadId,
    /// The blob holding the bytes with this digest, which was stored before the batch was
    /// resolved.
    pub blob: &'a dyn Fn(&blake3::Hash) -> BlobId,
}

/// What a batch is being resolved into.
pub(super) struct Frame<'a> {
    pub account: &'a AccountId,
    /// The server's idea of when a message arrived, used when its `Date` is missing.
    pub fallback: DateTime<Utc>,
}

/// One readable message of a batch: its parse, the identity it is deduplicated by, and where it
/// is filed.
pub(super) struct Entry<'a> {
    pub parsed: &'a Parsed,
    pub key: MessageKey,
    pub role: MailboxRole,
    /// The thread id the server gave it, where it gives one.
    pub provider: Option<&'a str>,
}

/// What a batch resolves to.
pub(super) struct Resolved {
    pub ingest: Ingest,
    /// Written in the ingest's transaction: what the messages taught about keys.
    pub rows: Vec<Row>,
}

/// Resolve a batch that came from `into` on a server: each message with the address it was
/// fetched from.
pub(super) fn resolve(
    frame: &Frame<'_>,
    into: Destination,
    cursor: Option<SyncCursor>,
    index: &ThreadIndex,
    batch: &[(&RemoteRef, &Parsed)],
    ids: &mut Ids<'_>,
) -> Resolved {
    let Destination { mailbox, role } = into;
    let entries: Vec<Entry<'_>> = batch
        .iter()
        .map(|(remote, parsed)| Entry {
            parsed,
            key: message_key(&parsed.fields, Tiebreak::Remote(remote)),
            role,
            provider: index.provider_of.get(*remote).map(String::as_str),
        })
        .collect();
    let messages: Vec<Fetched> = messages(frame, index, &entries, ids)
        .into_iter()
        .zip(batch)
        .map(|(message, (remote, parsed))| Fetched {
            remote: (*remote).clone(),
            key: message.key.clone(),
            raw: (ids.blob)(&parsed.digest),
            message,
        })
        .collect();
    let rows = if learns_from(role) {
        batch
            .iter()
            .flat_map(|(_, parsed)| taught(frame, parsed))
            .map(Row::KeyLearned)
            .collect()
    } else {
        Vec::new()
    };
    Resolved {
        ingest: Ingest {
            mailbox,
            validity: UidValidity::Same,
            cursor,
            messages,
            flags: Vec::new(),
            flag_colors: Vec::new(),
            labels: Vec::new(),
            label_names: Vec::new(),
            provider_threads: Vec::new(),
            gone: Vec::new(),
        },
        rows,
    }
}

/// What one message taught about its sender's keys: the sighting for the peer-state rules, with
/// the key its header carried, and the certificate its signature proved.
fn taught(frame: &Frame<'_>, parsed: &Parsed) -> Vec<Taught> {
    let fields = &parsed.fields;
    let Some(from) = &fields.from else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let address = from.email.trim();
    if !address.is_empty() {
        let at = effective_date(fields.date, frame.fallback);
        out.push(Taught::Autocrypt {
            sighting: Sighting {
                address: address.to_owned(),
                at,
                header: parsed
                    .autocrypt
                    .as_ref()
                    .map(|h| (h.key.fingerprint(), h.prefer_encrypt)),
            },
            key: parsed
                .autocrypt
                .as_ref()
                .map(|h| h.key.record(KeySource::Autocrypt, at, &[address])),
        });
    }
    out.extend(parsed.signer.iter().cloned().map(Taught::Signer));
    out
}

/// Turn parsed messages into domain [`Message`]s: threaded together and onto what is stored,
/// attachments held, unread and unstarred.
pub(super) fn messages(
    frame: &Frame<'_>,
    index: &ThreadIndex,
    entries: &[Entry<'_>],
    ids: &mut Ids<'_>,
) -> Vec<Message> {
    // Thread them together, and onto conversations already stored. `index` is what makes an
    // incremental sync join rather than renumber.
    let inputs: Vec<threading::ThreadInput<'_>> = entries
        .iter()
        .map(|e| threading::ThreadInput {
            message_id: e.parsed.fields.rfc_message_id.as_deref(),
            in_reply_to: e.parsed.fields.in_reply_to.as_deref(),
            references: &e.parsed.fields.references,
            subject: &e.parsed.fields.subject,
            provider: e.provider,
        })
        .collect();
    let threads = threading::thread_with(&inputs, &|id| index.known(id), &mut *ids.thread);

    entries
        .iter()
        .zip(threads)
        .map(|(entry, thread)| {
            let Parsed {
                fields,
                digest,
                part_digests,
                ..
            } = entry.parsed;
            let from = fields.from.clone().unwrap_or_else(|| Address {
                // A message with no From is malformed and still has to be listable; inventing a
                // plausible-looking sender would be worse than an obviously empty one.
                name: None,
                email: String::new(),
            });
            let attachments = fields
                .attachments
                .iter()
                .zip(part_digests)
                .map(|(part, held)| {
                    // Left on the server: nothing to store, and its size is the server's figure.
                    let (content, size) = match (&part.remote, held) {
                        (Some(remote), _) => (
                            PartContent::Remote {
                                section: remote.section.clone(),
                            },
                            remote.octets,
                        ),
                        (None, held) => {
                            let digest = held.unwrap_or_else(|| blake3::hash(&part.bytes));
                            (
                                PartContent::Held((ids.blob)(&digest)),
                                part.bytes.len() as u64,
                            )
                        }
                    };
                    Attachment {
                        name: part.name.clone(),
                        mime: part.mime.clone(),
                        size,
                        content,
                        inline: part.inline.clone(),
                    }
                })
                .collect();
            Message {
                id: (ids.message)(),
                thread,
                account: frame.account.clone(),
                key: entry.key.clone(),
                date: fields.date.unwrap_or(frame.fallback),
                from,
                reply_to: fields.reply_to.clone(),
                to: fields.to.clone(),
                cc: fields.cc.clone(),
                bcc: fields.bcc.clone(),
                subject: fields.subject.clone(),
                in_reply_to: fields.in_reply_to.clone(),
                references: fields.references.clone(),
                rfc_message_id: fields.rfc_message_id.clone(),
                read: ReadState::Unread,
                star: Star::Unstarred,
                flag: mail_domain::Hue::Red,
                mailbox: entry.role,
                labels: Vec::new(),
                body: Body::Present {
                    text: fields.text.clone(),
                    raw: (ids.blob)(digest),
                },
                attachments,
            }
        })
        .collect()
}

/// What separates two messages whose fields are identical and which have no `Message-ID`.
pub(super) enum Tiebreak<'a> {
    /// Where the server holds it.
    Remote(&'a RemoteRef),
    /// A digest of its bytes, for a message no server holds.
    Bytes(&'a blake3::Hash),
}

/// The identity a message is deduplicated by.
///
/// `Message-ID` where there is one. Where there is not — and real mail frequently has none —
/// a digest of the fields that do exist, so two copies of the same message still collapse
/// instead of arriving twice.
pub(super) fn message_key(fields: &mail_mime::Parsed, tiebreak: Tiebreak<'_>) -> MessageKey {
    if let Some(id) = &fields.rfc_message_id {
        return MessageKey::Rfc(id.clone());
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(fields.subject.as_bytes());
    hasher.update(
        fields
            .from
            .as_ref()
            .map(|a| a.email.as_str())
            .unwrap_or("")
            .as_bytes(),
    );
    if let Some(date) = fields.date {
        hasher.update(date.to_rfc3339().as_bytes());
    }
    // The address is included only as a last resort tiebreak: without it, two genuinely
    // different messages with no Message-ID, the same subject, sender and timestamp would
    // collapse into one and the user would silently lose mail. A message no server holds has
    // no address, and its bytes do the same job — the same file imported twice has the same
    // bytes, so it still collapses.
    match tiebreak {
        Tiebreak::Remote(RemoteRef::Pop { uidl }) => hasher.update(uidl.as_bytes()),
        Tiebreak::Remote(RemoteRef::Jmap { email_id }) => {
            hasher.update(email_id.as_str().as_bytes())
        }
        Tiebreak::Remote(RemoteRef::Imap { mailbox, uid, .. }) => {
            hasher.update(mailbox.as_bytes());
            hasher.update(&uid.to_le_bytes())
        }
        Tiebreak::Remote(RemoteRef::Graph { id, .. }) => hasher.update(id.as_bytes()),
        Tiebreak::Bytes(digest) => hasher.update(digest.as_bytes()),
    };
    MessageKey::Synthetic(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assemble::parse::{Provenance, Reading, parse};
    use chrono::TimeZone;
    use uuid::Uuid;

    fn at() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    fn account() -> AccountId {
        mail_domain::id::account_id_from_uuid(Uuid::from_u128(0xa1))
    }

    fn read(raw: &str, learns: bool) -> Parsed {
        parse(
            raw.replace('\n', "\r\n").as_bytes(),
            Reading {
                provenance: Provenance::Wire,
                learns,
                at: at(),
            },
        )
        .unwrap()
    }

    fn remote(n: u32) -> RemoteRef {
        RemoteRef::Pop {
            uidl: format!("u{n}"),
        }
    }

    fn inbox() -> Destination {
        Destination {
            mailbox: mail_domain::MailboxRef {
                account: account(),
                path: "INBOX".to_owned(),
            },
            role: MailboxRole::Inbox,
        }
    }

    /// Resolve with ids numbered from one: messages `0x1000+n`, conversations `0x2000+n`, and a
    /// blob named by the first bytes of its digest.
    fn run(
        index: &ThreadIndex,
        role_dest: Destination,
        parsed: &[(RemoteRef, Parsed)],
    ) -> Resolved {
        let (mut m, mut t) = (0u128, 0u128);
        let frame = Frame {
            account: &account(),
            fallback: at(),
        };
        let batch: Vec<(&RemoteRef, &Parsed)> = parsed.iter().map(|(r, p)| (r, p)).collect();
        resolve(
            &frame,
            role_dest,
            None,
            index,
            &batch,
            &mut Ids {
                message: &mut || {
                    m += 1;
                    MessageId::from_uuid(Uuid::from_u128(0x1000 + m))
                },
                thread: &mut || {
                    t += 1;
                    ThreadId::from_uuid(Uuid::from_u128(0x2000 + t))
                },
                blob: &|digest| {
                    let mut bytes = [0u8; 16];
                    bytes.copy_from_slice(&digest.as_bytes()[..16]);
                    BlobId::from_uuid(Uuid::from_bytes(bytes))
                },
            },
        )
    }

    const ROOT: &str = "From: Ada <ada@example.test>\nTo: me@example.test\nSubject: plan\n\
                        Message-ID: <root@example.test>\nDate: Tue, 14 Nov 2023 22:13:20 +0000\n\nhello\n";
    const REPLY: &str = "From: Bo <bo@example.test>\nTo: ada@example.test\nSubject: Re: plan\n\
                         Message-ID: <r1@example.test>\nIn-Reply-To: <root@example.test>\n\
                         References: <root@example.test>\n\nreply\n";
    const OTHER: &str =
        "From: Cy <cy@example.test>\nSubject: lunch\nMessage-ID: <o@example.test>\n\nx\n";

    #[test]
    fn a_batch_is_threaded_with_no_store_and_gets_the_ids_it_was_given() {
        let parsed = vec![
            (remote(1), read(ROOT, false)),
            (remote(2), read(REPLY, false)),
            (remote(3), read(OTHER, false)),
        ];
        let out = run(&ThreadIndex::default(), inbox(), &parsed);
        let messages: Vec<&Message> = out.ingest.messages.iter().map(|f| &f.message).collect();
        assert_eq!(messages[0].thread, messages[1].thread);
        assert_ne!(messages[0].thread, messages[2].thread);
        // Message ids in order; the conversations are the supplied ones, in the order drawn.
        let ids: Vec<MessageId> = messages.iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            (1..=3)
                .map(|n| MessageId::from_uuid(Uuid::from_u128(0x1000 + n)))
                .collect::<Vec<_>>()
        );
        assert!(
            messages
                .iter()
                .all(|m| (0x2001..=0x2003).contains(&m.thread.as_uuid().as_u128()))
        );
        assert_eq!(
            messages[1].key,
            MessageKey::Rfc("r1@example.test".to_owned())
        );
        // Resolving again gives the same answer: nothing in it is drawn from a clock or a rng.
        assert_eq!(
            out.ingest,
            run(&ThreadIndex::default(), inbox(), &parsed).ingest
        );
    }

    #[test]
    fn a_reply_joins_the_conversation_the_index_says_holds_its_parent() {
        let held = ThreadId::from_uuid(Uuid::from_u128(0x77));
        let index = ThreadIndex {
            by_rfc_id: [("root@example.test".to_owned(), held)].into(),
            ..ThreadIndex::default()
        };
        let out = run(&index, inbox(), &[(remote(2), read(REPLY, false))]);
        assert_eq!(out.ingest.messages[0].message.thread, held);
    }

    #[test]
    fn the_servers_thread_id_joins_messages_nothing_else_relates() {
        let held = ThreadId::from_uuid(Uuid::from_u128(0x78));
        let index = ThreadIndex {
            by_provider: [("T1".to_owned(), held)].into(),
            provider_of: [(remote(3), "T1".to_owned())].into(),
            ..ThreadIndex::default()
        };
        let out = run(&index, inbox(), &[(remote(3), read(OTHER, false))]);
        assert_eq!(out.ingest.messages[0].message.thread, held);
    }

    #[test]
    fn what_a_message_taught_about_its_sender_goes_out_as_rows_and_our_own_copies_teach_nothing() {
        let parsed = vec![(remote(1), read(ROOT, true))];
        let out = run(&ThreadIndex::default(), inbox(), &parsed);
        let [Row::KeyLearned(Taught::Autocrypt { sighting, key })] = &out.rows[..] else {
            panic!("one sighting: {:?}", out.rows);
        };
        assert_eq!(sighting.address, "ada@example.test");
        assert_eq!(sighting.header, None);
        assert_eq!(key, &None);
        // The message's own date, as it is earlier than when it arrived.
        assert_eq!(
            sighting.at,
            Utc.with_ymd_and_hms(2023, 11, 14, 22, 13, 20).unwrap()
        );

        let sent = Destination {
            role: MailboxRole::Sent,
            ..inbox()
        };
        assert!(run(&ThreadIndex::default(), sent, &parsed).rows.is_empty());
    }

    #[test]
    fn a_part_left_on_the_server_is_not_held_and_a_held_one_is_the_blob_it_was_given() {
        let raw = "From: a@example.test\nSubject: pic\nMessage-ID: <pic@example.test>\n\
                   MIME-Version: 1.0\nContent-Type: multipart/mixed; boundary=\"b\"\n\n\
                   --b\nContent-Type: text/plain\n\nlook\n\
                   --b\nContent-Type: application/octet-stream; name=\"x.bin\"\n\
                   Content-Disposition: attachment; filename=\"x.bin\"\n\
                   Content-Transfer-Encoding: base64\n\nAAECAwQF\n--b--\n";
        let parsed = vec![(remote(1), read(raw, false))];
        let out = run(&ThreadIndex::default(), inbox(), &parsed);
        let message = &out.ingest.messages[0].message;
        let PartContent::Held(blob) = &message.attachments[0].content else {
            panic!("held");
        };
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&blake3::hash(&[0, 1, 2, 3, 4, 5]).as_bytes()[..16]);
        assert_eq!(*blob, BlobId::from_uuid(Uuid::from_bytes(bytes)));
        assert_eq!(message.attachments[0].size, 6);
        assert_eq!(
            out.ingest.messages[0].raw,
            match &message.body {
                Body::Present { raw, .. } => *raw,
                Body::Absent => panic!("whole"),
            }
        );
    }
}
