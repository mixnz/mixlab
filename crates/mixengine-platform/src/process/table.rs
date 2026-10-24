//! What the operating system says about a process by its id: when it began, who started it, and
//! what it has spent.

use std::collections::BTreeMap;

use crate::Result;
use crate::sys::process as sys;

/// When a process began, in whatever this operating system counts such moments in.
///
/// **Opaque, and only ever compared for equality.** A `FILETIME` on Windows, microseconds since the
/// epoch on macOS, clock ticks since boot on Linux: three different units answering one question,
/// which is *is the process bearing this pid still the one I recorded*. Nothing may render it, do
/// arithmetic on it, or compare two of them for order — a value from one machine means nothing on
/// another, and on Linux a value from one boot means nothing in the next.
///
/// It is stored, so it crosses a process boundary as an integer and comes back as one: `services.pid_start_time`
/// holds exactly [`stored`](Self::stored), which `docs/architecture/data-model.md` describes as a
/// column that "exists to be compared, never read".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartTime(i64);

impl StartTime {
    /// The number to put in a column, and nothing a person should be shown.
    #[must_use]
    pub fn stored(self) -> i64 {
        self.0
    }

    /// A value read back out of one.
    ///
    /// Deliberately total: any integer is a start time this build might once have written, and a
    /// row that holds a value no live process has simply fails the comparison — which is the answer
    /// "that process is gone", arrived at without a special case.
    #[must_use]
    pub const fn from_stored(stored: i64) -> Self {
        Self(stored)
    }
}

/// When the process with this id began, or [`None`] if there is no such *running* process.
///
/// The other half of an identity, and the reason a pid alone is never enough: the OS reuses a pid
/// within minutes, so a supervisor that acted on one could ask an unrelated program to shut down.
/// Everything that is not a running process — a pid nobody holds, one this account may not ask
/// about, a process that has ended and not been reaped — is [`None`] rather than an error, because
/// they are one answer to the caller: *not the process you recorded*, and therefore not one to
/// signal.
///
/// # Errors
///
/// [`Error::Os`], or [`Error::Io`] on Linux, when the OS has the answer and would not give it — the
/// case that is neither "here it is" nor "there is no such process".
///
/// [`Error::Os`]: super::Error::Os
/// [`Error::Io`]: super::Error::Io
pub fn started_at(pid: u32) -> Result<Option<StartTime>> {
    sys::started_at(pid).map(|started| started.map(StartTime))
}

/// Who started each process on this machine, as `pid → parent pid`.
///
/// **The cheap half of a metrics reading.** Walking a service's group needs every process's parent
/// and nothing else, and reading only that is a fraction of what refreshing every process costs —
/// on macOS most of all, where `sysinfo` reads each process's arguments on every refresh. A process
/// this account may not ask about, or one that ended during the read, is simply absent; so is one
/// with no parent.
///
/// # Errors
///
/// [`Error::Os`], or [`Error::Io`] on Linux, when the list of processes itself could not be read.
/// The metrics sampler then refreshes the whole table the way it did before this existed.
///
/// [`Error::Os`]: super::Error::Os
/// [`Error::Io`]: super::Error::Io
#[cfg(feature = "host")]
pub(crate) fn parent_table() -> Result<BTreeMap<u32, u32>> {
    sys::parent_table()
}

/// What one process has spent: its CPU time so far and its resident size now.
#[cfg(all(feature = "host", windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Usage {
    /// Kernel and user time together, since the process began.
    pub(crate) cpu_time: std::time::Duration,

    /// The working set, which is what `sysinfo` reports as a process's memory on this system.
    pub(crate) rss_bytes: u64,
}

/// One process's [`Usage`], read without listing the machine.
///
/// Windows only: there `sysinfo` takes a snapshot of every process even when asked for a few, and
/// that snapshot was the whole cost of a metrics tick. [`None`] for every way of not being a
/// running process that this account may read, as [`started_at`] answers.
///
/// # Errors
///
/// [`Error::Os`] when the process was opened and could not be read.
///
/// [`Error::Os`]: super::Error::Os
#[cfg(all(feature = "host", windows))]
pub(crate) fn usage(pid: u32) -> Result<Option<Usage>> {
    sys::usage(pid)
}
