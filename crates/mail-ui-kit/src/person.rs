//! What can be done with a person named in mail: write to them, copy their address, find their
//! mail, open their card, mark them a VIP. One list, so every place that offers a person — ⌘K's
//! person, the reader's names — offers the same things in the same order, in the same words, and
//! a pick does the same thing wherever it was made.

use crate::menu::MenuItem;
use crate::motion::{Follow, tell};
use crate::view::Shell;
use dioxus::prelude::*;
use ds::prelude::Icon;
use mail_client::Client;
use mail_domain::Address;
use mail_ui_model::copy;

/// Someone named in mail: their address, and the name the mail or the book gave them (empty
/// for none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub address: String,
    pub name: String,
}

impl Person {
    /// What they are called: their name, else their address.
    pub fn shown(&self) -> &str {
        match self.name.trim() {
            "" => &self.address,
            name => name,
        }
    }

    /// Their address as a recipient, under the name they gave.
    fn recipient(&self) -> Address {
        Address {
            name: Some(self.name.trim().to_owned()).filter(|name| !name.is_empty()),
            email: self.address.clone(),
        }
    }
}

/// One thing to do with a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// A new message to them.
    Write,
    /// Their address on the clipboard.
    Copy,
    /// The list shows what they sent.
    MailFrom,
    /// The list shows every conversation they are in.
    MailWith,
    /// Their card in Settings › Contacts, made first when there is none.
    Card,
    /// Mark them a VIP, or take the mark off.
    Vip,
}

impl Act {
    /// Every act, in the order a person's list offers them.
    pub const ALL: [Act; 6] = [
        Act::Write,
        Act::Copy,
        Act::MailFrom,
        Act::MailWith,
        Act::Card,
        Act::Vip,
    ];

    /// The key the act's row carries.
    pub fn key(self) -> &'static str {
        match self {
            Act::Write => "person-write",
            Act::Copy => "person-copy",
            Act::MailFrom => "person-from",
            Act::MailWith => "person-with",
            Act::Card => "person-card",
            Act::Vip => "person-vip",
        }
    }

    /// The act a row keyed `key` does.
    pub fn of_key(key: &str) -> Option<Act> {
        Act::ALL.into_iter().find(|act| act.key() == key)
    }
}

/// The search for what `address` sent.
pub fn mail_from(address: &str) -> String {
    format!("from:{address}")
}

/// The search for every conversation `address` is in: what they sent, and what went to them.
pub fn mail_with(address: &str) -> String {
    [
        mail_from(address),
        mail_logic::query::OR.to_owned(),
        format!("to:{address}"),
    ]
    .join(" ")
}

/// The rows for what can be done with `person`, in [`Act::ALL`]'s order. The card's row says Show
/// Contact Card when a card lists them, else Add to Contacts (a window with no client knows no
/// cards); the VIP row is there only with a `shell` to mark in, and never for the person's own
/// address.
pub fn rows(person: &Person, client: Option<&Client>, shell: Option<&Shell>) -> Vec<MenuItem> {
    let address = person.address.as_str();
    Act::ALL
        .into_iter()
        .filter_map(|act| {
            let (icon, words) = match act {
                Act::Write => (Icon::Pen, copy::menus::NEW_MESSAGE),
                Act::Copy => (Icon::Copy, copy::menus::COPY_ADDRESS),
                Act::MailFrom => (Icon::Search, copy::menus::MAIL_FROM),
                Act::MailWith => (Icon::Mail, copy::menus::MAIL_WITH),
                Act::Card => {
                    match client.and_then(|client| crate::book::card_of(client, address)) {
                        Some(_) => (Icon::Group, copy::menus::SHOW_CONTACT_CARD),
                        None => (Icon::Plus, copy::menus::ADD_TO_CONTACTS),
                    }
                }
                Act::Vip => {
                    let shell = shell.filter(|shell| crate::vip::markable(shell, address))?;
                    (
                        crate::vip::GLYPH,
                        crate::vip::menu_words(&shell.vips, address),
                    )
                }
            };
            Some(MenuItem::icon(act.key(), icon, words))
        })
        .collect()
}

