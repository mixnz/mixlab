//! `mix elevation`: what waits for an administrator, and granting it.

use std::io::Write as _;
use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    ElevationDrop, ElevationGrant, ElevationStatus, Error, ErrorCode, JobSummary, PendingOpId, rpc,
};

use crate::autostart::Autostart;
use crate::by_hand::{self, Route, Terminal};
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

            // ADR 0065: which way to ask is settled before anybody is asked whether to. An empty
            // queue is `elevation.grant`'s own refusal to make, and it is left to it.
            let route = match waiting.pending.is_empty() {
                true => Route::Dialog,
                false => route_for(&waiting, json, "mix elevation grant")?,
            };
            nothing_left_over_ssh(&waiting, route, Terminal::here().ssh)?;

            // What is skipped is the question: there is nothing to put in front of somebody.
            if !waiting.pending.is_empty() && !yes && !confirmed(&waiting, json, route)? {
                // Saying no is an answer and not a failure — `docs/decisions/0005-on-demand-
                // elevation.md`. Nothing was written and nothing was dropped, so the same command
                // works when the person is ready.
                return Ok(ExitCode::SUCCESS);
            }

            let started = start_grant(&mut client, route).await?;

            if no_wait {
                emit(&rendered(json, &started, || {
                    render::job::job_status(&started)
                }))?;
                say_where_the_command_is(route);
                return Ok(ExitCode::SUCCESS);
            }

            let finished = see_through(&mut client, route, started, json).await?;
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

/// Which way a grant of `waiting` is raised from here (ADR 0065), or why it cannot be.
///
/// # Errors
///
/// `privileged_required` when neither a dialog nor this terminal can ask, naming `rerun`, and when
/// the helper a terminal would run is not one it may.
pub(crate) fn route_for(
    waiting: &ElevationStatus,
    json: bool,
    rerun: &str,
) -> Result<Route, Error> {
    let route = by_hand::route(waiting, Terminal::here(), json, rerun)?;

    if route == Route::ByHand {
        by_hand::trusted_here(waiting.helper.as_deref())?;
    }

    Ok(route)
}

/// Refuse a terminal over SSH whose every waiting row needs the machine's own screen — before
/// `continue? [y/N]`, rather than after it, where the daemon would refuse the same batch.
///
/// # Errors
///
/// `privileged_required`, with the daemon's sentence for the first such row.
pub(crate) fn nothing_left_over_ssh(
    waiting: &ElevationStatus,
    route: Route,
    over_ssh: bool,
) -> Result<(), Error> {
    if route != Route::ByHand || !over_ssh {
        return Ok(());
    }

    let mut reasons = waiting
        .pending
        .iter()
        .map(|pending| pending.needs_the_screen.as_deref());

    match reasons.next() {
        Some(Some(reason)) if reasons.all(|other| other.is_some()) => Err(Error::new(
            ErrorCode::PrivilegedRequired,
            format!("this cannot be allowed over SSH: {reason}"),
        )
        .with_hint(at_the_screen(waiting.can_prompt))),
        _ => Ok(()),
    }
}

/// Where a row left for the screen can be granted — the daemon's `at_the_screen`, which says the
/// same after `continue?`.
///
/// MixLab only when the daemon can raise a dialog: one started over SSH cannot, and the window then
/// has no Allow to offer. A terminal at the screen still can, through `sudo`.
fn at_the_screen(can_prompt: bool) -> &'static str {
    if can_prompt {
        "run `mix elevation grant` on this machine itself, or allow it in MixLab"
    } else {
        "run `mix elevation grant` in a terminal on this machine itself"
    }
}

/// `elevation.grant`, raised the way `route` says.
///
/// No parameters for a dialog, as before ADR 0065, so a daemon from before it still answers.
///
/// # Errors
///
/// Whatever `elevation.grant` refuses.
pub(crate) async fn start_grant(client: &mut Client, route: Route) -> Result<JobSummary, Error> {
    let asked = match route {
        Route::Dialog => None,
        Route::ByHand => encode(&ElevationGrant {
            by_hand: true,
            over_ssh: Terminal::here().ssh,
        }),
    };

    ask(client, rpc::method::ELEVATION_GRANT, asked).await
}

