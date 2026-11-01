//! What the running program tells the window of itself, once, as it starts: whose mail it is over
//! and whether its own watch, in this process, holds the lock. Fetching asks both to know who
//! schedules it.

use mail_client::profile::ProfileId;
use std::sync::OnceLock;

static PROFILE: OnceLock<ProfileId> = OnceLock::new();
static WATCHING_HERE: OnceLock<fn() -> bool> = OnceLock::new();

/// Record the program's Profile and whether it watches in this process. The first call wins.
pub fn record(profile: ProfileId, watching_here: fn() -> bool) {
    let _ = PROFILE.set(profile);
    let _ = WATCHING_HERE.set(watching_here);
}

/// Whose mail the running program is over: the default Profile's until [`record`] is called.
pub fn profile() -> ProfileId {
    PROFILE
        .get()
        .cloned()
        .unwrap_or_else(|| mail_client::profile::Profile::default().id().clone())
}

/// Whether the program's own watch, in this process, holds the lock: a window of this process asks
/// it before it knocks on the lock's door.
pub fn watching_here() -> bool {
    WATCHING_HERE.get().is_some_and(|here| here())
}
