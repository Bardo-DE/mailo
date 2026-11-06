//! mailo updating itself ([`mail_os::update`]): a newer release on the channel Settings names is
//! looked for a minute after the window opens and every six hours after, downloaded and checked
//! in the background, and said at the sidebar's foot once it is ready. It is put in place as
//! mailo quits ([`at_quit`]): at once by Restart to Update, or at whatever quit comes next.
//!
//! Only a mailo the release workflow built, run from its `.app`, looks
//! ([`mail_os::update::Running::here`]): a test's window, and a mailo built by hand, never do.

use dioxus::prelude::*;
use ds::prelude::*;
use ds::style::tokens::control_size::ControlSize;
use mail_os::update::{Channel, Failure, Running, fetch, install, manifest};
use mail_ui_kit::press::on_primary;
use mail_ui_model::copy;
use mail_ui_model::settings::{MailSettings, UpdateChannel};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

/// How long after the window opens the first look waits, so it is not in the launch's way.
const FIRST: Duration = Duration::from_secs(60);

/// How often it looks again while nothing is ready.
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// The staged mailo, waiting for the quit to put it in place.
struct Pending {
    staged: PathBuf,
    bundle: PathBuf,
    /// Whether the quit opens it after: Restart to Update was pressed.
    relaunch: bool,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

/// A newer mailo is staged: its version, for the banner.
#[derive(Clone, Copy)]
pub(super) struct Ready(Signal<Option<String>>);

/// Look for a newer mailo while the window is open, and say when one is ready.
pub(super) fn use_updates() {
    let settings = use_hook(try_consume_context::<Signal<MailSettings>>);
    let mut ready = use_context_provider(|| Ready(Signal::new(None))).0;
    use_future(move || async move {
        let Some(running) = Running::here() else {
            return;
        };
        let Some(dir) = mail_client::config::cache_dir().map(|dir| dir.join("update")) else {
            return;
        };
        let net = mail_ui_kit::live::client().net().runtime();
        tokio::time::sleep(FIRST).await;
        loop {
            let channel = match settings.map(|settings| settings.peek().updates.channel) {
                Some(UpdateChannel::Daily) => Channel::Daily,
                Some(UpdateChannel::Stable) => Channel::Stable,
                None => Channel::built(),
            };
            let fetched = net.spawn(staged(running.build, channel, dir.clone())).await;
            match fetched {
                Ok(Ok(Some((staged, version)))) => {
                    log::info!("mailo {version} is staged at {}", staged.display());
                    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pending {
                        staged,
                        bundle: running.bundle.clone(),
                        relaunch: false,
                    });
                    ready.set(Some(version));
                    return;
                }
                Ok(Ok(None)) => {}
                Ok(Err(why)) => log::warn!("looking for a newer mailo: {why}"),
                Err(why) => log::warn!("looking for a newer mailo: {why}"),
            }
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// The newest release on `channel`, when it is above `build`: downloaded into `dir`, unpacked
/// and checked; its staged app and version.
async fn staged(
    build: u64,
    channel: Channel,
    dir: PathBuf,
) -> Result<Option<(PathBuf, String)>, Failure> {
    let http = mail_client::update::client().map_err(|e| Failure::Network(e.to_string()))?;
    let key = manifest::release_key();
    let Some(offer) = fetch::look(&http, &fetch::Source::github(), channel, build, &key).await?
    else {
        return Ok(None);
    };
    let zip = fetch::download(&http, &offer, &dir).await?;
    let new = offer.manifest.build;
    let app = tokio::task::spawn_blocking(move || install::stage(&zip, &dir, new))
        .await
        .map_err(|e| Failure::Unpack(e.to_string()))??;
    Ok(Some((app, offer.manifest.version)))
}

/// Put a staged mailo in place, as this process ends, and open it when Restart to Update asked
/// for that. A swap that fails leaves the running mailo, and the restart opens it again.
pub(super) fn at_quit() {
    let Some(pending) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    if let Err(why) = install::swap(&pending.staged, &pending.bundle) {
        log::warn!("putting the new mailo in place: {why}");
    }
    if pending.relaunch
        && let Err(why) = install::relaunch(&pending.bundle)
    {
        log::warn!("opening mailo again: {why}");
    }
}

/// The sidebar's banner while a newer mailo is ready: Restart to Update quits, and the quit puts
/// it in place and opens it.
#[component]
pub(super) fn UpdateBanner() -> Element {
    let Some(Ready(ready)) = try_use_context::<Ready>() else {
        return rsx! {};
    };
    let Some(version) = ready() else {
        return rsx! {};
    };
    let restart = move || {
        if let Some(pending) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            pending.relaunch = true;
        }
        if let Some(app) = try_consume_context::<ds_blitz::AppHandle>() {
            let _ = app.quit();
        }
    };
    rsx! {
        div { class: "update-banner",
            InlineBanner {
                severity: Severity::Info,
                text: copy::status::update_ready(&version),
                actions: rsx! {
                    Button {
                        label: copy::controls::RESTART_TO_UPDATE,
                        size: ControlSize::Small,
                        onclick: on_primary(restart),
                    }
                },
            }
        }
    }
}
