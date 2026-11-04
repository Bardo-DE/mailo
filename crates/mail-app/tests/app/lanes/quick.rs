//! Lane 16: the quick filter. The toolbar's Filter button shows the row of toggles; each toggle
//! is an operator written into the search and shown as a chip in the header; Unread (⌥⌘U) and
//! Has Attachments narrow the Inbox together; Clear Filter takes the operators out and leaves
//! the rest of the line; a filter that leaves nothing says so and clears from the empty list;
//! and the line stays across places.

use mail_core::{SqliteStore, Store};
use mail_domain::{Change, ChangeId, Patch, ReadState};
use mail_ui_model::copy;

use super::drive::{Drive, Key, PRIMARY};
use super::look::conversation;
use super::window::{INBOX, Window, account, deliver, hours_ago, ms};
use ds_harness::Harness;

/// The newest conversation, unread, carrying a file.
const CONTRACT: &str = "The signed contract";
/// The oldest, read, carrying a file.
const RECEIPTS: &str = "Last year's receipts";

/// The toolbar's Filter button.
fn filter_button() -> String {
    format!(".list-head [*|aria-label=\"{}\"]", copy::aria::FILTER)
}

/// The `n`th toggle of the row (1-based): Unread, Starred, Has Attachments.
fn toggle(n: usize) -> String {
    format!(".quick-filter > :nth-child({n}) .ds-chip-press")
}

/// The operators the header's search shows as chips, in order; empty while no search is.
fn chips(h: &Harness) -> Vec<String> {
    (1..=h.count(".list-head .search-face .ds-chip"))
        .filter_map(|n| {
            h.text_of(&format!(".list-head .search-face .ds-chip:nth-child({n})"))
                .map(|text| text.trim().to_owned())
        })
        .collect()
}

/// A message from Bob called `subject`, carrying a PDF, `hours` old.
fn with_file(store: &SqliteStore, uidl: &str, subject: &str, hours: i64) {
    let raw = format!(
        "From: Bob <bob@example.test>\r\nTo: Me <me@example.test>\r\nSubject: {subject}\r\n\
         Date: {}\r\nMessage-ID: <{uidl}@example.test>\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"mix\"\r\n\r\n\
         --mix\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\nAttached.\r\n\
         --mix\r\nContent-Type: application/pdf; name=\"{uidl}.pdf\"\r\n\
         Content-Disposition: attachment; filename=\"{uidl}.pdf\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--mix--\r\n",
        hours_ago(hours)
    );
    deliver(store, uidl, &raw);
}

/// Every message of the conversation called `subject` marked read, as a sync from another
/// device would have it.
fn read(store: &SqliteStore, subject: &str) {
    let thread = conversation(store, subject).id;
    let changes = store
        .thread(thread)
        .expect("the conversation")
        .messages
        .into_iter()
        .map(|message| Change::MessageRead(message, ReadState::Read))
        .collect();
    store
        .apply(
            account(),
            &Patch {
                id: ChangeId::generate(),
                changes,
            },
            chrono::Utc::now(),
        )
        .expect("read");
}

/// The seeded Inbox, two conversations of it read, and two that carry a file: one unread and
/// newest, one read and oldest.
fn seeded(store: &SqliteStore) {
    with_file(store, "contract", CONTRACT, 0);
    with_file(store, "receipts", RECEIPTS, 9);
    read(store, INBOX[1].1);
    read(store, INBOX[3].1);
    read(store, RECEIPTS);
}

/// Wait until the list's rows are `want`, top to bottom.
fn until_rows(window: &mut Window, what: &str, want: &[&str]) {
    let want: Vec<String> = want.iter().map(|s| (*s).to_owned()).collect();
    window.until(what, |h| {
        let rows: Vec<String> = (1..=h.count(".list .ds-list-item"))
            .filter_map(|n| {
                let item = format!(".list .ds-list-item[*|aria-posinset=\"{n}\"]");
                if h.attr(&item, "data-exit").is_some() {
                    return None;
                }
                h.text_of(&format!("{item} .ds-thread-sub"))
                    .map(|text| text.trim().to_owned())
            })
            .collect();
        rows == want
    });
}

