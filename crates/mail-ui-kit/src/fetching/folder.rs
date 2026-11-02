//! Fetching a folder because it was opened: the client's to do (`mail_client::fetch`), and to
//! keep where each folder stands.

use super::Fetching;
use dioxus::prelude::*;
use mail_domain::MailboxRef;

impl Fetching {
    /// A folder's place was chosen: fetch it, unless that was done a moment ago or a pass is
    /// running on its account. What it stores is read again as the client hears it committed.
    pub fn open_folder(&self, mailbox: MailboxRef) {
        self.client.peek().open_folder(mailbox);
    }

    /// Sync was pressed: the next opening of any folder fetches it again.
    pub fn forget_folders(&self) {
        self.client.peek().forget_folders();
    }

    /// Where a folder's on-demand fetch stands. A folder never opened is `Unfetched`.
    #[cfg(any(test, feature = "testing"))]
    pub fn folder(
        &self,
        account: porter_core::AccountId,
        path: &str,
    ) -> mail_client::fetch::FolderFetch {
        self.folders
            .read()
            .get(&(account, path.to_owned()))
            .cloned()
            .unwrap_or_default()
    }
}

/// A folder's place was chosen: [`Fetching::open_folder`], for the window that has one.
pub fn opened(mailbox: MailboxRef) {
    if let Some(fetching) = try_consume_context::<Fetching>() {
        fetching.open_folder(mailbox);
    }
}
