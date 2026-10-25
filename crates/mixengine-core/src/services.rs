//! The `services` table: what the daemon believes about each supervised service between restarts.
//!
//! This module owns every write to that table. Most of it is the columns a *supervisor* writes: the
//! state machine and the guarantee that a transition is either persisted *and* announced or
//! neither, and beside it [`started`] and [`ended`], the two writes that turn a running process
//! into a row crash recovery can meet.
//!
//! **The row's own beginning and end are here too** — [`create`] and [`delete`]. What is *decided*
//! about a new service still happens above: which recipe, whether the package is installed, whether
//! the id's shape suits the recipe's instancing. What is here is the insert and the delete, so that
//! the table has one writer.
//!
//! **php-fpm gave the row a second possible parent.** A service used to be an instance of a
//! `packages` row and nothing else; php-fpm is an instance of a `runtime_installs` one, because the
//! process serving a user's sites lives inside the PHP they installed. [`Origin`] is which of the
//! two a caller means, `services` carries both columns with a `CHECK` that exactly one is set, and
//! the foreign key is what lets `runtime.uninstall` refuse to remove a PHP a pool still points at.
//!
//! `last_started_at` is still not written here, but the question of its format is closed: the
//! column holds epoch milliseconds — a [`mixengine_proto::Timestamp`] verbatim — rather than the
//! ISO-8601 text it was first declared as. It is read back by the supervisor on every exit to place
//! a restart inside or outside the crash-loop window, which makes it a moment the daemon does
//! arithmetic on rather than one a person reads; storing it as text would have bought a date
//! library this workspace needs for nothing else, to parse on the hot path of a restart. The other
//! `_at` columns stay text because nothing branches on them. `0001_initial.sql` was edited rather
//! than migrated, which was possible only because nothing had shipped yet.

mod error;

pub use error::ServiceError;

use std::collections::BTreeMap;

use mixengine_proto::{
    ExtensionId, PackageVersion, RuntimeKind, ServiceId, ServiceState, StateReason, Timestamp,
};

use crate::{Error, Result, Store};

pub mod activation;
mod claim;
pub mod client;
mod data_dir;
pub mod front_end;
pub mod graph;
pub mod handoff;
pub mod pools;
pub mod ports;
pub mod save_resources;
mod transitions;
mod write;

pub use claim::{Claimed, claim, port_movable};
pub use transitions::{ended, started, transition, transition_noting};
use transitions::{exit_code, failure_note, installed_version, listening_port, process_id};
pub use write::{
    autostart_ids, create, delete, idle_minutes, set_autostart, set_idle, set_limits, set_port,
};

pub use graph::{GraphError, Plan, ServiceGraph};

/// What the database says this service is doing.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such service; [`ServiceError::UnknownServiceState`] when
/// the row holds a word this build does not recognise; [`Error::Database`] when the file cannot be
/// read.
pub async fn state(store: &Store, service: &ServiceId) -> Result<ServiceState> {
    let id = service.as_str();

    let stored = sqlx::query_scalar!("SELECT state FROM services WHERE id = ?", id)
        .fetch_optional(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?
        .ok_or_else(|| Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        })?;

    parse_state(service, stored)
}

/// Who left a service stopped.
///
/// **Only meaningful while a service is [`ServiceState::Stopped`]**, and written on every arrival
/// there so it can never be left over from an older stop. On-demand activation is the reader: a stop
/// this answers [`Person`](Self::Person) for is one a connection may not undo, because `mix service
/// stop` followed by the next request restarting the service is the tool overruling its user.
///
/// **Three answers because on-demand activation's first boolean was one short.** `idle_stopped`
/// asked *did the daemon idle this?*, so a row that had never run and a service the daemon itself
/// put down both fell in beside a person's stop — and both were then refused a wake. A machine that
/// was restarted has every pool in that state, which is how a PHP site came to answer 502 until
/// somebody started its pool by hand, against `resource-isolation.md`'s own promise of *no error
/// page, no manual start*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppedBy {
    /// Nobody has. The service has not run yet, or it is not stopped at all.
    Never,

    /// A client asked, through `service.stop` or `service.restart`.
    Person,

    /// MixEngine itself: an idle sweep, its own shutdown, or a process that vanished under it.
    Daemon,
}

/// The wire's spelling of the same three words, which is where a client learns it.
impl From<StoppedBy> for mixengine_proto::StoppedBy {
    fn from(stopped_by: StoppedBy) -> Self {
        match stopped_by {
            StoppedBy::Never => Self::Never,
            StoppedBy::Person => Self::Person,
            StoppedBy::Daemon => Self::Daemon,
        }
    }
}

