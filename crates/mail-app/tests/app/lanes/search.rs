//! Lane 15: searching. ⌘K puts the keyboard in the search panel, where typed letters are its
//! query and not shortcuts, the first Escape empties it and the second gives the keyboard back;
//! the toolbar's magnifier brings it up too, what is typed filters the rows, and ⌘K brings it
//! up over an open conversation, where ⌘F finds in the conversation instead (`find`). A search
//! kept with Save as View is a place in the sidebar, grouped as it was saved, and is deleted
//! from its editor. A search that finds nothing here offers the server, which is asked only
//! when that is pressed in the search's header, or at once when Settings says so. A search the panel leaves behind is
//! the list's header until it is cleared, by its × or Esc.

use mail_core::{Arrival, Searched, ServerHits, SqliteStore, Store, absorb};
use mail_domain::{
    AccountPlan, AuthPlan, Change, GroupKey, Incoming, MailboxRef, MessageId, Outgoing, ReadState,
    RemoteRef, SaslMech, Tls, Username, View,
};
use mail_ui::ui::native::ServerSearcher;
use mail_ui_model::copy;
use std::sync::{Arc, Mutex, mpsc};

use super::drive::{Drive, Key, PRIMARY};
use super::look::conversation;
use super::window::{INBOX, Window, account, ms};
use crate::lanes::seed::ME;

/// The panel's field.
const FIELD: &str = ".ds-search-card input";
/// The server's hit: found by a word in its body, which a header fetch does not bring, so the
/// store cannot match it.
const HIT: &str = "Numbers for March 2019";
/// Search on Server, in the search shown in the list's header.
const OFFER: &str = ".list-head .search-server button";
/// What the header says of the server being asked.
const SAID: &str = ".list-head .search-server .search-server-said";

/// Held until a unit is sent, so a test sees the server being asked before it answers.
type Gate = Arc<Mutex<mpsc::Receiver<()>>>;

/// The account as an IMAP account, which a server can be searched on.
fn on_imap(store: &SqliteStore) {
    with_incoming(
        store,
        Incoming::Imap {
            host: "imap.example.test".to_owned(),
            port: 993,
            tls: Tls::Implicit,
        },
    );
}

/// The account as a local one, which has no server to search.
fn on_local(store: &SqliteStore) {
    with_incoming(store, Incoming::Local);
}

/// The account planned with `incoming`.
fn with_incoming(store: &SqliteStore, incoming: Incoming) {
    let plan = AccountPlan {
        address: ME.to_owned(),
        incoming,
        outgoing: Outgoing::Smtp {
            host: "smtp.example.test".to_owned(),
            port: 465,
            tls: Tls::Implicit,
        },
        auth: AuthPlan::Password {
            username: Username::SameAsAddress,
            sasl: vec![SaslMech::Plain],
        },
        identities: Vec::new(),
    };
    store.set_account_plan(account(), &plan).expect("the plan");
}

/// A server that holds [`HIT`] in its All Mail: asked, it is kept here as headers and marked
/// found, as `mail_runtime`'s search keeps what it finds. Records each line it was asked, and
/// answers once `gate`, if there is one, lets it.
fn server(asked: Arc<Mutex<Vec<String>>>, gate: Option<Gate>) -> ServerSearcher {
    ServerSearcher(Arc::new(move |client, account, input, now| {
        if let Some(gate) = &gate {
            let _ = gate.lock().unwrap_or_else(|e| e.into_inner()).recv();
        }
        let store = client.store_for_test();
        asked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(input.to_owned());
        let head = format!(
            "From: Bob <bob@example.test>\r\nTo: {ME}\r\nSubject: {HIT}\r\n\
             Date: Wed, 6 Mar 2019 12:00:00 +0000\r\nMessage-ID: <budget@example.test>\r\n\r\n"
        );
        let remote = RemoteRef::Imap {
            mailbox: "[Gmail]/All Mail".to_owned(),
            uidvalidity: 11,
            uid: 502,
        };
        let mailbox = MailboxRef {
            account: account.clone(),
            path: "[Gmail]/All Mail".to_owned(),
        };
        let arrival = Arrival {
            remote: remote.clone(),
            raw: head.into_bytes(),
        };
        let patch = absorb(
            &store,
            account.clone(),
            mailbox,
            None,
            vec![arrival],
            true,
            now,
        )
        .map_err(|e| e.to_string())?;
        let new: Vec<MessageId> = patch
            .changes
            .iter()
            .filter_map(|change| match change {
                Change::MessageUpsert(message) => Some(message.id),
                _ => None,
            })
            .collect();
        store
            .mark_found(account.clone(), &new, now)
            .map_err(|e| e.to_string())?;
        let messages = store
            .held_at(account, &[remote])
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(_, id)| id)
            .collect();
        Ok(Searched::Found(ServerHits {
            messages,
            fetched: new.len(),
            more: 0,
        }))
    }))
}

