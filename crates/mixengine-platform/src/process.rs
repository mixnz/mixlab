//! Starting a process, and deciding whether it may outlive the one that started it.
//!
//! Two opposite requests live here, and the pair is the whole of the module:
//!
//! - [`spawn_detached`] starts a process this one is letting go of. `mixengined --detach` and a
//!   client's autostart (roadmap task T10) are made of it.
//! - [`spawn_supervised`] starts a process this one owns and intends to outlive. Every managed
//!   service is made of it (roadmap task T13).
//!
//! A third relationship arrives with crash recovery and starts no process at all: [`Adopted`] is one
//! that survived the daemon which started it, taken over by the daemon running now on the strength
//! of its pid *and* its [`StartTime`] (roadmap task T18). It is the weakest of the three — liveness
//! and a stop, and nothing else — and its documentation says where that is paid for.
//!
//! [`hand_over`] is the fourth relationship and the strangest: a process that stops being itself.
//! The shim (roadmap task T25) resolves which PHP a directory uses and then has to *become* it —
//! same arguments, same streams, same terminal, same exit code. On Unix that is `exec` and there is
//! nothing left to describe; on Windows there is no such call, so it is a child in a Job Object with
//! the console events swallowed on the way past. See the function for what that costs.
//!
//! [`run_once`] is the fifth and the odd one out: a program run **to completion**, for its exit
//! status rather than for its running. `mariadb-admin ping` and `caddy stop` are what it is for
//! (roadmap task T15a) — a probe and a shutdown, both of which start a process the caller has no
//! interest in supervising. It is here rather than in the supervisor for the reason every spawn is:
//! a `Command` on Windows has to be told not to be given a console window, and that is a `#[cfg]` no
//! crate above this one is allowed to contain.
//!
//! **The difference is stated in the destructors, because that is where a reader will meet it**:
//! dropping a [`Detached`] deliberately does not stop the child, and dropping a [`Supervised`]
//! deliberately does.
//!
//! Detaching is the part every OS does differently. Surviving the parent is the easy half and comes
//! for free on both systems; what has to be arranged is that the child stops being *attached* to
//! whoever started it:
//!
//! - it must not be reached by a Ctrl-C meant for the terminal the parent was typed into,
//! - it must not write into that terminal hours later, and
//! - it must not keep the terminal's console alive on Windows.
//!
//! The stdio redirection is the one piece that is genuinely shared, so it is done here: all three
//! streams go to the null device. A daemon that has detached says everything it has to say in
//! `logs/daemon.log`, and inheriting the parent's handles is how a background process ends up
//! printing into a shell prompt somebody is using.
//!
//! Supervising is the part every OS does differently *and* does with different force. A supervised
//! child leads a process group of its own — a Job Object on Windows, a session on Unix — so that
//! stopping it means stopping everything it started, and so that a daemon which goes away takes it
//! along. How completely that last part is true depends on the system and is set out in
//! `docs/decisions/0007-supervised-child-owns-a-process-group.md`: a kernel guarantee on Windows,
//! a guard that covers the immediate child on Linux, and nothing at all on macOS, where crash
//! recovery at the next boot (roadmap task T18) is what closes the gap.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::sys::process as sys;
use crate::{Error, Result};

mod adopted;
mod once;
mod table;

pub use adopted::Adopted;
#[cfg(feature = "host")]
pub(crate) use table::parent_table;
pub use table::{StartTime, started_at};
#[cfg(all(feature = "host", windows))]
pub(crate) use table::{Usage, usage};

#[cfg(windows)]
pub(crate) use once::LAST_WORDS;
pub use once::{Ran, run_once, run_once_with_input};

/// Whether a supervised group can be *asked* to stop on this system, rather than only killed.
///
/// True on Unix, where [`Supervised::ask_to_stop`] sends `SIGTERM` to the group. **False on
/// Windows**, where there is no signal a daemon can send to a process it gave no console to — see
/// `docs/decisions/0008-no-signal-stop-on-windows.md`.
///
/// A supervisor reads this before it starts a grace period: waiting out five seconds for a request
/// that was never sent is not patience, it is five seconds added to every stop. On a system that
/// says `false`, a service that has to shut down cleanly does it through a command of its own
/// (`StopBehaviour::Command`) and everything else is killed at once.
pub const CAN_ASK_TO_STOP: bool = sys::CAN_ASK_TO_STOP;

/// Whether a running process can be sent a signal on this system.
///
/// True on Unix, false on Windows, and for the reason [`CAN_ASK_TO_STOP`] is false there: a daemon
/// has no signal to send a process it gave no console to. Its own constant rather than a second
/// reading of that one, because they are two capabilities that happen to be absent together —
/// [`Supervised::ask_to_stop`] addresses a *group* and this addresses a *leader*, and a system that
/// gained one without the other would need to say so.
///
/// **A caller checks this before it waits**, not after. A reload that could never be delivered is a
/// line in the log at the moment it is asked for, rather than a patience spent on nothing.
pub const CAN_SIGNAL: bool = sys::CAN_SIGNAL;

