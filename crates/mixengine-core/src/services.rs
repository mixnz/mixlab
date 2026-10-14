//! The `services` table: what the daemon believes about each supervised service between restarts.
//!
//! This module owns every write to that table. Most of it is the columns a *supervisor* writes:
//! what T14 owns is the state machine and the guarantee that a transition is either persisted *and*
//! announced or neither, and what T19 added beside it is [`started`] and [`ended`], the two writes
//! that turn a running process into a row the adoption after a daemon restart (T18) can meet.
//!
//! **T31a added the row's own beginning and end** — [`create`] and [`delete`] — which until then
//! were a comment here saying Phase 3 would bring them. What is *decided* about a new service still
//! happens above: which recipe, whether the package is installed, whether the id's shape suits the
//! recipe's instancing. What is here is the insert and the delete, so that the table has one writer.
//!
//! **T32 gave the row a second possible parent.** A service used to be an instance of a `packages`
//! row and nothing else; php-fpm is an instance of a `runtime_installs` one, because the process
//! serving a user's sites lives inside the PHP they installed. [`Origin`] is which of the two a
//! caller means, `services` carries both columns with a `CHECK` that exactly one is set, and the
//! foreign key is what lets `runtime.uninstall` refuse to remove a PHP a pool still points at.
//!
//! `last_started_at` is still not written here, but the question T14 left open is now closed: the
//! column holds epoch milliseconds — a [`mixengine_proto::Timestamp`] verbatim — rather than the
//! ISO-8601 text it was first declared as. It is read back by the supervisor on every exit to place
//! a restart inside or outside the crash-loop window, which makes it a moment the daemon does
//! arithmetic on rather than one a person reads; storing it as text would have bought a date library
//! this workspace needs for nothing else, to parse on the hot path of a restart. The other `_at`
//! columns stay text because nothing branches on them. `0001_initial.sql` was edited rather than
//! migrated for the same reason T14 edited it: nothing has shipped, so forward-only has nothing yet
//! to protect.

mod error;

pub use error::ServiceError;

use std::collections::BTreeMap;

use mixengine_proto::{
    ExtensionId, PackageVersion, ResourceLimits, RuntimeKind, ServiceId, ServiceState,
    ServiceTransition, StateReason, Timestamp,
};

use crate::{Error, Result, Store};

pub mod activation;
pub mod client;
mod data_dir;
pub mod front_end;
pub mod graph;
pub mod handoff;
pub mod pools;
pub mod ports;
pub mod save_resources;

pub use graph::{GraphError, Plan, ServiceGraph};

/// What the database says this service is doing.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such service; [`ServiceError::UnknownServiceState`] when the row
/// holds a word this build does not recognise; [`Error::Database`] when the file cannot be read.
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

/// Who left a service stopped — roadmap task **T123**.
///
/// **Only meaningful while a service is [`ServiceState::Stopped`]**, and written on every arrival
/// there so it can never be left over from an older stop. On-demand activation is the reader: a stop
/// this answers [`Person`](Self::Person) for is one a connection may not undo, because `mix service
/// stop` followed by the next request restarting the service is the tool overruling its user.
///
/// **Three answers because T70's boolean was one short.** `idle_stopped` asked *did the daemon idle
/// this?*, so a row that had never run and a service the daemon itself put down both fell in beside
/// a person's stop — and both were then refused a wake. A machine that was restarted has every pool
/// in that state, which is how a PHP site came to answer 502 until somebody started its pool by
/// hand, against `resource-isolation.md`'s own promise of *no error page, no manual start*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppedBy {
    /// Nobody has. The service has not run yet, or it is not stopped at all.
    Never,

    /// A client asked, through `service.stop` or `service.restart`.
    Person,

    /// MixEngine itself: an idle sweep, its own shutdown, or a process that vanished under it.
    Daemon,
}

