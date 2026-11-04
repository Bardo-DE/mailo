//! New mail, said out loud: which of the messages a pass stored deserve a word, how many words a
//! burst gets, and what a banner's buttons do. Where the words go is the seam [`Notifier`]; reading
//! the store after a pass and raising the notifications are `mail_core::notify`'s.

use crate::own::Own;
use crate::vip::Vips;
use chrono::{DateTime, Utc};
use mail_domain::ThreadId;
use mail_domain::{MailboxRole, Message, ReadState, Snooze};
use porter_core::AccountId;

/// Whether `mailo watch` raises notifications. On unless someone turned it off.
///
/// mail-app keeps the choice (`notifications.new_mail` in `mailo/settings.toml`) and hands it to
/// the watch as this. `notify.json` is where it was kept before; [`load`] reads it once, for
/// that file to start from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Setting {
    #[default]
    On,
    Off,
}

/// Whose new mail is announced: everyone's, or only the VIPs' ([`crate::vip`]).
///
/// mail-app keeps the choice (`notifications.notify_for`) and hands it to whatever announces, as
/// it hands [`Setting`] to the watch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Whom {
    #[default]
    Everyone,
    Vips,
}

/// The senders whose mail may be announced, as [`verdict`] weighs them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Heard {
    /// Anyone's.
    #[default]
    Everyone,
    /// These people's alone.
    Only(Vips),
}

impl Heard {
    pub fn hears(&self, email: &str) -> bool {
        match self {
            Heard::Everyone => true,
            Heard::Only(vips) => vips.includes(email),
        }
    }
}

/// Which mailboxes' new mail is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Places {
    /// The Inbox, and nothing else.
    #[default]
    Inbox,
    /// The Inbox and every other mailbox the account's server lists as followed: a folder the
    /// person made, or the server's own without a special use. The ones nobody is waiting on are
    /// never followed (Archive, All Mail, Starred and Important have a special use), and mail
    /// filed as Junk, Trash, Sent or Drafts is not announced wherever it is held. Mail filed as
    /// Archive is announced only from a followed folder, which is how a folder the person made
    /// is filed.
    Followed,
}

/// The sound a banner plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sound {
    /// The system's own sound for a message.
    #[default]
    Default,
    Ping,
    Glass,
    Pop,
    Submarine,
    /// A banner that makes no sound.
    Silent,
}

/// What the person has asked to hear about, beside what is never worth a word ([`verdict`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Interest {
    pub places: Places,
    /// Addresses of the accounts that are never announced, in lowercase.
    pub muted: Vec<String>,
    pub sound: Sound,
}

impl Interest {
    /// Whether the account signed in as `address` is one to hear about.
    pub fn hears(&self, address: &str) -> bool {
        let address = address.trim().to_ascii_lowercase();
        !self.muted.contains(&address)
    }
}

/// Why an arrival is not announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quiet {
    /// Read already — on another device, or by a rule, before this pass looked.
    Read,
    /// Not in the inbox: spam, trash, sent, drafts and archive are all places nobody is waiting
    /// on.
    Elsewhere(MailboxRole),
    /// From one of the user's own addresses: mail this client, or the user elsewhere, sent.
    FromSelf,
    /// Its conversation is snoozed, and waking it early is the one thing snoozing asked not to.
    Snoozed,
    /// Dated before the account was first watched: backfill, not news.
    BeforeFloor,
    /// Not from a VIP, while only the VIPs' mail is announced.
    NotVip,
}

/// Whether one arrival is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Announce,
    Quiet(Quiet),
}