/// The window over an IMAP account, with [`server`] as its server's search, and `settings`
/// written first.
fn open(
    asked: &Arc<Mutex<Vec<String>>>,
    settings: impl FnOnce(&mut mail_ui_model::settings::MailSettings),
) -> Window {
    open_gated(asked, None, settings)
}

/// [`open`], the server answering only when `gate` lets it.
fn open_gated(
    asked: &Arc<Mutex<Vec<String>>>,
    gate: Option<Gate>,
    settings: impl FnOnce(&mut mail_ui_model::settings::MailSettings),
) -> Window {
    let asked = Arc::clone(asked);
    Window::open_with(chrono::Utc::now(), on_imap, move |dir, contexts| {
        mail_ui_model::settings::change(
            &mail_ui_model::settings::root_for(&dir.join("config")),
            settings,
        )
        .expect("the settings are written");
        contexts.with(server(asked, gate))
    })
}

/// The rows' subjects, top to bottom: a row still sliding out is the previous answer's.
fn rows(window: &Window) -> Vec<String> {
    let h = &window.harness;
    (1..=h.count(".list .ds-list-item[*|aria-posinset]"))
        .filter_map(|n| {
            let item = format!(".list .ds-list-item[*|aria-posinset=\"{n}\"]");
            if h.attr(&item, "data-exit").is_some() {
                return None;
            }
            h.text_of(&format!("{item} .ds-thread-sub"))
        })
        .collect()
}

/// The list as drawn: a group header as `# title`, a row as its subject.
fn lines(window: &Window) -> Vec<String> {
    let h = &window.harness;
    (1..=h.count(".list .ds-list-item[*|aria-posinset]"))
        .filter_map(|n| {
            let at = format!(".list .ds-list-item[*|aria-posinset=\"{n}\"]");
            if h.count(&format!("{at} .ds-section-header")) > 0 {
                h.text_of(&format!("{at} .ds-section-header-title"))
                    .map(|title| format!("# {}", title.trim()))
            } else {
                h.text_of(&format!("{at} .ds-thread-sub"))
            }
        })
        .collect()
}

/// ⌘K, and `text` typed in the panel's field.
fn search(window: &mut Window, text: &str) {
    window.press(&[PRIMARY], Key::Char('k'), 1);
    window.until("⌘K puts the keyboard in the panel", |h| {
        h.is_focused(FIELD)
    });
    window.type_text(text);
}

/// A press outside the panel: it goes, and its search stays the list's.
fn put_away(window: &mut Window) {
    window.harness.click(ds::prelude::Point {
        x: ds::prelude::Px(4.0),
        y: ds::prelude::Px(4.0),
    });
    window.until("the panel goes", |h| h.count(".ds-search-card") == 0);
}