/// The wire's spelling of the same three words — roadmap task **T167d**, where a client learns it.
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
    /// **[`StateReason::CredentialReset`] is the second, and it is not a person** — roadmap task
    /// **T127**. It is here because of what the answer is *used for* rather than what it describes: a
    /// reset holds the data directory while it rewrites the credential inside it, so a connection
    /// must not start a server on top of that work. `Person` is the answer that says so, and the
    /// repair puts the service back itself when it is finished.
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

    /// Why it last failed — roadmap task **T200b**, D5. Written with the move into `failed`,
    /// cleared by the next move into `running`; a note this build cannot read is [`None`].
    pub last_failure: Option<mixengine_proto::ServiceFailureNote>,

    /// Who left it stopped — roadmap tasks **T70** and **T123**. See [`StoppedBy`].
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
    /// **Half of an identity and useless on its own**, which is why it travels beside the pid and is
    /// cleared with it. Crash recovery (T18) is the only reader: it asks the OS when the process
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

    /// Whether this service starts when the daemon does — roadmap task **T112**.
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

    /// Which version of its program this service runs — roadmap task **T183**.
    ///
    /// Out of whichever parent the row names: the package's for a server, the runtime's for
    /// php-fpm. [`None`] for an extension's service, whose version is the extension's rather than a
    /// program's, and for a stored text this build cannot parse, which a listing reports as a blank
    /// cell rather than failing over.
    pub version: Option<PackageVersion>,
}

/// Where the binary a service runs comes from.
///
/// **Two tables, one of which is not a package** — T32. Everything up to php-fpm was installed from
/// the index by `package.install` and has a `packages` row; a pool has no such row and must not be
/// given a fake one, because the directory it runs out of belongs to `runtime.install` and is
/// removed by `runtime.uninstall`. Which one a service names is what the `CHECK` on `services`
/// enforces, and this enum is that constraint said in Rust, so a caller cannot even assemble the row
/// the database would refuse.
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

    /// An `extensions` row: an installed extension whose manifest declares a `[service]` —
    /// roadmap task **T81**.
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

/// Where a new service's port comes from — roadmap task **T34c**.
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

