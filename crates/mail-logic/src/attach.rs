//! A file name safe to write, and which files run when the system opens them.
//!
//! The dangerous part of saving an attachment is its file name. It is chosen by whoever sent the
//! message — `name` is a MIME parameter, not a fact — so `../../../.ssh/authorized_keys` is a
//! perfectly well-formed attachment name and writing it where it asks is how a mail client hands
//! someone else's machine over. Everything written goes through [`safe_name`]. Listing, reading
//! and writing the files are `mail_core::attach`'s.

use std::path::Path;

/// The longest file name to write.
///
/// 255 bytes is the limit on ext4, btrfs, XFS and APFS alike. A name that does not fit is
/// truncated rather than refused: a file with a shortened name is useful, and an error is not.
const MAX_NAME: usize = 255;

/// What to call an attachment whose claimed name is unusable.
const FALLBACK: &str = "attachment";

/// A file name safe to write, derived from what the message claims.
///
/// The claim is the sender's, so this keeps only what a name may be: one path component, no
/// separators, no control characters, nothing that resolves upwards. The result is always a
/// non-empty single component that cannot escape the directory it is joined to.
///
/// Both separators are stripped, not just this platform's: a message written on Windows names
/// its parts with backslashes, and a Unix client that treats `..\\..\\evil` as one component
/// writes a file with a very strange name — harmless here, but the same code on Windows writes
/// it two directories up.
pub fn safe_name(claimed: &str) -> String {
    let last = claimed.rsplit(['/', '\\']).next().unwrap_or(claimed).trim();

    let cleaned: String = last
        .chars()
        // Control characters and NUL: a name containing a newline makes a terminal print
        // something other than what was written, and NUL truncates the name at the syscall.
        // And the invisible format characters: a right-to-left override shows `exe.pdf` for a
        // name that ends `.exe`.
        .filter(|c| !c.is_control() && !is_format(*c))
        // What Windows refuses in a name, `:` among them, which names a stream of another file.
        .map(|c| if WINDOWS_REFUSES.contains(c) { '_' } else { c })
        .collect();
    // Windows drops a trailing dot or space, so `report.pdf.` would be written as `report.pdf`.
    let cleaned = cleaned
        .trim_start_matches(char::is_whitespace)
        .trim_end_matches(|c: char| c.is_whitespace() || c == '.');

    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return FALLBACK.to_owned();
    }
    if names_a_device(cleaned) {
        return truncate_keeping_extension(&format!("_{cleaned}"));
    }
    truncate_keeping_extension(cleaned)
}

/// Whether `c` is a format character (Unicode's general category Cf, as of Unicode 15.1): one
/// that is not drawn, but changes how what is around it is drawn or read.
fn is_format(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// The characters Windows refuses in a file name.
const WINDOWS_REFUSES: &str = "<>:\"|?*";

/// Whether Windows reads `name` as a device rather than a file: `CON`, `NUL`, `COM1` and the
/// rest, whatever follows a dot.
fn names_a_device(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let stem = stem.to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|kind| {
            stem.strip_prefix(kind)
                .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0")
        })
}

/// Whether a file called `name` is a program or a script, which runs rather than shows when the
/// system opens it: the window shows such a file in its folder instead of opening it.
pub fn runs_when_opened(name: &str) -> bool {
    const KINDS: &[&str] = &[
        "app",
        "action",
        "applescript",
        "bash",
        "bat",
        "bin",
        "cmd",
        "com",
        "command",
        "cpl",
        "csh",
        "desktop",
        "exe",
        "hta",
        "jar",
        "js",
        "jse",
        "ksh",
        "lnk",
        "msi",
        "pif",
        "pkg",
        "mpkg",
        "pl",
        "ps1",
        "py",
        "rb",
        "reg",
        "run",
        "scpt",
        "scr",
        "sh",
        "terminal",
        "tool",
        "vb",
        "vbe",
        "vbs",
        "workflow",
        "wsf",
        "wsh",
        "zsh",
        "appimage",
    ];
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| KINDS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Shorten to [`MAX_NAME`] bytes, keeping the extension where there is one.
///
/// The extension is what decides which program opens the file, so losing it to a long name is
/// losing the point of saving it.
fn truncate_keeping_extension(name: &str) -> String {
    if name.len() <= MAX_NAME {
        return name.to_owned();
    }
    let extension = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.len() < 32)
        .map(|e| format!(".{e}"))
        // A name with no extension, or an unreadable one, keeps none.
        .unwrap_or_default();
    let room = MAX_NAME.saturating_sub(extension.len());
    let mut stem: String = String::new();
    for c in name.chars() {
        if stem.len() + c.len_utf8() > room {
            break;
        }
        stem.push(c);
    }
    format!("{stem}{extension}")
}

/// One attachment of a message opened for reading — decrypted, or with its signature taken off —
/// whose bytes are in the opened message rather than in the store: the stored parts of an
/// encrypted message are ciphertext, and [`save`] would write that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedAttachment {
    /// As the message names it, untrusted: [`save_opened`] makes it safe.
    pub name: String,
    /// The declared media type, untrusted.
    pub mime: String,
    pub bytes: Vec<u8>,
}
