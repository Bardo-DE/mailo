//! mailo updating itself ([`mail_os::update`]): a newer release on the channel Settings names is
//! looked for a minute after the window first opens and every six hours after, downloaded and
//! checked in the background, and said at the sidebar's foot once it is ready. It is put in place
//! as mailo quits ([`at_quit`]): at once by Restart to Update, or at whatever quit comes next.
//!
//! The looking is the process's, not a window's: one worker on the network runtime, started by
//! the first window, which outlives it. Each main window says what the worker staged, and tells
//! it the channel its settings name. A staged mailo of a channel no longer chosen is not put in
//! place, and the worker looks again on the new one.
//!
//! Only a mailo the release workflow built, run from an `.app` it can replace, looks
//! ([`mail_os::update::Running::here`]): a test's window, and a mailo built by hand, never do.

use dioxus::prelude::*;
use ds::prelude::*;
use ds::style::tokens::control_size::ControlSize;
use mail_os::update::{Channel, Failure, Running, fetch, install, manifest};
use mail_ui_kit::press::on_primary;
use mail_ui_model::copy;
use mail_ui_model::settings::{MailSettings, UpdateChannel};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How long after the first window opens the first look waits, so it is not in the launch's way.
const FIRST: Duration = Duration::from_secs(60);

/// How often the worker looks again.
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// How often a window asks what is staged, and tells the channel; and how often the worker, once
/// something is staged, checks the channel still is its.
const GLANCE: Duration = Duration::from_secs(30);

/// A staged mailo, waiting for the quit to put it in place.
struct Pending {
    staged: PathBuf,
    build: u64,
    version: String,
    channel: Channel,
    /// Whether the quit opens it after: Restart to Update was pressed.
    relaunch: bool,
}

/// What the process's updater holds: the mailo running, where downloads go, the channel the
/// settings name, and what is staged.
struct Updater {
    running: Running,
    dir: PathBuf,
    channel: Option<Channel>,
    pending: Option<Pending>,
}

static UPDATER: Mutex<Option<Updater>> = Mutex::new(None);
static STARTED: AtomicBool = AtomicBool::new(false);

fn updater() -> std::sync::MutexGuard<'static, Option<Updater>> {
    UPDATER.lock().unwrap_or_else(|e| e.into_inner())
}

fn channel_of(channel: UpdateChannel) -> Channel {
    match channel {
        UpdateChannel::Daily => Channel::Daily,
        UpdateChannel::Stable => Channel::Stable,
    }
}

/// The version staged for the channel chosen now, for the banner.
#[derive(Clone, Copy)]
pub(super) struct Ready(Signal<Option<String>>);

/// Start the process's updater once, and keep this window's banner and channel in step with it.
pub(super) fn use_updates() {
    let settings = use_hook(try_consume_context::<Signal<MailSettings>>);
    let mut ready = use_context_provider(|| Ready(Signal::new(None))).0;
    use_future(move || async move {
        if !STARTED.swap(true, Ordering::SeqCst) {
            start();
        }
        loop {
            let chosen = channel_of(
                settings
                    .map(|settings| settings.peek().updates.channel)
                    .unwrap_or_default(),
            );
            let staged = updater().as_mut().and_then(|updater| {
                updater.channel = Some(chosen);
                updater
                    .pending
                    .as_ref()
                    .filter(|pending| pending.channel == chosen)
                    .map(|pending| pending.version.clone())
            });
            if *ready.peek() != staged {
                ready.set(staged);
            }
            tokio::time::sleep(GLANCE).await;
        }
    });
}

/// The worker, on the network runtime, when this process is a mailo that may update.
fn start() {
    let Some(running) = Running::here() else {
        return;
    };
    let Some(dir) = mail_client::config::cache_dir().map(|dir| dir.join("update")) else {
        return;
    };
    *updater() = Some(Updater {
        running,
        dir,
        channel: None,
        pending: None,
    });
    mail_ui_kit::live::client().net().runtime().spawn(work());
}

/// Look, download and stage on the channel the windows name, until something is staged for it;
/// look again when the channel changes.
async fn work() {
    tokio::time::sleep(FIRST).await;
    loop {
        let (build, dir, channel, staged_for) = {
            let guard = updater();
            let Some(updater) = guard.as_ref() else {
                return;
            };
            (
                updater.running.build,
                updater.dir.clone(),
                updater.channel,
                updater.pending.as_ref().map(|pending| pending.channel),
            )
        };
        let Some(channel) = channel.filter(|&channel| staged_for != Some(channel)) else {
            tokio::time::sleep(GLANCE).await;
            continue;
        };
        match staged(build, channel, &dir).await {
            Ok(Some(pending)) => {
                log::info!(
                    "mailo {} is staged at {}",
                    pending.version,
                    pending.staged.display()
                );
                // What another channel staged goes once this takes its place.
                let kept = pending.build;
                if let Some(updater) = updater().as_mut() {
                    updater.pending = Some(pending);
                }
                install::tidy(&dir, Some(kept));
                continue;
            }
            Ok(None) => {}
            Err(why) => log::warn!("looking for a newer mailo: {why}"),
        }
        tokio::time::sleep(EVERY).await;
    }
}

/// The newest release on `channel`, when it is above `build`: downloaded into `dir`, unpacked
/// and checked.
async fn staged(build: u64, channel: Channel, dir: &Path) -> Result<Option<Pending>, Failure> {
    let http = mail_client::update::client().map_err(|e| Failure::Network(e.to_string()))?;
    let key = manifest::release_key();
    let Some(offer) = fetch::look(&http, &fetch::Source::github(), channel, build, &key).await?
    else {
        return Ok(None);
    };
    let zip = fetch::download(&http, &offer, dir).await?;
    let new = offer.manifest.build;
    let into = dir.to_path_buf();
    let app = tokio::task::spawn_blocking(move || install::stage(&zip, &into, new))
        .await
        .map_err(|e| Failure::Unpack(e.to_string()))??;
    Ok(Some(Pending {
        staged: app,
        build: new,
        version: offer.manifest.version,
        channel,
        relaunch: false,
    }))
}

/// Put a staged mailo in place, as this process ends, when it is of the channel chosen, and open
/// it when Restart to Update asked for that. A swap that fails leaves the running mailo, and the
/// restart opens it again; one that succeeds leaves nothing downloaded behind.
pub(super) fn at_quit() {
    let Some(updater) = updater().take() else {
        return;
    };
    let Some(pending) = updater.pending else {
        return;
    };
    if updater
        .channel
        .is_some_and(|chosen| chosen != pending.channel)
    {
        return;
    }
    match install::swap(&pending.staged, &updater.running.bundle) {
        Ok(()) => install::tidy(&updater.dir, None),
        Err(why) => log::warn!("putting the new mailo in place: {why}"),
    }
    if pending.relaunch
        && let Err(why) = install::relaunch(&updater.running.bundle)
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
        if let Some(pending) = updater()
            .as_mut()
            .and_then(|updater| updater.pending.as_mut())
        {
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
