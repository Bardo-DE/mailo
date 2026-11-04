//! Where an account's away reply is kept, and the values that go with it: a typed date as an
//! instant, the reply as the account would send it and whether switching it off is said to the
//! server. Talking to the server is `mail_core::rules::server`.

use crate::accounts::Configured;
use crate::snooze::TimeError;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use mail_domain::{AccountPlan, DateRange, Incoming, IsDefault, Vacation};
use mail_proto::sieve::{NoSieve, endpoint};

/// A date or a date and time, in `zone`, as an instant.
pub fn instant<Tz: TimeZone>(text: &str, zone: &Tz) -> Result<DateTime<Utc>, TimeError> {
    let local = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M"))
        .or_else(|_| {
            NaiveDate::parse_from_str(text, "%Y-%m-%d").map(|d| d.and_time(Default::default()))
        })
        .map_err(|_| TimeError::NotADate {
            text: text.to_owned(),
        })?;
    zone.from_local_datetime(&local)
        .earliest()
        .map(|t| t.with_timezone(&Utc))
        .ok_or_else(|| TimeError::NoSuchInstant {
            text: text.to_owned(),
        })
}

/// The reply as the account would send it: to mail addressed to any of its addresses, from its
/// usual one.
pub fn vacation_for(
    account: &Configured,
    subject: &str,
    body: &str,
    days: u16,
    during: DateRange,
) -> Vacation {
    let mut addresses = vec![account.address.to_lowercase()];
    for identity in &account.plan.identities {
        let address = identity.from.email.to_lowercase();
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    let from = account
        .plan
        .identities
        .iter()
        .find(|i| i.default == IsDefault::Default)
        .map(|i| i.from.email.clone());
    Vacation {
        account: account.id.clone(),
        subject: subject.to_owned(),
        body: body.to_owned(),
        days,
        addresses,
        from,
        during,
    }
}

/// Where an account's vacation reply is kept on its server, which sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwayServer {
    /// In this client's Sieve script, beside the rules.
    Sieve,
    /// The account's `VacationResponse`, where JMAP keeps it.
    Jmap,
    /// Exchange's automatic replies, through Microsoft Graph's mailbox settings.
    Graph,
}

/// Where `plan`'s vacation reply goes, or why it has nowhere to go.
pub fn away_server(plan: &AccountPlan) -> Result<AwayServer, NoSieve> {
    match plan.incoming {
        Incoming::Jmap { .. } => Ok(AwayServer::Jmap),
        Incoming::Graph => Ok(AwayServer::Graph),
        _ => endpoint(plan).map(|_| AwayServer::Sieve),
    }
}

/// Whether switching the reply off is said to the server: only when a reply was kept here to
/// switch off, and the server keeps one. One rule for the command and for the page.
pub fn off_reaches_server(kept_here: bool, server_keeps_one: bool) -> bool {
    kept_here && server_keeps_one
}
