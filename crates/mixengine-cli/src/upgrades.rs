//! The confirmation an upgrade asks for and the step that finishes it — the same for a runtime
//! and a package.

use std::io::Write as _;
use std::process::ExitCode;

use mixengine_proto::{Error, JobSummary, UpgradePlan};

use crate::client::Client;
use crate::confirm::unanswered;
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::{confirm, render};

/// Print the plan, and ask — or answer `true` without asking under `--yes`. T193, D7.
pub(crate) fn upgrade_confirmed(
    plan: &UpgradePlan,
    dry_run: bool,
    yes: bool,
    json: bool,
) -> Result<bool, Error> {
    // **One document per `--json` run**: the plan is the answer only to a dry run; otherwise the
    // finished job carries the same plan, marked, and is what is printed.
    if !json || dry_run {
        emit(&rendered(json, plan, || {
            render::upgrades::upgrade_plan(plan)
        }))?;
    }

    if dry_run {
        return Ok(false);
    }
    if yes {
        return Ok(true);
    }

    match confirm::ask(&format!(
        "\nupdate {} {} to {}? [y/N] ",
        plan.subject, plan.from, plan.to
    )) {
        confirm::Answer::Yes => Ok(true),
        confirm::Answer::No => {
            let _ = writeln!(std::io::stderr(), "nothing was changed");
            Ok(false)
        }
        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}

/// Follow an update job and print what it did.
pub(crate) async fn finish_upgrade(
    client: &mut Client,
    started: JobSummary,
    no_wait: bool,
    json: bool,
) -> Result<ExitCode, Error> {
    if no_wait {
        emit(&rendered(json, &started, || render::job_status(&started)))?;
        return Ok(ExitCode::SUCCESS);
    }

    let finished = follow(client, started, json).await?;
    let result = serde_json::to_value(&finished).ok().and_then(|value| {
        serde_json::from_value::<UpgradePlan>(value["outcome"]["result"].clone()).ok()
    });

    emit(&rendered(json, &finished, || match &result {
        Some(plan) => render::upgrades::upgrade_plan(plan),
        None => render::job_status(&finished),
    }))?;

    Ok(match render::job_succeeded(&finished) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    })
}
