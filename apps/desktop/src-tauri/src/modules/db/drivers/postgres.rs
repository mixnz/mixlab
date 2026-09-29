//! PostgreSQL, read side: connecting, listing, and reading a page of a table.
//!
//! Two things here have no counterpart in `mysql.rs`, and both come from the same fact — a
//! PostgreSQL connection is bound to one database and cannot see into another:
//!
//! * [`Pools`] holds a pool per database rather than a pool per server, opened the first time that
//!   database is asked for. What MySQL does with `USE`, this does by dialing again.
//! * A table is named by [`qualify`]/[`resolve`] rather than by a bare name, because PostgreSQL
//!   puts schemas between the database and its tables and two schemas may hold the same name.

use super::filters::{escape_like, split_list};
use crate::error::AppError;
use crate::modules::db::models::ServerInfo;
use serde::{Deserialize, Serialize};
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

pub async fn query(pool: &PgPool, sql: &str) -> Result<Vec<Map<String, Value>>, AppError> {
    // As in `mysql::query`: a borrowed, non-'static statement needs a connection of its own rather
    // than the pool, and this client runs user-authored SQL by design.
    let mut conn = pool.acquire().await.map_err(map_error)?;
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await
        .map_err(map_error)?;
    Ok(rows.iter().map(row_to_json).collect())
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

/// The row a foreign key column points at.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKey {
    /// [`qualify`]ed, so a key into another schema names a table the sidebar can actually open —
    /// unlike MySQL, where a cross-schema key has no name the grid could follow.
    pub table: String,
    pub column: String,
}

/// What is known about one column beyond its name — everything a new row has to respect.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    /// The declared type as PostgreSQL spells it: `character varying(255)`, `integer`, `jsonb`.
    pub data_type: String,
    pub nullable: bool,
    /// The column's DEFAULT as an expression, or `None` when it has none. PostgreSQL keeps every
    /// default as an expression, so even a literal one arrives cast — `'new'::text`.
    pub default_value: Option<String>,
    /// Which of the things the server fills in for itself this column is, as space-separated
    /// tokens: `identity`, `generated`, `nextval`. Read by `src/postgres/columns.ts`, and the
    /// counterpart of what `SHOW COLUMNS` calls Extra on MySQL.
    pub extra: String,
    pub foreign_key: Option<ForeignKey>,
}

