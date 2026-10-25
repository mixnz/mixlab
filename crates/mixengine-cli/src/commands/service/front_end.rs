//! `mix service front-end` and `mix service set-front-end`: which web server this home is on,
//! and moving it to the other one.

use std::io::Write as _;
use std::process::ExitCode;

use mixengine_proto::{
    Error, ErrorCode, FrontEndReport, FrontEndServer, FrontEndSwitch, JobOutcome, JobSummary,
    PackageVersion, ServiceList, ServiceRole, ServiceSummary, rpc,
};

use crate::client::{Client, ask, encode};
use crate::confirm::unanswered;
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::{confirm, render};
/// Which service in a listing is the front end, as the daemon answered.
///
/// **Three answers and not an [`Option`]**, because the third is a wire fact and not a home fact: a
/// daemon built before `ServiceSummary::role` sends none, and reading that as *no front end* would
/// tell somebody their sites are unserved when they are being served perfectly well.
#[derive(Debug)]
pub(super) enum Found<'a> {
    /// The row whose role says so.
    On(&'a ServiceSummary),

    /// Every row answered, and none of them is one.
    None,

    /// No row answered at all: this daemon predates the member.
    Unanswered,
}

/// Pick it out, by what the daemon said a service is *for*.
///
/// Not a client deciding anything — the role is the daemon's own answer, from the crate that owns
/// the recipes — this is where it is read. ADR 0026's enforcement clause is that no client maps a
/// package name to a role, and this function is `mix` obeying it.
pub(super) fn the_front_end(list: &ServiceList) -> Found<'_> {
    if let Some(service) = list
        .services
        .iter()
        .find(|service| matches!(service.role, Some(ServiceRole::FrontEnd { .. })))
    {
        return Found::On(service);
    }

    // A home with no services at all has no front end, and that is an answer whatever the daemon's
    // build: there is no row whose role could have been missing.
    match list.services.is_empty() || list.services.iter().any(|service| service.role.is_some()) {
        true => Found::None,
        false => Found::Unanswered,
    }
}

/// `mix service set-front-end <server>`: move every site to the other web server.
///
/// **The question is asked before the call and not after it**, on `mix cleanup`'s rule: a switch
/// stops the web server every site in this home is reached through, and somebody who typed it needs
/// to know that before it happens rather than to read it in a report.
///
/// `grant: true` is sent because the person has just been told that a permission prompt may appear
/// — which is the rule every elevation follows: what is about to be allowed is shown before it is
/// allowed — and because the switch cannot go ahead without the grant on the one system that needs
/// it.
pub(super) async fn set_front_end(
    client: &mut Client,
    json: bool,
    server: FrontEndServer,
    version: Option<PackageVersion>,
    yes: bool,
    no_wait: bool,
) -> Result<ExitCode, Error> {
    if !yes {
        let list: ServiceList = ask(client, rpc::method::SERVICE_LIST, None).await?;

        if !agreed_to_switch(&list, server, json)? {
            // Saying no is an answer and not a failure — `mix uninstall`'s rule. Nothing moved, so
            // the same command works when the person is ready.
            return Ok(ExitCode::SUCCESS);
        }
    }

    let started: JobSummary = ask(
        client,
        rpc::method::SERVICE_SET_FRONT_END,
        encode(&FrontEndSwitch {
            server,
            version,
            grant: true,
        }),
    )
    .await?;

    if no_wait {
        emit(&rendered(json, &started, || {
            render::job::job_status(&started)
        }))?;
        return Ok(ExitCode::SUCCESS);
    }

    let finished = follow(client, started, json).await?;

    let Some(JobOutcome::Succeeded { result }) = finished.outcome.clone() else {
        emit(&rendered(json, &finished, || {
            render::job::job_status(&finished)
        }))?;
        return Ok(ExitCode::FAILURE);
    };

    let report: FrontEndReport = serde_json::from_value(result).map_err(|error| {
        Error::new(
            ErrorCode::Internal,
            format!(
                "mix {} cannot read the front-end report: {error}",
                env!("CARGO_PKG_VERSION")
            ),
        )
    })?;

    emit(&rendered(json, &report, || {
        render::service::front_end_report(&report)
    }))?;

    Ok(match report.wanted_more() {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    })
}

/// Ask, once, in front of what is about to happen.
///
/// **Asking for the server a home is already on is a yes**, on `agreed_to_cleanup`'s rule: a command
/// that asked *"change nothing?"* is one people learn to answer without reading.
///
/// **`--json` never asks**, and never acts instead.
fn agreed_to_switch(list: &ServiceList, server: FrontEndServer, json: bool) -> Result<bool, Error> {
    let standing = match the_front_end(list) {
        Found::On(service) => Some(service),
        // A home with none has nothing to stop, and a daemon that will not say is one the switch
        // itself refuses a moment later — neither is a reason to withhold the question.
        Found::None | Found::Unanswered => None,
    };

    if standing.is_some_and(|service| service.id.name() == server.package()) {
        return Ok(true);
    }

    if json {
        return Err(unanswered());
    }

    // Printed rather than held back, because the question below is refused under `--json` anyway:
    // what somebody is about to allow is what they are shown.
    emit(&match standing {
        Some(service) => format!(
            "{} is this home's front end.\nswitching to {server} stops it, renders every site for \
             {server} and starts it; no site is reachable while that happens.\n",
            service.id,
            server = server.package()
        ),
        None => format!(
            "this home has no front end; {} becomes one, and is left stopped.\n",
            server.package()
        ),
    })?;

    emit(
        "this machine may ask permission for it to answer on 80 and 443, and anything else \
         MixEngine is already waiting for is asked for at the same time; `mix elevation status` \
         lists that.\n",
    )?;

    match confirm::ask(&format!("\nswitch to {}? [y/N] ", server.package())) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            // On stderr, beside the question it answers.
            let _ = writeln!(std::io::stderr(), "nothing was changed");

            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}
