//! `config.toml` — the user's preferences, read once at boot.
//!
//! This file is *not* state. Everything MixEngine decides for itself lives in `mixengine.db`;
//! everything the machine can regenerate lives under `etc/`. What is left — how loudly to log,
//! where the daemon listens, which directories were moved to a bigger disk — is what a user edits
//! by hand, so it stays a small, commented TOML file rather than a hidden database row.
//!
//! Keys arrive with the task that reads them. Declaring a section before anything honours it would
//! be a promise the build does not keep.

use std::io::Write as _;
use std::path::{Component, Path, PathBuf, Prefix};

use serde::{Deserialize, Deserializer};

use crate::{Error, Result};

/// The configuration file's name, directly under `MIXENGINE_HOME`.
pub const FILE_NAME: &str = "config.toml";

/// The commented starting point written on first run.
///
/// Public so `mix doctor` can restore a deleted file and so tests can prove it stays in step with
/// the types below.
pub const TEMPLATE: &str = include_str!("config/template.toml");

/// Everything a user may set.
///
/// `deny_unknown_fields` is deliberate: an unrecognised key fails the load instead of being
/// ignored. A misspelled key that does nothing looks exactly like a key that does not work, and
/// the user has no way to tell which they are looking at.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Log verbosity and format.
    pub log: Logging,
    /// Daemon-level settings.
    pub daemon: Daemon,
    /// The built-in DNS server.
    pub dns: Dns,
    /// Certificate upkeep.
    pub certs: Certs,
    /// Idle shutdown.
    pub services: Services,
    /// Ending a share nobody ended.
    pub sharing: Sharing,
    /// What a site answers when it has nothing to serve.
    pub sites: Sites,
    /// Looking for a newer MixEngine.
    pub updates: Updates,
    /// How often what is running is measured.
    pub metrics: Metrics,
    /// What is recorded when the daemon hits a bug in itself.
    pub crash: Crash,
    /// Overrides for the directories that grow.
    pub paths: PathOverrides,
}

/// How the daemon writes its log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Logging {
    /// How much to log.
    pub level: LogLevel,
    /// How to shape each line.
    pub format: LogFormat,
}

/// Verbosity of the daemon log.
///
/// A closed set: a free-form string would let a typo silence logging entirely, and the process
/// would look perfectly healthy while saying nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// User-visible failures only.
    Error,
    /// Degraded but continuing.
    Warn,
    /// Lifecycle events a user might care about.
    #[default]
    Info,
    /// Developer detail.
    Debug,
    /// Firehose.
    Trace,
}

/// Shape of each log line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable, for a terminal.
    #[default]
    Text,
    /// One JSON object per line, for a collector.
    Json,
}

/// Daemon-level settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Daemon {
    /// Where the daemon listens for clients: a Unix socket path or a Windows named pipe.
    ///
    /// `None` means "wherever the platform layer puts it", which is the answer for almost
    /// everyone. It exists because the default lands under `run/`, and a `MIXENGINE_HOME` on a
    /// filesystem that cannot host a socket (a network share, a path over the 108-byte `sun_path`
    /// limit) needs a way out that is not "move everything".
    #[serde(default, deserialize_with = "named_path")]
    pub ipc_path: Option<PathBuf>,

    /// How long the whole of a `daemon.shutdown` may spend stopping services, in seconds.
    ///
    /// **The budget is on the total, not on each service**, which is the distinction that makes it
    /// worth a key at all: a [`ServiceSpec`](mixengine_proto::ServiceSpec) already says how long
    /// *its* service needs in order to shut down cleanly, and eight services each allowed ten
    /// seconds is eighty seconds a user is waiting for a daemon they asked to stop. Each service
    /// gets what its spec asks for or what is left of this, whichever is less, so the sum is what a
    /// person can plan around and the individual grace periods stay statements about the services.
    ///
    /// Zero is a real answer and means every service is killed at once — recovery on the next start,
    /// deliberately chosen. A generous value only costs anything on the shutdown where something
    /// genuinely will not go, and stays generous up to ten minutes. Past that the file is refused
    /// rather than corrected: the number leaves here as a `Duration` added to an `Instant`, which
    /// panics on overflow, so an unbounded budget crashes the shutdown it was meant to bound.
    ///
    /// **Not what bounds a shutdown the operating system asked for.** A console control event on
    /// Windows runs on a clock the daemon cannot extend
    /// ([`STOP_CEILING`](mixengine_platform::signal::STOP_CEILING)), so that path takes the smaller
    /// of the two; on Unix and over the API this value is the whole of it.
    #[serde(deserialize_with = "shutdown_grace")]
    pub shutdown_grace_seconds: u64,
}

/// The default for [`Daemon::shutdown_grace_seconds`], and the reason [`Daemon`] writes its own
/// [`Default`] rather than deriving one: a derived default here would be zero, which is a real
/// setting meaning "kill everything at once" and is not what an absent key asks for.
const DEFAULT_SHUTDOWN_GRACE_SECONDS: u64 = 10;

