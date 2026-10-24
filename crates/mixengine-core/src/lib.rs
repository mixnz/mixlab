//! The domain: what a project, site, runtime and service *are*.
//!
//! Storage and platform access arrive as injected traits, so every rule in here — version
//! resolution, config rendering, blueprint diffing — is testable without touching the machine.
//! Modules are organised by capability (`sites/`, `runtimes/`, `certs/`), never by layer.
//!
//! `core` never depends on `daemon`.

#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use mixengine_platform::Host;

pub mod adopt;
pub mod bin_commands;
pub mod blueprints;
pub mod certs;
pub mod config;
pub mod domains;
pub mod elevation;
pub mod extensions;
pub mod generate;
pub mod home;
pub mod hosts;
pub mod index;
pub mod install;
pub mod jobs;
pub mod lines;
pub mod manifest;
pub mod metrics;
pub mod packages;
pub mod paths;
pub mod prerequisites;
pub mod projects;
pub mod requirements;
pub mod resolve;
pub mod runtimes;
pub mod services;
pub mod shims;
pub mod sites;
pub mod storage;
pub mod store;
pub mod updates;
pub mod upgrade;
pub mod window;

pub use blueprints::BlueprintError;
pub use config::Config;
pub use elevation::ElevationError;
pub use extensions::ExtensionError;
pub use install::InstallError;
pub use jobs::JobError;
pub use paths::Paths;
pub use projects::ProjectError;
pub use runtimes::RuntimeError;
pub use services::ServiceError;
pub use store::Store;
pub use updates::UpdateError;

/// An opened MixEngine home: the user's preferences and the directory layout they produce.
#[derive(Debug, Clone)]
pub struct Home {
    /// What the user asked for in `config.toml`.
    pub config: Config,
    /// Where everything lives, with any `[paths]` override already applied.
    pub paths: Paths,
}

/// Open the MixEngine home directory: resolve it, read its configuration, create what is missing.
///
/// This is the first thing a daemon or CLI process does, and the only place the four steps are
/// ordered — they depend on each other. `config.toml` lives *inside* the root, so the root has to
/// be resolved and created before the configuration that may relocate `runtimes/` or `data/` can
/// be read at all.
///
/// `root_override` is `MIXENGINE_HOME` (or `--home`), already read at `main`; `None` means "use
/// what this OS considers the right place". Running it twice is a no-op — every step is
/// idempotent, which is what makes `mix doctor` able to reuse it.
///
/// # Errors
///
/// [`Error::Platform`] when the OS cannot say where user data belongs and no override was given,
/// or when a directory cannot be made private; [`Error::EmptyHome`] when the override is present
/// but empty; [`Error::Config`] when `config.toml` does not parse; and [`Error::Io`] when a
/// directory cannot be created.
pub fn open_home(root_override: Option<&Path>, host: &dyn Host) -> Result<Home> {
    let root = paths::resolve_root(root_override, host)?;
    paths::create_dir(&root)?;

    let config = config::load_or_create(&root.join(config::FILE_NAME))?;
    let paths = Paths::new(root, &config.paths);
    paths.bootstrap(host)?;

    Ok(Home { config, paths })
}

