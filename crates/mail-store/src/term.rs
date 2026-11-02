//! The order the index's vocabulary is reported in.

use mail_domain::Term;

/// Most frequent first. Equal counts break by the term text, so two stores agree on the order.
pub(crate) fn by_frequency(terms: &mut [Term]) {
    terms.sort_by(|a, b| b.docs.cmp(&a.docs).then_with(|| a.text.cmp(&b.text)));
}
