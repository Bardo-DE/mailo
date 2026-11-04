//! Who counts as a VIP and what a menu row for them does: the list as a value, and the two sides of
//! the mark. Keeping the mark is the store's, and `mail_core::vip` does it.

use mail_domain::Filter;

/// The VIPs' addresses, lower-cased, sorted and once each: what a filter, a mark and a
/// notification ask of the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vips(Vec<String>);

impl Vips {
    pub fn new<S: AsRef<str>>(addresses: impl IntoIterator<Item = S>) -> Self {
        let mut all: Vec<String> = addresses
            .into_iter()
            .map(|a| a.as_ref().trim().to_lowercase())
            .filter(|a| !a.is_empty())
            .collect();
        all.sort();
        all.dedup();
        Vips(all)
    }

    /// Whether `email` is a VIP's, whatever its case.
    pub fn includes(&self, email: &str) -> bool {
        self.0.binary_search(&email.trim().to_lowercase()).is_ok()
    }

    /// The addresses, sorted.
    pub fn addresses(&self) -> &[String] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// What the VIPs place lists: [`Filter::Vip`] while there are any, and nothing at all while
    /// there are none. The place is built again when the list fills or empties, which is what
    /// lets the window move off it when the last VIP goes.
    pub fn filter(&self) -> Filter {
        if self.0.is_empty() {
            Filter::Nothing
        } else {
            Filter::Vip
        }
    }

    /// Whether any of `people` is a VIP: how a row marks a conversation, by everyone who wrote
    /// in it and not by the newest sender alone.
    pub fn any<'a>(&self, people: impl IntoIterator<Item = &'a mail_domain::Address>) -> bool {
        people
            .into_iter()
            .any(|person| self.includes(&person.email))
    }
}

/// Whether to mark someone a VIP or take the mark off: the two sides of one menu row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Vip,
    NotVip,
}

/// What a menu row for `address` would do, given `vips`: mark them, or take the mark off.
pub fn toggled(vips: &Vips, address: &str) -> Mark {
    if vips.includes(address) {
        Mark::NotVip
    } else {
        Mark::Vip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_folded_sorted_and_once_each() {
        let vips = Vips::new([
            " Dana@Example.test",
            "ada@example.test",
            "dana@example.test",
            " ",
        ]);
        assert_eq!(vips.addresses(), ["ada@example.test", "dana@example.test"]);
        assert!(vips.includes("DANA@example.TEST "));
        assert!(!vips.includes("dan@example.test"));
        assert!(!Vips::default().includes(""));
    }

    #[test]
    fn the_place_lists_the_vips_clause_while_there_are_any() {
        let cases: [(&str, Vips, Filter); 3] = [
            ("nobody", Vips::default(), Filter::Nothing),
            ("one", Vips::new(["ada@x.test"]), Filter::Vip),
            ("two", Vips::new(["dana@x.test", "ada@x.test"]), Filter::Vip),
        ];
        for (case, vips, want) in cases {
            assert_eq!(vips.filter(), want, "{case}");
        }
    }

    #[test]
    fn a_conversation_is_marked_when_any_of_its_senders_is_a_vip() {
        let person = |email: &str| mail_domain::Address {
            name: None,
            email: email.to_owned(),
        };
        let vips = Vips::new(["dana@x.test"]);
        let thread = [person("Dana@X.test"), person("me@x.test")];
        assert!(
            vips.any(&thread),
            "the VIP wrote first and someone else last"
        );
        assert!(!vips.any(&thread[1..]));
        assert!(!Vips::default().any(&thread));
    }
}
