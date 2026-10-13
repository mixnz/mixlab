//! `mix elevation`: what waits for an administrator, and granting it.

use std::io::Write as _;
use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    ElevationDrop, ElevationStatus, Error, ErrorCode, JobSummary, PendingOpId, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::confirm::unanswered;
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::{confirm, render};

/// `mix elevation …` — one subcommand per `elevation.*` method, and nothing that is not one.
///
/// **There is no `mix elevation enqueue`, and there will not be.** What needs an administrator's
/// permission is decided by the operation that needs it — creating a site, issuing a certificate —
/// and a command that let a person put an arbitrary privileged operation in the queue would be a
/// client deciding what runs as root.
#[derive(Debug, Subcommand)]
pub(crate) enum ElevationCommand {
    /// Say what is waiting for permission, and what each of them will change.
    Status,

    /// Ask once, for everything that is waiting.
    ///
    /// One prompt covers the whole list. Saying no is a normal answer: the list stays, and this
    /// command can be run again later.
    //
    // One prompt for the whole queue: `docs/decisions/0005-on-demand-elevation.md` calls asking
    // inside a loop a defect.
    Grant {
        /// Say yes in advance, instead of being asked.
        ///
        /// What it skips is the question, never the screen: every operation and what it will change
        /// is printed either way. It exists for a caller that cannot be asked, such as a script or
        /// a CI step, and for `--json`, which has no way to answer.
        #[arg(long)]
        yes: bool,

        /// Answer as soon as the prompt has been raised, without waiting for it.
        #[arg(long)]
        no_wait: bool,
    },

    /// Forget an operation that is waiting, so it is never asked about again.
    Drop {
        /// Which one, as `mix elevation status` numbers them.
        op: Option<i64>,

        /// Forget all of them.
        ///
        /// Its own flag rather than "drop with nothing named": emptying the queue by typing less is
        /// exactly the mistake worth making impossible.
        #[arg(long, conflicts_with = "op")]
        all: bool,
    },
}

/// `mix elevation …`: one call, one rendering — except the grant, which is one call and a wait.
pub(crate) async fn elevation(
    command: ElevationCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        ElevationCommand::Status => {
            let status: ElevationStatus =
                ask(&mut client, rpc::method::ELEVATION_STATUS, None).await?;
            emit(&rendered(json, &status, || {
                render::elevation::elevation_status(&status)
            }))?;

            Ok(ExitCode::SUCCESS)
        }

        ElevationCommand::Grant { yes, no_wait } => {
            // Roadmap task T64: what is about to be allowed is read before it is allowed. The
            // ordering is a property of the API rather than of this function — the daemon never
            // raises a prompt on its own initiative, so there is a moment between knowing the batch
            // and asking for it, and this is what happens in that moment.
            let waiting: ElevationStatus =
                ask(&mut client, rpc::method::ELEVATION_STATUS, None).await?;

            // An empty queue is `elevation.grant`'s own refusal to make, and it is left to it. What
            // is skipped is the question: there is nothing to put in front of somebody.
            if !waiting.pending.is_empty() && !yes && !confirmed(&waiting, json)? {
                // Saying no is an answer and not a failure — `docs/decisions/0005-on-demand-
                // elevation.md`. Nothing was written and nothing was dropped, so the same command
                // works when the person is ready.
                return Ok(ExitCode::SUCCESS);
            }

            let started: JobSummary = ask(&mut client, rpc::method::ELEVATION_GRANT, None).await?;

            if no_wait {
                emit(&rendered(json, &started, || {
                    render::job::job_status(&started)
                }))?;
                return Ok(ExitCode::SUCCESS);
            }

            let finished = follow(&mut client, started, json).await?;
            emit(&rendered(json, &finished, || {
                render::job::job_status(&finished)
            }))?;

            Ok(match render::job::job_succeeded(&finished) {
                true => ExitCode::SUCCESS,
                false => ExitCode::FAILURE,
            })
        }

        ElevationCommand::Drop { op, all } => {
            // Neither named is a person who has not decided which. Refused here rather than turned
            // into "all", which is the reading that empties a queue somebody meant to prune.
            if op.is_none() && !all {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "name an operation to forget, or pass --all to forget every one of them",
                ));
            }

            let asked = ElevationDrop {
                op: op.map(PendingOpId),
            };
            let left: ElevationStatus =
                ask(&mut client, rpc::method::ELEVATION_DROP, encode(&asked)).await?;

            emit(&rendered(json, &left, || {
                render::elevation::elevation_status(&left)
            }))?;

            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Show what a grant would allow, ask about it, and answer whether to go on — roadmap task **T64**.
///
/// The screen is printed whichever way this ends, because it is the point: a person is being asked
/// to give an administrator's permission to a batch of operations, and the batch is what they are
/// judging. The question comes after it, never instead of it.
///
/// # Errors
///
/// When there is nobody to ask — `--json`, or a standard input at end of file. Both are refused
/// rather than assumed either way: yes would raise a dialog on a machine nobody is sitting at, and
/// no would be a decline the caller could not tell from a grant that happened.
pub(crate) fn confirmed(waiting: &ElevationStatus, json: bool) -> Result<bool, Error> {
    if json {
        return Err(unanswered());
    }

    match confirm::ask(&format!(
        "{}\ncontinue? [y/N] ",
        render::elevation::elevation_prompt(waiting)
    )) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            // On stderr, beside the question it answers. Stdout carries what a command was asked
            // for, and this run was asked for a grant that is not going to happen.
            let _ = writeln!(
                std::io::stderr(),
                "nothing was asked for; run `mix elevation grant` again when you are ready"
            );

            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}
