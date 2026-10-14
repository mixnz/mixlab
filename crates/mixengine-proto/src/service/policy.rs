//! How a service is run, as opposed to which service it is: how it is known to be ready and
//! healthy, restarted, stopped and reloaded, what it may use, when it may be idled and how much of
//! its output is kept. Each is a field of [`ServiceSpec`], whose `validate` enforces the rules
//! between them.

use std::net::SocketAddr;
use std::path::PathBuf;

use super::ServiceId;
// For `IdlePolicy`'s doc link alone. A reference link written into the doc would reach the
// TypeScript binding, which carries a struct's doc comment as it is.
#[cfg(doc)]
use super::ServiceSpec;
use crate::Millis;

/// *Can traffic be routed to it yet?*
///
/// Distinct from [`HealthCheck`], which asks whether it is still fine, because MariaDB's first boot
/// initialises a schema and is slow while its steady-state ping is cheap. Every variant carries its
/// own timeout: exceeding it is what turns `Starting` into `Failed`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ReadyCheck {
    /// A TCP connection succeeds.
    Tcp {
        /// Where to connect.
        addr: SocketAddr,
        /// How long to keep trying before giving up.
        timeout: Millis,
    },

    /// A Unix domain socket accepts a connection. Unavailable on Windows, where php-fpm listens on
    /// TCP instead — a spec that names one there fails with `unsupported_platform` rather than
    /// hanging until its timeout.
    UnixSocket {
        /// The socket path.
        path: PathBuf,
        /// How long to keep trying before giving up.
        timeout: Millis,
    },

    /// An HTTP request comes back with the expected status.
    Http {
        /// The URL to request.
        url: String,
        /// The status that means ready. Often `200`, but a service whose root redirects says `302`.
        expect_status: u16,
        /// How long to keep trying before giving up.
        timeout: Millis,
    },

    /// A line matching a regex appears on stdout or stderr.
    ///
    /// For services that announce themselves and listen on nothing we can poll. The regex is
    /// compiled by the supervisor, not here — a spec is data, and this crate has no regex engine.
    LogPattern {
        /// The pattern, in the `regex` crate's syntax.
        regex: String,
        /// How long to keep reading before giving up.
        timeout: Millis,
    },

    /// The process is simply still alive after a settling period.
    ///
    /// The last resort, and it is genuinely weak: it cannot distinguish a service that is ready from
    /// one that is about to crash. Use it only where nothing better exists.
    PidAlive {
        /// How long the process must stay up to count as ready.
        settle: Millis,
    },

    /// A command exits zero — `mariadb-admin ping`, `pg_isready`.
    ///
    /// **The honest check for a database, and the reason it is a *ready* check and not only a
    /// health one.** A TCP accept proves the listener is up, which stays true for the whole of
    /// InnoDB's crash recovery while the server refuses every query — so a supervisor watching the
    /// port reports the service ready and hands the next caller a connection refusal.
    ///
    /// Run in the service's own surroundings, exactly as [`HealthProbe::Command`] and
    /// [`StopBehaviour::Command`] are: the credential MariaDB's client needs is in the environment
    /// the server was started with, and a probe that ran anywhere else would be asking a different
    /// server a question it cannot authenticate.
    Command {
        /// The program to run.
        program: PathBuf,
        /// Its arguments.
        args: Vec<String>,
        /// How long to keep retrying before giving up.
        timeout: Millis,
    },
}

impl ReadyCheck {
    /// The deadline this check gives up at, whichever field carries it.
    #[must_use]
    pub fn timeout(&self) -> Millis {
        match self {
            Self::Tcp { timeout, .. }
            | Self::UnixSocket { timeout, .. }
            | Self::Http { timeout, .. }
            | Self::LogPattern { timeout, .. }
            | Self::Command { timeout, .. } => *timeout,
            Self::PidAlive { settle } => *settle,
        }
    }
}

