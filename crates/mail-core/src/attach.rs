//! Attachments on a received message, and getting them back out.
//!
//! `mail-runtime` keeps every attachment of a message it ingests as a pointer into the message's
//! own bytes ([`PartContent::Inside`]) and `Message::attachments` has named them ever since;
//! messages kept before that have a blob of each part's own, and read as they did ([`blob_of`]). Nothing read that: no command listed them, nothing on screen showed them,
//! and there was no way at all to get a file out of a message. A mail client people are sent
//! PDFs through is not one that can only display text.
//!
//! The dangerous part is the file name. It is chosen by whoever sent the message — `name` is a
//! MIME parameter, not a fact — so `../../../.ssh/authorized_keys` is a perfectly well-formed
//! attachment name and writing it where it asks is how a mail client hands someone else's
//! machine over. Everything written here goes through [`safe_name`].

pub use mail_logic::attach::{OpenedAttachment, runs_when_opened, safe_name};

use crate::error::CoreError;
use mail_domain::{Attachment, BlobId, Message, MessageId, PartContent};
use mail_store::{SqliteStore, Store};
use std::path::{Path, PathBuf};

/// One attachment of a received message, as [`list`] shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// What it will be written as, not what it claims: the difference is the whole point, and a
    /// listing that showed the claim would mislead about what happens next.
    pub name: String,
    /// The declared media type, untrusted.
    pub mime: String,
    pub size: u64,
}

/// The attachments on a message, in the order [`save`] numbers them.
pub fn list(store: &SqliteStore, message: MessageId) -> Result<Vec<Listed>, CoreError> {
    let message = store.message(message)?;
    Ok(message
        .attachments
        .iter()
        .map(|attachment| Listed {
            name: safe_name(&attachment.name),
            mime: attachment.mime.clone(),
            size: attachment.size,
        })
        .collect())
}

/// Where the window puts a file it has been asked to save.
///
/// The folder the desktop names (`XDG_DOWNLOAD_DIR` on Linux, the system's Downloads folder on
/// macOS and Windows), `~/Downloads` when it names none, and the working directory as a last
/// resort. Deliberately not a file chooser: a chooser is a
/// dependency that pulls a toolkit in behind it, and every client that offers one also has a
/// default that most saves actually use. The path is reported afterwards, which is the part
/// that matters — a file saved somewhere the user cannot name is a file they have lost.
pub fn downloads_dir() -> PathBuf {
    downloads_from(
        mail_secrets::places::downloads_named().as_deref(),
        mail_secrets::places::home()
            .as_deref()
            .map(std::path::Path::as_os_str),
    )
}

/// The same decision, with the environment as arguments.
///
/// Separated because the workspace forbids `unsafe`, and `std::env::set_var` has been unsafe
/// since the 2024 edition — so a test that wanted to check this rule could not set the variable
/// it is about. The same answer the workspace gives for the clock: the ambient thing becomes a
/// parameter, and the decision becomes a function anything can call.
pub fn downloads_from(named: Option<&std::ffi::OsStr>, home: Option<&std::ffi::OsStr>) -> PathBuf {
    if let Some(named) = named {
        let path = PathBuf::from(named);
        // A relative setting would put the file wherever the process happens to have been
        // started, which for a desktop launcher is somewhere the user cannot guess.
        if path.is_absolute() {
            return path;
        }
    }
    match home {
        Some(home) => PathBuf::from(home).join("Downloads"),
        None => PathBuf::from("."),
    }
}

/// Write one attachment into `dir`, returning the path written.
///
/// Never overwrites. A message that arrives twice, or two messages with the same attachment
/// name, must not silently replace a file the user already has — so an existing name becomes
/// `report (2).pdf` rather than a lost file.
pub fn save(
    store: &SqliteStore,
    message: MessageId,
    index: usize,
    dir: &Path,
) -> Result<PathBuf, CoreError> {
    let message = store.message(message)?;
    let attachment: &Attachment =
        message
            .attachments
            .get(index)
            .ok_or_else(|| CoreError::NoAttachment {
                have: message.attachments.len(),
                index,
            })?;

    let blob = blob_of(store, &message, index)?;
    let bytes = store
        .blobs()
        .get(blob)
        .map_err(|e| CoreError::cannot("read the attachment", e))?;

    write_new(dir, &attachment.name, &bytes)
}

/// How many decoded parts [`blob_of`] remembers the blob of.
const REMEMBERED: usize = 16;

/// The decoded parts [`blob_of`] has put back lately, newest last, so a preview that asks for the
/// same part once a page does not decode the message each time. A blob that has since been swept
/// is found out by its size being unreadable, and decoded again.
static REMEMBERED_PARTS: std::sync::Mutex<Vec<((MessageId, usize), BlobId)>> =
    std::sync::Mutex::new(Vec::new());

