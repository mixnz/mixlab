//! Connecting, disconnecting, and everything the per-engine command files are built on.
//!
//! The helpers below stay private on purpose: a child module sees its parent's private items, so
//! `commands/mysql.rs` reaches them through `use super::...` while nothing outside `commands` can.
//!
//! The drivers are reached through `drivers::` here rather than imported by name, because
//! `pub mod mysql;` below and a `use ...drivers::mysql;` would be the same name in this one module.

use crate::error::AppError;
use crate::platform::{app_data_dir, in_background};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::modules::db::drivers;
use crate::modules::db::models::{ConnectionConfig, DbKind};
use crate::modules::db::state::{ActiveConnection, Cancel, DbHandle, DbState};
use crate::ssh::{self, SshConfig};

/// Runs a **read** command again exactly once, if the first attempt died along with the connection.
///
/// Reads only. An `INSERT` run again after losing the connection may become two rows — the
/// statement may have reached the server and only the answer was lost — so write commands report
/// the error and leave the decision to the user.
///
/// A macro rather than a function taking a closure: an `Fn() -> impl Future` borrowing
/// `State<'_, DbState>` drags the call into exactly the kind of lifetime trouble not worth
/// fighting, while a macro just writes the command body twice.
///
/// Called as `retry_read!({ ... })` — parentheses around the block, because a macro call opening
/// with a brace in statement position is parsed as a *statement macro*, not an expression with a
/// value.
macro_rules! retry_read {
    ($body:block) => {{
        match async { $body }.await {
            // No sleep between the two attempts: the second one will wait by itself in the pool's
            // `acquire`, behind the session reopening that is under way.
            Err(e) if e.code == "error.connectionLost" => async { $body }.await,
            first => first,
        }
    }};
}

pub mod clickhouse;
pub mod handoff;
pub mod mongo;
pub mod mssql;
pub mod mysql;
pub mod postgres;
pub mod redis;
pub mod sqlite;
pub mod tools;

const DB_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the settings screen listens for how far a tool download has got. Named here and in
/// `src/tools.ts`, which are the only two places that need to agree on it.
const TOOLS_PROGRESS_EVENT: &str = "tools://progress";

/// Where the workspace listens for how far a dump or a restore has got. Named here and in
/// `src/transfer.ts`.
const TRANSFER_PROGRESS_EVENT: &str = "transfer://progress";

/// Where a workspace listens for its SSH tunnel dropping and coming back. Named here and in
/// `src/modules/db/tunnel.ts`.
const TUNNEL_STATE_EVENT: &str = "tunnel://state";

/// One reading of a running transfer, with the connection it belongs to: two tabs can be dumping at
/// once, and neither overlay has any business showing the other's figures.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferProgress {
    id: String,
    #[serde(flatten)]
    progress: drivers::dump::Progress,
}

/// Hands every reading of one connection's transfer to the window, on [`TRANSFER_PROGRESS_EVENT`].
fn reporter(app: &AppHandle, id: &str) -> impl Fn(drivers::dump::Progress) {
    let app = app.clone();
    let id = id.to_string();
    move |progress| {
        // A dropped reading is not worth failing a transfer over: the next one is a quarter of a
        // second away, and the last word comes from the command's own result.
        let _ = app.emit(
            TRANSFER_PROGRESS_EVENT,
            TransferProgress {
                id: id.clone(),
                progress,
            },
        );
    }
}

/// A piece of news about a connection's tunnel, sent up to the window.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TunnelState {
    id: String,
    /// `"reconnecting"`, `"reconnected"` or `"failed"` — the same set of words as `TunnelState` on
    /// the TypeScript side.
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<AppError>,
}

/// Hands every turn of one connection's tunnel to the window, on [`TUNNEL_STATE_EVENT`].
fn tunnel_notify(app: &AppHandle, id: &str) -> ssh::TunnelNotify {
    let app = app.clone();
    let id = id.to_string();
    Arc::new(move |event: ssh::TunnelEvent| {
        let (state, error) = match event {
            ssh::TunnelEvent::Reconnecting => ("reconnecting", None),
            ssh::TunnelEvent::Reconnected => ("reconnected", None),
            ssh::TunnelEvent::Failed(e) => ("failed", Some(e)),
        };
        // A dropped notice is not worth failing anything over: the watcher says it again on its
        // next round.
        let _ = app.emit(
            TUNNEL_STATE_EVENT,
            TunnelState {
                id: id.clone(),
                state,
                error,
            },
        );
    })
}