#[test]
fn the_search_panel_filters_the_list_saves_a_view_and_offers_the_server() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut window = open(&asked, |_| {});

    // ⌘K, typed letters, then Escape twice.
    search(&mut window, "arch");
    window.until("the panel is up", |h| {
        h.count(".ds-search-card .ds-menu") == 1
    });
    assert_eq!(window.harness.count(".ds-palette"), 0, "a palette is drawn");
    assert_eq!(window.harness.attr(FIELD, "value").as_deref(), Some("arch"));
    window.press(&[], Key::Escape, 1);
    assert_eq!(window.harness.attr(FIELD, "value").as_deref(), Some(""));
    assert!(
        window.harness.is_focused(FIELD),
        "the first Escape left the field"
    );
    window.press(&[], Key::Escape, 1);
    window.until("the second Escape closes it", |h| {
        h.count(".ds-search-card") == 0 && h.is_focused(".app")
    });

    // The magnifier, and typing that filters the rows; out again, the rows back.
    window.click(".list-head .bar .ds-button");
    window.until("the magnifier brings it up", |h| h.is_focused(FIELD));
    window.type_text("invoice");
    window.until("only the invoice is listed", |h| {
        h.count(".list .ds-thread") == 1 && super::look::row_of(h, INBOX[1].1).is_some()
    });
    window.press(&[], Key::Escape, 2);
    window.until("the rows come back", |h| {
        h.count(".list .ds-thread") == INBOX.len() && h.count(".ds-search-card") == 0
    });

    // ⌘K over an open conversation.
    window.open_subject(INBOX[0].1);
    window.press(&[PRIMARY], Key::Char('k'), 1);
    window.until("⌘K brings it up", |h| h.is_focused(FIELD));
    window.press(&[], Key::Escape, 2);

    // Two read, two not; then in:inbox kept as a view, grouped unread first.
    for (_, subject) in &INBOX[..2] {
        window.open_subject(subject);
        window.until_stored("it is read", |store| {
            conversation(store, subject).read == ReadState::Read
        });
    }
    let place = |name: &str| format!("[*|data-place=\"{name}\"]");
    search(&mut window, "in:inbox");
    window.harness.advance(ms(800));
    put_away(&mut window);
    window.click(&format!("[*|aria-label=\"{}\"]", copy::aria::SAVE_AS_VIEW));
    window.until("the sheet opens", |h| h.count(".sheet-form") == 1);
    window.click(".sheet-form input");
    window.press(&[PRIMARY], Key::Char('a'), 1);
    window.type_text("Mine");
    window.click("[*|data-view=group]");
    window.click(".ds-menu .ds-menu-item:nth-child(3)");
    window.click("[*|data-view=save]");
    window.until("the sheet closes", |h| h.count(".sheet-form") == 0);
    let views = window.store.views().expect("the views");
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].name, "Mine");
    assert_eq!(views[0].group_by, Some(GroupKey::Read));
    window.until("the view is a place, and shown", |h| {
        h.count(&place("Mine")) == 1 && h.count(".list-head .search-face") == 0
    });
    assert_eq!(
        lines(&window),
        [
            format!("# {}", copy::headers::UNREAD),
            INBOX[2].1.to_owned(),
            INBOX[3].1.to_owned(),
            format!("# {}", copy::headers::READ),
            INBOX[0].1.to_owned(),
            INBOX[1].1.to_owned(),
        ]
    );

    // Deleted from its editor, it leaves the sidebar and the store.
    window.click(&format!("[*|aria-label=\"{}\"]", copy::aria::EDIT_VIEW));
    window.until("the editor opens", |h| h.count(".sheet-form") == 1);
    window.click("[*|data-view=delete]");
    window.until("the view goes", |h| {
        h.count(".sheet-form") == 0 && h.count(&place("Mine")) == 0
    });
    assert_eq!(window.store.views().expect("the views"), Vec::<View>::new());

    // A search that finds nothing here offers the server from the header, asked only once that
    // is pressed; the footer of the list is gone.
    window.click(&place("Inbox"));
    search(&mut window, "spreadsheet");
    window.harness.advance(ms(400));
    put_away(&mut window);
    window.until("the whole word has settled, with the offer", |h| {
        h.count(OFFER) == 1
            && h.attr(".search-server", "data-line").as_deref() == Some("spreadsheet")
    });
    assert_eq!(window.harness.count(".server-search"), 0, "a footer");
    assert!(rows(&window).is_empty(), "something here matched");
    assert!(asked.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
    window.click(OFFER);
    window.until("the server's hit is the one row", |h| {
        h.count(".list .ds-thread") == 1 && super::look::row_of(h, HIT).is_some()
    });
    assert_eq!(
        *asked.lock().unwrap_or_else(|e| e.into_inner()),
        ["spreadsheet"]
    );
    let server = copy::headers::FROM_SERVER;
    assert!(
        window.harness.html().contains(server),
        "no heading says where"
    );
    assert_eq!(
        window.harness.count(&format!("[*|data-chip=\"{server}\"]")),
        0,
        "the row repeats its heading"
    );
}

#[test]
fn turned_on_in_settings_the_server_is_asked_once_as_the_search_is_shown() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut window = open(&asked, |settings| {
        settings.search.server_automatically = mail_ui_model::settings::ServerSearch::On;
    });
    search(&mut window, "spreadsheet");
    window.until("the server's hit is listed unasked", |h| {
        super::look::row_of(h, HIT).is_some()
    });
    // A prefix asked while the line was typed can be what listed it: wait for the whole line.
    let recorded = Arc::clone(&asked);
    window.until("the whole line is asked", move |_| {
        recorded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|line| line == "spreadsheet")
    });
    window.harness.advance(ms(1500));
    assert_eq!(window.harness.count(".server-search"), 0, "a footer");
    let asked = asked.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut once = asked.clone();
    once.dedup();
    assert_eq!(once, asked, "a line asked twice: {asked:?}");
}

