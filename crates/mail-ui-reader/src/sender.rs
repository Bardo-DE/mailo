//! A person named in the reader's header — the sender, and each of whom it went to. A click on
//! their name opens their card, and a right click the same list at the pointer: what can be done
//! with them (`mail_ui_kit::person`, the list ⌘K's person offers), and Edit Contact…, whose
//! popover hangs from the name. It holds the one thing a person writes into the book by hand,
//! the name; Return or Save keeps it ([`book::name`]), and the entry is then the user's, so mail
//! never renames it. Escape or a click outside leaves the book as it was.

use dioxus::prelude::*;
use ds::base::geometry::placement::{Align, Side};
use ds::components::content::label::{LabelRole, LabelStyle};
use ds::components::controls::button_model::Bezel;
use ds::components::overlays::popover::Arrow;
use ds::host::measure::MountedRef;
use ds::prelude::*;
use ds::root::common::Common;
use mail_client::Client;
use mail_domain::Address;

use ds::components::content::avatar::{
    AvatarFace, AvatarShape, AvatarSize, AvatarTone, person_hue,
};
use mail_ui_kit::book;
use mail_ui_kit::menu::{MenuItem, MenuKey, anchor_at, menu_key};
use mail_ui_kit::motion::{Follow, tell};
use mail_ui_kit::person::{Act, Person};
use mail_ui_kit::press::on_primary;
use mail_ui_model::copy;

/// The name the field starts with: the book's, else the one the mail gave.
pub fn starting_name(client: &Client, address: &str, in_mail: &str) -> String {
    client
        .stored()
        .contact(address)
        .ok()
        .flatten()
        .and_then(|contact| contact.name)
        .unwrap_or_else(|| in_mail.to_owned())
}

/// Keep `typed` as `address`'s name, saying only a failure.
fn keep(address: &str, typed: &str, on_close: EventHandler<()>) {
    let client = mail_ui_kit::live::client();
    if let Err(why) = book::name(&client, address, typed) {
        tell(why, Follow::Nothing);
    }
    on_close.call(());
}

/// The editor for `address`, hung from `anchor`; `in_mail` is the name the message gave.
#[component]
pub fn ContactEditor(
    address: String,
    in_mail: String,
    anchor: Option<MountedRef>,
    on_close: EventHandler<()>,
) -> Element {
    let mut typed = use_signal({
        let address = address.clone();
        move || {
            let client = mail_ui_kit::live::client();
            starting_name(&client, &address, &in_mail)
        }
    });
    let on_enter = address.clone();
    rsx! {
        Popover {
            anchor: anchor_at(anchor),
            placement: Placement::new(Side::Bottom, Align::Start),
            gap: Px(4.0),
            arrow: Arrow::None,
            onclose: move |()| on_close.call(()),
            div {
                class: "contact-editor",
                onkeydown: move |event: KeyboardEvent| match menu_key(&event.key().to_string()) {
                    Some(MenuKey::Enter) => {
                        event.prevent_default();
                        keep(&on_enter, &typed.peek(), on_close);
                    }
                    Some(MenuKey::Escape) => {
                        event.stop_propagation();
                        on_close.call(());
                    }
                    _ => {}
                },
                Label { text: address.clone(), role: LabelRole::Secondary, style: LabelStyle::Footnote }
                TextField {
                    label: copy::fields::NAME.to_owned(),
                    placeholder: copy::fields::NAME.to_owned(),
                    value: typed(),
                    focus: FieldFocus::OnMount,
                    oninput: move |value: String| typed.set(value),
                }
                Button {
                    label: copy::controls::SAVE,
                    common: Common { aria_label: Some(copy::aria::save_contact(&address)), ..Common::default() },
                    onclick: on_primary({
                        let address = address.clone();
                        move || keep(&address, &typed.peek(), on_close)
                    }),
                }
            }
        }
    }
}

/// Where a person's name is: at rest, its card open under it, its menu open at the pointer, or
/// its editor open.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Asked {
    Resting,
    Card,
    Menu(Rect),
    Editing,
}

/// The key of Edit Contact…, the one row a name offers past the shared list.
const EDIT: &str = "edit-contact";

/// What a name offers, in its card and its menu: [`mail_ui_kit::person::rows`], with Edit
/// Contact… after the card's row.
fn offered(person: &Person, shell: Option<&mail_ui_kit::view::Shell>) -> Vec<MenuItem> {
    // A window with no client (a lone component in a test) knows no cards.
    let client = mail_ui_kit::live::try_client();
    let mut rows = mail_ui_kit::person::rows(person, client.as_ref(), shell);
    let card = rows
        .iter()
        .position(|row| row.key == Act::Card.key())
        .map_or(rows.len(), |at| at + 1);
    rows.insert(
        card,
        MenuItem::icon(EDIT, Icon::Pen, copy::menus::EDIT_CONTACT),
    );
    rows
}