async fn with_timeout<T>(
    fut: impl std::future::Future<Output = Result<T, AppError>>,
    kind: &'static str,
) -> Result<T, AppError> {
    match tokio::time::timeout(DB_CONNECT_TIMEOUT, fut).await {
        Ok(result) => result,
        Err(_) => Err(err!(
            "error.connectTimeout",
            kind = kind,
            seconds = DB_CONNECT_TIMEOUT.as_secs()
        )),
    }
}

#[tauri::command]
pub async fn test_ssh_tunnel(app: AppHandle, ssh: SshConfig) -> Result<(), AppError> {
    ssh::test_connection(&ssh, &app_data_dir(&app)?).await
}

/// The address to dial and, when the connection goes through SSH, the tunnel that has to stay
/// alive for it to keep working. A caller that gives up before storing the tunnel drops it, which
/// closes the forward again rather than leaving it running unattended.
async fn resolve_endpoint(
    config: &ConnectionConfig,
    app_data: &std::path::Path,
    notify: ssh::TunnelNotify,
) -> Result<(String, u16, Option<ssh::Tunnel>), AppError> {
    match &config.ssh {
        Some(ssh) => {
            let (local_port, tunnel) =
                ssh::open_tunnel(ssh, &config.host, config.port, app_data, notify).await?;
            Ok(("127.0.0.1".to_string(), local_port, Some(tunnel)))
        }
        None => Ok((config.host.clone(), config.port, None)),
    }
}

