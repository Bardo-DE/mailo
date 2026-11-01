//! What is kept, read where the caller is: the store's own answers, for a caller that has no store.
//!
//! A small read the caller needs at once (the draft it is about to open, the key it is about to
//! show) and the few plain writes it makes by hand. What is drawn and must keep up with the store is
//! a query ([`crate::Query`]) instead, read off the caller's thread and read again as it moves.

use crate::{CallError, Client};
use chrono::{DateTime, Utc};
use mail_domain::{
    CertFingerprint, Draft, DraftId, Filter, Fingerprint, Folder, InviteAnswer, Label, LabelId,
    Message, MessageId, Page, Patch, PgpKey, Placed, Query, ReceiptAnswer, Rule, SmimeCert, Thread,
    ThreadId, ThreadSummary, Vacation, View, ViewId,
};
use mail_store::{
    Blobs, Contact, Group, GroupId, Offline, Origin, OutboxEntry, SenderRecord, Store,
    StoredAccount,
};
use porter_core::AccountId;

/// The store's answers, through a client ([`Client::stored`]).
///
/// Every call here may block: it runs on the caller's thread and waits for a reader or the
/// writer, behind whatever an ingest or a long query holds. So none is made from a frame being
/// drawn, nor from a handler on the thread that draws; what a frame shows is a query
/// ([`crate::Query`]) and what a press changes is an act ([`crate::Act`]).
#[derive(Clone, Copy, Debug)]
pub struct Stored<'a>(&'a Client);

impl Client {
    /// What is kept, read on the caller's thread. May block ([`Stored`]).
    pub fn stored(&self) -> Stored<'_> {
        Stored(self)
    }

    /// The bodies and attachments kept, by their blob.
    pub fn blobs(&self) -> Blobs<'_> {
        self.store().blobs()
    }

    /// What a search reads ([`mail_core::search::run`]).
    pub fn source(&self) -> &dyn mail_core::search::Source {
        self.store()
    }
}

