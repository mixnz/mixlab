//! `mix service logs`' stream, and the line it prints for what it missed.

use std::io::Write as _;
use std::process::ExitCode;

use mixengine_proto::{Error, LogFrame};

use crate::client::Client;
use crate::output::emit;

/// `mix service logs`: what a service has printed, and what it prints next.
///
/// **Written out as it arrives rather than collected**, which is the whole difference between this
/// and every other command here: a `--follow` never has a last message, and a buffer that filled
/// until the stream ended would print nothing for as long as the service kept running.
///
/// **The text goes out exactly as the service wrote it.** No timestamp, no `[stderr]`, nothing of
/// MixEngine's — for the same reason `current.log` carries none: this is piped into `grep` by
/// somebody who greps MariaDB's log the same way, and a prefix of ours would break every one of
/// those to restate what `--json` already carries. What the human rendering does add is the one
/// thing that is not output: a gap, on stderr, where the daemon or this client fell behind and lines
/// were lost. Silence there would make a log with a hole in it look complete.
/// `subject` is the route's two segments — `service/<id>` or `job/<id>` (roadmap task **T78a**) —
/// because what differs between the two is the path and nothing else this function does.
pub(crate) async fn logs(
    client: &mut Client,
    subject: &str,
    lines: usize,
    follow: bool,
    json: bool,
) -> Result<ExitCode, Error> {
    let path = format!("/{subject}?tail={lines}&follow={}", u8::from(follow));
    let mut stream = client.stream(&path).await?;

    while let Some(frame) = stream.next::<LogFrame>().await? {
        match (json, &frame) {
            // Verbatim, one object per line: a script filtering on `stream` or ordering by `at`
            // needs what the human rendering deliberately drops.
            (true, _) => emit(&format!(
                "{}\n",
                serde_json::to_string(&frame).expect("a proto type always serialises")
            ))?,

            (false, LogFrame::Line(line)) => emit(&format!("{}\n", line.text))?,
            (false, LogFrame::Historic { text }) => emit(&format!("{text}\n"))?,

            (false, LogFrame::Gap { missed }) => {
                report_gap(*missed);
            }

            // A variant from a later daemon. Ignored rather than refused, which is what the wire
            // types are `non_exhaustive` for.
            (false, _) => {}
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Say on stderr that lines were lost, so that a redirected log stays exactly the log.
fn report_gap(missed: u64) {
    let mut stderr = std::io::stderr();

    // Nothing to do about a stderr that will not take it, and nothing worth failing the command
    // over: the output the user asked for is still going out.
    let _ = writeln!(
        stderr,
        "mix: {missed} lines were dropped; this client fell behind the service"
    );
}
