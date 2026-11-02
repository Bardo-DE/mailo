//! A junk filter of this client's own, as Mail has: it learns from what the user calls spam and
//! not spam, and judges new mail in the inbox by the words it has learned.
//!
//! Three questions, kept apart, as notifications keep theirs:
//!
//! - **What a message's words say** is [`score`]: Robinson's way of combining each word's
//!   spamminess (Fisher's method over the fifteen words that say most), a pure function of the
//!   counts. Nothing is said until the filter has learned [`ENOUGH`] of each kind.
//! - **Whether it is judged at all** is [`spared`]: never mail from someone in Contacts, someone
//!   the user has written to, or mail addressed to the user's full name, as Mail spares them.
//! - **What is done** is the [`Policy`]'s: moved to Spam, or left where it is with a Junk label.
//!   The server's own word (`X-Spam-Flag: YES`, `X-Spam-Status: Yes`) counts as spam when the
//!   policy trusts it, and mail the server filed in its own Junk is learned from, not judged.
//!
//! The counts are kept per account by the store, and reading the contacts that [`spared`] asks
//! about, learning, and acting on a verdict are `mail_core::junk`'s.

use mail_domain::{Body, JunkCounts, Message};
use std::collections::BTreeSet;

/// Whether the filter judges new mail at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filtering {
    On,
    /// A handle nobody has set the junk filter on does not judge: the settings loader says `On`
    /// when the person's settings do.
    #[default]
    Off,
}

/// What is done with mail judged junk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Treat {
    /// Into Spam, as the user's own Spam would put it.
    #[default]
    Move,
    /// Left where it is, with the [`JUNK_LABEL`] on it.
    Mark,
}

/// Whether one of the reasons to spare a sender, or to trust the server, applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Heed {
    #[default]
    Yes,
    No,
}

/// When the filter runs against the user's rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// After them: a rule that files a message has the last word over it.
    AfterRules,
    /// Before them: junk is out of the inbox before any rule sees it, so a rule that answers or
    /// forwards never speaks for mail the filter has called junk.
    #[default]
    BeforeRules,
}

/// How the filter behaves: Settings' Junk Mail section, as mail-app hands it over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Policy {
    pub filtering: Filtering,
    pub treat: Treat,
    /// Spare a sender in Contacts.
    pub contacts: Heed,
    /// Spare a sender the user has written to.
    pub written_to: Heed,
    /// Spare mail addressed to the user's full name.
    pub full_name: Heed,
    /// Take the server's `X-Spam-Flag` and `X-Spam-Status` as its verdict.
    pub server_says: Heed,
    pub order: Order,
}

/// The label a [`Treat::Mark`] puts on junk.
pub const JUNK_LABEL: &str = "Junk";

/// Messages of each kind the filter must have learned before it judges anything: until then
/// its counts are an accident of the first few messages.
pub const ENOUGH: u64 = 50;

/// How sure the filter must be before it calls a message junk.
pub const THRESHOLD: f64 = 0.9;

/// How many of a message's words decide: the ones furthest from neutral.
const TELLING: usize = 15;

/// The most words one message teaches or is judged by: a message of a hundred thousand
/// different tokens is not worth more than its first two thousand.
pub const MOST_WORDS: usize = 2000;

/// The distinct words a message is judged by: its sender's domain as one word, then its
/// subject's and its text's, folded to lower case, three to twenty letters or digits long, at
/// most [`MOST_WORDS`] of them, the earlier first.
pub fn words(message: &Message) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    // Whether there is room for no more.
    let mut add = |word: String| {
        if out.len() < MOST_WORDS && seen.insert(word.clone()) {
            out.push(word);
        }
        out.len() >= MOST_WORDS
    };
    if let Some((_, domain)) = message.from.email.rsplit_once('@') {
        add(format!("from:{}", domain.to_ascii_lowercase()));
    }
    let text = match &message.body {
        Body::Present { text, .. } => text.as_deref().unwrap_or(""),
        Body::Absent => "",
    };
    for source in [message.subject.as_str(), text] {
        for word in source.split(|c: char| !c.is_alphanumeric()) {
            let count = word.chars().count();
            if (3..=20).contains(&count)
                && !word.chars().all(|c| c.is_ascii_digit())
                && add(word.to_lowercase())
            {
                return out;
            }
        }
    }
    out
}

/// How likely a message is junk, from `counts` of the words it has: near 1 junk, near 0 not, 0.5
/// when the words say nothing. `None` until the filter has learned [`ENOUGH`] of each kind.
pub fn score(counts: &JunkCounts) -> Option<f64> {
    if counts.spam < ENOUGH || counts.ham < ENOUGH {
        return None;
    }
    let (spams, hams) = (counts.spam as f64, counts.ham as f64);
    // Robinson's f(w): the word's own spamminess, pulled towards a half by how little it has
    // been seen.
    let mut each: Vec<f64> = counts
        .words
        .iter()
        .map(|(_, spam, ham)| {
            let (b, g) = (*spam as f64 / spams, *ham as f64 / hams);
            let seen = (*spam + *ham) as f64;
            let p = if b + g == 0.0 { 0.5 } else { b / (b + g) };
            ((0.5 + seen * p) / (1.0 + seen)).clamp(0.01, 0.99)
        })
        .collect();
    if each.is_empty() {
        return Some(0.5);
    }
    each.sort_by(|a, b| (b - 0.5).abs().total_cmp(&(a - 0.5).abs()));
    each.truncate(TELLING);
    let n = each.len();
    let h = chi_squared_survival(-2.0 * each.iter().map(|f| f.ln()).sum::<f64>(), 2 * n);
    let s = chi_squared_survival(
        -2.0 * each.iter().map(|f| (1.0 - f).ln()).sum::<f64>(),
        2 * n,
    );
    Some((1.0 + h - s) / 2.0)
}