#[tauri::command]
pub async fn connect_db(
    app: AppHandle,
    state: State<'_, DbState>,
    config: ConnectionConfig,
) -> Result<String, AppError> {
    let app_data = app_data_dir(&app)?;
    // The id is generated here rather than after connecting: the reporting closure needs to know
    // its name, and the tunnel starts reporting as soon as it is opened.
    let id = Uuid::new_v4().to_string();
    let notify = tunnel_notify(&app, &id);
    let (handle, endpoint, tunnel) = match config.kind {
        DbKind::Mysql => {
            let (host, port, tunnel) =
                resolve_endpoint(&config, &app_data, Arc::clone(&notify)).await?;
            let username = config.username.clone().unwrap_or_default();
            let password = config.password.clone().unwrap_or_default();
            let pool = with_timeout(
                drivers::mysql::connect(
                    &host,
                    port,
                    &username,
                    &password,
                    config.database.as_deref(),
                    config.use_ssl,
                ),
                "MySQL",
            )
            .await?;
            let mariadb = drivers::mysql::detect_mariadb(&pool).await;
            (
                DbHandle::Mysql { pool, mariadb },
                Some((host, port)),
                tunnel,
            )
        }
        DbKind::Postgres => {
            let (host, port, tunnel) =
                resolve_endpoint(&config, &app_data, Arc::clone(&notify)).await?;
            let username = config.username.clone().unwrap_or_default();
            let password = config.password.clone().unwrap_or_default();
            let pools = with_timeout(
                drivers::postgres::connect(
                    &host,
                    port,
                    &username,
                    &password,
                    config.database.as_deref(),
                    config.use_ssl,
                ),
                "PostgreSQL",
            )
            .await?;
            (
                DbHandle::Postgres(Arc::new(pools)),
                Some((host, port)),
                tunnel,
            )
        }
        // MongoDB is configured as a whole connection string rather than host/port/user/password,
        // so the endpoint lives inside the URI. Tunneling therefore has to read the host back out
        // of it and then override it, instead of going through `resolve_endpoint`.
        DbKind::Mongo => {
            let uri = config.uri.as_deref().unwrap_or_default().trim();
            if uri.is_empty() {
                return Err(err!("error.mongoUriRequired"));
            }
            let (endpoint, tunnel) = match &config.ssh {
                Some(ssh) => {
                    let (host, port) = drivers::mongo::first_endpoint(uri).await?;
                    let (local_port, task) =
                        ssh::open_tunnel(ssh, &host, port, &app_data, Arc::clone(&notify)).await?;
                    (Some(("127.0.0.1".to_string(), local_port)), Some(task))
                }
                None => (None, None),
            };
            let client =
                with_timeout(drivers::mongo::connect(uri, endpoint.clone()), "MongoDB").await?;
            (DbHandle::Mongo(client), endpoint, tunnel)
        }
        // A file, not a server: no endpoint to resolve, so no tunnel and no TLS. `endpoint` stays
        // `None`, which is also what tells the dump path there is no address to hand a tool — see
        // `sql_endpoint`.
        DbKind::Sqlite => {
            let path = config.path.as_deref().unwrap_or_default();
            let pool = with_timeout(drivers::sqlite::connect(path), "SQLite").await?;
            (DbHandle::Sqlite(pool), None, None)
        }
        DbKind::Redis => {
            let (host, port, tunnel) =
                resolve_endpoint(&config, &app_data, Arc::clone(&notify)).await?;
            let db_index = config
                .database
                .as_deref()
                .and_then(|d| d.parse().ok())
                .unwrap_or(0);
            let conn = with_timeout(
                drivers::redis::connect(
                    &host,
                    port,
                    config.username.as_deref(),
                    config.password.as_deref(),
                    db_index,
                ),
                "Redis",
            )
            .await?;
            (
                DbHandle::Redis(Arc::new(Mutex::new(conn))),
                Some((host, port)),
                tunnel,
            )
        }
        DbKind::Clickhouse => {
            let (host, port, tunnel) =
                resolve_endpoint(&config, &app_data, Arc::clone(&notify)).await?;
            let username = config.username.clone().unwrap_or_default();
            let password = config.password.clone().unwrap_or_default();
            let conn = with_timeout(
                drivers::clickhouse::connect(&host, port, &username, &password, config.use_ssl),
                "ClickHouse",
            )
            .await?;
            (DbHandle::Clickhouse(conn), Some((host, port)), tunnel)
        }
        DbKind::Mssql => {
            let (host, port, tunnel) =
                resolve_endpoint(&config, &app_data, Arc::clone(&notify)).await?;
            let username = config.username.clone().unwrap_or_default();
            let password = config.password.clone().unwrap_or_default();
            let pool = with_timeout(
                drivers::mssql::connect(
                    &host,
                    port,
                    &username,
                    &password,
                    config.database.as_deref(),
                    config.use_ssl,
                ),
                "SQL Server",
            )
            .await?;
            (DbHandle::Mssql(pool), Some((host, port)), tunnel)
        }
    };

    state.connections.lock().await.insert(
        id.clone(),
        ActiveConnection {
            handle,
            config,
            endpoint,
            tunnel,
        },
    );
    Ok(id)
}

/// Closes a connection, and says goodbye before shutting the door.
///
/// Dropping the pool loses the socket too, but on the server side that is a connection vanishing
/// midway, and it writes a log line for each one. `close()` sends the goodbye and only then waits.
///
/// The order is deliberate: take it out of the map first (the lock is held across no `await`),
/// close the pool while the tunnel is **still alive** inside what was just taken out — the road
/// back to the server must still be there for the goodbye to arrive — and only then let go, and
/// `Tunnel`'s `Drop` takes the forwarded port down.
#[tauri::command]
pub async fn disconnect_db(state: State<'_, DbState>, id: String) -> Result<(), AppError> {
    /* First, and whether or not the connection is still in the map: a transfer runs an external
    tool that outlives everything here, so a tab closed mid-dump used to leave `mysqldump`
    writing a file nobody was waiting for. */
    cancel_transfer_in(&state, &id);
    let gone = state.connections.lock().await.remove(&id);
    let Some(connection) = gone else {
        return Ok(());
    };
    match &connection.handle {
        DbHandle::Mysql { pool, .. } => pool.close().await,
        DbHandle::Postgres(pools) => pools.close_all().await,
        DbHandle::Sqlite(pool) => pool.close().await,
        // Mongo cleans up by itself when the last handle goes, and Redis is a connection rather
        // than a pool. ClickHouse holds no socket at all — each command is a separate HTTP request,
        // so there is nothing to close here.
        DbHandle::Mongo(_) | DbHandle::Redis(_) | DbHandle::Clickhouse(_) | DbHandle::Mssql(_) => {}
    }
    drop(connection);
    Ok(())
}

