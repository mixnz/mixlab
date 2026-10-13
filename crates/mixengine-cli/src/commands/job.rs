//! `mix job`: listing, waiting for and cancelling what the daemon runs in the background.

use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    Error, JobFilter, JobId, JobList, JobQuery, JobState, JobSummary, JobWait, Millis, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::output::{emit, rendered};
use crate::parse::job_state;
use crate::{logs, render};

/// `mix job …` — one subcommand per `job.*` method.
#[derive(Debug, Subcommand)]
pub(crate) enum JobCommand {
    /// List what this home has run, newest first.
    List {
        /// Only jobs in this state.
        #[arg(long, value_name = "STATE", value_parser = job_state)]
        state: Option<JobState>,

        /// At most this many.
        #[arg(long, short = 'n', value_name = "COUNT", default_value_t = 50)]
        limit: u32,
    },

    /// Describe one job.
    Status {
        /// The job, as `mix job list` numbers them.
        #[arg(value_name = "JOB")]
        job: i64,
    },

    /// Wait for a job to finish.
    ///
    /// **Answers when the job ends or when the wait runs out**, and the second is not an error:
    /// what comes back is the job as it stands. The exit status is what a script branches on. It is
    /// non-zero for a job that failed, and for one that has not finished yet.
    Wait {
        /// The job to wait for.
        #[arg(value_name = "JOB")]
        job: i64,

        /// How long to wait. The daemon caps what it grants.
        #[arg(long, value_name = "SECONDS", default_value_t = 30)]
        timeout: u64,
    },

    /// Ask a running job to stop.
    ///
    /// Cancellation is cooperative, so what comes back may still say `running`: the work ends when
    /// it next looks. Cancelling a job that has already ended is not an error.
    Cancel {
        /// The job to cancel.
        #[arg(value_name = "JOB")]
        job: i64,
    },

    /// What a job printed.
    ///
    /// Only a job that runs another program prints anything. Today that is an apply running a
    /// blueprint's `[scaffold]` command; other jobs report progress and a result, and show nothing
    /// here. The lines stay in memory while the daemon keeps the job, so read them while it runs.
    //
    // Roadmap task T78a.
    Logs {
        /// The job, as `mix job list` numbers them.
        #[arg(value_name = "JOB")]
        job: i64,

        /// Keep printing as the job prints.
        #[arg(long, short = 'f')]
        follow: bool,

        /// How many of the lines already printed to begin with.
        #[arg(long, short = 'n', value_name = "COUNT", default_value_t = 200)]
        lines: usize,
    },
}

/// `mix job …`: one call, one rendering, and an exit status that means what a shell expects.
///
/// **A job that failed is an answer and not an error**, which is why this returns an [`ExitCode`]:
/// what happened is on stdout in both renderings, and what changes is the status — so
/// `mix job wait 3 && …` stops where a person reading the output would.
pub(crate) async fn job(
    command: JobCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    // **Only `wait` answers with the job's own outcome.** The other three did what they were asked
    // the moment the daemon answered — a status was reported, a cancellation was requested — and a
    // non-zero exit for `mix job status` on a job that failed yesterday would make asking about a
    // failure a failure.
    let (method, params, verdict) = match command {
        JobCommand::List { state, limit } => {
            let list: JobList = ask(
                &mut client,
                rpc::method::JOB_LIST,
                encode(&JobFilter { state, limit }),
            )
            .await?;
            emit(&rendered(json, &list, || render::job::job_list(&list)))?;
            return Ok(ExitCode::SUCCESS);
        }

        JobCommand::Status { job } => (
            rpc::method::JOB_STATUS,
            encode(&JobQuery { job: JobId(job) }),
            false,
        ),
        JobCommand::Cancel { job } => (
            rpc::method::JOB_CANCEL,
            encode(&JobQuery { job: JobId(job) }),
            false,
        ),

        // **The one job command that is not a call** — roadmap task **T78a**: output travels on its
        // own stream, never on the event stream and never as a method's answer (ADR 0009).
        JobCommand::Logs { job, follow, lines } => {
            return logs(&mut client, &format!("logs/job/{job}"), lines, follow, json).await;
        }
        JobCommand::Wait { job, timeout } => (
            rpc::method::JOB_WAIT,
            encode(&JobWait {
                job: JobId(job),
                timeout: Millis::from_secs(timeout),
            }),
            true,
        ),
    };

    let job: JobSummary = ask(&mut client, method, params).await?;
    emit(&rendered(json, &job, || render::job::job_status(&job)))?;

    // A wait that ran out is not a success either: it is what a script blocks on, and exiting zero
    // there would carry it past a download that is still running.
    Ok(match !verdict || render::job::job_succeeded(&job) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    })
}
