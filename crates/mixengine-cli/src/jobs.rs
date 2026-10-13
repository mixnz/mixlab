//! Following a job the daemon started until it ends, and drawing its progress while it runs.

use std::io::Write as _;

use mixengine_proto::{Error, JobSummary, JobWait, Millis, rpc};

use crate::client::{Client, ask, encode};

/// Poll a job until it ends, saying on stderr what it is doing as that changes.
///
/// A short timeout rather than the default thirty seconds: what is being waited for is a progress
/// report, and a wait that only answers when the job *ends* would leave a person watching nothing
/// for the length of a download. The daemon caps what it grants either way, so the cost of asking
/// often is one round trip a second on a socket that is not a network.
pub(crate) async fn follow(
    client: &mut Client,
    started: JobSummary,
    json: bool,
) -> Result<JobSummary, Error> {
    /// How long each `job.wait` asks for.
    const POLL: Millis = Millis(1_000);

    let mut job = started;
    let mut said = String::new();

    while !job.state.is_finished() {
        // Only in the human rendering, and only when it changed: a `--json` run emits one object,
        // and a progress line repeated once a second would be noise in a terminal and a log.
        if !json && job.message != said {
            said = job.message.clone();
            report_progress(job.percent, &said);
        }

        job = ask(
            client,
            rpc::method::JOB_WAIT,
            encode(&JobWait {
                job: job.id,
                timeout: POLL,
            }),
        )
        .await?;
    }

    Ok(job)
}

/// Say where a job has got to, where it will not be mistaken for the command's answer.
pub(crate) fn report_progress(percent: u8, message: &str) {
    if message.is_empty() {
        return;
    }

    // Nothing to do about a stderr that will not take it — the answer the user asked for is still
    // going out on stdout. `writeln!` rather than `eprintln!`, which panics when stderr is closed.
    let _ = writeln!(std::io::stderr(), "  {percent:>3}%  {message}");
}