/// Reopens a connection's SSH session right away, instead of waiting out the watcher's backoff.
/// This is what the banner's *Retry* button calls.
#[tauri::command]
pub async fn tunnel_reconnect(state: State<'_, DbState>, id: String) -> Result<(), AppError> {
    // The handle is copied out and the map unlocked **before** waiting: authentication takes up to
    // `CONNECT_TIMEOUT` (10 seconds), and holding the map that long would block every other command
    // in the app.
    let session = {
        let connections = state.connections.lock().await;
        let connection = connections
            .get(&id)
            .ok_or_else(|| err!("error.unknownConnection"))?;
        connection
            .tunnel
            .as_ref()
            .map(|tunnel| tunnel.session_handle())
            .ok_or_else(|| err!("error.noTunnel"))?
    };
    session.reconnect().await
}

/// The handle `id` names, cloned out of the connection map so that the map is unlocked again
/// before the command runs anything on it.
///
/// This is what `DbHandle` is cheap to clone for. Awaiting a query while holding the map would
/// turn that lock into a queue for the whole app: a slow `SELECT` in one tab would stop a key
/// listing in another, and stop the Disconnect button meant to put an end to it.
async fn handle(state: &State<'_, DbState>, id: &str) -> Result<DbHandle, AppError> {
    state
        .connections
        .lock()
        .await
        .get(id)
        .map(|connection| connection.handle.clone())
        .ok_or_else(|| err!("error.unknownConnection"))
}

async fn mysql_pool(state: &State<'_, DbState>, id: &str) -> Result<sqlx::MySqlPool, AppError> {
    mysql_connection(state, id).await.map(|(pool, _)| pool)
}

/// The MySQL pool for `id`, and whether the server it reaches is MariaDB — for the handful of
/// reads whose answer depends on which of the two answered.
async fn mysql_connection(
    state: &State<'_, DbState>,
    id: &str,
) -> Result<(sqlx::MySqlPool, bool), AppError> {
    match handle(state, id).await? {
        DbHandle::Mysql { pool, mariadb } => Ok((pool, mariadb)),
        _ => Err(err!("error.wrongConnectionKind", kind = "MySQL")),
    }
}

/// The pool for one database of a PostgreSQL connection, opening it if this is the first time that
/// database has been asked for. `database` empty means the one the connection was opened on.
///
/// Every PostgreSQL command goes through this rather than through a pool of its own, because that
/// is the whole difference from MySQL: there is no `USE`, so which database a command runs against
/// is decided by which pool it is handed.
async fn postgres_pool(
    state: &State<'_, DbState>,
    id: &str,
    database: &str,
) -> Result<sqlx::PgPool, AppError> {
    match handle(state, id).await? {
        DbHandle::Postgres(pools) => pools.pool(Some(database)).await,
        _ => Err(err!("error.wrongConnectionKind", kind = "PostgreSQL")),
    }
}

/// The whole PostgreSQL connection rather than one of its pools — for `DROP DATABASE`, which has
/// to close the pool on the database it is dropping before the server will allow it.
async fn postgres_pools(
    state: &State<'_, DbState>,
    id: &str,
) -> Result<Arc<drivers::postgres::Pools>, AppError> {
    match handle(state, id).await? {
        DbHandle::Postgres(pools) => Ok(pools),
        _ => Err(err!("error.wrongConnectionKind", kind = "PostgreSQL")),
    }
}

