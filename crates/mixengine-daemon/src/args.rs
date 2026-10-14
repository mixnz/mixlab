//! The daemon's command line: every flag `mixengined` takes, and the values `run` and
//! `serve` are handed out of it.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use mixengine_core::config;

use super::VERSION;
use crate::{credentials, runtimes, updates};

/// Command line of the daemon. Configuration enters the program here and is passed down; nothing
/// deeper reads the environment on its own.
#[derive(Debug, Parser)]
#[command(name = "mixengined", version = VERSION, about = "MixEngine daemon")]
pub(crate) struct Args {
    /// Root directory for everything MixEngine owns.
    ///
    /// Defaults to the OS convention (`%LOCALAPPDATA%\MixEngine`,
    /// `~/Library/Application Support/MixEngine`, `$XDG_DATA_HOME/mixengine`). Point it somewhere
    /// disposable while experimenting — this is the only thing separating a sandbox from a real
    /// install.
    #[arg(long, env = "MIXENGINE_HOME", value_name = "DIR")]
    pub(crate) home: Option<PathBuf>,

    /// Put installed language runtimes here instead of `<root>/runtimes`.
    ///
    /// **This writes `config.toml` rather than configuring one process** — roadmap task **T144**,
    /// and it is the one flag on this binary that does. Where a runtime lives is recorded in
    /// `runtime_installs.install_path` when it is installed, so a value that applied to one start
    /// would let a daemon a service manager launched and one a terminal launched disagree about a
    /// home while the database agreed with neither. A setting that lives in the home cannot do
    /// that.
    ///
    /// Asking for what the file already says is a silent no-op, so a launchd plist or a shell
    /// alias may carry this for the life of that plist. Asking for something else once anything is
    /// installed **fails the start** rather than being ignored, on `--log-format`'s reasoning:
    /// moving an installed home is a file move and a rewrite of those rows, which this does not do.
    ///
    /// Relative to the home, or absolute. The same values `[paths]` refuses are refused here.
    #[arg(long, value_name = "DIR")]
    runtimes: Option<PathBuf>,

    /// Put installed servers and databases here instead of `<root>/packages`. See `--runtimes`.
    #[arg(long, value_name = "DIR")]
    packages: Option<PathBuf>,

    /// Put service data here instead of `<root>/data`. See `--runtimes`.
    #[arg(long, value_name = "DIR")]
    data: Option<PathBuf>,

    /// Put logs here instead of `<root>/logs`. See `--runtimes`.
    ///
    /// This start's own log lines are already being written when the value is applied, so the move
    /// takes effect at the next start and `daemon.log` stays where it was for this one.
    #[arg(long, value_name = "DIR")]
    logs: Option<PathBuf>,

