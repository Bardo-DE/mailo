//! What a window is told of keeping a rule and of putting a kept change on a server, as the client
//! says it: why a draft was not kept, and what pushing came to.
//!
//! Each is what a call on the client returns in place of the core's value of the same name, which
//! also holds the core's own errors and so is not a caller's to name.

use crate::CallError;
use mail_logic::rules::condition::ConditionError;
use mail_logic::rules::server::AwayServer;

/// Why a draft was not kept.
#[derive(Debug)]
pub enum SaveError {
    NoName,
    /// Another rule of the account has this name.
    Taken {
        name: String,
    },
    Condition(ConditionError),
    /// It does nothing: no action, and no stop.
    NoAction,
    /// The store did not take it.
    Store(CallError),
}

impl From<mail_core::rules::manage::SaveError> for SaveError {
    fn from(error: mail_core::rules::manage::SaveError) -> Self {
        use mail_core::rules::manage::SaveError as Core;
        match error {
            Core::NoName => SaveError::NoName,
            Core::Taken { name } => SaveError::Taken { name },
            Core::Condition(why) => SaveError::Condition(why),
            Core::NoAction => SaveError::NoAction,
            Core::Store(why) => SaveError::Store(why.into()),
        }
    }
}

/// What pushing to the server came to, after a change that was kept here either way: kept here
/// is not the same as running there.
#[derive(Debug)]
pub enum PushDone {
    /// The rules were put on the server, in its script.
    Pushed { address: String },
    /// The reply was set where a JMAP or Graph server keeps one.
    Set { address: String, on: AwayServer },
    /// The change is kept here; the server did not take it, for this reason.
    NotPushed(CallError),
}

impl From<mail_core::rules::server::PushDone> for PushDone {
    fn from(done: mail_core::rules::server::PushDone) -> Self {
        use mail_core::rules::server::PushDone as Core;
        match done {
            Core::Pushed { address, .. } => PushDone::Pushed { address },
            Core::Set { address, on } => PushDone::Set { address, on },
            Core::NotPushed(why) => PushDone::NotPushed(why.into()),
        }
    }
}