/// Do `act` with `person`. A search is set in the list, `pages` back to the first; the composer,
/// the card and the VIP mark are the window's.
pub fn run(act: Act, person: &Person, shell: Signal<Shell>, pages: Option<Signal<u32>>) {
    let search = |mut shell: Signal<Shell>, query: String| {
        shell.write().search = query;
        if let Some(mut pages) = pages {
            pages.set(1);
        }
    };
    match act {
        Act::Write => {
            let client = crate::live::client();
            // A refusal is the composer's to say (`write_to` logs it).
            let _ = crate::ops::write_to(&client, shell, &[person.recipient()]);
        }
        Act::Copy => {
            crate::hover::copy(&person.address);
            tell(copy::toasts::ADDRESS_COPIED.to_owned(), Follow::Nothing);
        }
        Act::MailFrom => search(shell, mail_from(&person.address)),
        Act::MailWith => search(shell, mail_with(&person.address)),
        Act::Card => show_card(&person.address, &person.name),
        Act::Vip => {
            let vips = shell.peek().vips.clone();
            let name = Some(person.name.as_str()).filter(|name| !name.trim().is_empty());
            let mark = mail_client::vip::toggled(&vips, &person.address);
            crate::vip::set(crate::vip::Marking { shell }, &person.address, name, mark);
        }
    }
}

/// Add to Contacts, or Show Contact Card: `address`'s card, made if there was none (named
/// `in_mail`, the name the mail gave), opened in Settings › Contacts. A new card is said in a
/// toast.
pub fn show_card(address: &str, in_mail: &str) {
    let client = crate::live::client();
    match crate::book::add_sender(&client, address, in_mail) {
        Ok((card, said)) => {
            if let Some(said) = said {
                tell(said, Follow::Nothing);
            }
            crate::settings_at::open_card(card);
        }
        Err(why) => tell(why, Follow::Nothing),
    }
}

#[cfg(test)]
mod tests {
    use super::{Act, Person, mail_from, mail_with, rows};

    fn ada() -> Person {
        Person {
            address: "ada@example.test".to_owned(),
            name: "Ada".to_owned(),
        }
    }

    #[test]
    fn every_act_is_found_by_its_key() {
        for act in Act::ALL {
            assert_eq!(Act::of_key(act.key()), Some(act));
        }
        assert_eq!(Act::of_key("person:ada@example.test"), None);
    }

    #[test]
    fn a_person_is_shown_by_name_else_address() {
        assert_eq!(ada().shown(), "Ada");
        let bare = Person {
            name: "  ".to_owned(),
            ..ada()
        };
        assert_eq!(bare.shown(), "ada@example.test");
        assert_eq!(bare.recipient().name, None);
    }

    #[test]
    fn the_searches_are_the_lists_own_syntax() {
        assert_eq!(mail_from("ada@example.test"), "from:ada@example.test");
        assert_eq!(
            mail_with("ada@example.test"),
            "from:ada@example.test OR to:ada@example.test"
        );
    }

    #[test]
    fn without_a_shell_there_is_no_vip_row_and_without_a_client_no_card() {
        let keys: Vec<String> = rows(&ada(), None, None)
            .into_iter()
            .map(|row| row.key)
            .collect();
        assert_eq!(
            keys,
            [
                Act::Write,
                Act::Copy,
                Act::MailFrom,
                Act::MailWith,
                Act::Card
            ]
            .map(|act| act.key().to_owned())
        );
    }

    #[test]
    fn the_persons_own_address_is_no_vip() {
        let shell = crate::view::Shell {
            accounts: vec![(
                "Ada@Example.test".to_owned(),
                mail_domain::id::account_id_from_uuid(uuid::Uuid::from_u128(1)),
            )],
            ..Default::default()
        };
        let rows = rows(&ada(), None, Some(&shell));
        assert!(rows.iter().all(|row| row.key != Act::Vip.key()));
        let other = Person {
            address: "bob@example.test".to_owned(),
            name: String::new(),
        };
        let rows = super::rows(&other, None, Some(&shell));
        assert_eq!(
            rows.last().map(|row| row.key.as_str()),
            Some(Act::Vip.key())
        );
    }
}