/// *Is it still fine?* — polled periodically, only once ready.
///
/// Failing it makes a service `Degraded`, not `Failed`: the process is alive and the distinction is
/// what the GUI shows in amber and what `mix doctor` explains.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HealthCheck {
    /// What to ask.
    pub probe: HealthProbe,

    /// How often to ask it.
    pub interval: Millis,

    /// How long one probe may take before it counts as a failure.
    pub timeout: Millis,

    /// Consecutive failures before `Running` becomes `Degraded`.
    ///
    /// More than one, always: a single missed probe during a checkpoint flush is not a sick service,
    /// and a dashboard that flickers amber teaches people to ignore it.
    pub failures_before_degraded: u32,

    /// Consecutive successes before `Degraded` becomes `Running` again.
    pub successes_before_running: u32,
}

/// What a [`HealthCheck`] asks.
///
/// Deliberately not [`ReadyCheck`]: two of those variants only make sense once. `LogPattern` matches
/// a line printed during startup and would never match again, and `PidAlive` asks something the
/// supervisor already knows without probing — a health check that cannot fail is worse than none,
/// because it reports health it never measured.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum HealthProbe {
    /// A TCP connection still succeeds.
    Tcp {
        /// Where to connect.
        addr: SocketAddr,
    },

    /// A Unix domain socket still accepts a connection.
    UnixSocket {
        /// The socket path.
        path: PathBuf,
    },

    /// An HTTP request still returns the expected status.
    Http {
        /// The URL to request.
        url: String,
        /// The status that means healthy.
        expect_status: u16,
    },

    /// A MySQL or MariaDB server still greets a new connection — roadmap task **T190b**, D4.
    ///
    /// **The server's first packet, read without logging in.** The connection handler writes it,
    /// so it proves more than a TCP accept, and it is what `mysqladmin ping` rests on: that
    /// command succeeds whenever the server answers, *access denied* included. A handshake
    /// (`0x0a`) or an error packet (`0xff`, such as *too many connections*) is healthy; anything
    /// else within the timeout is not. No process is started, which on Windows was 10 ms of the
    /// daemon's CPU every ten seconds per database.
    MysqlGreeting {
        /// Where the server listens.
        addr: SocketAddr,
    },

    /// A Redis server still answers `PING` — roadmap task **T190b**, D4.
    ///
    /// `+PONG` is healthy, and so is any error reply but `-LOADING`: `-NOAUTH` is a server that
    /// answered. `-LOADING` is a server that will not serve reads yet, and is not healthy.
    RedisPing {
        /// Where the server listens.
        addr: SocketAddr,
    },

    /// A command exits zero — `pg_isready`, and any recipe with no protocol probe of its own.
    ///
    /// The honest check for a database: a TCP accept only proves the listener is up, which stays
    /// true while the server refuses every query.
    Command {
        /// The program to run.
        program: PathBuf,
        /// Its arguments.
        args: Vec<String>,
    },
}

/// How long to wait between restart attempts.
///
/// Exponential with a ceiling and a random spread, so a service whose dependency is down does not
/// retry in a tight loop and several restarting at once do not synchronise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Backoff {
    /// The wait before the first retry.
    pub initial: Millis,

    /// The longest wait, however many retries have happened.
    pub max: Millis,

    /// What each wait is multiplied by, as a percentage: `200` doubles it.
    ///
    /// An integer percentage rather than an `f64` because this type is compared and hashed like
    /// everything else in this crate, and because a `NaN` multiplier arriving from a hand-written
    /// manifest should not be representable in the first place.
    pub multiplier_percent: u32,

    /// How much to spread each wait by, as a percentage of itself: `20` means ±20 %.
    ///
    /// Under 100, always. At 100 the low end of the spread is zero, which is the tight retry loop
    /// the whole type exists to prevent — the same reason an `initial` of zero is refused.
    pub jitter_percent: u8,
}