/// A signal a running service can be sent.
///
/// This crate's own list rather than `mixengine_proto::ReloadSignal`: `mixengine-platform` depends
/// on no other crate in this workspace, and one enum is not a reason to open that edge — the daemon
/// holds both and maps one onto the other in three lines. The numbers stay inside `unix/process.rs`,
/// which is the only file in the workspace that may name a `libc` constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Signal {
    /// `SIGHUP`.
    Hup,

    /// `SIGUSR1`.
    Usr1,

    /// `SIGUSR2`.
    Usr2,
}

/// How a supervised service competes for CPU when nothing is capped.
///
/// **This crate's own rather than `mixengine_proto::Priority`, for [`Signal`]'s reason** and one
/// more of its own: the `process` feature does not enable `dep:mixengine-proto`, and
/// `mixengine-shim` compiles this module without it — so naming the proto type here would add proto
/// to the shim's dependency closure to describe a value the shim never has. The daemon holds both
/// and maps one onto the other in three lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    /// Competes with everything else the user is running.
    #[default]
    Normal,

    /// Yields to foreground work — a background QoS, a `nice` value, a below-normal priority class.
    Background,
}

/// A ceiling on what one supervised service may take.
///
/// [`Default`] is the ordinary state of a service nobody has capped: no ceiling on either resource,
/// and ordinary priority.
///
/// **What each field does on this machine is not answered here.** That is
/// [`ResourceControl`](crate::ResourceControl)'s, and the split is deliberate: this type is what the
/// caller *asked for*, and applying it is allowed to be a no-op on a system with no mechanism —
/// macOS has no hard memory cap, and a spawn there must succeed rather than refuse. See the T68
/// design, D6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Limits {
    /// A ceiling on CPU as a percentage of **one core**. `None` is uncapped.
    ///
    /// Per core rather than per machine, which is the unit `cpu.max` already uses and the unit a job
    /// object does not — `windows/process.rs` converts, and is the only place that does.
    pub cpu_percent: Option<u8>,

    /// A ceiling on memory in megabytes. `None` is uncapped.
    ///
    /// What "memory" means differs by system — see [`MemoryMeasure`](crate::MemoryMeasure), which is
    /// reported beside the number precisely because this one field cannot mean the same thing on a
    /// job object and in a cgroup.
    pub memory_mb: Option<u32>,

    /// How it competes for CPU.
    pub priority: Priority,
}

/// This process's standard handles, kept from every child started while it is held.
///
/// Returned by [`hide_stdio_from_children`]; puts them back when it drops.
#[derive(Debug)]
#[must_use = "the handles are passed on again the moment this is dropped"]
pub struct HiddenStdio(
    #[allow(
        dead_code,
        reason = "held for its Drop, which only Windows gives a body"
    )]
    sys::Detaching,
);

/// Keep this process's standard handles from reaching the children it starts, until the returned
/// guard is dropped.
///
/// **A caller that starts a detached process itself does not need this** — [`spawn_detached`] does
/// it already. What needs it is the caller one step further back: a client that starts
/// `mixengined --detach` and reads it to end-of-file. Inheritance on Windows is transitive, because
/// *inheritable* is a property of the handle and survives being inherited — so the client's stdout
/// reaches the middle process, and the middle process passes it on to the daemon before it has any
/// say in the matter. `spawn_detached` clears the flag on the handles the middle process owns, which
/// is one copy too late for the client's. The end-of-file the client is waiting for then arrives when
/// the *daemon* exits, days later.
///
/// Every process in a chain like that has to decline to pass its own handles on, and this is how one
/// says it: hold the guard across the spawn, drop it straight after. Held no longer than that, so a
/// program that goes on to start ordinary children — a `mix` running a hook, an editor — hands them
/// its stdio exactly as it would have.
///
/// Nothing to do on Unix, where only the three standard descriptors cross an `exec` and everything
/// else the standard library opens is `CLOEXEC`. The guard still exists there, so a caller has one
/// shape of code on all three systems.
pub fn hide_stdio_from_children() -> HiddenStdio {
    HiddenStdio(sys::hide_stdio())
}