/// The probability a chi-squared variable with `freedom` (even) degrees exceeds `x`.
fn chi_squared_survival(x: f64, freedom: usize) -> f64 {
    let m = x / 2.0;
    let mut term = (-m).exp();
    let mut sum = term;
    for i in 1..freedom / 2 {
        term *= m / i as f64;
        sum += term;
    }
    sum.min(1.0)
}

/// What the user's own records say of a sender: whether they are in Contacts (by the user's
/// hand or an address book's, not just by having written), and whether the user has written to
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Acquaintance {
    pub in_contacts: bool,
    pub written_to: bool,
}

/// Whether `message` is spared judging, by whom it is from or to: `from` is what the records
/// say of its sender, `names` are the user's full names, from their identities.
pub fn spared(from: Acquaintance, message: &Message, policy: &Policy, names: &[String]) -> bool {
    if policy.contacts == Heed::Yes && from.in_contacts {
        return true;
    }
    if policy.written_to == Heed::Yes && from.written_to {
        return true;
    }
    policy.full_name == Heed::Yes
        && message.to.iter().chain(&message.cc).any(|to| {
            to.name.as_deref().is_some_and(|name| {
                names
                    .iter()
                    .any(|mine| mine.eq_ignore_ascii_case(name.trim()))
            })
        })
}

/// Whether the server that delivered `raw` said it is spam.
pub fn server_says_spam(raw: &[u8]) -> bool {
    let flag = mail_archive::header(raw, "X-Spam-Flag")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("yes"));
    let status = mail_archive::header(raw, "X-Spam-Status").is_some_and(|value| {
        value
            .trim()
            .split([',', ' ', ';'])
            .next()
            .is_some_and(|first| first.eq_ignore_ascii_case("yes"))
    });
    flag || status
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::testing::message;

    #[test]
    fn a_messages_words_are_its_subjects_its_texts_and_its_senders_domain() {
        let words = words(&message(
            1,
            "Promo@Prizes.Example",
            "FREE prize!!",
            "a free 2026 lottery",
        ));
        assert_eq!(words, ["from:prizes.example", "free", "prize", "lottery"]);
    }

    #[test]
    fn the_score_says_nothing_until_enough_is_learned_and_then_leans_the_right_way() {
        let counts = |spam, ham, words: Vec<(&str, u64, u64)>| JunkCounts {
            spam,
            ham,
            words: words
                .into_iter()
                .map(|(w, s, h)| (w.to_owned(), s, h))
                .collect(),
        };
        assert_eq!(
            score(&counts(ENOUGH - 1, ENOUGH, vec![("prize", ENOUGH - 1, 0)])),
            None,
            "too little spam"
        );
        assert_eq!(
            score(&counts(ENOUGH, ENOUGH, vec![])),
            Some(0.5),
            "no word it knows"
        );
        let spammy = score(&counts(
            ENOUGH,
            ENOUGH,
            vec![("prize", ENOUGH - 1, 0), ("winner", ENOUGH - 2, 1)],
        ))
        .unwrap();
        let hammy = score(&counts(
            ENOUGH,
            ENOUGH,
            vec![("budget", 0, ENOUGH - 1), ("meeting", 1, ENOUGH - 2)],
        ))
        .unwrap();
        assert!(spammy > THRESHOLD, "{spammy}");
        assert!(hammy < 0.1, "{hammy}");
    }

    #[test]
    fn the_servers_verdict_is_read_from_its_headers() {
        let cases: &[(&[u8], bool)] = &[
            (b"X-Spam-Flag: YES\r\nSubject: hi\r\n\r\nbody", true),
            (b"X-Spam-Status: Yes, score=9.1\r\n\r\n", true),
            (b"X-Spam-Status: No, score=0.1\r\n\r\n", false),
            (b"Subject: hi\r\n\r\nX-Spam-Flag: YES", false),
            (b"", false),
        ];
        for (raw, want) in cases {
            assert_eq!(
                server_says_spam(raw),
                *want,
                "{}",
                String::from_utf8_lossy(raw)
            );
        }
    }

    #[test]
    fn a_message_of_a_hundred_thousand_words_is_judged_by_its_first_two_thousand() {
        let text: String = (0..100_000).map(|n| format!("w{n:06} ")).collect();
        let big = message(1, "promo@prizes.example", "Subject words", &text);
        let words = words(&big);
        assert_eq!(words.len(), MOST_WORDS);
        assert_eq!(words[0], "from:prizes.example");
        assert_eq!(&words[1..3], ["subject", "words"]);
        assert_eq!(words[3], "w000000");
    }
}
