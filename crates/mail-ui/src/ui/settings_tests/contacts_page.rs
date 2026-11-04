//! Settings › Contacts over the reference fixture: a card made from New Contact, edited from its
//! row and deleted after asking; a group made from New Group and given a member.

use crate::ui::fixtures::{click, drain_seen, type_into, work};
use crate::ui::settings_tests::window::opened_on;
use mail_client::testing::Store;
use mail_ui_kit::view::PageId;
use mail_ui_model::copy;
use mail_ui_settings::contacts::page::key_of;

#[tokio::test]
async fn a_contact_is_made_edited_and_deleted_from_its_page() {
    let built = work();
    let (mut dom, seen) = opened_on(&built, PageId::CONTACTS);
    let seen = click(
        &mut dom,
        seen.one("id", mail_ui_settings::contacts::page::NEW_ROW),
    )
    .merge(drain_seen(&mut dom));
    let seen = type_into(&mut dom, seen.one("id", "contact-name"), "Grace Hopper").merge(seen);
    let seen = type_into(
        &mut dom,
        seen.one("id", "contact-email-0"),
        "grace@example.test",
    )
    .merge(seen);
    assert_eq!(
        built.store.card_of("grace@example.test").unwrap(),
        None,
        "kept before Add Contact"
    );
    let seen = click(&mut dom, seen.one("id", "contact-save"))
        .merge(drain_seen(&mut dom))
        .merge(seen);
    let card = built
        .store
        .card_of("grace@example.test")
        .unwrap()
        .expect("Add Contact kept the card");
    assert_eq!(card.name, "Grace Hopper");

    let row = format!("contacts-pane-card-{}", key_of(&card.id.0));
    let seen = click(&mut dom, seen.one("id", &row))
        .merge(drain_seen(&mut dom))
        .merge(seen);
    let page = dioxus_ssr::render(&dom);
    assert!(page.contains("value=\"Grace Hopper\""), "its page: {page}");
    let phone = *seen
        .all("id", "contact-phone-0")
        .last()
        .expect("the phone field");
    let seen = type_into(&mut dom, phone, "+1 555 0100").merge(seen);
    let save = *seen.all("id", "contact-save").last().expect("Save");
    let seen = click(&mut dom, save)
        .merge(drain_seen(&mut dom))
        .merge(seen);
    let kept = mail_client::testing::core::contacts::people::details(
        &built.store.card(&card.id).unwrap().unwrap(),
    );
    assert_eq!(kept.phones, ["+1 555 0100"]);
    assert_eq!(
        kept.emails,
        ["grace@example.test"],
        "what was not touched stays"
    );

    let delete = seen.one("aria-label", &copy::confirm::delete_contact("Grace Hopper"));
    let seen = click(&mut dom, delete).merge(drain_seen(&mut dom));
    assert!(
        built.store.card(&card.id).unwrap().is_some(),
        "deleted before asking"
    );
    let confirm = *seen
        .all("data-role", "destructive")
        .last()
        .expect("the alert's Delete");
    click(&mut dom, confirm);
    assert_eq!(built.store.card(&card.id).unwrap(), None, "the card stayed");
}

#[tokio::test]
async fn a_group_is_made_and_given_members_from_its_page() {
    let built = work();
    let (mut dom, seen) = opened_on(&built, PageId::CONTACTS);
    let before = built.store.groups().unwrap().len();
    let seen =
        click(&mut dom, seen.one("id", "contacts-pane-new-group")).merge(drain_seen(&mut dom));
    let groups = built.store.groups().unwrap();
    assert_eq!(groups.len(), before + 1, "New Group made no group");
    let made = groups
        .iter()
        .find(|group| group.members.is_empty())
        .expect("the new group")
        .clone();
    let seen = type_into(&mut dom, seen.one("id", "group-name"), "Team").merge(seen);
    let seen = type_into(
        &mut dom,
        seen.one("id", "group-add"),
        "ada@example.test, bob@example.test",
    )
    .merge(seen);
    click(&mut dom, seen.one("id", "group-add-button"));
    let group = built.store.group(&made.id).unwrap().unwrap();
    assert_eq!(
        group.members,
        ["mailto:ada@example.test", "mailto:bob@example.test"]
    );
}

/// A card called `name` at `email`, made before the window opens.
fn card_named(built: &crate::ui::fixtures::Work, name: &str, email: &str) {
    let details = mail_client::contacts::people::Details {
        name: name.to_owned(),
        emails: vec![email.to_owned()],
        ..Default::default()
    };
    mail_ui_kit::book::keep_card(&crate::ui::fixtures::client(&built.store), None, &details)
        .unwrap_or_else(|why| panic!("a card: {why}"));
}

/// The search field filters once the box is still, not on each key of a burst; Escape empties
/// it at once and lists everyone again.
#[tokio::test]
async fn the_contacts_search_waits_for_a_pause_and_escape_empties_it() {
    use crate::ui::fixtures::{chord, settle_until};
    use dioxus::html::input_data::keyboard_types::Modifiers;
    use std::time::Duration;

    let built = work();
    card_named(&built, "Grace Hopper", "grace@example.test");
    card_named(&built, "Alan Turing", "alan@example.test");
    let (mut dom, mut seen) = opened_on(&built, PageId::CONTACTS);
    let page = dioxus_ssr::render(&dom);
    assert!(
        page.contains("Grace Hopper") && page.contains("Alan Turing"),
        "{page}"
    );

    let field = seen.one("id", "contacts-pane-find");
    for typed in ["a", "al", "ala"] {
        type_into(&mut dom, field, typed);
    }
    let burst = dioxus_ssr::render(&dom);
    assert!(
        burst.contains("value=\"ala\"") && burst.contains("Grace Hopper"),
        "the list was filtered before the box was still:\n{burst}"
    );

    settle_until(
        &mut dom,
        &mut seen,
        Duration::from_millis(80),
        Duration::from_secs(10),
        |dom| !dioxus_ssr::render(dom).contains("Grace Hopper"),
    )
    .await;
    assert!(dioxus_ssr::render(&dom).contains("Alan Turing"));

    chord(&mut dom, "Escape", Modifiers::empty(), field);
    drain_seen(&mut dom);
    let cleared = dioxus_ssr::render(&dom);
    assert!(
        !cleared.contains("value=\"ala\"")
            && cleared.contains("Grace Hopper")
            && cleared.contains("Alan Turing"),
        "Escape left the filter:\n{cleared}"
    );
}