impl Default for Backoff {
    /// The curve `docs/architecture/process-supervision.md` specifies: 500 ms doubling to 30 s.
    fn default() -> Self {
        Self {
            initial: Millis(500),
            max: Millis::from_secs(30),
            multiplier_percent: 200,
            jitter_percent: 20,
        }
    }
}

/// What to do when a supervised process exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum RestartPolicy {
    /// Leave it stopped. For one-shot initialisation and for anything a user starts by hand.
    Never,

    /// Restart it when it exits non-zero, up to a point.
    OnFailure {
        /// How many restarts are allowed inside [`window`](RestartPolicy::OnFailure::window) before
        /// the service goes `Failed` and stays there until an explicit `service.start`.
        max_retries: u32,
        /// The span the retries are counted over. A service that crashes once a day is not in a
        /// crash loop, and counting since boot would eventually say it was.
        window: Millis,
        /// The wait between attempts.
        backoff: Backoff,
    },

    /// Restart it whenever it exits, zero or not, forever.
    ///
    /// No retry ceiling on purpose — this is for a service whose absence is the failure. The backoff
    /// is what keeps it from spinning.
    Always {
        /// The wait between attempts.
        backoff: Backoff,
    },
}

impl Default for RestartPolicy {
    /// `OnFailure`, five retries in five minutes — the crash-loop cutoff in
    /// `docs/architecture/process-supervision.md`.
    fn default() -> Self {
        Self::OnFailure {
            max_retries: 5,
            window: Millis::from_secs(300),
            backoff: Backoff::default(),
        }
    }
}

/// How to ask a service to stop before making it.
///
/// Every variant ends the same way: when the grace period expires the process group is killed. What
/// differs is what is tried first.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum StopBehaviour {
    /// Signal the process group and wait — `SIGTERM` to `-pgid` on Unix, the console control event
    /// on Windows.
    Signal {
        /// How long to wait for it to leave on its own.
        grace: Millis,
    },

    /// Run a shutdown command and wait — `mariadb-admin shutdown`, `caddy stop`.
    ///
    /// For a service that needs to flush before it exits, where a signal loses data or leaves a
    /// recovery to do on next boot.
    Command {
        /// The program to run.
        program: PathBuf,
        /// Its arguments.
        args: Vec<String>,
        /// How long to wait for it to work.
        grace: Millis,
    },

    /// Kill the process group immediately.
    ///
    /// Correct for something stateless that has nothing to flush, and honest about it — the
    /// alternative is a grace period nobody needs, paid on every stop.
    Kill,
}

impl Default for StopBehaviour {
    /// A signal and ten seconds — the same grace period `daemon.shutdown` uses.
    fn default() -> Self {
        Self::Signal {
            grace: Millis::from_secs(10),
        }
    }
}