/// Whether `message`, just stored, is worth interrupting someone for.
///
/// `snooze` is its conversation's, `floor` is when its account was first watched
/// ([`floor::armed`]), and `heard` whose mail is announced at all. `followed` is whether it is held
/// in a mailbox the person follows beside the Inbox, which only [`Places::Followed`] asks. Checked
/// in the order a reader would explain it: where it is, whether it has been read, who sent it,
/// then when.
pub fn verdict(
    message: &Message,
    snooze: Snooze,
    own: &Own,
    heard: &Heard,
    floor: DateTime<Utc>,
    now: DateTime<Utc>,
    followed: bool,
) -> Verdict {
    let waited_on = message.mailbox == MailboxRole::Inbox
        || (followed
            && !matches!(
                message.mailbox,
                MailboxRole::Spam | MailboxRole::Trash | MailboxRole::Sent | MailboxRole::Drafts
            ));
    let quiet = if !waited_on {
        Some(Quiet::Elsewhere(message.mailbox))
    } else if message.read == ReadState::Read {
        Some(Quiet::Read)
    } else if own.includes(&message.from.email) {
        Some(Quiet::FromSelf)
    } else if !heard.hears(&message.from.email) {
        Some(Quiet::NotVip)
    } else if matches!(snooze, Snooze::Until(at) if at > now) {
        Some(Quiet::Snoozed)
    } else if message.date < floor {
        Some(Quiet::BeforeFloor)
    } else {
        None
    };
    quiet.map_or(Verdict::Announce, Verdict::Quiet)
}

/// The verdict on an arrival a rule asked to be told of: said even when read, filed away or
/// snoozed (and, under "VIPs only", when the sender is not one: [`announce`] asks it as if
/// everyone were heard), which is what asking was for, and still not the user's own mail or mail from before
/// the account was watched, which no rule meant.
pub fn heard(verdict: Verdict) -> Verdict {
    match verdict {
        Verdict::Quiet(Quiet::Read | Quiet::Elsewhere(_) | Quiet::Snoozed) => Verdict::Announce,
        other => other,
    }
}

/// What a click on a notification should open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opens {
    Thread(ThreadId),
    /// A summary of several: there is no one conversation to open, so the inbox, with the
    /// account the summary was about, so the window can show it where that account's mail is.
    Inbox(AccountId),
}

/// One desktop notification, before any transport has touched it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub account: AccountId,
    /// The headline: who it is from, or how many arrived.
    pub summary: String,
    /// Plain text. A transport that speaks markup escapes it.
    pub body: String,
    pub opens: Opens,
    /// Whether the banner offers Reply: only a banner about one message does, and the reply is
    /// to its conversation ([`Opens::Thread`]), in the account the message is in.
    pub reply: bool,
}

/// What a button on a banner, or a click on the banner, does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Show what the notification was about.
    Open,
    /// Archive the conversation, in the window, which offers the Undo.
    Archive,
    /// Open the composer on a reply to the conversation.
    Reply,
}

/// The action key of a banner's click, which every server invokes for the banner itself.
pub const DEFAULT_ACTION: &str = "default";
/// The action keys of its two buttons.
pub const ARCHIVE_ACTION: &str = "archive";
pub const REPLY_ACTION: &str = "reply";

/// What follows from the action a server or the system reports for a notification that opens
/// `opens` and offers Reply when `reply`: nothing for an action that was not offered.
pub fn act_for(action: &str, opens: &Opens, reply: bool) -> Option<Act> {
    match (action, opens, reply) {
        (DEFAULT_ACTION, _, _) => Some(Act::Open),
        (ARCHIVE_ACTION, Opens::Thread(_), _) => Some(Act::Archive),
        (REPLY_ACTION, Opens::Thread(_), true) => Some(Act::Reply),
        _ => None,
    }
}

/// At most this many arrivals are announced one by one; more become one summary.
pub const ONE_BY_ONE: usize = 3;

/// At most this many senders are named in a summary.
const NAMED: usize = 3;