impl StoppedBy {
    /// The word the column holds.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Person => "person",
            Self::Daemon => "daemon",
        }
    }

    /// Which of the three a stop for this reason is.
    ///
    /// **Two words are a person and every other reason is the machine.** [`StateReason::Requested`]
    /// is what a client's `service.stop` reaches this function with and the only thing that does —
    /// the daemon's own walks each carry a reason of their own ([`StateReason::Shutdown`],
    /// [`StateReason::Idle`], [`StateReason::Vanished`], [`StateReason::Unadopted`]), and so does
    /// every way a process ends without being asked to.
    ///
    /// **[`StateReason::CredentialReset`] is the second, and it is not a person.** It is here
    /// because of what the answer is *used for* rather than what it describes: a reset holds the
    /// data directory while it rewrites the credential inside it, so a connection must not start a
    /// server on top of that work. `Person` is the answer that says so, and the repair puts the
    /// service back itself when it is finished.
    ///
    /// **The `_` arm is why this is worth a test of its own.** A reason added to the protocol and not
    /// named here becomes `Daemon`, which is wakeable, and the compiler says nothing.
    ///
    /// Consulted only on the way into [`ServiceState::Stopped`]; the same word means something else
    /// entirely on a start, and nothing calls this there.
    #[must_use]
    pub fn of(reason: &StateReason) -> Self {
        match reason {
            StateReason::Requested | StateReason::CredentialReset => Self::Person,
            _ => Self::Daemon,
        }
    }

    /// Whether a connection that needs this service may start it again.
    ///
    /// The question both readers ask — the web activator and the address holder — in one place, so
    /// that the answer cannot come out differently on the two paths a wake can arrive by.
    #[must_use]
    pub fn may_be_woken(self) -> bool {
        !matches!(self, Self::Person)
    }

    /// The column's word, read back.
    ///
    /// **A word this build does not know is read as [`Person`](Self::Person)**, which the `CHECK`
    /// makes unreachable and which is the safe direction anyway: refusing a wake costs a 502 and one
    /// `mix service start`, while granting one wrongly undoes somebody's deliberate stop.
    fn parse(service: &str, column: &str) -> Self {
        match column {
            "never" => Self::Never,
            "person" => Self::Person,
            "daemon" => Self::Daemon,
            other => {
                tracing::warn!(
                    service,
                    value = other,
                    "this service's row says it was stopped by something this build cannot read; \
                     treating it as a stop somebody meant"
                );

                Self::Person
            }
        }
    }
}

/// Everything a `services` row says about the process behind a service.
///
/// The columns a supervisor writes, read back in one value. What is **not** in it is the reason for
/// the state: no column holds one, because a reason explains a *move* and the row keeps only where
/// the machine ended up. `DaemonEvent::ServiceStateChanged` is what carries the why, as it happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRecord {
    /// What the row says the service is doing.
    pub state: ServiceState,

    /// Why it last failed. Written with the move into `failed`, cleared by the next move into
    /// `running`; a note this build cannot read is [`None`].
    pub last_failure: Option<mixengine_proto::ServiceFailureNote>,

    /// Who left it stopped. See [`StoppedBy`].
    pub stopped_by: StoppedBy,

    /// The process it is running as, where there is one. Cleared by [`ended`].
    pub pid: Option<u32>,

    /// The port this service was allocated when its row was written, where it has one.
    ///
    /// Read from the row rather than from the rendering, because the row is where it was decided:
    /// see [`Port`].
    pub port: Option<u16>,

    /// When that process began, as the OS counts such moments — a
    /// [`StartTime`](mixengine_platform::process::StartTime) stored verbatim.
    ///
    /// **Half of an identity and useless on its own**, which is why it travels beside the pid and
    /// is cleared with it. Crash recovery is the only reader: it asks the OS when the process
    /// bearing `pid` began and compares the two, because a pid the machine has handed out again
    /// names somebody else's program. A row with a pid and no start time is one adoption refuses —
    /// see [`started`].
    ///
    /// An `i64` rather than the platform type: this crate stores what it is given and does not
    /// interpret it, and the column is `docs/architecture/data-model.md`'s "exists to be
    /// compared, never read".
    pub pid_start_time: Option<i64>,

    /// When it was last started, whether or not it is still running.
    pub last_started_at: Option<Timestamp>,

    /// What its process exited with the last time one ended.
    pub last_exit_code: Option<i32>,

    /// Whether this service starts when the daemon does.
    ///
    /// **Read here rather than through [`declaration`]**, which also carries it. A *listing* wants
    /// this: `declaration` is a four-table join per service, and paying for one per row to report
    /// one boolean would make `service.list` quadratic in the number of services it reports.
    ///
    /// It is a setting and not a reading, which is the one thing in this struct that is: everything
    /// else here is what a supervisor wrote about a process. It travels with them because they are
    /// read in the same statement and answered in the same summary, and splitting it out would buy
    /// a second query per listing to keep a distinction nothing acts on.
    pub autostart: bool,

    /// Which version of its program this service runs.
    ///
    /// Out of whichever parent the row names: the package's for a server, the runtime's for
    /// php-fpm. [`None`] for an extension's service, whose version is the extension's rather than a
    /// program's, and for a stored text this build cannot parse, which a listing reports as a blank
    /// cell rather than failing over.
    pub version: Option<PackageVersion>,
}

