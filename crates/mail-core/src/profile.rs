//! Which Profiles there are: `profiles.json` in the data directory, read and written.
//!
//! A Profile, its id and where it keeps what is its own are `mail_logic::profile`. The registry
//! stays whole here: loading and saving are mixed through its edits ([`Registry::begin_remove`]
//! saves), and splitting the list from the file would leave two types for one idea.
//!
//! Which Profiles there are is kept in `profiles.json` in the data directory ([`Registry`]). No
//! such file is the ordinary case of one Profile, the default, and reading the registry writes
//! nothing: the file appears when a second Profile is added. Removing a Profile is two steps, so
//! that a removal stopped half way (its folder half deleted) is found and finished on the next
//! run: [`Registry::begin_remove`] records it, the caller deletes what the Profile kept, and
//! [`Registry::finish_remove`] forgets it.

use crate::config::write_json;
use crate::error::CoreError;
use mail_logic::profile::{Profile, ProfileId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "profiles.json";

/// The newest `profiles.json` this build reads. A newer one is refused rather than read and
/// written back without what it added.
const VERSION: u32 = 1;

/// `profiles.json` as it is written. Every field has a default, so a later field reads against
/// an older file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct File {
    version: u32,
    profiles: Vec<Profile>,
    /// Profiles whose removal began and has not finished.
    removing: Vec<ProfileId>,
}

impl Default for File {
    fn default() -> Self {
        Self {
            version: VERSION,
            profiles: Vec::new(),
            removing: Vec::new(),
        }
    }
}

impl File {
    /// The default Profile first, whether or not the file names it, and each id once.
    fn normalized(mut self) -> Self {
        let mut seen = std::collections::BTreeSet::new();
        self.profiles.retain(|profile| {
            let first = seen.insert(profile.id().clone());
            if !first {
                let id = &profile.id();
                log::warn!("{FILE_NAME}: profile {id} is listed twice; keeping the first");
            }
            first
        });
        match self.profiles.iter().position(|p| p.id().is_default()) {
            Some(0) => {}
            Some(at) => {
                let default = self.profiles.remove(at);
                self.profiles.insert(0, default);
            }
            None => self.profiles.insert(0, Profile::default()),
        }
        let mut seen = std::collections::BTreeSet::new();
        self.removing.retain(|id| seen.insert(id.clone()));
        self
    }
}

/// The Profiles there are, read from and written to `profiles.json` in a data directory.
#[derive(Debug, Clone)]
pub struct Registry {
    root: PathBuf,
    file: File,
}

impl Registry {
    /// The Profiles kept in `data_root`: one, the default, when there is no `profiles.json`.
    /// Writes nothing.
    ///
    /// A file that cannot be read, is not a registry, or was written by a newer mailo is an
    /// error, not one default Profile: that would hide every other Profile, and the next save
    /// would forget them.
    pub fn load(data_root: &Path) -> Result<Self, CoreError> {
        let path = data_root.join(FILE_NAME);
        let file = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<File>(&bytes)
                .map_err(|e| CoreError::context(path.display().to_string(), e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => File::default(),
            Err(e) => return Err(CoreError::at(&path)(e)),
        };
        if file.version > VERSION {
            return Err(CoreError::ProfilesTooNew {
                path,
                version: file.version,
            });
        }
        Ok(Self {
            root: data_root.to_path_buf(),
            file: file.normalized(),
        })
    }

    /// Write the registry to `profiles.json`, through a temporary file and a rename.
    pub fn save(&self) -> Result<(), CoreError> {
        write_json(&self.root, FILE_NAME, &self.file)
    }

    /// Every Profile, the default first.
    pub fn profiles(&self) -> &[Profile] {
        &self.file.profiles
    }

    pub fn get(&self, id: &ProfileId) -> Option<&Profile> {
        self.file.profiles.iter().find(|p| p.id() == id)
    }

    /// The Profile `text` names: its id in any case, else its name in any case.
    pub fn resolve(&self, text: &str) -> Result<&Profile, CoreError> {
        let by_id = text
            .to_ascii_lowercase()
            .parse::<ProfileId>()
            .ok() // text that is no id may still be a name, tried next
            .and_then(|id| self.get(&id));
        let wanted = text.trim().to_lowercase();
        by_id
            .or_else(|| {
                self.file
                    .profiles
                    .iter()
                    .find(|p| p.name().is_some_and(|n| n.to_lowercase() == wanted))
            })
            .ok_or_else(|| CoreError::NoProfile(text.to_owned()))
    }

    /// A new Profile called `name`, with a folder home and an id no Profile has. Not saved until
    /// [`save`](Self::save).
    pub fn add(&mut self, name: &str) -> Result<Profile, CoreError> {
        let name = self.usable_name(name, None)?;
        let id = loop {
            let id = fresh_id();
            if self.get(&id).is_none() && !self.file.removing.contains(&id) {
                break id;
            }
        };
        let profile = Profile::named(id, name);
        self.file.profiles.push(profile.clone());
        Ok(profile)
    }

    /// Call the Profile `id` `name`. Not saved until [`save`](Self::save).
    pub fn rename(&mut self, id: &ProfileId, name: &str) -> Result<(), CoreError> {
        let name = self.usable_name(name, Some(id))?;
        let profile = self
            .file
            .profiles
            .iter_mut()
            .find(|p| p.id() == id)
            .ok_or_else(|| CoreError::NoProfile(id.to_string()))?;
        profile.rename(name);
        Ok(())
    }

    /// `name` trimmed, if it can name a Profile other than `except`: not blank, not something
    /// [`resolve`](Self::resolve) would read as an id, and no other Profile's name.
    fn usable_name(&self, name: &str, except: Option<&ProfileId>) -> Result<String, CoreError> {
        let name = name.trim();
        if name.is_empty() || name.to_ascii_lowercase().parse::<ProfileId>().is_ok() {
            return Err(CoreError::ProfileNameUnusable(name.to_owned()));
        }
        let lower = name.to_lowercase();
        let taken =
            self.file.profiles.iter().any(|p| {
                Some(p.id()) != except && p.name().is_some_and(|n| n.to_lowercase() == lower)
            });
        if taken {
            return Err(CoreError::ProfileNameTaken(name.to_owned()));
        }
        Ok(name.to_owned())
    }

    /// Record that the Profile `id` is being removed, and save. It stays listed until
    /// [`finish_remove`](Self::finish_remove); deleting what it kept is the caller's, between
    /// the two.
    pub fn begin_remove(&mut self, id: &ProfileId) -> Result<(), CoreError> {
        if id.is_default() {
            return Err(CoreError::DefaultProfileStays);
        }
        if self.get(id).is_none() {
            return Err(CoreError::NoProfile(id.to_string()));
        }
        if !self.file.removing.contains(id) {
            self.file.removing.push(id.clone());
        }
        self.save()
    }

    /// Forget the Profile `id`, whose removal is done, and save.
    pub fn finish_remove(&mut self, id: &ProfileId) -> Result<(), CoreError> {
        if id.is_default() {
            return Err(CoreError::DefaultProfileStays);
        }
        self.file.profiles.retain(|p| p.id() != id);
        self.file.removing.retain(|r| r != id);
        self.save()
    }
}

/// A new id, from random bytes.
fn fresh_id() -> ProfileId {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    ProfileId::from_random(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
