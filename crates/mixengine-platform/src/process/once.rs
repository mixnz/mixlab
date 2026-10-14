//! Running a program once and keeping what it said — a health probe, a stop command, a
//! first-run step. Not supervised: [`run_once`] waits for it to end, or for the time it
//! was given to run out, and reports how it ended beside the last of its output.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
#[cfg(not(windows))]
use std::process::Stdio;
use std::time::Duration;

#[cfg(not(windows))]
use tokio::io::{AsyncReadExt, AsyncWriteExt as _};

use super::Exit;
#[cfg(not(windows))]
use super::{describe, hide_stdio_from_children, state_the_whole_environment};
#[cfg(not(windows))]
use crate::Error;
use crate::Result;
use crate::sys::process as sys;

/// How long the last words of a one-shot are waited for once the process itself has ended.
///
/// Bounded because end of file on a pipe is not the process exiting but the *last holder of that
/// pipe* exiting, and a one-shot that left a descendant behind holds its own stdout open through
/// that descendant for as long as it lives. An unbounded wait here would hang [`run_once`] at the
/// one moment it has an answer to give.
///
/// Short because there is nothing left to wait for: the process has gone, so anything still in the
/// pipe is already written and only has to be read. Two seconds is the supervisor's number for the
/// same wait on a service's last log lines.
pub(crate) const LAST_WORDS: Duration = Duration::from_secs(2);

/// A program that was run to completion — or was still running when its patience ran out.
///
/// What [`run_once`] hands back, and it carries the output as well as the status because the two
/// callers that exist both need it for the same reason: a health probe that failed and a shutdown
/// command that failed are each a line in `daemon.log` that is worth nothing without the sentence
/// the program printed. `ERROR 1045: Access denied` is the whole of what a user has to act on, and
/// the exit code alone is `1`.
#[derive(Debug)]
pub struct Ran {
    /// How it ended, or [`None`] for one that was killed at its deadline.
    exit: Option<Exit>,

    /// What it printed, lossily decoded and trimmed. Whatever had arrived by the end for a run that
    /// timed out, which is usually all of it — the streams are read while the program runs, so a
    /// deadline cuts off a program that is still talking rather than one nobody was listening to.
    stdout: String,

    /// The same, for the stream a program complains on.
    stderr: String,
}

impl Ran {
    /// One run, described.
    ///
    /// The constructor exists because there are now three callers — the two returns of [`run`] and
    /// the Windows one-shot in `windows/process.rs` — and a fourth literal `Ran { .. }` is a fourth
    /// chance for one of them to trim a stream differently from the others.
    pub(crate) fn new(exit: Option<Exit>, stdout: &[u8], stderr: &[u8]) -> Self {
        Self {
            exit,
            stdout: said(stdout),
            stderr: said(stderr),
        }
    }

    /// How it ended, or [`None`] if it was still running when the deadline passed and was killed.
    #[must_use]
    pub fn exit(&self) -> Option<&Exit> {
        self.exit.as_ref()
    }

    /// Whether it ran out of time.
    ///
    /// Distinct from failing, and the distinction is the caller's to act on: a `mariadb-admin ping`
    /// that answers "no" in 20 ms is a database refusing queries, and one that never answers is a
    /// database that has stopped listening — the second is also what a health check with no deadline
    /// would have read as *healthy* for as long as it stayed broken.
    #[must_use]
    pub fn timed_out(&self) -> bool {
        self.exit.is_none()
    }

    /// Whether it exited the way a program that did its job exits.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.exit.as_ref().is_some_and(Exit::is_success)
    }

    /// The one line to put in a log about a run that did not go well.
    ///
    /// The last line the program printed, from `stderr` if it said anything there and from `stdout`
    /// otherwise — programs of this kind disagree about which stream a complaint belongs on, and the
    /// *last* line rather than the first because a usage error follows the banner it printed above
    /// it. [`None`] when the program said nothing at all, which is what the caller wants to know
    /// before it writes an empty field into a log line.
    #[must_use]
    pub fn complaint(&self) -> Option<&str> {
        self.complaints().lines().next_back()
    }

    /// The whole of the stream it complained on — `stderr` if it said anything there, `stdout`
    /// otherwise.
    ///
    /// [`complaint`](Self::complaint) is the one line to log; this is for the caller that has to
    /// pick a *different* line out of the same stream, because the program it ran puts the reason
    /// somewhere other than the end. Choosing the stream here rather than at each call site is the
    /// point: a caller reading the first line of `stderr` while `complaint` read the last line of
    /// `stdout` would be quoting two different runs of the same program.
    #[must_use]
    pub fn complaints(&self) -> &str {
        if self.stderr.lines().next().is_some() {
            &self.stderr
        } else {
            &self.stdout
        }
    }

    /// Everything the program wrote to standard output, whatever it wrote beside it.
    ///
    /// **Deliberately not [`complaints`](Self::complaints)**, which answers *where it complained*
    /// and prefers `stderr`. A caller reading a program's **answer** wants this stream and only
    /// this one: a database client that warns on `stderr` and prints its result on `stdout` would
    /// otherwise have its warning read as the result. Roadmap task **T77a**, whose probe is exactly
    /// that caller.
    #[must_use]
    pub fn output(&self) -> &str {
        &self.stdout
    }
}

