//! A send made from somewhere other than the composer's page (an unsubscribe message), put on the
//! outbox pill like any other: [`ShowQueued`], which the composer's desk answers, and the parts
//! that queue a send ask through without naming the desk.

use dioxus::prelude::*;
use mail_domain::DraftId;

/// Show `draft`, queued to leave at once, on the pill: a context the composer's desk provides.
/// Copy, so a task can take it.
#[derive(Clone, Copy)]
pub struct ShowQueued(pub Callback<DraftId>);

impl std::fmt::Debug for ShowQueued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ShowQueued")
    }
}

impl ShowQueued {
    /// Show `draft` on the pill.
    pub fn show(self, draft: DraftId) {
        self.0.call(draft);
    }
}