/// Write down a service somebody asked for.
///
/// **The row and nothing else.** Whether the package has a recipe, whether that version is
/// installed and whether the id's shape suits the recipe are decided by the caller, and rendering
/// the configuration happens after this returns — which is what makes [`delete`] the rollback for a
/// rendering that failed.
///
/// # Errors
///
/// [`ServiceError::ServiceAlreadyDeclared`] when a row with this id exists or the parent already has an
/// instance of this name, [`ServiceError::DataDirectoryTaken`] when another service is already pointed at
/// the `data_dir` this one names, [`Error::NotFound`] when the [`Origin`] names a package or a
/// runtime that is not installed, and [`Error::Database`] when the row cannot be written.
pub async fn create(
    store: &Store,
    host: &dyn mixengine_platform::Host,
    declaration: &Declaration,
) -> Result<Written> {
    let Declaration {
        service,
        origin,
        instance_name,
        port,
        bind_addr,
        data_dir,
        autostart,
        overrides,
    } = declaration;

    let id = service.as_str();
    let autostart_column = i64::from(*autostart);

    // Held until this function returns, which is what makes the number below still free when the
    // row that claims it lands — see [`ports::hold`].
    let _allocating = ports::hold().await;

    // Inside that lock for the same reason the port search is: two calls naming one directory both
    // read a table neither of them has written to yet, and both are told it is free.
    if let Some(path) = data_dir
        && let Some(holder) = data_dir::held_by(store, path).await?
    {
        return Err(Error::Service(ServiceError::DataDirectoryTaken {
            path: path.clone(),
            holder,
        }));
    }

    let chosen = match *port {
        Port::None => Written {
            port: None,
            moved_from: None,
        },

        Port::Fixed(port) => Written {
            port: Some(port),
            moved_from: None,
        },

        Port::Allocate { preferred } => {
            let bind = ports::bind_address(bind_addr.as_deref());
            let allocation = ports::allocate(store, host, bind, preferred).await?;

            Written {
                port: Some(allocation.port),
                moved_from: allocation.moved_from,
            }
        }
    };

    let port_column = chosen.port.map(i64::from);

    // Checked here as well as by the caller, because the alternative is a constraint violation
    // whose message names a column: the row and the lookup are one statement otherwise, and a
    // subquery that found nothing is not a failure SQLite explains.
    let (package_id, runtime_install_id, extension_id) = match origin {
        Origin::Extension { id } => {
            let id_column = id.as_str();

            let found: Option<String> =
                sqlx::query_scalar!("SELECT id FROM extensions WHERE id = ?", id_column)
                    .fetch_optional(store.pool())
                    .await
                    .map_err(|source| store.failure("read", source))?;

            let found = found.ok_or_else(|| Error::NotFound {
                kind: "extension",
                id: id_column.to_owned(),
            })?;

            (None, None, Some(found))
        }

        Origin::Package { name, version } => {
            let version_column = version.as_str();

            let found: Option<i64> = sqlx::query_scalar!(
                "SELECT id FROM packages WHERE name = ? AND version = ?",
                name,
                version_column
            )
            .fetch_optional(store.pool())
            .await
            .map_err(|source| store.failure("read", source))?;

            let found = found.ok_or_else(|| Error::NotFound {
                kind: "package",
                id: format!("{name} {version}"),
            })?;

            (Some(found), None, None)
        }

        Origin::Runtime { kind, version } => {
            let kind_column = kind.as_str();
            let version_column = version.as_str();

            let found: Option<i64> = sqlx::query_scalar!(
                "SELECT id FROM runtime_installs WHERE kind = ? AND version = ?",
                kind_column,
                version_column
            )
            .fetch_optional(store.pool())
            .await
            .map_err(|source| store.failure("read", source))?;

            let found = found.ok_or_else(|| Error::NotFound {
                kind: "runtime",
                id: format!("{kind_column} {version_column}"),
            })?;

            (None, Some(found), None)
        }
    };

    let written = sqlx::query!(
        "INSERT INTO services
             (id, package_id, runtime_install_id, extension_id, instance_name, state, autostart,
              port, bind_addr, data_dir, config_overrides_json)
         VALUES (?, ?, ?, ?, ?, 'stopped', ?, ?, COALESCE(?, '127.0.0.1'), ?, ?)
         ON CONFLICT DO NOTHING",
        id,
        package_id,
        runtime_install_id,
        extension_id,
        instance_name,
        autostart_column,
        port_column,
        bind_addr,
        data_dir,
        overrides
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    // `DO NOTHING` over every unique constraint rather than letting one raise: the id, and one
    // `(parent, instance_name)` per kind of parent — and what a person did wrong is the same in all
    // of those cases, they asked for a service that is already here.
    if written.rows_affected() == 0 {
        return Err(Error::Service(ServiceError::ServiceAlreadyDeclared {
            service: service.clone(),
        }));
    }

    tracing::info!(%id, origin = ?origin, "a service was created");

    Ok(chosen)
}

/// Remove a service's row, and say what its `data_dir` column held.
///
/// **The column verbatim, not the directory it resolves to.** [`None`] is a row that left the
/// placement to the home's layout, and only the generator knows what that layout made of it — so the
/// caller reconstructs it, and this stays a function about a table.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such row, and [`Error::Database`] when it cannot be written.
pub async fn delete(store: &Store, service: &ServiceId) -> Result<Option<String>> {
    let id = service.as_str();

    let removed = sqlx::query_scalar!("DELETE FROM services WHERE id = ? RETURNING data_dir", id)
        .fetch_optional(store.pool())
        .await
        .map_err(|source| store.failure("write", source))?
        .ok_or_else(|| Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        })?;

    tracing::info!(%id, "a service was deleted");

    Ok(removed)
}

