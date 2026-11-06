use super::manifest::{self, File, Release};
use super::{Channel, Failure, bundle_of};
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use std::path::Path;

/// A key of the tests' own: the release key's private half is in no test.
fn test_key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

fn signed(key: &SigningKey, json: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(key.sign(json).to_bytes())
}

const MANIFEST: &str = r#"{"build":42,"version":"0.1.0-daily.42","channel":"daily","tag":"daily-42","commit":"abc","files":{"macos_arm64":{"name":"mailo-0.1.0-daily.42-macos-arm64.zip","sha256":"AB01","size":2},"deb_amd64":{"name":"x.deb","sha256":"00","size":1}}}"#;

/// The key built in is the one `packaging/update-key.pub` holds.
#[test]
fn the_release_key_reads() {
    let key = manifest::release_key();
    let file = include_str!("../../../../packaging/update-key.pub");
    assert_eq!(Some(key), manifest::key_of(file));
}

/// A manifest signed with the key, found under its own tag, is read; files this mailo does not
/// know are passed over.
#[test]
fn a_signed_manifest_under_its_tag_is_read() {
    let key = test_key();
    let read = manifest::verify(
        MANIFEST.as_bytes(),
        &signed(&key, MANIFEST.as_bytes()),
        &key.verifying_key(),
        "daily-42",
    )
    .expect("it verifies");
    assert_eq!(read.build, 42);
    assert_eq!(
        read.files.macos_arm64.as_ref().map(|f| f.size),
        Some(2),
        "the macOS file is read"
    );
}

/// A manifest changed after signing, signed with another key, carrying no signature, or found
/// under another release's tag is refused.
#[test]
fn a_manifest_not_signed_as_found_is_refused() {
    let key = test_key();
    let other = SigningKey::from_bytes(&[9; 32]);
    let good = signed(&key, MANIFEST.as_bytes());
    let public = key.verifying_key();
    let changed = MANIFEST.replace("\"build\":42", "\"build\":43");
    assert_eq!(
        manifest::verify(changed.as_bytes(), &good, &public, "daily-42"),
        Err(Failure::Signature)
    );
    assert_eq!(
        manifest::verify(
            MANIFEST.as_bytes(),
            &signed(&other, MANIFEST.as_bytes()),
            &public,
            "daily-42"
        ),
        Err(Failure::Signature)
    );
    assert_eq!(
        manifest::verify(MANIFEST.as_bytes(), "not base64!", &public, "daily-42"),
        Err(Failure::Signature)
    );
    assert!(matches!(
        manifest::verify(MANIFEST.as_bytes(), &good, &public, "daily-41"),
        Err(Failure::Manifest(_))
    ));
}

fn release(tag: &str, draft: bool, prerelease: bool) -> Release {
    Release {
        tag_name: tag.into(),
        draft,
        prerelease,
    }
}

/// A daily mailo takes the newest daily, a stable one the newest stable release, even when a
/// stable release is listed above the daily; a draft neither.
#[test]
fn each_channel_takes_its_newest_published_release() {
    let listed = [
        release("v0.2.0", true, false),
        release("v0.1.0", false, false),
        release("daily-9", false, true),
        release("daily-8", false, true),
    ];
    let tag = |channel| manifest::pick(&listed, channel).map(|r| r.tag_name.as_str());
    assert_eq!(tag(Channel::Daily), Some("daily-9"));
    assert_eq!(tag(Channel::Stable), Some("v0.1.0"));
    assert_eq!(manifest::pick(&listed[..1], Channel::Stable), None);
}

/// A download is the manifest's file only with its size and its hash, in either case of hex.
#[test]
fn a_file_matches_only_its_size_and_hash() {
    let file = File {
        name: "f".into(),
        sha256: "AB01".into(),
        size: 2,
    };
    assert!(manifest::matches(&file, 2, &[0xab, 0x01]));
    assert!(!manifest::matches(&file, 3, &[0xab, 0x01]));
    assert!(!manifest::matches(&file, 2, &[0xab, 0x02]));
}

/// The bundle is the `.app` around `Contents/MacOS/<exe>`, and only that, on macOS.
#[test]
fn the_bundle_is_the_app_around_the_executable() {
    let found = bundle_of(Path::new("/Applications/mailo.app/Contents/MacOS/mailo"));
    if cfg!(target_os = "macos") {
        assert_eq!(found.as_deref(), Some(Path::new("/Applications/mailo.app")));
    } else {
        assert_eq!(found, None);
    }
    assert_eq!(bundle_of(Path::new("/repo/target/release/mailo")), None);
    assert_eq!(
        bundle_of(Path::new("/x/mailo.app/Contents/Other/mailo")),
        None
    );
}

/// An update is put only where it can be: not on the disk image, not translocated, and not in
/// a folder that is not there to write in.
#[test]
fn an_app_on_the_disk_image_or_translocated_is_not_replaced() {
    let scratch = tempfile::tempdir().expect("scratch");
    assert!(super::replaceable(&scratch.path().join("mailo.app")));
    assert!(!super::replaceable(Path::new("/Volumes/mailo/mailo.app")));
    assert!(!super::replaceable(Path::new(
        "/private/var/folders/x/AppTranslocation/ABC/d/mailo.app"
    )));
    assert!(!super::replaceable(&scratch.path().join("gone/mailo.app")));
}