/// Keep a child this process starts on its own — not through [`run_once`] or either spawn here —
/// from opening a console window.
///
/// **For the one-off program a crate above this one runs with a `Command` of its own**: the
/// privileged helper's probe, a freshly installed runtime's smoke test. Every spawn *inside* this
/// crate already says it; this is how a caller that has to build the `Command` itself — because it
/// needs `tokio`'s child, or a deadline `run_once` does not offer — says the same thing.
///
/// What goes wrong without it is measured, not reasoned about. A process that has no console — a
/// `--detach`ed `mixengined`, and so every daemon a client autostarts — gives a console-subsystem
/// child nothing to inherit, and Windows answers that by creating one. On Windows 11 a new console is
/// handed to the default terminal application, which opens a window for it: one `mix doctor` was
/// one Windows Terminal window flashing on the desktop for the `netsh` it runs, and every daemon
/// start another for the helper it probes. `CREATE_NO_WINDOW` is the answer for a child whose output
/// the caller reads: the console is still created, so the pipes work as usual, and no window is
/// handed out for it.
///
/// Nothing to do on either Unix, where a console window is not a thing a child can be given.
///
/// Takes the standard library's `Command` so that a tokio one is covered too — `as_std_mut` is the
/// bridge — and so this crate's `process` feature does not have to grow a second signature.
pub fn without_a_window(command: &mut Command) {
    sys::without_a_window(command);
}

/// Let go of a console this process is the only one attached to — roadmap task **T85b**.
///
/// **For the daemon a service manager starts.** A console-subsystem program launched with no console
/// to inherit is handed a new one by Windows, and on Windows 11 that is a terminal window on the
/// user's desktop — measured under Task Scheduler, where it is also `IsWindowVisible`. A console
/// nobody else is attached to is one Windows made rather than one somebody is reading, so this lets
/// it go, and the console host, having no attached process left, exits and takes the window with it.
/// The standard handles are pointed at the null device first, so nothing this process writes
/// afterwards fails.
///
/// **Does nothing on either Unix, and nothing on Windows when the console is shared** — a
/// `mixengined` run from a terminal keeps printing into it, which is how every developer uses it,
/// and a `--detach`ed one has no console to begin with.
///
/// Here rather than behind [`Host`](crate::Host) for this module's own reason: it is not a question
/// about the machine that a mock could answer from memory, it is a concrete handle being released.
///
/// Returns whether a console was actually released, which is the one thing the daemon logs.
#[must_use]
pub fn release_unattended_console() -> bool {
    crate::sys::console::release_unattended()
}

/// A process that has been started and let go.
///
/// Dropping it does **not** stop the child — neither system kills a process when its parent lets go
/// of the handle, which is the entire point. What the handle is still good for is the question the
/// caller asks while it waits for the child to come up: is it still there?
///
/// **A caller that goes on running has to reap it.** [`Child`] does not wait on drop, so on Unix the
/// process started here stays this process's child and becomes a zombie the moment it exits, for as
/// long as this process lives. `mixengined --detach` exits straight after the spawn and never meets
/// that; `mix` and the GUI autostarting a daemon (roadmap task T10) will, and have to keep the
/// handle and go on asking [`exited`](Self::exited) rather than dropping it once the daemon answers.
#[derive(Debug)]
pub struct Detached {
    child: Child,
}

/// How a detached process ended.
///
/// Two questions in one answer, because the caller needs both and they come from the same syscall.
/// [`is_success`](Self::is_success) is what a decision is made on — a `--detach` whose child exited
/// *successfully* because another daemon already held the home has not failed and must keep waiting
/// for that daemon, while one whose child failed has nothing left to wait for. The [`Display`](fmt::Display)
/// form is for the message to a person, and is rendered rather than reduced to a code because the
/// two systems disagree about what one is: a Unix child that died on a signal has no exit code at
/// all, and reporting `None` for it would lose the only interesting thing about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    success: bool,
    code: Option<i32>,
    described: String,
}

impl Detached {
    /// The child's process id, for a message to a person and for the lock file to be checked
    /// against.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Whether it has already exited, and how it put it.
    ///
    /// Never waits. A caller polling for a daemon to come up needs this to tell "not ready yet"
    /// from "gone", which are otherwise the same silence — and the second one is the answer that
    /// must not be waited out.
    ///
    /// # Errors
    ///
    /// [`Error::Os`] if the OS will not say. Not the same as "still running",
    /// which is `Ok(None)`.
    pub fn exited(&mut self) -> Result<Option<Exit>> {
        exited(&mut self.child)
    }
}

/// A process this one owns, and does not intend to be survived by.
///
/// The mirror image of [`Detached`], and the contrast is the point of both types existing. A
/// supervised child leads a process group of its own, so it is stopped as a whole — the php-fpm
/// master and every worker it forked, the shell script and the `mariadbd` it started — rather than
/// one pid at a time.
///
/// **Dropping this stops the child.** That is the opposite of [`Detached`] and is deliberate: this
/// value *is* the group's ownership, so a supervisor handle going out of scope while its processes
/// kept running would be an orphan produced by the one module that exists to prevent them. A child
/// that has already exited is left alone, so the ordinary path — a service that stopped by itself
/// and was waited for — kills nothing.
///
/// # What survives this process being killed
///
/// One of a child's output streams, whichever way that child was started.
///
/// **A type of this crate's rather than the standard library's, and T34a is why.** On Windows a
/// supervised child is created by `CreateProcessAsUserW` from a restricted token — see
/// `docs/decisions/0010-supervised-child-never-inherits-administrators.md` — and a
/// [`std::process::ChildStdout`] cannot be built from a handle that call returns. A [`File`] can, on
/// both systems, and a pipe read to end of file is the whole of what either caller wants.
///
/// Blocking, deliberately: the one consumer is `mixengine_supervisor::logs`, which reads each stream
/// on a thread of its own.
#[derive(Debug)]
pub struct OutputPipe(File);

