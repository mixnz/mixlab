//! The commands Settings → Updates and the update panel call — T187 spec D9, T188 spec D2.
//!
//! No unit tests of their own: each one reads the placement and the records, or hands work to a
//! module that has its tests (`feed`, `install`, `handover`). What the pane draws from their answer
//! is `src/shell/update/view.ts`, tested in vitest.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::error::AppError;

use super::feed::{self, Cache, Feed};
use super::placement::{self, Placement};
use super::records::Decision;
use super::{handover, install};

/// What this window knows between two calls: the feed last read, when, and whether an install is
/// running now.
#[derive(Default)]
pub struct UpdaterState {
    feed: Mutex<Option<Feed>>,
    checked_at: Mutex<Option<String>>,
    installing: AtomicBool,
    /// A download is running; a second one would write the same `.partial` file (T188).
    downloading: AtomicBool,
    /// Wakes the running download's `select!` so it stops, keeping the partial file.
    cancel: tokio::sync::Notify,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedSummary {
    version: String,
    notes: String,
    notes_url: Option<String>,
    /// The download's size for this kind of install, when the release has one for this machine.
    size: Option<u64>,
    has_build: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    current: String,
    placement: Placement,
    feed: Option<FeedSummary>,
    skipped: Option<String>,
    automatic: bool,
    installing: bool,
    /// The offered version when its download is on disk and proved (T188 D2), so a window starting
    /// after a finished download opens on *Ready*.
    downloaded: Option<String>,
    checked_at: Option<String>,
    /// Why a check a person asked for failed. Always `None` for the automatic one (spec D6).
    failure: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Progress {
    received: u64,
    total: u64,
}

fn summary(feed: &Feed, placement: &Placement) -> FeedSummary {
    let (os, arch) = super::host();
    let size = match placement {
        Placement::Swap { .. } => feed.artifact(os, arch).map(|a| a.size),
        Placement::Installer { installer, .. } => {
            feed.installer(os, arch, installer).map(|i| i.size)
        }
        Placement::Development | Placement::Elsewhere => None,
    };
    FeedSummary {
        version: feed.version.clone(),
        notes: feed.notes.clone(),
        notes_url: feed.notes_url.clone(),
        has_build: size.is_some(),
        size,
    }
}

/// The SHA-256 of what this kind of install downloads for `feed`, if the release has one here.
fn expected_sha256<'a>(feed: &'a Feed, placement: &Placement) -> Option<&'a str> {
    let (os, arch) = super::host();
    match placement {
        Placement::Swap { .. } => feed.artifact(os, arch).map(|a| a.sha256.as_str()),
        Placement::Installer { installer, .. } => feed
            .installer(os, arch, installer)
            .map(|i| i.sha256.as_str()),
        Placement::Development | Placement::Elsewhere => None,
    }
}

/// The offered version, when `ready` proves its download for this kind of install.
fn downloaded(app: &AppHandle, feed: &Feed, placement: &Placement) -> Option<String> {
    let sha256 = expected_sha256(feed, placement)?;
    let staging = install::updates_dir(app).ok()?.join(&feed.version);
    super::ready::read_for(&staging, &feed.version, sha256).map(|ready| ready.version)
}

fn status(
    app: &AppHandle,
    state: &UpdaterState,
    failure: Option<String>,
) -> Result<UpdateStatus, AppError> {
    let placement = placement::read();
    let decision = install::records(app)?.decision();
    let feed = state.feed.lock().unwrap().clone();
    let checked_at = state.checked_at.lock().unwrap().clone();
    let downloaded = feed.as_ref().and_then(|f| downloaded(app, f, &placement));
    Ok(UpdateStatus {
        current: env!("CARGO_PKG_VERSION").to_owned(),
        feed: feed.as_ref().map(|f| summary(f, &placement)),
        placement,
        skipped: decision.skipped,
        automatic: decision.automatic,
        installing: state.installing.load(Ordering::SeqCst),
        downloaded,
        checked_at,
        failure,
    })
}

fn cache(app: &AppHandle) -> Result<Cache, AppError> {
    Ok(Cache::new(install::updates_dir(app)?))
}

/// What the pane shows on opening: the last verified feed, from the cache, with no request made.
#[tauri::command]
pub fn update_status(
    app: AppHandle,
    state: State<'_, UpdaterState>,
) -> Result<UpdateStatus, AppError> {
    let empty = state.feed.lock().unwrap().is_none();
    if empty {
        let cached = cache(&app)?.load(super::PUBLIC_KEY);
        *state.feed.lock().unwrap() = cached;
    }
    status(&app, &state, None)
}

