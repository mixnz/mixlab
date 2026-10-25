//! `mix service set`: moving a service to another port.

use std::io::Write as _;
use std::process::ExitCode;

use mixengine_proto::{
    Error, ServiceId, ServiceQuery, ServiceSet, ServiceSetResult, ServiceState, ServiceSummary, rpc,
};

use crate::client::{Client, ask, encode};
use crate::confirm::{self, unanswered};
use crate::output::{emit, rendered};
use crate::render;

/// Move `service` to `port`.
///
/// **A running service is asked about first**, because the move restarts it and every open
/// connection is dropped — `set-front-end`'s rule: `--yes` answers in advance, and `--json` or a
/// terminal nobody is at refuses rather than assume. A stopped one is not asked: nothing is
/// restarted, so there is nothing to agree to.
///
/// Whether it is running is the daemon's answer, read just before the call.
pub(super) async fn set(
    client: &mut Client,
    json: bool,
    service: &ServiceId,
    port: u16,
    yes: bool,
) -> Result<ExitCode, Error> {
    if !yes {
        let summary: ServiceSummary = ask(
            client,
            rpc::method::SERVICE_STATUS,
            encode(&ServiceQuery {
                service: service.clone(),
            }),
        )
        .await?;

        if is_live(&summary) && !agreed_to_move(service, port, json)? {
            // Saying no is an answer, not a failure — `set-front-end`'s rule.
            return Ok(ExitCode::SUCCESS);
        }
    }

    let result: ServiceSetResult = ask(
        client,
        rpc::method::SERVICE_SET,
        encode(&ServiceSet {
            service: service.clone(),
            port: Some(port),
        }),
    )
    .await?;

    emit(&rendered(json, &result, || {
        render::service::service_set(&result)
    }))?;

    Ok(ExitCode::SUCCESS)
}

/// Whether a move would restart it: up, on its way up, or supervised.
fn is_live(summary: &ServiceSummary) -> bool {
    summary.supervised
        || matches!(
            summary.state,
            Some(
                ServiceState::Running
                    | ServiceState::Starting
                    | ServiceState::Restarting
                    | ServiceState::Degraded
            )
        )
}

/// Ask, once, in front of the restart.
///
/// # Errors
///
/// [`unanswered`] under `--json` and where there is nobody to ask.
fn agreed_to_move(service: &ServiceId, port: u16, json: bool) -> Result<bool, Error> {
    if json {
        return Err(unanswered());
    }

    match confirm::ask(&format!(
        "\n{service} is running; moving it to {port} restarts it, and every open connection is \
         dropped.\n\nmove it? [y/N] "
    )) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            // On stderr, beside the question it answers.
            let _ = writeln!(std::io::stderr(), "nothing was changed");

            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}
