//! A message's header in the reader, set as the composer's is (`compose::props`): one label
//! column, From, To and Cc, every value at one x, and one hairline under them. From is the
//! sender's avatar and name with the date at the row's end; To is drawn unless the message went
//! only to you. Each name opens that person's card (`crate::sender`).
//!
//! A conversation of one message has one header. In a longer one every message keeps a compact
//! header (a small avatar, the name, the date) and only the message the conversation opens on,
//! the newest, says whom it went to and carries the trust glyph beside the sender's name
//! (`ui::checks`), with one line under the head when the sender may not be who they say.

use super::sender_face;
use crate::checks::{TrustMark, trust_of, use_checks};
use dioxus::prelude::*;
use ds::components::content::avatar::{AvatarFace, AvatarSize};
use ds::components::content::label::{LabelRole, LabelStyle};
use ds::components::overlays::inline_banner::InlineBanner;
use ds::prelude::*;
use mail_domain::{BlobId, Message, MessageId};
use mail_ui_kit::lines::{Line, List, list_label};
use mail_ui_kit::person::Person;
use mail_ui_kit::text::{address, from_name};

/// How much a message's header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// The newest message: whom it went to, and what is known about its sender.
    Full,
    /// An earlier message: who wrote it and when.
    Compact,
}

/// The detail for the message at `at` of `count`: the last is the full one.
pub fn detail_at(at: usize, count: usize) -> Detail {
    if at + 1 == count {
        Detail::Full
    } else {
        Detail::Compact
    }
}

/// Whether the To row is drawn: unless the message went to nobody but `yours` (the person's own
/// addresses, lowercased), which is what a message in their inbox goes to and needs no saying.
pub fn shows_to(message: &Message, yours: &[String]) -> bool {
    let only_you = message.cc.is_empty()
        && message.to.len() == 1
        && yours.contains(&message.to[0].email.trim().to_lowercase());
    !message.to.is_empty() && !only_you
}

/// What the newest message's header is keyed by in the reader: the message and its body, which
/// the trust glyph and the logo are looked up for, and the reader's `landed`, moved when the
/// seal finds what the message's protection says.
#[derive(Debug, Clone, PartialEq)]
pub struct Extras {
    pub checked: (MessageId, Option<BlobId>),
    pub landed: Signal<u64>,
}

/// One message's header.
#[component]
pub fn MessageHead(
    message: Message,
    detail: Detail,
    /// When it came, briefly, and in full for the tooltip.
    when: (String, String),
    #[props(default)] extras: Option<Extras>,
    /// The person's own addresses, lowercased: a To line of only them is not drawn.
    #[props(default)]
    yours: Vec<String>,
) -> Element {
    let (short, full) = when;
    let date = when_label(short.clone(), full.clone());
    let face = sender_face(&message);
    let (name, _) = named(&message);
    match detail {
        Detail::Compact => rsx! {
            header { class: "msg-head", "data-detail": "compact",
                Avatar { initial: face.initial, size: AvatarSize::Size22, tone: face.tone }
                div { class: "msg-line",
                    Label { text: name, style: LabelStyle::Headline, common: mail_ui_kit::common::classed("truncate") }
                    crate::receipt::ReceiptMark { message: message.id }
                    {date}
                }
            }
        },
        Detail::Full => match extras {
            Some(Extras {
                checked: (id, raw),
                landed,
            }) => rsx! {
                FullHead { key: "{id}-{raw:?}", message, when: (short, full), yours, landed }
            },
            None => full_head(&message, date, &yours, None, None),
        },
    }
}