/// The largest [`Daemon::shutdown_grace_seconds`] MixEngine accepts: ten minutes.
///
/// A ceiling is needed at all because the number does not stay a number. The daemon turns it into a
/// [`Duration`](std::time::Duration) and adds it to an [`Instant`](std::time::Instant) to work out
/// when the budget runs out, and `impl Add<Duration> for Instant` *panics* on overflow rather than
/// saturating. A budget of `u64::MAX` seconds therefore does not produce a shutdown that waits
/// forever, it produces one that panics immediately — on the task running the shutdown, unwinding
/// past the write-ahead log checkpoint and leaving every service's row still saying `stopping`.
/// That is the exact outcome the budget exists to prevent, reached by asking for more of it.
///
/// Ten minutes because it has to sit above every honest answer and nowhere near the arithmetic. The
/// slowest polite stop MixEngine supervises is a database writing its buffers out — PostgreSQL's
/// own `shutdown_timeout` defaults to sixty seconds, and MariaDB with a large InnoDB pool and
/// `innodb_fast_shutdown = 0` can spend a few minutes on a slow disk — so this clears the real
/// cases several times over while still being a wait a person can be told to expect. Past it the
/// number has stopped being a preference: it is a typo, or it is somebody spelling "never", and
/// "never" is not a budget.
const MAX_SHUTDOWN_GRACE_SECONDS: u64 = 600;

impl Default for Daemon {
    fn default() -> Self {
        Self {
            ipc_path: None,
            shutdown_grace_seconds: DEFAULT_SHUTDOWN_GRACE_SECONDS,
        }
    }
}

/// The built-in DNS server — roadmap task **T44**.
///
/// It answers `A` for every name under a TLD MixEngine manages and refuses everything else, which
/// is what lets a site be created without an elevation prompt: a wildcard needs no record per
/// domain, where a hosts file needs a line per domain and a prompt per line.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Dns {
    /// Whether to run it at all.
    ///
    /// Turning it off is choosing the hosts file explicitly, and it is reported as such rather than
    /// silently: `daemon.status` says which mechanism this home is on and why.
    pub enabled: bool,

    /// The loopback port it listens on, or [`None`] for this system's default.
    ///
    /// The default is **53 on Windows and 53535 elsewhere**, and the split is about what a resolver
    /// rule can express rather than about privilege: Windows' NRPT rule names a nameserver with no
    /// way to state a port, while `/etc/resolver` and `resolvectl` both can. It is deliberately not
    /// 5353, which belongs to mDNS on both of the systems where the number is free.
    ///
    /// A key at all because a machine where something already holds the default needs a way out
    /// that is not "move your home directory" — and because a test cannot legitimately bind the
    /// real one (`docs/standards/testing.md`).
    ///
    /// **`0` asks the operating system to pick one**, which is what every suite that starts a real
    /// daemon does (`mixengine_testkit::Home`). It is a real setting rather than a special case,
    /// and it is useless to anybody else: a port that changes on every start is a port no resolver
    /// can be wired to.
    pub port: Option<u16>,
}

/// [`Dns`] writes its own [`Default`] for [`Daemon`]'s reason: a derived one would leave the server
/// switched off, and an absent section asks for the ordinary behaviour rather than for none.
impl Default for Dns {
    fn default() -> Self {
        Self {
            enabled: true,
            port: None,
        }
    }
}

/// How MixEngine keeps this home's certificates from expiring — roadmap task **T52**.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Certs {
    /// How often the daemon looks for a certificate that is running out, in seconds.
    ///
    /// **Not a promise about accuracy, and it does not need to be one.** A leaf is replaced a full
    /// month before it expires — `mixengine_core::certs::leaf::RENEW_WITHIN_DAYS` — so the
    /// threshold is the tolerance: a check that arrives hours late still renews with weeks in hand.
    /// That matters because the clock underneath is not a wall clock. Tokio measures from
    /// `std::time::Instant`, which counts no time on Linux or macOS while the machine is suspended,
    /// so a laptop closed over a weekend makes a period of a day into a period of four. Rather than
    /// make the alarm accurate, the check is made cheap enough that its accuracy stops mattering.
    ///
    /// It is a key at all because the daemon's own renewal suite has to watch the loop run, and a
    /// period no test can move would leave the loop the one part of that task nothing exercises —
    /// which is how T51 nearly shipped an nginx TLS port no unprivileged machine could bind.
    #[serde(deserialize_with = "renew_check")]
    pub renew_check_seconds: u64,
}

/// The default for [`Certs::renew_check_seconds`]: hourly.
const DEFAULT_RENEW_CHECK_SECONDS: u64 = 3_600;

/// [`Certs`] writes its own [`Default`] for [`Daemon`]'s reason: a derived one would be zero, which
/// is the one value this key refuses.
impl Default for Certs {
    fn default() -> Self {
        Self {
            renew_check_seconds: DEFAULT_RENEW_CHECK_SECONDS,
        }
    }
}

/// Refuse a renewal period of zero.
///
/// **A floor and no ceiling to go with it**, unlike [`shutdown_grace`]. A period longer than sixty
/// days would be the first that could let a 90-day certificate pass its threshold unnoticed, and
/// somebody who writes that number has answered a different question than this key asks. Zero is
/// different in kind: it is not a long period, it is no pause at all.
fn renew_check<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let seconds = u64::deserialize(deserializer)?;

    if seconds == 0 {
        return Err(serde::de::Error::custom(format!(
            "a renewal check every 0 seconds is a loop with no pause in it rather than a schedule; \
             give it a number of seconds, or remove the key for the default of \
             {DEFAULT_RENEW_CHECK_SECONDS}"
        )));
    }

    Ok(seconds)
}

