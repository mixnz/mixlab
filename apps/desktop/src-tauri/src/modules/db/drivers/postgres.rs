//! PostgreSQL, read side: connecting, listing, and reading a page of a table.
//!
//! Two things here have no counterpart in `mysql.rs`, and both come from the same fact — a
//! PostgreSQL connection is bound to one database and cannot see into another:
//!
//! * [`Pools`] holds a pool per database rather than a pool per server, opened the first time that
//!   database is asked for. What MySQL does with `USE`, this does by dialing again.
//! * A table is named by [`qualify`]/[`resolve`] rather than by a bare name, because PostgreSQL
//!   puts schemas between the database and its tables and two schemas may hold the same name.

pub mod ddl;
pub mod dump;
pub mod engine;
pub(super) mod page;
pub mod script;
pub mod structure;

use crate::error::AppError;
pub use crate::modules::db::drivers::sql::types::{
    ColumnMeta, Filter, ForeignKey, PageQuery, TablePage,
};
use crate::modules::db::models::ServerInfo;
pub use page::table_data;
use serde_json::{Map, Value};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgRow, PgSslMode};
use sqlx::{Column, PgPool, Row, TypeInfo};
use std::collections::{BTreeMap, HashMap};
use tokio::sync::Mutex;

/// What every PostgreSQL command using a connection uses instead of
/// `err!("error.postgres", message = e)` — the same distinction as `mysql::map_error`; see
/// `mysql::lost_connection`.
pub(super) fn map_error(e: sqlx::Error) -> AppError {
    if super::mysql::lost_connection(&e) {
        err!("error.connectionLost")
    } else {
        err!("error.postgres", message = e)
    }
}

/// The database dialed when the connection form leaves the field empty. PostgreSQL insists on one
/// to connect at all, and this is the maintenance database every server is created with.
pub(super) const FALLBACK_DATABASE: &str = "postgres";

/// The schema whose tables are named without a prefix, here and in the sidebar — it is the one on
/// the default `search_path`, so its tables are also what an unqualified name in a query means.
pub const DEFAULT_SCHEMA: &str = "public";

/// One live PostgreSQL connection: a pool for each database that has been opened on it, and what
/// it takes to open another.
///
/// A pool here is what a whole connection is elsewhere. Selecting a database in the sidebar cannot
/// be a `USE`, so it is a second pool to the same server — kept once opened, since walking a
/// server database by database would otherwise pay the connection cost on every step back.
pub struct Pools {
    /// What every pool is opened with, short of the database it points at.
    options: PgConnectOptions,
    /// The database this connection was opened on: what the commands that name none use, and the
    /// one whose failure means the credentials themselves are wrong.
    ///
    /// Behind a lock because it can move once: dropping this very database leaves the name pointing
    /// at nothing, and every later command would dial it. See [`Self::forget_default_database`].
    default_database: std::sync::RwLock<String>,
    /// Guards the map and nothing else — never held across a connection attempt. See [`Self::pool`].
    pools: Mutex<HashMap<String, PgPool>>,
}

impl Pools {
    /// The pool for `database`, opening one if this is the first time it has been asked for.
    ///
    /// `None` — or the empty string — means the database the connection was opened on.
    pub async fn pool(&self, database: Option<&str>) -> Result<PgPool, AppError> {
        let name = match database.filter(|d| !d.is_empty()) {
            Some(database) => database.to_string(),
            None => self.default_database(),
        };

        if let Some(pool) = self.pools.lock().await.get(&name) {
            return Ok(pool.clone());
        }

        // Dialed with the map unlocked: a connection attempt takes as long as the server takes to
        // answer, and every other tab on this connection would wait behind it. Two callers can
        // therefore open the same database at once; the second pool is dropped below rather than
        // replacing the first, which would leave whoever holds the first with a pool nothing
        // closes.
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(self.options.clone().database(&name))
            .await
            // Not through `map_error`: this is where pools are opened, including the first one
            // `connect()` opens — failing here is "could not connect", and the real reason is in
            // `message`.
            .map_err(|e| err!("error.postgres", message = e))?;

        let mut pools = self.pools.lock().await;
        Ok(pools.entry(name).or_insert(pool).clone())
    }

