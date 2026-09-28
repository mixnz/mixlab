//! What a process group is costing this machine right now — roadmap task **T71**.
//!
//! **Beside [`ConnectionCount`](crate::ConnectionCount) rather than inside
//! [`ResourceControl`](crate::ResourceControl)**, and the pair is worth stating: `ResourceControl`
//! answers what this machine will *enforce*, asked once at boot; this is asked every tick for as
//! long as the daemon runs and answers what is being *spent*. A ceiling and a measurement are
//! different questions — and on macOS, where there is no memory ceiling to ask about, the second is
//! all there is (roadmap task **T71a**).

use crate::process::StartTime;

/// The process at the head of one supervised group.
///
/// **A pid and the moment it began, never a pid alone.** The operating system reuses a pid within
/// minutes, so a sampler that asked by pid would draw a stranger's memory on the chart of a service
/// that exited between two ticks. This is the identity
/// [`Adopted::identify`](crate::process::Adopted::identify) already uses, for the same reason and
/// against the same column pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupRoot {
    /// The process id, as the daemon recorded it.
    pub pid: u32,

    /// When that process began, as this system counts such moments.
    pub started: StartTime,
}

/// What one group was measured at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupReading {
    /// Which [`GroupRoot::pid`] this answers for.
    pub pid: u32,

    /// Percentage of **one** core, so 250 is two and a half cores' worth.
    ///
    /// The unit `ResourceLimits::cpu_percent` is declared in, deliberately: a client that offers a
    /// cap and then draws the usage must not have to convert between the two.
    ///
    /// [`None`] where no figure could be taken, and never `0.0` for that case. A CPU reading is a
    /// difference between two moments, so a group this sampler is seeing for the first time has
    /// nothing to subtract from — reporting a zero there would draw an idle service during the one
    /// second it is most expensive.
    pub cpu_percent: Option<f32>,

    /// Resident bytes, summed over the group.
    ///
    /// **Shared pages are counted once per process**, so a php-fpm master and its four workers
    /// overstate the group by most of what they share. There is no cross-platform way to do better —
    /// PSS is Linux's alone — so the number is an overestimate, and the error is in the safe
    /// direction for a footprint MixEngine defends in a README.
    ///
    /// **Not the quantity a `memory_mb` limit is judged against**, which is commit charge on Windows
    /// and charged pages on Linux — see [`MemoryMeasure`](crate::MemoryMeasure). Rendering this and
    /// a limit as one ratio is rendering two different measurements.
    pub rss_bytes: u64,

    /// How many processes the group holds, the root included.
    ///
    /// Free, because the group is walked anyway, and it answers something otherwise unanswerable:
    /// how many workers a pool is running right now.
    pub processes: u32,
}

/// What the supervised groups on this machine are spending.
pub trait ProcessMetrics: std::fmt::Debug + Send + Sync {
    /// One reading per root that is still the process it was recorded as.
    ///
    /// **A root that has ended, or whose pid the system handed to something else, is absent from the
    /// answer** — never a reading of zero. Absence means *not measured*, which is
    /// [`ConnectionCount`](crate::ConnectionCount)'s rule about its own errors and the reason a
    /// minute with no row is drawn as a gap rather than as a flat line.
    ///
    /// **Never fails.** Every way this can go wrong — a process that ended between the refresh and
    /// the walk, one this account may not ask about — is one root missing from the answer, and a
    /// caller that had to distinguish those would act identically on all of them.
    ///
    /// # What one call costs
    ///
    /// Two halves since roadmap task **T181**. Every process's *parent* is read the cheapest way the
    /// system offers, because the parent map has to exist before any group can be walked; then only
    /// the processes in a group are refreshed for their CPU and memory. Since **T190** the first
    /// half is kept between calls and taken again only when the roots change, when a member that
    /// was read has ended, or after ten seconds; on Windows the second half reads each member by pid,
    /// because `sysinfo` walks a snapshot of every process there even when asked for a few.
    ///
    /// Before T181 the whole table was refreshed every call. Measured over ten calls on one
    /// developer's machine with 276 processes running, that was **about 10 ms on Windows 11** (9–16
    /// ms across runs) and **about 2 ms under WSL Ubuntu 24.04**; on a Mac with 773 running it was
    /// **12–15 ms**, because `sysinfo` reads every process's arguments there on each refresh. After
    /// it, the same Mac measures **about 0.5 ms**.
    ///
    /// Windows had not gained from T181: with seven groups on a machine with 446 processes a call
    /// still cost **14.6 ms**, two whole-machine snapshots, and the daemon held 5–6% of a core while
    /// one stream was open. Since T190 a call at the fast rate costs **41 µs** there, and a call
    /// that lists the machine 6.9 ms, once in ten seconds; under WSL the same calls cost 198 µs and
    /// 1.1 ms.
    ///
    /// That is the number the sampling periods are chosen against, and it is measured rather than
    /// argued because the documents in this repository criticise polling a sleeping laptop by name:
    /// once a minute is 0.02% of one core, and the one-second rate is spent only while somebody has
    /// a stream open and stops when they close it. Re-measure with
    /// `cargo test -p mixengine-platform --release --lib one_refresh_costs -- --ignored --nocapture`.
    fn measure(&self, roots: &[GroupRoot]) -> Vec<GroupReading>;
}
