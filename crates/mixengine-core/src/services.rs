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
/// [`Error::NotFound`] when there is no such service; [`Error::UnknownServiceState`] when the row
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
/// [`Error::ServiceAlreadyDeclared`] when a row with this id exists or the parent already has an
/// instance of this name, [`Error::DataDirectoryTaken`] when another service is already pointed at
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
        return Err(Error::DataDirectoryTaken {
            path: path.clone(),
            holder,
        });
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
        return Err(Error::ServiceAlreadyDeclared {
            service: service.clone(),
        });
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
/// such service, [`Error::UnknownServiceState`] when the row holds a word this build does not
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
/// [`Error::NotFound`] when there is no such service, [`Error::UnreadableServiceRow`] when the row
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

    let unreadable = |column: &'static str, value: String| Error::UnreadableServiceRow {
        service: id.to_owned(),
        column,
        value,
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
/// [`Error::UnknownServiceState`] when a row holds a state word this build does not recognise, and
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
            let state =
                ServiceState::parse(&row.state).ok_or_else(|| Error::UnknownServiceState {
                    service: row.id.clone(),
                    value: row.state,
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
/// assertion that this is really so; [`Error::StateRaced`] is what it reports if it ever is not.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such service; [`Error::IllegalTransition`] when the machine
/// has no such edge, which is a bug in the caller rather than a condition to handle;
/// [`Error::StateRaced`] when something else changed the state in between;
/// [`Error::UnknownServiceState`] when the row holds a word this build does not recognise; and
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
        return Err(Error::IllegalTransition {
            service: id.to_owned(),
            from,
            to,
        });
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
        return Err(Error::StateRaced {
            service: id.to_owned(),
            expected: from,
        });
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
    ServiceState::parse(&stored).ok_or_else(|| Error::UnknownServiceState {
        service: service.as_str().to_owned(),
        value: stored,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// A `services` row, without the Phase 3 machinery that will eventually create one.
    ///
    /// A service cannot exist without a parent even in a test — the `CHECK` demands one of the two
    /// columns and `foreign_keys=ON` demands the row it names — which is the constraint doing its
    /// job, not an obstacle to route around.
    async fn service_row(store: &Store, id: &str, state: ServiceState) -> ServiceId {
        sqlx::query(
            "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES (?, '1.0.0', '/packages/x', '2026-08-12T00:00:00Z', 'https://example', 'ab')
             ON CONFLICT (name, version) DO NOTHING",
        )
        .bind(id)
        .execute(store.pool())
        .await
        .expect("a package for the service to belong to");

        sqlx::query(
            "INSERT INTO services (id, package_id, instance_name, state)
             VALUES (?, (SELECT id FROM packages WHERE name = ?), 'main', ?)",
        )
        .bind(id)
        .bind(id)
        .bind(state.as_str())
        .execute(store.pool())
        .await
        .expect("the service row");

        ServiceId::parse(id).expect("a valid id")
    }

    /// A `packages` row for a service to be an instance of.
    async fn package(store: &Store, name: &str) {
        sqlx::query(
            "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES (?, '1.0.0', '/packages/x', '2026-08-21T00:00:00Z', 'https://example', 'ab')",
        )
        .bind(name)
        .execute(store.pool())
        .await
        .expect("a package for the service to belong to");
    }

    /// What a caller hands [`create`], with everything but the port left at its plainest.
    fn declaration(id: &str, package: &str, port: Port) -> Declaration {
        Declaration {
            service: ServiceId::parse(id).expect("a valid id"),
            origin: Origin::Package {
                name: package.to_owned(),
                version: PackageVersion::parse("1.0.0").expect("a version"),
            },
            instance_name: "main".to_owned(),
            port,
            bind_addr: None,
            data_dir: None,
            autostart: false,
            overrides: "{}".to_owned(),
        }
    }

    /// Each of the column's three states round-trips, and each is distinguishable afterwards.
    ///
    /// The pair that matters is `Some(0)` and `None`: both are a service that will not be stopped,
    /// and a client that could not tell them apart would say "switched off here" to somebody whose
    /// real answer is "no default exists yet" — and send them to change a setting that was never
    /// the cause.
    #[tokio::test]
    async fn idle_minutes_round_trips_through_all_three_states() {
        let (_home, store) = store().await;
        let service = service_row(&store, "fakeservice", ServiceState::Stopped).await;

        assert_eq!(
            idle_minutes(&store, &service).await.expect("the column"),
            None,
            "a row nobody has set is null, not zero"
        );

        set_idle(&store, &service, Some(45)).await.expect("a write");
        assert_eq!(
            idle_minutes(&store, &service).await.expect("the column"),
            Some(45)
        );

        set_idle(&store, &service, Some(0)).await.expect("a write");
        assert_eq!(
            idle_minutes(&store, &service).await.expect("the column"),
            Some(0),
            "zero is stored as zero rather than collapsed back to null"
        );

        set_idle(&store, &service, None).await.expect("a write");
        assert_eq!(
            idle_minutes(&store, &service).await.expect("the column"),
            None
        );
    }

    /// A service nobody declared is a `NotFound` rather than a silent no-op.
    ///
    /// `UPDATE … WHERE id = ?` affects no rows and reports success, so without this check a client
    /// that misspelled a service id would be told its setting was applied.
    #[tokio::test]
    async fn setting_idle_on_a_service_that_is_not_declared_is_refused() {
        let (_home, store) = store().await;
        let missing = ServiceId::parse("nothing@here").expect("a valid id");

        assert!(matches!(
            set_idle(&store, &missing, Some(10)).await,
            Err(Error::NotFound {
                kind: "service",
                ..
            })
        ));

        assert!(matches!(
            idle_minutes(&store, &missing).await,
            Err(Error::NotFound {
                kind: "service",
                ..
            })
        ));
    }

    async fn store() -> (tempfile::TempDir, Store) {
        let home = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(&home.path().join(crate::paths::DATABASE_FILE_NAME))
            .await
            .expect("a database");
        (home, store)
    }

    const NOW: Timestamp = Timestamp(1_760_000_000_000);

    #[tokio::test]
    async fn a_transition_is_written_and_handed_back() {
        let (_home, store) = store().await;
        let id = service_row(&store, "caddy", ServiceState::Stopped).await;

        let change = transition(
            &store,
            &id,
            ServiceState::Starting,
            StateReason::Requested,
            NOW,
        )
        .await
        .expect("stopped can start");

        assert_eq!(change.from, ServiceState::Stopped);
        assert_eq!(change.to, ServiceState::Starting);
        assert_eq!(change.service, id);
        assert_eq!(change.at, NOW);

        assert_eq!(
            state(&store, &id).await.expect("the row"),
            ServiceState::Starting,
            "the value handed back is the value that survived the commit"
        );
    }

    /// **T200b, D5.** A move into `failed` keeps the sentence; a move into `running` forgets it.
    #[tokio::test]
    async fn a_failure_is_noted_with_its_sentence_and_forgotten_once_running() {
        let (_home, store) = store().await;
        let id = service_row(&store, "pool", ServiceState::Starting).await;

        transition_noting(
            &store,
            &id,
            ServiceState::Failed,
            StateReason::SpawnFailed,
            Some("the environment entry X: no credential is stored at mixengine/a/b"),
            NOW,
        )
        .await
        .expect("a starting service can fail");

        let note = record(&store, &id)
            .await
            .expect("the row")
            .last_failure
            .expect("a note");
        assert_eq!(note.reason, StateReason::SpawnFailed);
        assert_eq!(note.at, NOW);
        assert!(
            note.detail.contains("no credential is stored"),
            "{}",
            note.detail
        );

        for to in [ServiceState::Starting, ServiceState::Running] {
            transition(&store, &id, to, StateReason::Requested, NOW)
                .await
                .expect("an edge the machine has");
        }
        assert_eq!(
            record(&store, &id).await.expect("the row").last_failure,
            None,
            "a failure that has been recovered from is history"
        );
    }

    /// **T200b, D5.** With no sentence, the reason's own words are the note — a client never
    /// translates a `StateReason`.
    #[tokio::test]
    async fn a_failure_without_a_sentence_is_noted_in_the_reasons_words() {
        let (_home, store) = store().await;
        let id = service_row(&store, "pool", ServiceState::Starting).await;

        transition(
            &store,
            &id,
            ServiceState::Failed,
            StateReason::SpawnFailed,
            NOW,
        )
        .await
        .expect("a starting service can fail");

        let note = record(&store, &id)
            .await
            .expect("the row")
            .last_failure
            .expect("a note");
        assert_eq!(note.detail, StateReason::SpawnFailed.to_string());
        assert_eq!(
            records(&store).await.expect("the rows")["pool"].last_failure,
            Some(note),
            "the listing reads the same note"
        );
    }

    /// **T200b, Review Focus 3.** A note this build cannot read is no note, not an error.
    #[tokio::test]
    async fn an_unreadable_failure_note_reads_as_none() {
        let (_home, store) = store().await;
        let id = service_row(&store, "pool", ServiceState::Failed).await;
        sqlx::query("UPDATE services SET last_failure_json = '{\"kind\":\"from a newer build\"}' WHERE id = ?")
            .bind(id.as_str())
            .execute(store.pool())
            .await
            .expect("the row");

        assert_eq!(
            record(&store, &id).await.expect("the row").last_failure,
            None
        );
    }

    /// Walk a service up and back down the way a supervisor does, arriving at `stopped` for `reason`.
    async fn down(store: &Store, id: &ServiceId, reason: StateReason) {
        for to in [
            ServiceState::Starting,
            ServiceState::Running,
            ServiceState::Stopping,
        ] {
            transition(store, id, to, StateReason::Requested, NOW)
                .await
                .expect("an edge the machine has");
        }

        transition(store, id, ServiceState::Stopped, reason, NOW)
            .await
            .expect("a stopping service can stop");
    }

    /// **A stop somebody asked for is theirs** — the design's D8, which T123 does not touch.
    ///
    /// `mix service stop mariadb@main` followed by the next connection starting it again is the tool
    /// overruling its user, and this is the word that forbids it.
    #[tokio::test]
    async fn a_stop_somebody_asked_for_is_recorded_as_theirs() {
        let (_home, store) = store().await;
        let id = service_row(&store, "caddy", ServiceState::Stopped).await;

        down(&store, &id, StateReason::Requested).await;

        assert_eq!(
            record(&store, &id).await.expect("the row").stopped_by,
            StoppedBy::Person,
        );
    }

    /// **A shutdown is not one of those** — roadmap task **T123**.
    ///
    /// `daemon.shutdown` and `service.stop` walk the same `Registry::stop`, so this row is the only
    /// place the difference can survive to the next boot — and on-demand activation is what reads it
    /// there. Recorded as a person's, every service on the machine came back from every restart
    /// unwakeable, and every site through one answered 502 until somebody started it by hand.
    #[tokio::test]
    async fn a_stop_the_daemon_made_on_its_way_out_is_not_recorded_as_a_person_s() {
        let (_home, store) = store().await;
        let id = service_row(&store, "caddy", ServiceState::Stopped).await;

        down(&store, &id, StateReason::Shutdown).await;

        assert_eq!(
            record(&store, &id).await.expect("the row").stopped_by,
            StoppedBy::Daemon,
        );
    }

    /// **Nor is a process that went away under the daemon** — roadmap task **T123**.
    ///
    /// What a restarted machine leaves: rows claiming to be running, reconciled to `stopped` by a
    /// daemon that finds no process behind them. Nobody decided this, so nothing about it may keep a
    /// connection from starting the service again.
    #[tokio::test]
    async fn a_process_that_vanished_is_nobody_s_stop() {
        let (_home, store) = store().await;
        let id = service_row(&store, "caddy", ServiceState::Stopped).await;

        down(&store, &id, StateReason::Vanished).await;

        assert_eq!(
            record(&store, &id).await.expect("the row").stopped_by,
            StoppedBy::Daemon,
        );
    }

    /// **And a service that is not stopped is nobody's stop at all** — roadmap task **T123**.
    ///
    /// The column is written on the way out of `stopped` as well as into it, so a row can never
    /// answer with a stop that is over. A reader that asks while the service is running gets the
    /// truth rather than a leftover.
    #[tokio::test]
    async fn leaving_stopped_takes_the_stop_with_it() {
        let (_home, store) = store().await;
        let id = service_row(&store, "caddy", ServiceState::Stopped).await;

        down(&store, &id, StateReason::Requested).await;
        transition(
            &store,
            &id,
            ServiceState::Starting,
            StateReason::Requested,
            NOW,
        )
        .await
        .expect("a stopped service can start");

        assert_eq!(
            record(&store, &id).await.expect("the row").stopped_by,
            StoppedBy::Never,
        );
    }

    /// **A row that has never run has never been stopped** — roadmap task **T123**.
    ///
    /// `service.create` writes `stopped`, because there is nowhere else for a service to begin. The
    /// column's default is what keeps that from reading as a decision somebody made.
    #[tokio::test]
    async fn a_service_that_has_never_run_was_stopped_by_nobody() {
        let (_home, store) = store().await;
        let id = service_row(&store, "caddy", ServiceState::Stopped).await;

        assert_eq!(
            record(&store, &id).await.expect("the row").stopped_by,
            StoppedBy::Never,
        );
    }

    #[tokio::test]
    async fn an_edge_the_machine_does_not_have_leaves_the_row_alone() {
        let (_home, store) = store().await;
        let id = service_row(&store, "caddy", ServiceState::Stopped).await;

        let error = transition(&store, &id, ServiceState::Running, StateReason::Ready, NOW)
            .await
            .expect_err("a stopped service cannot be running without starting");

        assert!(
            matches!(
                error,
                Error::IllegalTransition {
                    from: ServiceState::Stopped,
                    to: ServiceState::Running,
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(
            state(&store, &id).await.expect("the row"),
            ServiceState::Stopped,
            "the refused transition rolled back rather than half-applying"
        );
    }

    #[tokio::test]
    async fn a_service_that_is_not_there_is_named_as_such() {
        let (_home, store) = store().await;
        let id = ServiceId::parse("mariadb@main").expect("a valid id");

        let error = state(&store, &id).await.expect_err("no such row");

        assert!(
            matches!(&error, Error::NotFound { kind: "service", id } if id == "mariadb@main"),
            "{error:?}"
        );
    }

    /// The constraint added in `0001_initial.sql` and the enum have to agree, or one of them is
    /// decoration: every state the machine can be in must be storable, and nothing else may be.
    #[tokio::test]
    async fn the_column_accepts_every_state_and_nothing_else() {
        let (_home, store) = store().await;

        for state in ServiceState::ALL {
            service_row(&store, &format!("svc-{state}"), state).await;
        }

        let refused = sqlx::query(
            "INSERT INTO services (id, package_id, instance_name, state)
             VALUES ('bogus', (SELECT id FROM packages LIMIT 1), 'main', 'crashed')",
        )
        .execute(store.pool())
        .await;

        assert!(
            refused.is_err(),
            "the CHECK let a word through that ServiceState cannot read back"
        );
    }

    /// The pair, and the clearing of it, are one round trip: what T18 adopts is what T19 wrote.
    #[tokio::test]
    async fn the_process_a_service_is_running_as_is_written_and_then_cleared() {
        let (_home, store) = store().await;
        let id = service_row(&store, "caddy", ServiceState::Starting).await;

        started(&store, &id, 4321, Some(1_234_567), NOW)
            .await
            .expect("the row takes a pid");

        let (pid, start_time, at): (Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT pid, pid_start_time, last_started_at FROM services WHERE id = ?",
        )
        .bind(id.as_str())
        .fetch_one(store.pool())
        .await
        .expect("the row");

        assert_eq!(pid, Some(4321));
        assert_eq!(at, Some(NOW.0), "epoch milliseconds, not text");
        assert_eq!(
            start_time,
            Some(1_234_567),
            "the reading is stored as it was given, since only the OS that made it can read it"
        );

        ended(&store, &id, Some(3)).await.expect("the row lets go");

        let (pid, start_time, code): (Option<i64>, Option<i64>, Option<i64>) =
            sqlx::query_as("SELECT pid, pid_start_time, last_exit_code FROM services WHERE id = ?")
                .bind(id.as_str())
                .fetch_one(store.pool())
                .await
                .expect("the row");

        assert_eq!(
            pid, None,
            "a row that kept a dead pid is a row the next daemon would adopt"
        );
        assert_eq!(
            start_time, None,
            "and half an identity is one the next daemon would have to guess at"
        );
        assert_eq!(code, Some(3));
    }

    #[tokio::test]
    async fn recording_a_process_against_a_service_that_is_not_there_names_it() {
        let (_home, store) = store().await;
        let id = ServiceId::parse("caddy").expect("a valid id");

        let error = started(&store, &id, 1, None, NOW)
            .await
            .expect_err("no such row");

        assert!(
            matches!(&error, Error::NotFound { kind: "service", id } if id == "caddy"),
            "{error:?}"
        );
    }

    /// What a hand-edited database looks like from in here.
    ///
    /// Asked of the function rather than through a doctored database on purpose. The `CHECK` above
    /// makes the row unreachable through any write of ours, so producing one would mean disabling
    /// the constraint with `PRAGMA writable_schema` — which is per *connection*, and this store
    /// hands out four of them. That test would be about SQLite's schema cache; this one is about
    /// what the reader does when the word does not parse, which is the part we wrote.
    #[test]
    fn a_state_this_build_does_not_know_blames_the_row() {
        let id = ServiceId::parse("caddy").expect("a valid id");

        let error = parse_state(&id, "crashed".to_owned()).expect_err("not a service state");

        assert!(
            matches!(&error, Error::UnknownServiceState { service, value }
                if service == "caddy" && value == "crashed"),
            "{error:?}"
        );
    }
    /// **T97.** A row reads back as the value that wrote it, which is what makes a switch's
    /// rollback a second [`create`] rather than a reconstruction from a summary.
    #[tokio::test]
    async fn a_row_reads_back_as_the_declaration_that_wrote_it() {
        let (_home, store) = store().await;

        sqlx::query(
            "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES ('mariadb', '11.4.2', '/packages/mariadb/11.4.2', '2026-09-07T00:00:00Z',
                     'https://example.invalid/mariadb', 'ab')",
        )
        .execute(store.pool())
        .await
        .expect("a package for the service to belong to");

        let service = ServiceId::parse("mariadb@legacy").expect("a valid id");
        let written = Declaration {
            service: service.clone(),
            origin: Origin::Package {
                name: "mariadb".to_owned(),
                version: PackageVersion::parse("11.4.2").expect("a version"),
            },
            instance_name: "legacy".to_owned(),
            port: Port::Fixed(3307),
            bind_addr: Some("0.0.0.0".to_owned()),
            data_dir: Some("/somewhere/else".to_owned()),
            autostart: true,
            overrides: r#"{"innodb_buffer_pool_size":"512M"}"#.to_owned(),
        };

        create(
            &store,
            &mixengine_platform::mock::Host::with_home("/mixengine"),
            &written,
        )
        .await
        .expect("a service");

        let read = super::declaration(&store, &service)
            .await
            .expect("the row back");

        assert!(
            matches!(
                &read.origin,
                Origin::Package { name, version }
                    if name == "mariadb" && version.as_str() == "11.4.2"
            ),
            "{:?}",
            read.origin
        );
        assert_eq!(read.service, service);
        assert_eq!(read.instance_name, "legacy");
        assert_eq!(
            read.port,
            Port::Fixed(3307),
            "a restore takes the number back as itself and does not go looking for one again"
        );
        assert_eq!(read.bind_addr.as_deref(), Some("0.0.0.0"));
        assert_eq!(read.data_dir.as_deref(), Some("/somewhere/else"));
        assert!(read.autostart);
        assert_eq!(read.overrides, written.overrides);
    }

    /// A service with no port has none coming back, and not one somebody would have to allocate.
    #[tokio::test]
    async fn a_row_with_no_port_reads_back_as_a_service_that_has_none() {
        let (_home, store) = store().await;
        let service = service_row(&store, "caddy", ServiceState::Stopped).await;

        let read = super::declaration(&store, &service)
            .await
            .expect("the row back");

        assert_eq!(read.port, Port::None);
        assert!(!read.autostart);
        assert_eq!(
            read.bind_addr.as_deref(),
            Some("127.0.0.1"),
            "the column's own default, read back as the value it holds"
        );
    }

    #[tokio::test]
    async fn a_declaration_of_a_service_that_is_not_there_is_named_as_such() {
        let (_home, store) = store().await;
        let id = ServiceId::parse("nginx").expect("a valid id");

        let error = super::declaration(&store, &id)
            .await
            .expect_err("no such row");

        assert!(
            matches!(&error, Error::NotFound { kind: "service", id } if id == "nginx"),
            "{error:?}"
        );
    }

    /// A row whose binary comes from an installed runtime rather than from a package.
    ///
    /// The whole of T32's schema change seen from the only place that writes it: `create` resolves
    /// `runtime_installs` instead of `packages`, and the row that lands names one parent.
    #[tokio::test]
    async fn a_service_can_come_from_a_runtime_install() {
        let (_home, store) = store().await;

        sqlx::query(
            "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url, sha256)
             VALUES ('php', '8.3.33', 'stable', '/runtimes/php/8.3.33', '2026-08-19T00:00:00Z',
                     1, 'https://example.invalid/php', 'abc')",
        )
        .execute(store.pool())
        .await
        .expect("a runtime install");

        let service = ServiceId::parse("php-fpm@8.3.33").expect("a valid id");

        create(
            &store,
            &mixengine_platform::mock::Host::with_home("/mixengine"),
            &Declaration {
                service,
                origin: Origin::Runtime {
                    kind: RuntimeKind::Php,
                    version: PackageVersion::parse("8.3.33").expect("a version"),
                },
                instance_name: "8.3.33".to_owned(),
                port: Port::None,
                bind_addr: None,
                data_dir: None,
                autostart: false,
                overrides: "{}".to_owned(),
            },
        )
        .await
        .expect("a pool for an installed PHP");

        let (package_id, runtime_install_id): (Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT package_id, runtime_install_id FROM services WHERE id = 'php-fpm@8.3.33'",
        )
        .fetch_one(store.pool())
        .await
        .expect("the row that was written");

        assert_eq!(package_id, None, "a pool has no package to point at");
        assert!(
            runtime_install_id.is_some(),
            "the row points at the PHP it runs out of"
        );
    }

    /// A recipe's preferred port is a wish; what lands in the row is what the machine allowed.
    ///
    /// The two halves have to agree or the service is configured for a port it was never given:
    /// what [`create`] answers is what a client prints, and what the column holds is what the
    /// template renders from.
    #[tokio::test]
    async fn an_allocated_port_is_the_one_that_lands_in_the_row() {
        let (_home, store) = store().await;
        package(&store, "mariadb").await;

        let squatter =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("a squatter");
        let preferred = squatter.local_addr().expect("its address").port();

        let written = create(
            &store,
            &mixengine_platform::mock::Host::with_home("/mixengine"),
            &declaration("mariadb@main", "mariadb", Port::Allocate { preferred }),
        )
        .await
        .expect("a service");

        assert!(
            written.port > Some(preferred),
            "the preferred port was held by this test's own listener"
        );
        assert!(
            written.moved_from.is_some(),
            "a service that did not get 3306 has to say so"
        );

        let column: Option<i64> =
            sqlx::query_scalar("SELECT port FROM services WHERE id = 'mariadb@main'")
                .fetch_one(store.pool())
                .await
                .expect("the row that was written");

        assert_eq!(column, written.port.map(i64::from));
    }

    /// Somebody who names a port has already decided, and is not moved off it.
    ///
    /// The allocation exists because a recipe's 3306 is a *wish*. `--port 3307` is not one, and a
    /// daemon that answered it with 3308 because something was listening would be overruling the
    /// one instruction in the call — the start that then fails says who holds it (T38), which is
    /// the honest place for that news.
    #[tokio::test]
    async fn a_port_the_caller_named_is_written_down_even_when_it_is_busy() {
        let (_home, store) = store().await;
        package(&store, "mariadb").await;

        let squatter =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("a squatter");
        let named = squatter.local_addr().expect("its address").port();

        let written = create(
            &store,
            &mixengine_platform::mock::Host::with_home("/mixengine"),
            &declaration("mariadb@main", "mariadb", Port::Fixed(named)),
        )
        .await
        .expect("a service");

        assert_eq!(written.port, Some(named));
        assert_eq!(
            written.moved_from, None,
            "nothing was moved, so there is nothing to explain"
        );
    }

    /// Two creates racing for one preferred port come out with two ports.
    ///
    /// **This is what the critical section is for**, and it is the failure that would otherwise
    /// reach a user as a second database that starts, binds nothing and dies: both calls read a
    /// table neither has written to, both bind-test a port neither has taken, and both write down
    /// the same number.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_creates_racing_for_one_port_are_given_two() {
        let (_home, store) = store().await;
        package(&store, "mariadb").await;
        package(&store, "mysql").await;

        let host = mixengine_platform::mock::Host::with_home("/mixengine");
        let preferred = {
            let probe =
                std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("a probe");
            probe.local_addr().expect("its address").port()
        };

        let one = declaration("mariadb@main", "mariadb", Port::Allocate { preferred });
        let other = declaration("mysql@main", "mysql", Port::Allocate { preferred });

        let (first, second) =
            tokio::join!(create(&store, &host, &one), create(&store, &host, &other),);

        let first = first.expect("a service").port.expect("a port");
        let second = second.expect("a service").port.expect("a port");

        assert_ne!(
            first, second,
            "both services were told to listen on {first}"
        );
    }

    /// Two services pointed at one data directory, which is the conflict a port conflict is not.
    ///
    /// A port two services share is a start that fails and says who holds it (T38). A *directory*
    /// two database servers share is two `mysqld`s over one set of InnoDB files, and what it costs
    /// is the data rather than the start — so it is refused where it is written down rather than
    /// discovered where it is opened. Only an explicit `data_dir` can reach this: the layout the
    /// generator derives is `data/<package>/<instance>` and two instances cannot collide in it.
    #[tokio::test]
    async fn a_data_directory_another_service_holds_is_refused() {
        let (home, store) = store().await;
        package(&store, "mariadb").await;
        package(&store, "mysql").await;

        let host = mixengine_platform::mock::Host::with_home("/mixengine");
        let shared = home.path().join("shared").display().to_string();

        create(
            &store,
            &host,
            &Declaration {
                data_dir: Some(shared.clone()),
                ..declaration("mariadb@main", "mariadb", Port::None)
            },
        )
        .await
        .expect("the first service to claim the directory");

        let error = create(
            &store,
            &host,
            &Declaration {
                data_dir: Some(shared.clone()),
                ..declaration("mysql@main", "mysql", Port::None)
            },
        )
        .await
        .expect_err("two servers over one data directory");

        let said = error.to_string();

        assert!(
            said.contains(&shared) && said.contains("mariadb@main"),
            "the refusal names the directory and whoever already holds it, and said: {said}"
        );
    }

    /// A relative path and the absolute one it means are one directory.
    ///
    /// `--data-dir db` resolves against the shell it was typed in, and the row beside it may hold
    /// the same directory written out in full — from the GUI, or from an earlier create in another
    /// terminal. The strings share nothing; the servers would share every file. A `.` segment needs
    /// no help here, because [`PathBuf`](std::path::PathBuf) compares components and drops those on
    /// its own — what this asserts is the part that has to be asked for.
    #[tokio::test]
    async fn a_relative_data_directory_is_the_absolute_one_it_resolves_to() {
        let (_home, store) = store().await;
        package(&store, "mariadb").await;
        package(&store, "mysql").await;

        let host = mixengine_platform::mock::Host::with_home("/mixengine");
        let relative = Path::new("target").join("t36-shared-data");
        let absolute = std::env::current_dir()
            .expect("a working directory")
            .join(&relative);

        create(
            &store,
            &host,
            &Declaration {
                data_dir: Some(absolute.display().to_string()),
                ..declaration("mariadb@main", "mariadb", Port::None)
            },
        )
        .await
        .expect("the first service to claim the directory");

        let error = create(
            &store,
            &host,
            &Declaration {
                data_dir: Some(relative.display().to_string()),
                ..declaration("mysql@main", "mysql", Port::None)
            },
        )
        .await
        .expect_err("the same directory, named the short way");

        assert!(
            matches!(error, Error::DataDirectoryTaken { .. }),
            "refused for the directory rather than for something else: {error}"
        );
    }

    /// **T183.** Each of the three parents answers for its own version, and a listing and a single
    /// read agree.
    #[tokio::test]
    async fn a_record_carries_the_version_of_the_program_it_runs() {
        let (_home, store) = store().await;

        // A server, out of a `packages` row — `service_row` installs its package at 1.0.0.
        let server = service_row(&store, "fakeservice", ServiceState::Stopped).await;

        // php-fpm, out of a `runtime_installs` row.
        sqlx::query(
            "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url, sha256)
             VALUES ('php', '8.3.33', 'stable', '/runtimes/php/8.3.33', '2026-09-24T00:00:00Z',
                     1, 'https://example.invalid/php', 'abc')",
        )
        .execute(store.pool())
        .await
        .expect("a runtime install");
        sqlx::query(
            "INSERT INTO services (id, runtime_install_id, instance_name, state)
             VALUES ('php-fpm@8.3.33', (SELECT id FROM runtime_installs LIMIT 1), '8.3.33',
                     'stopped')",
        )
        .execute(store.pool())
        .await
        .expect("a php-fpm row");

        // An extension's service: its version is the extension's, not a program's.
        sqlx::query(
            "INSERT INTO extensions (id, name, version, kind, manifest_json, install_dir, data_dir,
                                     source, signed, installed_at)
             VALUES ('mailpit', 'Mailpit', '0.9.0', 'service', '{}', '/ext/mailpit',
                     '/data/extensions/mailpit', 'path', 0, '2026-09-24T00:00:00Z')",
        )
        .execute(store.pool())
        .await
        .expect("an extension row");
        sqlx::query(
            "INSERT INTO services (id, extension_id, instance_name, state)
             VALUES ('mailpit', 'mailpit', 'mailpit', 'stopped')",
        )
        .execute(store.pool())
        .await
        .expect("an extension's service row");

        let listed = records(&store).await.expect("the rows");
        let version_of = |id: &str| listed[id].version.as_ref().map(PackageVersion::as_str);

        assert_eq!(version_of("fakeservice"), Some("1.0.0"));
        assert_eq!(version_of("php-fpm@8.3.33"), Some("8.3.33"));
        assert_eq!(
            version_of("mailpit"),
            None,
            "an extension's version is not the program's"
        );

        assert_eq!(
            record(&store, &server).await.expect("the row").version,
            listed["fakeservice"].version,
            "the single read and the listing must not drift"
        );
    }

    /// A version nobody can parse is a listing with one blank cell, not a listing that fails.
    #[tokio::test]
    async fn a_version_that_does_not_parse_is_reported_as_none() {
        let (_home, store) = store().await;
        let service = service_row(&store, "fakeservice", ServiceState::Stopped).await;
        sqlx::query("UPDATE packages SET version = 'not a version' WHERE name = 'fakeservice'")
            .execute(store.pool())
            .await
            .expect("a hand-edited row");

        assert_eq!(
            record(&store, &service).await.expect("the row").version,
            None
        );
        assert_eq!(
            records(&store).await.expect("the rows")["fakeservice"].version,
            None
        );
    }

    /// The `CHECK` is the whole guarantee that [`Origin`] is not a suggestion.
    ///
    /// Written through raw SQL rather than through [`create`], because `create` cannot express
    /// either of these — which is the point: what is being asserted is that a hand-edited database,
    /// or a future writer nobody has written yet, cannot express them either.
    #[tokio::test]
    async fn a_row_names_one_parent_and_not_two_and_not_none() {
        let (_home, store) = store().await;

        sqlx::query(
            "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES ('caddy', '2.11.4', '/packages/caddy', '2026-08-19T00:00:00Z',
                     'https://example.invalid/caddy', 'ab')",
        )
        .execute(store.pool())
        .await
        .expect("a package");

        sqlx::query(
            "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url, sha256)
             VALUES ('php', '8.3.33', 'stable', '/runtimes/php/8.3.33', '2026-08-19T00:00:00Z',
                     1, 'https://example.invalid/php', 'abc')",
        )
        .execute(store.pool())
        .await
        .expect("a runtime install");

        let both = sqlx::query(
            "INSERT INTO services (id, package_id, runtime_install_id, instance_name, state)
             VALUES ('both', (SELECT id FROM packages LIMIT 1),
                     (SELECT id FROM runtime_installs LIMIT 1), 'both', 'stopped')",
        )
        .execute(store.pool())
        .await;
        assert!(both.is_err(), "a service with two parents was accepted");

        let neither = sqlx::query(
            "INSERT INTO services (id, instance_name, state) VALUES ('orphan', 'orphan', 'stopped')",
        )
        .execute(store.pool())
        .await;
        assert!(neither.is_err(), "a service with no parent was accepted");
    }

    /// **A service stopped for a credential reset may not be woken by a connection** — roadmap task
    /// **T127**.
    ///
    /// **Asserted through [`StoppedBy::may_be_woken`] and not through the reason word**, because
    /// [`StoppedBy::of`] reaches [`StoppedBy::Daemon`] through a catch-all arm: a reason that is not
    /// named there becomes wakeable, and nothing fails to compile over it. What that costs is a
    /// server started against a data directory a reset is part-way through writing — the one thing
    /// the repair holds the service down for.
    #[test]
    fn a_stop_for_a_credential_reset_may_not_be_woken() {
        let stopped_by = StoppedBy::of(&StateReason::CredentialReset);

        assert_eq!(stopped_by, StoppedBy::Person);
        assert!(!stopped_by.may_be_woken());
    }

    /// And the daemon's own stops are still wakeable, which is the half the arm above must not break.
    ///
    /// **[`StateReason::SuperuserRefused`] is in here although it never reaches this function** —
    /// roadmap task **T127a**. It is only ever carried into [`ServiceState::Failed`] and this is
    /// consulted on the way into [`ServiceState::Stopped`], so the catch-all arm is what would
    /// answer for it. This says which answer that has to be if the word ever does arrive: a database
    /// that refused its superuser is not a stop anybody asked for, and a connection may still try.
    #[test]
    fn the_daemons_own_stops_are_still_wakeable() {
        for reason in [
            StateReason::Shutdown,
            StateReason::Idle {
                after: mixengine_proto::Millis(600_000),
            },
            StateReason::SuperuserRefused {
                said: "FATAL:  password authentication failed for user \"postgres\"".to_owned(),
            },
        ] {
            let stopped_by = StoppedBy::of(&reason);

            assert_eq!(stopped_by, StoppedBy::Daemon, "{reason:?}");
            assert!(stopped_by.may_be_woken(), "{reason:?}");
        }
    }
}