/// Replace how long a service may look idle before it is stopped — roadmap task **T69**.
///
/// **Three states, and the caller says which one it means.** `None` writes SQL `NULL`, "use
/// whatever the recipe wants"; `Some(0)` writes `0`, "never, whatever the recipe wants"; `Some(n)`
/// is minutes. They are different rows on purpose — a default arriving in a later release must
/// reach the first and must never reach the second, and once both have been stored as `NULL` no
/// migration can tell them apart again.
///
/// Writes the row and nothing else. The policy reaches a running service the way `limits_json`
/// does: through the next generation pass, which the registry makes at the top of every
/// `service.*` call.
///
/// # Errors
///
/// [`Error::NotFound`] when no such service is declared, and the store's own failure when the write
/// does not land.
pub async fn set_idle(store: &Store, service: &ServiceId, minutes: Option<u32>) -> Result<()> {
    let id = service.as_str();
    let minutes = minutes.map(i64::from);

    let changed = sqlx::query!(
        "UPDATE services SET idle_minutes = ? WHERE id = ?",
        minutes,
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?
    .rows_affected();

    if changed == 0 {
        return Err(Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        });
    }

    tracing::info!(%id, ?minutes, "a service's idle policy was replaced");

    Ok(())
}

/// Replace whether this service starts when the daemon does — roadmap task **T112**.
///
/// [`set_idle`]'s shape, and for its reason: the column is the whole of the setting, and what reads
/// it is a walk that has not happened yet. **Nothing is started or stopped by this call.**
///
/// **A `bool` and not an `Option<bool>`.** `set_idle` takes three states because an absent value
/// restores the recipe's own default; no recipe declares an autostart, so there is no third state
/// for one to restore.
///
/// # Errors
///
/// [`Error::NotFound`] when no row has that id — the same refusal [`set_idle`] gives, and for the
/// same reason: a setting accepted for a name nobody has is a row nobody can read back.
/// [`Error::Database`] when the file cannot be written.
pub async fn set_autostart(store: &Store, service: &ServiceId, autostart: bool) -> Result<()> {
    let id = service.as_str();
    let column = i64::from(autostart);

    let changed = sqlx::query!("UPDATE services SET autostart = ? WHERE id = ?", column, id)
        .execute(store.pool())
        .await
        .map_err(|source| store.failure("write", source))?
        .rows_affected();

    if changed == 0 {
        return Err(Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        });
    }

    tracing::info!(%id, autostart, "a service's autostart setting was replaced");

    Ok(())
}

