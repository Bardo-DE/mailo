//! What a release says of itself, and whether to believe it: `update.json`, its signature, and
//! which of GitHub's releases a channel takes. No I/O.

use super::{Channel, Failure};
use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;

/// The public half of the key the release workflow signs with: 32 bytes, base64.
const PUBLIC_KEY: &str = include_str!("../../../../packaging/update-key.pub");

/// The key a manifest must be signed with.
pub fn release_key() -> VerifyingKey {
    key_of(PUBLIC_KEY).expect("packaging/update-key.pub is an Ed25519 public key")
}

/// An Ed25519 public key from its 32 bytes in base64.
pub fn key_of(base64: &str) -> Option<VerifyingKey> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.trim())
        .ok()?;
    VerifyingKey::from_bytes(&bytes.try_into().ok()?).ok()
}

/// `update.json`: the release it is part of and its files.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    /// The release workflow's run number: it only goes up, so it orders every release.
    pub build: u64,
    pub version: String,
    pub channel: String,
    /// The release's tag, which the release it was found under must have.
    pub tag: String,
    pub commit: String,
    pub files: Files,
}

/// The files a release carries, by system. One this mailo does not know is not read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Files {
    pub macos_arm64: Option<File>,
}

/// One file of a release: its name among the release's assets, its SHA-256 (hex) and its size.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct File {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

impl Manifest {
    /// The file this system runs; `None` on a system the releases carry no app for.
    pub fn file_here(&self) -> Option<&File> {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            self.files.macos_arm64.as_ref()
        } else {
            None
        }
    }
}

/// The manifest in `json`, when `signature` (base64, as in `update.json.sig`) is `key`'s over
/// exactly those bytes, and it is the manifest of the release tagged `tag`.
pub fn verify(
    json: &[u8],
    signature: &str,
    key: &VerifyingKey,
    tag: &str,
) -> Result<Manifest, Failure> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(signature.trim())
        .map_err(|_| Failure::Signature)?;
    let signature = Signature::from_slice(&bytes).map_err(|_| Failure::Signature)?;
    key.verify_strict(json, &signature)
        .map_err(|_| Failure::Signature)?;
    let manifest: Manifest =
        serde_json::from_slice(json).map_err(|e| Failure::Manifest(e.to_string()))?;
    if manifest.tag != tag {
        return Err(Failure::Manifest(format!(
            "it is {}'s, found under {tag}",
            manifest.tag
        )));
    }
    Ok(manifest)
}

/// One of GitHub's releases, as its API lists them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
}

/// The release `channel` takes from `releases`, newest first as GitHub lists them: a daily
/// mailo the newest daily, a stable one the newest stable release. A draft is no one's yet. A
/// daily does not take a stable release: one tagged on an older commit than the newest daily has
/// a higher build all the same, and would take the daily back.
pub fn pick(releases: &[Release], channel: Channel) -> Option<&Release> {
    releases
        .iter()
        .find(|release| !release.draft && release.prerelease == (channel == Channel::Daily))
}

/// Whether `bytes` are the file `file` names.
pub fn matches(file: &File, size: u64, sha256: &[u8]) -> bool {
    size == file.size && hex(sha256).eq_ignore_ascii_case(&file.sha256)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
