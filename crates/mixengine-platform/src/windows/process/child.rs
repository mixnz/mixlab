//! Starting a child on Windows and reading what it prints: `CreateProcessW` with no handles
//! inherited but the three it is given, a program found by `PATHEXT` the way `cmd` finds
//! one, and its pipes read on threads of their own so neither can fill and stall the other.

use std::fs::File;
use std::io;
use std::io::{Read as _, Write as _};
use std::os::windows::io::{AsRawHandle as _, OwnedHandle};
use std::os::windows::process::ExitStatusExt as _;
use std::path::Path;
use std::process::ExitStatus;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, INFINITE, TerminateProcess, WaitForSingleObject,
};

use super::Group;
use crate::process::LAST_WORDS;
use crate::{Error, Result};

/// A supervised child, created from a restricted copy of this process's token.
///
/// **Not a [`std::process::Child`]**, and it cannot be: `CreateProcessAsUserW` hands back a raw handle and the
/// standard library offers no way to build a [`std::process::Child`] from one. See [`restricted`]
/// for the call, and `docs/decisions/0010-supervised-child-never-inherits-administrators.md` for
/// why the token is restricted at all.
///
/// [`restricted`]: super::super::restricted
#[derive(Debug)]
pub(crate) struct RawChild {
    process: OwnedHandle,
    pid: u32,
    stdout: Option<crate::process::OutputPipe>,
    stderr: Option<crate::process::OutputPipe>,

    /// Remembered, because a handle stays signalled for ever once the process ends and a caller
    /// that asks twice should get one answer — which is what [`std::process::Child`] does too.
    ended: Option<ExitStatus>,
}

impl RawChild {
    /// The child's process id.
    pub(crate) fn id(&self) -> u32 {
        self.pid
    }

    /// Whether it has ended, without waiting for it to.
    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.finished(0)
    }

    /// Wait for it to end, however long that takes.
    pub(crate) fn wait(&mut self) -> io::Result<ExitStatus> {
        loop {
            if let Some(ended) = self.finished(INFINITE)? {
                return Ok(ended);
            }
        }
    }

    /// Kill it. Exit code 1, so a killed process is not read as one that succeeded.
    pub(crate) fn kill(&mut self) -> io::Result<()> {
        if self.ended.is_some() {
            return Ok(());
        }

        #[expect(
            unsafe_code,
            reason = "the handle is owned by this value and is not closed by the call"
        )]
        let killed = unsafe { TerminateProcess(self.process.as_raw_handle().cast(), 1) };

        if killed == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    /// Its standard output, taken once.
    pub(crate) fn take_stdout(&mut self) -> Option<crate::process::OutputPipe> {
        self.stdout.take()
    }

    /// Its standard error, taken once.
    pub(crate) fn take_stderr(&mut self) -> Option<crate::process::OutputPipe> {
        self.stderr.take()
    }

    /// The process handle, for the job object that has to adopt it.
    pub(crate) fn raw_handle(&self) -> HANDLE {
        self.process.as_raw_handle().cast()
    }

    /// `WaitForSingleObject` for `patience` milliseconds, and the status if it ended.
    fn finished(&mut self, patience: u32) -> io::Result<Option<ExitStatus>> {
        if let Some(ended) = self.ended {
            return Ok(Some(ended));
        }

        #[expect(
            unsafe_code,
            reason = "the handle is owned by this value and neither call closes it; the exit code \
                      is written into a local this frame owns"
        )]
        let ended = unsafe {
            let handle: HANDLE = self.process.as_raw_handle().cast();

            match WaitForSingleObject(handle, patience) {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => return Ok(None),
                _ => return Err(io::Error::last_os_error()),
            }

            let mut code: u32 = 0;

            if GetExitCodeProcess(handle, &raw mut code) == 0 {
                return Err(io::Error::last_os_error());
            }

            ExitStatus::from_raw(code)
        };

        self.ended = Some(ended);

        Ok(Some(ended))
    }
}

