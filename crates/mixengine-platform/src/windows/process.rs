//! `DETACHED_PROCESS`, a process group of the child's own, and no inherited handles.
//!
//! A console is inherited on Windows, not merely written to: a child started from a terminal joins
//! that terminal's console, keeps it alive, and receives every control event sent to it.
//! `DETACHED_PROCESS` is the flag that says do none of that — the child gets no console, so closing
//! the window it was started from cannot reach it and it has nothing to print into.
//!
//! `CREATE_NEW_PROCESS_GROUP` on top of it is about `GenerateConsoleCtrlEvent`, which addresses a
//! process *group*: without it, a Ctrl-Break aimed at the group the parent belongs to would still be
//! delivered to a child that has no console of its own.
//!
//! The consequence is deliberate and is written down in `windows/signal.rs`: a daemon started this
//! way can never receive a console control event, because there is no console to send it one.
//!
//! **The third thing is the one that had to be found rather than designed.** `CreateProcessW` is
//! called by the standard library with `bInheritHandles = TRUE`, which it needs for the child's
//! stdio, and *inheritable* is a property that survives inheritance: a handle this process was
//! itself handed by its parent arrives still marked inheritable and is passed on again. So a
//! `mixengined --detach` started with its stdout on a pipe — which is exactly what a client
//! autostarting a daemon does, and what `std::process::Command::output` does — gave the daemon a
//! copy of that pipe. Redirecting the child's own stdio to the null device does not help: the extra
//! copy is not the child's stdout, it is simply a handle the child holds, and the writing end of a
//! pipe stays open while anybody holds one. The caller reading that pipe then waits for an
//! end-of-file that arrives when the daemon exits, days later. Reproduced before this was written,
//! as a `--detach` that returned promptly and a parent that never did.
//!
//! Clearing that flag is the only way to say it — `bInheritHandles` is per `CreateProcessW` and the
//! flag is per handle, so there is no third place to put "this one child gets nothing". What can be
//! narrowed is *how long* it is said for: the flag is put back as soon as the spawn returns, so a
//! caller that goes on to start other children — `mix`, or the GUI autostarting a daemon — passes
//! its stdio on to them exactly as it would have. The window that remains is the spawn itself, and
//! it is the same window `bInheritHandles` already makes process-wide for every concurrent
//! `CreateProcessW` in the program; this makes it no wider.
//!
//! # The other half: a child that must *not* outlive us
//!
//! Everything above is about letting go of a process. [`group`] is the opposite request, and on this
//! system it is the easy one: a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` takes its whole
//! membership down when the last handle to it closes, which a killed daemon does exactly as reliably
//! as an exiting one. No code of ours runs, and grandchildren are covered — neither of which is true
//! of the Unix side, and
//! `docs/decisions/0007-supervised-child-owns-a-process-group.md` is where that difference is
//! written down rather than averaged out.

use std::io;
use std::os::windows::io::{AsRawHandle as _, OwnedHandle};
use std::os::windows::process::CommandExt as _;
use std::process::Command;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, GetHandleInformation,
    HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS, GetProcessTimes, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess,
};

use crate::{Error, Result};

mod child;
mod job;

pub(crate) use job::{Group, group};

pub(crate) use child::{RawChild, find_program, run_child, spawn_child, spawn_shell_child};

/// One holder's share of "this process is not handing its standard handles on".
///
/// Held by [`detach`]'s caller across the spawn and dropped straight after it. Restoring in `Drop`
/// rather than in a function the caller has to remember is the point: a spawn that fails must put
/// the flags back too, and there is no path out of `spawn_detached` that skips a drop.
///
/// **A share and not the state itself, because the state is the process's.** The flag lives on a
/// handle this whole program shares, so two threads asking for it at once — a daemon starting a
/// service while a health probe runs its ten-second command, which is now the ordinary case — are
/// two holders of one thing rather than two independent guards. Held independently, the second
/// caller would find the flags already cleared and record nothing to restore, and the *first* one
/// dropping would hand the handles back out while the second was still spawning; the long-lived
/// service started in that window inherits the daemon's stdio and holds a client's pipe open for
/// days, which is the exact bug this module exists to prevent, made common instead of rare. So the
/// count is kept in [`HIDING`] and only the last holder out puts the flags back.
///
/// The private field carries no information and is not optional: without it this is a unit struct,
/// which anything in the crate can write for itself. Such a value counts as a holder when it drops
/// and never was one — the count goes below what is live, and in a release build it wraps to
/// `usize::MAX`, where the last real holder leaving finds a number above zero and puts the flags
/// back never. The only way to hold one is to have asked [`hide_stdio`] for it.
#[derive(Debug)]
pub(crate) struct Detaching(());

