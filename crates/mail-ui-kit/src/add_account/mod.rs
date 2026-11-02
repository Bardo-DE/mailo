//! Opening the Add Account window, as the parts that offer it see it: what it opens with
//! ([`Ask`], [`Join`]), the context that answers ([`AddAccountWindows`]) and the one the program
//! records for the launched window ([`record_opener`]), so a part asks and does not name the
//! window. And signing in straight in the browser ([`direct`]), which Settings, the Space's menu
//! and the dialog all offer.

pub mod direct;

use crate::space::{self, SpaceId, Spaces};
use dioxus::prelude::*;
use mail_client::profile::ProfileId;
use porter_core::AccountId;
use std::sync::{Arc, OnceLock};

/// What the window opens with: the address of an account being signed in again, already typed,
/// and which Space an account added there joins.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ask {
    pub address: Option<String>,
    pub join: Join,
}

/// Which Space an account added joins. Accounts and Spaces are apart: an account is in a Space
/// only because someone put it there, so one added from a Space (its menu, its sidebar, the
/// window with nothing in it) joins that Space, and one added from Settings › Accounts, or signed
/// in again, joins none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Join {
    /// The Space on screen when the account is added.
    #[default]
    OnScreen,
    /// This Space, whose menu it was added from.
    Into(SpaceId),
    /// No Space.
    Nowhere,
}

/// Whatever opens the add-account window: quire's event loop in the launched window, a recorder
/// in a test.
pub trait OpenAddAccount: Send + Sync + 'static {
    /// Open the window for `ask`, or raise it when it is open.
    fn open(&self, ask: Ask);
}

/// Where "Add account…" goes, as a root context. Without one it is quire's `open_window_with`.
#[derive(Clone)]
pub struct AddAccountWindows(pub Arc<dyn OpenAddAccount>);

impl std::fmt::Debug for AddAccountWindows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AddAccountWindows")
    }
}

/// Open the add-account window, or raise it. Call it from an event handler.
pub fn open() {
    request(Ask::default());
}

/// Open the window with `address` already typed: signing an account in again. Call it from an
/// event handler.
pub fn open_for(address: String) {
    request(Ask {
        address: Some(address),
        join: Join::Nowhere,
    });
}

/// What a failure that asks to allow Mail again opens: Add Account, the one way an account comes
/// back. Call it from an event handler.
pub fn allow_again() {
    request(Ask::default());
}

fn request(ask: Ask) {
    match try_consume_context::<AddAccountWindows>() {
        Some(windows) => windows.0.open(ask),
        None => match OPENER.get() {
            Some(open) => open(ask),
            None => log::warn!("nothing opens the Add Account window here"),
        },
    }
}

static OPENER: OnceLock<fn(Ask)> = OnceLock::new();

/// Record how the launched program opens the window, for an ask no context answers. The first
/// call wins.
pub fn record_opener(open: fn(Ask)) {
    let _ = OPENER.set(open);
}

/// The account mailo's client has for `address`, which the add has just written.
pub fn account_at(address: &str) -> Option<AccountId> {
    let client = crate::live::client();
    crate::data::account_rows(&client)
        .into_iter()
        .find(|row| row.address == address)
        .map(|row| row.id)
}

/// A new account joins the Space it was added from, as `join` names it; or none.
pub fn into_scope(spaces: &mut Signal<Spaces>, account: AccountId, join: Join) {
    if join == Join::Nowhere {
        return;
    }
    // The Spaces as they are on disk: this window's copy may be older than a Space another
    // window added or switched to meanwhile, and keeping it would undo that.
    if let Some(dirs) = try_consume_context::<crate::appearance::WindowDirs>()
        && let Some(stored) = space::stored(&dirs)
        && *spaces.peek() != stored
    {
        spaces.set(stored);
    }
    let widened = {
        let mut all = spaces.write();
        let into = match join {
            Join::Into(space) => space,
            Join::OnScreen | Join::Nowhere => all.current().id,
        };
        let mut widened = false;
        all.edit(into, |space| {
            widened = space.payload.widen(&ProfileId::default(), account)
        });
        widened
    };
    if widened {
        crate::frame::keep(&spaces.read());
    }
}