/// Where the binary a service runs comes from.
///
/// **Two tables, one of which is not a package.** Everything up to php-fpm was installed from the
/// index by `package.install` and has a `packages` row; a pool has no such row and must not be
/// given a fake one, because the directory it runs out of belongs to `runtime.install` and is
/// removed by `runtime.uninstall`. Which one a service names is what the `CHECK` on `services`
/// enforces, and this enum is that constraint said in Rust, so a caller cannot even assemble the
/// row the database would refuse.
#[derive(Debug, Clone)]
pub enum Origin {
    /// A `packages` row: Caddy, MariaDB, Redis — anything the signed index publishes as a server.
    Package {
        /// `packages.name`, as the caller resolved it from the id.
        ///
        /// Passed rather than read off the id, because the caller has already held it to the
        /// catalogue and this is that answer rather than a second derivation of it.
        name: String,

        /// Which installed version of that package to run.
        version: PackageVersion,
    },

    /// An `extensions` row: an installed extension whose manifest declares a `[service]`.
    ///
    /// **The third origin, and the one that carries its own recipe.** A package's row points at
    /// knowledge compiled into this build; an extension's row carries the manifest it was installed
    /// from, so what runs is what somebody consented to rather than what a later release decided.
    Extension {
        /// Which installed extension.
        id: ExtensionId,
    },

    /// A `runtime_installs` row: php-fpm, whose process lives inside an installed PHP.
    Runtime {
        /// Which language.
        kind: RuntimeKind,

        /// Which installed version of it, in full — `8.3.33` and not `8.3`, because
        /// `runtime_installs` is `UNIQUE (kind, version)` over the full version and two patch
        /// releases of one minor can both be installed.
        version: PackageVersion,
    },
}

/// Where a new service's port comes from.
///
/// **Three answers and not an [`Option`]**, because the missing case is the interesting one: a
/// caller that names no port is not a service without one, it is a service whose recipe names 3306
/// and has to be *given* a number before the row exists. Squeezing that into [`None`] is what left
/// `mariadb@main` with a column holding nothing and a template asking it for a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Port {
    /// The row carries none: a pool listening on a Unix socket, or a service whose ports are its
    /// own settings rather than a number the daemon hands out — Caddy's 80 and 443 are its own.
    None,

    /// The caller named one and is taken at its word.
    ///
    /// **No allocation and no diagnosis.** Somebody who typed `--port 3307` has already decided,
    /// and a daemon that moved them to 3308 because something answered on 3307 would be overruling
    /// the one instruction in the call.
    Fixed(u16),

    /// The recipe's own preferred port, to be allocated at the moment the row is written.
    Allocate {
        /// What the recipe would like: 3306 for either database, 6379 for Redis, 9000 for a pool.
        preferred: u16,
    },
}

/// What a [`create`] wrote down about the port, which the caller has to be able to say out loud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The number in the row, and [`None`] for a service that listens on no port.
    pub port: Option<u16>,

    /// Why it is not the port the recipe preferred, when it is not.
    pub moved_from: Option<mixengine_proto::PortMoved>,
}

/// Everything a new `services` row is made of.
///
/// Taken as one value rather than as nine arguments, on
/// [`packages::Installation`](crate::packages::Installation)'s reasoning: most of them are optional
/// and of similar types, and a caller assembling them positionally would produce a row that is wrong
/// rather than one that fails to insert.
#[derive(Debug, Clone)]
pub struct Declaration {
    /// Which service, which is also which package it is an instance of.
    pub service: ServiceId,

    /// Which table supplies the binary, and which row in it.
    pub origin: Origin,

    /// What goes in `instance_name`, which `UNIQUE (package_id, instance_name)` is enforced over.
    ///
    /// The half after the `@` for a package that has instances, and the package's own name for one
    /// that exists once — a decision that belongs to the recipe and so arrives made.
    pub instance_name: String,

    /// Which port this service gets, and who decides.
    pub port: Port,