/// Which columns of the table are foreign keys, and what each one points at.
///
/// Only the constraints the connected user can see are here, and a failure is swallowed by the
/// caller: the markers are decoration on the grid, and losing them must not cost the rows.
async fn foreign_keys(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<BTreeMap<String, ForeignKey>, AppError> {
    let rows = sqlx::query(
        "SELECT att.attname AS column_name,
                refn.nspname AS ref_schema,
                refc.relname AS ref_table,
                refatt.attname AS ref_column
         FROM pg_constraint con
         JOIN pg_class rel ON rel.oid = con.conrelid
         JOIN pg_namespace nsp ON nsp.oid = rel.relnamespace
         JOIN pg_class refc ON refc.oid = con.confrelid
         JOIN pg_namespace refn ON refn.oid = refc.relnamespace
         JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS k(att, refatt, ord) ON TRUE
         JOIN pg_attribute att ON att.attrelid = con.conrelid AND att.attnum = k.att
         JOIN pg_attribute refatt ON refatt.attrelid = con.confrelid AND refatt.attnum = k.refatt
         WHERE con.contype = 'f' AND nsp.nspname = $1 AND rel.relname = $2
         ORDER BY con.conname, k.ord",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    let mut keys = BTreeMap::new();
    for row in &rows {
        keys.entry(row.get::<String, _>("column_name"))
            .or_insert_with(|| ForeignKey {
                table: qualify(
                    &row.get::<String, _>("ref_schema"),
                    &row.get::<String, _>("ref_table"),
                ),
                column: row.get::<String, _>("ref_column"),
            });
    }
    Ok(keys)
}

/// One condition on the rows a page is cut out of, as the grid's filter bar sends it. The same
/// shape as MySQL's, and the operator ids are the same — only what they become differs.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub column: String,
    pub operator: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// Turns the filter rows into a WHERE clause and the values to bind into it, in order.
///
/// Every value reaches the server as a bound parameter; the column name is the one part that has
/// to be interpolated, which is why it is checked against the table's own columns first.
///
/// Two operators differ from MySQL's by more than spelling. Comparisons are made against the
/// column cast to text, because a filter value arrives as text and PostgreSQL — unlike MySQL —
/// will not coerce it to the column's type on its own. And `regexp` is `~`, PostgreSQL's own
/// match operator.
///
/// The ordering operators are the one place that cast has to go the other way. `"id"::text > $1`
/// would sort 10 before 9, so the column is left as it is and the *value* is cast to the column's
/// type instead — which is what `columns` carries the type for. Binding the text alone is not
/// enough: sqlx names a bound `&str` as `text` on the wire, and PostgreSQL will not compare an
/// `integer` with one ("operator does not exist: integer > text").
fn build_where(
    filters: &[Filter],
    columns: &BTreeMap<String, String>,
) -> Result<(String, Vec<String>), AppError> {
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    // The number the next placeholder gets: PostgreSQL numbers them, so unlike MySQL's `?` they
    // cannot simply be counted off the bind list at the end.
    let mut next = 1usize;
    let mut placeholder = |binds: &mut Vec<String>, value: String| {
        binds.push(value);
        let p = format!("${next}");
        next += 1;
        p
    };

    for filter in filters {
        let Some(base_type) = columns.get(&filter.column) else {
            return Err(err!("error.unknownFilterColumn", column = &filter.column));
        };
        // Cast to text so that the value bound against it — always text — compares as typed. The
        // ordering operators are the exception: `>` on text would sort 10 before 9, so there the
        // column stands and the bound value is cast to its type with `typed` below.
        let col = format!("{}::text", quote_ident(&filter.column));
        let raw = quote_ident(&filter.column);
        // The type comes from `format_type`, which is PostgreSQL's own spelling of it — already
        // quoted where a name needs it, so it can be written into the cast as it stands.
        let typed = |p: String| format!("CAST({p} AS {base_type})");
        let value = filter.value.as_deref().unwrap_or("");
        let operator = filter.operator.as_str();

        let clause = match operator {
            "eq" => format!("{col} = {}", placeholder(&mut binds, value.to_string())),
            "ne" => format!("{col} <> {}", placeholder(&mut binds, value.to_string())),
            "gt" => format!(
                "{raw} > {}",
                typed(placeholder(&mut binds, value.to_string()))
            ),
            "gte" => format!(
                "{raw} >= {}",
                typed(placeholder(&mut binds, value.to_string()))
            ),
            "lt" => format!(
                "{raw} < {}",
                typed(placeholder(&mut binds, value.to_string()))
            ),
            "lte" => format!(
                "{raw} <= {}",
                typed(placeholder(&mut binds, value.to_string()))
            ),
            "contains" => format!(
                "{col} LIKE {}",
                placeholder(&mut binds, format!("%{}%", escape_like(value)))
            ),
            "notContains" => format!(
                "{col} NOT LIKE {}",
                placeholder(&mut binds, format!("%{}%", escape_like(value)))
            ),
            "startsWith" => format!(
                "{col} LIKE {}",
                placeholder(&mut binds, format!("{}%", escape_like(value)))
            ),
            "endsWith" => format!(
                "{col} LIKE {}",
                placeholder(&mut binds, format!("%{}", escape_like(value)))
            ),
            "like" => format!("{col} LIKE {}", placeholder(&mut binds, value.to_string())),
            "notLike" => format!(
                "{col} NOT LIKE {}",
                placeholder(&mut binds, value.to_string())
            ),
            "regexp" => format!("{col} ~ {}", placeholder(&mut binds, value.to_string())),
            "notRegexp" => format!("{col} !~ {}", placeholder(&mut binds, value.to_string())),
            "in" | "notIn" => {
                let items = split_list(value);
                if items.is_empty() {
                    continue;
                }
                let placeholders = items
                    .into_iter()
                    .map(|item| placeholder(&mut binds, item))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql_op = if operator == "in" { "IN" } else { "NOT IN" };
                format!("{col} {sql_op} ({placeholders})")
            }
            "between" | "notBetween" => {
                let items = split_list(value);
                // Two bounds or nothing — one of them alone says nothing about a range.
                if items.len() < 2 {
                    continue;
                }
                let low = typed(placeholder(&mut binds, items[0].clone()));
                let high = typed(placeholder(&mut binds, items[1].clone()));
                let sql_op = if operator == "between" {
                    "BETWEEN"
                } else {
                    "NOT BETWEEN"
                };
                format!("{raw} {sql_op} {low} AND {high}")
            }
            "isNull" => format!("{raw} IS NULL"),
            "isNotNull" => format!("{raw} IS NOT NULL"),
            "isEmpty" => format!("{col} = ''"),
            "isNotEmpty" => format!("{col} <> ''"),
            other => return Err(err!("error.unknownFilterOperator", operator = other)),
        };

        clauses.push(clause);
    }

    if clauses.is_empty() {
        return Ok((String::new(), binds));
    }
    Ok((format!(" WHERE {}", clauses.join(" AND ")), binds))
}

/// Which page of a table is wanted, and what it is cut out of.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageQuery {
    pub page: i64,
    pub page_size: i64,
    /// Ignored unless it names a real column of this table.
    #[serde(default)]
    pub sort_column: Option<String>,
    #[serde(default)]
    pub sort_desc: bool,
    /// ANDed together; `total` counts what is left after them.
    #[serde(default)]
    pub filters: Vec<Filter>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TablePage {
    pub columns: Vec<String>,
    pub column_meta: BTreeMap<String, ColumnMeta>,
    pub primary_key: Vec<String>,
    /// The identity or `serial` column, if the table has one — the only kind with a counter worth
    /// offering to reset after a delete.
    pub auto_increment_column: Option<String>,
    pub rows: Vec<Map<String, Value>>,
    pub total: i64,
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

/// Whether [`column_value`] has a decoder for this type, given as `format_type` spells it.
///
/// The ones it has not — arrays, user-defined enums, `inet`, `interval`, `money`, ranges,
/// composites — are asked for as text instead, which PostgreSQL can always produce and which shows
/// the value the way the server itself prints it. Without that a `text[]` column would arrive as
/// null, indistinguishable in the grid from a row that really is NULL.
///
/// Every number that can hold `NaN` or an infinity is on that list too, though [`column_value`] has
/// a decoder for each: `numeric` because `rust_decimal::Decimal` refuses those values outright, and
/// `real`/`double precision` because JSON has no way to write them — `serde_json::Number::from_f64`
/// answers `None`, and the branch falls through to null. No later branch would claim the column
/// either way, so a row holding `Infinity` would read in the grid as one holding nothing.
fn is_decodable(data_type: &str) -> bool {
    let data_type = data_type.trim();
    // Tested before the modifier is cut off, since the `[]` comes *after* it: a
    // `character varying(255)[]` would otherwise be read as a plain `character varying`, decoded as
    // one, and arrive as null.
    if data_type.ends_with("[]") {
        return false;
    }
    let base = data_type.split('(').next().unwrap_or("").trim();
    matches!(
        base,
        "boolean"
            | "smallint"
            | "integer"
            | "bigint"
            | "text"
            | "character varying"
            | "character"
            | "name"
            | "uuid"
            | "json"
            | "jsonb"
            | "bytea"
            | "date"
            | "time without time zone"
            | "timestamp without time zone"
            | "timestamp with time zone"
    )
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

/// Reads one page of a table. `query.sort_column` orders the whole table before the page is cut
/// out of it, and is ignored unless it names a real column — which is also what keeps it out of
/// the SQL text unchecked. `query.filters` narrows the table down first, and `total` counts what
/// is left after them.
pub async fn table_data(
    pool: &PgPool,
    table: &str,
    query: &PageQuery,
) -> Result<TablePage, AppError> {
    let (schema, name) = resolve(table);
    let qualified = qualified_sql(&schema, &name);

    let column_rows = table_columns(pool, &schema, &name).await?;
    let columns: Vec<String> = column_rows
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    let mut foreign_keys = foreign_keys(pool, &schema, &name).await.unwrap_or_default();

    let column_meta: BTreeMap<String, ColumnMeta> = column_rows
        .iter()
        .map(|r| {
            let name = r.get::<String, _>("name");
            let foreign_key = foreign_keys.remove(&name);
            (
                name,
                ColumnMeta {
                    data_type: r.get::<String, _>("data_type"),
                    nullable: r.get::<bool, _>("nullable"),
                    default_value: r.get::<Option<String>, _>("default_value"),
                    extra: extra_tokens(r),
                    foreign_key,
                },
            )
        })
        .collect();

    let primary_key = primary_key(pool, &schema, &name).await.unwrap_or_default();
    let auto_increment_column = column_rows
        .iter()
        .find(|r| {
            let extra = extra_tokens(r);
            extra.contains("identity") || extra.contains("nextval")
        })
        .map(|r| r.get::<String, _>("name"));

    // The type without its modifier, which is what a filter value is cast to — `character varying`
    // rather than `character varying(255)`, since a cast that carries the length would truncate
    // rather than compare.
    let column_types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|r| (r.get::<String, _>("name"), r.get::<String, _>("base_type")))
        .collect();
    let (where_clause, binds) = build_where(&query.filters, &column_types)?;

    let count_sql = format!("SELECT COUNT(*) FROM {qualified}{where_clause}");
    let mut count_query = sqlx::query_scalar(sqlx::AssertSqlSafe(count_sql));
    for value in &binds {
        count_query = count_query.bind(value.as_str());
    }
    let total: i64 = count_query.fetch_one(pool).await.map_err(map_error)?;

    // The ceiling is the largest page size the grid offers; see `mysql::table_data`.
    let page_size = query.page_size.clamp(1, 5000);
    let offset = query.page.max(0).saturating_mul(page_size);
    let order_by = query
        .sort_column
        .as_deref()
        .filter(|c| columns.iter().any(|existing| existing == c))
        .map(|c| {
            format!(
                " ORDER BY {} {}",
                quote_ident(c),
                if query.sort_desc { "DESC" } else { "ASC" }
            )
        })
        .unwrap_or_default();
    // Named one by one rather than `SELECT *`, so that a column of a type with no decoder can be
    // asked for as text — see {@link is_decodable}. The alias keeps the name the cast would
    // otherwise replace with `text`.
    let select_list = column_rows
        .iter()
        .map(|r| {
            let name = r.get::<String, _>("name");
            let column = quote_ident(&name);
            if is_decodable(&r.get::<String, _>("data_type")) {
                column
            } else {
                format!("{column}::text AS {column}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let data_sql = format!(
        "SELECT {select_list} FROM {qualified}{where_clause}{order_by} LIMIT {page_size} OFFSET {offset}"
    );
    let mut data_query = sqlx::query(sqlx::AssertSqlSafe(data_sql));
    for value in &binds {
        data_query = data_query.bind(value.as_str());
    }
    let rows = data_query.fetch_all(pool).await.map_err(map_error)?;

    Ok(TablePage {
        columns,
        column_meta,
        primary_key,
        auto_increment_column,
        rows: rows.iter().map(row_to_json).collect(),
        total,
    })
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
mod tests {
    use super::{build_where, is_decodable, map_error, qualify, quote_ident, resolve, Filter};
    use super::{key_predicate, placeholder};
    use serde_json::{Map, Value};

    /// A placeholder carries the column's own type with it.
    ///
    /// Every value the frontend sends is text, so without the cast PostgreSQL sees `$1` as
    /// `unknown` and refuses to compare it to an `integer` or a `timestamptz` — the update fails
    /// on a type error rather than on anything the user did.
    #[test]
    fn a_placeholder_is_cast_to_the_column_it_stands_for() {
        let mut types = BTreeMap::new();
        types.insert("age".to_string(), "integer".to_string());
        types.insert("tags".to_string(), "text[]".to_string());

        assert_eq!(placeholder(1, "age", &types), "$1::integer");
        assert_eq!(placeholder(2, "tags", &types), "$2::text[]");
        // A column that is not in the catalogue does not exist; the statement will say so, and
        // saying so is better than casting to a type invented here.
        assert_eq!(placeholder(3, "nope", &types), "$3");
    }

    /// `IS NOT DISTINCT FROM`, not `=`.
    ///
    /// A key column that is itself NULL compares to NULL as NULL under `=` — which is not true —
    /// so a row with a NULL in its key could never be found again after being edited once.
    #[test]
    fn a_key_predicate_can_match_a_null_key() {
        let mut types = BTreeMap::new();
        types.insert("id".to_string(), "integer".to_string());
        types.insert("region".to_string(), "text".to_string());

        let mut key = Map::new();
        key.insert("id".to_string(), Value::String("7".to_string()));
        assert_eq!(
            key_predicate(&key, &types, 1),
            r#""id" IS NOT DISTINCT FROM $1::integer"#,
        );

        // A composite key, numbered on from where the caller left off — the placeholders before
        // it belong to the columns being written.
        key.insert("region".to_string(), Value::Null);
        assert_eq!(
            key_predicate(&key, &types, 4),
            r#""id" IS NOT DISTINCT FROM $4::integer AND "region" IS NOT DISTINCT FROM $5::text"#,
        );
    }

    use std::collections::BTreeMap;

    fn filter(column: &str, operator: &str, value: Option<&str>) -> Filter {
        Filter {
            column: column.to_string(),
            operator: operator.to_string(),
            value: value.map(str::to_string),
        }
    }

    /// The table the filters below are written against: what a column is, as `format_type` spells
    /// it without its modifier.
    fn columns() -> BTreeMap<String, String> {
        [("id", "integer"), ("name", "text")]
            .into_iter()
            .map(|(name, base_type)| (name.to_string(), base_type.to_string()))
            .collect()
    }

    #[test]
    fn quotes_identifiers_the_way_postgres_does() {
        assert_eq!(quote_ident("users"), "\"users\"");
        assert_eq!(quote_ident("od\"d"), "\"od\"\"d\"");
    }

    /// A table of `public` reads as its bare name, which is what makes a database that uses only
    /// that schema look exactly as it did under MySQL.
    #[test]
    fn names_public_tables_without_a_prefix() {
        assert_eq!(qualify("public", "users"), "users");
        assert_eq!(qualify("sales", "orders"), "sales.orders");
    }

    /// The point of the quoting: a name holding the separator still comes back as itself.
    #[test]
    fn qualifying_survives_a_round_trip() {
        for (schema, table) in [
            ("public", "users"),
            ("sales", "orders"),
            ("public", "odd.name"),
            ("odd.schema", "t"),
            ("public", "quo\"ted"),
        ] {
            let qualified = qualify(schema, table);
            assert_eq!(
                resolve(&qualified),
                (schema.to_string(), table.to_string()),
                "round trip of {qualified}"
            );
        }
    }

    /// Placeholders are numbered rather than positional, so every value has to line up with the
    /// `$n` that was minted for it.
    #[test]
    fn numbers_placeholders_in_order() {
        let filters = vec![
            filter("id", "eq", Some("1")),
            filter("name", "contains", Some("ann")),
        ];
        let (clause, binds) = build_where(&filters, &columns()).unwrap();
        assert_eq!(
            clause,
            " WHERE \"id\"::text = $1 AND \"name\"::text LIKE $2"
        );
        assert_eq!(binds, ["1", "%ann%"]);
    }

    /// A list operator mints one placeholder per item, and the numbering carries on past it.
    #[test]
    fn numbers_a_list_and_what_follows_it() {
        let filters = vec![
            filter("id", "in", Some("1,2,3")),
            filter("name", "eq", Some("x")),
        ];
        let (clause, binds) = build_where(&filters, &columns()).unwrap();
        assert_eq!(
            clause,
            " WHERE \"id\"::text IN ($1, $2, $3) AND \"name\"::text = $4"
        );
        assert_eq!(binds, ["1", "2", "3", "x"]);
    }

    /// The ordering operators compare the column itself, not its text: `>` on text would put 10
    /// before 9. The value is cast instead — without that the server is asked to compare an
    /// `integer` with the `text` sqlx names the bound parameter, and refuses.
    #[test]
    fn compares_ranges_on_the_column_with_the_value_cast_to_its_type() {
        let (clause, _) = build_where(&[filter("id", "gt", Some("5"))], &columns()).unwrap();
        assert_eq!(clause, " WHERE \"id\" > CAST($1 AS integer)");

        let (clause, binds) =
            build_where(&[filter("id", "between", Some("5,9"))], &columns()).unwrap();
        assert_eq!(
            clause,
            " WHERE \"id\" BETWEEN CAST($1 AS integer) AND CAST($2 AS integer)"
        );
        assert_eq!(binds, ["5", "9"]);
    }

    /// An array's `[]` comes after the type's modifier, so a decoder is looked for under the whole
    /// spelling: `character varying(255)[]` is an array, and asking sqlx to decode it as a string
    /// would show NULL where a value is.
    #[test]
    fn reads_an_array_of_a_type_that_takes_a_modifier_as_text() {
        assert!(!is_decodable("character varying(255)[]"));
        assert!(!is_decodable("numeric(10,2)[]"));
        assert!(!is_decodable("text[]"));
        assert!(is_decodable("character varying(255)"));
        assert!(is_decodable("timestamp without time zone"));
    }

    /// The number types are read as text although decoders for them exist: `NaN` and the infinities
    /// are values every one of these columns may hold and none of the decoders can carry — Decimal
    /// refuses them, and JSON has no way to write them — so they would arrive as null and read in
    /// the grid as NULL.
    #[test]
    fn reads_the_numbers_that_can_be_nan_as_text() {
        assert!(!is_decodable("numeric"));
        assert!(!is_decodable("numeric(10,2)"));
        assert!(!is_decodable("real"));
        assert!(!is_decodable("double precision"));
        // The integers cannot be NaN, and still decode.
        assert!(is_decodable("integer"));
        assert!(is_decodable("bigint"));
    }

    /// A row whose operator wants a value it wasn't given is dropped rather than matched
    /// literally — the bar's opening `id =` row must not become a condition before it is typed in.
    #[test]
    fn drops_a_list_row_with_nothing_in_it() {
        let (clause, binds) = build_where(&[filter("id", "in", Some(" "))], &columns()).unwrap();
        assert!(clause.is_empty());
        assert!(binds.is_empty());
    }

    /// A column the table does not have never reaches the SQL text.
    #[test]
    fn refuses_a_column_the_table_does_not_have() {
        assert!(build_where(&[filter("nope", "eq", Some("1"))], &columns()).is_err());
    }

    #[test]
    fn postgres_tells_a_lost_connection_from_a_server_error() {
        let eof = sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "expected to read 4 bytes, got 0 bytes at EOF",
        ));
        assert_eq!(map_error(eof).code, "error.connectionLost");
        assert_eq!(
            map_error(sqlx::Error::PoolTimedOut).code,
            "error.connectionLost"
        );
        assert_eq!(map_error(sqlx::Error::RowNotFound).code, "error.postgres");
    }
}
