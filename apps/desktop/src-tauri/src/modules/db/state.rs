use crate::modules::db::models::ConnectionConfig;
use crate::ssh::Tunnel;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A live connection to one server, in the form its driver hands out.
///
/// Cloning one is cheap by construction — a pool, a driver client, or an `Arc` — because that is
/// how a command gets hold of a connection without holding the connection map while it runs: see
/// `commands::handle`.
#[derive(Clone)]
pub enum DbHandle {
    /// The pool, and whether the server it reaches is MariaDB rather than MySQL. The flavour
    /// travels with the pool because it is a property of that one server and is read once, when
    /// the connection is opened — see `db::mysql::detect_mariadb`.
    Mysql {
        pool: sqlx::MySqlPool,
        mariadb: bool,
    },
    /// One pool per database rather than one for the server: a PostgreSQL connection is bound to
    /// the database it was opened on, so selecting another in the sidebar dials again. Behind an
    /// `Arc` because the set of them grows as databases are opened — see `db::postgres::Pools`.
    Postgres(Arc<crate::modules::db::drivers::postgres::Pools>),
    Mongo(mongodb::Client),
    /// Behind a lock of its own: unlike the others, a Redis connection is used through `&mut`,
    /// and selecting a database replaces it outright.
    Redis(Arc<Mutex<crate::modules::db::drivers::redis::Connection>>),
    /// A file rather than a server, so there is no endpoint, no tunnel and no credential behind
    /// this one — the pool is the whole of it. Still a pool and not a single connection: the
    /// workspace reads a page of a table while the Query tab runs a script, and SQLite serialises
    /// those itself.
    Sqlite(sqlx::SqlitePool),
    /// A base URL and a set of credentials, not a socket — every call is its own HTTP request, so
    /// there is nothing here for `disconnect_db` to close.
    Clickhouse(crate::modules::db::drivers::clickhouse::Connection),
    /// One pool for the whole server, like MySQL and unlike PostgreSQL: a TDS session moves
    /// between databases with `USE` or a three-part name, so `database` in a command names a
    /// database to reach into rather than a pool to pick.
    Mssql(crate::modules::db::drivers::mssql::Pool),
}

pub struct ActiveConnection {
    pub handle: DbHandle,
    /// What the connection was opened with. Kept because the dump and restore tools dial the
    /// server themselves rather than borrowing the pool, and so need the credentials again.
    pub config: ConnectionConfig,
    /// The address actually dialed: the tunnel's local end when there is one, and the configured
    /// host and port when there is not. `None` for a MongoDB connection with no tunnel, whose
    /// address is whatever its URI says.
    pub endpoint: Option<(String, u16)>,
    /// Keeps the SSH port forward open, and is what the Retry button reaches through to open the
    /// session again — see `commands::tunnel_reconnect`. Dropping this connection drops the tunnel
    /// with it, and that is what tears the forward down — see {@link Tunnel}.
    pub tunnel: Option<Tunnel>,
}

/// A transfer that can be called off: a flag the tool's own loop reads four times a second.
///
/// An `AtomicBool` rather than a channel because the thing being stopped is a blocking loop around
/// a child process — see `drivers::dump::run`, which polls it and kills the child.
pub type Cancel = Arc<AtomicBool>;

#[derive(Default)]
pub struct DbState {
    /// Every open connection, by the id handed to the frontend.
    ///
    /// The lock guards the map and nothing else. It is taken to look a connection up and released
    /// again before anything is run on what was found — a query awaited while holding it would
    /// stop every other command in the app, in every tab, for as long as it took.
    pub connections: Mutex<HashMap<String, ActiveConnection>>,
    /// The server-side id of the session each *run* is using while it lasts — MySQL's thread id,
    /// or PostgreSQL's backend pid. It is what `KILL QUERY` and `pg_cancel_backend` name, and so
    /// the only thing that lets the Cancel button reach a statement already in flight.
    ///
    /// Keyed by the run, not by the connection. Two scripts on one connection are two runs, and a
    /// map keyed by the connection would fail them both: the second's insert would overwrite the
    /// first's, so Cancel on the first would kill the second, and then whichever finished first
    /// would remove the entry the other was still relying on. One `QueryEditor` per workspace
    /// hides all of that today — a second query tab is what would show it.
    ///
    /// A blocking lock rather than an async one: nothing is awaited while it is held, and it has
    /// to be usable from the plain closure `mysql::script::run` announces the id through.
    pub running_queries: std::sync::Mutex<HashMap<String, u64>>,
    /// The transfer each connection is running, while it runs one.
    ///
    /// A dump or a restore is an external tool that can run for minutes, and nothing in the app
    /// used to be able to stop one: closing the tab left `mysqldump` writing a file nobody was
    /// waiting for, and its progress arriving on `transfer://progress` for a connection that no
    /// longer existed. This is what `disconnect_db` and the Cancel button reach through.
    ///
    /// One per connection: the workspace is covered while a transfer runs, so there is never a
    /// second to keep track of.
    pub transfers: std::sync::Mutex<HashMap<String, Cancel>>,
}