/// How MixEngine ends a share nobody ended — roadmap task **T76**.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sharing {
    /// How often the daemon compares a shared site against the network it is on, in seconds.
    ///
    /// **Short, and it can afford to be.** A pass is one enumeration of this machine's own
    /// interfaces and one read of the site rows, and a finding has to be seen twice before anything
    /// happens — so this is the resolution of the answer rather than a promise about it.
    ///
    /// Thirty rather than three hundred because of what the delay costs at the far end. Between an
    /// address vanishing and the revoke, the front end holds a listener bound to an address this
    /// machine no longer has, so any re-render in that window — a certificate renewal, an unrelated
    /// new site — may fail to reload, for *every* site rather than only the shared one.
    ///
    /// It is a key at all for the reason [`Certs::renew_check_seconds`] gives about its own: a
    /// period no test can move leaves the loop the one part of that task nothing exercises.
    #[serde(deserialize_with = "sharing_check")]
    pub check_seconds: u64,
}

/// What a site answers when it has nothing behind it — roadmap task **T124**.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sites {
    /// Whether a site with nothing to serve answers with MixEngine's own page instead of a bare 404
    /// or a bare 502.
    ///
    /// **On.** The moment this product is most likely to be judged is the first time a browser is
    /// pointed at a new site, and without this that moment is a web server's default error page —
    /// which three of the six shipped blueprints reach by design, since they carry no `[scaffold]`
    /// and an apply of one ends at a configured site over an empty directory.
    ///
    /// **A property of the machine rather than of any one site**, which is why there is no per-site
    /// field: the page cannot appear on a site that serves anything at `/` or has anything
    /// listening, so the population that wants it off is not "this site" but "this machine's
    /// owner" — someone whose tests assert a bare 404. The T124 design, D6.
    pub welcome_page: bool,
}

/// [`Sites`] writes its own [`Default`] for [`Sharing`]'s reason: a derived one would be `false`,
/// which is the opposite of the decision above.
impl Default for Sites {
    fn default() -> Self {
        Self { welcome_page: true }
    }
}

/// The default for [`Sharing::check_seconds`]: every half minute.
const DEFAULT_SHARING_CHECK_SECONDS: u64 = 30;

/// [`Sharing`] writes its own [`Default`] for [`Certs`]' reason: a derived one would be zero, which
/// is the one value this key refuses.
impl Default for Sharing {
    fn default() -> Self {
        Self {
            check_seconds: DEFAULT_SHARING_CHECK_SECONDS,
        }
    }
}

/// Refuse a sharing check of zero, on [`renew_check`]'s reasoning: it is not a short pause, it is
/// no pause at all.
fn sharing_check<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let seconds = u64::deserialize(deserializer)?;

    if seconds == 0 {
        return Err(serde::de::Error::custom(format!(
            "a sharing check every 0 seconds is a loop with no pause in it rather than a schedule; \
             give it a number of seconds, or remove the key for the default of \
             {DEFAULT_SHARING_CHECK_SECONDS}"
        )));
    }

    Ok(seconds)
}

/// `[updates]` — read and ignored since T187.
///
/// **Neither key does anything any more** ([ADR 0056](../../../docs/decisions/0056-mixlab-stands-without-mixengine.md)
/// rule 8): the daemon never reads the update feed unprompted, so there is no check at start and no
/// clock for them to turn off or to time. They are kept, and still validated, because this struct is
/// `deny_unknown_fields`: removing them would make every `config.toml` that sets them fail to load.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Updates {
    /// Whether the daemon read the update feed on its own, before T187. Ignored.
    pub enabled: bool,

    /// How long there was between those checks, before T187. Ignored.
    #[serde(deserialize_with = "update_check")]
    pub check_seconds: u64,
}

/// The default for [`Updates::check_seconds`]: once a day, which is what
/// `docs/features/updates.md` promises.
const DEFAULT_UPDATE_CHECK_SECONDS: u64 = 24 * 60 * 60;

/// [`Updates`] writes its own [`Default`] for [`Sharing`]'s reason: a derived one would be zero
/// seconds and `false`, and neither is this feature's default.
impl Default for Updates {
    fn default() -> Self {
        Self {
            enabled: true,
            check_seconds: DEFAULT_UPDATE_CHECK_SECONDS,
        }
    }
}

/// Refuse an update check of zero, on [`sharing_check`]'s reasoning.
///
/// Somebody who wants no checks at all writes `enabled = false`, which is the key that says so.
fn update_check<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let seconds = u64::deserialize(deserializer)?;

    if seconds == 0 {
        return Err(serde::de::Error::custom(format!(
            "an update check every 0 seconds is a loop with no pause in it rather than a schedule; \
             give it a number of seconds, set `enabled = false` to stop checking, or remove the key \
             for the default of {DEFAULT_UPDATE_CHECK_SECONDS}"
        )));
    }

    Ok(seconds)
}

/// What the daemon writes down about a bug in itself — roadmap task **T91**.
///
/// **One key, and it is not the consent that task's sentence asked for**, because there is nothing
/// here to consent to. Nothing is transmitted: the file is written into this home and stays there,
/// and the only thing that ever puts it in an archive is `mix doctor --bundle`, which is a command
/// somebody types. See
/// `docs/decisions/0022-a-crash-report-is-recorded-by-default-and-sent-by-nothing.md`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Crash {
    /// Whether a crash report file is written at all.
    ///
    /// **`false` stops the file and nothing else.** The daemon log still records that a panic
    /// happened, because that is logging rather than crash reporting and
    /// `docs/standards/rust.md` asks for it regardless of this key.
    pub enabled: bool,
}