    /// The address it binds, or [`None`] for the column's own `127.0.0.1`.
    pub bind_addr: Option<String>,

    /// Where its data lives, or [`None`] for the home's own layout.
    pub data_dir: Option<String>,

    /// Whether it starts with the daemon.
    pub autostart: bool,

    /// The settings this instance overrides, as the document the column holds.
    pub overrides: String,
}

/// What version a service is an instance of, or [`None`] when nothing here can say.
///
/// **Two possible parents and one query** (`0001_initial.sql`): a service comes out of a `packages`
/// row or out of a `runtime_installs` one. Both carry the version, and a caller asking "what should
/// a colleague install" does not care which table answered.
///
/// [`None`] rather than an error for a version this build cannot parse: the reader is
/// `project.export`, and a link dropped from the file to punish an unreadable version would lose
/// the thing the file exists to send.
///
/// # Errors
///
/// [`Error::Database`] when the tables cannot be read.
pub async fn version(store: &Store, service: &ServiceId) -> Result<Option<PackageVersion>> {
    let id = service.as_str();

    let found = sqlx::query_scalar!(
        "SELECT coalesce(p.version, r.version) AS version
         FROM services s
         LEFT JOIN packages p ON p.id = s.package_id
         LEFT JOIN runtime_installs r ON r.id = s.runtime_install_id
         WHERE s.id = ?",
        id
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    .flatten();

    Ok(installed_version(found))
}

/// One service's row.
///
/// # Errors
///
/// As [`state`], whose narrower question this answers as well: [`Error::NotFound`] when there is no
/// such service, [`ServiceError::UnknownServiceState`] when the row holds a word this build does
/// not recognise, [`Error::Database`] when the file cannot be read.
pub async fn record(store: &Store, service: &ServiceId) -> Result<ServiceRecord> {
    let id = service.as_str();

    let row = sqlx::query!(
        r#"SELECT s.state, s.pid, s.pid_start_time, s.last_started_at, s.last_exit_code, s.port,
                  s.stopped_by, s.autostart, s.last_failure_json,
                  coalesce(p.version, r.version) AS version
           FROM services s
           LEFT JOIN packages p ON p.id = s.package_id
           LEFT JOIN runtime_installs r ON r.id = s.runtime_install_id
           WHERE s.id = ?"#,
        id
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    .ok_or_else(|| Error::NotFound {
        kind: "service",
        id: id.to_owned(),
    })?;

    Ok(ServiceRecord {
        state: parse_state(service, row.state)?,
        last_failure: failure_note(id, row.last_failure_json.as_deref()),
        stopped_by: StoppedBy::parse(id, &row.stopped_by),
        pid: process_id(row.pid),
        pid_start_time: row.pid_start_time,
        last_started_at: row.last_started_at.map(Timestamp),
        last_exit_code: exit_code(row.last_exit_code),
        port: listening_port(row.port),
        autostart: row.autostart != 0,
        version: installed_version(row.version),
    })
}

/// One service's row, read back as the [`Declaration`] that wrote it.
///
/// **The inverse of [`create`], and it is a value rather than a query so that the two cannot
/// drift.** Its caller is `service.set_front_end`, which deletes the row a home's front end is
/// before it creates the row the new one will be: a create that will not render has to put the old
/// row back exactly, and *exactly* is not something a caller can reassemble from a summary. So the
/// declaration is read while the row is still there, and the rollback is a second [`create`] of the
/// same value.
///
/// **The port comes back as [`Port::Fixed`] and never as [`Port::Allocate`].** A restore is not a
/// creation: the number in the column was decided once, is in somebody's `.env` by now, and must
/// come back as itself rather than be searched for again — which is the whole of what
/// [`Port::Fixed`] means.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such service, [`ServiceError::UnreadableServiceRow`] when
/// the row names no parent at all or holds a kind or a version this build cannot read, and
/// [`Error::Database`] when the file cannot be read.
pub async fn declaration(store: &Store, service: &ServiceId) -> Result<Declaration> {
    let id = service.as_str();

    let row = sqlx::query!(
        "SELECT s.instance_name, s.autostart, s.port, s.bind_addr, s.data_dir,
                s.config_overrides_json, s.extension_id,
                p.name AS package, p.version AS package_version,
                r.kind AS runtime_kind, r.version AS runtime_version
         FROM services s
         LEFT JOIN packages p ON p.id = s.package_id
         LEFT JOIN runtime_installs r ON r.id = s.runtime_install_id
         WHERE s.id = ?",
        id
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    .ok_or_else(|| Error::NotFound {
        kind: "service",
        id: id.to_owned(),
    })?;

    let unreadable = |column: &'static str, value: String| {
        Error::Service(ServiceError::UnreadableServiceRow {
            service: id.to_owned(),
            column,
            value,
        })
    };

    // The `CHECK` on the table says exactly one parent is set, so this is a match on which — and
    // the `else` is the hand-edited row that constraint is there to catch.
    let origin = match (row.package, row.extension_id, row.runtime_kind) {
        (Some(name), _, _) => {
            let version = row
                .package_version
                .ok_or_else(|| unreadable("packages.version", "nothing".to_owned()))?;

            Origin::Package {
                name,
                version: PackageVersion::parse(version.clone())
                    .map_err(|_| unreadable("packages.version", version))?,
            }
        }

        (_, Some(extension), _) => Origin::Extension {
            id: ExtensionId::parse(extension.clone())
                .map_err(|_| unreadable("services.extension_id", extension))?,
        },

        (_, _, Some(kind)) => {
            let version = row
                .runtime_version
                .ok_or_else(|| unreadable("runtime_installs.version", "nothing".to_owned()))?;

            Origin::Runtime {
                kind: RuntimeKind::parse(&kind)
                    .ok_or_else(|| unreadable("runtime_installs.kind", kind.to_string()))?,
                version: PackageVersion::parse(version.clone())
                    .map_err(|_| unreadable("runtime_installs.version", version))?,
            }
        }

        (None, None, None) => {
            return Err(unreadable("package_id", "no parent at all".to_owned()));
        }
    };

    Ok(Declaration {
        service: service.clone(),
        origin,
        instance_name: row.instance_name,
        port: match listening_port(row.port) {
            Some(port) => Port::Fixed(port),
            None => Port::None,
        },
        bind_addr: Some(row.bind_addr),
        data_dir: row.data_dir,
        autostart: row.autostart != 0,
        overrides: row.config_overrides_json,
    })
}

/// Every service's row, keyed by the id the row itself holds.
///
/// **One query rather than one per service**, because the caller is answering `service.list` and a
/// question per declared service would be a round trip per declared service to say the same thing.
///
/// The key is the stored string and not a [`ServiceId`]: the caller already holds the ids it is
/// asking about — they came from the declarations, not from here — and parsing a column back into
/// one would give a hand-edited row the power to fail a listing that does not even mention it.
///
/// # Errors
///
/// [`ServiceError::UnknownServiceState`] when a row holds a state word this build does not
/// recognise, and [`Error::Database`] when the file cannot be read. Not [`Error::NotFound`]: a home
/// with no services has no rows, which is an answer and not a failure.
pub async fn records(store: &Store) -> Result<BTreeMap<String, ServiceRecord>> {
    let rows = sqlx::query!(
        r#"SELECT s.id, s.state, s.pid, s.pid_start_time, s.last_started_at, s.last_exit_code,
                  s.port, s.stopped_by, s.autostart, s.last_failure_json,
                  coalesce(p.version, r.version) AS version
           FROM services s
           LEFT JOIN packages p ON p.id = s.package_id
           LEFT JOIN runtime_installs r ON r.id = s.runtime_install_id"#
    )
    .fetch_all(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    rows.into_iter()
        .map(|row| {
            let state = ServiceState::parse(&row.state).ok_or_else(|| {
                Error::Service(ServiceError::UnknownServiceState {
                    service: row.id.clone(),
                    value: row.state,
                })
            })?;

            let stopped_by = StoppedBy::parse(&row.id, &row.stopped_by);
            let last_failure = failure_note(&row.id, row.last_failure_json.as_deref());

            Ok((
                row.id,
                ServiceRecord {
                    state,
                    last_failure,
                    stopped_by,
                    pid: process_id(row.pid),
                    pid_start_time: row.pid_start_time,
                    last_started_at: row.last_started_at.map(Timestamp),
                    last_exit_code: exit_code(row.last_exit_code),
                    port: listening_port(row.port),
                    autostart: row.autostart != 0,
                    version: installed_version(row.version),
                },
            ))
        })
        .collect()
}

/// Turn the stored word into a state, blaming the row rather than the reader.
///
/// The `CHECK` constraint on the column means nothing this build wrote can land here, so a failure
/// is a database edited by hand or written by a version that knew a state this one does not — and
/// naming the service is what makes either one findable.
fn parse_state(service: &ServiceId, stored: String) -> Result<ServiceState> {
    ServiceState::parse(&stored).ok_or_else(|| {
        Error::Service(ServiceError::UnknownServiceState {
            service: service.as_str().to_owned(),
            value: stored,
        })
    })
}

#[cfg(test)]
mod tests;
