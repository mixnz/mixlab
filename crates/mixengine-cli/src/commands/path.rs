//! `mix path`: whether `<root>/bin` is on the PATH, and putting it there.

use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{Error, PathReport, rpc};

use crate::autostart::Autostart;
use crate::client::{Client, ask};
use crate::output::{emit, rendered};
use crate::render;

/// `mix path …` — one subcommand per `path.*` method.
///
/// **None of the three takes an argument.** There is one directory this can be about, `<root>/bin`,
/// and the daemon is what knows where it is — a `--dir` here would be a command for putting
/// arbitrary directories on somebody's PATH, which is not a thing MixEngine does.
#[derive(Debug, Subcommand)]
pub(crate) enum PathCommand {
    /// Say whether a new terminal would find this home's commands.
    Status,

    /// Fill `<root>/bin` and put it on this user's PATH.
    ///
    /// Idempotent, and it says which of the two it did: a profile that already carries the line is
    /// left exactly as it is.
    Install,

    /// Take `<root>/bin` back off this user's PATH.
    ///
    /// The commands stay in the directory. They are inside the home, and removing the home is what
    /// removes them.
    Uninstall,

    /// Look for a tool you installed into a runtime, now.
    ///
    /// The daemon notices on its own when a runtime's folder changes, so `npm install -g yarn`
    /// makes `yarn` a command without anybody asking. This is for a folder the system would not
    /// let it watch.
    Rescan,
}

/// `mix path …`: one call, one rendering.
///
/// No exit code of its own — unlike `mix service start`, every one of these either did what it said
/// or failed outright, and there is no partial answer for a status to describe. A `bin/` with a
/// leftover in it is reported in the rendering and is not a failure: the commands that should be
/// there are there.
pub(crate) async fn path(
    command: PathCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let (method, pathed) = match command {
        PathCommand::Status => (rpc::method::PATH_STATUS, render::path::Pathed::Asked),
        PathCommand::Install => (rpc::method::PATH_INSTALL, render::path::Pathed::Installed),
        PathCommand::Uninstall => (
            rpc::method::PATH_UNINSTALL,
            render::path::Pathed::Uninstalled,
        ),
        PathCommand::Rescan => (rpc::method::PATH_RESCAN, render::path::Pathed::Rescanned),
    };

    let report: PathReport = ask(&mut client, method, None).await?;
    emit(&rendered(json, &report, || {
        render::path::path_report(pathed, &report)
    }))?;

    Ok(ExitCode::SUCCESS)
}
