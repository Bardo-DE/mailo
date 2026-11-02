//! Signing an account in again, as the parts that offer it see it: the word on the button, and
//! [`SignAgain`], the root of what answers it (the program's Add Account window and the dialog's
//! state), which a part asks through and does not name.

use crate::need::Need;
use dioxus::prelude::*;

/// What the button that signs an account in again says.
pub const SIGN_IN: &str = "Sign In";

/// Sign `address`'s account in again, for what it `Need`s: a context the window provides once.
/// Copy, so a handler can take it.
#[derive(Clone, Copy)]
pub struct SignAgain(pub Callback<(String, Need)>);

impl std::fmt::Debug for SignAgain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SignAgain")
    }
}

impl SignAgain {
    /// Sign `address`'s account in again. Call it from an event handler.
    pub fn ask(self, address: &str, need: Need) {
        self.0.call((address.to_owned(), need));
    }
}
