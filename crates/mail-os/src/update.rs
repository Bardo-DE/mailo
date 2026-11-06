//! A newer mailo, found, fetched and put in place of the running one.
//!
//! Each release (`.github/workflows/release.yml`) carries `update.json`, which names its build,
//! its tag and each file with its SHA-256, and `update.json.sig`, the manifest's Ed25519
//! signature. What is trusted is that signature, against the public key built in
//! (`packaging/update-key.pub`): a release is taken only when its manifest is signed, names the
//! release it was found under and a build above the running one, and a file only with the hash
//! the manifest names ([`manifest`]).
//!
//! The steps, each its own call so the window decides when each runs:
//! 1. [`fetch::look`] asks GitHub for the newest release on the channel and returns an
//!    [`fetch::Offer`] when it is newer than the running mailo;
//! 2. [`fetch::download`] downloads the offer's file and checks its hash;
//! 3. [`install::stage`] unpacks it beside the others downloaded and checks the app it holds;
//! 4. [`install::swap`] puts the staged app where the running one is, as mailo quits, and
//!    [`install::relaunch`] opens it once this process is gone.
//!
//! Only a mailo built by the release workflow looks ([`Running::here`]): one built by hand has
//! build 0, and is someone's own, not to be replaced. macOS only, for now: the Linux packages are
//! the package manager's to update.

pub mod fetch;
pub mod install;
pub mod manifest;

use std::fmt;
use std::path::PathBuf;

/// Which releases a mailo follows: the daily each push to master publishes, or only the stable
/// releases a person publishes from a `v` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Daily,
    Stable,
}

impl Channel {
    /// The channel the release workflow built this mailo for (`MAILO_CHANNEL`): a daily follows
    /// the dailies, anything else the stable releases.
    pub fn built() -> Self {
        match option_env!("MAILO_CHANNEL") {
            Some("daily") => Self::Daily,
            _ => Self::Stable,
        }
    }
}

/// The mailo this process is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    /// The release workflow's run number this mailo was built in (`MAILO_BUILD`).
    pub build: u64,
    /// The `.app` it runs from.
    pub bundle: PathBuf,
}

impl Running {
    /// This process, when it is a mailo the release workflow built and runs from its `.app`;
    /// `None` for one built by hand (build 0), run from `target/`, or on another system.
    pub fn here() -> Option<Self> {
        let build = built()?;
        let bundle = bundle_of(&std::env::current_exe().ok()?)?;
        Some(Self { build, bundle })
    }
}

/// The build number the release workflow gave this mailo; `None` for one built by hand.
pub fn built() -> Option<u64> {
    option_env!("MAILO_BUILD")
        .and_then(|build| build.parse().ok())
        .filter(|&build| build > 0)
}

/// The `.app` an executable at `exe` sits in (`mailo.app/Contents/MacOS/mailo`), on macOS.
pub fn bundle_of(exe: &std::path::Path) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let named = |path: &std::path::Path, name: &str| path.file_name().is_some_and(|n| n == name);
    (named(macos, "MacOS")
        && named(contents, "Contents")
        && bundle.extension().is_some_and(|e| e == "app"))
    .then(|| bundle.to_path_buf())
}

/// Why no update was found, fetched or put in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// GitHub, or the download, did not answer as asked: its words.
    Network(String),
    /// The manifest's signature is not the release key's, or is not a signature at all.
    Signature,
    /// The manifest was signed but is not one this mailo reads, or names another release.
    Manifest(String),
    /// The release has no file for this system.
    NoFile,
    /// The downloaded file is not the one the manifest names.
    Hash,
    /// The download could not be unpacked, or holds no app this mailo would run.
    Unpack(String),
    /// The app could not be put where the running one is: the system's words.
    Install(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(said) => write!(f, "the release could not be fetched: {said}"),
            Self::Signature => f.write_str("the release is not signed with mailo's key"),
            Self::Manifest(said) => write!(f, "the release's manifest is not usable: {said}"),
            Self::NoFile => f.write_str("the release has no file for this system"),
            Self::Hash => f.write_str("the download is not the file the release names"),
            Self::Unpack(said) => write!(f, "the download could not be unpacked: {said}"),
            Self::Install(said) => write!(f, "the new mailo could not be put in place: {said}"),
        }
    }
}

impl std::error::Error for Failure {}

#[cfg(test)]
mod tests;
