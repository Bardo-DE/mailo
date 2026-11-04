//! What the reader has already rendered, kept so opening a conversation twice costs a lookup.
//!
//! [`frame_of`] is a pure function of the message's raw blob and the sanitize policy: a blob is
//! content-addressed, so the same [`BlobId`] is the same bytes for ever, and the cache can never
//! need invalidating. The whole [`SanitizePolicy`] is in the key, version included — a key
//! without it would serve markup sanitized under one policy to a reader that asked for another,
//! which for `RemoteImages` is a read receipt nobody granted.
//!
//! Bounded by the size of the html it holds, least recently used out first. It is a `Mutex`, not
//! a signal, so [`Frames::warm`] can fill it from an ordinary thread that nothing has to poll
//! (a future spawned from a render is never polled). The cache is a value the front-end owns
//! and hands to whoever renders, not a process global: two of them share nothing.

pub use mail_logic::frames::{FIRST_PAGE, INLINE_BYTES, Key, Sent, ahead};

use crate::error::Logged;
use mail_domain::{Message, ThreadId};
use mail_mime::Parsed;
use mail_reader::{FrameBody, Prefer, SanitizePolicy, Source, frame_of};
use mail_store::{SqliteStore, Store};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Render a message body into its sandboxed frame, every time, saying what it rendered from.
/// The reader's [`frame_of`], given the stored bytes this reads (`unsealed` is the body OpenPGP
/// or S/MIME opened the message to, when it has been opened). [`Frames::rendered`] remembers the
/// answer; this is the work itself, for the cache to call and for a measurement that must not
/// hit it.
pub fn render(
    store: &SqliteStore,
    message: &Message,
    policy: SanitizePolicy,
    prefer: Prefer,
    unsealed: &dyn Fn(&Message) -> Option<Parsed>,
) -> (FrameBody, Source) {
    frame_of(message, policy, prefer, unsealed, &|message| {
        let raw = message.body.raw()?;
        // A reader, not the writer: a sync's ingest holds the writer for a whole batch.
        let bytes = store
            .blobs()
            .get(raw)
            .or_log("a message's bytes could not be read")?;
        mail_mime::parse(&bytes).ok()
    })
}

/// How much rendered html to keep, in bytes. A long newsletter is a few hundred kilobytes.
const BUDGET: usize = 32 * 1024 * 1024;

/// The body OpenPGP or S/MIME opened a message to, when it has been opened. Such a message is
/// not cached: its rendering is a function of more than the blob, and of a key that may be
/// locked again.
pub type Unsealed = Arc<dyn Fn(&Message) -> Option<Parsed> + Send + Sync>;

struct Entry {
    /// Shared, so a hit hands out the rendering rather than a copy of its html and images.
    body: Arc<FrameBody>,
    bytes: usize,
    /// When it was last put or read, by [`Cache::tick`]: the smallest goes first.
    used: u64,
}

/// Hashed, so a lookup on the thread that draws costs the same with ten thousand small bodies in
/// it as with ten. Eviction is the one walk over it, and only when a put goes over the budget.
struct Cache {
    entries: HashMap<Key, Entry>,
    held: usize,
    budget: usize,
    tick: u64,
}

impl Cache {
    fn new(budget: usize) -> Self {
        Cache {
            entries: HashMap::new(),
            held: 0,
            budget,
            tick: 0,
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn get(&mut self, key: Key) -> Option<Arc<FrameBody>> {
        self.tick += 1;
        let tick = self.tick;
        let entry = self.entries.get_mut(&key)?;
        entry.used = tick;
        Some(Arc::clone(&entry.body))
    }

    fn put(&mut self, key: Key, body: Arc<FrameBody>) {
        self.tick += 1;
        let used = self.tick;
        if let Some(gone) = self.entries.remove(&key) {
            self.held -= gone.bytes;
        }
        let bytes = size(&body);
        // One body larger than the whole budget is not worth evicting everything for.
        if bytes > self.budget {
            return;
        }
        self.held += bytes;
        self.entries.insert(key, Entry { body, bytes, used });
        while self.held > self.budget {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key);
            let Some(gone) = oldest.and_then(|key| self.entries.remove(&key)) else {
                break;
            };
            self.held -= gone.bytes;
        }
    }
}

fn size(body: &FrameBody) -> usize {
    match body {
        FrameBody::NotFetched => 0,
        FrameBody::Present { html, fetches, .. } => {
            html.len() + fetches.iter().map(String::len).sum::<usize>()
        }
    }
}

/// The reader's rendered bodies, and the thread that renders them ahead.
pub struct Frames {
    unsealed: Unsealed,
    cache: Mutex<Cache>,
    /// Bumped by every [`Frames::warm`], so that only the newest one keeps going.
    warming: AtomicU64,
    /// Which body a message with both is shown as: the reader's setting, set by the front-end
    /// ([`Frames::prefer`]) and part of every key.
    prefer: Mutex<Prefer>,
}

impl std::fmt::Debug for Frames {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frames")
            .field("warming", &self.warming)
            .finish_non_exhaustive()
    }
}