/// Follow a grant to its end — running `sudo` on it first, when it waits for this terminal.
///
/// # Errors
///
/// What [`by_hand::run_waiting`] refuses, and a job that cannot be followed.
pub(crate) async fn see_through(
    client: &mut Client,
    route: Route,
    started: JobSummary,
    json: bool,
) -> Result<JobSummary, Error> {
    let ended = match route {
        Route::ByHand => by_hand::run_waiting(client, started.id).await?,
        Route::Dialog => None,
    };

    match ended {
        Some(finished) => Ok(finished),
        None => follow(client, started, json).await,
    }
}

/// After `--no-wait`, where a terminal's grant can be found.
pub(crate) fn say_where_the_command_is(route: Route) {
    if route == Route::ByHand {
        let _ = writeln!(
            std::io::stderr(),
            "`mix elevation status` shows the command to run in a terminal"
        );
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
pub(crate) fn confirmed(
    waiting: &ElevationStatus,
    json: bool,
    route: Route,
) -> Result<bool, Error> {
    if json {
        return Err(unanswered());
    }

    let screen = match route {
        Route::Dialog => render::elevation::elevation_prompt(waiting),
        Route::ByHand => render::elevation::elevation_prompt_by_hand(waiting, Terminal::here().ssh),
    };

    match confirm::ask(&format!("{screen}\ncontinue? [y/N] ")) {
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

#[cfg(test)]
mod tests {
    use mixengine_proto::{ElevationStatus, ErrorCode, PendingOp, PendingOpId, Timestamp};

    use super::*;

    fn a_row(id: i64, screen: bool) -> PendingOp {
        let op = mixengine_proto::privileged::PrivilegedOp::Probe {};

        PendingOp {
            id: PendingOpId(id),
            description: op.describe(),
            op,
            requested_at: Timestamp(1_760_000_000_000),
            needs_the_screen: screen.then(|| "it needs this Mac's own screen".to_owned()),
        }
    }

    fn waiting_on(pending: Vec<PendingOp>) -> ElevationStatus {
        ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: None,
            installed_helper: None,
            pending,
            last: None,
            waiting: None,
        }
    }

    /// Refused before `continue? [y/N]`, as the daemon would refuse after it.
    #[test]
    fn over_ssh_a_queue_of_screen_rows_is_refused_before_the_question() {
        let error = nothing_left_over_ssh(&waiting_on(vec![a_row(1, true)]), Route::ByHand, true)
            .expect_err("nothing a terminal over SSH can do");

        assert_eq!(error.code, ErrorCode::PrivilegedRequired);
        assert!(error.message.contains("over SSH"), "{}", error.message);
        assert!(error.to_string().contains("mix elevation grant"), "{error}");
    }

    /// A daemon started over SSH has no dialog, so MixLab is no way out.
    #[test]
    fn over_ssh_to_a_daemon_without_a_dialog_mixlab_is_not_offered() {
        let mut waiting = waiting_on(vec![a_row(1, true)]);
        waiting.can_prompt = false;

        let error = nothing_left_over_ssh(&waiting, Route::ByHand, true)
            .expect_err("nothing a terminal over SSH can do");

        assert!(!error.to_string().contains("MixLab"), "{error}");
        assert!(error.to_string().contains("in a terminal"), "{error}");
    }

    #[test]
    fn over_ssh_one_row_it_can_do_is_enough() {
        assert!(
            nothing_left_over_ssh(
                &waiting_on(vec![a_row(1, true), a_row(2, false)]),
                Route::ByHand,
                true
            )
            .is_ok()
        );
    }

    #[test]
    fn at_the_screen_or_by_dialog_nothing_is_refused() {
        let screen_only = waiting_on(vec![a_row(1, true)]);

        assert!(nothing_left_over_ssh(&screen_only, Route::ByHand, false).is_ok());
        assert!(nothing_left_over_ssh(&screen_only, Route::Dialog, true).is_ok());
    }
}
