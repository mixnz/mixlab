//! The Windows update, in the order spec D4 gives — split by T188 into a download
//! ([`stage_payload`]) and an install ([`install_staged`]), each its own click — and what the next
//! start does when a window died half way through it (spec D8).

use std::path::Path;

use tauri::{AppHandle, Runtime};

use crate::error::AppError;
use crate::modules::mixengine::for_update;

use super::feed::Feed;
use super::lock::{self, Acquired};
use super::records::{InProgress, Records};
use super::{ready, stage, swap};

#[derive(Debug, PartialEq, Eq)]
pub enum Recovery {
    /// A daemon was stopped and nothing started it again: start it from what is on disk.
    StartAgain { finished: bool },
    /// Nothing was stopped: forget the record.
    Clear { finished: bool },
}

/// What to do about an interrupted update, given the version this process is — which is the
/// version on disk: a relaunched window is the new one, and a window never replaced is the old.
pub fn decide_recovery(record: &InProgress, on_disk: &str) -> Recovery {
    let finished = on_disk == record.to;
    if record.daemon_was_running {
        Recovery::StartAgain { finished }
    } else {
        Recovery::Clear { finished }
    }
}

/// Where the updater keeps its files: `<app data>/updates/`.
pub fn updates_dir<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, AppError> {
    Ok(crate::platform::app_data_dir(app)?.join("updates"))
}

pub fn records<R: Runtime>(app: &AppHandle<R>) -> Result<Records, AppError> {
    Ok(Records::new(updates_dir(app)?))
}

fn failed(message: impl std::fmt::Display) -> AppError {
    err!("error.updateFailed", message = message)
}

/// Spec D4 steps 2 to 4, then the `ready` marker (T188 D2). Touches nothing installed and takes no
/// lock: the files it writes are under `updates/<version>/`, which nothing but this updater reads.
pub async fn stage_payload<R: Runtime>(
    app: &AppHandle<R>,
    feed: &Feed,
    progress: impl Fn(u64, u64),
) -> Result<(), AppError> {
    let (os, arch) = super::host();
    let artifact = feed
        .artifact(os, arch)
        .ok_or_else(|| err!("error.updateNoBuild"))?;

    let staging = updates_dir(app)?.join(&feed.version);
    let archive = staging.join("payload.zip");
    // A marker from an earlier try is taken back first: from here until the new one is written,
    // what is on disk is not proved.
    let _ = std::fs::remove_file(staging.join(ready::FILE));
    stage::download(
        &reqwest::Client::new(),
        &artifact.url,
        &artifact.sha256,
        &archive,
        progress,
    )
    .await
    .map_err(failed)?;
    let unpacked = staging.join("unpacked");
    let _ = std::fs::remove_dir_all(&unpacked);
    stage::unpack(&archive, &artifact.provides, &unpacked).map_err(failed)?;
    stage::smoke_test(&unpacked, &artifact.provides, &feed.version).map_err(failed)?;

    ready::write(
        &staging,
        &ready::Ready {
            version: feed.version.clone(),
            sha256: artifact.sha256.clone(),
        },
    )
    .map_err(failed)
}