/// How many [`Detaching`] guards are alive, and what the first of them turned off.
///
/// Process-wide because the thing it describes is: `HANDLE_FLAG_INHERIT` is per handle, and these
/// three handles belong to the program rather than to any thread of it.
static HIDING: Mutex<Hiding> = Mutex::new(Hiding {
    holders: 0,
    restore: Vec::new(),
});

/// The state behind [`HIDING`].
#[derive(Debug)]
struct Hiding {
    /// Live guards. The flags stay off while this is above zero.
    holders: usize,

    /// Only the handles the *first* guard actually changed, as `usize` because a raw pointer is not
    /// `Send` and this outlives every thread that touches it. A handle that was already
    /// non-inheritable is left out, so nothing here can hand out an inheritance the process did not
    /// have to begin with.
    restore: Vec<usize>,
}

/// [`crate::process::without_a_window`] on this system.
///
/// The same flag `command::without_a_window` puts on the tools the platform layer runs itself, and
/// the same one `restricted` passes to `CreateProcessAsUserW` for a supervised child: the console
/// is still created, so the caller reads the child's output as usual, and no window is handed out
/// for it. Written out here rather than delegated because `command` is a `host`/`elevated` module
/// and this one is `process`, and a build of either feature without the other has to compile.
pub(crate) fn without_a_window(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Arrange for the child to have no console, no group in common with this process, and none of the
/// handles this process was given.
pub(crate) fn detach(command: &mut Command) -> Detaching {
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);

    hide_stdio()
}

/// The handle half of [`detach`], on its own.
///
/// Separate because the hazard is not the detached child's alone. Inheritance is transitive: a
/// client that starts `mixengined --detach` and reads it to end-of-file hands *its* standard handles
/// to that middle process, which hands them on again to the daemon — so the daemon ends up holding a
/// pipe two processes away, and the one reading it waits forever. Clearing the flag inside
/// `spawn_detached` cannot help there, because by then the copy has already been made. Every process
/// in a chain like that has to decline to pass its own handles on, which is what this is for.
/// Reentrant: the first caller clears the flags, every caller after it joins the one that is already
/// in force, and the last one out restores them. See [`Detaching`] for what goes wrong without that.
pub(crate) fn hide_stdio() -> Detaching {
    let mut hiding = HIDING.lock().unwrap_or_else(PoisonError::into_inner);

    if hiding.holders == 0 {
        hiding.restore = stop_handing_on_the_standard_handles();
    }

    hiding.holders += 1;

    Detaching(())
}

impl Drop for Detaching {
    fn drop(&mut self) {
        let mut hiding = HIDING.lock().unwrap_or_else(PoisonError::into_inner);

        hiding.holders -= 1;

        if hiding.holders > 0 {
            return;
        }

        for handle in std::mem::take(&mut hiding.restore) {
            #[expect(
                unsafe_code,
                reason = "SetHandleInformation only sets a flag on the handle it is given, which is \
                          one this process fetched from GetStdHandle and has not closed"
            )]
            unsafe {
                SetHandleInformation(handle as HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
            }
        }
    }
}

