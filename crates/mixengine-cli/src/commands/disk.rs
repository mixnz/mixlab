//! `mix disk` and `mix cleanup`.

use std::io::Write as _;
use std::process::ExitCode;

use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    CleanupQuery, CleanupReport, DiskCategory, DiskUsage, DiskUsageQuery, Error, ErrorCode,
    JobOutcome, JobSummary, Reclaim, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::confirm::unanswered;
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::{confirm, render};

/// `mix disk` — roadmap task **T96**.
///
/// **`refresh: true`, always.** The daemon keeps a reading for a minute so that a dashboard
/// re-reading on every event does not walk `runtimes/` each time; somebody who typed a command is
/// asking about now, and a stale figure under a command they just ran would read as a command that
/// did nothing.
pub(crate) async fn disk(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let usage: DiskUsage = ask(
        &mut client,
        rpc::method::DAEMON_DISK_USAGE,
        encode(&DiskUsageQuery { refresh: true }),
    )
    .await?;

    emit(&rendered(json, &usage, || render::disk_usage(&usage)))?;

    Ok(ExitCode::SUCCESS)
}

/// `mix cleanup` — roadmap task **T96**.
///
/// **What is about to go is read and shown first**, on `mix uninstall`'s rule: `daemon.disk_usage`
/// is the plan half of this pair, and a command that deleted before anybody had seen the number
/// would be the *"measured and deleted in one breath"* T96 exists to refuse.
pub(crate) async fn cleanup(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    keep_logs: bool,
    keep_cache: bool,
    yes: bool,
    no_wait: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    if !yes {
        let usage: DiskUsage = ask(
            &mut client,
            rpc::method::DAEMON_DISK_USAGE,
            encode(&DiskUsageQuery { refresh: true }),
        )
        .await?;

        // Printed and not held back under `--json`, because the question below is refused there
        // anyway: what a person is about to allow is what they are shown.
        emit(&render::disk_usage(&usage))?;

        if !agreed_to_cleanup(&usage, keep_logs, keep_cache, json)? {
            // Saying no is an answer and not a failure — `mix uninstall`'s rule. Nothing went, so
            // the same command works when the person is ready.
            return Ok(ExitCode::SUCCESS);
        }
    }

    let started: JobSummary = ask(
        &mut client,
        rpc::method::DAEMON_CLEANUP,
        encode(&CleanupQuery {
            keep_logs,
            keep_cache,
        }),
    )
    .await?;

    if no_wait {
        emit(&rendered(json, &started, || {
            render::job::job_status(&started)
        }))?;
        return Ok(ExitCode::SUCCESS);
    }

    let finished = follow(&mut client, started, json).await?;

    let Some(JobOutcome::Succeeded { result }) = finished.outcome.clone() else {
        emit(&rendered(json, &finished, || {
            render::job::job_status(&finished)
        }))?;
        return Ok(ExitCode::FAILURE);
    };

    let report: CleanupReport = serde_json::from_value(result).map_err(|error| {
        Error::new(
            ErrorCode::Internal,
            format!(
                "mix {} cannot read the cleanup report: {error}",
                env!("CARGO_PKG_VERSION")
            ),
        )
    })?;

    emit(&rendered(json, &report, || render::cleanup_report(&report)))?;

    Ok(match report.left_behind() {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    })
}

/// Ask, once, in front of the table that was just printed — roadmap task **T96**.
///
/// **Nothing to take is a yes.** A command that asked *"remove nothing?"* would be one people learn
/// to answer without reading, which is the habit the question exists to prevent.
///
/// **`--json` never asks**, on
/// [`agreed_to_uninstall`](crate::commands::uninstall::agreed_to_uninstall)'s rule and for its
/// reason.
fn agreed_to_cleanup(
    usage: &DiskUsage,
    keep_logs: bool,
    keep_cache: bool,
    json: bool,
) -> Result<bool, Error> {
    let asked_for: u64 = usage
        .categories
        .iter()
        .filter(|category| match category.id {
            DiskCategory::Logs => !keep_logs,
            DiskCategory::Cache => !keep_cache,
            _ => false,
        })
        .map(|category| match category.reclaim {
            Reclaim::ByCleanup { bytes, .. } => bytes,
            _ => 0,
        })
        .sum();

    if asked_for == 0 {
        return Ok(true);
    }

    if json {
        return Err(unanswered());
    }

    match confirm::ask("\nremove them? nothing else is touched. [y/N] ") {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            // On stderr, beside the question it answers. Stdout carried the table above.
            let _ = writeln!(std::io::stderr(), "nothing was removed");

            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}
