//! What the pipeline asks of an index, behind a trait this crate owns.
//!
//! The real answers are the store's `terms_with_prefix`, `threads` and `top_hits`, which
//! `mail-core` puts behind this trait (`StoreSource`). [`Source`] is the seam so a test can
//! assert against terms and scores it chose, which a real index would not give it on demand.

use chrono::{DateTime, Utc};
use mail_domain::{Filter, PageReq, ThreadId, ThreadSummary};

use mail_domain::Term;

/// Where prefix expansion, the date-ordered list and the top hits come from.
///
/// Two implementations are swapped here: the real store (through `mail-core`'s `StoreSource`), and
/// a fake the tests build with the terms and scores they want to assert about. The pipeline takes `&dyn Source` for that reason.
pub trait Source {
    /// Vocabulary entries whose text starts with `prefix`, most frequent first is the caller's
    /// job. `prefix` is already folded with `search_tokens`.
    fn terms_with_prefix(&self, prefix: &str, limit: usize) -> Vec<Term>;

    /// One page of the threads matching `filter`, newest first: what a place lists, and what a
    /// search lists under its top results. Cheap, because it is paginated and never scores.
    fn listed(&self, filter: &Filter, page: PageReq, now: DateTime<Utc>) -> Vec<ThreadSummary>;

    /// The best `k` of `window` by full-text relevance to `filter`, best first, each with a
    /// bm25 score where higher is better.
    ///
    /// `window` is threads [`Source::listed`] already returned for `filter`: ranking what is in
    /// hand, rather than listing again, is what keeps a strip from costing a second search.
    /// Empty when the filter has no text term: there is nothing to rank by. The cost is
    /// bounded by `window`, not by how many messages a common word hits.
    fn top(
        &self,
        filter: &Filter,
        k: usize,
        window: &[ThreadId],
        now: DateTime<Utc>,
    ) -> Vec<(ThreadSummary, f64)>;
}

/// The first `limit` rows, newest first.
pub fn first(limit: usize) -> PageReq {
    PageReq {
        after: None,
        limit: u32::try_from(limit).unwrap_or(u32::MAX),
    }
}