// **There is no `arrange` on this system, and its absence is the point.** A supervised child is
// created by `CreateProcessAsUserW` rather than by a `Command` — see
// [`restricted`](super::restricted) and
// `docs/decisions/0010-supervised-child-never-inherits-administrators.md` — so the flags that
// used to go on a `Command` are passed to that call directly. `CREATE_NO_WINDOW` is still among
// them, for the reason it always was: a daemon has no console of its own, so a console subsystem
// child would otherwise be given a new one, which is a black window on the user's desktop every
// time a service starts. Still deliberately **not** `CREATE_NEW_PROCESS_GROUP`, whose only purpose
// is to make a child addressable by `GenerateConsoleCtrlEvent`, which a daemon with no console
// cannot send — `docs/decisions/0008-no-signal-stop-on-windows.md`. The Unix counterpart keeps
// its `arrange`, because there the child is still started by a `Command`.

// **And no `arrange_one_shot` either, for the same reason.** A one-shot is created by
// [`run_child`] from the same restricted token — ADR 0010 — so its flags go on that call rather than
// on a `Command`. `CREATE_NO_WINDOW` is still among them, and it bites harder here than for a
// service: a health probe runs every ten seconds for as long as the service is up, so a console
// handed out per run would be a window opening on the user's desktop six times a minute, for ever.
// Still deliberately not `DETACHED_PROCESS`, which would take the child's console away *and* its
// inherited standard handles — and this call is made for the output those handles carry.

/// Whether a group can be *asked* to stop on this system, as opposed to being killed.
///
/// **False**, and this is the honest answer rather than a missing feature. A console control event
/// is the only signal-shaped thing Windows has, and it travels through a *console*: a supervised
/// child is started with `CREATE_NO_WINDOW` and the daemon that started it has no console at all, so
/// sending one would mean `FreeConsole`, `AttachConsole(child)`, disabling this process's own control
/// handler, `GenerateConsoleCtrlEvent`, and putting all three back — process-wide state, changed
/// from one thread of a daemon that is supervising other services on the others. The services that
/// need a graceful stop here have a command for it (`mariadb-admin shutdown`), which is what
/// `StopBehaviour::Command` is for. `docs/decisions/0008-no-signal-stop-on-windows.md` has the
/// alternatives that lost.
pub(crate) const CAN_ASK_TO_STOP: bool = false;

/// There are no signals here. See [`crate::process::CAN_SIGNAL`], and
/// `docs/decisions/0008-no-signal-stop-on-windows.md` for the alternatives that lost.
pub(crate) const CAN_SIGNAL: bool = false;

/// The variables a child is given even though its spec did not name them.
///
/// **The list is long here because Windows programs do not survive an empty environment.** A cleared
/// block costs a service `SystemRoot`, and everything that loads a system DLL by relative path —
/// Winsock, the crypto providers, the C runtime — fails to initialise with an error that names none
/// of this. On Unix the same list is nine entries and none of them is load-bearing; here the first
/// eight are, which is why the two are written out per OS rather than merged into one list with
/// `#[cfg]`s through it.
///
/// Everything after the loader's needs is the same session-not-service rule Unix uses: a temporary
/// directory, who the user is, where their profile lives. Inherited only when this process has them
/// and never invented, and a spec that names one overrides it.
pub(crate) const INHERITED_ENV: &[&str] = &[
    // The loader's, and not optional.
    "SystemRoot",
    "SystemDrive",
    "windir",
    "COMSPEC",
    "PATH",
    "PATHEXT",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    // The session's.
    "TEMP",
    "TMP",
    "USERPROFILE",
    "LOCALAPPDATA",
    "APPDATA",
    "PROGRAMDATA",
    "ALLUSERSPROFILE",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "PUBLIC",
    "HOMEDRIVE",
    "HOMEPATH",
    "USERNAME",
    "USERDOMAIN",
    "COMPUTERNAME",
    "OS",
];

/// What [`Group::terminate`] says when [`GROUP_GONE`] has passed with a member still running.
fn still_running() -> Error {
    Error::Os {
        action: "wait for a supervised process group to end after killing it",
        source: io::Error::new(
            io::ErrorKind::TimedOut,
            format!("a member was still running after {GROUP_GONE:?}"),
        ),
    }
}

