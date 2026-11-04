//! What the server that received a message says it checked about the sender: SPF, DKIM and
//! DMARC, and what the checks come to. Which field is believed, and reading it from the stored
//! message, are `mail_core::auth`'s.

use mail_mime::{AuthResults, Check, Verdict};

/// What the checks come to, as one word the reader can show.
///
/// DMARC's rule (RFC 7489 §4.2), which is the receiving server's own when it reports DMARC: a
/// message is the sender's when a check that passed was about the sender's domain. Either check
/// is enough. SPF judges the server that last handed the mail on, so it fails whenever the mail
/// was forwarded, fetched into another mailbox or sent through a list; DKIM signs the message
/// itself and survives those, and a list that rewrites the body breaks it. A failure of one is
/// therefore never a verdict while the other passed, and a failure on its own is weak: only the
/// domain's own word that a server is not its (DMARC's fail, or SPF's hard fail for the sender's
/// domain) is shown as a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// DMARC passed; or, with no DMARC verdict, DKIM or SPF passed for the sender's domain.
    Passed,
    /// DMARC failed; or, with no DMARC verdict and no pass for the sender's domain, that domain's
    /// SPF record says the sending server is not its.
    Failed,
    /// Neither: nothing was checked, what passed was about another domain (a mailing service
    /// signing as itself), or what failed is what forwarding breaks (SPF's softfail, a DKIM
    /// signature the trip changed).
    Unsure,
}

impl Standing {
    /// The `data-standing` word.
    pub fn word(self) -> &'static str {
        match self {
            Standing::Passed => "pass",
            Standing::Failed => "fail",
            Standing::Unsure => "unsure",
        }
    }
}

/// What `results` come to.
pub fn standing(results: &AuthResults) -> Standing {
    match results.dmarc.as_ref().map(|check| check.verdict) {
        Some(Verdict::Pass) => return Standing::Passed,
        Some(Verdict::Fail) => return Standing::Failed,
        _ => {}
    }
    let Some(from) = results.from.as_deref() else {
        return Standing::Unsure;
    };
    let ours = |check: &Check| check.domain.as_deref().is_some_and(|d| aligned(d, from));
    let passed = |check: &Check| check.verdict == Verdict::Pass && ours(check);
    if results.signatures.iter().any(passed) || results.spf.as_ref().is_some_and(passed) {
        return Standing::Passed;
    }
    match &results.spf {
        Some(spf) if spf.verdict == Verdict::Fail && ours(spf) => Standing::Failed,
        _ => Standing::Unsure,
    }
}

/// Whether a check about `domain` speaks for mail from `from`: DMARC's relaxed alignment
/// (RFC 7489 §3.1), the two under the same registered domain. `news.example.com` signs for
/// `example.com`; `example.com.evil.test` does not.
fn aligned(domain: &str, from: &str) -> bool {
    let registered = |d: &str| psl::domain_str(d).map(str::to_owned);
    match (registered(domain), registered(from)) {
        (Some(a), Some(b)) => a == b,
        // A name with no registered domain (a bare suffix, a single label) aligns only with
        // itself.
        _ => domain == from,
    }
}

/// The checks in words: "SPF pass · DKIM pass (example.com) · DMARC pass". Only the methods
/// the field mentions.
pub fn words(results: &AuthResults) -> String {
    [
        ("SPF", &results.spf),
        ("DKIM", &results.dkim),
        ("DMARC", &results.dmarc),
    ]
    .into_iter()
    .filter_map(|(name, check)| {
        let check = check.as_ref()?;
        let said = verdict_word(check.verdict);
        Some(match (&check.domain, check.verdict) {
            // Whose signature passed is the part that matters: a DKIM pass from a domain that
            // is not the sender's proves only that someone signed it.
            (Some(domain), Verdict::Pass) if name == "DKIM" => format!("{name} {said} ({domain})"),
            _ => format!("{name} {said}"),
        })
    })
    .collect::<Vec<_>>()
    .join(" · ")
}

fn verdict_word(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Pass => "pass",
        Verdict::Fail => "fail",
        Verdict::SoftFail => "softfail",
        Verdict::Neutral => "neutral",
        Verdict::None => "none",
        Verdict::TempError => "temperror",
        Verdict::PermError => "permerror",
        Verdict::Policy => "policy",
        Verdict::Unknown => "unknown",
    }
}

/// The one line the reader and the sender card show: what it comes to, and who said so.
pub fn sentence(results: &AuthResults) -> String {
    let checks = words(results);
    let checks = if checks.is_empty() {
        "nothing checked".to_owned()
    } else {
        checks
    };
    match &results.authserv_id {
        Some(by) => format!("{checks} — checked by {by}"),
        None => checks,
    }
}