/// A person's name in the reader's header: the sender's in the headline type, a recipient's
/// (`quiet`) in the body's, and the VIP mark after it when they are one. A click opens their
/// card under it — who they are, then what can be done with them, the list ⌘K's person offers —
/// and a right click the same list at the pointer. Edit Contact… opens [`ContactEditor`] hung
/// from the name. The acts that need the window (writing, a search, the VIP mark) are there
/// where the reader says where its shell is ([`mail_ui_kit::vip::Marking`]).
#[component]
pub fn PersonName(person: Person, #[props(default)] quiet: bool) -> Element {
    let mut asked = use_signal(|| Asked::Resting);
    let mut at = use_signal(|| None::<MountedRef>);
    let marking = try_use_context::<mail_ui_kit::vip::Marking>();
    let vip = marking.is_some_and(|marking| marking.shell.read().vips.includes(&person.address));
    let open = !matches!(asked(), Asked::Resting);
    let items = if open {
        offered(
            &person,
            marking.map(|marking| marking.shell.read().clone()).as_ref(),
        )
    } else {
        Vec::new()
    };
    let on_pick = {
        let person = person.clone();
        move |key: String| {
            if key == EDIT {
                asked.set(Asked::Editing);
                return;
            }
            asked.set(Asked::Resting);
            let Some(act) = Act::of_key(&key) else {
                return;
            };
            mail_ui_kit::person::run(act, &person, marking.map(|marking| marking.shell), None);
        }
    };
    let style = if quiet {
        LabelStyle::Body
    } else {
        LabelStyle::Headline
    };
    let shown = person.shown().to_owned();
    rsx! {
        span {
            class: "person-name",
            role: "button",
            "aria-haspopup": "menu",
            "aria-expanded": if open { "true" } else { "false" },
            onclick: move |event: MouseEvent| {
                event.stop_propagation();
                asked.set(Asked::Card);
            },
            oncontextmenu: move |event: MouseEvent| {
                event.prevent_default();
                event.stop_propagation();
                asked.set(Asked::Menu(mail_ui_kit::menu::point_rect(event.client_coordinates())));
            },
            onmounted: move |event: MountedEvent| at.set(Some(MountedRef(event.data()))),
            Label { text: shown, style, common: mail_ui_kit::common::classed("truncate") }
        }
        if vip {
            mail_ui_kit::vip::VipMark {}
        }
        match asked() {
            Asked::Resting => rsx! {},
            Asked::Card => rsx! {
                PersonCard {
                    person: person.clone(),
                    anchor: at(),
                    items,
                    on_pick,
                    on_close: move |()| {
                        if *asked.peek() == Asked::Card {
                            asked.set(Asked::Resting);
                        }
                    },
                }
            },
            Asked::Menu(placed) => rsx! {
                mail_ui_kit::menu::Floating {
                    anchor: None,
                    placed: Some(placed),
                    title: String::new(),
                    items,
                    on_pick,
                    on_close: move |()| {
                        if matches!(*asked.peek(), Asked::Menu(_)) {
                            asked.set(Asked::Resting);
                        }
                    },
                }
            },
            Asked::Editing => rsx! {
                ContactEditor {
                    address: person.address.clone(),
                    in_mail: person.name.clone(),
                    anchor: at(),
                    on_close: move |()| asked.set(Asked::Resting),
                }
            },
        }
    }
}

/// A person's card, hung from their name: their face, their name and their address, then what
/// can be done with them as a menu drawn in the card.
#[component]
fn PersonCard(
    person: Person,
    anchor: Option<MountedRef>,
    items: Vec<MenuItem>,
    on_pick: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let face = AvatarFace {
        initial: crate::reading::initial(person.shown()),
        size: AvatarSize::Size34,
        tone: AvatarTone::Person(person_hue(&person.address)),
        shape: AvatarShape::Round,
    };
    let named = !person.name.trim().is_empty();
    rsx! {
        Popover {
            anchor: anchor_at(anchor),
            placement: Placement::new(Side::Bottom, Align::Start),
            gap: Px(4.0),
            arrow: Arrow::None,
            onclose: move |()| on_close.call(()),
            div { class: "person-card",
                div { class: "person-card-head",
                    Avatar { initial: face.initial, size: face.size, tone: face.tone }
                    div { class: "person-card-who",
                        Label { text: person.shown().to_owned(), style: LabelStyle::Headline, common: mail_ui_kit::common::classed("truncate") }
                        if named {
                            Label { text: person.address.clone(), role: LabelRole::Secondary, style: LabelStyle::Footnote, common: mail_ui_kit::common::classed("truncate") }
                        }
                    }
                }
                mail_ui_kit::menu::Floating {
                    anchor: None,
                    title: String::new(),
                    items,
                    flow: Flow::Inline,
                    on_pick: move |key: String| on_pick.call(key),
                    on_close: move |()| on_close.call(()),
                }
            }
        }
    }
}

/// A line of recipients: the first [`NAMED`] by name (their address when they gave none), each
/// its own [`PersonName`], then how many more, which shows the rest when pressed.
#[component]
pub fn People(people: Vec<Address>) -> Element {
    let mut all = use_signal(|| false);
    let more = people.len().saturating_sub(NAMED);
    let shown = if all() || more == 0 {
        people.len()
    } else {
        NAMED
    };
    let last = shown.saturating_sub(1);
    rsx! {
        span { class: "people", "data-all": if all() { "true" } else { "false" },
            for (at, person) in people.iter().take(shown).enumerate() {
                span { key: "{at}-{person.email}", class: "person",
                    PersonName { person: person_of(person), quiet: true }
                    if at < last {
                        Label { text: copy::status::LIST_SEPARATOR.to_owned() }
                    }
                }
            }
            if more > 0 && !all() {
                Button {
                    label: copy::controls::more_people(more),
                    bezel: Bezel::Inline,
                    common: Common { aria_label: Some(copy::aria::show_all_people(people.len())), ..Common::default() },
                    onclick: on_primary(move || all.set(true)),
                }
            }
        }
    }
}

/// How many names a line of recipients spells out before it counts the rest.
pub const NAMED: usize = 2;

/// `address` as a person: their address, and the name the mail gave.
pub fn person_of(address: &Address) -> Person {
    Person {
        address: address.email.clone(),
        name: address.name.clone().unwrap_or_default(),
    }
}