/// How long [`Group::terminate`] waits for a killed job to empty.
///
/// A killed process ends in milliseconds; this is the bound for one that has I/O the kernel must
/// cancel first, and it is what keeps a stop from waiting for ever on a member that will not go.
const GROUP_GONE: Duration = Duration::from_secs(5);

/// When the process with this id began, as a `FILETIME` — 100-nanosecond ticks since 1601.
///
/// **Wall clock, so it identifies a process across a reboot as well as within one.** `GetProcessTimes`
/// reports the creation time the kernel recorded, which is the same number for the same process for
/// as long as it lives; a pid the OS has handed out again names a process created later and is
/// refused by the comparison the caller makes.
///
/// [`None`] rather than an error for every way of *not* being a running process, because that is the
/// answer the caller acts on and they are otherwise three different failures:
///
/// - the pid names nothing, which Windows reports as `ERROR_INVALID_PARAMETER` from `OpenProcess`;
/// - the pid names something this account may not ask about (`ERROR_ACCESS_DENIED`) — a service
///   account's process that was given a pid MixEngine once used. A process this daemon cannot even
///   query is not one it started, and answering "no" here is what keeps the caller from ever
///   signalling it;
/// - the process has ended and its object is still there because somebody holds a handle to it, in
///   which case the exit time is set. Windows keeps such an object openable by pid, so the exit time
///   is the only thing separating "it is running" from "it ran".
///
/// The handle is opened with `PROCESS_QUERY_LIMITED_INFORMATION`, which is the narrowest right that
/// answers the question and — unlike `PROCESS_QUERY_INFORMATION` — is granted for processes at a
/// higher integrity level.
pub(crate) fn started_at(pid: u32) -> Result<Option<i64>> {
    #[expect(
        unsafe_code,
        reason = "OpenProcess takes three integers and returns a handle this function owns and \
                  closes on every path"
    )]
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };

    if process.is_null() {
        let error = io::Error::last_os_error();

        return match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_INVALID_PARAMETER | ERROR_ACCESS_DENIED) => Ok(None),

            _ => Err(Error::Os {
                action: "ask the OS when a process began",
                source: error,
            }),
        };
    }

    let mut created = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut ended = created;
    let mut kernel = created;
    let mut user = created;

    #[expect(
        unsafe_code,
        reason = "the four pointers are to locals this frame owns and the handle is the one opened \
                  above; the call writes exactly four FILETIMEs and closes nothing"
    )]
    let asked = unsafe {
        GetProcessTimes(
            process,
            &raw mut created,
            &raw mut ended,
            &raw mut kernel,
            &raw mut user,
        )
    };

    let failure = (asked == 0).then(io::Error::last_os_error);

    #[expect(
        unsafe_code,
        reason = "the handle was opened by this function, is not used after this, and is closed \
                  exactly once"
    )]
    unsafe {
        CloseHandle(process);
    }

    if let Some(source) = failure {
        return Err(Error::Os {
            action: "ask the OS when a process began",
            source,
        });
    }

    // A process that is still running has no exit time at all, which is how a handle-kept corpse is
    // told from the process the caller is asking about.
    if ticks(ended) != 0 {
        return Ok(None);
    }

    // A `FILETIME` is under 2^63 for any date this side of the year 30000, so the sign is never in
    // question and the value is stored as it is.
    Ok(Some(ticks(created) as i64))
}

