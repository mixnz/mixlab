//! `mix home`: restoring what a previous copy of the home left. Not
//! [`crate::home`](mod@crate::home), which decides which home `mix` talks about.

use std::io::Write as _;
use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{Error, rpc};

use crate::autostart::Autostart;
use crate::client::{Client, ask};
use crate::confirm::unanswered;
use crate::output::{emit, rendered};
use crate::{confirm, render};

/// `mix home …` — what an earlier install left a copy of, and bringing it back (T182h).
#[derive(Debug, Subcommand)]
pub(crate) enum HomeCommand {
    /// Say what the copy an earlier install left in its kept folders holds, if there is one.
    Previous,

    /// Restore that copy into this home: projects, sites, domains and services.
    ///
    /// Every database gets a new admin password and is left stopped; the databases in it are kept.
    Restore {
        /// Do not ask first.
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

/// `mix home previous` and `mix home restore`.
pub(crate) async fn home(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    command: &HomeCommand,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        HomeCommand::Previous => {
            let previous: mixengine_proto::HomePrevious =
                ask(&mut client, rpc::method::HOME_PREVIOUS, None).await?;
            emit(&rendered(json, &previous, || {
                render::home::home_previous(&previous)
            }))?;
        }

        HomeCommand::Restore { yes } => {
            if !*yes && !agreed_to_restore()? {
                return Ok(ExitCode::SUCCESS);
            }
            let report: mixengine_proto::HomeRestoreReport =
                ask(&mut client, rpc::method::HOME_RESTORE, None).await?;
            emit(&rendered(json, &report, || {
                render::home::home_restored(&report)
            }))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn agreed_to_restore() -> Result<bool, Error> {
    match confirm::ask(
        "\nThe projects, sites and services an earlier install left a copy of come back into this \
         home.\nEvery database gets a new admin password and stays stopped; the databases in it are \
         kept.\n\nrestore? [y/N] ",
    ) {
        confirm::Answer::Yes => Ok(true),
        confirm::Answer::No => {
            let _ = writeln!(std::io::stderr(), "nothing was changed");
            Ok(false)
        }
        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}