/// The pool for the one database file `id` names. There is no per-database pool to choose between
/// the way PostgreSQL has: a SQLite connection is the file, and the file is one database.
async fn sqlite_pool(state: &State<'_, DbState>, id: &str) -> Result<sqlx::SqlitePool, AppError> {
    match handle(state, id).await? {
        DbHandle::Sqlite(pool) => Ok(pool),
        _ => Err(err!("error.wrongConnectionKind", kind = "SQLite")),
    }
}

async fn mongo_client(state: &State<'_, DbState>, id: &str) -> Result<mongodb::Client, AppError> {
    match handle(state, id).await? {
        DbHandle::Mongo(client) => Ok(client),
        _ => Err(err!("error.wrongConnectionKind", kind = "MongoDB")),
    }
}

/// The pool for `id`. One for the whole server, so unlike `postgres_pool` there is no database to
/// choose between — every command writes the database it wants into its own SQL instead.
async fn mssql_pool(
    state: &State<'_, DbState>,
    id: &str,
) -> Result<drivers::mssql::Pool, AppError> {
    match handle(state, id).await? {
        DbHandle::Mssql(pool) => Ok(pool),
        _ => Err(err!("error.wrongConnectionKind", kind = "SQL Server")),
    }
}

async fn clickhouse_connection(
    state: &State<'_, DbState>,
    id: &str,
) -> Result<drivers::clickhouse::Connection, AppError> {
    match handle(state, id).await? {
        DbHandle::Clickhouse(conn) => Ok(conn),
        _ => Err(err!("error.wrongConnectionKind", kind = "ClickHouse")),
    }
}

/// The Redis connection `id` names. Locked by the caller for the length of one command, which is
/// as long as anything needs it: the lock is this connection's own, so two tabs no longer wait on
/// each other.
async fn redis_connection(
    state: &State<'_, DbState>,
    id: &str,
) -> Result<Arc<Mutex<drivers::redis::Connection>>, AppError> {
    match handle(state, id).await? {
        DbHandle::Redis(conn) => Ok(conn),
        _ => Err(err!("error.wrongConnectionKind", kind = "Redis")),
    }
}

/// What a dump or restore needs to dial the server itself: the address actually in use (the
/// tunnel's local end, when there is one) and the credentials that opened the connection.
///
/// Read out of the connection and returned by value on purpose — the tools run for as long as the
/// database is big, and holding the connection lock across that would stop every other command in
/// the app until the dump finished.
struct SqlEndpoint {
    host: String,
    port: u16,
    user: String,
    password: String,
}

/// The endpoint of a MySQL or PostgreSQL connection, checking on the way that it is the kind the
/// caller expects — the tools of one engine cannot be pointed at the other's server.
async fn sql_endpoint(
    state: &State<'_, DbState>,
    id: &str,
    kind: DbKind,
) -> Result<SqlEndpoint, AppError> {
    let connections = state.connections.lock().await;
    let connection = connections
        .get(id)
        .ok_or_else(|| err!("error.unknownConnection"))?;
    /* Written out rather than closed with a `_`, so that a kind added later is a compile error
    here instead of a wrong message: SQLite reaches this with no endpoint and used to come back
    as `error.noDumpAddress`, which reads as "the address is missing" for a kind that has none
    by construction. */
    let matches = match kind {
        DbKind::Mysql => matches!(connection.handle, DbHandle::Mysql { .. }),
        DbKind::Postgres => matches!(connection.handle, DbHandle::Postgres(_)),
        DbKind::Mongo | DbKind::Redis | DbKind::Sqlite | DbKind::Clickhouse | DbKind::Mssql => {
            false
        }
    };
    if !matches {
        let name = if kind == DbKind::Postgres {
            "PostgreSQL"
        } else {
            "MySQL"
        };
        return Err(err!("error.wrongConnectionKind", kind = name));
    }
    let (host, port) = connection
        .endpoint
        .clone()
        .ok_or_else(|| err!("error.noDumpAddress"))?;
    Ok(SqlEndpoint {
        host,
        port,
        user: connection.config.username.clone().unwrap_or_default(),
        password: connection.config.password.clone().unwrap_or_default(),
    })
}

