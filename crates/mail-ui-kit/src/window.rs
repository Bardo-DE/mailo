//! A conversation in a window of its own, as the parts that ask for one see it: "Open in new window"
//! on a row's menu and in the reader's, and what answers is the program's ([`Windows`], a root
//! context) and not theirs.

use dioxus::prelude::*;
use ds::prelude::Icon;
use mail_domain::ThreadId;
use std::sync::Arc;

/// A conversation to open in a window of its own, and the window's title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub thread: ThreadId,
    /// The conversation's subject.
    pub title: String,
}

/// Whatever opens a conversation's window: quire's event loop in the launched window, a recorder
/// in a test.
pub trait OpenWindow: Send + Sync + 'static {
    /// Open `ask`'s conversation in a window of its own.
    fn open(&self, ask: Ask);
}

/// X/// `open_window_with`. Cloning shares it.
#[derive(Clone)]
pub struct Windows(pub Arc<dyn OpenWindow>);

impl std::fmt::Debug for Windows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Windows")
    }
}

/// Open `thread` in a window of its own, titled with its subject. A conversation the client no
/// longer has opens nothing, and so does a window given no [`Windows`].
pub fn open_in_window(thread: ThreadId) {
    let client = crate::live::client();
    let Ok(loaded) = client.stored().thread(thread) else {
        return;
    };
    let ask = Ask {
        thread,
        title: mail_domain::subject_or_none(&loaded.summary.subject).into_owned(),
    };
    if let Some(windows) = try_consume_context::<Windows>() {
        windows.0.open(ask);
    }
}

/// What the menu row that opens a conversation in its own window is called.
pub const OPEN_NAME: &str = "Open in new window";

/// The menu row that opens a conversation in its own window, with the key that does it as
/// `keymap` has it (no key beside it when the keymap binds none).
pub fn menu_item(keymap: &chordkit::Keymap) -> crate::menu::MenuItem {
    let shortcut = crate::actions::hint_own(keymap, crate::actions::Own::OpenInWindow);
    crate::menu::MenuItem {
        right: shortcut.map_or(
            crate::menu::Right::None,
            crate::menu::Right::Shortcut,
        ),
        ..crate::menu::MenuItem::icon(OPEN_KEY, Icon::Window, OPEN_NAME)
    }
}

/// The menu row's key.
pub const OPEN_KEY: &str = "open-window";