/// The blob holding attachment `index` of `message`'s decoded bytes: its own when the message was
/// kept with one, else the part decoded out of the message's raw bytes and put in the store.
///
/// A part kept only inside the raw message is the cost of the message and nothing more on disk;
/// the copy this puts is for whoever is reading it now, and nothing names it, so a sweep of the
/// blobs takes it back once it has gone unused. Blocking: reads and parses the raw message.
///
/// A part still on the server is [`CoreError::AttachmentOnServer`]. One whose message has lost its
/// raw bytes, or no longer holds a part of that name and size at that place, is
/// [`CoreError::AttachmentUnreadable`], never a panic and never another part's bytes.
pub fn blob_of(store: &SqliteStore, message: &Message, index: usize) -> Result<BlobId, CoreError> {
    let attachment = message
        .attachments
        .get(index)
        .ok_or_else(|| CoreError::NoAttachment {
            have: message.attachments.len(),
            index,
        })?;
    let unreadable = |why: &str| CoreError::AttachmentUnreadable {
        name: attachment.name.clone(),
        why: why.to_owned(),
    };
    let place = match &attachment.content {
        PartContent::Held(blob) => return Ok(*blob),
        PartContent::Remote { .. } => {
            return Err(CoreError::AttachmentOnServer {
                name: attachment.name.clone(),
            });
        }
        PartContent::Inside { index } => *index as usize,
    };

    let key = (message.id, index);
    let remembered = match REMEMBERED_PARTS.lock() {
        Ok(parts) => parts.iter().find(|(k, _)| *k == key).map(|(_, blob)| *blob),
        // A poisoned memory is no memory: the part is decoded again.
        Err(_) => None,
    };
    if let Some(blob) = remembered
        && store.blobs().size(blob).is_ok()
    {
        return Ok(blob);
    }

    let raw = message
        .body
        .raw()
        .ok_or_else(|| unreadable("the message's bytes are not kept"))?;
    let bytes = store
        .blobs()
        .get(raw)
        .map_err(|_| unreadable("the message's bytes are missing from the store"))?;
    let parsed = mail_mime::parse(&bytes).map_err(|why| unreadable(&why.to_string()))?;
    // The place it was kept at, if a part of its name and size is still there: a reader that
    // numbers parts differently from the one that kept this must not hand over another file.
    let same = |part: &&mail_mime::ParsedPart| {
        part.name == attachment.name && part.bytes.len() as u64 == attachment.size
    };
    let part = parsed
        .attachments
        .get(place)
        .filter(same)
        .or_else(|| parsed.attachments.iter().find(same))
        .ok_or_else(|| unreadable("the message no longer holds a part like it"))?;
    let blob = store
        .blobs()
        .put(&part.bytes)
        .map_err(|e| CoreError::cannot("keep the attachment for reading", e))?;
    if let Ok(mut parts) = REMEMBERED_PARTS.lock() {
        parts.retain(|(k, _)| *k != key);
        if parts.len() >= REMEMBERED {
            parts.remove(0);
        }
        parts.push((key, blob));
    }
    Ok(blob)
}

/// Attachment `index` of an opened message, numbered as the reader lists an opened message's
/// attachments: in the order [`mail_mime::Parsed::attachments`] holds them, which is the order a
/// stored message's own attachments are numbered in too.
pub fn opened_attachment(
    shown: &mail_mime::Parsed,
    index: usize,
) -> Result<OpenedAttachment, CoreError> {
    let part = shown
        .attachments
        .get(index)
        .ok_or_else(|| CoreError::NoAttachment {
            have: shown.attachments.len(),
            index,
        })?;
    Ok(OpenedAttachment {
        name: part.name.clone(),
        mime: part.mime.clone(),
        bytes: part.bytes.clone(),
    })
}

/// Save an opened message's attachment into `dir`, as [`save`] saves a stored one: under its
/// name made safe, never over a file already there.
pub fn save_opened(attachment: &OpenedAttachment, dir: &Path) -> Result<PathBuf, CoreError> {
    write_new(dir, &attachment.name, &attachment.bytes)
}

/// Write `bytes` into `dir` under `name` made safe, never over a file already there, returning
/// the path written. How every file the window saves reaches the disk.
pub fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, CoreError> {
    create_unique(dir, &safe_name(name), bytes)
}

