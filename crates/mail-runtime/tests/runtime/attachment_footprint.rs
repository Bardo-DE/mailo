//! What storing each attachment next to the raw message costs, measured.
//!
//! The raw message is a blob and every held attachment is a second blob of its decoded bytes, so
//! an attachment is on disk twice: once inside the raw message as base64 (about 1.37 times its
//! size with the line breaks) and once decoded. This ingests a synthetic mailbox into a
//! throwaway store and reports how much of the blob bytes the second copy is. Run with
//! `--no-capture` to see the table.

use crate::fixture::{acct_account, now, store};
use base64::Engine;
use mail_domain::*;
use mail_runtime::{Arrival, absorb};
use mail_store::SqliteStore;

/// `len` bytes that do not repeat, so the blob store cannot fold them together.
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

fn message(n: usize, html: Option<usize>, attached: &[usize]) -> Vec<u8> {
    let mut m = format!(
        "From: a{n}@b.test\r\nTo: me@example.test\r\nSubject: m{n}\r\nMessage-ID: <m{n}@b.test>\r\n\
         MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n"
    )
    .into_bytes();
    m.extend_from_slice(b"--b\r\nContent-Type: text/plain\r\n\r\nhello\r\n");
    if let Some(len) = html {
        m.extend_from_slice(b"--b\r\nContent-Type: text/html\r\n\r\n");
        m.extend_from_slice(&b"<p>hello world</p>".repeat(len / 18));
        m.extend_from_slice(b"\r\n");
    }
    for (i, len) in attached.iter().enumerate() {
        let body = noise(*len, (n * 31 + i) as u64);
        m.extend_from_slice(
            format!(
                "--b\r\nContent-Type: application/octet-stream\r\nContent-Transfer-Encoding: base64\r\n\
                 Content-Disposition: attachment; filename=\"f{n}-{i}.bin\"\r\n\r\n"
            )
            .as_bytes(),
        );
        let b64 = base64::engine::general_purpose::STANDARD.encode(body);
        for line in b64.as_bytes().chunks(76) {
            m.extend_from_slice(line);
            m.extend_from_slice(b"\r\n");
        }
    }
    m.extend_from_slice(b"--b--\r\n");
    m
}

fn ingest(store: &SqliteStore, raws: Vec<Vec<u8>>) {
    let arrivals = raws
        .into_iter()
        .enumerate()
        .map(|(i, raw)| Arrival {
            remote: RemoteRef::Pop {
                uidl: format!("u{i}"),
            },
            raw,
        })
        .collect();
    absorb(
        store,
        acct_account(),
        MailboxRef {
            account: acct_account(),
            path: "INBOX".to_owned(),
        },
        Some(SyncCursor::Pop),
        arrivals,
        false,
        now(),
    )
    .unwrap();
}

/// Bytes of the blobs the messages' raw bodies are, and of everything else.
fn blob_bytes(store: &SqliteStore) -> (i64, i64) {
    let c = store.raw_connection();
    let total: i64 = c
        .query_row("SELECT coalesce(sum(size),0) FROM blobs", [], |r| r.get(0))
        .unwrap();
    let raw: i64 = c
        .query_row(
            "SELECT coalesce(sum(size),0) FROM blobs WHERE id IN (SELECT body_raw FROM messages)",
            [],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    (total, raw)
}

fn report(name: &str, raws: Vec<Vec<u8>>) -> f64 {
    let (store, _dir) = store();
    let wire: usize = raws.iter().map(Vec::len).sum();
    ingest(&store, raws);
    let (total, raw) = blob_bytes(&store);
    let extra = total - raw;
    let share = extra as f64 / total as f64 * 100.0;
    println!(
        "{name:<18} wire {:>9} B  blobs {:>9} B  raw {:>9} B  second copies {:>9} B  = {share:.1}% of blob bytes",
        wire, total, raw, extra
    );
    share
}

#[test]
fn the_second_copy_of_attachments_is_measured() {
    let plain: Vec<_> = (0..300).map(|n| message(n, None, &[])).collect();
    let html: Vec<_> = (0..150).map(|n| message(n, Some(20_000), &[])).collect();
    let sizes = [50_000, 200_000, 1_000_000, 5_000_000];
    let heavy: Vec<_> = (0..24)
        .map(|n| message(n, Some(5_000), &[sizes[n % 4]]))
        .collect();
    let mixed: Vec<_> = (0..300)
        .map(|n| message(n, Some(5_000), &[]))
        .chain((300..330).map(|n| message(n, None, &[sizes[n % 3]])))
        .collect();

    let a = report("plain", plain);
    let b = report("html", html);
    let c = report("attachment-heavy", heavy);
    let d = report("mixed mailbox", mixed);
    assert!(a < 5.0 && b < 5.0, "text-only mail has no second copies");
    assert!(c > 0.0 && d > 0.0);
}