/// How to hand a running service a configuration that has changed, without stopping it.
///
/// **The whole point is what it is not**: a restart. `docs/features/services.md` puts it as
/// "reload beats restart", and the cost it is avoiding is real — the front-end web server is the
/// thing every site is reached through, and dropping every connection because one site was edited is
/// a cost the user did not ask for and cannot see the reason for.
///
/// `None` means a service with no such mechanism, which is most of them: Redis and Memcached read
/// their configuration once, and a change to one is a restart whichever way it is spelled.
///
/// Two variants, because the two servers this ships with reload by different means: Caddy runs a
/// program, php-fpm takes a signal. Windows has neither of those signals, so a recipe there returns
/// no reload at all rather than one that would be refused — `docs/decisions/0008-no-signal-stop-on-windows.md`.
/// `#[non_exhaustive]` is what leaves a third addition additive.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ReloadBehaviour {
    /// Run a command that tells the process to re-read its configuration — `caddy reload`,
    /// `nginx -s reload`.
    ///
    /// Run **in the service's own surroundings**, exactly as a health probe and a stop command are:
    /// see `mixengine_supervisor::Surroundings`.
    Command {
        /// The program to run.
        program: PathBuf,
        /// Its arguments.
        args: Vec<String>,
        /// How long it is given before it is abandoned and the reload counted as not done.
        ///
        /// Not a grace period: nothing is killed when this expires and the service goes on running
        /// the configuration it already had. What it bounds is how long the daemon waits.
        patience: Millis,
    },

    /// Send a signal to the running process — php-fpm's `SIGUSR2` — roadmap task **T32**.
    ///
    /// **There is nothing to run.** php-fpm's reload is a signal to the master, which finishes the
    /// requests its workers are serving and replaces them with workers that read the new file; the
    /// daemon already holds the pid it goes to. A [`Command`](Self::Command) spelled with a `kill`
    /// would be a program looked up on a `PATH` to do something this process can do directly, and
    /// would not exist on Windows at all.
    ///
    /// **Unavailable on Windows**, where there is no signal a daemon can send a process it gave no
    /// console to — `docs/decisions/0008-no-signal-stop-on-windows.md`. A recipe there returns no
    /// reload at all rather than this, so nothing is ever asked for and then refused: the supervisor
    /// says once, in `daemon.log`, that the running process is still on its previous configuration.
    Signal {
        /// Which signal.
        signal: ReloadSignal,

        /// How long the daemon treats the reload as in progress before it stops waiting.
        ///
        /// Not a grace period: nothing is killed when it expires, and unlike a command there is no
        /// exit status to read either way. What it bounds is the window in which a second reload is
        /// not sent on top of the first.
        patience: Millis,
    },
}

/// Which signal a [`ReloadBehaviour::Signal`] sends.
///
/// **A closed list and not a number.** This crate is the wire vocabulary, is compiled for three
/// systems and must not leak `libc` — the same reason [`StopBehaviour::Signal`] names no signal
/// either. Three variants because three are what the servers MixEngine runs use: `SIGHUP` almost
/// everywhere, `SIGUSR1` for a log reopen, `SIGUSR2` for php-fpm's graceful pool restart. A fourth
/// is a variant added the day a recipe needs it, which is a decision somebody makes — where an
/// `i32` would be a number a recipe could invent and the platform layer would have to trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ReloadSignal {
    /// `SIGHUP` — re-read the configuration, the convention almost every daemon follows.
    Hup,

    /// `SIGUSR1` — reopen log files, without touching anything else.
    Usr1,

    /// `SIGUSR2` — php-fpm's graceful pool restart: the workers finish what they are serving and
    /// their replacements read the new file.
    Usr2,
}

/// Scheduling priority, the one resource control every OS can honour.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Priority {
    /// The default: competes with everything else the user is running.
    #[default]
    Normal,

    /// Yields to foreground work — macOS background QoS, `nice` on Linux, `BELOW_NORMAL` on
    /// Windows. The right setting for anything the user is not waiting on.
    Background,
}

/// Per-service CPU and memory caps.
///
/// Applied through the platform layer, which cannot honour all of it everywhere: Windows has Job
/// Objects and Linux has cgroup v2, but **macOS has no hard memory cap**, so a limit there becomes a
/// watchdog rather than a wall. That asymmetry is a fact the GUI must show rather than hide — see
/// `docs/features/resource-isolation.md`. Enforcement is roadmap task T68; the fields exist now
/// so a spec written before it does not have to be revisited.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
// **Every field is optional on the way in**, which `Option` alone does not say: serde asks for a
// field even when its type can express absence. The document this is read from is
// `services.limits_json`, whose column default is `{}` — the ordinary state of a service nobody has
// capped — and an `extension.toml` that writes `memory_mb` and nothing else is the ordinary shape of
// one that has. Without this, both are a parse failure naming a field the author was right to leave
// out.
#[serde(default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ResourceLimits {
    /// A ceiling on CPU, as a percentage of one core. `None` is uncapped.
    pub cpu_percent: Option<u8>,

    /// A ceiling on resident memory, in megabytes. `None` is uncapped.
    pub memory_mb: Option<u32>,

    /// How this service competes for CPU when nothing is capped.
    pub priority: Priority,
}