    /// Print where this home's directories are, as JSON, and exit — roadmap task **T145**.
    ///
    /// **A read that creates nothing.** No home, no `config.toml`, no database: the caller is a
    /// window drawing its "choose a disk" screen before any daemon has run, and an answer that
    /// created the home it was asked about would make the question itself the reason the choice was
    /// no longer free. A machine before its first start answers with the default layout and
    /// `changeable: free`, which is the true answer and the one that screen needs.
    ///
    /// It is here rather than in `mix` because only this binary can open the database, and whether
    /// the choice is still free is a question about rows.
    #[arg(
        long,
        conflicts_with_all = ["detach", "runtimes", "packages", "data", "logs"]
    )]
    pub(crate) storage: bool,

    /// Start the daemon in the background and print the endpoint it is listening on.
    ///
    /// Without this the daemon stays in the foreground, which is what a service manager wants —
    /// systemd, launchd and Task Scheduler all supervise the process themselves and would treat a
    /// process that forked away as one that had died. This flag exists for the other caller: a
    /// client that finds no daemon running and starts one (roadmap task T10) cannot sit holding it.
    ///
    /// It returns only once the daemon answers on its endpoint, so a client that gets a zero exit
    /// status can connect immediately rather than retrying against a daemon that may still be
    /// migrating a database.
    #[arg(long)]
    pub(crate) detach: bool,

    /// How much to log. Overrides `log.level` in `config.toml`.
    #[arg(long, value_enum)]
    pub(crate) log_level: Option<LogLevel>,

    /// How to shape each log line. Overrides `log.format` in `config.toml`.
    ///
    /// The environment variable is the one a log collector sets: it wraps a command it did not
    /// write, so it cannot add a flag to it. A value neither this build nor the collector
    /// recognises fails the start rather than being ignored — silently text-formatted output is a
    /// log nobody is reading.
    #[arg(long, value_enum, env = "MIXENGINE_LOG_FORMAT")]
    pub(crate) log_format: Option<LogFormat>,

    /// Where credentials are kept: `os`, the machine's credential store, or `home`, a private file
    /// in the home — roadmap task **T184**, ADR 0052.
    ///
    /// A release uses `os` and refuses `home`. Any other build defaults to `home`, because an
    /// unsigned daemon is a stranger to the Keychain after every rebuild and asks for the login
    /// password to read what the last one wrote. Pass `os` to a development build to work on the
    /// hand-off to MixLab, which reads the machine's store.
    #[arg(long, value_enum, env = "MIXENGINE_CREDENTIAL_STORE")]
    pub(crate) credential_store: Option<credentials::Store>,

    /// Read the package index from here instead of the one MixEngine publishes.
    ///
    /// A team mirror, or a test's own registry. `docs/operations/runtime-packaging.md` promises
    /// this and promises that the signature requirement stays — which is why it is useless without
    /// the flag below, and why the two are read together.
    #[arg(long, env = "MIXENGINE_INDEX_URL", value_name = "URL")]
    pub(crate) index_url: Option<String>,

    /// Verify that index against this minisign public key instead of the compiled-in one.
    ///
    /// **Overriding it is trusting a different publisher**, and nothing about that is hidden: only
    /// somebody who already controls how this daemon starts can set it, and a daemon started with it
    /// says so in its log. The alternative — a URL that can move while the key cannot — would be a
    /// mirror setting that can only ever fail, since nobody else can sign with our key.
    #[arg(
        long,
        env = "MIXENGINE_INDEX_KEY",
        value_name = "KEY",
        requires = "index_url"
    )]
    pub(crate) index_key: Option<String>,

    /// Read the update feed from here instead of the one MixEngine publishes.
    ///
    /// A staging release, or a test's own server. The index's mechanism verbatim, and for its
    /// reason: useless without the flag below, and the two are read together — roadmap task **T88**.
    #[arg(long, env = "MIXENGINE_UPDATE_URL", value_name = "URL")]
    pub(crate) update_url: Option<String>,

    /// Verify that feed against this minisign public key instead of the compiled-in one.
    ///
    /// **Overriding it is trusting a different publisher with the binaries this machine runs as
    /// itself**, which is a heavier thing than trusting one with a package index — and nothing about
    /// it is hidden: only somebody who already controls how this daemon starts can set it, and a
    /// daemon started with it says so in its log.
    #[arg(
        long,
        env = "MIXENGINE_UPDATE_KEY",
        value_name = "KEY",
        requires = "update_url"
    )]
    pub(crate) update_key: Option<String>,
}

impl Args {
    /// The four relocations this start was asked for — roadmap task **T144**.
    ///
    /// Assembled here rather than read one field at a time further down, on the rule the rest of
    /// this impl follows: configuration enters the program at `main` and is passed down.
    pub(crate) fn requested_paths(&self) -> mixengine_core::config::RequestedPaths {
        mixengine_core::config::RequestedPaths {
            runtimes: self.runtimes.clone(),
            packages: self.packages.clone(),
            data: self.data.clone(),
            logs: self.logs.clone(),
        }
    }