/// Run `program` to completion and report how it ended, killing it if `patience` runs out first.
///
/// **The one-shot, and the whole of what separates it from [`spawn_supervised`] is intent**: the
/// caller wants an exit status, not a service. There is no process group, no ready check and no
/// restart — a program run for its answer that started children of its own would be a program doing
/// something this call is not for. `stdin` is the null device and both other streams are captured,
/// so a probe that decides to ask an interactive question is a timeout rather than a hang.
///
/// The environment and the working directory are the *service's*, applied by the same rule
/// [`spawn_supervised`] uses and by the same code: a probe that saw a different environment from the
/// server it is asking about would be asking about a different server.
///
/// **On Windows it is given no console window**, which is the reason this function is here at all
/// and not in the crate that wants it: a daemon has no console, so a console-subsystem child is
/// handed a *new* one, and on Windows 11 that is a terminal window appearing on the user's desktop —
/// every ten seconds, for a health probe. `windows/process.rs` has the whole of that story, measured
/// rather than reasoned about.
///
/// # What running out of patience means
///
/// The process is killed and the run answers [`Ran::timed_out`]. Only the process this call started:
/// a one-shot that forked is out of scope, deliberately, because the alternative is a session and a
/// group for something whose whole life is meant to be a few milliseconds — and no probe or
/// shutdown command any service documents behaves that way. What is guaranteed is that no process
/// outlives this call unnoticed, which the kill covers.
///
/// **`patience` bounds the process ending, not its pipes closing**, and the two are only usually the
/// same moment. End of file on a pipe arrives when the *last holder* of it exits, and a one-shot
/// that leaves a descendant behind has handed that descendant its stdout: a `mariadb-admin shutdown`
/// of that shape would be reported as having timed out long after it exited `0`, and the caller
/// would kill the database it had just asked to shut down cleanly — every ten seconds, for a probe.
/// So the deadline is read off [`tokio::process::Child::wait`] alone, and the last of the output is
/// then waited for separately and briefly (`LAST_WORDS`). The supervisor bounds the same wait, for
/// the same reason, for a service's last log lines.
///
/// What a timed-out program printed is kept rather than thrown away — the streams are drained
/// alongside the wait, so by the time the deadline passes it is already in hand. That draining is
/// not only for the log: a pipe holds a page or two and a program that fills one blocks on its own
/// write until somebody reads, so a probe with a screen of complaint to make would otherwise stop at
/// that write and be timed out as if it had hung.
///
/// # Errors
///
/// [`Error::Io`] naming the program when it cannot be started at all — a probe whose binary is not
/// installed, which is a spec to fix rather than a service to degrade — and [`Error::Os`] if the OS
/// will not say how a process it started ended.
///
/// [`spawn_supervised`]: super::spawn_supervised
/// [`Error::Io`]: crate::Error::Io
/// [`Error::Os`]: crate::Error::Os
pub async fn run_once(
    program: &Path,
    args: &[OsString],
    directory: &Path,
    env: &BTreeMap<String, String>,
    patience: Duration,
) -> Result<Ran> {
    run(program, args, directory, env, patience, None).await
}

/// [`run_once`], with `input` written to the program's standard input, which is then closed.
///
/// **For a program whose whole instruction is what it reads** — `mariadbd --bootstrap`, which takes
/// its SQL there and listens on no port and no socket while it does. The alternative for that case
/// is a temporary file, which for a statement that sets a root password would be a plaintext
/// credential on disk.
///
/// # The size of `input` is the caller's to bound, and has to be
///
/// The write happens *before* the wait, so an input larger than the kernel will take in one go
/// would block here rather than being timed out. That is affordable because the only caller writes
/// a handful of SQL statements and a pipe buffer is a page or more — and it is stated rather than
/// guarded, because a guard would be a second task and a second failure path for a case nothing in
/// this workspace has.
///
/// # Errors
///
/// As [`run_once`], and [`Error::Io`] naming the program when its standard input cannot be written
/// to at all — a program that closed it and exited, which is the same class of failure as one that
/// would not start.
///
/// [`Error::Io`]: crate::Error::Io
pub async fn run_once_with_input(
    program: &Path,
    args: &[OsString],
    directory: &Path,
    env: &BTreeMap<String, String>,
    patience: Duration,
    input: &str,
) -> Result<Ran> {
    run(program, args, directory, env, patience, Some(input)).await
}