impl ResourceLimits {
    /// Refuse the one shape that is wrong on every machine there will ever be.
    ///
    /// **`cpu_percent` is deliberately not checked here, however large it is.** Its rule — no more
    /// than 100% of one core per core the machine has — is measured against a property of the
    /// *machine*, and this crate is the shared vocabulary: it has no host to ask and must not grow
    /// one. That refusal belongs to the daemon, in `service.set_limits`, where the core count is
    /// already being read to answer with. Roadmap task **T68**.
    ///
    /// Called by [`ServiceSpecBuilder::build`], so a spec that exists is one that was checked, and
    /// separately by the method that writes `services.limits_json`, which does not go through a
    /// builder.
    ///
    /// # Errors
    ///
    /// A reason phrased for a person, for the caller to carry into whatever error it reports with.
    ///
    /// [`ServiceSpecBuilder::build`]: super::ServiceSpecBuilder::build
    pub const fn validate(&self) -> Result<(), &'static str> {
        if matches!(self.memory_mb, Some(0)) {
            return Err(
                "a memory limit of 0 would let the service allocate nothing — leave it unset to \
                 run uncapped",
            );
        }

        Ok(())
    }
}

/// How to tell that a service has nothing to do.
///
/// Measured, never assumed: an idle policy that only watched the clock would stop a database in the
/// middle of a long import. A service with an open connection is never idle.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum IdleProbe {
    /// Count established connections to a port, from the service's own status endpoint where it has
    /// one and the OS socket table otherwise.
    Connections {
        /// The port whose connections mean "in use".
        port: u16,
    },

    /// Read a monotonic counter out of a JSON status endpoint — php-fpm's `accepted conn`, a
    /// database's query count — and compare it with the previous sample.
    HttpCounter {
        /// The status URL to read.
        url: String,
        /// The field within it holding the counter.
        field: String,
    },

    /// Ask php-fpm's own status page, over FastCGI, on the socket the pool listens on — roadmap
    /// task **T72a**.
    ///
    /// **A socket and not an address of either shape**, because this exists for the system where a
    /// pool has no port at all. Windows runs `php-cgi.exe -b`, which is not php-fpm and publishes
    /// no status page, and it already has [`Connections`](Self::Connections) and a real port to
    /// count on. A variant that cannot express the wrong thing is better than one that can.
    ///
    /// **The probe is itself a request**, so it costs the pool exactly one `accepted conn` and
    /// occupies one worker for the length of it — measured against php-fpm 8.3.6. What the reader
    /// does with that is `mixengine-supervisor`'s `idle::observe`, which subtracts its own
    /// connection and reads `active processes` beside it.
    FastCgiStatus {
        /// The pool's own listening socket.
        socket: std::path::PathBuf,

        /// The `pm.status_path` the pool was configured with.
        ///
        /// **Never ends in `.php`**, which is the whole of what keeps it unreachable from a site:
        /// both front ends hand FastCGI only what matches `.php`.
        path: String,
    },
}

/// When to stop a service that nothing is using.
///
/// The product's central promise is that idle costs nothing, and this is the mechanism. `None` on a
/// [`ServiceSpec`] means never idle-stop — correct for the front-end web server, which is the thing
/// that starts everything else back up. Enforcement is roadmap task T69.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct IdlePolicy {
    /// How long the service must look idle before it is stopped.
    pub after: Millis,

    /// What "idle" is measured with.
    pub probe: IdleProbe,
}