/// Write `bytes` into `dir` as `name`, or `name (2)` and so on, returning the path written.
///
/// A name is free only if claiming it succeeds (`create_new`), so neither a file written in the
/// meantime nor a link left in its place is written through. Only its owner can read the file,
/// and on macOS it is marked as come from elsewhere ([`mail_runtime::quarantine`]).
pub(crate) fn create_unique(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, CoreError> {
    use std::io::Write as _;
    std::fs::create_dir_all(dir).map_err(CoreError::at(dir))?;
    for candidate in candidates(name).take(1000) {
        let path = dir.join(&candidate);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        match options.open(&path) {
            Ok(mut file) => {
                file.write_all(bytes).map_err(CoreError::at(&path))?;
                // A file that could not be marked is still the file asked for.
                let _ = mail_runtime::quarantine::mark_now(&path, AGENT);
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(CoreError::at(&path)(e)),
        }
    }
    Err(CoreError::NamesTaken {
        name: name.to_owned(),
        dir: dir.to_owned(),
    })
}

/// Who wrote a marked file, as the quarantine mark names it.
const AGENT: &str = "mailo";

/// `name`, then `name (2)`, `name (3)` and on, the number before the extension.
fn candidates(name: &str) -> impl Iterator<Item = String> + '_ {
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    std::iter::once(name.to_owned()).chain((2..).map(move |n| format!("{stem} ({n}){extension}")))
}

/// [`save`], downloading the attachment first if a sync left it on the server.
///
/// `download` fetches one section of `message` and records it held; the application passes
/// [`crate::SyncOps::fetch_part`]. Taken as a parameter so a test can check "remote, then saved"
/// without a server.
///
/// Blocking and network-bound for a part that is still remote, so the UI calls it off the
/// render thread. Returns the path written.
pub fn fetch_and_save(
    store: &SqliteStore,
    message: MessageId,
    index: usize,
    dir: &Path,
    download: impl FnOnce(&mail_domain::Section) -> Result<(), CoreError>,
) -> Result<PathBuf, CoreError> {
    let stored = store.message(message)?;
    let attachment = stored
        .attachments
        .get(index)
        .ok_or_else(|| CoreError::NoAttachment {
            have: stored.attachments.len(),
            index,
        })?;
    if let mail_domain::PartContent::Remote { section } = &attachment.content {
        download(section)?;
    }
    save(store, message, index, dir)
}

/// A name in `dir` that nothing is using yet, to suggest: `name`, or `name (2)` and so on. A file
/// and a directory are the same kind of obstacle, so an export directory is named by it too.
/// Only a suggestion: whatever writes there claims it with `create_new`, as [`write_new`] does.
pub fn free_path(dir: &Path, name: &str) -> PathBuf {
    candidates(name)
        .take(1000)
        .map(|candidate| dir.join(candidate))
        .find(|path| !path.exists())
        .unwrap_or_else(|| dir.join(name))
}

pub use mail_domain::human_size;

#[cfg(test)]
mod tests {
    use super::{candidates, write_new};

    /// The number goes before the extension, and a name with none, or only a leading dot, takes
    /// it at the end.
    #[test]
    fn a_taken_name_is_numbered_before_its_extension() {
        // (name, the first three tried)
        const CASES: &[(&str, [&str; 3])] = &[
            (
                "report.pdf",
                ["report.pdf", "report (2).pdf", "report (3).pdf"],
            ),
            (
                "photos.tar.gz",
                ["photos.tar.gz", "photos.tar (2).gz", "photos.tar (3).gz"],
            ),
            ("README", ["README", "README (2)", "README (3)"]),
            (".profile", [".profile", ".profile (2)", ".profile (3)"]),
            (
                "Q3 (final).xlsx",
                [
                    "Q3 (final).xlsx",
                    "Q3 (final) (2).xlsx",
                    "Q3 (final) (3).xlsx",
                ],
            ),
        ];
        for (name, want) in CASES {
            let tried: Vec<String> = candidates(name).take(3).collect();
            assert_eq!(tried, want, "{name:?}");
        }
    }

    /// Several attachments written into one folder, as Save All writes a message's: each claimed
    /// name made safe, each clash numbered, nothing written over or outside the folder.
    #[test]
    fn a_message_s_attachments_written_into_one_folder_keep_each_other() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.path().join("minutes.txt"), b"already here")
            .unwrap_or_else(|e| panic!("{e}"));
        // (claimed name, what it holds, the name written)
        const CASES: &[(&str, &str, &str)] = &[
            ("minutes.txt", "one", "minutes (2).txt"),
            ("minutes.txt", "two", "minutes (3).txt"),
            ("../../minutes.txt", "three", "minutes (4).txt"),
            ("..\\evil.exe", "four", "evil.exe"),
            ("CON.txt", "five", "_CON.txt"),
            ("a\u{0}b\u{202E}.txt", "six", "ab.txt"),
            ("", "seven", "attachment"),
        ];
        for (claimed, holds, want) in CASES {
            let path =
                write_new(dir.path(), claimed, holds.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(path, dir.path().join(want), "{claimed:?}");
            assert_eq!(
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}")),
                *holds,
                "{claimed:?}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("minutes.txt"))
                .unwrap_or_else(|e| panic!("{e}")),
            "already here",
            "a file already there was written over"
        );
        let count = std::fs::read_dir(dir.path())
            .unwrap_or_else(|e| panic!("{e}"))
            .count();
        assert_eq!(count, CASES.len() + 1);
    }
}