/// Tidying keeps the build named and removes every other download and staged app.
#[test]
fn tidying_keeps_only_the_build_named() {
    let scratch = tempfile::tempdir().expect("scratch");
    let dir = scratch.path();
    for name in ["staged-41", "staged-42"] {
        std::fs::create_dir_all(dir.join(name)).expect("a staged app");
    }
    for name in ["mailo-41.zip", "mailo-42.zip", "mailo-43.zip.part"] {
        std::fs::write(dir.join(name), b"zip").expect("a download");
    }
    super::install::tidy(dir, Some(42));
    let mut left: Vec<_> = std::fs::read_dir(dir)
        .expect("listed")
        .map(|e| e.expect("entry").file_name().into_string().expect("utf-8"))
        .collect();
    left.sort();
    assert_eq!(left, ["mailo-42.zip", "staged-42"]);
    super::install::tidy(dir, None);
    assert_eq!(std::fs::read_dir(dir).expect("listed").count(), 0);
}

#[cfg(target_os = "macos")]
mod macos {
    use super::super::Failure;
    use super::super::install::{IDENTIFIER, stage, swap};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// A mailo.app of `build` in `dir`, holding a copy of `true` as its program, signed ad hoc
    /// when `sign`.
    fn fake_app(dir: &Path, identifier: &str, build: u64, sign: bool) -> PathBuf {
        let app = dir.join("mailo.app");
        let macos = app.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).expect("the bundle");
        std::fs::copy("/usr/bin/true", macos.join("mailo")).expect("a program");
        std::fs::write(
            app.join("Contents/Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>mailo</string>
<key>CFBundleIdentifier</key><string>{identifier}</string>
<key>CFBundleVersion</key><string>{build}</string>
</dict></plist>"#
            ),
        )
        .expect("its Info.plist");
        if sign {
            let signed = Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(&app)
                .status()
                .expect("codesign runs");
            assert!(signed.success(), "signed ad hoc");
        }
        app
    }

    /// `app` zipped as the release workflow zips it.
    fn zipped(app: &Path, dir: &Path) -> PathBuf {
        let zip = dir.join("mailo.zip");
        let made = Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(app)
            .arg(&zip)
            .status()
            .expect("ditto runs");
        assert!(made.success(), "zipped");
        zip
    }

    /// A signed mailo.app of the build the manifest names is staged; one of another build,
    /// another app, or with no signature is not.
    #[test]
    fn only_a_signed_mailo_of_the_named_build_is_staged() {
        let scratch = tempfile::tempdir().expect("scratch");
        let made = |name: &str, identifier: &str, build: u64, sign: bool| {
            let dir = scratch.path().join(name);
            std::fs::create_dir_all(&dir).expect("its folder");
            zipped(&fake_app(&dir, identifier, build, sign), &dir)
        };
        let cache = scratch.path().join("cache");

        let good = made("good", IDENTIFIER, 42, true);
        assert!(stage(&good, &cache, 42).is_ok(), "staged once");
        let app = stage(&good, &cache, 42).expect("staged");
        assert!(app.join("Contents/MacOS/mailo").exists());

        assert!(matches!(stage(&good, &cache, 43), Err(Failure::Unpack(_))));
        let other = made("other", "com.example.other", 42, true);
        assert!(matches!(stage(&other, &cache, 42), Err(Failure::Unpack(_))));
        let unsigned = made("unsigned", IDENTIFIER, 42, false);
        assert!(matches!(
            stage(&unsigned, &cache, 42),
            Err(Failure::Unpack(_))
        ));
    }

    /// The staged app takes the running one's place and name, and nothing is left beside it.
    #[test]
    fn a_swap_puts_the_staged_app_where_the_running_one_was() {
        let scratch = tempfile::tempdir().expect("scratch");
        let installed = scratch.path().join("Applications");
        let staging = scratch.path().join("staging");
        std::fs::create_dir_all(&installed).expect("the folder");
        std::fs::create_dir_all(&staging).expect("the folder");
        let running = fake_app(&installed, IDENTIFIER, 41, false);
        let staged = fake_app(&staging, IDENTIFIER, 42, false);

        swap(&staged, &running).expect("swapped");

        let plist = std::fs::read_to_string(running.join("Contents/Info.plist")).expect("plist");
        assert!(
            plist.contains("<string>42</string>"),
            "the new build is in place"
        );
        let left: Vec<_> = std::fs::read_dir(&installed)
            .expect("listed")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(left, ["mailo.app"]);
    }

    /// A swap that cannot rename leaves the running app as it was.
    #[test]
    fn a_failed_swap_leaves_the_running_app() {
        let scratch = tempfile::tempdir().expect("scratch");
        let staging = scratch.path().join("staging");
        std::fs::create_dir_all(&staging).expect("the folder");
        let staged = fake_app(&staging, IDENTIFIER, 42, false);
        let missing = scratch.path().join("nowhere/mailo.app");
        assert!(matches!(swap(&staged, &missing), Err(Failure::Install(_))));
        assert!(staged.exists(), "the staged app stays for another try");
    }
}