#[test]
fn the_quick_filter_narrows_the_list_through_the_search_and_clears() {
    let mut window = Window::open(seeded);
    let every = [
        CONTRACT, INBOX[0].1, INBOX[1].1, INBOX[2].1, INBOX[3].1, RECEIPTS,
    ];
    until_rows(&mut window, "the Inbox lists all six", &every);
    assert_eq!(window.harness.count(".quick-filter"), 0, "the row is up");
    assert_eq!(
        window
            .harness
            .attr(&filter_button(), "aria-pressed")
            .as_deref(),
        Some("false")
    );

    // The Filter button shows the row; Unread keeps the three unread.
    window.click(&filter_button());
    window.until("the row shows", |h| h.count(".quick-filter .ds-chip") == 3);
    window.click(&toggle(1));
    let unread = [CONTRACT, INBOX[0].1, INBOX[2].1];
    until_rows(&mut window, "Unread keeps the unread", &unread);
    assert_eq!(
        window.harness.attr(&toggle(1), "aria-pressed").as_deref(),
        Some("true")
    );
    assert_eq!(
        window
            .harness
            .attr(&filter_button(), "aria-pressed")
            .as_deref(),
        Some("true"),
        "the Filter button does not say the list is narrowed"
    );

    // The toggle is an operator of the search, and the header shows it as a chip.
    window.until("the header shows is:unread", |h| chips(h) == ["is:unread"]);
    assert_eq!(
        window
            .harness
            .count(".list-head .search-face .search-words"),
        0
    );
    assert_eq!(
        window
            .harness
            .text_of(".list-head .search-face .search-scope")
            .as_deref()
            .map(str::trim),
        Some("Inbox"),
        "the list left the Inbox for all mail"
    );

    // Toggled off, the header and the list are the Inbox's again.
    window.click(&toggle(1));
    until_rows(&mut window, "Unread off restores every row", &every);
    window.until("the chip goes", |h| chips(h).is_empty());
    assert_eq!(window.harness.count(".list-head .search-face"), 0);
    window.click(&toggle(1));
    until_rows(&mut window, "Unread on again", &unread);

    // Has Attachments as well: both hold.
    window.click(&toggle(3));
    until_rows(&mut window, "Unread and Has Attachments", &[CONTRACT]);
    window.until("both chips", |h| {
        chips(h) == ["is:unread", "has:attachment"]
    });

    // Clear Filter: every row back, the row still up with nothing pressed.
    window.click(".quick-filter > .ds-button");
    until_rows(&mut window, "Clear Filter brings every row back", &every);
    assert_eq!(window.harness.count(".quick-filter .ds-chip"), 3);
    assert_eq!(
        window.harness.attr(&toggle(1), "aria-pressed").as_deref(),
        Some("false")
    );
    window.until("Clear Filter takes the chips out of the header", |h| {
        chips(h).is_empty()
    });

    // Starred leaves nothing: the list says so, and clears from there.
    window.click(&toggle(2));
    window.until("the empty list says the filter left nothing", |h| {
        h.text_of(".ds-empty-state-title")
            .is_some_and(|title| title.contains(copy::empty::NOTHING_MATCHES_FILTER))
    });
    window.click(".ds-empty-state-action .ds-button");
    until_rows(&mut window, "the empty list's Clear Filter", &every);

    // ⌥⌘U is Unread, from the keyboard.
    window.press(&[PRIMARY, Key::Alt], Key::Char('u'), 1);
    until_rows(&mut window, "⌥⌘U keeps the unread", &unread);

    // Another place and back: the filter is in the search line, which stays, so it narrows
    // Sent as it narrowed the Inbox.
    window.click("[*|data-place=\"Sent\"]");
    window.until("Sent keeps the chip", |h| chips(h) == ["is:unread"]);
    assert_eq!(window.harness.count(".quick-filter .ds-chip"), 3);
    window.click("[*|data-place=\"Inbox\"]");
    until_rows(&mut window, "the Inbox is still narrowed", &unread);

    // The Filter button hides the row and takes the toggles out of the search.
    window.click(&filter_button());
    until_rows(&mut window, "hiding the row restores every row", &every);
    window.until("the row and the chip go", |h| {
        h.count(".quick-filter") == 0 && chips(h).is_empty()
    });
    assert_eq!(
        window
            .harness
            .attr(&filter_button(), "aria-pressed")
            .as_deref(),
        Some("false")
    );
}

/// A search with words keeps them when the toggles come and go: Unread is added after them,
/// all mail is searched, and Clear Filter takes the operator out and leaves the words.
#[test]
fn clear_filter_leaves_the_rest_of_the_search() {
    let mut window = Window::open(seeded);
    window.press(&[PRIMARY], Key::Char('k'), 1);
    window.until("⌘K puts the keyboard in the panel", |h| {
        h.is_focused(".ds-search-card input")
    });
    window.type_text("from:bob");
    window.harness.advance(ms(800));
    window.press(&[], Key::Enter, 1);
    window.until("Bob's two are listed", |h| {
        h.count(".ds-search-card") == 0 && h.count(".list .ds-thread") == 2
    });
    window.click(&filter_button());
    window.until("the row shows", |h| h.count(".quick-filter .ds-chip") == 3);
    window.click(&toggle(1));
    window.until("the header holds both operators", |h| {
        chips(h) == ["from:bob", "is:unread"]
    });
    until_rows(&mut window, "only the unread one of Bob's", &[CONTRACT]);
    window.click(".quick-filter > .ds-button");
    window.until("Clear Filter leaves from:bob", |h| chips(h) == ["from:bob"]);
    until_rows(&mut window, "both of Bob's again", &[CONTRACT, RECEIPTS]);
}