/// Read the feed. `force` is *Check now*; without it the check is the automatic one, which does
/// nothing while the switch is off and never reports a failure.
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    force: bool,
) -> Result<UpdateStatus, AppError> {
    if !force && !install::records(&app)?.decision().automatic {
        return status(&app, &state, None);
    }
    let cache = cache(&app)?;
    let checked = feed::refresh(
        &reqwest::Client::new(),
        super::FEED_URL,
        super::PUBLIC_KEY,
        &cache,
    )
    .await;
    *state.feed.lock().unwrap() = checked.feed;
    *state.checked_at.lock().unwrap() = Some(chrono::Utc::now().to_rfc3339());
    status(&app, &state, if force { checked.failure } else { None })
}

#[tauri::command]
pub fn update_set_automatic(app: AppHandle, on: bool) -> Result<(), AppError> {
    let records = install::records(&app)?;
    let decision = Decision {
        automatic: on,
        ..records.decision()
    };
    records
        .set_decision(&decision)
        .map_err(|e| err!("error.updateFailed", message = e))
}

#[tauri::command]
pub fn update_skip(app: AppHandle, version: String) -> Result<(), AppError> {
    let records = install::records(&app)?;
    let decision = Decision {
        skipped: Some(version),
        ..records.decision()
    };
    records
        .set_decision(&decision)
        .map_err(|e| err!("error.updateFailed", message = e))
}

fn offered(state: &UpdaterState) -> Result<Feed, AppError> {
    state
        .feed
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| err!("error.updateNoBuild"))
}

/// Clears the flag however the download ends, a cancel and a panic included.
struct Downloading<'a>(&'a AtomicBool);

impl Drop for Downloading<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Download and prove the offered release (T188 D2): the payload on Windows, the installer on macOS
/// and Linux, ending in a `ready` marker. Installs nothing. `update_cancel_download` stops it with
/// `error.updateCancelled` and keeps the partial file for the next try.
#[tauri::command]
pub async fn update_download(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    on_progress: Channel<Progress>,
) -> Result<(), AppError> {
    let feed = offered(&state)?;
    if state.downloading.swap(true, Ordering::SeqCst) {
        return Err(err!("error.updateDownloading"));
    }
    let _downloading = Downloading(&state.downloading);

    let progress = |received, total| {
        let _ = on_progress.send(Progress { received, total });
    };
    let work = async {
        match placement::read() {
            Placement::Swap { .. } => install::stage_payload(&app, &feed, progress).await,
            Placement::Installer { installer, .. } => {
                handover::download_installer(&app, &feed, &installer, progress).await
            }
            Placement::Development | Placement::Elsewhere => Err(err!("error.updateUnwritable")),
        }
    };
    // `notified()` is registered when it is created, so a cancel that lands before the first poll
    // still reaches it.
    let cancelled = state.cancel.notified();
    tokio::select! {
        result = work => result,
        () = cancelled => Err(err!("error.updateCancelled")),
    }
}

#[tauri::command]
pub fn update_cancel_download(state: State<'_, UpdaterState>) {
    // `notify_waiters` and not `notify_one`: a cancel pressed when nothing is downloading must not
    // be stored and stop the next download the moment it starts.
    state.cancel.notify_waiters();
}

/// Windows: swap in what `update_download` staged, and relaunch. Returns only on failure.
#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    state: State<'_, UpdaterState>,
) -> Result<(), AppError> {
    let Placement::Swap { directory } = placement::read() else {
        return Err(err!("error.updateUnwritable"));
    };
    let feed = offered(&state)?;
    state.installing.store(true, Ordering::SeqCst);
    let result = install::install_staged(&app, &feed, &directory).await;
    state.installing.store(false, Ordering::SeqCst);
    result
}

/// macOS and Linux: open the installer `update_download` fetched. Stops nothing.
#[tauri::command]
pub fn update_open_installer(
    app: AppHandle,
    state: State<'_, UpdaterState>,
) -> Result<handover::HandedOver, AppError> {
    let Placement::Installer { installer, .. } = placement::read() else {
        return Err(err!("error.updateUnwritable"));
    };
    handover::open_installer(&app, &offered(&state)?, &installer)
}

/// How many services an install would restart, or `None` with no daemon running (T188 D1).
#[tauri::command]
pub async fn update_restarts() -> Option<u32> {
    crate::modules::mixengine::for_update::running_services().await
}

#[tauri::command]
pub fn update_version_on_disk() -> Option<String> {
    handover::version_on_disk(&placement::read())
}

#[tauri::command]
pub async fn update_finish(app: AppHandle) -> Result<(), AppError> {
    handover::finish(&app).await
}