/// While a search narrows the list and the panel is down, the search is the list's header: the
/// search as the panel's field shows it, its operators as chips, where it looked at its end,
/// Save as View and the ×, and none of the place's own tools. A press on it brings the panel back
/// over it; the ×, or Esc in the list, lets go of it and the place's header comes back.
#[test]
fn a_search_shown_is_the_lists_header_until_it_is_cleared() {
    let mut window = Window::open(|store| {
        let raw = format!(
            "From: Ada Lovelace <ada@example.test>\r\nTo: {ME}\r\nSubject: Lunch on Friday?\r\n\
             Date: {}\r\nMessage-ID: <lunch@example.test>\r\n\r\nShall we?\r\n",
            super::window::hours_ago(1)
        );
        super::window::deliver(store, "lunch", &raw);
    });
    let all = INBOX.len() + 1;
    let named = |name: &str| format!(".list-head [*|aria-label=\"{name}\"]");
    let face = ".list-head .search-face";
    let applied = |window: &mut Window| {
        search(window, "from:ada lunch");
        window.harness.advance(ms(800));
        window.press(&[], Key::Enter, 1);
        window.until("Return leaves the search in the list", |h| {
            h.count(".ds-search-card") == 0 && h.count(face) == 1
        });
        window.until("only Ada's lunch is listed", |h| {
            h.count(".list .ds-thread") == 1
        });
    };

    applied(&mut window);
    let h = &window.harness;
    assert_eq!(
        h.attr(face, "aria-label").as_deref(),
        Some(copy::aria::EDIT_SEARCH)
    );
    assert_eq!(
        h.text_of(&format!("{face} .ds-chip"))
            .as_deref()
            .map(str::trim),
        Some("from:ada")
    );
    assert_eq!(
        h.text_of(&format!("{face} .search-words"))
            .as_deref()
            .map(str::trim),
        Some("lunch")
    );
    assert_eq!(
        h.text_of(&format!("{face} .search-scope"))
            .as_deref()
            .map(str::trim),
        Some(copy::status::SEARCHED_ALL_MAIL)
    );
    assert_eq!(h.count(&named(copy::aria::SAVE_AS_VIEW)), 1);
    assert_eq!(h.count(&named(copy::aria::CLEAR_SEARCH)), 1);
    assert_eq!(h.count(&named(copy::aria::NEW_MESSAGE)), 0, "New Message");
    assert_eq!(h.count(&named(copy::aria::MORE)), 0, "the ⋯");
    assert_eq!(h.count(".list-head .list-title"), 0, "the place's title");
    // The search takes the header's width but for the server, Filter, Save as View and the ×.
    let head = h.rect(".list-head").expect("the header");
    let shown = h.rect(face).expect("the search");
    assert!(
        shown.size.width.0 >= head.size.width.0 * 0.6 - 0.5,
        "{shown:?} in {head:?}"
    );

    // A press on it brings the panel back over the search.
    window.click(face);
    window.until("the panel comes back over it", |h| {
        h.is_focused(FIELD) && h.attr(FIELD, "value").as_deref() == Some("from:ada lunch")
    });
    put_away(&mut window);
    assert_eq!(
        window.harness.count(face),
        1,
        "the search went with the panel"
    );

    // The × lets go of it: the place's header and its rows come back.
    window.click(&named(copy::aria::CLEAR_SEARCH));
    window.until("the × gives the place back", |h| {
        h.count(face) == 0
            && h.count(&named(copy::aria::NEW_MESSAGE)) == 1
            && h.count(&named(copy::aria::MORE)) == 1
            && h.count(".list .ds-thread") == all
    });

    // So does Esc in the list, with nothing open.
    applied(&mut window);
    window.press(&[], Key::Escape, 1);
    window.until("Esc gives the place back", |h| {
        h.count(face) == 0
            && h.count(&named(copy::aria::NEW_MESSAGE)) == 1
            && h.count(".list .ds-thread") == all
    });
}

/// The header offers the server while a search is shown and an account in view has one to ask,
/// and not otherwise: not over a plain account, not once the search is cleared.
#[test]
fn the_header_offers_the_server_only_while_a_searchable_account_is_searched() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut window = open(&asked, |_| {});
    let any = ".list-head .search-server";
    assert_eq!(window.harness.count(any), 0, "offered with no search");
    search(&mut window, "lunch");
    window.harness.advance(ms(400));
    put_away(&mut window);
    window.until("the offer is in the header", |h| h.count(OFFER) == 1);
    assert_eq!(
        window.harness.attr(OFFER, "aria-label").as_deref(),
        Some(copy::aria::SEARCH_ON_SERVER)
    );
    window.click(&format!("[*|aria-label=\"{}\"]", copy::aria::CLEAR_SEARCH));
    window.until("the offer goes with the search", |h| h.count(any) == 0);

    let mut plain = Window::open(on_local);
    search(&mut plain, "lunch");
    plain.harness.advance(ms(400));
    put_away(&mut plain);
    plain.until("the search is the header", |h| {
        h.count(".list-head .search-face") == 1
    });
    assert_eq!(plain.harness.count(any), 0, "offered with no server to ask");
}