    /// Where the package index comes from: what was asked for, or what MixEngine publishes.
    ///
    /// The one place either value is read. Configuration enters at `main` and is passed down —
    /// `docs/standards/rust.md` — so nothing below this reaches for an environment variable to
    /// find out where to download from.
    fn index_source(&self) -> runtimes::IndexSource {
        let default = runtimes::IndexSource::default();

        runtimes::IndexSource {
            url: self.index_url.clone().unwrap_or(default.url),
            public_key: self.index_key.clone().unwrap_or(default.public_key),
        }
    }

    /// Where the update feed comes from: what was asked for, or what MixEngine publishes.
    ///
    /// [`Args::index_source`]'s twin, and separate from it because the two documents are signed by
    /// two different keys on purpose — a compromise of the packaging repository must not hand
    /// somebody the right to sign the `mixengined` a machine runs as itself.
    fn feed_source(&self) -> updates::FeedSource {
        let default = updates::FeedSource::default();

        updates::FeedSource {
            url: self.update_url.clone().unwrap_or(default.url),
            public_key: self.update_key.clone().unwrap_or(default.public_key),
        }
    }

    /// Both of them, as [`serve`] takes them.
    ///
    /// [`serve`]: crate::serve
    pub(crate) fn sources(&self) -> Sources {
        Sources {
            index: self.index_source(),
            feed: self.feed_source(),
        }
    }
}

/// What this process brought with it from `main` beside its configuration: its own path, and where
/// its credentials live (T184). Grouped for [`Sources`]' reason — `serve` is at clippy's seven.
#[derive(Debug)]
pub(crate) struct Launch {
    /// The running binary — see the note where `run` reads it.
    pub(crate) program: PathBuf,

    /// The store `serve` builds its host with.
    pub(crate) credentials: mixengine_platform::Credentials,

    /// What `daemon.status` says about that store — roadmap task T194.
    pub(crate) credential_facts: credentials::Facts,
}

/// The two signed documents this daemon reads, and what verifies each.
///
/// **One argument and not two**, which is what the note inside [`serve`] predicted when T88 was
/// still ahead of it: seven parameters is what clippy allows, and the update feed would have been
/// the eighth. Grouping them is also the truer shape — they are the same kind of thing, they are
/// overridden the same way, and a third signed document would join them here rather than widen a
/// signature again.
///
/// [`serve`]: crate::serve
#[derive(Debug)]
pub(crate) struct Sources {
    /// The package index: what can be installed, and where to get it.
    pub(crate) index: runtimes::IndexSource,

    /// The update feed: whether there is a newer MixEngine — roadmap task **T88**.
    pub(crate) feed: updates::FeedSource,
}

/// Verbosity of the daemon log.
///
/// A closed set on purpose: a free-form string would let a typo silence logging entirely, and the
/// process would start looking perfectly healthy while saying nothing. It mirrors
/// [`config::LogLevel`] because `clap` belongs to the binary and `core` must not depend on it.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for config::LogLevel {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => Self::Error,
            LogLevel::Warn => Self::Warn,
            LogLevel::Info => Self::Info,
            LogLevel::Debug => Self::Debug,
            LogLevel::Trace => Self::Trace,
        }
    }
}

/// Shape of each log line, mirroring [`config::LogFormat`] for the same reason as [`LogLevel`].
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum LogFormat {
    Text,
    Json,
}

impl From<LogFormat> for config::LogFormat {
    fn from(format: LogFormat) -> Self {
        match format {
            LogFormat::Text => Self::Text,
            LogFormat::Json => Self::Json,
        }
    }
}

/// The spelling a flag's value has on a command line, for the child a `--detach` start builds.
///
/// Asked of `clap` rather than written out a second time. A table repeating these by hand drifts the
/// moment a variant is renamed, and it drifts silently: the child would be started with a value this
/// same binary no longer accepts, and nothing would say so until somebody ran `--detach`.
pub(crate) fn as_arg(value: impl ValueEnum) -> String {
    value
        .to_possible_value()
        .expect("every value of a MixEngine flag is one clap can spell — none of them is skipped")
        .get_name()
        .to_owned()
}
