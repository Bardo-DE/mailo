//! The composer's search fields behave as ⌘K's does: Escape empties the text before it closes
//! the field's picker, the `/` menu's own rows narrow by the matcher every picker uses, and the
//! composer's templates are ⌘K's.

use super::*;
use crate::ui::fixtures::{Seen, acct_account, chord, drain_seen, seeded, type_into};
use dioxus::html::input_data::keyboard_types::Modifiers;
use mail_ui_compose::compose::page::Float;
use mail_ui_compose::compose::templates::{
    EMOJI_KEY, every, page_slash_items, save_named, template_rows,
};
use mail_ui_model::copy;

/// The window with the composer open on a fresh draft.
fn opened() -> (
    Window,
    Seen,
    std::sync::Arc<mail_client::testing::SqliteStore>,
    tempfile::TempDir,
) {
    let (store, dir) = seeded();
    let draft = mail_client::testing::core::compose::draft_new(
        &store,
        acct_account(),
        &[],
        "",
        "",
        Utc::now(),
    )
    .unwrap_or_else(|why| panic!("a new draft: {why}"));
    let (window, seen) = Window::open(crate::ui::fixtures::client(&store), draft, None);
    (window, seen, store, dir)
}

/// Escape in the emoji picker's search empties it and leaves the picker up; it is the second
/// Escape, on the empty field, that reaches the popover.
#[tokio::test]
async fn escape_in_the_emoji_search_clears_the_text_before_it_closes_the_picker() {
    let (mut window, _seen, _store, _dir) = opened();
    let page = window.page();
    window.dom.in_runtime(|| {
        let mut page = page;
        page.write().float = Float::EmojiPicker;
    });
    let seen = drain_seen(&mut window.dom);
    let field = seen.one("aria-label", copy::fields::SEARCH_EMOJI);
    type_into(&mut window.dom, field, "cat");
    let typed = window.render();
    assert!(typed.contains("value=\"cat\""), "nothing typed:\n{typed}");

    chord(&mut window.dom, "Escape", Modifiers::empty(), field);
    let cleared = window.render();
    assert!(
        !cleared.contains("value=\"cat\""),
        "Escape left the text:\n{cleared}"
    );
    assert_eq!(
        window.dom.in_runtime(|| page.peek().float.clone()),
        Float::EmojiPicker,
        "the first Escape closed the picker"
    );
}

/// The `/` menu's extra rows narrow like every picker's: a few letters in order find a row, the
/// letters that matched are marked, and the words a row is known by still find it.
#[test]
fn the_slash_menus_own_rows_narrow_fuzzily_like_the_rest() {
    let mut page = page_of("");
    type_text(&mut page, "/emj");
    let offered = page_slash_items(&page);
    let emoji = offered
        .iter()
        .find(|item| item.key == EMOJI_KEY)
        .unwrap_or_else(|| panic!("/emj found no Emoji…: {offered:?}"));
    assert!(!emoji.marks.is_empty(), "no letters marked");

    let mut page = page_of("");
    type_text(&mut page, "/smile");
    assert!(
        page_slash_items(&page)
            .iter()
            .any(|item| item.key == EMOJI_KEY),
        "a word the row is known by no longer finds it"
    );

    let mut page = page_of("");
    type_text(&mut page, "/zzzq");
    assert_eq!(
        page.float,
        Float::Closed,
        "a typed word that matches nothing still offered a menu"
    );
}

/// ⌘K and the composer list the same templates: same order, same labels, same marks.
#[test]
fn the_search_bar_and_the_composer_list_the_same_template_rows() {
    let (window, _seen, store, _dir) = opened();
    let client = crate::ui::fixtures::client(&store);
    let page = window.page();
    for name in ["Weekly notes", "Introduction", "Weekend plans"] {
        window.dom.in_runtime(|| {
            let mut page = page;
            let mut write = page.write();
            write.subject = name.to_owned();
            write.float = Float::SaveTemplate(name.to_owned());
            save_named(&client, &mut write, Utc::now()).unwrap_or_else(|why| panic!("{why}"));
        });
    }
    for typed in ["", "wee", "notes", "zzz"] {
        let bar = crate::ui::command::templates::rows(&client, typed);
        let composer = template_rows(&every(&client), typed);
        assert_eq!(bar, composer, "the two lists differ for {typed:?}");
        if typed == "wee" {
            assert_eq!(bar.len(), 2, "{bar:?}");
            assert!(bar.iter().all(|row| !row.marks.is_empty()));
        }
    }
}

/// Escape in the composer's list of templates empties its field first, as ⌘K's does, and
/// closes the list on the empty one.
#[tokio::test]
async fn escape_in_the_template_list_clears_the_text_before_it_closes_the_list() {
    let (mut window, _seen, store, _dir) = opened();
    let client = crate::ui::fixtures::client(&store);
    let page = window.page();
    window.dom.in_runtime(|| {
        let mut page = page;
        let mut write = page.write();
        write.subject = "Weekly notes".to_owned();
        write.float = Float::SaveTemplate("Weekly notes".to_owned());
        save_named(&client, &mut write, Utc::now()).unwrap_or_else(|why| panic!("{why}"));
        write.float = Float::Templates { active: 0 };
    });
    let seen = drain_seen(&mut window.dom);
    let field = *seen
        .all("aria-label", copy::menus::START_FROM_A_TEMPLATE)
        .last()
        .unwrap_or_else(|| panic!("no field"));
    type_into(&mut window.dom, field, "wee");
    assert!(window.render().contains("value=\"wee\""));

    chord(&mut window.dom, "Escape", Modifiers::empty(), field);
    let cleared = window.render();
    assert!(
        !cleared.contains("value=\"wee\""),
        "Escape left the text:\n{cleared}"
    );
    assert!(
        matches!(
            window.dom.in_runtime(|| page.peek().float.clone()),
            Float::Templates { .. }
        ),
        "the first Escape closed the list"
    );

    chord(&mut window.dom, "Escape", Modifiers::empty(), field);
    window.render();
    assert_eq!(
        window.dom.in_runtime(|| page.peek().float.clone()),
        Float::Closed,
        "Escape on the empty field left the list open"
    );
}
