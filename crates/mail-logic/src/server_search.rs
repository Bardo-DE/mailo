//! Searching the server from a search typed here: whether it runs by itself, and the typed line as
//! the filter a server is asked for.
//!
//! A search runs over the store. It is asked of the server, not automatic, unless the user turned
//! "Search the server automatically" on: a server search is a connection and a query against
//! someone else's machine, and a keystroke should not make one. Reading the switch's file and
//! asking the server are `mail_core::server_search`'s.
//!
//! What a server is asked is the line exactly as the store is asked it, parsed by the same
//! [`crate::query::parse_with`], except that `label:` resolves against the one account being
//! searched and a `re:/…/` pattern, which no server matches, is said rather than dropped.

use mail_domain::{Filter, LabelId};
use porter_core::AccountId;

/// Whether a search shown in the list is asked of the server without being asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Automatic {
    On,
    /// Only when "Search … on the server" is pressed.
    #[default]
    Off,
}

/// The search line `input` as a server is asked it, on an account whose labels are `labels`.
///
/// `Err` when a part of it no server can match: a `re:/…/` pattern, which is matched here over
/// what the store holds. An invalid pattern is that too: it matches nothing anywhere.
///
/// Parsed with the names the store parses with ([`crate::query::Known`]): `label:` against this
/// account's `labels`, and `account:` against every account's address (`accounts`), which a
/// server is told of as it is any account's clause. `is:vip` is asked as the VIPs' addresses
/// (`vips`), each the address alone: the list is the store's, which a server cannot read.
pub fn filter_of(
    input: &str,
    labels: &[(String, LabelId)],
    accounts: &[(String, AccountId)],
    vips: &crate::vip::Vips,
) -> Result<Filter, mail_proto::search::Unsaid> {
    let extracted =
        crate::search::extract(input).map_err(|why| mail_proto::search::Unsaid(vec![why.to_string()]))?;
    if let Some(pattern) = &extracted.regex {
        return Err(mail_proto::search::Unsaid(vec![format!(
            "re:/{}/ (a pattern is matched on this computer, not by a server)",
            pattern.as_str()
        )]));
    }
    let known = crate::query::Known { labels, accounts };
    Ok(with_vips(
        crate::query::parse_with(&extracted.rest, &chrono::Local, &known),
        vips,
    ))
}

/// `filter` with each `is:vip` clause in it said as the addresses of `vips`, for a server that
/// has no such list.
fn with_vips(filter: Filter, vips: &crate::vip::Vips) -> Filter {
    match filter {
        // Nobody on the list is a clause that finds nothing, which every translation has a
        // word for, where an either-or of no clauses it has not.
        Filter::Vip => match vips.addresses() {
            [] => Filter::Nothing,
            [one] => Filter::FromAddress(one.clone()),
            many => Filter::Or(many.iter().cloned().map(Filter::FromAddress).collect()),
        },
        Filter::And(parts) => Filter::And(parts.into_iter().map(|f| with_vips(f, vips)).collect()),
        Filter::Or(parts) => Filter::Or(parts.into_iter().map(|f| with_vips(f, vips)).collect()),
        Filter::Not(inner) => Filter::Not(Box::new(with_vips(*inner, vips))),
        other => other,
    }
}

/// Whether the line `input` asks for something only a server knows: a Gmail category, which a
/// message held here never says. Searched on this computer alone it lists nothing (and `-category:`
/// everything), so such a line is asked of the server without waiting to be told to, whatever the
/// automatic setting says; and where the server cannot answer either, the answer says why.
pub fn needs_server(input: &str) -> bool {
    fn names_category(filter: &Filter) -> bool {
        match filter {
            Filter::Category(_) => true,
            Filter::And(parts) | Filter::Or(parts) => parts.iter().any(names_category),
            Filter::Not(part) => names_category(part),
            _ => false,
        }
    }
    filter_of(input, &[], &[], &crate::vip::Vips::default())
        .is_ok_and(|filter| names_category(&filter))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vip::Vips;
    use mail_domain::TextMatch;

    #[test]
    fn a_category_is_the_servers_to_say_and_nothing_else_is() {
        for (input, want) in [
            ("category:promotions", true),
            ("from:ada -category:social", true),
            ("from:ada category:forums OR subject:lunch", true),
            ("from:ada", false),
            ("promotions", false),
        ] {
            assert_eq!(needs_server(input), want, "{input}");
        }
    }

    #[test]
    fn the_line_is_parsed_as_the_store_parses_it_and_a_pattern_is_said() {
        let travel = LabelId::generate();
        let labels = [("travel".to_owned(), travel)];
        let cases: Vec<(&str, Result<Filter, ()>)> = vec![
            (
                "from:ada",
                Ok(Filter::From(TextMatch::Contains("ada".to_owned()))),
            ),
            ("label:travel", Ok(Filter::HasLabel(travel))),
            (
                "label:work",
                Ok(Filter::Text(TextMatch::Contains("label:work".to_owned()))),
            ),
            ("re:/lun.h/ friday", Err(())),
        ];
        for (input, expected) in cases {
            assert_eq!(
                filter_of(input, &labels, &[], &Vips::default()).map_err(|_| ()),
                expected,
                "{input}"
            );
        }
        let said = filter_of("re:/lun.h/", &labels, &[], &Vips::default()).unwrap_err();
        assert_eq!(
            said.0,
            vec!["re:/lun.h/ (a pattern is matched on this computer, not by a server)".to_owned()]
        );
    }

    #[test]
    fn an_account_and_the_vips_are_said_as_the_store_hears_them() {
        let work = mail_domain::id::account_id_from_uuid(uuid::Uuid::from_u128(1));
        let accounts = [("work@example.test".to_owned(), work.clone())];
        let some = Vips::new(["Dana@example.test", "bob@example.test"]);
        let said =
            |input: &str, vips: &Vips| filter_of(input, &[], &accounts, vips).unwrap();
        assert_eq!(
            said("account:work", &some),
            Filter::Account(work),
            "an account is the account, not the text account:work"
        );
        assert_eq!(
            said("is:vip", &some),
            Filter::Or(vec![
                Filter::FromAddress("bob@example.test".to_owned()),
                Filter::FromAddress("dana@example.test".to_owned()),
            ]),
            "the address alone, each, as the list keeps them"
        );
        assert_eq!(
            said("-is:vip", &Vips::new(["bob@example.test"])),
            Filter::Not(Box::new(Filter::FromAddress("bob@example.test".to_owned())))
        );
        assert_eq!(
            said("is:vip", &Vips::default()),
            Filter::Nothing,
            "nobody on the list finds nothing, not an either-or of none"
        );
    }
}
