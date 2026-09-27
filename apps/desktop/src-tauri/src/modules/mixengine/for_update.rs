//! What MixLab's updater asks this module for, and nothing more — T187, spec D1.
//!
//! The updater is MixLab's and names no MixEngine crate (ADR 0056 rule 3). When it has to stop a
//! daemon that is running and start it again after the swap, it asks here, over the public API,
//! exactly as the rest of this module talks to the daemon. With no daemon, `running` answers
//! `false` and nothing else here is called: nothing in this file ever starts a daemon that was not
//! running before an update.
//!
//! No unit tests: every function is a passthrough to the daemon, like `commands.rs`' passthroughs.
//! They are exercised by the spec's manual paths 2 and 3.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::error::AppError;

use super::health::{self, Presence};
use super::rpc;

/// How long a stopped daemon is given to let go of its endpoint.
const GONE_TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(200);

/// The directory holding the `mixengined` the platform finds: where an installer put it, which is
/// not the window's directory after a `.pkg` or a `.deb` (`/usr/local/bin`, `/usr/bin`).
pub fn daemon_directory() -> Option<std::path::PathBuf> {
    mixengine_platform::install::program_path("mixengined")
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Whether a daemon answers on this machine's endpoint. Never starts one.
pub async fn running() -> bool {
    health::presence().await.presence == Presence::Running
}

/// How many services an install would stop and start again, for the update panel's sentence
/// (T188). `None` when no daemon answers, or it would not say; never starts one.
pub async fn running_services() -> Option<u32> {
    if !running().await {
        return None;
    }
    let answer: Value = rpc::call("service.list", json!({})).await.ok()?;
    let count = answer["services"]
        .as_array()?
        .iter()
        .filter(|service| {
            matches!(
                service["state"].as_str(),
                Some("running" | "degraded" | "starting" | "restarting")
            )
        })
        .count();
    Some(u32::try_from(count).unwrap_or(u32::MAX))
}

/// `daemon.shutdown`, then wait until it has gone. Answers the services it stopped.
pub async fn stop() -> Result<Vec<String>, AppError> {
    let answer: Value = rpc::call("daemon.shutdown", json!({})).await?;
    let stopped = answer["services"]["reached"]
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let deadline = Instant::now() + GONE_TIMEOUT;
    while running().await {
        if Instant::now() > deadline {
            return Err(err!("error.updateDaemonWouldNotStop"));
        }
        tokio::time::sleep(POLL).await;
    }
    Ok(stopped)
}

/// Start the `mixengined` in `directory`, wait until it answers, then start each of `services`.
///
/// `--detach` returns only once the daemon answers on its endpoint (`health::start_daemon` says
/// the same), so there is no wait loop here.
pub async fn start_again(directory: &Path, services: &[String]) -> Result<(), AppError> {
    let program = directory.join(format!("mixengined{}", std::env::consts::EXE_SUFFIX));
    let output = tauri::async_runtime::spawn_blocking(move || {
        let mut command = std::process::Command::new(program);
        command.arg("--detach");
        crate::platform::hide_console(&mut command).output()
    })
    .await
    .map_err(|e| err!("error.mixengineStartFailed", message = e))?
    .map_err(|e| err!("error.mixengineStartFailed", message = e))?;

    if !output.status.success() {
        return Err(err!(
            "error.mixengineStartFailed",
            message = String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    for service in services {
        rpc::call::<Value>("service.start", json!({ "service": service })).await?;
    }
    Ok(())
}