/// The MongoDB connection string to hand the tools, and the tunnel endpoint to point it at.
async fn mongo_endpoint(
    state: &State<'_, DbState>,
    id: &str,
) -> Result<(String, Option<(String, u16)>), AppError> {
    let connections = state.connections.lock().await;
    let connection = connections
        .get(id)
        .ok_or_else(|| err!("error.unknownConnection"))?;
    if !matches!(connection.handle, DbHandle::Mongo(_)) {
        return Err(err!("error.wrongConnectionKind", kind = "MongoDB"));
    }
    let uri = connection
        .config
        .uri
        .clone()
        .filter(|uri| !uri.trim().is_empty())
        .ok_or_else(|| err!("error.noDumpUri"))?;
    Ok((uri, connection.endpoint.clone()))
}

/// Where MixLab keeps the tools it downloaded for itself.
fn tools_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    app_data_dir(app).map(|dir| dir.join("tools"))
}

/// Tells whatever transfer this connection is running to stop. Silent when it is running none,
/// which is every call but the few that matter.
fn cancel_transfer_in(state: &DbState, id: &str) {
    if let Some(cancel) = state.transfers.lock().unwrap().get(id) {
        cancel.store(true, Ordering::Relaxed);
    }
}

/// The Cancel button on the transfer overlay.
#[tauri::command]
pub fn cancel_db_transfer(state: State<'_, DbState>, id: String) {
    cancel_transfer_in(&state, &id);
}

/// Remembers which server-side session a run is using, and forgets it however the run ends.
///
/// The entry must go whether the script finished, failed, or was killed: left behind, it names a
/// session that by the next press of Cancel is running somebody else's statement. A guard rather
/// than a line at the end of the function, for the reason [`Transfer`] is one — an early `?` or a
/// panic would step over that line.
struct RunningQuery<'a> {
    state: &'a DbState,
    run_id: String,
}

impl<'a> RunningQuery<'a> {
    fn start(state: &'a DbState, run_id: &str) -> Self {
        Self {
            state,
            run_id: run_id.to_string(),
        }
    }
}

impl Drop for RunningQuery<'_> {
    fn drop(&mut self) {
        self.state
            .running_queries
            .lock()
            .unwrap()
            .remove(&self.run_id);
    }
}

/// Registers a transfer for the length of its run, and takes it out again however it ends.
///
/// A guard rather than a pair of calls because the middle of it is a `?` away from returning: a
/// dump that fails on its first line must not leave a flag behind for the *next* dump on that
/// connection to find already set.
struct Transfer<'a> {
    state: &'a DbState,
    id: String,
    cancel: Cancel,
}

impl<'a> Transfer<'a> {
    fn start(state: &'a DbState, id: &str) -> Self {
        let cancel: Cancel = Arc::new(AtomicBool::new(false));
        state
            .transfers
            .lock()
            .unwrap()
            .insert(id.to_string(), Arc::clone(&cancel));
        Transfer {
            state,
            id: id.to_string(),
            cancel,
        }
    }

    /// What `Watch` polls. Owns a handle of its own, so the closure can outlive this guard's
    /// borrow of the state and go to the blocking thread.
    fn flag(&self) -> Cancel {
        Arc::clone(&self.cancel)
    }
}

