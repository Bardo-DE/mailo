//! The document in a message's frame, borrowed through the frame's node on Blitz: free only
//! when the window's own document is, which an event being handled holds.

use blitz_dom::BaseDocument;
use dioxus::prelude::*;
use dioxus_native_dom::NodeHandle;
use ds::base::time::{FRAME_SLACK, clock::sleep};

/// How many frames a task waits for the window's document to be free before it gives up.
const TRIES: usize = 30;

/// `frame`'s node, if the window's document is free now; `None` if it is not, or `frame` is
/// not a node of a Blitz document.
pub(super) fn now(frame: &MountedData) -> Option<&NodeHandle> {
    let handle = frame.downcast::<NodeHandle>()?;
    handle.try_doc().is_some().then_some(handle)
}

/// `frame`'s node, once the window's document is free to borrow; `None` if it is not a node of
/// a Blitz document, or never comes free.
pub(super) async fn free(frame: &MountedData) -> Option<&NodeHandle> {
    for _ in 0..TRIES {
        if let Some(handle) = now(frame) {
            return Some(handle);
        }
        sleep(FRAME_SLACK).await;
    }
    None
}

/// Run `change` on the document in `handle`'s frame, and draw it again. Free a moment ago, and
/// nothing has run since: the caller has not awaited between.
pub(super) fn in_frame<T>(
    handle: &NodeHandle,
    change: impl FnOnce(&mut BaseDocument) -> T,
) -> Option<T> {
    let mut doc = handle.doc_mut();
    let sub = doc.subdoc_mut(handle.node_id())?;
    let mut inner = sub.inner_mut();
    let said = change(&mut inner);
    inner.shell_provider.request_redraw();
    Some(said)
}