/// The newest message's header in a reader: the logo, and what is known about the sender, looked
/// up off the thread that draws. Keyed by its parent on the message and its body, so a body
/// arriving asks again and one message's answer is never drawn under another.
#[component]
fn FullHead(
    message: Message,
    when: (String, String),
    yours: Vec<String>,
    landed: Signal<u64>,
) -> Element {
    let checks = use_checks(message.id, message.body.raw());
    let looks = mail_ui_kit::cache::use_looks();
    // Moved by the seal once it knows; what it found is kept in `looks`, and read here.
    let _ = landed();
    let look = mail_ui_kit::pgp::last(&looks, message.id, message.body.raw());
    let results = checks();
    let trust = trust_of(look.as_ref(), results.as_ref().and_then(Option::as_ref));
    let (short, full) = when;
    let date = when_label(short, full);
    let face = row_face(&message);
    let avatar = rsx! {
        crate::brand::ReaderAvatar {
            key: "{message.id}-{message.body.raw():?}",
            message: message.id,
            body: message.body.raw(),
            from: message.from.email.clone(),
            face,
        }
    };
    let forged = trust
        .forged()
        .then(|| mail_ui_model::copy::errors::may_not_be_from(&named(&message).0));
    rsx! {
        {full_head(&message, date, &yours, Some(avatar), Some(trust))}
        if let Some(said) = forged {
            div { class: "trust-warn",
                InlineBanner { severity: Severity::Warn, icon: Some(Icon::TriangleAlert), text: said }
            }
        }
    }
}

/// The sender's face in the From row, at the size the composer's rows draw a person at.
fn row_face(message: &Message) -> AvatarFace {
    AvatarFace {
        size: AvatarSize::Size22,
        ..sender_face(message)
    }
}

/// What a header calls the sender, and the address its name's tooltip gives: a sender who gave
/// no name is named by their address, once, with no tooltip.
pub(super) fn named(message: &Message) -> (String, String) {
    let name = from_name(message);
    let addr = address(message);
    if name.trim().is_empty() {
        (addr, String::new())
    } else {
        (name, addr)
    }
}

/// The date at the end of a header's line: brief, and in full as its tooltip and its name.
pub(super) fn when_label(short: String, full: String) -> Element {
    rsx! {
        Tooltip { text: full.clone(),
            // The full date is the time's name as well as its tip: what a screen reader says.
            time { class: "msg-when", aria_label: full,
                Label { text: short, role: LabelRole::Tertiary, style: LabelStyle::Footnote }
            }
        }
    }
}

/// The full header's markup, the composer's rows: From, the avatar (`avatar`, else the sender's
/// initial) and the name with the trust glyph beside it and the date at the row's end, then To
/// and Cc. `data-standing` says the sender's checks once they are known.
fn full_head(
    message: &Message,
    date: Element,
    yours: &[String],
    avatar: Option<Element>,
    trust: Option<crate::checks::Trust>,
) -> Element {
    // The sender as their card and editor start from: the name the mail gave, and whom.
    let sender = Person {
        address: message.from.email.clone(),
        name: from_name(message),
    };
    let face = row_face(message);
    let to = shows_to(message, yours).then(|| message.to.clone());
    let cc = (!message.cc.is_empty()).then(|| message.cc.clone());
    let avatar = avatar.unwrap_or_else(|| {
        rsx! {
            div { class: "reader-av",
                Avatar { initial: face.initial, size: face.size, tone: face.tone }
            }
        }
    });
    let standing = trust
        .as_ref()
        .and_then(|trust| trust.standing)
        .map(|standing| standing.word());
    let mark = trust.and_then(|trust| trust.mark.map(|mark| (mark, trust.lines)));
    rsx! {
        header { class: "msg-head reader-meta", "data-detail": "full", "data-standing": standing,
            Line { label: mail_ui_model::copy::fields::FROM, row: "from",
                {avatar}
                crate::sender::PersonName { person: sender }
                if let Some((mark, lines)) = mark {
                    TrustMark { mark, lines }
                }
                crate::receipt::ReceiptMark { message: message.id }
                {date}
            }
            if let Some(to) = to {
                Line { label: list_label(List::To), row: "to",
                    crate::sender::People { people: to }
                }
            }
            if let Some(cc) = cc {
                Line { label: list_label(List::Cc), row: "cc",
                    crate::sender::People { people: cc }
                }
            }
        }
    }
}
