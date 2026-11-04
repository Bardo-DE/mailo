//! What the reader renders ahead, and where each rendering has got to: the key a rendering is
//! kept under, which conversations to render before anyone opens one, and how much raw mail the
//! frame that draws it may render itself. The cache that holds the renderings is
//! `mail_core::message::Frames`.

use mail_domain::{BlobId, ThreadId};
use mail_reader::{Prefer, SanitizePolicy};

/// What a rendering is cached under: the bytes, the policy, and which body was preferred.
pub type Key = (BlobId, SanitizePolicy, Prefer);

/// Raw mail, in bytes, that the reader renders on the frame that draws it. A conversation nobody
/// rendered ahead (one opened from a search, far down the list) would otherwise hold that frame
/// for every parse and sanitize in it; what is past this renders on a blocking thread instead,
/// and is drawn when it lands. A plain reply is a few kilobytes, a newsletter a few hundred.
pub const INLINE_BYTES: u64 = 128 * 1024;

/// How many conversations from the top of the list are rendered before anyone opens one.
pub const FIRST_PAGE: usize = 20;

/// Where each rendering one reader sent off the thread has got to.
#[derive(Debug, Default)]
pub struct Sent {
    /// Being rendered: drawn as a placeholder until it lands.
    going: Vec<Key>,
    /// Rendered, and still not in the cache — it was larger than the whole budget, or evicted
    /// since. Rendered on the frame from then on, or it would be sent for ever.
    landed: Vec<Key>,
}

impl Sent {
    /// `keys` are being rendered off the thread.
    pub fn start(&mut self, keys: &[Key]) {
        self.going.extend(keys.iter().copied());
    }

    /// Whether `key` is being rendered off the thread.
    pub fn is_going(&self, key: &Key) -> bool {
        self.going.contains(key)
    }

    /// Whether `key` was rendered off the thread and is still not in the cache.
    pub fn has_landed(&self, key: &Key) -> bool {
        self.landed.contains(key)
    }

    /// `keys` have been rendered.
    pub fn finish(&mut self, keys: Vec<Key>) {
        self.going.retain(|key| !keys.contains(key));
        self.landed.extend(keys);
    }
}

/// Which conversations to render ahead, most wanted first: the two after the open one and the
/// one before it — where `j` and `k` go next — then the top of the list. Never the open one,
/// which the reader is rendering already, and never one twice.
pub fn ahead(open: Option<ThreadId>, list: &[ThreadId], page: usize) -> Vec<ThreadId> {
    let mut order = Vec::new();
    if let Some(here) = open.and_then(|id| list.iter().position(|it| *it == id)) {
        let neighbours = [
            here.checked_add(1),
            here.checked_add(2),
            here.checked_sub(1),
        ];
        order.extend(neighbours.into_iter().flatten().filter_map(|i| list.get(i)));
    }
    for id in list.iter().take(page) {
        if !order.contains(id) {
            order.push(*id);
        }
    }
    order.retain(|id| Some(*id) != open);
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: u128) -> Vec<ThreadId> {
        (1..=n)
            .map(|i| ThreadId::from_uuid(uuid::Uuid::from_u128(i)))
            .collect()
    }

    /// Which conversations are warmed ahead, by their place in the list: the open one's
    /// neighbours first, then the top of the list, never the open one, and nothing past either
    /// end.
    #[test]
    fn ahead_order() {
        type Row = (&'static str, u128, Option<usize>, usize, &'static [usize]);
        const CASES: &[Row] = &[
            ("neighbours, then the top", 6, Some(2), 3, &[3, 4, 1, 0]),
            ("nothing open is the top", 6, None, 3, &[0, 1, 2]),
            ("open at the last row", 3, Some(2), 0, &[1]),
            ("open at the first row", 3, Some(0), 0, &[1, 2]),
        ];
        for (name, rows, open, page, want) in CASES {
            let list = ids(*rows);
            let want: Vec<ThreadId> = want.iter().map(|row| list[*row]).collect();
            assert_eq!(
                ahead(open.map(|row| list[row]), &list, *page),
                want,
                "{name}"
            );
        }
    }
}