    /// The database this connection was opened on — or the maintenance database, once that one has
    /// been dropped.
    pub fn default_database(&self) -> String {
        self.default_database
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Points the connection at the maintenance database instead, for after the one it was opened
    /// on has been dropped. Without this every command that names no database would go on dialing a
    /// database that is no longer there, and the sidebar would fail until the user reconnected.
    pub fn forget_default_database(&self) {
        *self
            .default_database
            .write()
            .unwrap_or_else(|e| e.into_inner()) = FALLBACK_DATABASE.to_string();
    }

    /// Closes the pool for `database` and forgets it, so the next command that needs it dials
    /// again. Does nothing when none is open.
    ///
    /// This exists for `DROP DATABASE`, which PostgreSQL refuses while anyone is connected to the
    /// database being dropped — and a pool kept open from browsing it a moment ago is exactly such
    /// a connection. Closing waits for the connections to actually go, rather than only dropping
    /// the handle: the server has to see them leave before it will accept the drop.
    pub async fn close_pool(&self, database: &str) {
        let pool = self.pools.lock().await.remove(database);
        if let Some(pool) = pool {
            pool.close().await;
        }
    }

    /// Every pool this connection opened, said goodbye to properly.
    ///
    /// Dropping them would work in the sense that the sockets go, but the server's view of it is a
    /// connection that vanished mid-conversation, and it writes a line about each one. Closing
    /// sends the terminate message first. Taken out of the map before the wait, so nothing new can
    /// be handed one of these on the way out.
    pub async fn close_all(&self) {
        let pools: Vec<PgPool> = self
            .pools
            .lock()
            .await
            .drain()
            .map(|(_, pool)| pool)
            .collect();
        for pool in pools {
            pool.close().await;
        }
    }
}

/// Opens a connection, and proves it: the database it names is dialed here rather than at the
/// first command, so that a wrong password is reported by the Connect button and not by the
/// sidebar a moment later.
pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: Option<&str>,
    use_ssl: Option<bool>,
) -> Result<Pools, AppError> {
    let database = database
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(FALLBACK_DATABASE)
        .to_string();

    let mut options = PgConnectOptions::new()
        .host(host)
        .port(port)
        .username(username)
        .ssl_mode(if use_ssl == Some(false) {
            PgSslMode::Disable
        } else {
            PgSslMode::Prefer
        });
    if !password.is_empty() {
        options = options.password(password);
    }

    let pools = Pools {
        options,
        default_database: std::sync::RwLock::new(database),
        pools: Mutex::new(HashMap::new()),
    };
    pools.pool(None).await?;
    Ok(pools)
}

/// Double-quotes an identifier for interpolation into SQL text, doubling embedded quotes —
/// PostgreSQL's own escaping rule, and what keeps a name that needs quoting (mixed case, a space,
/// a reserved word) from changing meaning on the way in.
pub(super) fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// How a table is named everywhere outside this module: in the sidebar, in the frontend's state,
/// and back down in every command that takes a table.
///
/// A table in `public` is named by itself, since that is the schema an unqualified name already
/// resolves to — so a database that only uses `public`, which is most of them, reads exactly as it
/// did under MySQL. Anything else carries its schema.
///
/// The quoting is what makes this reversible by [`resolve`]: a name holding a `.` or a `"` would
/// otherwise render to something that reads as a different pair.
pub fn qualify(schema: &str, table: &str) -> String {
    let table = if needs_quoting(table) {
        quote_ident(table)
    } else {
        table.to_string()
    };
    if schema == DEFAULT_SCHEMA {
        return table;
    }
    let schema = if needs_quoting(schema) {
        quote_ident(schema)
    } else {
        schema.to_string()
    };
    format!("{schema}.{table}")
}

/// Whether a name would be misread if it were written plainly — the two characters [`qualify`]
/// builds its result out of.
fn needs_quoting(name: &str) -> bool {
    name.contains('.') || name.contains('"')
}

/// The schema and table a [`qualify`]ed name stands for. An unqualified name is a table of
/// [`DEFAULT_SCHEMA`], which is what `qualify` leaves the prefix off for.
pub fn resolve(qualified: &str) -> (String, String) {
    let (schema, table) = split_qualified(qualified);
    (unquote_ident(schema), unquote_ident(table))
}

