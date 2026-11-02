//! A send made from somewhere other than the composer's page (an unsubscribe message), put on the
//! outbox pill like any other: [`ShowQueued`], which the composer's desk answers, and the parts
//! that queue a send ask through without naming the desk.

use crate::view::Shell;
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

/// What a stored draft's row menu asks of the composer: delete it, or take back its scheduled
/// send. A context the composer's desk provides, which the list asks through and does not name
/// the composer. Copy, so a handler can take it.
#[derive(Clone, Copy)]
pub struct DraftActs {
    /// Delete the stored draft, and offer it back.
    pub delete: Callback<(Signal<Shell>, DraftId)>,
    /// Cancel the draft's scheduled send, said in the toast if refused; whether it was.
    pub cancel_sending: Callback<(Signal<Shell>, DraftId), bool>,
}

impl std::fmt::Debug for DraftActs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DraftActs")
    }
}

impl DraftActs {
    /// Delete the stored draft `draft`. Call it from an event handler.
    pub fn delete(self, shell: Signal<Shell>, draft: DraftId) {
        self.delete.call((shell, draft));
    }

    /// Cancel `draft`'s scheduled send; whether it was cancelled.
    pub fn cancel_sending(self, shell: Signal<Shell>, draft: DraftId) -> bool {
        self.cancel_sending.call((shell, draft))
    }
}