impl OutputPipe {
    /// The pipe an OS handle already names.
    pub(crate) fn new(file: File) -> Self {
        Self(file)
    }
}

impl Read for OutputPipe {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buffer)
    }
}

/// Nothing on Windows: the job object takes the whole group down when the last handle to it closes,
/// which is a kernel guarantee and needs no code of ours to run. The immediate child on Linux, via
/// `PR_SET_PDEATHSIG`; anything *it* started is reparented and carries on. Everything on macOS,
/// which has neither mechanism. Crash recovery at the next boot (roadmap task T18) is what covers
/// the difference, and
/// `docs/decisions/0007-supervised-child-owns-a-process-group.md` is where it is written down
/// rather than rounded up.
#[derive(Debug)]
pub struct Supervised {
    child: sys::RawChild,

    /// The group the child leads. Held for its `Drop` on Windows, where closing the job handle is
    /// what kills the group.
    group: sys::Group,

    /// Whether the group has already been killed through this handle.
    ///
    /// What it prevents is a second `kill(-pgid, …)` after the leader has been reaped, which is the
    /// one moment a pgid stops being reliably ours: a process group exists while any member does,
    /// including a zombie, so an *unreaped* leader keeps the number reserved and a reaped one does
    /// not. Terminating before waiting is therefore always sound, and terminating a second time
    /// afterwards is the residual race ADR 0007 accepts — worth not running into twice for free.
    stopped: bool,
}

impl Supervised {
    /// Change what this service may take, now, while it is running.
    ///
    /// **There is no `pending, needs restart` state, deliberately.** Both mechanisms accept a
    /// rewrite with processes already inside them, so a limit that has been set is a limit that is
    /// in effect — which spares every reader of limits a flag to carry and every client a state to
    /// render. The T68 design, D7.
    ///
    /// **A service already over a newly-lowered memory cap can be killed by this call**, and that is
    /// the correct behaviour for the thing being asked for. The caller is expected to report the
    /// service's running state afterwards rather than assume it survived.
    ///
    /// The same code the spawn runs, so a limit applied to a running service and one applied at
    /// start cannot drift apart.
    ///
    /// # Errors
    ///
    /// When the mechanism rejects the values it was given. A system with no mechanism succeeds and
    /// does nothing — see [`Limits`].
    pub fn set_limits(&self, limits: &Limits) -> Result<()> {
        self.group.set_limits(limits)?;

        // The same second step the spawn takes, and for its reason: on Unix a priority belongs to
        // processes rather than to the group object, so it has to be aimed at a pid. A no-op on
        // Windows, where the job object already carries it.
        sys::set_priority(self.child.id(), limits.priority);

        Ok(())
    }

    /// The child's process id, which on Unix is also its process group id.
    ///
    /// Recorded together with the process start time by whoever persists it: a pid on its own is not
    /// an identity, because the number is reused. That pairing is what makes adoption after a daemon
    /// restart sound (roadmap task T18) — see [`started_at`](Self::started_at) for the other half.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// When this child began, for the row that has just recorded its pid.
    ///
    /// **Asked while the handle is held**, which is what makes the answer this child's rather than
    /// somebody else's: an unreaped child keeps its pid reserved on Unix, and on Windows this
    /// process holds a handle to it, so the number cannot have been given away between the spawn and
    /// this call.
    ///
    /// [`None`] for a child that has already ended — a service that failed in its first
    /// milliseconds — and that is the honest answer rather than a defect: what a null
    /// `pid_start_time` says is "this row cannot be identified later", and a row naming a process
    /// that is already gone is exactly one that must never be adopted.
    ///
    /// # Errors
    ///
    /// [`Error::Os`], or [`Error::Io`] on Linux, when the OS has the answer and would not give it.
    /// Not the same as "it has ended", which is `Ok(None)`.
    pub fn started_at(&self) -> Result<Option<StartTime>> {
        started_at(self.child.id())
    }

    /// Whether it has already exited, and how it put it. Never waits.
    ///
    /// Says nothing about the rest of the group: a service whose master process exited while a
    /// worker it forked is still running answers `Some` here. That is the honest answer for a
    /// restart policy, which is about the process it started, and it is why stopping goes through
    /// [`stop`](Self::stop) rather than through this.
    ///
    /// # Errors
    ///
    /// [`Error::Os`] if the OS will not say. Not the same as "still running", which is `Ok(None)`.
    pub fn exited(&mut self) -> Result<Option<Exit>> {
        exited(&mut self.child)
    }