/// Every process's parent — see `crate::process::parent_table`.
///
/// One `CreateToolhelp32Snapshot` of the whole system, walked for `th32ParentProcessID`, where
/// `sysinfo` opens every process to read it. No process is opened here, so there is nothing this
/// account can be refused.
///
/// # Errors
///
/// [`Error::Os`] when the snapshot cannot be taken or its first entry cannot be read.
#[cfg(feature = "host")]
pub(crate) fn parent_table() -> Result<std::collections::BTreeMap<u32, u32>> {
    use std::os::windows::io::FromRawHandle as _;

    use windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let failed = |source| Error::Os {
        action: "list this machine's processes",
        source,
    };

    #[expect(
        unsafe_code,
        reason = "two integers in, and a handle out that is owned by an OwnedHandle at once"
    )]
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(failed(io::Error::last_os_error()));
    }

    #[expect(
        unsafe_code,
        reason = "the handle was just returned valid by CreateToolhelp32Snapshot and nothing else \
                  holds it, so this is its only owner and closes it once"
    )]
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };

    #[expect(
        unsafe_code,
        reason = "PROCESSENTRY32W is a repr(C) struct of integers and arrays of them, so all \
                  zeroes is a valid value for the kernel to overwrite"
    )]
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

    let mut table = std::collections::BTreeMap::new();

    #[expect(
        unsafe_code,
        reason = "the handle is the snapshot owned above and the pointer is to a local whose \
                  dwSize says how much of it may be written"
    )]
    let mut more = unsafe { Process32FirstW(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    if !more {
        let error = io::Error::last_os_error();
        if error.raw_os_error().map(|code| code as u32) != Some(ERROR_NO_MORE_FILES) {
            return Err(failed(error));
        }
    }

    while more {
        if entry.th32ProcessID != 0 && entry.th32ParentProcessID != 0 {
            table.insert(entry.th32ProcessID, entry.th32ParentProcessID);
        }

        #[expect(
            unsafe_code,
            reason = "the same snapshot and the same local as the first read"
        )]
        let next = unsafe { Process32NextW(snapshot.as_raw_handle(), &raw mut entry) };
        more = next != 0;
    }

    Ok(table)
}

/// One process's CPU time and working set — see `crate::process::usage`.
///
/// **The calls `sysinfo` makes, without the snapshot around them.** `GetProcessTimes` for CPU and
/// `GetProcessMemoryInfo`'s `WorkingSetSize` for memory, so the numbers are the same quantities a
/// `sysinfo` refresh reports. The handle is opened and closed here, and `None` covers the same three
/// cases [`started_at`] documents.
#[cfg(feature = "host")]
pub(crate) fn usage(pid: u32) -> Result<Option<crate::process::Usage>> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };

    #[expect(
        unsafe_code,
        reason = "OpenProcess takes three integers and returns a handle this function owns and \
                  closes on every path"
    )]
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };

    if process.is_null() {
        let error = io::Error::last_os_error();

        return match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_INVALID_PARAMETER | ERROR_ACCESS_DENIED) => Ok(None),

            _ => Err(Error::Os {
                action: "read what a process has spent",
                source: error,
            }),
        };
    }

    let mut created = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut ended = created;
    let mut kernel = created;
    let mut user = created;

    #[expect(
        unsafe_code,
        reason = "the four pointers are to locals this frame owns and the handle is the one opened \
                  above; the call writes exactly four FILETIMEs and closes nothing"
    )]
    let timed = unsafe {
        GetProcessTimes(
            process,
            &raw mut created,
            &raw mut ended,
            &raw mut kernel,
            &raw mut user,
        )
    };
    let mut failure = (timed == 0).then(io::Error::last_os_error);

    #[expect(
        unsafe_code,
        reason = "PROCESS_MEMORY_COUNTERS is a repr(C) struct of integers, so all zeroes is a valid \
                  value for the kernel to overwrite"
    )]
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;

    if failure.is_none() {
        #[expect(
            unsafe_code,
            reason = "the handle is the one opened above and the pointer is to a local whose cb \
                      says how much of it may be written"
        )]
        let measured = unsafe { K32GetProcessMemoryInfo(process, &raw mut counters, counters.cb) };
        failure = (measured == 0).then(io::Error::last_os_error);
    }

    #[expect(
        unsafe_code,
        reason = "the handle was opened by this function, is not used after this, and is closed \
                  exactly once"
    )]
    unsafe {
        CloseHandle(process);
    }

    if let Some(source) = failure {
        return Err(Error::Os {
            action: "read what a process has spent",
            source,
        });
    }

    if ticks(ended) != 0 {
        return Ok(None);
    }

    // A FILETIME tick is 100 nanoseconds.
    let spent = ticks(kernel)
        .saturating_add(ticks(user))
        .saturating_mul(100);

    Ok(Some(crate::process::Usage {
        cpu_time: std::time::Duration::from_nanos(spent),
        rss_bytes: counters.WorkingSetSize as u64,
    }))
}