/// Start a supervised child from a restricted token: both streams piped, standard input the null
/// device.
///
/// The environment arrives already composed — see
/// [`whole_environment`](crate::process::whole_environment).
///
/// # Errors
///
/// [`Error::Os`] when the token or the pipes cannot be made, and [`Error::Io`] naming the program
/// when it cannot be started at all.
pub(crate) fn spawn_child(
    program: &Path,
    args: &[std::ffi::OsString],
    directory: &Path,
    env: &std::collections::BTreeMap<String, String>,
    // Unused here and load-bearing on Unix, where the child writes itself into the group's cgroup
    // between `fork` and `exec`. A job object is joined *after* the spawn instead — see
    // [`Group::adopt`] — so this system has nothing to hand the child on the way in.
    _group: &Group,
) -> Result<RawChild> {
    let spawned = super::super::restricted::spawn(
        program,
        args,
        directory,
        &crate::process::whole_environment(env),
        None,
    )?;

    Ok(RawChild {
        process: spawned.process,
        pid: spawned.pid,
        stdout: Some(crate::process::OutputPipe::new(spawned.stdout)),
        stderr: Some(crate::process::OutputPipe::new(spawned.stderr)),
        ended: None,
    })
}

/// [`spawn_child`], with the command handed to `cmd.exe /C` — roadmap task **T78a**.
///
/// **The command is the line's tail, verbatim** (the T78a design, D12): `cmd.exe` parses its own
/// command line by rules that are not `CommandLineToArgvW`'s, and a command containing a quote put
/// through this crate's ordinary quoting would reach the shell mangled.
///
/// `cmd.exe` is taken from `%SystemRoot%` rather than found on a `PATH`, which is what
/// `services/step.rs` already does and for the same reason: the shell that runs somebody's command
/// should not itself be something a `PATH` decided.
pub(crate) fn spawn_shell_child(
    command: &str,
    directory: &Path,
    env: &std::collections::BTreeMap<String, String>,
    _group: &Group,
) -> Result<RawChild> {
    let mut tail = std::ffi::OsString::from("/C ");
    tail.push(command);

    let spawned = super::super::restricted::spawn_raw(
        &shell(),
        &tail,
        directory,
        &crate::process::whole_environment(env),
    )?;

    Ok(RawChild {
        process: spawned.process,
        pid: spawned.pid,
        stdout: Some(crate::process::OutputPipe::new(spawned.stdout)),
        stderr: Some(crate::process::OutputPipe::new(spawned.stderr)),
        ended: None,
    })
}

/// What `cmd.exe` appends when `PATHEXT` is unset — its own compiled-in default.
const DEFAULT_PATHEXT: &[&str] = &[".COM", ".EXE", ".BAT", ".CMD"];