/// The services whose `autostart` column is set — roadmap task **T113**.
///
/// **The ids and not the rows**, because what the caller does with them is build a
/// [`ServiceGraph::start_plan`](crate::services::graph::ServiceGraph::start_plan): the plan pulls in
/// what each one depends on, so a walk built from a filtered *listing* would be a walk that starts a
/// pool before its database.
///
/// A row holding an id this build cannot parse is passed over rather than failing the read, on
/// [`records`]'s reasoning: a hand-edited row gets to describe a service nobody starts, not to stop
/// every other service on the machine from starting.
///
/// # Errors
///
/// [`Error::Database`] when the table cannot be read.
pub async fn autostart_ids(store: &Store) -> Result<Vec<ServiceId>> {
    let rows = sqlx::query_scalar!("SELECT id FROM services WHERE autostart != 0 ORDER BY id")
        .fetch_all(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    Ok(rows
        .iter()
        .filter_map(|id| ServiceId::parse(id).ok())
        .collect())
}

/// What `services.idle_minutes` holds for this service — roadmap task **T69**.
///
/// The raw column rather than a resolved duration, because resolving it needs the recipe's default
/// and this crate's callers hold the catalogue that has it. What turns the two into an answer a
/// person reads is [`IdleSource::of`](mixengine_proto::IdleSource::of).
///
/// # Errors
///
/// [`Error::NotFound`] when no such service is declared, and the store's own failure when the read
/// does not land.
pub async fn idle_minutes(store: &Store, service: &ServiceId) -> Result<Option<i64>> {
    let id = service.as_str();

    let row = sqlx::query!(
        r#"SELECT idle_minutes AS "idle_minutes: i64" FROM services WHERE id = ?"#,
        id
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    .ok_or_else(|| Error::NotFound {
        kind: "service",
        id: id.to_owned(),
    })?;

    Ok(row.idle_minutes)
}

/// Replace what a service may take — roadmap task **T68**.
///
/// **The whole value, never a delta.** `limits_json` holds a complete `ResourceLimits`, so a write
/// that merged would need to read first, and two clients capping one service would then race over
/// which read won. Writing the document the caller handed over is one statement about one row.
///
/// Writes the row and nothing else: applying the ceilings to a *running* process is the daemon's, in
/// `Registry::set_limits`, because this crate has no supervisor. A service that is stopped needs
/// nothing else — the next spawn reads this row.
///
/// # Errors
///
/// [`Error::NotFound`] when no such service is declared, and the store's own failure when the write
/// does not land.
pub async fn set_limits(store: &Store, service: &ServiceId, limits: ResourceLimits) -> Result<()> {
    let id = service.as_str();
    // Three scalar fields and an enum: this cannot fail, and the alternative to saying so is an
    // error variant no caller could ever act on. `recipe.rs` makes the same claim the same way.
    let document = serde_json::to_string(&limits).expect("a set of resource limits serialises");

    let changed = sqlx::query!(
        "UPDATE services SET limits_json = ? WHERE id = ?",
        document,
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?
    .rows_affected();

    if changed == 0 {
        return Err(Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        });
    }

    tracing::info!(%id, %document, "a service's resource limits were replaced");

    Ok(())
}

/// What version a service is an instance of, or [`None`] when nothing here can say.
///
/// **Two possible parents and one query** (`0001_initial.sql`): a service comes out of a `packages`
/// row or, since T32, out of a `runtime_installs` one. Both carry the version, and a caller asking
/// "what should a colleague install" does not care which table answered.
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
/// such service, [`ServiceError::UnknownServiceState`] when the row holds a word this build does not
/// recognise, [`Error::Database`] when the file cannot be read.
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

/// One service's row, read back as the [`Declaration`] that wrote it — roadmap task **T97**.
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
/// [`Error::NotFound`] when there is no such service, [`ServiceError::UnreadableServiceRow`] when the row
/// names no parent at all or holds a kind or a version this build cannot read, and
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
/// [`ServiceError::UnknownServiceState`] when a row holds a state word this build does not recognise, and
/// [`Error::Database`] when the file cannot be read. Not [`Error::NotFound`]: a home with no
/// services has no rows, which is an answer and not a failure.
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

/// A stored failure note, or [`None`] where there is none or it is one this build cannot read —
/// roadmap task **T200b**. Said in `daemon.log` and not refused: a note is an explanation, and a row
/// whose explanation a newer build wrote must still list.
fn failure_note(
    service: &str,
    stored: Option<&str>,
) -> Option<mixengine_proto::ServiceFailureNote> {
    let stored = stored?;

    match serde_json::from_str(stored) {
        Ok(note) => Some(note),
        Err(error) => {
            tracing::warn!(service, %error, "a failure note this build cannot read is ignored");
            None
        }
    }
}

/// A stored pid, or [`None`] where the column holds something no pid could be.
///
/// [`started`] writes an `i64` widened from a `u32`, so the narrowing here cannot lose one this
/// build wrote. A value that does not fit is a hand-edited row, and "no process" is the safer of the
/// two readings available: the alternative is handing a number to something that will signal it.
fn process_id(stored: Option<i64>) -> Option<u32> {
    stored.and_then(|pid| u32::try_from(pid).ok())
}

/// The same, for a port — [`create`] writes a `u16` and nothing else ever writes the column.
///
/// A row holding a number no port could be is answered as no port rather than as a failure, on
/// [`process_id`]'s reasoning: what a hand-edited database gets to do is describe a service with no
/// port, not take a listing down.
fn listening_port(stored: Option<i64>) -> Option<u16> {
    stored.and_then(|port| u16::try_from(port).ok())
}

/// The same, for a version — a text this build cannot parse is a blank cell, not a failed listing
/// (T183).
fn installed_version(stored: Option<String>) -> Option<PackageVersion> {
    stored.and_then(|version| PackageVersion::parse(version).ok())
}

/// The same, for an exit code — [`ended`] writes an `i32`.
fn exit_code(stored: Option<i64>) -> Option<i32> {
    stored.and_then(|code| i32::try_from(code).ok())
}

/// Move a service to `to`, and hand back the transition that was written.
///
/// The return value is the whole point: it is a [`ServiceTransition`], which is also what
/// [`mixengine_proto::DaemonEvent::ServiceStateChanged`] carries. The caller publishes the value
/// this function persisted rather than describing the same event a second time, so the row and the
/// event cannot drift — and because it only comes back on success, an event is impossible to
/// publish for a transition that did not happen.
///
/// `at` is passed in rather than read from the clock here for the same reason
/// [`Timestamp::from_system_time`] takes a [`std::time::SystemTime`]: the caller already has a
/// reading, and a test needs to be able to say when.
///
/// **The read and the write are one `BEGIN IMMEDIATE` transaction.** Two supervisors racing — a
/// health check going `Degraded` while a user's `service.stop` arrives — must not both see `Running`
/// and both write, and the plain `BEGIN` sqlx issues by default would not stop them: it takes no
/// lock, so the `SELECT` only pins a read snapshot and the `UPDATE` that follows has to *upgrade* to
/// a writer. In WAL mode that upgrade fails with `SQLITE_BUSY_SNAPSHOT` the moment anybody else has
/// committed since the snapshot — and SQLite deliberately does **not** run the busy handler for it,
/// because no amount of waiting can resolve it while this transaction still holds its old read. The
/// [`crate::Store`]'s `busy_timeout` would be bypassed and the loser would get a bare database
/// error instead of an answer.
///
/// `BEGIN IMMEDIATE` takes the write lock up front, so the two supervisors serialise at the `BEGIN`
/// — where `busy_timeout` does apply — and the second one reads the state the first one committed
/// and re-judges its move against it. The `WHERE state = ?` on the `UPDATE` stays as a cheap
/// assertion that this is really so; [`ServiceError::StateRaced`] is what it reports if it ever is not.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such service; [`ServiceError::IllegalTransition`] when the machine
/// has no such edge, which is a bug in the caller rather than a condition to handle;
/// [`ServiceError::StateRaced`] when something else changed the state in between;
/// [`ServiceError::UnknownServiceState`] when the row holds a word this build does not recognise; and
/// [`Error::Database`] when the file cannot be written — including when the write lock could not be
/// taken within the store's `busy_timeout`.
pub async fn transition(
    store: &Store,
    service: &ServiceId,
    to: ServiceState,
    reason: StateReason,
    at: Timestamp,
) -> Result<ServiceTransition> {
    transition_noting(store, service, to, reason, None, at).await
}

/// [`transition`], carrying the sentence that explains a move into `failed` — roadmap task
/// **T200b**, its design's D5.
///
/// **In the transition's own transaction**, so a row cannot say `failed` without saying why: a note
/// written beside it could be the one write that did not land. `detail` is used only for a move into
/// `failed`, and the reason's own words stand in when it is [`None`]. A move into `running` clears
/// the note — a failure that has been recovered from is history.
///
/// # Errors
///
/// As [`transition`].
pub async fn transition_noting(
    store: &Store,
    service: &ServiceId,
    to: ServiceState,
    reason: StateReason,
    detail: Option<&str>,
    at: Timestamp,
) -> Result<ServiceTransition> {
    let id = service.as_str();

    // Not `begin()`: that is a deferred `BEGIN`, which would leave the `UPDATE` below to upgrade a
    // read snapshot into a write and fail unrecoverably against any concurrent writer. See above.
    let mut tx = store
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|source| store.failure("write", source))?;

    let stored = sqlx::query_scalar!("SELECT state FROM services WHERE id = ?", id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|source| store.failure("read", source))?
        .ok_or_else(|| Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        })?;

    let from = parse_state(service, stored)?;

    if !from.can_become(to) {
        return Err(Error::Service(ServiceError::IllegalTransition {
            service: id.to_owned(),
            from,
            to,
        }));
    }

    let (next, current) = (to.as_str(), from.as_str());

    // **Written on the transition into `stopped`, never separately** — roadmap tasks T70 and T123.
    // On-demand activation may start a service a connection needed, and must not do it to one a
    // person stopped; a transition is not stored anywhere, so a daemon that restarts would
    // otherwise forget who had stopped which of its services. Set on every arrival at `stopped`, so
    // it can never be left over from an older stop, and reset on the way out so that a row which is
    // not stopped says what is true of it: nobody has stopped this.
    let stopped_by = match to {
        ServiceState::Stopped => StoppedBy::of(&reason),
        _ => StoppedBy::Never,
    }
    .as_str();

    let updated = sqlx::query!(
        "UPDATE services SET state = ?, stopped_by = ? WHERE id = ? AND state = ?",
        next,
        stopped_by,
        id,
        current
    )
    .execute(&mut *tx)
    .await
    .map_err(|source| store.failure("write", source))?;

    if updated.rows_affected() == 0 {
        return Err(Error::Service(ServiceError::StateRaced {
            service: id.to_owned(),
            expected: from,
        }));
    }

    // **T200b, D5.** The note moves with the state, in this transaction.
    let note = match to {
        ServiceState::Failed => Some(
            serde_json::to_string(&mixengine_proto::ServiceFailureNote {
                at,
                reason: reason.clone(),
                detail: detail.map_or_else(|| reason.to_string(), str::to_owned),
            })
            .expect("a note is made of strings and a timestamp"),
        ),
        _ => None,
    };

    if note.is_some() || to == ServiceState::Running {
        sqlx::query!(
            "UPDATE services SET last_failure_json = ? WHERE id = ?",
            note,
            id
        )
        .execute(&mut *tx)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    tx.commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    tracing::info!(service = id, %from, to = %to, ?reason, "service state changed");

    Ok(ServiceTransition {
        service: service.clone(),
        from,
        to,
        reason,
        at,
    })
}

/// Record the process this service is now running as.
///
/// **The pair is the point.** A pid on its own is not an identity — the OS reuses the number within
/// minutes — so what makes a row adoptable after a daemon restart (T18) is `pid` *and* the moment
/// that process began, read by
/// [`Supervised::started_at`](mixengine_platform::process::Supervised::started_at) while the child
/// is still held. `pid_start_time` is [`None`] where that reading could not be made — a process that
/// ended in its first milliseconds is the ordinary case — and a null column is the honest answer for
/// it: adoption refuses a row it cannot identify, where a zero would look like a reading.
///
/// Separate from [`transition`] rather than folded into it, because these are facts about a process
/// and not an edge in the machine: the state was already written when the service reached
/// `Starting`, and a spawn is what happens after that.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such service, and [`Error::Database`] when the file cannot
/// be written.
pub async fn started(
    store: &Store,
    service: &ServiceId,
    pid: u32,
    pid_start_time: Option<i64>,
    at: Timestamp,
) -> Result<()> {
    let id = service.as_str();
    let (pid, at) = (i64::from(pid), at.0);

    let updated = sqlx::query!(
        "UPDATE services SET last_started_at = ?, pid = ?, pid_start_time = ? WHERE id = ?",
        at,
        pid,
        pid_start_time,
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    if updated.rows_affected() == 0 {
        return Err(Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        });
    }

    Ok(())
}

/// Record that the process is gone, and what it exited with.
///
/// Clearing `pid` and `pid_start_time` is the half that matters and is why this is not optional
/// bookkeeping: a row that keeps a dead pid is a row the next daemon adopts, and the number will by
/// then belong to something else. `code` is [`None`] where the OS reports none — a Unix process
/// killed by a signal — for the same reason [`mixengine_proto::StateReason::Exited`] carries an
/// option there: writing `0` for it would say "clean exit" about a crash.
///
/// # Errors
///
/// As [`started`].
pub async fn ended(store: &Store, service: &ServiceId, code: Option<i32>) -> Result<()> {
    let id = service.as_str();

    let updated = sqlx::query!(
        "UPDATE services SET pid = NULL, pid_start_time = NULL, last_exit_code = ? WHERE id = ?",
        code,
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    if updated.rows_affected() == 0 {
        return Err(Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        });
    }

    Ok(())
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
