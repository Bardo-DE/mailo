use dioxus::prelude::*;
use mail_domain::*;
use mail_ui_reader::reading::header::{Detail, MessageHead, detail_at, shows_to};

fn person(name: &str, email: &str) -> Address {
    Address {
        name: (!name.is_empty()).then(|| name.to_owned()),
        email: email.to_owned(),
    }
}

#[test]
fn only_the_newest_message_says_everything() {
    let cases: &[(usize, usize, Detail)] = &[
        (0, 1, Detail::Full),
        (0, 3, Detail::Compact),
        (1, 3, Detail::Compact),
        (2, 3, Detail::Full),
    ];
    for &(at, count, want) in cases {
        assert_eq!(detail_at(at, count), want, "{at} of {count}");
    }
}

fn message(to: Vec<Address>, cc: Vec<Address>) -> Message {
    Message {
        id: MessageId::generate(),
        thread: ThreadId::generate(),
        account: crate::ui::fixtures::acct_account(),
        key: MessageKey::Rfc("m1@example.test".to_owned()),
        date: chrono::Utc::now(),
        from: person("Bob", "bob@example.test"),
        reply_to: vec![],
        to,
        cc,
        bcc: vec![],
        subject: "hi".to_owned(),
        in_reply_to: None,
        references: vec![],
        rfc_message_id: None,
        read: ReadState::Read,
        star: Star::Unstarred,
        flag: mail_domain::Hue::Red,
        mailbox: MailboxRole::Inbox,
        labels: vec![],
        body: Body::Absent,
        attachments: vec![],
    }
}

fn drawn(message: Message, detail: Detail) -> String {
    #[component]
    fn Head(message: Message, detail: Detail) -> Element {
        rsx! {
            ds::prelude::Ds {
                appearance: ds::prelude::Appearance::default(),
                material: ds::prelude::Material::Window,
                MessageHead { message, detail, when: ("07:13".to_owned(), "2023-11-15 07:13".to_owned()) }
            }
        }
    }
    let mut dom = dioxus_core::VirtualDom::new_with_props(Head, HeadProps { message, detail });
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

#[test]
fn the_full_header_says_who_when_and_to_whom_once() {
    let to = vec![
        person("Ada", "ada@example.test"),
        person("Grace", "grace@example.test"),
        person("Alan", "alan@example.test"),
        person("", "carol@example.test"),
        person("Dan", "dan@example.test"),
    ];
    let page = drawn(
        message(to, vec![person("Edsger", "edsger@example.test")]),
        Detail::Full,
    );
    assert_eq!(page.matches("class=\"ds-avatar\"").count(), 1, "{page}");
    assert_eq!(page.matches(">Bob<").count(), 1, "{page}");
    // The address is in the name's card, not a word on the line.
    assert!(!page.contains(">bob@example.test<"), "{page}");
    // Each recipient is a name of their own, the first two named and the rest counted.
    assert_eq!(page.matches("class=\"person-name\"").count(), 4, "{page}");
    assert!(
        !page.contains(">Alan<") && !page.contains(">carol@example.test<"),
        "{page}"
    );
    assert!(page.contains("aria-label=\"2023-11-15 07:13\""), "{page}");
    assert!(page.contains(">07:13<"), "{page}");
    // The composer's rows: a label, then its value.
    for (row, label, value) in [
        ("from", "From", "Bob"),
        ("to", "To", "Ada"),
        ("to", "To", "Grace"),
        ("to", "To", "+3"),
        ("cc", "Cc", "Edsger"),
    ] {
        let at = page
            .find(&format!("data-row=\"{row}\""))
            .unwrap_or_else(|| panic!("no {row} row: {page}"));
        let rest = &page[at..];
        let (label_at, value_at) = (
            rest.find(&format!(">{label}<")),
            rest.find(&format!(">{value}<")),
        );
        assert!(
            matches!((label_at, value_at), (Some(l), Some(v)) if l < v),
            "{row}: {page}"
        );
    }
}

#[test]
fn a_message_to_only_you_has_no_to_line() {
    let yours = vec!["me@example.test".to_owned()];
    let me = person("Me", "Me@Example.test");
    let ada = person("Ada", "ada@example.test");
    type Case<'a> = (&'a str, Vec<Address>, Vec<Address>, bool);
    let cases: &[Case] = &[
        ("only you", vec![me.clone()], vec![], false),
        (
            "you and another",
            vec![me.clone(), ada.clone()],
            vec![],
            true,
        ),
        (
            "you, copied to another",
            vec![me.clone()],
            vec![ada.clone()],
            true,
        ),
        ("someone else", vec![ada.clone()], vec![], true),
        ("nobody", vec![], vec![ada.clone()], false),
    ];
    for (name, to, cc, want) in cases {
        let got = shows_to(&message(to.clone(), cc.clone()), &yours);
        assert_eq!(got, *want, "{name}");
    }
}

#[test]
fn a_compact_header_is_who_and_when() {
    let page = drawn(
        message(vec![person("Ada", "ada@example.test")], vec![]),
        Detail::Compact,
    );
    assert_eq!(page.matches("class=\"ds-avatar\"").count(), 1, "{page}");
    assert!(page.contains(">Bob<") && page.contains(">07:13<"), "{page}");
    assert!(!page.contains("data-row=\""), "{page}");
    assert!(!page.contains(">bob@example.test<"), "{page}");
}

/// The count at the end of a line of recipients names the rest when pressed.
#[tokio::test]
async fn the_count_of_the_rest_names_them_when_pressed() {
    use crate::ui::fixtures::{click, dispatching, rebuild_into};
    #[component]
    fn Line() -> Element {
        let people = ["Ada", "Grace", "Alan", "Dan"]
            .map(|name| person(name, &format!("{}@example.test", name.to_lowercase())))
            .to_vec();
        rsx! {
            ds::prelude::Ds {
                appearance: ds::prelude::Appearance::default(),
                material: ds::prelude::Material::Window,
                mail_ui_reader::sender::People { people }
            }
        }
    }
    dispatching();
    let mut dom = dioxus_core::VirtualDom::new(Line);
    let seen = rebuild_into(&mut dom);
    let page = dioxus_ssr::render(&dom);
    assert!(page.contains(">+2<") && !page.contains(">Alan<"), "{page}");
    let more = seen.one("aria-label", &mail_ui_model::copy::aria::show_all_people(4));
    click(&mut dom, more);
    dom.render_immediate(&mut dioxus_core::NoOpMutations);
    let page = dioxus_ssr::render(&dom);
    assert!(page.contains(">Alan<") && page.contains(">Dan<"), "{page}");
    assert!(!page.contains(">+2<"), "{page}");
}
