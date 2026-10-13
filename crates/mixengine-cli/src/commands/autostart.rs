//! `mix autostart`: the login entry that starts the daemon. Not [`crate::autostart`], which starts
//! one on demand.

use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{AutostartReport, Error, rpc};

use crate::autostart::Autostart;
use crate::client::{Client, ask};
use crate::output::{emit, rendered};
use crate::render;

/// `mix autostart …` — one subcommand per `autostart.*` method.
///
/// **None of the three takes an argument**, for `mix path`'s reason
/// ([`PathCommand`](crate::commands::path::PathCommand)): there is one entry per user and one home
/// this can be about, and an argument would be a command for registering arbitrary programs to run
/// at somebody's login.
///
/// **`mix autostart` and not `mix daemon autostart`.** `daemon.*` is about the daemon that is
/// running; a logon task, a LaunchAgent and a systemd user unit outlive every daemon that ever
/// registered them.
#[derive(Debug, Subcommand)]
pub(crate) enum AutostartCommand {
    /// Say whether this home's daemon starts when you log in.
    Status,

    /// Register it.
    ///
    /// Does **not** start the daemon: there is one running, and it is the one answering this. What
    /// it changes is what happens at your next login. Idempotent, and it says which of the two it
    /// did.
    Enable,

    /// Remove it.
    ///
    /// Does **not** stop the daemon that is running. Turning off "start at login" is not a request
    /// to lose the daemon you are using.
    Disable,
}

/// `mix autostart …`: one call, one rendering — roadmap task **T85b**.
///
/// No exit code of its own, on `mix path`'s rule: each of these either did what it said or failed
/// outright, and a machine with no mechanism at all is something to report rather than a failure of
/// the command.
///
/// **The `autostart` parameter is not what this command is about.** It is the client's own — the
/// thing that starts a daemon when none answers, in [`crate::autostart`] — and every command in this
/// file takes it under that name. The two meet here and nowhere else, which is why this function is
/// `autostart_entry` and the parameter is left alone.
pub(crate) async fn autostart_entry(
    command: AutostartCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let (method, rendered_as) = match command {
        AutostartCommand::Status => (rpc::method::AUTOSTART_STATUS, render::Autostarted::Asked),
        AutostartCommand::Enable => (rpc::method::AUTOSTART_ENABLE, render::Autostarted::Enabled),
        AutostartCommand::Disable => (
            rpc::method::AUTOSTART_DISABLE,
            render::Autostarted::Disabled,
        ),
    };

    let report: AutostartReport = ask(&mut client, method, None).await?;
    emit(&rendered(json, &report, || {
        render::autostart_report(rendered_as, &report)
    }))?;

    Ok(ExitCode::SUCCESS)
}