/// [`Crash`] writes its own [`Default`] for [`Updates`]' reason: a derived one would be `false`,
/// and a crash nobody recorded is a crash nobody can fix — the switch would have to be thrown
/// *before* the first one, which is the moment its answer is always "no".
impl Default for Crash {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// How MixEngine stops paying for services nobody is using — roadmap task **T69**.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Services {
    /// How often the idle sweeper takes a reading, in seconds.
    ///
    /// **This is the unit an idle policy is spent in, not a promise about accuracy.** A service's
    /// `after` is honoured as that many consecutive sweeps that saw it idle, so shortening the
    /// period shortens the observations and not the wait. It has to be that way round for the
    /// reason [`Certs::renew_check_seconds`] explains about the clock underneath: tokio counts no
    /// time while a laptop is suspended, and a reading taken eight hours late is one observation
    /// rather than eight hours of evidence.
    ///
    /// Thirty seconds. It is a key at all because the daemon's own idle suite has to watch the loop
    /// run, and a period no test can move would leave the loop unexercised.
    #[serde(deserialize_with = "idle_check")]
    pub idle_check_seconds: u64,

    /// How many finished minutes over its ceiling a service is given — roadmap task **T71a**.
    ///
    /// **Minutes, and not seconds, because the unit is the metrics row**: the sampler completes one
    /// per subject per minute, and this count is spent in *those* exactly as an idle policy is spent
    /// in sweeps. Shortening it does not make the watchdog notice sooner than the next finished
    /// minute; it changes how many of them a service is allowed.
    ///
    /// Three. Short enough to catch a leak before a laptop starts swapping, long enough that nothing
    /// is ever restarted on a single reading — which is what one minute holds, at the rate this
    /// daemon samples when nobody is watching.
    ///
    /// It is a key at all for [`Services::idle_check_seconds`]' reason: the daemon's own suite has
    /// to watch the count run out, and a constant no test could move would leave that path
    /// unexercised.
    #[serde(deserialize_with = "memory_over_minutes")]
    pub memory_over_minutes: u32,
}

/// The default for [`Services::idle_check_seconds`]: every thirty seconds.
///
/// Short enough that a thirty-minute policy is sixty observations rather than two — one late
/// reading then costs a fraction of the wait rather than half of it — and long enough that the
/// cost of asking is nothing: on macOS each reading is a `lsof`, and in this build no recipe ships
/// an idle default at all, so the usual number of readings per sweep is zero.
const DEFAULT_IDLE_CHECK_SECONDS: u64 = 30;

/// The default for [`Services::memory_over_minutes`]: three finished minutes.
///
/// The argument is on the field. The number is deliberately the same on all three systems: a
/// watchdog that was more patient where it is the only enforcement would be most patient exactly
/// where it matters most.
const DEFAULT_MEMORY_OVER_MINUTES: u32 = 3;

/// [`Services`] writes its own [`Default`] for [`Certs`]' reason: a derived one would be zero,
/// which is the one value this key refuses.
impl Default for Services {
    fn default() -> Self {
        Self {
            idle_check_seconds: DEFAULT_IDLE_CHECK_SECONDS,
            memory_over_minutes: DEFAULT_MEMORY_OVER_MINUTES,
        }
    }
}

/// Refuse a sweep period of zero.
///
/// A floor and no ceiling, as [`renew_check`] has: a very long period makes idle shutdown slow to
/// notice, which is a choice somebody may want on a workstation that is never short of memory.
/// Zero is not a long period — it is a loop with no pause in it, and it would spend a core reading
/// the socket table.
fn idle_check<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let seconds = u64::deserialize(deserializer)?;

    if seconds == 0 {
        return Err(serde::de::Error::custom(format!(
            "an idle check every 0 seconds is a loop with no pause in it rather than a schedule; \
             give it a number of seconds, or remove the key for the default of \
             {DEFAULT_IDLE_CHECK_SECONDS}"
        )));
    }

    Ok(seconds)
}

/// Refuse a watchdog that acts on one finished minute.
///
/// A floor and no ceiling, as [`idle_check`] has, and for a sharper reason: at the rate this daemon
/// samples when nobody is watching, one minute holds one reading — so zero would restart a service
/// on a single instantaneous measurement, which is the one thing the count exists to prevent. A very
/// patient watchdog is a choice somebody may want; an impatient one is a bug with a config key.
fn memory_over_minutes<'de, D>(deserializer: D) -> std::result::Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let minutes = u32::deserialize(deserializer)?;

    if minutes == 0 {
        return Err(serde::de::Error::custom(format!(
            "restarting a service after 0 minutes over its ceiling would act on a single reading; \
             give it a number of minutes, or remove the key for the default of \
             {DEFAULT_MEMORY_OVER_MINUTES}"
        )));
    }

    Ok(minutes)
}