/// Which of the three states `services.idle_minutes` is in, and what the recipe said about it.
///
/// **Four values over three column states**, because `NULL` means two different things depending on
/// whether the recipe offers a default — and those two are exactly the pair a person needs told
/// apart. Both are a service that will not be stopped; only one of them is a setting anybody chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum IdleSource {
    /// The column is null and the recipe supplies the duration.
    ///
    /// Reached only while the home saves resources (**T167b**, ADR 0041): no recipe supplies one
    /// otherwise, and the duration is then the recipe's saving number.
    Recipe,

    /// The column holds a duration, and it is the one in force.
    Row,

    /// The column holds `0` — switched off here, whatever the recipe says.
    Never,

    /// The column is null and the recipe offers nothing, so nothing idles this service.
    Unset,

    /// A duration was asked for and this service cannot be measured, so nothing idles it.
    ///
    /// **The state a report without it would describe as "never", which is true and useless.** A
    /// php-fpm pool on a Unix socket has no port to count connections on, so its recipe offers no
    /// probe; somebody who has just typed `--after 30m` and is told "never" has been told the
    /// outcome and not the reason, and will type it again.
    Unmeasurable,
}

impl IdleSource {
    /// Which state a column value and a recipe default add up to.
    ///
    /// A free function over all three rather than a method on any, because no one of them can
    /// answer alone: the column cannot tell `Recipe` from `Unset`, the recipe cannot tell `Row`
    /// from `Never`, and neither can tell either from a policy that was asked for and could not be
    /// assembled. One definition, so a client's rendering and the sweeper's decision cannot drift.
    ///
    /// `policy` is what the service's spec actually ended up with — the join of the two halves. A
    /// duration asked for that produced no policy is [`Unmeasurable`](Self::Unmeasurable): the
    /// recipe declared no probe, which is what a pool on a Unix socket looks like.
    #[must_use]
    pub fn of(
        column: Option<i64>,
        recipe_default: Option<Millis>,
        policy: Option<&IdlePolicy>,
    ) -> Self {
        match (column, recipe_default) {
            (Some(0), _) => Self::Never,
            (None, None) => Self::Unset,
            // Something asked for a policy. Whether one exists is the question `policy` answers,
            // and the two arms below are the same answer for the two ways of asking.
            (Some(_), _) | (None, Some(_)) if policy.is_none() => Self::Unmeasurable,
            (Some(_), _) => Self::Row,
            (None, Some(_)) => Self::Recipe,
        }
    }
}

/// Why a service would not be idle-stopped right now even if its policy said so.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum IdleExemption {
    /// Something that depends on it is running.
    ///
    /// A database with no connections underneath a running pool is not a database nobody wants; it
    /// is a database between two requests.
    DependentRunning {
        /// The dependent that is running.
        service: ServiceId,
    },

    /// A project is being worked on and asked for its services to stay warm.
    ProjectKeptWarm {
        /// The project, by name.
        project: String,
    },
}

/// How much of a service's output to keep, and where the ceiling is.
///
/// Rotation is the supervisor's own job rather than an external logrotate's, because the supervisor
/// is the process holding the file handle. Enforcement is roadmap task T16.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LogPolicy {
    /// Rotate once the live file passes this size.
    ///
    /// *Passes*, not *reaches*: a line is never split across two files, so one long backtrace can
    /// carry the file over its limit.
    pub max_file_bytes: u64,

    /// How many rotated copies to keep beside the live file.
    pub max_files: u8,

    /// How many recent lines to keep in memory, so `service.logs` and the GUI's log panel answer
    /// instantly instead of reading from disk.
    pub ring_lines: u16,
}

impl Default for LogPolicy {
    /// 10 MB × 5 files, 500 lines in memory — the defaults in
    /// `docs/architecture/process-supervision.md`.
    fn default() -> Self {
        Self {
            max_file_bytes: 10 * 1024 * 1024,
            max_files: 5,
            ring_lines: 500,
        }
    }
}

impl RestartPolicy {
    /// The backoff curve this policy retries on, if it retries at all.
    #[must_use]
    pub fn backoff(&self) -> Option<Backoff> {
        match self {
            Self::Never => None,
            Self::OnFailure { backoff, .. } | Self::Always { backoff } => Some(*backoff),
        }
    }
}