/// The two halves of a `FILETIME` as the one number it is.
fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

/// Stop a process this one did not start, and is not the parent of.
///
/// **This process only, not a group**, and that is the honest half of adoption on Windows: the job
/// object that made a service's group killable belonged to the daemon that created it and went with
/// it. A survivor here exists at all only through the one-call-wide window between `CreateProcessW`
/// and `AssignProcessToJobObject` that
/// `docs/decisions/0007-supervised-child-owns-a-process-group.md` accepts, so it is a process
/// that had not yet been assigned to anything and, in every case that has ever been reproduced, has
/// not started children of its own either.
///
/// Exit code 1, for the same reason [`Group::terminate`] uses it: a process killed this way must not
/// be read afterwards as one that finished successfully.
///
/// A process that has already gone is success, not failure — the same rule Unix's `ESRCH` gets, and
/// what the caller wanted either way.
pub(crate) fn stop_foreign(pid: u32) -> Result<()> {
    #[expect(
        unsafe_code,
        reason = "OpenProcess takes three integers and returns a handle this function owns and \
                  closes on every path"
    )]
    let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };

    if process.is_null() {
        let error = io::Error::last_os_error();

        return match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_INVALID_PARAMETER) => Ok(()),

            _ => Err(Error::Os {
                action: "stop a process that outlived the daemon supervising it",
                source: error,
            }),
        };
    }

    #[expect(
        unsafe_code,
        reason = "the handle is the one opened above and the exit code is passed by value"
    )]
    let terminated = unsafe { TerminateProcess(process, 1) };

    let failure = (terminated == 0).then(io::Error::last_os_error);

    #[expect(
        unsafe_code,
        reason = "the handle was opened by this function, is not used after this, and is closed \
                  exactly once"
    )]
    unsafe {
        CloseHandle(process);
    }

    match failure {
        None => Ok(()),

        Some(source) => Err(Error::Os {
            action: "stop a process that outlived the daemon supervising it",
            source,
        }),
    }
}

/// There is no way to ask a process with no console to stop; see [`CAN_ASK_TO_STOP`].
///
/// Adoption inherits that answer rather than working around it: a survivor was started with
/// `CREATE_NO_WINDOW` by a daemon that is now gone, so there is even less of a console to reach it
/// through than there was before.
pub(crate) fn ask_foreign_to_stop(_pid: u32) -> Result<()> {
    Err(Error::UnsupportedPlatform {
        capability: "asking a process that outlived its supervisor to stop",
        reason: "Windows has no signal a daemon can send to a process it did not give a console to"
            .to_owned(),
    })
}

/// Nothing to do: the priority class went into the job object with the rest of the ceilings.
///
/// The counterpart on Unix is a real call, because there a priority is a property of processes and
/// not of the group object — see `unix/process.rs::set_priority`. Here the job already carries
/// `JOB_OBJECT_LIMIT_PRIORITY_CLASS`, so a second call would set the same thing twice.
pub(crate) const fn set_priority(_pid: u32, _priority: crate::process::Priority) {}

/// An all-zero job-object information struct.
///
/// Every one of them is `#[repr(C)]` and holds integers, unions of integers, or another such struct
/// — there is no reference, no pointer and no enum in any of them — so all zeroes is a valid value
/// and means "no limits set", which is what every field this code does not fill in has to say.
///
/// **Generic, because resource limits brought a second such struct:**
/// `JOBOBJECT_CPU_RATE_CONTROL_INFORMATION` beside `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`. The
/// caller names the type, and the sentence above is the safety condition for both.
#[expect(
    unsafe_code,
    reason = "zeroed is sound for a repr(C) struct of integers, and the alternative is naming ten \
              fields whose only correct value is 0"
)]
fn unsafe_zeroed_because_every_field_is_a_number<T>() -> T {
    unsafe { std::mem::zeroed() }
}