/// How often this home measures what it is running — roadmap task **T71**.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Metrics {
    /// How often a reading is taken while a client is watching, in seconds.
    ///
    /// One second, which is what `features/client-surface.md` promises whoever opens the stream. It
    /// is a key at all for [`Services::idle_check_seconds`]' reason: a suite that has to watch two
    /// frames arrive cannot wait out a period no test can move.
    #[serde(deserialize_with = "sample_period")]
    pub sample_seconds: u64,

    /// How often a reading is taken while nobody is watching, in seconds.
    ///
    /// **The history's own rate, and the one number in this file spent on a machine nobody is
    /// looking at.** `features/resource-isolation.md` promises a history that answers *what was
    /// eating my battery*, and a history kept only while somebody watched would hold exactly the
    /// minutes that needed no recording — the night is not observed by definition.
    ///
    /// Sixty seconds against a reading that costs about 10 ms on Windows and about 2 ms on Linux
    /// (measured; see
    /// [`ProcessMetrics::measure`](mixengine_platform::ProcessMetrics::measure)) is 0.02% of one
    /// core. That ratio is the whole argument for sampling a machine nobody is watching, so a home
    /// that lengthens this loses history rather than gaining anything worth having.
    #[serde(deserialize_with = "sample_period")]
    pub idle_sample_seconds: u64,

    /// How long a minute row is kept, in hours.
    ///
    /// Twenty-four, which is what the feature promises. A key because no test can wait a day to
    /// watch the trim happen. Zero is allowed and means *keep nothing*: unlike a period, it is not a
    /// loop with no pause in it — it is a home that wants the live numbers and no history at all.
    pub retention_hours: u32,
}

/// The default for [`Metrics::sample_seconds`]: a reading a second while somebody is watching.
const DEFAULT_SAMPLE_SECONDS: u64 = 1;

/// The default for [`Metrics::idle_sample_seconds`]: a reading a minute while nobody is.
const DEFAULT_IDLE_SAMPLE_SECONDS: u64 = 60;

/// The default for [`Metrics::retention_hours`]: the day the feature promises.
const DEFAULT_RETENTION_HOURS: u32 = 24;

/// [`Metrics`] writes its own [`Default`] for [`Services`]' reason: a derived one would be zero,
/// which is the value both of its periods refuse.
impl Default for Metrics {
    fn default() -> Self {
        Self {
            sample_seconds: DEFAULT_SAMPLE_SECONDS,
            idle_sample_seconds: DEFAULT_IDLE_SAMPLE_SECONDS,
            retention_hours: DEFAULT_RETENTION_HOURS,
        }
    }
}

/// Refuse a sampling period of zero, on [`idle_check`]'s reasoning.
///
/// Zero is not a short period. It is a loop with no pause in it, and here it would spend a core
/// enumerating this machine's processes for as long as the daemon ran.
fn sample_period<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let seconds = u64::deserialize(deserializer)?;

    if seconds == 0 {
        return Err(serde::de::Error::custom(
            "a reading every 0 seconds is a loop with no pause in it rather than a schedule; give \
             it a number of seconds, or remove the key for the default",
        ));
    }

    Ok(seconds)
}

/// Relocations for the directories that grow without bound.
///
/// Only these four: runtimes and packages are re-downloadable, `data/` and `logs/` are the two that
/// get large. `bin/`, `etc/`, `certs/`, `extensions/`, `blueprints/`, `run/` and `mixengine.db`
/// stay together next to `config.toml`, because a home directory that can be split into eleven
/// pieces is a home directory no uninstaller can promise to remove.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PathOverrides {
    /// Where installed language runtimes go instead of `<root>/runtimes`.
    #[serde(default, deserialize_with = "relocation")]
    pub runtimes: Option<PathBuf>,
    /// Where installed servers and databases go instead of `<root>/packages`.
    #[serde(default, deserialize_with = "relocation")]
    pub packages: Option<PathBuf>,
    /// Where service data goes instead of `<root>/data`.
    #[serde(default, deserialize_with = "relocation")]
    pub data: Option<PathBuf>,
    /// Where logs go instead of `<root>/logs`.
    #[serde(default, deserialize_with = "relocation")]
    pub logs: Option<PathBuf>,
}

impl PathOverrides {
    /// The four keys and what each is set to, in the order the template lists them.
    ///
    /// **One list, walked by everything that has to treat the four alike** — roadmap task **T143**.
    /// The alternative is four copies of the same `match`, and the fifth key somebody adds later
    /// reaching three of them.
    #[must_use]
    pub fn entries(&self) -> [(&'static str, Option<&Path>); 4] {
        [
            ("runtimes", self.runtimes.as_deref()),
            ("packages", self.packages.as_deref()),
            ("data", self.data.as_deref()),
            ("logs", self.logs.as_deref()),
        ]
    }
}

/// What a caller asked to relocate — roadmap task **T143**.
///
/// [`PathOverrides`]'s shape without its serde, because the two are different statements: that one
/// is *what the file says*, and this is *what somebody just asked for*. A key left [`None`] here is
/// a key nobody mentioned, and [`set_paths`] leaves it exactly as the file has it — which is what
/// makes `--data` on its own a request about `data/` and about nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestedPaths {
    /// Where installed language runtimes should go.
    pub runtimes: Option<PathBuf>,
    /// Where installed servers and databases should go.
    pub packages: Option<PathBuf>,
    /// Where service data should go.
    pub data: Option<PathBuf>,
    /// Where logs should go.
    pub logs: Option<PathBuf>,
}

impl RequestedPaths {
    /// The four keys and what each was asked for. [`PathOverrides::entries`]'s shape.
    #[must_use]
    pub fn entries(&self) -> [(&'static str, Option<&Path>); 4] {
        [
            ("runtimes", self.runtimes.as_deref()),
            ("packages", self.packages.as_deref()),
            ("data", self.data.as_deref()),
            ("logs", self.logs.as_deref()),
        ]
    }

    /// Did anybody ask for anything?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries().iter().all(|(_, path)| path.is_none())
    }