/// The body both of the above are, differing only in what the child is given to read.
async fn run(
    program: &Path,
    args: &[OsString],
    directory: &Path,
    env: &BTreeMap<String, String>,
    patience: Duration,
    input: Option<&str>,
) -> Result<Ran> {
    // **Windows takes the other path entirely**, because a child that inherits Administrators is a
    // child `postgres --single` refuses to be — see `windows/restricted.rs` and ADR 0010. There is
    // no `tokio::process::Child` to be had from a foreign handle, so the whole of it runs on a
    // blocking thread and the two paths meet again at `Ran`.
    #[cfg(windows)]
    {
        return sys::run_child(
            program,
            args,
            directory,
            env,
            patience,
            input.map(str::to_owned),
        )
        .await;
    }

    #[cfg(not(windows))]
    {
        let mut command = tokio::process::Command::new(program);
        command
            .args(args)
            .current_dir(directory)
            .stdin(match input {
                None => Stdio::null(),
                Some(_) => Stdio::piped(),
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // What makes the timeout below a promise rather than a hope: the future being dropped is
            // what kills the process, so a caller that gives up on this call — a health loop cancelled
            // by a daemon shutting down, say — does not leave the probe behind it.
            .kill_on_drop(true);

        state_the_whole_environment(command.as_std_mut(), env);
        sys::arrange_one_shot(command.as_std_mut());

        // Held for the length of the spawn, for the reason `spawn_supervised` holds it: an inheritable
        // handle this process was given is passed on to every child it starts, and a probe running every
        // ten seconds is ten seconds of another process's pipe being held open, for ever.
        let hiding = hide_stdio_from_children();
        let spawned = command.spawn();
        drop(hiding);

        let mut child = spawned.map_err(|source| Error::Io {
            action: "start",
            path: program.to_path_buf(),
            source,
        })?;

        // Written and closed before anything waits: the end of file is what tells the program its
        // instruction is complete, and a pipe left open is a `mariadbd --bootstrap` sitting there for a
        // statement that will never come, until this call's patience runs out.
        if let Some(input) = input {
            let mut sink = child.stdin.take().ok_or_else(|| Error::Io {
                action: "write to the standard input of",
                path: program.to_path_buf(),
                source: std::io::Error::other("the child was given no standard input to write to"),
            })?;

            sink.write_all(input.as_bytes())
                .await
                .map_err(|source| Error::Io {
                    action: "write to the standard input of",
                    path: program.to_path_buf(),
                    source,
                })?;

            // Dropping the handle is what closes the pipe; flushed first so nothing is left buffered.
            let _ = sink.shutdown().await;
            drop(sink);
        }

        // Taken out of the child so that what the deadline below bounds is the process ending and
        // nothing else. `wait_with_output` would have waited on these too, which is the hazard this
        // function's documentation describes: end of file is the last holder of the pipe exiting.
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let waited = {
            // Both streams at once, and alongside the wait rather than after it — one at a time is not
            // enough, because whichever went unread would be the one the program filled and blocked on.
            let mut saying = std::pin::pin!(async {
                tokio::join!(
                    drain(out.as_mut(), &mut stdout),
                    drain(err.as_mut(), &mut stderr),
                );
            });
            let mut ending = std::pin::pin!(child.wait());
            let mut quiet = false;

            let waited = tokio::time::timeout(patience, async {
                loop {
                    tokio::select! {
                        // The only branch that ends this: the deadline is the process's.
                        status = &mut ending => break status,

                        // Guarded because a future that has finished must not be polled again, and this
                        // one usually finishes first — a well-behaved program closes its pipes as it
                        // exits, and most of them have already been closed for it by the kernel.
                        () = &mut saying, if !quiet => quiet = true,
                    }
                }
            })
            .await;

            // It has ended; its pipes may not have, and there is no telling how long they will take.
            if waited.is_ok() && !quiet {
                let _ = tokio::time::timeout(LAST_WORDS, saying).await;
            }

            waited
        };

        // Killed at the deadline by the drop at the end of this function, which `kill_on_drop` arranged.
        // What it managed to say on the way is kept: it was read while it ran, so it costs nothing here.
        let Ok(status) = waited else {
            return Ok(Ran::new(None, &stdout, &stderr));
        };

        let status = status.map_err(|source| Error::Os {
            action: "wait for a program it ran",
            source,
        })?;

        Ok(Ran::new(Some(describe(status)), &stdout, &stderr))
    }
}

/// Read everything a program says on one of its pipes, keeping whatever arrived before any error.
///
/// The [`Option`] is the child's own — [`tokio::process::Child`] holds each stream in one, and it is
/// [`None`] for a stream that was taken already.
///
/// A read error is dropped rather than raised, because what this call exists to produce is an exit
/// status: a short read on a pipe is no reason to withhold one, and the bytes that did arrive are
/// still the program's complaint.
#[cfg(not(windows))]
async fn drain<R>(pipe: Option<&mut R>, into: &mut Vec<u8>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    if let Some(pipe) = pipe {
        let _ = pipe.read_to_end(into).await;
    }
}

/// What a program printed, as something that can go in a log line.
///
/// Lossy on purpose: a probe's complaint is read by a person, and refusing to show it because a
/// database wrote a byte in the machine's own eight-bit encoding would lose the only evidence there
/// is. Trimmed because the last thing a program writes is a newline and a log line does not want it.
fn said(stream: &[u8]) -> String {
    String::from_utf8_lossy(stream).trim().to_owned()
}