/// Clear `HANDLE_FLAG_INHERIT` on this process's standard handles, and say which ones that changed.
///
/// The three of them are the whole set worth clearing: everything else MixEngine opens is created
/// non-inheritable (the API pipe says so explicitly, and the standard library's files are), so the
/// only inheritable handles this process can be holding are the ones it was handed.
///
/// Failures are ignored on purpose. A missing standard handle — the normal state of a process that
/// is itself already detached — is not something to report, and a handle type that will not take the
/// call leaves this process no worse off than not having tried. What matters is that a handle only
/// joins the restore list once the call that cleared it has succeeded.
fn stop_handing_on_the_standard_handles() -> Vec<usize> {
    let mut cleared = Vec::new();

    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        #[expect(
            unsafe_code,
            reason = "GetStdHandle borrows nothing, and Get/SetHandleInformation read and clear a \
                      flag on the handle they are given; none of the three closes anything, and \
                      the only pointer written through is a u32 this frame owns"
        )]
        unsafe {
            let handle: HANDLE = GetStdHandle(id);

            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                continue;
            }

            // Asked rather than assumed, so the restore below cannot make a handle inheritable that
            // was not: a process launched with `bInheritHandles = FALSE` holds standard handles
            // nobody was ever meant to pass on.
            let mut flags = 0;

            if GetHandleInformation(handle, &mut flags) == 0 || flags & HANDLE_FLAG_INHERIT == 0 {
                continue;
            }

            if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) != 0 {
                cleared.push(handle as usize);
            }
        }
    }

    cleared
}