    /// Which of the requested keys say something `current` does not already say.
    ///
    /// **An empty answer is the whole of the silent no-op.** A daemon started from a launchd plist
    /// or a shell alias carries the same four flags for the life of that plist; asking for what the
    /// file already says has to be a start that writes nothing and complains about nothing, or the
    /// flag becomes a thing that works once.
    ///
    /// Compared as written rather than as resolved: `bulk/data` and an absolute path to the same
    /// directory are two different answers to *"what should the file say"*, and this function is
    /// only ever asked in order to decide whether to write one.
    #[must_use]
    pub fn differing_from(&self, current: &PathOverrides) -> Vec<&'static str> {
        let held = current.entries();

        self.entries()
            .into_iter()
            .zip(held)
            .filter_map(|((key, asked), (_, have))| {
                let asked = asked?;

                (Some(asked) != have).then_some(key)
            })
            .collect()
    }
}

/// Set the named keys in `[paths]`, leaving every comment where it was — roadmap task **T143**.
///
/// **A `toml_edit` document and not a re-serialised [`Config`]**, for the reason
/// [`crate::manifest::write`] gives about a project manifest and which holds twice over here: this
/// file ships with sixty lines of comments explaining the very keys being written, [`TEMPLATE`]
/// puts them there on first run, and a writer that round-tripped the parsed value would throw all
/// of them away the first time anybody chose a disk.
///
/// **The commented examples are left alone.** `#runtimes = "bulk/runtimes"` stays where the
/// template put it, above the real key this writes. Removing it would mean matching on comment
/// text, which is a parser for prose.
///
/// The file has to be there: this is called after [`load_or_create`], and creating one here would
/// mean a `config.toml` with these four keys and none of the documentation.
///
/// # Errors
///
/// [`Error::Io`] when the file cannot be read or replaced, and [`Error::ConfigEdit`] when it cannot
/// be parsed or when a value is one [`load`] would refuse — the same sentence, from the same check.
pub fn set_paths(path: &Path, requested: &RequestedPaths) -> Result<Vec<&'static str>> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
        action: "read",
        path: path.to_path_buf(),
        source,
    })?;

    let mut document: toml_edit::DocumentMut =
        text.parse()
            .map_err(|error: toml_edit::TomlError| Error::ConfigEdit {
                path: path.to_path_buf(),
                reason: error.to_string(),
            })?;

    // Every value checked before any of them is written: a batch that refused the fourth key after
    // writing three would leave the file saying something nobody asked for.
    let mut written = Vec::new();
    for (key, asked) in requested.entries() {
        let Some(asked) = asked else {
            continue;
        };

        check_relocation(asked).map_err(|reason| Error::ConfigEdit {
            path: path.to_path_buf(),
            reason: format!("{key}: {reason}"),
        })?;

        // TOML is UTF-8, so a path this cannot spell is a path that cannot be written down at all.
        // Refused rather than lossily converted: a `to_string_lossy` here would store a path with
        // a replacement character in it and report success.
        if asked.to_str().is_none() {
            return Err(Error::ConfigEdit {
                path: path.to_path_buf(),
                reason: format!(
                    "{key}: this path is not valid UTF-8, so it cannot be written into a TOML file"
                ),
            });
        }

        written.push(key);
    }

    if written.is_empty() {
        return Ok(written);
    }

    let table = document
        .entry("paths")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));

    let Some(table) = table.as_table_mut() else {
        return Err(Error::ConfigEdit {
            path: path.to_path_buf(),
            reason: "[paths] is there and is not a table".to_owned(),
        });
    };

    // Without this a table created here is *implicit* and renders as nothing at all, so the keys
    // would be written and the file would come back without them.
    table.set_implicit(false);

    for (key, asked) in requested.entries() {
        if let Some(asked) = asked.and_then(Path::to_str) {
            table[key] = toml_edit::value(asked);
        }
    }

    replace_atomically(path, &document.to_string())?;

    Ok(written)
}

/// Write `contents` over `path` without ever leaving a torn file behind.
///
/// A temporary in the same directory, flushed, then renamed — the rule
/// `mixengine_platform`'s own replace states and which this cannot call: that one is
/// `pub(crate)` to its crate and compiled only for the elevated helper, deliberately, so that a
/// third caller cannot reach the mechanism that rewrites `/etc/hosts`. This writes a file inside
/// `MIXENGINE_HOME` and needs none of that one's ownership carrying.
fn replace_atomically(path: &Path, contents: &str) -> Result<()> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let temporary = directory.join(format!(
        "{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));

    let failed = |path: &Path, source: std::io::Error| Error::Io {
        action: "write",
        path: path.to_path_buf(),
        source,
    };

    let staged = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()
    })();

    if let Err(source) = staged {
        let _ = std::fs::remove_file(&temporary);
        return Err(failed(&temporary, source));
    }

    // The Windows case `crate::generate` already carries: a rename replaces an existing file
    // everywhere this runs, except while another process holds the target open without sharing
    // deletion. Removing it first opens a window where the file is absent, which is worse than a
    // rename and better than a start that fails because somebody had the file in an editor.
    if let Err(source) = std::fs::rename(&temporary, path) {
        if !path.exists() {
            let _ = std::fs::remove_file(&temporary);
            return Err(failed(path, source));
        }

        std::fs::remove_file(path).map_err(|source| failed(path, source))?;

        if let Err(source) = std::fs::rename(&temporary, path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(failed(path, source));
        }
    }

    Ok(())
}

