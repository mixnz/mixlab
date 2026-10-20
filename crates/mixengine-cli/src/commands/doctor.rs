//! `mix doctor`: the report, `--repair` and `--bundle`.

use std::path::Path;
use std::process::ExitCode;

use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    BundleReport, DiagnosticsBundle, DoctorRepair, DoctorReport, ElevationStatus, Error, ErrorCode,
    RepairReport, rpc,
};

use crate::autostart::Autostart;
use crate::by_hand::Route;
use crate::client::{Client, ask, encode};
use crate::commands::elevation::{
    confirmed, route_for, say_where_the_command_is, see_through, start_grant,
};
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

    // ADR 0065. With `--yes` the daemon raises the prompt inside the repair, which is a dialog; from
    // a terminal over SSH it is this process that asks instead, once the repair has queued the rest.
    // A machine where neither can ask is left to the daemon, as before: the rows wait.
    let early = match yes {
        true => {
            let status: ElevationStatus =
                ask(&mut client, rpc::method::ELEVATION_STATUS, None).await?;
            route_for(&status, json, "mix doctor --repair").unwrap_or(Route::Dialog)
        }
        false => Route::Dialog,
    };

    let repaired: RepairReport = ask(
        &mut client,
        rpc::method::DAEMON_DOCTOR_REPAIR,
        encode(&DoctorRepair {
            grant: the_daemon_raises_it(yes, early),
        }),
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

    let (started, route) = match repaired.granting {
        // `--yes`: the daemon raised it because the person said so before it ran.
        Some(started) => (started, Route::Dialog),

        None => {
            let waiting: ElevationStatus =
                ask(&mut client, rpc::method::ELEVATION_STATUS, None).await?;

            // Nothing to grant is the ordinary end of a repair: either nothing needed an
            // administrator, or a machine that cannot prompt at all left the queue where it was.
            if waiting.pending.is_empty() {
                return Ok(outcome);
            }

            // Settled before the question, as `mix elevation grant` settles it.
            let route = match yes {
                true => early,
                false => route_for(&waiting, json, "mix doctor --repair")?,
            };

            if !yes && !confirmed(&waiting, json, route)? {
                // Saying no is an answer and not a failure — the same rule as `mix elevation grant`.
                // Nothing was dropped, so the same command works when the person is ready.
                return Ok(outcome);
            }

            (start_grant(&mut client, route).await?, route)
        }
    };

    if no_wait {
        emit(&rendered(json, &started, || {
            render::job::job_status(&started)
        }))?;
        say_where_the_command_is(route);
        return Ok(outcome);
    }

    let finished = see_through(&mut client, route, started, json).await?;
    emit(&rendered(json, &finished, || {
        render::job::job_status(&finished)
    }))?;

    // A grant that failed is a machine still holding what it held, whatever the repairs did.
    Ok(match render::job::job_succeeded(&finished) {
        true => outcome,
        false => ExitCode::FAILURE,
    })
}

/// Does the repair itself raise the prompt? Only for `--yes`, and only as a dialog: a prompt from
/// this terminal is raised by this process once the repair has queued what needs it (ADR 0065).
fn the_daemon_raises_it(yes: bool, route: Route) -> bool {
    yes && route == Route::Dialog
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yes_over_a_dialog_lets_the_daemon_raise_it() {
        assert!(the_daemon_raises_it(true, Route::Dialog));
    }

    /// Over SSH the daemon's dialog would land where nobody is, so the repair only queues.
    #[test]
    fn yes_from_a_terminal_keeps_the_prompt_here() {
        assert!(!the_daemon_raises_it(true, Route::ByHand));
    }

    #[test]
    fn without_yes_the_daemon_never_raises_it() {
        assert!(!the_daemon_raises_it(false, Route::Dialog));
        assert!(!the_daemon_raises_it(false, Route::ByHand));
    }
}
