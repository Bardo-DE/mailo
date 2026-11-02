//! The store behind the search pipeline's [`Source`] seam.
//!
//! The pipeline itself is `mail_logic::search`, which asks an index through the trait and knows
//! no store. This is the index that answers with the real one.

use crate::error::Logged;
use chrono::{DateTime, Utc};
use mail_domain::{Filter, PageReq, Property, Query, Sort, SortDir, Term, ThreadId, ThreadSummary};
use mail_logic::search::Source;
use mail_store::Store;

/// A store, as the thing a search asks for terms, a page of matches and top hits.
///
/// A store error is an empty answer and a line in the log: a search box that fails because the
/// store missed a row is worse than one that shows nothing.
pub struct StoreSource<'a, S: ?Sized>(pub &'a S);

impl<S: ?Sized> std::fmt::Debug for StoreSource<'_, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreSource").finish_non_exhaustive()
    }
}

impl<S: Store + ?Sized> Source for StoreSource<'_, S> {
    fn terms_with_prefix(&self, prefix: &str, limit: usize) -> Vec<Term> {
        // Every account's vocabulary, on purpose: a term that lives on another account still
        // has to pass the account filter on the results. A store error is an empty suggestion
        // list, and the word falls back to matching as typed.
        Store::terms_with_prefix(self.0, prefix, limit)
            .or_log_default("search: the store could not suggest terms")
    }

    fn listed(&self, filter: &Filter, page: PageReq, now: DateTime<Utc>) -> Vec<ThreadSummary> {
        let query = Query {
            filter: filter.clone(),
            sort: Sort {
                property: Property::Date,
                dir: SortDir::Desc,
            },
            page,
        };
        // A search box that panics because the store missed a row is worse than an empty
        // list. The error goes to the log; here there is nothing to show.
        Store::threads(self.0, &query, now)
            .map(|page| page.items)
            .or_log_default("search: the store could not list the matches")
    }

    fn top(
        &self,
        filter: &Filter,
        k: usize,
        window: &[ThreadId],
        now: DateTime<Utc>,
    ) -> Vec<(ThreadSummary, f64)> {
        Store::top_hits(self.0, filter, k, window, now)
            .or_log_default("search: the store could not rank the matches")
    }
}