/// The extensions `cmd.exe` would try, in its order.
fn pathext() -> Vec<String> {
    std::env::var("PATHEXT")
        .ok()
        .map(|held| {
            held.split(';')
                .filter(|one| !one.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|found| !found.is_empty())
        .unwrap_or_else(|| {
            DEFAULT_PATHEXT
                .iter()
                .map(|one| (*one).to_owned())
                .collect()
        })
}

/// [`crate::process::program_on_path`], by `cmd.exe`'s rule.
///
/// A name with an extension of its own is tried as written first; every name is then tried with
/// each `PATHEXT` extension appended. A bare file with no extension is never a hit — the file a
/// person copies from a Unix machine, and the one `cmd.exe` says it does not recognise.
pub(crate) fn find_program(name: &str, path: &std::ffi::OsStr) -> Option<std::path::PathBuf> {
    let extensions = pathext();

    std::env::split_paths(path)
        .filter(|directory| !directory.as_os_str().is_empty())
        .find_map(|directory| {
            let as_written = name.contains('.').then(|| directory.join(name));
            let with_extension = extensions
                .iter()
                .map(|extension| directory.join(format!("{name}{extension}")));

            as_written
                .into_iter()
                .chain(with_extension)
                .find(|candidate| candidate.is_file())
        })
}

/// Where this system's command interpreter is.
///
/// `%SystemRoot%` when the variable is there and the well-known path when it is not — a machine
/// whose `SystemRoot` is unset is a machine where `C:\Windows` is the answer anyway.
fn shell() -> std::path::PathBuf {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());

    std::path::PathBuf::from(root)
        .join("System32")
        .join("cmd.exe")
}

/// A one-shot, from a restricted token, with the whole of it on a blocking thread.
///
/// **What is lost against the `tokio` path, stated rather than discovered**: `kill_on_drop`. A
/// caller that gives up on this future does not kill the child — the blocking task goes on and ends
/// it at its own deadline instead. So the promise is weaker in one word and no weaker in effect: no
/// process outlives `patience`, but the moment it dies is the deadline rather than the drop.
///
/// The two pipes are read on threads of their own, and for the reason the async path reads them
/// alongside the wait: a pipe holds a page or two, and a program with a screen of complaint blocks
/// on its own write until somebody reads.
///
/// # Errors
///
/// [`Error::Os`] when the token or the pipes cannot be made or the wait fails, and [`Error::Io`]
/// naming the program when it cannot be started or cannot be written to.
pub(crate) async fn run_child(
    program: &Path,
    args: &[std::ffi::OsString],
    directory: &Path,
    env: &std::collections::BTreeMap<String, String>,
    patience: std::time::Duration,
    input: Option<String>,
) -> Result<crate::process::Ran> {
    let program = program.to_path_buf();
    let args = args.to_vec();
    let directory = directory.to_path_buf();
    let env = crate::process::whole_environment(env);

    tokio::task::spawn_blocking(move || {
        let mut spawned = super::super::restricted::spawn(
            &program,
            &args,
            &directory,
            &env,
            input.as_ref().map(|_| ()),
        )?;

        // Written and closed before anything waits: the end of file is what tells the program its
        // instruction is complete, and a pipe left open is a `postgres --single` sitting there for a
        // statement that will never come, until this call's patience runs out.
        if let Some(text) = input {
            let mut sink = spawned.stdin.take().ok_or_else(|| Error::Io {
                action: "write to the standard input of",
                path: program.clone(),
                source: io::Error::other("the child was given no standard input to write to"),
            })?;

            sink.write_all(text.as_bytes())
                .map_err(|source| Error::Io {
                    action: "write to the standard input of",
                    path: program.clone(),
                    source,
                })?;

            drop(sink);
        }

        let out = read_on_a_thread(spawned.stdout);
        let err = read_on_a_thread(spawned.stderr);

        let mut child = RawChild {
            process: spawned.process,
            pid: spawned.pid,
            stdout: None,
            stderr: None,
            ended: None,
        };

        let patience = u32::try_from(patience.as_millis()).unwrap_or(u32::MAX);
        let ended = child.finished(patience).map_err(|source| Error::Os {
            action: "wait for a program it ran",
            source,
        })?;

        if ended.is_none() {
            let _ = child.kill();
        }

        // **Bounded, and this is the regression that found out why.** End of file on a pipe is not
        // the process exiting, it is the *last holder of that pipe* exiting — so a one-shot that
        // left a helper behind holds its own stdout open through that helper for as long as it
        // lives, and a join here would wait out the helper rather than the program. What is waited
        // for is the same `LAST_WORDS` the other system's path waits, from one deadline rather than
        // two, and what is kept is whatever had arrived by then.
        let deadline = Instant::now() + LAST_WORDS;
        let stdout = out.taken(deadline);
        let stderr = err.taken(deadline);

        Ok(crate::process::Ran::new(
            ended.map(crate::process::describe),
            &stdout,
            &stderr,
        ))
    })
    .await
    .map_err(|source| Error::Os {
        action: "wait for a program it ran",
        source: io::Error::other(source),
    })?
}

/// One of a one-shot's output streams, being read on a thread of its own.
///
/// **Shared rather than returned**, because the reader may still be running when the caller has to
/// answer: a helper the program left behind holds the pipe open, and end of file then means the
/// helper exiting rather than the program. So the bytes accumulate where the caller can take them
/// at a deadline — see [`taken`](Self::taken) — and the thread is left to end on its own.
struct Reading {
    /// What has arrived so far.
    into: std::sync::Arc<Mutex<Vec<u8>>>,

    /// Signalled once, when the reader reaches end of file or gives up.
    done: std::sync::mpsc::Receiver<()>,
}

impl Reading {
    /// What arrived, waiting no longer than `deadline` for the rest.
    fn taken(self, deadline: Instant) -> Vec<u8> {
        let _ = self
            .done
            .recv_timeout(deadline.saturating_duration_since(Instant::now()));

        self.into
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Read one pipe on a thread, keeping whatever arrived before any error.
///
/// A read error ends the thread rather than being raised, for the reason the other system's `drain`
/// drops one: what this call exists to produce is an exit status, and the bytes that did arrive are
/// still the program's complaint.
///
/// In chunks rather than [`std::io::Read::read_to_end`], so that a caller taking the answer at a deadline
/// gets what had arrived by then instead of nothing at all.
fn read_on_a_thread(mut pipe: File) -> Reading {
    let into = std::sync::Arc::new(Mutex::new(Vec::new()));
    let (finished, done) = std::sync::mpsc::channel();
    let writing = std::sync::Arc::clone(&into);

    std::thread::spawn(move || {
        let mut chunk = [0_u8; 8 * 1024];

        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => writing
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(&chunk[..read]),
            }
        }

        // Nobody may be listening any more, which is the ordinary case for a slow helper.
        let _ = finished.send(());
    });

    Reading { into, done }
}
