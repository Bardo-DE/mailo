//! A person edited from their name in mail: the name the field starts with, and the whole way
//! from a right click on the sender's name to the book holding what was typed.

use std::sync::Arc;

use dioxus::prelude::*;
use dioxus_core::VirtualDom;
use ds::prelude::*;
use mail_client::testing::{SqliteStore, Store};

use crate::ui::fixtures::{
    Seen, click, dispatching, drain_seen, pick_named, rebuild_into, right_click, type_into,
};
use crate::ui::settings_tests::contacts::{ADDED, HEARD, the_book};
use mail_ui_kit::person::Person;
use mail_ui_model::copy;
use mail_ui_reader::sender::{PersonName, starting_name};

#[test]
fn the_field_starts_with_the_books_name_else_the_one_the_mail_gave() {
    let (store, _dir) = the_book();
    let kept = store
        .contact(ADDED)
        .unwrap()
        .and_then(|contact| contact.name);
    assert_eq!(
        starting_name(&crate::ui::fixtures::client(&store), ADDED, "Someone Else"),
        kept.unwrap_or_else(|| "Someone Else".to_owned()),
        "the book's name is not the one offered"
    );
    assert_eq!(
        starting_name(
            &crate::ui::fixtures::client(&store),
            "nobody@example.test",
            "Nobody"
        ),
        "Nobody",
        "a stranger's name is not the mail's"
    );
}

/// A sender's name, inside a quire root as the reader has it: the menu and the editor float in
/// its overlay.
#[component]
fn Named() -> Element {
    rsx! {
        Ds {
            appearance: Appearance::default(),
            material: Material::Window,
            stylesheet: ds::assembly::ds::Inject::Host,
            PersonName { person: Person { address: HEARD.to_owned(), name: "Dana".to_owned() } }
        }
    }
}

/// The renders after an act, until the dom has nothing left to do.
async fn settled(dom: &mut VirtualDom, mut seen: Seen) -> Seen {
    for _ in 0..40 {
        let quiet = std::time::Duration::from_millis(150);
        if tokio::time::timeout(quiet, dom.wait_for_work())
            .await
            .is_err()
        {
            break;
        }
        dom.render_immediate(&mut seen);
    }
    seen.merge(drain_seen(dom))
}

#[tokio::test]
async fn edit_contact_from_the_senders_name_keeps_the_typed_name_in_the_book() {
    dispatching();
    let (store, _dir): (Arc<SqliteStore>, _) = the_book();
    let mut dom = VirtualDom::new(Named).with_root_context(crate::ui::fixtures::client(&store));
    let seen = rebuild_into(&mut dom);
    let seen = settled(&mut dom, seen).await;

    let opened = right_click(&mut dom, seen.fixed("class", "person-name")[0]);
    let opened = settled(&mut dom, opened).await;
    let editing = pick_named(&mut dom, &opened, copy::menus::EDIT_CONTACT).await;
    let editing = settled(&mut dom, editing).await;
    let page = dioxus_ssr::render(&dom);
    assert!(page.contains("contact-editor"), "no editor: {page}");

    type_into(
        &mut dom,
        editing.one("aria-label", copy::fields::NAME),
        "Dana Whitfield",
    );
    click(
        &mut dom,
        editing.one("aria-label", &copy::aria::save_contact(HEARD)),
    );
    settled(&mut dom, Seen::default()).await;
    let kept = store
        .contact(HEARD)
        .unwrap()
        .and_then(|contact| contact.name);
    assert_eq!(
        kept.as_deref(),
        Some("Dana Whitfield"),
        "the name was not kept"
    );
    assert!(
        !dioxus_ssr::render(&dom).contains("contact-editor"),
        "the editor stayed open after Save"
    );
}

/// A click on a name opens their card: their address, then what can be done with them, Edit
/// Contact… among it. Copy Address puts the address on the clipboard and closes the card.
#[tokio::test]
async fn a_click_on_a_name_opens_their_card() {
    use mail_ui_kit::host::{Ask, Recorder};
    dispatching();
    let (store, _dir): (Arc<SqliteStore>, _) = the_book();
    let recorder = Recorder::default();
    let mut dom = VirtualDom::new(Named)
        .with_root_context(crate::ui::fixtures::client(&store))
        .with_root_context(recorder.host());
    let seen = rebuild_into(&mut dom);
    let seen = settled(&mut dom, seen).await;

    let opened = click(&mut dom, seen.fixed("class", "person-name")[0]);
    let opened = settled(&mut dom, opened).await;
    let page = dioxus_ssr::render(&dom);
    assert!(page.contains("person-card"), "no card: {page}");
    for words in [
        HEARD,
        copy::menus::NEW_MESSAGE,
        copy::menus::COPY_ADDRESS,
        copy::menus::MAIL_WITH,
        copy::menus::EDIT_CONTACT,
    ] {
        assert!(page.contains(words), "the card has no {words:?}: {page}");
    }
    let copied = pick_named(&mut dom, &opened, copy::menus::COPY_ADDRESS).await;
    settled(&mut dom, copied).await;
    assert!(
        recorder.asked().contains(&Ask::Copy(HEARD.to_owned())),
        "{:?}",
        recorder.asked()
    );
    assert!(
        !dioxus_ssr::render(&dom).contains("person-card"),
        "the card stayed open after a pick"
    );
}
