//! What assembling a batch makes of it, pinned through the public entry points alone, so the
//! split of assembly into parse, resolve and commit has something to be held to: threading
//! within a batch and onto what is stored, blobs written once, S/MIME signers learnt, and the
//! ids the messages come out with.

use crate::fixture::{acct_account, inbox, now, store};
use mail_crypto::smime::{self, Sealing};
use mail_crypto::testing::smime::{DATE, Person, identity, message, pki, rng};
use mail_domain::*;
use mail_runtime::{Arrival, Destination, absorb_counted, absorb_into};
use mail_store::{SqliteStore, Store};
use std::collections::BTreeSet;

fn pop(uidl: &str, raw: &str) -> Arrival {
    Arrival {
        remote: RemoteRef::Pop {
            uidl: uidl.to_owned(),
        },
        raw: raw.replace('\n', "\r\n").into_bytes(),
    }
}

fn absorb_in(store: &SqliteStore, role: MailboxRole, arrivals: Vec<Arrival>) -> Vec<Message> {
    let absorbed = absorb_counted(
        store,
        acct_account(),
        Destination {
            mailbox: inbox(),
            role,
        },
        None,
        arrivals,
        false,
        now(),
    )
    .unwrap();
    absorbed
        .patch
        .changes
        .into_iter()
        .filter_map(|change| match change {
            Change::MessageUpsert(message) => Some(*message),
            _ => None,
        })
        .collect()
}

#[test]
fn a_batch_threads_within_itself_and_onto_what_is_stored() {
    let (store, _dir) = store();
    let first = absorb_in(
        &store,
        MailboxRole::Inbox,
        vec![pop(
            "1",
            "From: a@example.test\nSubject: plan\nMessage-ID: <root@example.test>\n\nroot\n",
        )],
    );
    let batch = absorb_in(
        &store,
        MailboxRole::Inbox,
        vec![
            // Answers the stored root.
            pop(
                "2",
                "From: b@example.test\nSubject: Re: plan\nMessage-ID: <r1@example.test>\n\
                 In-Reply-To: <root@example.test>\nReferences: <root@example.test>\n\nr1\n",
            ),
            // Answers the one before it, in this same batch.
            pop(
                "3",
                "From: a@example.test\nSubject: Re: plan\nMessage-ID: <r2@example.test>\n\
                 In-Reply-To: <r1@example.test>\nReferences: <root@example.test> <r1@example.test>\n\nr2\n",
            ),
            // Nothing to do with it.
            pop(
                "4",
                "From: c@example.test\nSubject: lunch\nMessage-ID: <other@example.test>\n\nother\n",
            ),
        ],
    );
    assert_eq!(batch.len(), 3);
    assert_eq!(batch[0].thread, first[0].thread);
    assert_eq!(batch[1].thread, first[0].thread);
    assert_ne!(batch[2].thread, first[0].thread);
    let ids: BTreeSet<_> = first.iter().chain(&batch).map(|m| m.id).collect();
    assert_eq!(ids.len(), 4, "every message has an id of its own");
    assert!(
        batch
            .iter()
            .all(|m| m.read == ReadState::Unread && m.star == Star::Unstarred)
    );
}

#[test]
fn bytes_are_stored_once_whoever_names_them_and_attachments_are_held() {
    let (store, _dir) = store();
    let raw = "From: a@example.test\nSubject: pic\nMessage-ID: <pic@example.test>\n\
               MIME-Version: 1.0\nContent-Type: multipart/mixed; boundary=\"b\"\n\n\
               --b\nContent-Type: text/plain\n\nlook\n\
               --b\nContent-Type: application/octet-stream; name=\"x.bin\"\n\
               Content-Disposition: attachment; filename=\"x.bin\"\n\
               Content-Transfer-Encoding: base64\n\nAAECAwQF\n--b--\n";
    let stored = absorb_in(
        &store,
        MailboxRole::Inbox,
        vec![pop("1", raw), pop("2", raw)],
    );
    // The same bytes under two addresses are one message.
    assert_eq!(stored.len(), 1, "{stored:?}");
    let message = &stored[0];
    let Body::Present { raw: blob, .. } = &message.body else {
        panic!("a whole message has its body");
    };
    assert_eq!(
        store.blobs().get(*blob).unwrap(),
        raw.replace('\n', "\r\n").into_bytes()
    );
    assert_eq!(message.attachments.len(), 1);
    let PartContent::Held(part) = &message.attachments[0].content else {
        panic!("a part the message carries is held");
    };
    assert_eq!(store.blobs().get(*part).unwrap(), vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(message.attachments[0].size, 6);
}

fn alice() -> mail_crypto::smime::Identity {
    identity(
        pki(),
        &Person::new("Alice", &["alice@example.test"], 10, 1001),
    )
}

fn signed(from_header: &str, by: &mail_crypto::smime::Identity) -> Vec<u8> {
    smime::seal(
        &message(from_header, "me@example.test", DATE),
        &Sealing {
            mode: Mode::Sign,
            signer: Some(by),
            recipients: &[],
            now: mail_crypto::testing::smime::now(),
            boundary_prefix: "mailo",
        },
        &mut rng(7),
    )
    .unwrap()
}

fn arrive_signed(store: &SqliteStore, role: MailboxRole, raw: Vec<u8>, uidl: &str) {
    absorb_into(
        store,
        acct_account(),
        Destination {
            mailbox: inbox(),
            role,
        },
        None,
        vec![Arrival {
            remote: RemoteRef::Pop {
                uidl: uidl.to_owned(),
            },
            raw,
        }],
        false,
        mail_crypto::testing::smime::now(),
    )
    .unwrap();
}

#[test]
fn a_signers_certificate_is_kept_when_mail_from_its_address_proves_the_key() {
    let (store, _dir) = store();
    assert!(store.smime_certs().unwrap().is_empty());
    arrive_signed(
        &store,
        MailboxRole::Inbox,
        signed("alice@example.test", &alice()),
        "a",
    );
    let kept = store.smime_certs().unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].fingerprint, alice().cert.fingerprint());
    assert_eq!(kept[0].source, CertSource::Received);
}

#[test]
fn a_certificate_is_not_kept_for_mail_from_another_address_or_from_our_own_copies() {
    let (store, _dir) = store();
    // Signed by Alice's key but claiming to be from someone her certificate is not for.
    arrive_signed(
        &store,
        MailboxRole::Inbox,
        signed("mallory@example.test", &alice()),
        "m",
    );
    // Our own Sent and Drafts copies teach nothing.
    arrive_signed(
        &store,
        MailboxRole::Sent,
        signed("alice@example.test", &alice()),
        "s",
    );
    arrive_signed(
        &store,
        MailboxRole::Drafts,
        signed("alice@example.test", &alice()),
        "d",
    );
    assert!(store.smime_certs().unwrap().is_empty());
}
