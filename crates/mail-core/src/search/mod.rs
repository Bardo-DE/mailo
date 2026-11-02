//! Search over the store: the real index behind `mail_logic::search`'s [`Source`] seam.
//!
//! What a typed line means, how it completes, ranks and marks is `mail-logic`, which asks an
//! index through a trait. [`StoreSource`] is the index that answers with the store.

mod source;

#[cfg(test)]
mod group_tests;
#[cfg(test)]
mod live;
#[cfg(test)]
mod pipeline_tests;
#[cfg(test)]
mod superset;

pub use source::StoreSource;