impl Stored<'_> {
    /// The blob holding attachment `index` of `message` decoded, made from the message's own
    /// bytes when it is not kept apart (`mail_core::attach::blob_of`). May decode a whole
    /// message, so it is called where a read is, not where a frame is drawn.
    pub fn attachment_blob(
        &self,
        message: &Message,
        index: usize,
    ) -> Result<mail_domain::BlobId, CallError> {
        mail_core::attach::blob_of(self.0.store(), message, index).map_err(CallError::from)
    }

    pub fn identities(&self, account: AccountId) -> Result<Vec<mail_domain::Identity>, CallError> {
        self.0.store().identities(account).map_err(CallError::from)
    }

    pub fn identity(
        &self,
        id: mail_domain::IdentityId,
    ) -> Result<Option<mail_domain::Identity>, CallError> {
        self.0.store().identity(id).map_err(CallError::from)
    }

    pub fn account_prefs(&self, account: AccountId) -> Result<mail_store::AccountPrefs, CallError> {
        self.0
            .store()
            .account_prefs(account)
            .map_err(CallError::from)
    }

    pub fn signature(
        &self,
        id: mail_domain::SignatureId,
    ) -> Result<mail_domain::Signature, CallError> {
        self.0.store().signature(id).map_err(CallError::from)
    }

    pub fn followed_folders(&self) -> Result<Vec<mail_domain::MailboxRef>, CallError> {
        self.0.store().followed_folders().map_err(CallError::from)
    }

    pub fn card(
        &self,
        id: &mail_store::CardId,
    ) -> Result<Option<mail_store::PersonCard>, CallError> {
        self.0.store().card(id).map_err(CallError::from)
    }

    pub fn card_of(&self, address: &str) -> Result<Option<mail_store::PersonCard>, CallError> {
        self.0.store().card_of(address).map_err(CallError::from)
    }

    pub fn cards(&self) -> Result<Vec<mail_store::PersonCard>, CallError> {
        self.0.store().cards().map_err(CallError::from)
    }

    pub fn account(&self, id: AccountId) -> Result<Option<StoredAccount>, CallError> {
        self.0.store().account(id).map_err(CallError::from)
    }

    pub fn list_accounts(&self) -> Result<Vec<StoredAccount>, CallError> {
        self.0.store().list_accounts().map_err(CallError::from)
    }

    pub fn sender_history(&self) -> Result<Vec<SenderRecord>, CallError> {
        self.0.store().sender_history().map_err(CallError::from)
    }

    /// What the account's server was last seen to support ([`mail_core::act::caps_here`]).
    pub fn caps(&self, account: AccountId, now: DateTime<Utc>) -> mail_domain::AccountCaps {
        mail_core::act::caps_here(self.0.store(), account, now)
    }

    /// How fresh the account is judged to be ([`mail_core::sync::fresh::Pace::of`]).
    pub fn pace(&self, account: AccountId) -> mail_core::sync::fresh::Pace {
        mail_core::sync::fresh::Pace::of(self.0.store(), account)
    }

    pub fn folders(&self, account: AccountId) -> Result<Vec<Folder>, CallError> {
        self.0.store().folders(account).map_err(CallError::from)
    }

    /// Write the caller's own changes, atomically.
    pub fn apply(&self, account: AccountId, patch: &Patch) -> Result<(), CallError> {
        self.0
            .store()
            .apply(account, patch)
            .map_err(CallError::from)
    }

    pub fn put_vacation(
        &self,
        account: AccountId,
        vacation: Option<&Vacation>,
        now: DateTime<Utc>,
    ) -> Result<(), CallError> {
        self.0
            .store()
            .put_vacation(account, vacation, now)
            .map_err(CallError::from)
    }

    pub fn put_view(&self, view: &View) -> Result<(), CallError> {
        self.0.store().put_view(view).map_err(CallError::from)
    }

    pub fn thread(&self, id: ThreadId) -> Result<Thread, CallError> {
        self.0.store().thread(id).map_err(CallError::from)
    }

    pub fn threads(
        &self,
        query: &Query,
        now: DateTime<Utc>,
    ) -> Result<Page<ThreadSummary>, CallError> {
        self.0.store().threads(query, now).map_err(CallError::from)
    }

    pub fn count(&self, filter: &Filter, now: DateTime<Utc>) -> Result<u64, CallError> {
        self.0.store().count(filter, now).map_err(CallError::from)
    }

    pub fn message(&self, id: MessageId) -> Result<Message, CallError> {
        self.0.store().message(id).map_err(CallError::from)
    }

    pub fn placed(&self, message: MessageId) -> Result<Vec<Placed>, CallError> {
        self.0.store().placed(message).map_err(CallError::from)
    }

    pub fn draft(&self, id: DraftId) -> Result<Draft, CallError> {
        self.0.store().draft(id).map_err(CallError::from)
    }

    pub fn drafts(&self, account: AccountId) -> Result<Vec<Draft>, CallError> {
        self.0.store().drafts(account).map_err(CallError::from)
    }

    pub fn outbox_due(
        &self,
        account: AccountId,
        now: DateTime<Utc>,
    ) -> Result<Vec<OutboxEntry>, CallError> {
        self.0
            .store()
            .outbox_due(account, now)
            .map_err(CallError::from)
    }

    pub fn offline(&self, account: AccountId) -> Result<Offline, CallError> {
        self.0.store().offline(account).map_err(CallError::from)
    }

    pub fn set_account_description(
        &self,
        account: AccountId,
        description: Option<&str>,
    ) -> Result<(), CallError> {
        self.0
            .store()
            .set_account_description(account, description)
            .map_err(CallError::from)
    }

    pub fn set_account_syncing(
        &self,
        account: AccountId,
        syncing: mail_store::Syncing,
    ) -> Result<(), CallError> {
        self.0
            .store()
            .set_account_syncing(account, syncing)
            .map_err(CallError::from)
    }

    /// Keep `account`'s labels on its server as keywords, or here alone.
    pub fn set_account_keeps_labels(
        &self,
        account: AccountId,
        keeps: bool,
    ) -> Result<(), CallError> {
        self.0
            .store()
            .set_account_keeps_labels(account, keeps, chrono::Utc::now())
            .map_err(CallError::from)
    }

    pub fn set_folder_role(
        &self,
        account: AccountId,
        role: mail_domain::MailboxRole,
        path: Option<&str>,
    ) -> Result<(), CallError> {
        self.0
            .store()
            .set_folder_role(account, role, path)
            .map_err(CallError::from)
    }

    pub fn vacation(&self, account: AccountId) -> Result<Option<Vacation>, CallError> {
        self.0.store().vacation(account).map_err(CallError::from)
    }

    pub fn rules(&self, account: AccountId) -> Result<Vec<Rule>, CallError> {
        self.0.store().rules(account).map_err(CallError::from)
    }

    pub fn labels(&self, account: AccountId) -> Result<Vec<Label>, CallError> {
        self.0.store().labels(account).map_err(CallError::from)
    }

    pub fn create_label(&self, account: AccountId, name: &str) -> Result<LabelId, CallError> {
        self.0
            .store()
            .create_label(account, name)
            .map_err(CallError::from)
    }

    pub fn delete_view(&self, id: ViewId) -> Result<(), CallError> {
        self.0.store().delete_view(id).map_err(CallError::from)
    }

    pub fn contacts(&self) -> Result<Vec<Contact>, CallError> {
        self.0.store().contacts().map_err(CallError::from)
    }

    pub fn contact(&self, address: &str) -> Result<Option<Contact>, CallError> {
        self.0.store().contact(address).map_err(CallError::from)
    }

    pub fn contacts_matching(&self, typed: &str, k: usize) -> Result<Vec<Contact>, CallError> {
        self.0
            .store()
            .contacts_matching(typed, k)
            .map_err(CallError::from)
    }

    pub fn put_contact(
        &self,
        address: &str,
        name: Option<&str>,
        origin: &Origin,
    ) -> Result<Contact, CallError> {
        self.0
            .store()
            .put_contact(address, name, origin)
            .map_err(CallError::from)
    }

    pub fn groups(&self) -> Result<Vec<Group>, CallError> {
        self.0.store().groups().map_err(CallError::from)
    }

    pub fn group(&self, id: &GroupId) -> Result<Option<Group>, CallError> {
        self.0.store().group(id).map_err(CallError::from)
    }

    pub fn put_group(&self, group: &Group) -> Result<(), CallError> {
        self.0.store().put_group(group).map_err(CallError::from)
    }

    pub fn pgp_keys(&self) -> Result<Vec<PgpKey>, CallError> {
        self.0.store().pgp_keys().map_err(CallError::from)
    }

    pub fn pgp_key(&self, fingerprint: Fingerprint) -> Result<Option<PgpKey>, CallError> {
        self.0.store().pgp_key(fingerprint).map_err(CallError::from)
    }

    pub fn smime_certs(&self) -> Result<Vec<SmimeCert>, CallError> {
        self.0.store().smime_certs().map_err(CallError::from)
    }

    pub fn smime_cert(&self, fingerprint: CertFingerprint) -> Result<Option<SmimeCert>, CallError> {
        self.0
            .store()
            .smime_cert(fingerprint)
            .map_err(CallError::from)
    }

    pub fn receipt_answer(&self, message: MessageId) -> Result<Option<ReceiptAnswer>, CallError> {
        self.0
            .store()
            .receipt_answer(message)
            .map_err(CallError::from)
    }

    pub fn invite_answer(&self, message: MessageId) -> Result<Option<InviteAnswer>, CallError> {
        self.0
            .store()
            .invite_answer(message)
            .map_err(CallError::from)
    }
}