impl Frames {
    /// An empty cache. `unsealed` says which messages were opened to a body of their own.
    pub fn new(unsealed: Unsealed) -> Self {
        Self::with_budget(unsealed, BUDGET)
    }

    /// A cache for messages that are never sealed.
    pub fn plain() -> Self {
        Self::new(Arc::new(|_| None))
    }

    fn with_budget(unsealed: Unsealed, budget: usize) -> Self {
        Frames {
            unsealed,
            cache: Mutex::new(Cache::new(budget)),
            warming: AtomicU64::new(0),
            prefer: Mutex::new(Prefer::default()),
        }
    }

    /// Show a message that has both bodies as `prefer` from now on. What was rendered under
    /// the other choice stays cached under it, so switching back is a lookup.
    pub fn prefer(&self, prefer: Prefer) {
        *self.prefer.lock().unwrap_or_else(|held| held.into_inner()) = prefer;
    }

    /// Which body a message that has both is shown as now.
    pub fn preferred(&self) -> Prefer {
        *self.prefer.lock().unwrap_or_else(|held| held.into_inner())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|held| held.into_inner())
    }

    /// What `message` renders to under `policy`, every time, past the cache. For a measurement
    /// that must not hit it.
    pub fn uncached(
        &self,
        store: &SqliteStore,
        message: &Message,
        policy: SanitizePolicy,
    ) -> FrameBody {
        render(store, message, policy, self.preferred(), &*self.unsealed).0
    }

    /// What `message` renders to under `policy`, from the cache when it has been rendered before.
    ///
    /// A message with no body yet, or one opened to a body of its own by OpenPGP or S/MIME, is
    /// not cached: the first has nothing to key on and the second is a function of more than the
    /// blob.
    pub fn rendered(
        &self,
        store: &SqliteStore,
        message: &Message,
        policy: SanitizePolicy,
    ) -> Arc<FrameBody> {
        let Some(key) = self.keyed(message, policy) else {
            return Arc::new(self.uncached(store, message, policy));
        };
        if let Some(had) = self.lock().get(key) {
            return had;
        }
        // Rendered outside the lock: a parse and a sanitize must not hold up a lookup. Kept only
        // when it was rendered from the blob: a seal opened since `keyed` looked would otherwise
        // leave plaintext under the blob's key, served once the key is locked again.
        let (body, source) = render(store, message, policy, key.2, &*self.unsealed);
        let body = Arc::new(body);
        if source == Source::Stored {
            self.lock().put(key, Arc::clone(&body));
        }
        body
    }

    /// The cached rendering, if there is one. Never renders.
    pub fn peek(&self, message: &Message, policy: SanitizePolicy) -> Option<Arc<FrameBody>> {
        self.lock().get(self.keyed(message, policy)?)
    }

    /// The key `message` is cached under at `policy`, when it can be.
    pub fn keyed(&self, message: &Message, policy: SanitizePolicy) -> Option<Key> {
        if (self.unsealed)(message).is_some() {
            return None;
        }
        Some((message.body.raw()?, policy, self.preferred()))
    }

    /// What `message` renders to under `policy`, if it can be had on this frame: from the cache,
    /// or rendered here while `room` still holds its raw size. `None` when it is to be rendered
    /// off the thread; it is then in `later`, unless it is already being rendered.
    pub fn render_inline(
        &self,
        store: &SqliteStore,
        message: &Message,
        policy: SanitizePolicy,
        room: &mut u64,
        sent: &Sent,
        later: &mut Vec<(Message, SanitizePolicy)>,
    ) -> Option<Arc<FrameBody>> {
        let Some(key) = self.keyed(message, policy) else {
            return Some(self.rendered(store, message, policy));
        };
        if let Some(had) = self.lock().get(key) {
            return Some(had);
        }
        if sent.is_going(&key) {
            return None;
        }
        let size = store
            .blobs()
            .size(key.0)
            .or_log_default("a picture's size could not be read");
        if size <= *room || sent.has_landed(&key) {
            *room = room.saturating_sub(size);
            return Some(self.rendered(store, message, policy));
        }
        if !later
            .iter()
            .any(|(m, p)| m.id == message.id && *p == policy)
        {
            later.push((message.clone(), policy));
        }
        None
    }

    /// Render the messages of `threads` into the cache on a thread of its own, in order, so that
    /// opening any of them is a lookup.
    ///
    /// A plain `std::thread`, not a task: it writes into a `Mutex` and needs nothing to poll it
    /// and nothing to notice when it finishes, which is why it works whatever the window's event
    /// loop is doing. Each call supersedes the one before: a selection that has moved on
    /// abandons what it was warming between messages rather than finishing it. The store is read
    /// on that thread too, so asking costs the caller nothing.
    pub fn warm(
        self: &Arc<Self>,
        store: Arc<SqliteStore>,
        threads: Vec<ThreadId>,
        policy: SanitizePolicy,
    ) {
        let generation = self.warming.fetch_add(1, Ordering::Relaxed) + 1;
        let frames = Arc::clone(self);
        let current = move || frames.warming.load(Ordering::Relaxed) == generation;
        let frames = Arc::clone(self);
        // Warming is ahead of need: without the thread, a conversation is drawn when opened.
        std::thread::Builder::new()
            .name("reader-warm".to_owned())
            .spawn(move || frames.warm_now(&store, &threads, policy, current))
            .or_log("the reader could not start warming conversations");
    }

    /// [`Frames::warm`]'s work, on the calling thread. `still_wanted` is asked before each
    /// message.
    pub fn warm_now(
        &self,
        store: &SqliteStore,
        threads: &[ThreadId],
        policy: SanitizePolicy,
        still_wanted: impl Fn() -> bool,
    ) {
        for thread in threads {
            let Some(loaded) = store
                .thread(*thread)
                .or_log("a conversation could not be read to warm it")
            else {
                continue;
            };
            for id in &loaded.messages {
                if !still_wanted() {
                    return;
                }
                let Some(message) = store
                    .message(*id)
                    .or_log("a message could not be read to warm it")
                else {
                    continue;
                };
                if self.peek(&message, policy).is_none() {
                    // Not a result: rendering is what fills the cache, and the frame is kept there.
                    let _ = self.rendered(store, &message, policy);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_domain::BlobId;
    use mail_reader::RemoteImages;

    fn html(n: usize) -> Arc<FrameBody> {
        Arc::new(FrameBody::Present {
            html: "x".repeat(n),
            blocked_remote: false,
            fetches: Vec::new(),
            plain: false,
            inline: Vec::new(),
        })
    }

    fn key(n: u8, remote_images: RemoteImages, version: u32) -> Key {
        (
            BlobId::from_uuid(uuid::Uuid::from_u128(u128::from(n))),
            SanitizePolicy {
                remote_images,
                version,
                ..SanitizePolicy::FRAME
            },
            Prefer::Html,
        )
    }

    #[test]
    fn a_hit_is_the_body_that_was_put() {
        let mut cache = Cache::new(1000);
        let k = key(1, RemoteImages::Blocked, 1);
        cache.put(k, html(10));
        assert_eq!(cache.get(k), Some(html(10)));
    }

    #[test]
    fn the_policy_is_part_of_the_key() {
        let mut cache = Cache::new(1000);
        cache.put(key(1, RemoteImages::Blocked, 1), html(10));
        // The same bytes under another policy, or the same policy at another version, is a miss.
        assert_eq!(cache.get(key(1, RemoteImages::Allowed, 1)), None);
        assert_eq!(cache.get(key(1, RemoteImages::Blocked, 2)), None);
        // And so is which body was preferred.
        let (blob, policy, _) = key(1, RemoteImages::Blocked, 1);
        assert_eq!(cache.get((blob, policy, Prefer::Plain)), None);
    }

    /// The preference the front-end sets is the one every key is made with.
    #[test]
    fn the_preferred_body_keys_what_is_cached() {
        let frames = Frames::plain();
        assert_eq!(frames.preferred(), Prefer::Html);
        frames.prefer(Prefer::Plain);
        assert_eq!(frames.preferred(), Prefer::Plain);
    }

    #[test]
    fn it_stays_under_its_budget_and_drops_the_least_recently_used() {
        let mut cache = Cache::new(100);
        let (a, b, c) = (
            key(1, RemoteImages::Blocked, 1),
            key(2, RemoteImages::Blocked, 1),
            key(3, RemoteImages::Blocked, 1),
        );
        cache.put(a, html(40));
        cache.put(b, html(40));
        // `a` is read again, so `b` is now the oldest.
        assert!(cache.get(a).is_some());
        cache.put(c, html(40));
        assert!(cache.held <= 100);
        assert!(cache.get(b).is_none(), "the oldest went first");
        assert!(cache.get(a).is_some() && cache.get(c).is_some());
    }

    #[test]
    fn a_body_larger_than_the_budget_is_not_kept_and_evicts_nothing() {
        let mut cache = Cache::new(100);
        let small = key(1, RemoteImages::Blocked, 1);
        cache.put(small, html(10));
        cache.put(key(2, RemoteImages::Blocked, 1), html(500));
        assert!(cache.get(small).is_some());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn putting_a_key_twice_does_not_count_it_twice() {
        let mut cache = Cache::new(100);
        let k = key(1, RemoteImages::Blocked, 1);
        cache.put(k, html(60));
        cache.put(k, html(60));
        assert_eq!(cache.held, 60);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn two_caches_share_nothing() {
        let (one, two) = (Frames::plain(), Frames::plain());
        let k = key(1, RemoteImages::Blocked, 1);
        one.lock().put(k, html(10));
        assert!(one.lock().get(k).is_some());
        assert!(two.lock().get(k).is_none());
    }
}
