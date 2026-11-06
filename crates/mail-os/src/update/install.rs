//! Putting a downloaded mailo where the running one is: unpacked and checked beside the
//! download ([`stage`]), swapped in as mailo quits ([`swap`]), and opened once this process is
//! gone ([`relaunch`]). macOS: the tools are the system's own (`ditto`, `plutil`, `codesign`).

use super::Failure;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The bundle identifier every mailo.app has (`packaging/macos/bundle.sh`).
pub const IDENTIFIER: &str = "io.github.PoHsuanLai.mailo";

/// The app in `zip`, unpacked into `dir/staged`, once it is a mailo.app of build `build` whose
/// signature holds. What an earlier stage left there goes first.
pub fn stage(zip: &Path, dir: &Path, build: u64) -> Result<PathBuf, Failure> {
    let unpack = |said: String| Failure::Unpack(said);
    let staged = dir.join("staged");
    if staged.exists() {
        std::fs::remove_dir_all(&staged).map_err(|e| unpack(e.to_string()))?;
    }
    run(Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(&staged))
    .map_err(unpack)?;
    let app = staged.join("mailo.app");
    let plist = app.join("Contents/Info.plist");
    let said = |key: &str| {
        run(Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(&plist))
        .map(|out| out.trim().to_owned())
        .map_err(unpack)
    };
    let identifier = said("CFBundleIdentifier")?;
    if identifier != IDENTIFIER {
        return Err(unpack(format!("it holds {identifier}, not mailo")));
    }
    let holds = said("CFBundleVersion")?;
    if holds != build.to_string() {
        return Err(unpack(format!("it holds build {holds}, not {build}")));
    }
    run(Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict"])
        .arg(&app))
    .map_err(unpack)?;
    Ok(app)
}

/// `staged` put where `bundle` is. A copy is made beside `bundle` first and renamed over it, so
/// a failure on the way leaves the running mailo where it was.
pub fn swap(staged: &Path, bundle: &Path) -> Result<(), Failure> {
    let install = |said: String| Failure::Install(said);
    let parent = bundle
        .parent()
        .ok_or_else(|| install("the app is in no folder".into()))?;
    let incoming = parent.join(".mailo-update.app");
    let outgoing = parent.join(".mailo-previous.app");
    for left in [&incoming, &outgoing] {
        if left.exists() {
            std::fs::remove_dir_all(left).map_err(|e| install(e.to_string()))?;
        }
    }
    run(Command::new("/usr/bin/ditto").arg(staged).arg(&incoming)).map_err(install)?;
    if let Err(e) = std::fs::rename(bundle, &outgoing) {
        let _ = std::fs::remove_dir_all(&incoming);
        return Err(install(e.to_string()));
    }
    if let Err(e) = std::fs::rename(&incoming, bundle) {
        let _ = std::fs::rename(&outgoing, bundle);
        let _ = std::fs::remove_dir_all(&incoming);
        return Err(install(e.to_string()));
    }
    let _ = std::fs::remove_dir_all(&outgoing);
    Ok(())
}

/// Opens `bundle` once this process has ended, from a shell left behind to wait for it: mailo
/// hands a second start to the one running, so the new one must start after.
pub fn relaunch(bundle: &Path) -> Result<(), Failure> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open "$2""#)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| Failure::Install(e.to_string()))
}

/// What `command` printed, when it succeeded; what it said otherwise.
fn run(command: &mut Command) -> Result<String, String> {
    let out = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}
