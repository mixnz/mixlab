//! `mix doctor`: the report, `--repair` and `--bundle`.

use std::path::Path;
use std::process::ExitCode;

use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    BundleReport, DiagnosticsBundle, DoctorRepair, DoctorReport, ElevationStatus, Error, ErrorCode,
    RepairReport, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::commands::elevation::confirmed;
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::render;

/// `mix doctor` — roadmap task **T47a**.
pub(crate) async fn doctor(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let report: DoctorReport = ask(
        &mut client,
        rpc::method::DAEMON_DOCTOR,
        encode(&serde_json::json!({})),
    )
    .await?;

    emit(&rendered(json, &report, || render::doctor::doctor(&report)))?;

    // **The exit code is the report and not the call.** A doctor that exits 0 because it managed to
    // ask cannot be used in a script, and the shell is where the second question gets asked.
    Ok(if report.has_a_problem() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// `mix doctor --bundle` — roadmap task **T93**.
///
/// **This one exits zero when the archive was written**, unlike bare `mix doctor`, whose exit code
/// is the report. The deliverable of this command is the file: a bundle is taken *because*
/// something is wrong, so a non-zero exit every time would make the ordinary success read as a
/// failure to the person watching their terminal and to whatever wrapped it. What the exit code
/// answers here is "did I get the archive"; the answer to "is this machine well" is inside it,
/// where the person asking will be looking.
pub(crate) async fn bundle(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    out: Option<&Path>,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let report: BundleReport = ask(
        &mut client,
        rpc::method::DAEMON_BUNDLE,
        encode(&DiagnosticsBundle::default()),
    )
    .await?;

    // **The copy is the client's and never the daemon's.** A destination on the method would be a
    // way for any local caller to have the daemon write a file anywhere that daemon can reach — so
    // the archive lands in the home, and moving it out is done by whoever asked, with their own
    // permissions.
    if let Some(destination) = out {
        std::fs::copy(&report.path, destination).map_err(|source| {
            Error::new(
                ErrorCode::Io,
                format!(
                    "the bundle was written to {} but could not be copied to {}: {source}",
                    report.path,
                    destination.display()
                ),
            )
        })?;
    }

    emit(&rendered(json, &report, || {
        render::doctor::bundle(&report, out)
    }))?;

    Ok(ExitCode::SUCCESS)
}

/// `mix doctor --repair` — roadmap task **T47b**.
///
/// **Two calls, and T64 is the reason.** The first repairs what needs no privilege and *queues* what
/// does; then the batch is read, shown and answered before the second raises the prompt — which is
/// the rule `mix elevation grant` obeys, over the same queue, for the same reason. `--yes` collapses
/// the two into one call by saying so on the command line, which is a person answering in advance
/// rather than a client skipping the question.
pub(crate) async fn self_repair(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    yes: bool,
    no_wait: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let repaired: RepairReport = ask(
        &mut client,
        rpc::method::DAEMON_DOCTOR_REPAIR,
        encode(&DoctorRepair { grant: yes }),
    )
    .await?;

    emit(&rendered(json, &repaired, || {
        render::doctor::repair(&repaired)
    }))?;

    // **The exit code is the report and not the call**, as `mix doctor`'s is: everything found was
    // either repaired or queued, or it was not.
    let outcome = match repaired.left_something_undone() {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    };

    let started = match repaired.granting {
        // `--yes`: the daemon raised it because the person said so before it ran.
        Some(started) => started,

        None => {
            let waiting: ElevationStatus =
                ask(&mut client, rpc::method::ELEVATION_STATUS, None).await?;

            // Nothing to grant is the ordinary end of a repair: either nothing needed an
            // administrator, or a machine that cannot prompt at all left the queue where it was.
            if waiting.pending.is_empty() {
                return Ok(outcome);
            }

            if !confirmed(&waiting, json)? {
                // Saying no is an answer and not a failure — the same rule as `mix elevation grant`.
                // Nothing was dropped, so the same command works when the person is ready.
                return Ok(outcome);
            }

            ask(&mut client, rpc::method::ELEVATION_GRANT, None).await?
        }
    };

    if no_wait {
        emit(&rendered(json, &started, || {
            render::job::job_status(&started)
        }))?;
        return Ok(outcome);
    }

    let finished = follow(&mut client, started, json).await?;
    emit(&rendered(json, &finished, || {
        render::job::job_status(&finished)
    }))?;

    // A grant that failed is a machine still holding what it held, whatever the repairs did.
    Ok(match render::job::job_succeeded(&finished) {
        true => outcome,
        false => ExitCode::FAILURE,
    })
}