/// Nothing to sweep on this system.
///
/// A job object is a kernel object, not a directory: the last handle to it closing is what removes
/// it, and a killed daemon closes every handle it held. There is nothing on disk to sweep.
pub(crate) const fn sweep_stale_groups() {}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_JOB_MEMORY,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectCpuRateControlInformation,
        JobObjectExtendedLimitInformation, QueryInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::BELOW_NORMAL_PRIORITY_CLASS;

    use super::*;
    use crate::process::{Limits, Priority};

    #[cfg(feature = "host")]
    #[test]
    fn usage_reads_this_process() {
        let first = usage(std::process::id())
            .expect("this process can be read")
            .expect("this process is running");
        assert!(first.rss_bytes > 0, "{first:?}");

        // Spend some CPU so the second reading has something to show.
        let began = Instant::now();
        let mut spin = 0_u64;
        while began.elapsed() < std::time::Duration::from_millis(50) {
            spin = spin.wrapping_add(1);
        }
        std::hint::black_box(spin);

        let second = usage(std::process::id())
            .expect("this process can be read")
            .expect("this process is running");
        assert!(
            second.cpu_time > first.cpu_time,
            "{first:?} then {second:?}"
        );
    }

    #[cfg(feature = "host")]
    #[test]
    fn usage_of_a_process_that_has_exited_is_none() {
        // The Child keeps a handle, so the pid still names this exited process, not a stranger.
        let mut child = Command::new("cmd")
            .args(["/c", "exit", "0"])
            .spawn()
            .expect("cmd starts");
        child.wait().expect("cmd exits");

        assert_eq!(
            usage(child.id()).expect("an ended process is not an error"),
            None
        );
    }

    /// Read back what was written, rather than time a busy loop.
    ///
    /// **A CPU cap is a rate**, and asserting a rate means timing work on a shared CI runner —
    /// which the warm-start suite already taught this project to be careful with. What this test
    /// owes is that the right value reached the right object; that a cap *slows anything down* is
    /// for CI's budgets, whose `bench` job knows how to compare against master.
    ///
    /// A unit test rather than one in `tests/`, because the alternative is a `#[doc(hidden)]`
    /// accessor on `Supervised` that exists for no other reason — and the job is deliberately
    /// anonymous ([`group`]), so nothing outside this file can open it by name.
    #[test]
    fn a_job_object_carries_the_cap_that_was_written_to_it() {
        let group = group().expect("a job object can be created");

        group
            .set_limits(&Limits {
                cpu_percent: Some(50),
                memory_mb: Some(256),
                priority: Priority::Background,
            })
            .expect("a job object accepts a cap");

        let cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION =
            query(&group, JobObjectCpuRateControlInformation);
        let extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
            query(&group, JobObjectExtendedLimitInformation);

        // 50% of one core, expressed as hundredths of a percent of the whole machine.
        let expected = (50 * 100 / super::super::limits::cores()).max(1);

        #[expect(
            unsafe_code,
            reason = "reading the union's only member, which the flags above say is the one that was written"
        )]
        let rate = unsafe { cpu.Anonymous.CpuRate };

        assert_eq!(rate, expected);
        assert_eq!(extended.JobMemoryLimit, 256 * 1024 * 1024);
        assert_eq!(
            extended.BasicLimitInformation.PriorityClass,
            BELOW_NORMAL_PRIORITY_CLASS
        );
    }

    /// **Capping a job must not un-kill it**, and this is the regression test for the one way it could.
    ///
    /// `LimitFlags` is a single word describing every extended limit at once, so a memory cap written
    /// through a second code path would clear `KILL_ON_JOB_CLOSE` silently — and ADR 0007's guarantee
    /// that a killed daemon takes its services with it would be gone with nothing failing.
    #[test]
    fn capping_a_job_leaves_it_still_killing_its_members_when_it_closes() {
        let group = group().expect("a job object can be created");

        group
            .set_limits(&Limits {
                memory_mb: Some(64),
                ..Limits::default()
            })
            .expect("a job object accepts a cap");

        let extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
            query(&group, JobObjectExtendedLimitInformation);

        assert_ne!(
            extended.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            0,
            "a capped job still takes its members down with the daemon",
        );
    }

    /// Clearing a cap is a write, not a silent return — so a removed limit is really removed.
    ///
    /// **What "removed" means for CPU is this system's answer rather than ours**: rate control stays
    /// switched on, at a hundred per cent of the whole machine and with the hard cap off, because a
    /// write that switched it off is refused — see [`Group::set_cpu`]. The memory ceiling really does
    /// come off, because `LimitFlags` has a bit for it and clearing that bit is a legal write.
    #[test]
    fn clearing_a_cap_takes_the_previous_ceiling_off() {
        let group = group().expect("a job object can be created");

        group
            .set_limits(&Limits {
                cpu_percent: Some(25),
                memory_mb: Some(64),
                priority: Priority::Background,
            })
            .expect("a job object accepts a cap");
        group
            .set_limits(&Limits::default())
            .expect("a job object accepts an empty cap");

        let cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION =
            query(&group, JobObjectCpuRateControlInformation);
        let extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
            query(&group, JobObjectExtendedLimitInformation);

        #[expect(
            unsafe_code,
            reason = "reading the union member the flags above say was written"
        )]
        let rate = unsafe { cpu.Anonymous.CpuRate };

        assert_eq!(
            cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
            0,
            "the hard cap is off",
        );
        assert_eq!(rate, 10_000, "and what is left is the whole machine");
        assert_eq!(
            extended.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_JOB_MEMORY,
            0,
            "the memory ceiling is off again",
        );
        assert_ne!(
            extended.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            0,
            "and the one limit the job exists for survived both writes",
        );
    }

    /// Ask the kernel what a job object currently holds.
    fn query<T>(group: &Group, class: i32) -> T {
        let mut info: T = unsafe_zeroed_because_every_field_is_a_number();

        #[expect(
            unsafe_code,
            reason = "the pointer is to a local this frame owns and the length is that local's own \
                      size, so the kernel writes exactly the struct that is there"
        )]
        let read = unsafe {
            QueryInformationJobObject(
                group.job,
                class,
                std::ptr::from_mut(&mut info).cast(),
                size_of::<T>() as u32,
                std::ptr::null_mut(),
            )
        };

        assert_ne!(read, 0, "a job object this test owns can be queried");

        info
    }
}