/// Splits at the dot that separates the two halves, leaving both still quoted as they were. A
/// quoted first half is scanned to its closing quote first, so a dot inside it is not the split.
fn split_qualified(qualified: &str) -> (&str, &str) {
    let mut quoted = false;
    for (i, ch) in qualified.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '.' if !quoted => return (&qualified[..i], &qualified[i + 1..]),
            _ => {}
        }
    }
    (DEFAULT_SCHEMA, qualified)
}

/// Takes the double quotes off an identifier that carries them, undoubling what is inside.
fn unquote_ident(ident: &str) -> String {
    let trimmed = ident.trim();
    match trimmed.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(inner) => inner.replace("\"\"", "\""),
        None => trimmed.to_string(),
    }
}

/// A table as it is written into SQL text: both halves quoted, whatever they hold.
pub(super) fn qualified_sql(schema: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(table))
}

/// A condition leaving out the schemas that belong to the server rather than to the user — their
/// tables are of no use to anyone browsing their own data.
///
/// Takes the alias `pg_namespace` was joined under, since the queries that need it do not all
/// spell it the same way, and a filter naming the wrong alias fails at the server rather than in
/// the compiler.
pub(super) fn system_schema_filter(namespace: &str) -> String {
    format!(
        "{namespace}.nspname NOT IN ('pg_catalog', 'information_schema') \
         AND {namespace}.nspname NOT LIKE 'pg\\_toast%' \
         AND {namespace}.nspname NOT LIKE 'pg\\_temp%'"
    )
}

/// What the header shows about the server. `version()` is one sentence carrying both halves —
/// "PostgreSQL 16.2 on x86_64-pc-linux-gnu, compiled by gcc..." — so the number comes from
/// `server_version`, which is only the number, and the machine is cut out of the sentence.
pub async fn server_info(pool: &PgPool) -> Result<ServerInfo, AppError> {
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(pool)
        .await
        .map_err(map_error)?;
    let banner: String = sqlx::query_scalar("SELECT version()")
        .fetch_one(pool)
        .await
        .unwrap_or_default();

    // "PostgreSQL 16.2 on x86_64-pc-linux-gnu, compiled by ..." — what stands between " on " and
    // the comma is the machine, and anything else leaves the header showing the version alone.
    let os = banner
        .split_once(" on ")
        .map(|(_, rest)| rest.split(',').next().unwrap_or("").trim().to_string())
        .unwrap_or_default();
    Ok(ServerInfo { version, os })
}

/// Every database on the server that can be connected to.
///
/// Templates are left out: `template0` refuses connections outright, and neither is a database
/// anyone browses — they are what `CREATE DATABASE` copies from.
pub async fn list_databases(pool: &PgPool) -> Result<Vec<String>, AppError> {
    sqlx::query_scalar(
        "SELECT datname FROM pg_database
         WHERE datallowconn AND NOT datistemplate
         ORDER BY datname",
    )
    .fetch_all(pool)
    .await
    .map_err(map_error)
}