/// A pass's announced arrivals as notifications: one each for a few, one summary for a burst.
///
/// A summary names the senders who sent most, ties broken by who came first, because "7 new
/// messages" alone sends the user to the window to find out whether any of them matter.
pub fn batch(account: AccountId, announced: &[&Message]) -> Vec<Notification> {
    if announced.len() <= ONE_BY_ONE {
        return announced
            .iter()
            .map(|message| Notification {
                account: account.clone(),
                summary: sender(message),
                body: mail_domain::subject_or_none(&message.subject).into_owned(),
                opens: Opens::Thread(message.thread),
                reply: true,
            })
            .collect();
    }
    let mut senders: Vec<(String, usize)> = Vec::new();
    for message in announced {
        let name = sender(message);
        match senders.iter_mut().find(|(seen, _)| *seen == name) {
            Some((_, count)) => *count += 1,
            None => senders.push((name, 1)),
        }
    }
    // Stable, so equal counts keep the order they arrived in.
    senders.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let named: Vec<&str> = senders
        .iter()
        .take(NAMED)
        .map(|(name, _)| name.as_str())
        .collect();
    let others = senders.len().saturating_sub(NAMED);
    let body = match others {
        0 => format!("From {}", named.join(", ")),
        1 => format!("From {} and 1 other", named.join(", ")),
        n => format!("From {} and {n} others", named.join(", ")),
    };
    vec![Notification {
        opens: Opens::Inbox(account.clone()),
        account,
        summary: format!("{} new messages", announced.len()),
        body,
        reply: false,
    }]
}

/// The name a person would recognise: the display name where there is one, else the address.
fn sender(message: &Message) -> String {
    message.from.shown().to_owned()
}

/// Where notifications go.
///
/// A seam with two sides: the desktop's notification server, and a recorder in the tests.
/// Infallible on purpose — a notification that could not be shown is not a reason to stop
/// fetching mail, and there is nobody to tell except the terminal the watch already prints to.
pub trait Notifier: Send + Sync {
    fn show(&self, notification: &Notification);

