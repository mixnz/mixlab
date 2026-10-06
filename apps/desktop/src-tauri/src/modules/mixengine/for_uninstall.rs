//! What the shell's *Remove MixLab* asks this module for, and nothing more — roadmap task
//! **T182a**, spec D4.
//!
//! The removal is the shell's (`crate::uninstall`), and names no MixEngine crate. It reaches the
//! daemon here, over the public API, typed against `mixengine-proto`, and gets plain JSON back.
//! [`ensure_daemon`] is the one place the window starts a daemon nobody else started: a person who
//! asks for MixEngine's traces to be undone has asked for the daemon that knows them.

use std::time::{Duration, Instant};

use mixengine_platform::process::{started_at, StartTime};
use mixengine_proto::{
    DaemonStatus, JobOutcome, JobSummary, JobWait, Millis, UninstallQuery, UninstallReport,
};
use serde_json::{json, Value};

use crate::error::AppError;

use super::health::{self, Presence};
use super::rpc;

/// How long one `job.wait` asks for. The uninstall's prompt can sit on the screen far longer, so
/// the wait is asked again until the job ends.
const WAIT: Millis = Millis(10_000);
const POLL: Duration = Duration::from_millis(200);

/// The daemon this removal talks to: by pid and the moment it began, so the wait at the end is for
/// this process and not for whatever the system later hands the pid to (T182b, D8).
#[derive(Debug, Clone, Copy)]
pub struct Daemon {
    pid: u32,
    began: Option<StartTime>,
}

/// The running daemon, or one started for this.
pub async fn ensure_daemon() -> Result<Daemon, AppError> {
    match health::presence().await.presence {
        Presence::Running => {}
        Presence::NotRunning => {
            health::start_daemon(None).await?;
        }
        Presence::NotAnswering | Presence::NotInstalled => {
            return Err(err!("error.uninstallNoDaemon"));
        }
    }

    let status: DaemonStatus = rpc::call("daemon.status", json!({})).await?;
    Ok(Daemon {
        pid: status.pid,
        began: started_at(status.pid).ok().flatten(),
    })
}

fn query(keep_home: bool, keep_relocated: bool, grant: bool) -> UninstallQuery {
    UninstallQuery {
        keep_home,
        keep_relocated,
        grant,
        skip_holders: false,
        package: true,
    }
}

/// `daemon.uninstall_plan`, with the package asked for: what the dialog shows before anything changes.
pub async fn plan(keep_home: bool, keep_relocated: bool) -> Result<Value, AppError> {
    let report: UninstallReport = rpc::call(
        "daemon.uninstall_plan",
        json!(query(keep_home, keep_relocated, false)),
    )
    .await?;
    serde_json::to_value(report).map_err(|e| err!("error.uninstallFailed", message = e))
}

/// `daemon.uninstall`, followed to its end: the report as the rows settled.
pub async fn run(keep_home: bool, keep_relocated: bool) -> Result<Value, AppError> {
    let mut job: JobSummary = rpc::call(
        "daemon.uninstall",
        json!(query(keep_home, keep_relocated, true)),
    )
    .await?;

    while !job.state.is_finished() {
        job = rpc::call(
            "job.wait",
            json!(JobWait {
                job: job.id,
                timeout: WAIT,
            }),
        )
        .await?;
    }

    match job.outcome {
        Some(JobOutcome::Succeeded { result }) => Ok(result),
        Some(JobOutcome::Failed { error }) => {
            Err(err!("error.uninstallFailed", message = error.message))
        }
        _ => Err(err!("error.uninstallFailed", message = job.message)),
    }
}

/// Wait up to `within` for the daemon to have ended: by pid and start time when it was read, by
/// the endpoint going quiet when it was not.
pub async fn wait_gone(daemon: &Daemon, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    loop {
        let ended = match daemon.began {
            Some(began) => started_at(daemon.pid).ok().flatten() != Some(began),
            None => health::presence().await.presence != Presence::Running,
        };
        if ended {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL).await;
    }
}