    /// The child's standard output, taken once.
    ///
    /// Piped by [`spawn_supervised`] and **owed a reader**: a pipe holds tens of kilobytes, after
    /// which the service blocks on its next line and looks like it has hung. Log capture (roadmap
    /// task T16) is that reader; until it exists, a caller that does not take these streams should
    /// only supervise a process that says nothing.
    #[must_use]
    pub fn take_stdout(&mut self) -> Option<OutputPipe> {
        self.child.take_stdout()
    }

    /// The child's standard error, taken once. See [`take_stdout`](Self::take_stdout).
    #[must_use]
    pub fn take_stderr(&mut self) -> Option<OutputPipe> {
        self.child.take_stderr()
    }

    /// Ask the whole group to stop and give it a chance to tidy up.
    ///
    /// `SIGTERM` to `-pgid` on Unix. Returns as soon as the request is sent — how long the group is
    /// then given, and the [`stop`](Self::stop) that ends the wait, belong to the supervisor, which
    /// is the only thing that knows what the service's `StopBehaviour` asked for.
    ///
    /// **Check [`CAN_ASK_TO_STOP`] first.** On Windows there is no such request and this says so
    /// rather than succeeding quietly, because a grace period spent waiting for a message nobody
    /// sent is time added to every stop for nothing.
    ///
    /// Sent to the *group*, not to the leader, which is the whole point: the processes holding the
    /// port and the data directory are the workers, and a master that has already crashed cannot
    /// pass a signal on to them.
    ///
    /// # Errors
    ///
    /// [`Error::UnsupportedPlatform`] where the system has no way to ask, and [`Error::Os`] if it
    /// has one and refused. A group that has already gone is not a failure.
    pub fn ask_to_stop(&self) -> Result<()> {
        self.group.request_stop(self.child.id())
    }

    /// Send `signal` to the process this handle names.
    ///
    /// **To the leader and not to the group**, which is the whole difference between this and
    /// [`ask_to_stop`](Self::ask_to_stop). A stop is addressed at every process holding the port,
    /// because a master that has already crashed cannot pass one on. A reload is addressed at the
    /// master precisely because it has not crashed: php-fpm's `SIGUSR2` is an instruction to
    /// *replace the workers*, and the same signal delivered to a worker mid-request is that request
    /// dropped.
    ///
    /// **Check [`CAN_SIGNAL`] first.** On Windows there is no such thing and this says so rather
    /// than succeeding quietly, because a caller that believed it would go on waiting for an effect
    /// nothing was ever asked to produce.
    ///
    /// # Errors
    ///
    /// [`Error::UnsupportedPlatform`] where the system has no signals, and [`Error::Os`] if it has
    /// them and refused. A process that has already gone is not a failure.
    pub fn signal(&self, signal: Signal) -> Result<()> {
        self.group.signal_leader(self.child.id(), signal)
    }

    /// Kill the whole group, at once, without a chance to tidy up.
    ///
    /// `TerminateJobObject` on Windows, `SIGKILL` to `-pgid` on Unix. This is the ungraceful stop by
    /// construction: [`ask_to_stop`](Self::ask_to_stop) is the polite half, and this is what the
    /// grace period after it ends in.
    ///
    /// Reaps the child afterwards, so the pid is not left to a zombie on Unix — which also means the
    /// call returns only once the process this handle names is really gone. **On Windows every other
    /// member of the job is waited for too**, because a killed job's members leave in their own time
    /// and the one still leaving holds its files: `mysqld.exe` runs the real server as a child of a
    /// monitor, and a reset that started once the monitor alone had gone met `ibdata1` still held.
    /// On Unix the other members are not waited for, having never been this process's children.
    ///
    /// **The group is killed even when the leader has already exited**, and that is the correction
    /// T15 owed T13. A php-fpm master that crashed leaves its pool holding the port; a wrapper script
    /// that `exec`ed and died leaves the server it started. Skipping the kill because the process we
    /// named is gone would leave exactly the processes a restart is about to collide with — and
    /// "gone" is also the state a stop is *trying* to reach, so making it a precondition read the
    /// question backwards. What the old guard bought was not signalling a pgid the OS may have given
    /// away since; that window is the residual race
    /// `docs/decisions/0007-supervised-child-owns-a-process-group.md` already accepts, and this
    /// handle remembers that it has killed so it cannot enter that window twice.
    ///
    /// # Errors
    ///
    /// [`Error::Os`] if the OS refuses to stop the group, or cannot be waited on. A group that had
    /// already gone is not a failure.
    pub fn stop(&mut self) -> Result<Exit> {
        if !self.stopped {
            self.group.terminate(self.child.id())?;
            self.stopped = true;
        }

        self.wait()
    }