/// Pressed, the header says the server is being asked, then what it found; the offer comes back
/// as "again".
#[test]
fn the_header_says_the_server_is_asked_and_then_what_it_found() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let (release, gate) = mpsc::channel();
    let mut window = open_gated(&asked, Some(Arc::new(Mutex::new(gate))), |_| {});
    search(&mut window, "spreadsheet");
    window.harness.advance(ms(400));
    put_away(&mut window);
    window.until("the offer is in the header", |h| h.count(OFFER) == 1);
    window.click(OFFER);
    window.until("the header says it is asking", |h| {
        h.text_of(SAID)
            .is_some_and(|said| said.contains("Searching") && said.contains("on the server"))
    });
    assert_eq!(window.harness.count(&format!("{SAID} .pending")), 1);
    release.send(()).expect("the server waits");
    window.until("the header says what it found", |h| {
        h.text_of(SAID)
            .is_some_and(|said| said.contains("1 conversation found"))
    });
    window.until("the offer is to search again", |h| {
        h.attr(OFFER, "aria-label").as_deref() == Some(copy::aria::SEARCH_ON_SERVER_AGAIN)
    });
    assert_eq!(window.harness.count(".server-search"), 0, "a footer");
    assert_eq!(
        *asked.lock().unwrap_or_else(|e| e.into_inner()),
        ["spreadsheet"]
    );
}

/// The chips the header draws are the parser's own reading of a line with quotes and operators.
#[test]
fn the_headers_chips_are_the_parsers_tokens() {
    let mut window = Window::open(|_| {});
    let line = "from:ada subject:\"lunch plans\" \"on friday\" is:unread";
    search(&mut window, line);
    window.harness.advance(ms(800));
    put_away(&mut window);
    window.until("the search is the header", |h| {
        h.count(".list-head .search-face .ds-chip") == 3
    });
    let parsed = mail_client::search::parse(line, &chrono::Utc, &mail_logic::query::unnamed);
    let drawn: Vec<String> = (1..=3)
        .filter_map(|n| {
            window
                .harness
                .text_of(&format!(".list-head .search-face .ds-chip:nth-child({n})"))
        })
        .map(|text| text.trim().to_owned())
        .collect();
    assert_eq!(drawn, parsed.operator_tokens);
    assert_eq!(
        window
            .harness
            .text_of(".list-head .search-face .search-words")
            .as_deref()
            .map(str::trim),
        Some("\"on friday\"")
    );
}

/// The panel's chips act: a press on an offered operator types it, the keyboard staying in the
/// field; once the search holds it, its remove button takes it out with its value.
#[test]
fn the_panels_chips_type_an_operator_and_take_one_out() {
    let mut window = Window::open(|_| {});
    let tokens = ".ds-search-card .ds-text-field-tokens";
    let first = format!("{tokens} .ds-chip:nth-child(1)");
    search(&mut window, "");
    window.until("the syntax is offered, each chip a button", |h| {
        h.text_of(&format!("{first} .ds-chip-press"))
            .is_some_and(|chip| chip.trim() == "from:")
            && h.count(&format!("{tokens} .ds-chip-remove")) == 0
    });
    window.click(&format!("{first} .ds-chip-press"));
    window.until("the press types from:", |h| {
        h.attr(FIELD, "value").as_deref() == Some("from:")
    });
    window.until("the keyboard is still in the field", |h| {
        h.is_focused(FIELD)
    });

    window.type_text("ada lunch");
    window.harness.advance(ms(800));
    window.until("from:ada is held, with a remove button", |h| {
        h.text_of(&first)
            .is_some_and(|chip| chip.contains("from:ada"))
            && h.count(&format!("{tokens} .ds-chip-remove")) == 1
            && h.count(&format!("{tokens} .ds-chip-press")) == 0
    });
    window.click(&format!("{first} .ds-chip-remove"));
    window.until("from:ada is taken out, its value with it", |h| {
        h.attr(FIELD, "value").as_deref() == Some("lunch")
    });
    window.until("the keyboard is still in the field", |h| {
        h.is_focused(FIELD)
    });
}
