//! The archives mail comes in and goes out as, named by the client: a path found to hold mail
//! ([`Source`]), a format mail can be written in ([`Writer`]), and where an export goes
//! ([`Target`]).
//!
//! Each holds the core's format handle and gives a caller no way to open or write one: a caller
//! picks one, shows its name and hands it back to a call on the client.

use mail_core::export as core_export;
use mail_core::formats::{self, Writer as CoreWriter};
use mail_core::import::Source as CoreSource;
use mail_core::transfer::Seen as CoreSeen;
use mail_logic::transfer::Refusal;
use std::path::{Path, PathBuf};

/// What a path holds: the format it was found to be, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source(pub(crate) CoreSource);

impl Source {
    /// The format's stable name: `mbox`, `maildir`.
    pub fn slug(&self) -> &'static str {
        self.0.archive.slug()
    }

    /// Where the mail is.
    pub fn path(&self) -> &Path {
        &self.0.path
    }
}

/// A format mail is written in.
#[derive(Clone, Copy)]
pub struct Writer(pub(crate) &'static dyn CoreWriter);

impl Writer {
    /// The format's stable name: the command line's `--mbox`, `--maildir`, `--eml`.
    pub fn slug(&self) -> &'static str {
        self.0.slug()
    }

    /// The format's proper name.
    pub fn label(&self) -> &'static str {
        self.0.label()
    }
}

impl std::fmt::Debug for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Writer").field(&self.slug()).finish()
    }
}

impl PartialEq for Writer {
    fn eq(&self, other: &Self) -> bool {
        self.slug() == other.slug()
    }
}

impl Eq for Writer {}

/// Every format mail can be written in.
pub fn writers() -> impl Iterator<Item = Writer> {
    formats::writers().map(Writer)
}

/// The format an export is written in unless another is chosen: an mbox.
pub fn usual() -> Writer {
    Writer(core_export::usual())
}

/// Where an export goes: the format, and the path.
#[derive(Debug, Clone)]
pub struct Target {
    pub writer: Writer,
    pub path: PathBuf,
}

impl Target {
    pub fn new(writer: Writer, path: PathBuf) -> Self {
        Self { writer, path }
    }

    pub(crate) fn core(&self) -> core_export::Target {
        core_export::Target::new(self.writer.0, self.path.clone())
    }
}

/// What a typed path holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// Nothing typed yet.
    Blank,
    /// Mail that can be imported: which kind, and how many messages.
    Mail {
        source: Source,
        count: usize,
    },
    Refused(Refusal),
}

impl From<CoreSeen> for Seen {
    fn from(seen: CoreSeen) -> Self {
        match seen {
            CoreSeen::Blank => Seen::Blank,
            CoreSeen::Mail { source, count } => Seen::Mail {
                source: Source(source),
                count,
            },
            CoreSeen::Refused(why) => Seen::Refused(why),
        }
    }
}

/// A name for the export in `dir` that nothing uses yet, from the query and the format:
/// `mailo-inbox.mbox`, `mailo-inbox` for a Maildir, `mailo-inbox-eml` for a directory of files.
pub fn suggested(dir: &Path, query: &str, writer: Writer) -> PathBuf {
    mail_core::transfer::suggested(dir, query, writer.0)
}

/// Look at `path` the way an import will read it, and count what is there.
///
/// Blocking and as slow as reading the source: an mbox is read to its end to count it.
pub fn look(path: &Path) -> Seen {
    mail_core::transfer::look(path).into()
}

/// A typed path, with a leading `~` meaning the user's home directory.
pub fn expand_here(typed: &str) -> PathBuf {
    mail_core::transfer::expand_here(typed)
}
