//! Asking GitHub for the newest release on a channel, and downloading its file.

use super::manifest::{self, File, Manifest, Release};
use super::{Channel, Failure};
use ed25519_dalek::VerifyingKey;
use sha2::{Digest, Sha256};
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Where releases are listed and their files downloaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The repository's API root: `<api>/releases` lists its releases, newest first.
    pub api: String,
    /// The root of release files: `<download>/<tag>/<name>`.
    pub download: String,
}

impl Source {
    /// mailo's own repository.
    pub fn github() -> Self {
        Self {
            api: "https://api.github.com/repos/Bardo-DE/mailo".into(),
            download: "https://github.com/Bardo-DE/mailo/releases/download".into(),
        }
    }
}

/// A release newer than the running mailo, its manifest verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub manifest: Manifest,
    /// This system's file in it.
    pub file: File,
    /// Where that file is downloaded from.
    pub url: String,
}

/// The release `channel` takes, when its signed manifest names a build above `running`. `None`
/// when there is no such release, or it is not newer.
pub async fn look(
    http: &reqwest::Client,
    source: &Source,
    channel: Channel,
    running: u64,
    key: &VerifyingKey,
) -> Result<Option<Offer>, Failure> {
    let listed = get(http
        .get(format!("{}/releases?per_page=30", source.api))
        .header(reqwest::header::ACCEPT, "application/vnd.github+json"))
    .await?;
    let releases: Vec<Release> =
        serde_json::from_slice(&listed).map_err(|e| Failure::Network(e.to_string()))?;
    let Some(release) = manifest::pick(&releases, channel) else {
        return Ok(None);
    };
    let base = format!("{}/{}", source.download, release.tag_name);
    let json = get(http.get(format!("{base}/update.json"))).await?;
    let signature = get(http.get(format!("{base}/update.json.sig"))).await?;
    let signature = String::from_utf8_lossy(&signature);
    let manifest = manifest::verify(&json, &signature, key, &release.tag_name)?;
    if manifest.build <= running {
        return Ok(None);
    }
    let file = manifest.file_here().cloned().ok_or(Failure::NoFile)?;
    Ok(Some(Offer {
        url: format!("{base}/{}", file.name),
        file,
        manifest,
    }))
}

/// `offer`'s file, downloaded into `dir` as `mailo-<build>.zip`, once its size and hash are the
/// ones the manifest names. What is downloaded is removed on any failure.
pub async fn download(
    http: &reqwest::Client,
    offer: &Offer,
    dir: &Path,
) -> Result<PathBuf, Failure> {
    let part = dir.join(format!("mailo-{}.zip.part", offer.manifest.build));
    let done = part.with_extension("");
    let got = fill(http, offer, dir, &part).await;
    let kept =
        got.and_then(|()| std::fs::rename(&part, &done).map_err(|e| Failure::Disk(e.to_string())));
    if kept.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    kept.map(|()| done)
}

/// `part` written with `offer`'s file, checked as it arrives.
async fn fill(
    http: &reqwest::Client,
    offer: &Offer,
    dir: &Path,
    part: &Path,
) -> Result<(), Failure> {
    let disk = |e: std::io::Error| Failure::Disk(e.to_string());
    std::fs::create_dir_all(dir).map_err(disk)?;
    let mut response = http
        .get(&offer.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| Failure::Network(e.to_string()))?;
    let mut out = std::fs::File::create(part).map_err(disk)?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| Failure::Network(e.to_string()))?
    {
        size += chunk.len() as u64;
        if size > offer.file.size {
            break;
        }
        hash.update(&chunk);
        out.write_all(&chunk).map_err(disk)?;
    }
    if manifest::matches(&offer.file, size, &hash.finalize()) {
        Ok(())
    } else {
        Err(Failure::Hash)
    }
}

async fn get(request: reqwest::RequestBuilder) -> Result<Vec<u8>, Failure> {
    let response = request
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| Failure::Network(e.to_string()))?;
    let body = response
        .bytes()
        .await
        .map_err(|e| Failure::Network(e.to_string()))?;
    Ok(body.to_vec())
}