impl Drop for Transfer<'_> {
    fn drop(&mut self) {
        self.state.transfers.lock().unwrap().remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::{cancel_transfer_in, RunningQuery, Transfer};
    use crate::error::AppError;
    use crate::modules::db::state::DbState;
    use std::cell::Cell;
    use std::sync::atomic::Ordering;

    /// Two scripts running on one connection are two entries, and each forgets only its own.
    ///
    /// Keyed by connection — as this was — the second run's insert would land on the first's key:
    /// Cancel on the first would kill the second, and then whichever finished first would remove
    /// the entry the other was still relying on, so the survivor could not be cancelled at all.
    /// That the key is the run and not the connection is now the command signatures' business;
    /// what is checked here is the half that is not — that finishing forgets one run, not the map.
    #[test]
    fn two_runs_on_one_connection_keep_their_own_session_ids() {
        let state = DbState::default();
        let session = |run: &str| state.running_queries.lock().unwrap().get(run).copied();

        let first = RunningQuery::start(&state, "run-1");
        state
            .running_queries
            .lock()
            .unwrap()
            .insert("run-1".to_string(), 111);
        let second = RunningQuery::start(&state, "run-2");
        state
            .running_queries
            .lock()
            .unwrap()
            .insert("run-2".to_string(), 222);

        assert_eq!(session("run-1"), Some(111));
        assert_eq!(session("run-2"), Some(222));

        // The first script finishing leaves the second's thread id where the Cancel button can
        // still find it.
        drop(first);
        assert_eq!(session("run-1"), None);
        assert_eq!(session("run-2"), Some(222));

        drop(second);
        assert!(state.running_queries.lock().unwrap().is_empty());
    }

    /* What the tab closing has to be able to do, without a database in the room: reach a running
    transfer and set the flag its tool is polling. */

    #[test]
    fn a_running_transfer_can_be_reached_by_its_connection_id() {
        let state = DbState::default();
        let transfer = Transfer::start(&state, "conn-1");
        let flag = transfer.flag();
        assert!(!flag.load(Ordering::Relaxed));

        cancel_transfer_in(&state, "conn-1");
        assert!(
            flag.load(Ordering::Relaxed),
            "the tool's own poll would still say keep going"
        );
    }

    #[test]
    fn cancelling_a_connection_that_is_transferring_nothing_does_nothing() {
        // `disconnect_db` calls this on every close, and most closes are not mid-dump.
        let state = DbState::default();
        cancel_transfer_in(&state, "conn-1");
        assert!(state.transfers.lock().unwrap().is_empty());
    }

    #[test]
    fn a_finished_transfer_leaves_no_flag_for_the_next_one_to_find() {
        // The failure this rules out: a dump that fails on its first line returns early, and the
        // next dump on that connection starts on a flag that is already set.
        let state = DbState::default();
        {
            let transfer = Transfer::start(&state, "conn-1");
            cancel_transfer_in(&state, "conn-1");
            assert!(transfer.flag().load(Ordering::Relaxed));
        }
        assert!(state.transfers.lock().unwrap().is_empty());

        let next = Transfer::start(&state, "conn-1");
        assert!(!next.flag().load(Ordering::Relaxed));
    }

    #[test]
    fn two_connections_transfer_without_stopping_each_other() {
        let state = DbState::default();
        let one = Transfer::start(&state, "conn-1");
        let two = Transfer::start(&state, "conn-2");
        cancel_transfer_in(&state, "conn-1");
        assert!(one.flag().load(Ordering::Relaxed));
        assert!(!two.flag().load(Ordering::Relaxed));
    }

    /// Exactly one rerun, and only when the first attempt died along with the connection. The
    /// counter is what shows it: a write command slipping in here would run twice, so "how many
    /// times it runs" is what has to be locked down.
    #[tokio::test]
    async fn a_read_runs_again_only_after_a_lost_connection() {
        let runs = Cell::new(0);
        let result: Result<u32, AppError> = retry_read!({
            runs.set(runs.get() + 1);
            if runs.get() == 1 {
                Err(err!("error.connectionLost"))
            } else {
                Ok(7)
            }
        });
        assert_eq!(result, Ok(7));
        assert_eq!(runs.get(), 2);

        // If the first attempt succeeded there is no second.
        let runs = Cell::new(0);
        let result: Result<u32, AppError> = retry_read!({
            runs.set(runs.get() + 1);
            // The error type has to be spelled out here: the real command body has its type pinned
            // by `?`, while an `Ok` standing alone in `async` has nothing to infer `E` from.
            Ok::<u32, AppError>(1)
        });
        assert_eq!(result, Ok(1));
        assert_eq!(runs.get(), 1);

        // A server error is no reason to ask again: a wrong SQL statement is still wrong the
        // second time.
        let runs = Cell::new(0);
        let result: Result<u32, AppError> = retry_read!({
            runs.set(runs.get() + 1);
            Err(err!("error.mysql", message = "syntax"))
        });
        assert_eq!(result, Err(err!("error.mysql", message = "syntax")));
        assert_eq!(runs.get(), 1);
    }
}