/// Refuse a relocation that does not name a directory of its own, or that names one ambiguously.
///
/// `Path::join("")` hands the original path straight back, so `data = ""` would not fail, it would
/// make `data/` *be* `MIXENGINE_HOME`. Everything downstream would then treat the whole install as
/// the service data directory — and "reset the data directory" is a thing MixEngine offers to do.
/// `"."`, `".."`, `"bulk/.."` and `"/"` all end up somewhere just as destructive: the home itself,
/// a directory containing it, or an entire filesystem. A user who wants the default deletes the
/// key.
fn relocation<'de, D>(deserializer: D) -> std::result::Result<Option<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    let path = Option::<PathBuf>::deserialize(deserializer)?;

    if let Some(candidate) = path.as_deref() {
        check_relocation(candidate).map_err(serde::de::Error::custom)?;
    }

    Ok(path)
}

/// The rule [`relocation`] enforces, as a function anything may call — roadmap task **T143**.
///
/// **Lifted out of the deserializer rather than copied beside it.** [`set_paths`] writes the same
/// four keys the file declares, and two lists of refused values are two lists that disagree the
/// first time one of them grows a case. This is the only place the rule exists; the deserializer
/// and the writer are both callers.
///
/// # Errors
///
/// The sentence to show whoever wrote the path — the same sentence either door produces.
fn check_relocation(candidate: &Path) -> std::result::Result<(), String> {
    // "Is it anchored?" before "does it name anything?": both questions are answered by refusing
    // the path, but a half-anchored Windows path has its own diagnosis and deserves to hear it
    // rather than the generic one.
    if is_drive_less_root(candidate) {
        return Err(
            "this path starts at the root of a drive without saying which one; write it \
                    in full (D:\\bulk\\data) or make it relative to the MixEngine home (bulk/data)"
                .to_owned(),
        );
    }

    if is_drive_relative(candidate) {
        return Err(
            "this path names a drive but does not start at its root, so it would land \
                    wherever that drive's current directory happens to be; write it in full \
                    (D:\\bulk\\data) or make it relative to the MixEngine home (bulk/data)"
                .to_owned(),
        );
    }

    if !names_a_directory(candidate) {
        return Err(
            "after resolving `.` and `..` this names no directory of its own — it points \
                    at the MixEngine home, at a directory containing it, or at the root of a \
                    filesystem; remove the key to use the default"
                .to_owned(),
        );
    }

    Ok(())
}

/// Refuse an empty path where a name is required.
///
/// Deliberately *not* the checks [`relocation`] runs: a listening address is not a place data is
/// written to, it is never joined to the home, the platform layer has the final say over what shape
/// it may take (T7), and a socket path resolved against the wrong drive fails loudly at bind time
/// instead of quietly storing a database somewhere nobody will look.
fn named_path<'de, D>(deserializer: D) -> std::result::Result<Option<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    let path = Option::<PathBuf>::deserialize(deserializer)?;

    if path
        .as_deref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(serde::de::Error::custom(
            "the path is empty; remove the key to use the default",
        ));
    }

    Ok(path)
}

/// Does anything survive resolving `.` and `..` — is there a directory here at all?
///
/// Counts how deep the path ends up, purely lexically: `bulk/runtimes` is two down and fine,
/// `bulk/..` is back where it started, `..` is above it, `""`, `"."` and `"/"` never went anywhere.
/// A path that ends at depth zero names the MixEngine home, one of its ancestors, or a filesystem
/// root — and "reset this directory" would then take far more than the directory with it.
///
/// A `..` that climbs and then descends again is left alone: `../bulk` is a sibling of the home,
/// which is an ordinary thing to want and contains nothing dangerous.
fn names_a_directory(path: &Path) -> bool {
    let mut depth = 0usize;

    for part in path.components() {
        match part {
            Component::Normal(_) => depth += 1,
            // Climbing above where we started is a no-op here: the components that follow are what
            // decide whether the path names something, and `../bulk` names `bulk`.
            Component::ParentDir => depth = depth.saturating_sub(1),
            // A share *is* a directory — `\\server\share` is somewhere data can go, and Windows
            // folds the whole of it into the prefix, leaving nothing behind for the count. A drive
            // letter is not: `C:\` is a filesystem, and relocating `data/` onto one whole is the
            // case above.
            Component::Prefix(prefix) => {
                if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..)) {
                    depth += 1;
                }
            }
            Component::CurDir | Component::RootDir => {}
        }
    }

    depth > 0
}

/// `\bulk` or `/bulk` on Windows: rooted, but with no drive to root it to.
///
/// `Path::join` resolves these against the *current* drive rather than against the path it is
/// joined to, so `C:\home\MixEngine`.join("/bulk") is `C:\bulk`. Treating that as "relative to the
/// home" — which is what the surrounding code promises — would be a lie, and treating it as
/// absolute would silently pick a drive the user never named. Refusing is the only honest answer.
///
/// On Unix `has_root()` and `is_absolute()` are the same question, so this is always `false` and
/// the check costs nothing.
fn is_drive_less_root(path: &Path) -> bool {
    path.has_root() && !path.is_absolute()
}

/// `C:bulk` on Windows: a drive named, but not the root of it.
///
/// The mirror image of [`is_drive_less_root`] and just as dishonest. A path carrying a prefix
/// replaces the whole of whatever it is joined to — `C:\home\MixEngine`.join("C:bulk") is plain
/// `C:bulk` — and the OS then resolves it against the *current directory of drive C*, which no
/// part of the configuration file mentions and which changes under the daemon's feet.
///
/// On Unix there are no prefixes, so `C:bulk` is an ordinary directory name and this is `false`.
fn is_drive_relative(path: &Path) -> bool {
    matches!(path.components().next(), Some(Component::Prefix(_))) && !path.is_absolute()
}