    /// What the person has asked to hear about. Everything in the Inbox unless the notifier knows
    /// better.
    fn interest(&self) -> Interest {
        Interest::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use mail_domain::testing::account;
    use mail_domain::{Address, Body, MessageId, MessageKey, Star};

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 24, hour, 0, 0).unwrap()
    }

    fn message(n: u128, from: (&str, &str), subject: &str) -> Message {
        Message {
            id: MessageId::from_uuid(uuid::Uuid::from_u128(0x9000 + n)),
            thread: ThreadId::from_uuid(uuid::Uuid::from_u128(0x7000 + n)),
            account: account(),
            key: MessageKey::Rfc(format!("m{n}@example.test")),
            date: at(12),
            from: Address {
                name: (!from.0.is_empty()).then(|| from.0.to_owned()),
                email: from.1.to_owned(),
            },
            reply_to: Vec::new(),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: subject.to_owned(),
            in_reply_to: None,
            references: Vec::new(),
            rfc_message_id: None,
            read: ReadState::Unread,
            star: Star::Unstarred,
            flag: mail_domain::Hue::Red,
            mailbox: MailboxRole::Inbox,
            labels: Vec::new(),
            body: Body::Absent,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn only_new_unread_inbox_mail_from_someone_else_is_announced() {
        let own = Own::new(["Me@Example.test", "alias@example.test"]);
        let floor = at(9);
        let now = at(13);
        let base = message(1, ("Ada", "ada@example.test"), "lunch");
        let with = |change: &dyn Fn(&mut Message)| {
            let mut m = base.clone();
            change(&mut m);
            m
        };
        let inactive = Snooze::Inactive;
        let cases: [(&str, Message, Snooze, Verdict); 12] = [
            ("plain new mail", base.clone(), inactive, Verdict::Announce),
            (
                "spam",
                with(&|m| m.mailbox = MailboxRole::Spam),
                inactive,
                Verdict::Quiet(Quiet::Elsewhere(MailboxRole::Spam)),
            ),
            (
                "sent",
                with(&|m| m.mailbox = MailboxRole::Sent),
                inactive,
                Verdict::Quiet(Quiet::Elsewhere(MailboxRole::Sent)),
            ),
            (
                "trash",
                with(&|m| m.mailbox = MailboxRole::Trash),
                inactive,
                Verdict::Quiet(Quiet::Elsewhere(MailboxRole::Trash)),
            ),
            (
                "archived",
                with(&|m| m.mailbox = MailboxRole::Archive),
                inactive,
                Verdict::Quiet(Quiet::Elsewhere(MailboxRole::Archive)),
            ),
            (
                "read on another device",
                with(&|m| m.read = ReadState::Read),
                inactive,
                Verdict::Quiet(Quiet::Read),
            ),
            (
                "from my own address, in another case",
                with(&|m| m.from.email = "ME@example.TEST".to_owned()),
                inactive,
                Verdict::Quiet(Quiet::FromSelf),
            ),
            (
                "from another identity of mine",
                with(&|m| m.from.email = "alias@example.test".to_owned()),
                inactive,
                Verdict::Quiet(Quiet::FromSelf),
            ),
            (
                "older than the floor: the first backfill",
                with(&|m| m.date = at(8)),
                inactive,
                Verdict::Quiet(Quiet::BeforeFloor),
            ),
            (
                "dated exactly at the floor",
                with(&|m| m.date = floor),
                inactive,
                Verdict::Announce,
            ),
            (
                "its conversation is snoozed",
                base.clone(),
                Snooze::Until(at(18)),
                Verdict::Quiet(Quiet::Snoozed),
            ),
            (
                "its snooze is already due",
                base.clone(),
                Snooze::Until(at(10)),
                Verdict::Announce,
            ),
        ];
        for (name, message, snooze, expect) in cases {
            assert_eq!(
                verdict(&message, snooze, &own, &Heard::Everyone, floor, now, false),
                expect,
                "{name}"
            );
        }
    }

    #[test]
    fn vips_only_lets_a_vip_s_mail_through_and_holds_back_everyone_else_s() {
        let own = Own::new(["me@example.test"]);
        let vips = Heard::Only(Vips::new(["Dana@Example.test"]));
        let dana = message(1, ("Dana", "dana@example.test"), "plans");
        let ada = message(2, ("Ada", "ada@example.test"), "lunch");
        let read = Message {
            read: ReadState::Read,
            ..dana.clone()
        };
        // (case, message, whose mail is heard, verdict)
        let cases: [(&str, &Message, &Heard, Verdict); 5] = [
            (
                "a VIP, everyone heard",
                &dana,
                &Heard::Everyone,
                Verdict::Announce,
            ),
            (
                "someone else, everyone heard",
                &ada,
                &Heard::Everyone,
                Verdict::Announce,
            ),
            ("a VIP, VIPs only", &dana, &vips, Verdict::Announce),
            (
                "someone else, VIPs only",
                &ada,
                &vips,
                Verdict::Quiet(Quiet::NotVip),
            ),
            (
                "a VIP's mail already read",
                &read,
                &vips,
                Verdict::Quiet(Quiet::Read),
            ),
        ];
        for (case, message, heard, want) in cases {
            assert_eq!(
                verdict(message, Snooze::Inactive, &own, heard, at(9), at(13), false),
                want,
                "{case}"
            );
        }

    }

    #[test]
    fn up_to_three_arrivals_are_announced_one_by_one() {
        let a = message(1, ("Ada Lovelace", "ada@example.test"), "lunch on friday");
        let b = message(2, ("", "bob@example.test"), "  ");
        assert_eq!(
            batch(account(), &[&a, &b]),
            [
                Notification {
                    account: account(),
                    summary: "Ada Lovelace".to_owned(),
                    body: "lunch on friday".to_owned(),
                    opens: Opens::Thread(a.thread),
                    reply: true,
                },
                Notification {
                    account: account(),
                    summary: "bob@example.test".to_owned(),
                    body: "(no subject)".to_owned(),
                    opens: Opens::Thread(b.thread),
                    reply: true,
                },
            ]
        );
        let c = message(3, ("Cy", "cy@example.test"), "hi");
        assert_eq!(batch(account(), &[&a, &b, &c]).len(), 3);
        assert!(
            batch(account(), &[]).is_empty(),
            "nothing arrived, nothing said"
        );
    }

    #[test]
    fn a_burst_becomes_one_summary_naming_the_busiest_senders() {
        let from = [
            ("Newsletter", "news@example.test"),
            ("Ada", "ada@example.test"),
            ("Bob", "bob@example.test"),
            ("Ada", "ada@example.test"),
            ("Cy", "cy@example.test"),
            ("Ada", "ada@example.test"),
            ("Dee", "dee@example.test"),
        ];
        let all: Vec<Message> = from
            .iter()
            .enumerate()
            .map(|(i, f)| message(i as u128, *f, "x"))
            .collect();
        let refs: Vec<&Message> = all.iter().collect();
        assert_eq!(
            batch(account(), &refs),
            [Notification {
                account: account(),
                summary: "7 new messages".to_owned(),
                // Ada sent three; the rest one each, in the order they came.
                body: "From Ada, Newsletter, Bob and 2 others".to_owned(),
                opens: Opens::Inbox(account()),
                reply: false,
            }]
        );
        assert_eq!(
            batch(account(), &refs[..4])[0].body,
            "From Ada, Newsletter, Bob",
            "four from three people names all three"
        );
        assert_eq!(
            batch(account(), &refs[..5])[0].body,
            "From Ada, Newsletter, Bob and 1 other"
        );
    }

    #[test]
    fn mail_a_rule_asked_about_is_said_wherever_it_went() {
        let cases = [
            (Verdict::Announce, Verdict::Announce),
            (Verdict::Quiet(Quiet::Read), Verdict::Announce),
            (
                Verdict::Quiet(Quiet::Elsewhere(MailboxRole::Archive)),
                Verdict::Announce,
            ),
            (Verdict::Quiet(Quiet::Snoozed), Verdict::Announce),
            (
                Verdict::Quiet(Quiet::FromSelf),
                Verdict::Quiet(Quiet::FromSelf),
            ),
            (
                Verdict::Quiet(Quiet::BeforeFloor),
                Verdict::Quiet(Quiet::BeforeFloor),
            ),
        ];
        for (said, want) in cases {
            assert_eq!(heard(said), want, "{said:?}");
        }
    }

    #[test]
    fn an_account_is_heard_unless_its_address_is_muted_whatever_the_case() {
        let interest = Interest {
            muted: vec!["work@example.test".to_owned()],
            ..Interest::default()
        };
        assert!(!interest.hears("Work@Example.test"));
        assert!(interest.hears("home@example.test"));
        assert!(Interest::default().hears("work@example.test"));
    }

    #[test]
    fn a_banner_offers_archive_and_reply_for_a_conversation_and_only_when_it_offers_them() {
        let thread = || Opens::Thread(ThreadId::from_uuid(uuid::Uuid::from_u128(1)));
        let link = true;
        let cases = [
            (
                "the banner",
                DEFAULT_ACTION,
                thread(),
                link,
                Some(Act::Open),
            ),
            (
                "the banner of a summary",
                DEFAULT_ACTION,
                Opens::Inbox(account()),
                false,
                Some(Act::Open),
            ),
            (
                "archive",
                ARCHIVE_ACTION,
                thread(),
                link,
                Some(Act::Archive),
            ),
            (
                "archive of a summary",
                ARCHIVE_ACTION,
                Opens::Inbox(account()),
                false,
                None,
            ),
            ("reply", REPLY_ACTION, thread(), link, Some(Act::Reply)),
            ("reply not offered", REPLY_ACTION, thread(), false, None),
            (
                "reply of a summary",
                REPLY_ACTION,
                Opens::Inbox(account()),
                true,
                None,
            ),
            ("an action never offered", "delete", thread(), link, None),
        ];
        for (name, action, opens, reply, expect) in cases {
            assert_eq!(act_for(action, &opens, reply), expect, "{name}");
        }
    }
}
