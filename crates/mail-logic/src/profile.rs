//! Whose mail a store holds, so that one person can keep two lives apart: a Profile's id, its
//! name, and where on disk and in the keyring it keeps what is its own.
//!
//! A Profile is a store of its own: its own mail database and blobs, its accounts' settings, its
//! own signing keys in the keyring, and its own agent. Spaces sit over Profiles the way Arc's do,
//! each Space showing the mail of one; two Profiles share only the program and its app-wide
//! settings.
//!
//! The default Profile is the install that was here before Profiles were: its database stays in
//! the data directory itself and its settings in the config directory, and its keyring entries and
//! agent keep the names they always had, so nothing moves and an older mailo still finds it. Every
//! other Profile lives in a folder of its own, `profiles/<id>/` under the data directory.
//!
//! The directories are the caller's to find and hand in ([`Roots`]); nothing here reads the
//! environment, the disk or a source of randomness. Which Profiles there are, kept in
//! `profiles.json`, is `mail_core::profile::Registry`'s.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// The default Profile's id: the install that was here before Profiles were.
const DEFAULT_ID: &str = "default";

/// What names a Profile on disk, in the keyring and to the command line: `default`, or eight
/// lowercase hex digits.
///
/// It becomes a directory name and part of a keyring service, so it is checked once, where it
/// enters (parsing, and reading `profiles.json`), and nothing holding one checks it again.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProfileId(String);

impl ProfileId {
    /// An id from 32 random bits the caller draws. It may be one already in use: whoever keeps the
    /// registry draws again until it is not.
    pub fn from_random(bits: u32) -> Self {
        Self(format!("{bits:08x}"))
    }

    /// Whether this is the default Profile's.
    pub fn is_default(&self) -> bool {
        self.0 == DEFAULT_ID
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ProfileId {
    /// The default Profile's id.
    fn default() -> Self {
        Self(DEFAULT_ID.to_owned())
    }
}

fn is_valid(text: &str) -> bool {
    text == DEFAULT_ID
        || (text.len() == 8 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Text that is not a Profile id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a profile id: an id is \"default\" or eight lowercase hex digits")]
pub struct NotAProfileId(pub String);

impl FromStr for ProfileId {
    type Err = NotAProfileId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if is_valid(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(NotAProfileId(text.to_owned()))
        }
    }
}

impl TryFrom<String> for ProfileId {
    type Error = NotAProfileId;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        if is_valid(&text) {
            Ok(Self(text))
        } else {
            Err(NotAProfileId(text))
        }
    }
}

impl From<ProfileId> for String {
    fn from(id: ProfileId) -> Self {
        id.0
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a Profile keeps what is its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Home {
    /// Where the install before Profiles kept it: the data directory itself holds the database
    /// and blobs, and the settings are the config directory's files.
    Legacy,
    /// `profiles/<id>/` under the data directory, settings in its `settings/`.
    Folder,
}

/// The two directories every Profile's paths are under, found by the caller: mailo's data
/// directory and its config directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    pub data: PathBuf,
    pub config: PathBuf,
}

/// One Profile: its id, and the name it was given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    id: ProfileId,
    /// `None` for the default Profile until someone names it: the front end words it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

impl Default for Profile {
    /// The default Profile, unnamed: what a run with no registry and no `--profile` is over.
    fn default() -> Self {
        Self {
            id: ProfileId::default(),
            name: None,
        }
    }
}

impl Profile {
    /// A Profile called `name`.
    pub fn named(id: ProfileId, name: String) -> Self {
        Self {
            id,
            name: Some(name),
        }
    }

    pub fn id(&self) -> &ProfileId {
        &self.id
    }

    /// Call it `name`.
    pub fn rename(&mut self, name: String) {
        self.name = Some(name);
    }

    /// The name it was given; `None` for a default Profile nobody has named.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Where it keeps what is its own. The default Profile is the install in place, every other
    /// one has a folder.
    pub fn home(&self) -> Home {
        if self.id.is_default() {
            Home::Legacy
        } else {
            Home::Folder
        }
    }

    /// The directory its database and blobs are in.
    pub fn data_dir(&self, roots: &Roots) -> PathBuf {
        match self.home() {
            Home::Legacy => roots.data.clone(),
            Home::Folder => roots.data.join("profiles").join(self.id.as_str()),
        }
    }

    /// Its mail database.
    pub fn db(&self, roots: &Roots) -> PathBuf {
        self.data_dir(roots).join("mail.db")
    }

    /// Its blob directory: message bodies and attachments.
    pub fn blobs(&self, roots: &Roots) -> PathBuf {
        self.data_dir(roots).join("blobs")
    }

    /// The keyring service its signing keys are kept under: `mailo` for the default Profile, as
    /// before, and `mailo/<id>` for another, so one key in two Profiles is two entries.
    pub fn signing_service(&self) -> String {
        match self.home() {
            Home::Legacy => "mailo".to_owned(),
            Home::Folder => format!("mailo/{}", self.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn an_id_that_is_not_one_is_refused_when_parsed() {
        for text in [
            "",
            "deadbee",
            "deadbeef0",
            "DEADBEEF",
            "ghijklmn",
            "../../x",
            "Default",
        ] {
            assert_eq!(
                text.parse::<ProfileId>(),
                Err(NotAProfileId(text.to_owned()))
            );
        }
        assert!("deadbeef".parse::<ProfileId>().is_ok());
    }

    #[test]
    fn the_default_profile_is_the_install_in_place_and_another_has_a_folder() {
        let roots = Roots {
            data: PathBuf::from("/data/mailo"),
            config: PathBuf::from("/config/mailo"),
        };
        let default = Profile::default();
        assert_eq!(default.db(&roots), Path::new("/data/mailo/mail.db"));
        assert_eq!(default.blobs(&roots), Path::new("/data/mailo/blobs"));
        assert_eq!(default.signing_service(), "mailo");

        let work = Profile {
            id: "0a1b2c3d".parse().unwrap(),
            name: Some("Work".to_owned()),
        };
        assert_eq!(work.home(), Home::Folder);
        assert_eq!(
            work.db(&roots),
            Path::new("/data/mailo/profiles/0a1b2c3d/mail.db")
        );
        assert_eq!(
            work.blobs(&roots),
            Path::new("/data/mailo/profiles/0a1b2c3d/blobs")
        );
        assert_eq!(work.signing_service(), "mailo/0a1b2c3d");
    }
}