/// Spec D4 steps 1 and 5 to 8, from what [`stage_payload`] left. Returns only on failure: success
/// ends in a relaunch.
pub async fn install_staged<R: Runtime>(
    app: &AppHandle<R>,
    feed: &Feed,
    directory: &Path,
) -> Result<(), AppError> {
    let (os, arch) = super::host();
    let artifact = feed
        .artifact(os, arch)
        .ok_or_else(|| err!("error.updateNoBuild"))?;

    // 1. The lock, held until this function returns or the process ends.
    let _lock = match lock::acquire(directory).map_err(failed)? {
        Acquired::Held(held) => held,
        Acquired::Taken(pid) => {
            return Err(err!(
                "error.updateLocked",
                pid = pid.map_or_else(|| "?".to_owned(), |p| p.to_string())
            ))
        }
        Acquired::Unwritable => return Err(err!("error.updateUnwritable")),
    };

    // 2 to 4 happened in `stage_payload`. What it proved has to be this release's and still whole:
    // checked under the lock, and before anything is stopped.
    let staging = updates_dir(app)?.join(&feed.version);
    let unpacked = ready::staged(
        &staging,
        &feed.version,
        &artifact.sha256,
        &artifact.provides,
    )
    .map_err(|_| err!("error.updateNotDownloaded"))?;

    // What an earlier update renamed has to be gone before this one renames anything. The window's
    // own, still in use, is this window: that update put newer files on disk and never restarted
    // it. Restarting now finishes that update, and the new window offers this one again if it is
    // still newer. Anything else in use is another program, which a restart would not free.
    if let Err(old) = swap::clear_old(&artifact.provides, directory) {
        if old == swap::window_old(directory) {
            log::warn!("an earlier update never restarted this window; restarting it now");
            return crate::relaunch::restart(app);
        }
        return Err(err!("error.updateOldInUse", path = old.display()));
    }

    // 5. The record first, then the stop, so a window that dies here is finished by the next one.
    let records = records(app)?;
    let mut record = InProgress {
        from: env!("CARGO_PKG_VERSION").to_owned(),
        to: feed.version.clone(),
        directory: directory.to_path_buf(),
        daemon_was_running: for_update::running().await,
        services: Vec::new(),
    };
    records.set_in_progress(&record).map_err(failed)?;
    if record.daemon_was_running {
        match for_update::stop().await {
            Ok(services) => {
                record.services = services;
                records.set_in_progress(&record).map_err(failed)?;
            }
            Err(error) => {
                records.clear_in_progress();
                return Err(error);
            }
        }
    }

    // 6. The swap, and on failure everything back as it was, the daemon included.
    if let Err(error) = swap::swap(&unpacked, &artifact.provides, directory) {
        if record.daemon_was_running {
            let _ = for_update::start_again(directory, &record.services).await;
        }
        records.clear_in_progress();
        return Err(failed(error));
    }
    let _ = std::fs::remove_dir_all(&staging);

    // 7. The daemon back, from the new files. **A failure does not stop step 8.** From the swap on
    // this window runs from a renamed file, and staying open on it leaves every later try failing on
    // that file. The record stays, so the relaunched window starts MixEngine again (`recover`).
    if record.daemon_was_running {
        if let Err(error) = for_update::start_again(directory, &record.services).await {
            log::warn!("the update is in place, but MixEngine did not start again: {error}");
        }
    }

    // 8. The window, from the new files. The relaunched copy clears the record (`recover`).
    crate::relaunch::restart(app)
}

/// At every start: remove `.old` files, and finish what a window that died half way left behind.
pub async fn recover<R: Runtime>(app: &AppHandle<R>) {
    // Only where this updater swaps: elsewhere the window's directory is `/Applications` or
    // `/usr/bin`, and nothing there is ours to clean.
    if let super::placement::Placement::Swap { directory } = super::placement::read() {
        swap::discard_old(&directory);
    }
    let Ok(records) = records(app) else {
        return;
    };
    let Some(record) = records.in_progress() else {
        return;
    };

    match decide_recovery(&record, env!("CARGO_PKG_VERSION")) {
        Recovery::StartAgain { .. } => {
            if !for_update::running().await {
                if let Err(error) =
                    for_update::start_again(&record.directory, &record.services).await
                {
                    // The record stays; the next start tries again.
                    log::warn!("an interrupted update could not start MixEngine again: {error}");
                    return;
                }
            }
            records.clear_in_progress();
        }
        Recovery::Clear { .. } => records.clear_in_progress(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(running: bool) -> InProgress {
        InProgress {
            from: "0.0.9".into(),
            to: "0.0.10".into(),
            directory: "C:/MixEngine".into(),
            daemon_was_running: running,
            services: vec!["mariadb@main".into()],
        }
    }

    #[test]
    fn new_files_and_a_stopped_daemon_start_it_from_the_new_files() {
        assert_eq!(
            decide_recovery(&record(true), "0.0.10"),
            Recovery::StartAgain { finished: true }
        );
    }

    #[test]
    fn old_files_and_a_stopped_daemon_start_it_from_the_old_files() {
        assert_eq!(
            decide_recovery(&record(true), "0.0.9"),
            Recovery::StartAgain { finished: false }
        );
    }

    #[test]
    fn no_daemon_means_there_is_only_the_record_to_clear() {
        assert_eq!(
            decide_recovery(&record(false), "0.0.10"),
            Recovery::Clear { finished: true }
        );
        assert_eq!(
            decide_recovery(&record(false), "0.0.9"),
            Recovery::Clear { finished: false }
        );
    }
}
