//! What a message is read into, kept so it is read once: its frame ([`Frames`]) and what else the
//! reader shows of it ([`Looks`]: the sender's checks, an invitation, a receipt asked for, a way
//! out of a list, what OpenPGP or S/MIME opened). The caches are the caller's, one for the app;
//! what fills them is read from the client's store.

use crate::Client;
use chrono::{DateTime, Utc};
use mail_core::SigningStore;
use mail_core::message as core;
use mail_domain::{BlobId, Message, MessageId, ThreadId};
use mail_logic::frames::{Key, Sent, Unsealed};
use mail_logic::looks::{Bodies, Invited, Read, Receipt};
use mail_logic::password::Password;
use mail_logic::protection::Opening;
use mail_logic::unsubscribe::Offer;
use mail_mime::AuthResults;
use mail_mime::Parsed;
use mail_reader::Prefer;
use mail_reader::{FrameBody, SanitizePolicy};
use std::sync::Arc;

/// What the reader has found out about messages, kept so that opening one again costs a lookup.
/// One for the whole app, which it hands to whoever reads: two of them share nothing. It is filled
/// by a call on the client ([`Client::looking`]).
#[derive(Debug, Default)]
pub struct Looks(core::Looks);

impl Looks {
    /// A fresh set of lookups, with nothing found yet.
    pub fn new() -> Self {
        Looks(core::Looks::new())
    }

    /// How often `read` really read, so a test can say that a path did not.
    pub fn reads(&self, read: Read) -> usize {
        self.0.reads(read)
    }

    /// The checks the receiving server made of `message`'s sender, when they have been read.
    pub fn checks_cached(&self, message: MessageId, raw: BlobId) -> Option<Option<AuthResults>> {
        self.0.checks_cached(message, raw)
    }

    /// The invitation `message` carries, when it has been looked for under `body`.
    pub fn invite_cached(
        &self,
        message: MessageId,
        body: Option<BlobId>,
    ) -> Option<Option<Invited>> {
        self.0.invite_cached(message, body)
    }

    /// The one-time code in `message`, when it has been looked for under `body`.
    pub fn code_cached(&self, message: MessageId, body: Option<BlobId>) -> Option<Option<String>> {
        self.0.code_cached(message, body)
    }

    /// The read receipt standings of the messages in `key`, when they have been read.
    pub fn receipts_cached(&self, key: &[(MessageId, Option<BlobId>)]) -> Option<Vec<Receipt>> {
        self.0.receipts_cached(key)
    }

    /// The thread's way out of its list, when it has been looked for under `key`.
    pub fn offer_cached(&self, thread: ThreadId, key: &Bodies) -> Option<Option<Offer>> {
        self.0.offer_cached(thread, key)
    }

    /// The message in the thread that names its list's address, when it has been looked for.
    pub fn post_cached(&self, thread: ThreadId, key: &Bodies) -> Option<Option<MessageId>> {
        self.0.post_cached(thread, key)
    }

    /// What was found for `message` holding `body`, if it was opened under the keys held now.
    pub fn sealed_cached(&self, message: MessageId, body: Option<BlobId>) -> Option<Opening> {
        self.0.sealed_cached(message, body)
    }

    /// What was last found for `message` holding `body`, under whichever keys.
    pub fn sealed_last(&self, message: MessageId, body: Option<BlobId>) -> Option<Opening> {
        self.0.sealed_last(message, body)
    }

    /// The body `message` was opened to, when it has been opened.
    pub fn opened_body(&self, message: &Message) -> Option<Box<Parsed>> {
        self.0.opened_body(message)
    }

    /// Whether the message as opened may load remote content.
    pub fn opened_may_load_remote(&self, message: &Message) -> bool {
        self.0.opened_may_load_remote(message)
    }

    /// The subject of the message as opened, when it has one of its own.
    pub fn opened_subject(&self, message: &Message) -> Option<String> {
        self.0.opened_subject(message)
    }
}

/// The reader's rendered bodies, and the thread that renders them ahead: one for the whole app,
/// handed to whoever renders. It is filled by a call on the client ([`Client::framing`]).
#[derive(Debug)]
pub struct Frames(Arc<core::Frames>);

impl Frames {
    /// An empty cache. `unsealed` says which messages were opened to a body of their own.
    pub fn new(unsealed: Unsealed) -> Self {
        Frames(Arc::new(core::Frames::new(unsealed)))
    }

    /// A cache for messages that are never sealed.
    pub fn plain() -> Self {
        Frames(Arc::new(core::Frames::plain()))
    }

    /// Show a message that has both bodies as `prefer` from now on.
    pub fn prefer(&self, prefer: Prefer) {
        self.0.prefer(prefer)
    }

    /// Which body a message that has both is shown as now.
    pub fn preferred(&self) -> Prefer {
        self.0.preferred()
    }