    /// Wait for the child to end, however it ends.
    ///
    /// # Errors
    ///
    /// [`Error::Os`] if the OS cannot be waited on for a process this one started.
    pub fn wait(&mut self) -> Result<Exit> {
        self.child.wait().map(describe).map_err(|source| Error::Os {
            action: "wait for the process it is supervising",
            source,
        })
    }
}

impl Drop for Supervised {
    /// Stop what this handle owns, on the way out.
    ///
    /// Failures are dropped because there is nowhere to report them from and nothing left to do
    /// about them. A handle that has already been [`stop`](Self::stop)ped kills nothing a second
    /// time; one whose child merely *exited* still kills the group, because what a destructor is
    /// for is the processes nobody is left holding — see [`stop`](Self::stop).
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Start `program` supervised by this process, with `directory` as its working directory and `env`
/// as — very nearly — its whole environment.
///
/// The child leads a process group of its own and is not meant to outlive this process. Both
/// streams are **piped**, because everything a managed service says has to reach
/// `logs/services/<id>/current.log` (roadmap task T16) — see
/// [`take_stdout`](Supervised::take_stdout) for the obligation that comes with them. `stdin` is the
/// null device: a service that reads from a terminal is one that hangs where nobody can see it.
///
/// `directory` is required rather than inherited for the same reason [`spawn_detached`] requires it
/// — a process's working directory is a reference the OS holds for the process's whole life.
///
/// # The environment is the caller's, not this process's
///
/// The opposite of [`spawn_detached`], which passes its own on. A `ServiceSpec` states its
/// environment in full, so a service behaves the same whether the daemon was started from a shell, a
/// login item or a scheduled task — and so that a variable the *user* exported, or one an installer
/// left behind, cannot change what a managed MariaDB does. `env` arrives already resolved: a
/// credential named by a spec has been fetched from the [`Keyring`](crate::Keyring) by whoever built
/// this map, and this function neither knows nor logs which of these values was one.
///
/// **A short per-OS floor is applied underneath it**, from this process's own environment and only
/// where this process has the variable: `PATH`, `HOME` and the locale on Unix, and on Windows the
/// eight or so names — `SystemRoot` first among them — without which a program cannot even load the
/// system DLLs it was linked against. They are the session's rather than the service's, nothing is
/// invented, and `env` overrides every one of them. `sys::INHERITED_ENV` is the list, per OS, with
/// the reasoning entry by entry.
///
/// # Errors
///
/// [`Error::Os`] if the process group cannot be created or the child cannot be put into it, and
/// [`Error::Io`] naming the program when it cannot be started at all. A child that was started and
/// could not then be adopted is killed **by pid** and waited for rather than returned or left
/// behind: it is a process nothing would own, and the group it was meant to belong to is not the
/// thing that can stop it — on Windows a failed adoption is exactly the case where the job is empty.
pub fn spawn_supervised(
    program: &Path,
    args: &[OsString],
    directory: &Path,
    env: &BTreeMap<String, String>,
    limits: &Limits,
) -> Result<Supervised> {
    supervise(limits, |group| {
        sys::spawn_child(program, args, directory, env, group)
    })
}

/// Start one command through this system's shell, supervised as a group.
///
/// **For a blueprint's `[scaffold]`** — roadmap task **T78a**, its design's D9 — and for nothing
/// else yet. `cmd.exe /C <command>` on Windows and `/bin/sh -c <command>` everywhere else: the
/// string is what somebody read and agreed to, and the shell is what makes `composer install && npm
/// ci` mean what whoever wrote the blueprint meant. Splitting it into a program and arguments here
/// would be a quoting rule of MixEngine's own, documented nowhere its author is looking.
///
/// Supervised rather than one-shot, for the group: `composer` starts children, and a cancellation
/// has to stop the tree rather than orphan it. The caller carries the same obligation
/// [`spawn_supervised`] documents — the pipes have to be read, or the command blocks on a full one
/// and looks like it has hung.
///
/// **On Windows the command is appended to the line verbatim** (D12). This crate builds the
/// `CreateProcess` line itself and quotes each argument the way `CommandLineToArgvW` parses it back,
/// which is the right rule for a program and the wrong one for `cmd.exe` — whose own parser does not
/// honour a backslash-escaped quote, so a command with a quote in it would arrive mangled.
///
/// # Errors
///
/// As [`spawn_supervised`].
pub fn spawn_shell_supervised(
    command: &str,
    directory: &Path,
    env: &BTreeMap<String, String>,
    limits: &Limits,
) -> Result<Supervised> {
    supervise(limits, |group| {
        sys::spawn_shell_child(command, directory, env, group)
    })
}

/// Where the shell would find `name` on `path`, or [`None`].
///
/// **For a plan to read a `[scaffold]` command's first word** — roadmap task **T78b**, its design's
/// D4 — and by the rule the shell that runs it would use, which is the only rule worth asking:
/// `cmd.exe` appends every `PATHEXT` extension and never runs a bare file, `execvp` wants an
/// execute bit and does not care what the name ends in. The caller supplies `path` so that the
/// answer is about the PATH the command would run with and not about this process's, and so that a
/// test can hand in a temporary directory.
///
/// An empty entry means the current directory to both shells and is skipped: the scaffold's current
/// directory is a project root that holds nothing yet.
#[must_use]
pub fn program_on_path(name: &str, path: &std::ffi::OsStr) -> Option<std::path::PathBuf> {
    sys::find_program(name, path)
}

/// The half both spawns share: a group, the caps on it before there is anything in it, the child,
/// and the adoption that can fail with a process already running.
fn supervise(
    limits: &Limits,
    spawn: impl FnOnce(&sys::Group) -> Result<sys::RawChild>,
) -> Result<Supervised> {
    let group = sys::group()?;

    // **Before the child exists, on both systems and for two different reasons.** On Windows the job
    // is capped while it still holds no processes, so a process is capped from the instant it is
    // assigned rather than a moment afterwards; on Linux the cgroup has to exist before the child's
    // `pre_exec` can write itself into its `cgroup.procs`. Neither system has anything useful to do
    // with a limit applied after the fact that it cannot also do here, so there is one ordering
    // rather than one per system.
    group.set_limits(limits)?;

    let child = spawn(&group)?;

    let mut supervised = Supervised {
        child,
        group,
        stopped: false,
    };

    // Adoption is the step that can fail on Windows, and the process it fails about is already
    // running — while being, by definition of the failure, *outside* the group. So this is the one
    // path the destructor cannot be left to: it would ask an empty job object to terminate, kill
    // nothing, and then wait for a child nothing had stopped, which for a service is forever. The
    // child is killed by pid instead, which is sound here for the reason it is not sound anywhere
    // else in this module — the pid was returned by a spawn a few lines above and has not been
    // reaped, so it is still this one process and cannot have been given away.
    //
    // Waited for rather than only signalled, so that what leaves this function is an error and not
    // also a zombie, and so the destructor below finds a child that has already ended and does
    // nothing at all.
    if let Err(error) = supervised.group.adopt(&supervised.child) {
        let _ = supervised.child.kill();
        let _ = supervised.child.wait();

        return Err(error);
    }

    // **After the spawn, unlike the ceilings above.** A priority is a property of processes on Unix
    // rather than of the group object, so there is nothing to set until there are processes — and
    // `PRIO_PGRP` needs the pgid, which after `setsid` is the pid this line finally has. Windows has
    // already done it: the priority class went into the job object with the rest of the caps.
    sys::set_priority(supervised.child.id(), limits.priority);

    Ok(supervised)
}

/// Remove whatever a daemon that was killed left behind of its supervised groups.
///
/// **Called once, at daemon start**, beside the stale socket and pidfile cleanup roadmap task T18
/// put there. Empty on Windows and macOS and real only on Linux, where a group is a *directory*: a
/// job object goes away when the last handle to it closes, which a killed process does for free,
/// and macOS makes no group object at all.
///
/// Takes no argument and reports nothing, because there is no decision here for a caller to make. A
/// cgroup that still holds a process cannot be removed, and one that still holds a process belongs
/// to a service this daemon is about to adopt — so "what could not be swept" is not a fault, it is
/// the recovery that is about to happen.
pub fn sweep_stale_groups() {
    sys::sweep_stale_groups();
}

/// Give this child the environment `env` states, over the short per-OS floor and nothing else.
///
/// Shared by [`spawn_supervised`] and [`run_once`], and it has to be: a `mariadb-admin ping` that
/// saw a different environment from the `mariadbd` it is asking about would be asking about a
/// different server — a different socket, a different data directory, a different credential.
/// Duplicating the rule would let those two drift apart one edit at a time.
///
/// **Computed rather than applied**, because the two spawn paths no longer share a [`Command`]:
/// Windows builds an environment block by hand for `CreateProcessAsUserW`. Stating the rule once
/// here is what stops a probe and the server it is asking about from drifting apart.
///
/// Cleared first, so what follows is the whole of it. The floor goes on before the caller's own
/// entries, which is what makes a spec able to override one — and on Windows that comparison is
/// case-insensitive, so a spec naming `Path` replaces the inherited `PATH` rather than adding a
/// second variable the child would see only one of. That is what `Command` did for free and what
/// this has to do on purpose.
pub(crate) fn whole_environment(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut whole = BTreeMap::new();

    for name in sys::INHERITED_ENV {
        if let Some(value) = std::env::var_os(name) {
            whole.insert((*name).to_owned(), value.to_string_lossy().into_owned());
        }
    }

    for (name, value) in env {
        if cfg!(windows) {
            whole.retain(|held, _| !held.eq_ignore_ascii_case(name));
        }

        whole.insert(name.clone(), value.clone());
    }

    whole
}

/// [`whole_environment`], put on a [`Command`] — the shape the `tokio` one-shot still wants.
#[cfg(not(windows))]
fn state_the_whole_environment(command: &mut Command, env: &BTreeMap<String, String>) {
    command.env_clear();
    command.envs(whole_environment(env));
}

/// Become `program` — moved to [`crate::handover`] with T185, and kept here for every caller that
/// already has `process`.
pub use crate::handover::hand_over;

/// A child this process can ask whether it has ended yet.
///
/// Two types answer it and neither can be the other: a [`Detached`] daemon is a plain [`Child`],
/// while a supervised service is `sys::RawChild`, which on Windows is created from a restricted
/// token and is not a [`Child`] at all. The question, though, is the same one, and so is the answer
/// — hence one trait with one method rather than two copies of [`exited`].
trait Asked {
    /// Whether it has ended, without waiting for it to.
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>>;
}

impl Asked for Child {
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        Child::try_wait(self)
    }
}