/// Failure of a domain operation.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The requested entity does not exist.
    #[error("no such {kind}: {id}")]
    NotFound {
        /// The kind of entity, named as its RPC namespace: `"site"`, `"project"`, `"runtime"`,
        /// `"service"`, `"job"`, `"domain"`, `"blueprint"`, `"extension"`.
        ///
        /// Not free text. The daemon turns this into the hint `mix <kind> list`, which is a
        /// command only because the namespaces in `docs/architecture/daemon-and-ipc.md` are
        /// also the nouns the CLI uses — a `kind` invented outside that list would send the user
        /// to a command that does not exist.
        kind: &'static str,
        /// The identifier that was looked up.
        id: String,
    },

    /// A file or directory under `MIXENGINE_HOME` could not be touched.
    ///
    /// The path is part of the message because "permission denied" on its own has never helped
    /// anybody: on a `[paths]` override pointing at an unmounted disk, the path *is* the answer.
    // The OS error is the `#[source]`, not part of this message: `anyhow` in the binaries and the
    // wire error at the daemon boundary both walk the chain, and a message that repeats its own
    // cause prints it twice.
    #[error("cannot {action} {}", path.display())]
    Io {
        /// What was being attempted, e.g. `"create"`.
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// `config.toml` is not valid.
    ///
    /// Unknown keys count: a silently ignored typo is a setting the user believes is in effect.
    #[error("{} is not valid configuration", path.display())]
    Config {
        /// The configuration file that failed to parse.
        path: PathBuf,
        /// The parse failure, which carries the line, the column and the accepted keys.
        #[source]
        source: toml::de::Error,
    },

    /// `config.toml` could not be written to.
    ///
    /// [`Error::Config`]'s sibling on the write path, and separate from it for
    /// [`ProjectError::ManifestEdit`]'s reason: the first says the user's file is wrong, and this
    /// says this build could not put something into a file that may be perfectly right. It also
    /// carries a value this build refused, which is a third thing again — the file is fine, the
    /// request is not.
    #[error("{} could not be edited: {reason}", path.display())]
    ConfigEdit {
        /// The configuration file.
        path: PathBuf,
        /// What stopped it, phrased for whoever wrote the value.
        reason: String,
    },

    /// The database could not be opened or read.
    ///
    /// Shaped like [`Error::Io`] and for the same reason: the path is the answer often enough —
    /// a `[paths]` root on a disk nobody mounted, a home directory copied from another account —
    /// that leaving it out would waste the user's afternoon.
    #[error("cannot {action} the database at {}", path.display())]
    Database {
        /// What was being attempted, e.g. `"open"`.
        action: &'static str,
        /// The database file.
        path: PathBuf,
        /// What SQLite said.
        #[source]
        source: sqlx::Error,
    },

    /// The copy that has to exist before a migration could not be written.
    ///
    /// Its own variant rather than an [`Error::Io`] because of what happens next: the upgrade is
    /// abandoned. A migration is the one operation here that can destroy declared state, and
    /// running it without the copy that undoes it is not a degraded mode, it is a gamble.
    #[error("cannot copy the database to {}", path.display())]
    Backup {
        /// Where the copy was going.
        path: PathBuf,
        /// What SQLite said.
        #[source]
        source: sqlx::Error,
    },

    /// A migration failed while running, which means the SQL in this build is wrong.
    ///
    /// Not something a user can act on — the database is left as the failed migration's
    /// transaction found it, and the fix is a new release.
    #[error("cannot bring the database at {} up to date", path.display())]
    Migration {
        /// The database file.
        path: PathBuf,
        /// Which migration, and how it failed.
        #[source]
        source: sqlx::migrate::MigrateError,
    },

    /// The database on disk was written by a build whose migrations are not this build's.
    ///
    /// A newer MixEngine that has since been downgraded, a file copied from another machine, or an
    /// upgrade that stopped half way. Distinct from [`Error::Migration`] because the user *can* act
    /// on it: the copy taken before the last upgrade is sitting next to it.
    #[error("the database at {} was written by a different version of MixEngine", path.display())]
    IncompatibleDatabase {
        /// The database file.
        path: PathBuf,
        /// Which version does not line up, and how.
        #[source]
        source: sqlx::migrate::MigrateError,
    },

    /// This home's `settings` holds no id, or holds something that is not one.
    ///
    /// `0001_initial.sql` writes one at the first migration and keeps it through every later one,
    /// so a home reaching this has a database that skipped the migration or was edited by hand.
    ///
    /// **Refused rather than replaced.** Every credential this home has stored is addressed under
    /// the id it had; minting a second one would answer this call and orphan all of them at once,
    /// which is the outage a home id exists to remove rather than a recovery from it.
    #[error("this home has no id, so a credential cannot say which home it belongs to")]
    HomeHasNoId,

    /// A set of service specs does not form a dependency graph.
    ///
    /// Transparent because [`services::GraphError`] already says everything there is to say and
    /// names the services it is about — wrapping it in a sentence of ours would only push the useful
    /// half one level further down a cause chain.
    #[error(transparent)]
    Graph(#[from] services::GraphError),

    /// A certificate could not be made.
    ///
    /// Generating a key pair or signing with it, which fails only when the machine's own crypto
    /// does — a kernel with no CSPRNG, a build whose backend refused the curve. **Not** the error
    /// for a certificate that is *there* and unusable: that is a state rather than a failed
    /// operation, and it travels as [`mixengine_proto::CaState`] because it is a fact about the
    /// home and not about this call.
    #[error("cannot {action} {subject}")]
    Certificate {
        /// What was being attempted, e.g. `"sign"`.
        action: &'static str,
        /// What it was being attempted on, phrased for a person.
        subject: String,
        /// The underlying failure.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// `MIXENGINE_HOME` (or `--home`) was given, but empty.
    ///
    /// Distinct from "not given": the user meant to point somewhere and the value went missing on
    /// the way, so the platform default would be the one place they did *not* ask for.
    #[error("MIXENGINE_HOME is empty; unset it to use this platform's default location")]
    EmptyHome,

    /// The OS refused to answer a question only it can answer.
    #[error(transparent)]
    Platform(#[from] mixengine_platform::Error),

    /// The ways a job's row and its life can fail — see [`JobError`].
    #[error(transparent)]
    Job(#[from] jobs::JobError),

    /// The ways asking the privileged helper can fail — see [`ElevationError`].
    #[error(transparent)]
    Elevation(#[from] elevation::ElevationError),

    /// The ways updating MixEngine and its helper can fail — see [`UpdateError`].
    #[error(transparent)]
    Update(#[from] updates::UpdateError),

    /// The ways reading, naming and fetching a blueprint can fail — see [`BlueprintError`].
    #[error(transparent)]
    Blueprint(#[from] blueprints::BlueprintError),

    /// The ways recording, resolving and restoring runtimes and packages can fail. — see
    /// [`RuntimeError`].
    #[error(transparent)]
    Runtime(#[from] runtimes::RuntimeError),

    /// The ways a project, its manifest, its sites and their domains can be wrong. — see
    /// [`ProjectError`].
    #[error(transparent)]
    Project(#[from] projects::ProjectError),

    /// The ways an extension, its manifest and its registry can be wrong. — see [`ExtensionError`].
    #[error(transparent)]
    Extension(#[from] extensions::ExtensionError),

    /// The ways reading the signed index and installing what it names can fail. — see
    /// [`InstallError`].
    #[error(transparent)]
    Install(#[from] install::InstallError),

    /// The ways a service's state, settings, generated configuration and ports can be wrong. — see
    /// [`ServiceError`].
    #[error(transparent)]
    Service(#[from] services::ServiceError),
}

/// Result of a domain operation.
pub type Result<T> = std::result::Result<T, Error>;