/// Refuse a shutdown budget longer than [`MAX_SHUTDOWN_GRACE_SECONDS`].
///
/// Refused rather than quietly lowered, which is the answer this file gives to every other value
/// it will not take: an empty `ipc_path` and a relocation that names nothing are both errors
/// carrying a sentence about what to write instead, and nothing here rewrites a setting behind the
/// user's back. Two things make that the right answer for this key in particular rather than merely
/// the consistent one. A budget is a *promise about how long a stop takes*, so silently turning an
/// hour into ten minutes would be exactly the surprise the key was set to avoid, and it would
/// arrive during a shutdown rather than at the moment the file was edited. And the only place a
/// correction could be announced is the log, which does not exist yet: `config.toml` is what the
/// daemon reads in order to decide how to log at all, so a `warn!` from here is emitted before any
/// subscriber is installed and is seen by nobody.
///
/// Zero needs no floor to go with this. It is a real setting — see
/// [`Daemon::shutdown_grace_seconds`] — and the arithmetic downstream is happy with it.
fn shutdown_grace<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let seconds = u64::deserialize(deserializer)?;

    if seconds > MAX_SHUTDOWN_GRACE_SECONDS {
        return Err(serde::de::Error::custom(format!(
            "a shutdown budget of {seconds} seconds is longer than the \
             {MAX_SHUTDOWN_GRACE_SECONDS} seconds MixEngine accepts; nothing it supervises takes \
             ten minutes to stop, and a budget that never runs out is not one — lower it, or \
             remove the key for the default of {DEFAULT_SHUTDOWN_GRACE_SECONDS}"
        )));
    }

    Ok(seconds)
}

/// Read `config.toml`.
///
/// A missing file is not an error — it means "all defaults", which is exactly what a user who
/// deleted the file is asking for.
///
/// # Errors
///
/// [`Error::Config`] when the file does not parse or contains a key MixEngine does not know;
/// [`Error::Io`] when it exists but cannot be read.
pub fn load(path: &Path) -> Result<Config> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(source) => {
            return Err(Error::Io {
                action: "read",
                path: path.to_path_buf(),
                source,
            });
        }
    };

    toml::from_str(&text).map_err(|source| Error::Config {
        path: path.to_path_buf(),
        source,
    })
}

/// Read `config.toml`, writing the commented template first if there is nothing there.
///
/// # Errors
///
/// As [`load`], plus [`Error::Io`] when the template cannot be written.
pub fn load_or_create(path: &Path) -> Result<Config> {
    write_template(path)?;
    load(path)
}

/// Write [`TEMPLATE`] to `path` unless a file is already there. Returns whether it wrote one.
///
/// An existing file is never touched, not even to add a key introduced by an update: the file
/// belongs to the user. New keys are documented, and their absence means the default, so an old
/// `config.toml` keeps working unchanged.
///
/// # Errors
///
/// [`Error::Io`] when the file cannot be created or written.
pub fn write_template(path: &Path) -> Result<bool> {
    // `create_new` rather than "check, then write": between the check and the write sits every
    // other MixEngine process on this machine, and the loser of that race would truncate a config
    // file the winner had just written.
    let mut file = match std::fs::File::create_new(path) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(source) => {
            return Err(Error::Io {
                action: "create",
                path: path.to_path_buf(),
                source,
            });
        }
    };

    if let Err(source) = file.write_all(TEMPLATE.as_bytes()) {
        // A half-written template is worse than none: it parses, so it looks like a choice the
        // user made, and `create_new` would never replace it. Take it back out of the way and let
        // the next start try again. If even that fails, the write error is still what gets
        // reported — it is the one the user needs.
        drop(file);
        let _ = std::fs::remove_file(path);

        return Err(Error::Io {
            action: "write",
            path: path.to_path_buf(),
            source,
        });
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **On unless the user says otherwise**, and the shipped template says so in words —
    /// roadmap task **T124**.
    #[test]
    fn the_welcome_page_is_on_by_default() {
        let config: Config = toml::from_str("").expect("an empty configuration");
        assert!(config.sites.welcome_page);

        let off: Config = toml::from_str(
            "[sites]
welcome_page = false
",
        )
        .expect("a configuration");
        assert!(!off.sites.welcome_page);

        assert!(
            TEMPLATE.contains("[sites]"),
            "a key nothing in the template mentions is a key nobody finds"
        );
    }

    /// The default, and the one value this key refuses — roadmap task **T71a**.
    ///
    /// A test at all because the key exists for the daemon's own suite to move: one that could not
    /// be set would be a constant with a longer name.
    #[test]
    fn a_memory_watchdog_may_not_act_on_a_single_minute() {
        assert_eq!(
            Services::default().memory_over_minutes,
            DEFAULT_MEMORY_OVER_MINUTES
        );

        assert!(
            toml::from_str::<Config>("[services]\nmemory_over_minutes = 0\n").is_err(),
            "zero would restart a service on one reading, which is what a minute holds when \
             nobody is watching"
        );

        let taken: Config = toml::from_str("[services]\nmemory_over_minutes = 10\n")
            .expect("a number of minutes is a setting");

        assert_eq!(taken.services.memory_over_minutes, 10);
    }
}