impl Asked for sys::RawChild {
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        sys::RawChild::try_wait(self)
    }
}

/// Whether a child has exited, and how it put it.
///
/// Shared by both handles: the question is the same one whether this process is going to outlive the
/// child or the other way round.
fn exited(child: &mut impl Asked) -> Result<Option<Exit>> {
    child
        .try_wait()
        .map(|status| status.map(describe))
        .map_err(|source| Error::Os {
            action: "check on the process it started",
            source,
        })
}

/// Render an exit status into the answers a caller needs from it.
pub(crate) fn describe(status: std::process::ExitStatus) -> Exit {
    Exit {
        success: status.success(),
        code: status.code(),
        described: status.to_string(),
    }
}

impl Exit {
    /// Whether it ended the way a process that did its job ends.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.success
    }

    /// The status it exited with, where the OS reports one.
    ///
    /// `None` for a Unix process killed by a signal, which has no exit code at all — reporting `0`
    /// for one would say "clean exit" about a crash, and that is why the wire type
    /// (`StateReason::Exited`) carries an `Option` too rather than flattening it. The
    /// [`Display`](fmt::Display) form is what to show a person; this is what a policy branches on.
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        self.code
    }
}

impl fmt::Display for Exit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.described)
    }
}

/// Start `program` detached from this process and from its terminal, with `directory` as its
/// working directory.
///
/// The environment is inherited, with `extra_env` applied on top. Inheriting is deliberate:
/// `MIXENGINE_LOG_FORMAT` is set by a log collector that wraps a command it did not write, and a
/// child that dropped it would stop being collected halfway through a start. `extra_env` exists for
/// the one caller that adds something — this install's window handed a credential (roadmap task
/// T83, through [`crate::DesktopApps`]) — and is empty for every other.
///
/// **`directory` is required rather than inherited**, and the caller is expected to name something
/// it is happy to have held for the child's whole life: a process's working directory is a reference
/// the OS keeps, so a daemon that kept its caller's would stop that directory from being renamed or
/// deleted on Windows and stop its filesystem from being unmounted on Unix. A client autostarting a
/// daemon is typically run from a project folder the person is working in, which is the last
/// directory worth pinning for days. The daemon's own home is the obvious answer.
///
/// # What this does to the calling process on Windows
///
/// It clears `HANDLE_FLAG_INHERIT` on **this process's** standard handles for the length of the
/// spawn, because there is no narrower way to keep the child from inheriting a pipe its caller is
/// reading to end-of-file — `windows/process.rs` has the whole of why. They are put back before this
/// returns, so a caller that goes on to start other children passes its stdio on to them as usual;
/// what remains is that a `CreateProcessW` running *concurrently* on another thread may not inherit
/// them, which is a window `bInheritHandles` already opens for every spawn in the program.
///
/// # Errors
///
/// [`Error::Io`] naming the program when it cannot be started — the usual reasons
/// being a binary that has been moved since this process was launched, or one an antivirus is
/// holding.
pub fn spawn_detached(
    program: &Path,
    args: &[OsString],
    directory: &Path,
    extra_env: &BTreeMap<String, String>,
) -> Result<Detached> {
    let mut command = Command::new(program);

    command
        .args(args)
        .current_dir(directory)
        .envs(extra_env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Held across the spawn and no further: on Windows detaching a child means turning something
    // off on *this* process, and the drop below is what turns it back on — including when the spawn
    // is the thing that failed.
    let detaching = sys::detach(&mut command);
    let spawned = command.spawn();
    drop(detaching);

    spawned
        .map(|child| Detached { child })
        .map_err(|source| Error::Io {
            action: "start",
            path: program.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests;