/// Every table and view of the connected database, across every schema the user can see, named as
/// [`qualify`] names them.
///
/// `public` first and the other schemas after it, alphabetically — so the tables of the schema
/// that needs no prefix are together at the top, and the rest read as the groups they are. That is
/// as much of a tree as a flat list can be.
pub async fn list_tables(pool: &PgPool) -> Result<Vec<String>, AppError> {
    let schemas = system_schema_filter("n");
    let sql = format!(
        "SELECT n.nspname, c.relname
         FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND {schemas}
         ORDER BY (n.nspname <> '{DEFAULT_SCHEMA}'), n.nspname, c.relname"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .map_err(map_error)?;
    Ok(rows
        .iter()
        .map(|row| {
            qualify(
                &row.get::<String, _>("nspname"),
                &row.get::<String, _>("relname"),
            )
        })
        .collect())
}

/// Reads the columns of one table: the names in table order, with what each one is declared as.
///
/// Two spellings of the type, because they answer different questions. `data_type` is what the
/// column is, modifiers and all, and is what the structure panel shows. `base_type` drops the
/// modifier, and is what a written value is cast to — see [`placeholder`].
///
/// Read from the catalogue rather than from a result set, so that a table with no rows still
/// describes itself — and `attnum > 0 AND NOT attisdropped` is what leaves out the system columns
/// and the tombstones a dropped column leaves behind.
async fn table_columns(pool: &PgPool, schema: &str, table: &str) -> Result<Vec<PgRow>, AppError> {
    sqlx::query(
        "SELECT a.attname AS name,
                format_type(a.atttypid, a.atttypmod) AS data_type,
                format_type(a.atttypid, NULL) AS base_type,
                NOT a.attnotnull AS nullable,
                pg_get_expr(d.adbin, d.adrelid) AS default_value,
                a.attidentity::text AS identity,
                a.attgenerated::text AS generated
         FROM pg_attribute a
         JOIN pg_class c ON c.oid = a.attrelid
         JOIN pg_namespace n ON n.oid = c.relnamespace
         LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
         WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped
         ORDER BY a.attnum",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(map_error)
}

/// The tokens `ColumnMeta::extra` carries for one column — what the server fills in itself.
pub(super) fn extra_tokens(row: &PgRow) -> String {
    let mut tokens: Vec<&str> = Vec::new();
    // 'a' is GENERATED ALWAYS AS IDENTITY, 'd' BY DEFAULT; '' is neither.
    if !row.get::<String, _>("identity").is_empty() {
        tokens.push("identity");
    }
    // 's' is a STORED generated column. PostgreSQL has no virtual ones to tell it apart from.
    if !row.get::<String, _>("generated").is_empty() {
        tokens.push("generated");
    }
    // A `serial` column is an ordinary integer whose default draws from a sequence — the older
    // spelling of identity, and still what most existing schemas use.
    if row
        .get::<Option<String>, _>("default_value")
        .is_some_and(|d| d.starts_with("nextval("))
    {
        tokens.push("nextval");
    }
    tokens.join(" ")
}

/// The primary key's columns, in the order the key declares them — which is not the table's order,
/// and is the order a composite key has to be written in.
async fn primary_key(pool: &PgPool, schema: &str, table: &str) -> Result<Vec<String>, AppError> {
    sqlx::query_scalar(
        "SELECT a.attname
         FROM pg_index i
         JOIN pg_class c ON c.oid = i.indrelid
         JOIN pg_namespace n ON n.oid = c.relnamespace
         JOIN LATERAL unnest(i.indkey::smallint[]) WITH ORDINALITY AS k(attnum, ord) ON TRUE
         JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum
         WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2
         ORDER BY k.ord",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(map_error)
}

/// What each column is declared as, without its length or precision — keyed by column name.
///
/// The modifier is dropped on purpose. A value cast to `character varying(20)` is *truncated* to
/// twenty characters, silently; cast to `character varying` and then assigned to the column, the
/// same value is refused by the column's own length check. Refusing is the right answer: the user
/// asked for those characters to be stored.
async fn column_types(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<BTreeMap<String, String>, AppError> {
    Ok(table_columns(pool, schema, table)
        .await?
        .iter()
        .map(|r| (r.get::<String, _>("name"), r.get::<String, _>("base_type")))
        .collect())
}

/// The `$n` a written value goes in as, cast to the type of the column it is going into.
///
/// The cast is what makes writing work at all here. The frontend sends every edited cell as text,
/// and MySQL coerces text to the column's type on its own — PostgreSQL does not, and refuses
/// `SET quantity = $1` outright when `$1` arrives declared as `text` and `quantity` is an integer.
/// Casting explicitly is the ordinary way round that, and it holds for every type: PostgreSQL will
/// convert text to anything by handing it to that type's own input function, which is exactly how
/// the same value typed into `psql` would be read.
///
/// The type is interpolated rather than bound because a cast target cannot be a parameter. It is
/// safe to interpolate: it is not the user's text but `format_type`'s output, read back from the
/// catalogue, and already quoted by the server wherever quoting is needed. A column whose type is
/// unknown here is left uncast — it does not exist, and the statement will say so.
fn placeholder(n: usize, column: &str, types: &BTreeMap<String, String>) -> String {
    match types.get(column) {
        Some(kind) => format!("${n}::{kind}"),
        None => format!("${n}"),
    }
}

/// A predicate matching the row `key` names, with its placeholders numbered from `first`.
///
/// `IS NOT DISTINCT FROM` rather than `=`, so that a key column which is itself NULL still matches
/// — plain `=` yields NULL, which is not true, so such a row could never be found again. It is
/// PostgreSQL's spelling of MySQL's `<=>`.
fn key_predicate(
    key: &Map<String, Value>,
    types: &BTreeMap<String, String>,
    first: usize,
) -> String {
    key.keys()
        .enumerate()
        .map(|(i, column)| {
            format!(
                "{} IS NOT DISTINCT FROM {}",
                quote_ident(column),
                placeholder(first + i, column, types)
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Binds a JSON value — always `Null` or `String`, since the frontend only ever sends edited text
/// or an explicit null — as `Option<&str>`, for a placeholder that carries the cast to the column's
/// real type. See [`placeholder`].
fn bind_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    value: &'q Value,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match value {
        Value::String(s) => query.bind(Some(s.as_str())),
        _ => query.bind(None::<&str>),
    }
}

/// Updates exactly one row, identified by `key` (the primary key's columns, or — when a table has
/// none — every column as a fallback). Runs inside a transaction that first counts what the key
/// matches, so the no-primary-key fallback cannot silently clobber a row's duplicate.
pub async fn update_row(
    pool: &PgPool,
    table: &str,
    updates: &Map<String, Value>,
    key: &Map<String, Value>,
) -> Result<(), AppError> {
    if updates.is_empty() {
        return Ok(());
    }
    if key.is_empty() {
        return Err(err!("error.updateWithoutKey"));
    }

    let (schema, name) = resolve(table);
    let qualified = qualified_sql(&schema, &name);
    let types = column_types(pool, &schema, &name).await?;

    let set_clause = updates
        .keys()
        .enumerate()
        .map(|(i, column)| {
            format!(
                "{} = {}",
                quote_ident(column),
                placeholder(i + 1, column, &types)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    let mut tx = pool.begin().await.map_err(map_error)?;

    let count_sql = format!(
        "SELECT COUNT(*) FROM {qualified} WHERE {}",
        key_predicate(key, &types, 1)
    );
    let mut count_query = sqlx::query_scalar(sqlx::AssertSqlSafe(count_sql));
    for value in key.values() {
        count_query = match value {
            Value::String(s) => count_query.bind(Some(s.as_str())),
            _ => count_query.bind(None::<&str>),
        };
    }
    let matched: i64 = count_query.fetch_one(&mut *tx).await.map_err(map_error)?;
    if matched != 1 {
        tx.rollback().await.map_err(map_error)?;
        return Err(err!("error.rowsMatched", matched = matched));
    }

    // The key's placeholders carry on where the SET clause's left off: one numbering runs through
    // the whole statement, unlike MySQL's positional `?`.
    let update_sql = format!(
        "UPDATE {qualified} SET {set_clause} WHERE {}",
        key_predicate(key, &types, updates.len() + 1)
    );
    let mut update_query = sqlx::query(sqlx::AssertSqlSafe(update_sql));
    for value in updates.values() {
        update_query = bind_value(update_query, value);
    }
    for value in key.values() {
        update_query = bind_value(update_query, value);
    }
    update_query.execute(&mut *tx).await.map_err(map_error)?;

    tx.commit().await.map_err(map_error)?;
    Ok(())
}

/// Inserts `rows`, one map per new row, all in a single transaction: if any one of them is
/// rejected, none of them land.
///
/// A row only carries the columns it has something to say about — a column left out of the map is
/// left out of that row's INSERT too, so the table's own DEFAULT (or identity, or a generated
/// expression) is what fills it. That is also why each row is its own statement rather than one
/// multi-VALUES INSERT: rows may fill in different sets of columns, and the error a rejected row
/// produces can then say which row it was.
pub async fn insert_rows(
    pool: &PgPool,
    table: &str,
    rows: &[Map<String, Value>],
) -> Result<(), AppError> {
    if rows.is_empty() {
        return Ok(());
    }

    let (schema, name) = resolve(table);
    let qualified = qualified_sql(&schema, &name);
    let types = column_types(pool, &schema, &name).await?;
    let mut tx = pool.begin().await.map_err(map_error)?;

    for (i, row) in rows.iter().enumerate() {
        // `DEFAULT VALUES` is PostgreSQL's way of spelling "a row that is nothing but defaults".
        let sql = if row.is_empty() {
            format!("INSERT INTO {qualified} DEFAULT VALUES")
        } else {
            let columns = row
                .keys()
                .map(|c| quote_ident(c))
                .collect::<Vec<_>>()
                .join(", ");
            let values = row
                .keys()
                .enumerate()
                .map(|(n, column)| placeholder(n + 1, column, &types))
                .collect::<Vec<_>>()
                .join(", ");
            format!("INSERT INTO {qualified} ({columns}) VALUES ({values})")
        };
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for value in row.values() {
            query = bind_value(query, value);
        }
        if let Err(e) = query.execute(&mut *tx).await {
            tx.rollback().await.map_err(map_error)?;
            return Err(err!("error.rowFailed", index = i + 1).caused_by(map_error(e)));
        }
    }

    tx.commit().await.map_err(map_error)?;
    Ok(())
}

/// Deletes the rows `keys` names — each map is one row's primary key columns, or every column when
/// the table has no primary key — or every row in the table when `all` is set. The deletes run in
/// one transaction: if any of them fails, none of them land.
///
/// `reset_sequence` afterwards puts the table's identity or `serial` counter back to 1, so the next
/// insert numbers from 1 again.
pub async fn delete_rows(
    pool: &PgPool,
    table: &str,
    keys: &[Map<String, Value>],
    all: bool,
    reset_sequence: bool,
) -> Result<(), AppError> {
    if !all && keys.is_empty() && !reset_sequence {
        return Ok(());
    }

    let (schema, name) = resolve(table);
    let qualified = qualified_sql(&schema, &name);
    let types = column_types(pool, &schema, &name).await?;
    // A table with a primary key is identified exactly by it, so one row's predicate can only ever
    // match one row. Without one there is nothing to stop a predicate matching a row's duplicate,
    // and PostgreSQL has no `DELETE ... LIMIT` to cap it with — hence the subquery below.
    let keyed = !primary_key(pool, &schema, &name).await?.is_empty();

    let mut tx = pool.begin().await.map_err(map_error)?;

    if all {
        let sql = format!("DELETE FROM {qualified}");
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
    } else {
        for key in keys {
            if key.is_empty() {
                tx.rollback().await.map_err(map_error)?;
                return Err(err!("error.deleteWithoutKey"));
            }
            let predicate = key_predicate(key, &types, 1);
            let sql = if keyed {
                format!("DELETE FROM {qualified} WHERE {predicate}")
            } else {
                // One physical row, picked and then deleted by where it sits. `tableoid` travels
                // with `ctid` because a `ctid` is only unique within one table, and a partitioned
                // table is several — matching on the pair cannot reach into the wrong partition.
                format!(
                    "DELETE FROM {qualified} WHERE (tableoid, ctid) = \
                     (SELECT tableoid, ctid FROM {qualified} WHERE {predicate} LIMIT 1)"
                )
            };
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
            for value in key.values() {
                query = bind_value(query, value);
            }
            query.execute(&mut *tx).await.map_err(map_error)?;
        }
    }

    tx.commit().await.map_err(map_error)?;

    if reset_sequence {
        restart_sequence(pool, &schema, &name).await?;
    }
    Ok(())
}

/// Puts the table's identity or `serial` counter back to 1.
///
/// The sequence is asked for by column rather than guessed at by name: `pg_get_serial_sequence`
/// follows the dependency the server itself recorded, so a sequence renamed since — or one an
/// identity column owns, which never had the `_seq` name to guess at — is still found. A table with
/// no such column has nothing to reset, and that is not an error: the caller offers the reset
/// alongside the delete, and a table without a counter simply has no counter to put back.
async fn restart_sequence(pool: &PgPool, schema: &str, table: &str) -> Result<(), AppError> {
    let counter = table_columns(pool, schema, table)
        .await?
        .into_iter()
        .find(|r| {
            let extra = extra_tokens(r);
            extra.contains("identity") || extra.contains("nextval")
        })
        .map(|r| r.get::<String, _>("name"));
    let Some(column) = counter else {
        return Ok(());
    };

    let sequence: Option<String> = sqlx::query_scalar("SELECT pg_get_serial_sequence($1, $2)")
        .bind(qualified_sql(schema, table))
        .bind(&column)
        .fetch_one(pool)
        .await
        .map_err(map_error)?;
    let Some(sequence) = sequence else {
        return Ok(());
    };

    // Already quoted by `pg_get_serial_sequence`, which returns the sequence written as SQL.
    let sql = format!("ALTER SEQUENCE {sequence} RESTART WITH 1");
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(pool)
        .await
        .map_err(map_error)?;
    Ok(())
}

pub(super) fn row_to_json(row: &PgRow) -> Map<String, Value> {
    let mut obj = Map::new();
    for (i, col) in row.columns().iter().enumerate() {
        obj.insert(col.name().to_string(), column_value(row, i));
    }
    obj
}

/// One value, as the closest JSON the frontend can show.
///
/// Unlike MySQL, PostgreSQL decodes by the column's declared type rather than by what the bytes
/// could be read as, so the order below is only about which types exist — `i32` will not claim an
/// `int8` column the way MySQL's `bool` would claim an `int`.
///
/// A type not listed here never reaches this function from the grid: {@link table_data} asks for
/// such a column as text, having read what it is declared as first. The fall through to null at
/// the end is for the callers that cannot do that — a statement whose result set is only known
/// once it comes back.
pub(super) fn column_value(row: &PgRow, i: usize) -> Value {
    if let Ok(v) = row.try_get::<Option<bool>, _>(i) {
        return v.map(Value::from).unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<i16>, _>(i) {
        return v.map(Value::from).unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<i32>, _>(i) {
        return v.map(Value::from).unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<i64>, _>(i) {
        return v.map(Value::from).unwrap_or(Value::Null);
    }
    // NUMERIC, rendered as a string to keep the precision an f64 would round away.
    if let Ok(v) = row.try_get::<Option<rust_decimal::Decimal>, _>(i) {
        return v
            .map(|d| Value::String(d.to_string()))
            .unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<f32>, _>(i) {
        return v
            .and_then(|n| serde_json::Number::from_f64(n as f64).map(Value::Number))
            .unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(i) {
        return v
            .and_then(|n| serde_json::Number::from_f64(n).map(Value::Number))
            .unwrap_or(Value::Null);
    }
    // `timestamptz` is the one that carries a zone; `timestamp` decodes as naive and would be
    // refused as `DateTime<Utc>`, so both spellings are tried.
    if let Ok(v) = row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(i) {
        return v
            .map(|d| Value::String(d.naive_utc().to_string()))
            .unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<chrono::NaiveDateTime>, _>(i) {
        return v
            .map(|d| Value::String(d.to_string()))
            .unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<chrono::NaiveDate>, _>(i) {
        return v
            .map(|d| Value::String(d.to_string()))
            .unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<chrono::NaiveTime>, _>(i) {
        return v
            .map(|t| Value::String(t.to_string()))
            .unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<uuid::Uuid>, _>(i) {
        return v
            .map(|u| Value::String(u.to_string()))
            .unwrap_or(Value::Null);
    }
    // Gated on the column's own type, as on MySQL: `Json` would also claim a text column that
    // merely happens to hold JSON, and reparsing it would reorder its keys.
    let type_name = row.column(i).type_info().name();
    if type_name == "JSON" || type_name == "JSONB" {
        if let Ok(v) = row.try_get::<Option<Value>, _>(i) {
            return v.unwrap_or(Value::Null);
        }
    }
    if let Ok(v) = row.try_get::<Option<String>, _>(i) {
        return v.map(Value::String).unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(i) {
        use base64::Engine;
        return v
            .map(|bytes| Value::String(base64::engine::general_purpose::STANDARD.encode(bytes)))
            .unwrap_or(Value::Null);
    }
    Value::Null
}

/// The parts of this module that decide what SQL is written, rather than what a server answers.
#[cfg(test)]
mod tests;