    /// The rendering of `message` under `policy`, when it is cached.
    pub fn peek(&self, message: &Message, policy: SanitizePolicy) -> Option<Arc<FrameBody>> {
        self.0.peek(message, policy)
    }

    /// What a rendering of `message` under `policy` is kept under.
    pub fn keyed(&self, message: &Message, policy: SanitizePolicy) -> Option<Key> {
        self.0.keyed(message, policy)
    }
}

impl Client {
    /// `looks`, filled from this client's store.
    pub fn looking<'a>(&'a self, looks: &'a Looks) -> Looking<'a> {
        Looking {
            client: self,
            of: &looks.0,
        }
    }

    /// `frames`, filled from this client's store.
    pub fn framing<'a>(&'a self, frames: &'a Frames) -> Framing<'a> {
        Framing {
            client: self,
            of: &frames.0,
        }
    }
}

/// A [`Looks`] read through a client ([`Client::looking`]).
#[derive(Clone, Copy, Debug)]
pub struct Looking<'a> {
    client: &'a Client,
    of: &'a core::Looks,
}

impl Looking<'_> {
    /// [`core::Looks::checks`].
    pub fn checks(&self, message: MessageId, raw: BlobId) -> Option<AuthResults> {
        self.of.checks(self.client.store(), message, raw)
    }

    /// [`core::Looks::invite`].
    pub fn invite(&self, message: MessageId, body: Option<BlobId>) -> Option<Invited> {
        self.of.invite(self.client.store(), message, body)
    }

    /// [`core::Looks::invite_again`].
    pub fn invite_again(&self, message: MessageId) -> Option<Invited> {
        self.of.invite_again(self.client.store(), message)
    }

    /// [`core::Looks::receipts`].
    pub fn receipts(&self, key: &[(MessageId, Option<BlobId>)]) -> Vec<Receipt> {
        self.of.receipts(self.client.store(), key)
    }

    /// [`core::Looks::code`].
    pub fn code(&self, message: MessageId, body: Option<BlobId>) -> Option<String> {
        self.of.code(self.client.store(), message, body)
    }

    /// [`core::Looks::post`].
    pub fn post(&self, thread: ThreadId, key: &Bodies) -> Option<MessageId> {
        self.of.post(self.client.store(), thread, key)
    }

    /// [`core::Looks::receipt_again`].
    pub fn receipt_again(&self, message: MessageId) {
        self.of.receipt_again(self.client.store(), message)
    }

    /// [`core::Looks::offer`].
    pub fn offer(&self, thread: ThreadId, key: &Bodies) -> Option<Offer> {
        self.of.offer(self.client.store(), thread, key)
    }

    /// [`core::Looks::sealed`].
    pub fn sealed(
        &self,
        secrets: &dyn SigningStore,
        message: MessageId,
        body: Option<BlobId>,
        now: DateTime<Utc>,
    ) -> Opening {
        self.of
            .sealed(self.client.store(), secrets, message, body, now)
    }

    /// [`core::Looks::unlocked`].
    pub fn unlocked(
        &self,
        secrets: &dyn SigningStore,
        message: MessageId,
        body: Option<BlobId>,
        passphrase: Password,
        now: DateTime<Utc>,
    ) -> Opening {
        self.of
            .unlocked(self.client.store(), secrets, message, body, passphrase, now)
    }
}

/// [`Frames`] read through a client ([`Client::framing`]).
#[derive(Clone, Copy, Debug)]
pub struct Framing<'a> {
    client: &'a Client,
    of: &'a Arc<core::Frames>,
}

impl Framing<'_> {
    /// [`core::Frames::uncached`].
    pub fn uncached(&self, message: &Message, policy: SanitizePolicy) -> FrameBody {
        self.of.uncached(self.client.store(), message, policy)
    }

    /// [`core::Frames::rendered`].
    pub fn rendered(&self, message: &Message, policy: SanitizePolicy) -> Arc<FrameBody> {
        self.of.rendered(self.client.store(), message, policy)
    }

    /// [`core::Frames::render_inline`].
    pub fn render_inline(
        &self,
        message: &Message,
        policy: SanitizePolicy,
        room: &mut u64,
        sent: &Sent,
        later: &mut Vec<(Message, SanitizePolicy)>,
    ) -> Option<Arc<FrameBody>> {
        self.of
            .render_inline(self.client.store(), message, policy, room, sent, later)
    }

    /// [`core::Frames::warm`].
    pub fn warm(&self, threads: Vec<ThreadId>, policy: SanitizePolicy) {
        self.of.warm(self.client.store_arc(), threads, policy)
    }

    /// [`core::Frames::warm_now`].
    pub fn warm_now(
        &self,
        threads: &[ThreadId],
        policy: SanitizePolicy,
        still_wanted: impl Fn() -> bool,
    ) {
        self.of
            .warm_now(self.client.store(), threads, policy, still_wanted)
    }
}
