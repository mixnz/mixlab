use super::filters::{escape_like, split_list};
use crate::error::AppError;
use crate::modules::db::models::ServerInfo;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlRow, MySqlSslMode};
use sqlx::{Column, MySqlPool, Row, TypeInfo};
use std::collections::BTreeMap;

/// This error is a broken pipe, not the server refusing.
///
/// `Io` is where the message "expected to read N bytes, got 0 bytes at EOF" comes out — sqlx builds
/// it in `net/socket/buffered.rs` when the socket closes midway. The two pool variants come along
/// because while the tunnel is being reopened, what the user hits is not a broken socket but an
/// `acquire` with no connection to hand out.
pub(super) fn lost_connection(e: &sqlx::Error) -> bool {
    matches!(
        e,
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed
    )
}

/// What every MySQL command using a connection uses instead of
/// `err!("error.mysql", message = e)`.
pub(super) fn map_error(e: sqlx::Error) -> AppError {
    if lost_connection(&e) {
        err!("error.connectionLost")
    } else {
        err!("error.mysql", message = e)
    }
}

pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: Option<&str>,
    use_ssl: Option<bool>,
) -> Result<MySqlPool, AppError> {
    let mut opts = MySqlConnectOptions::new()
        .host(host)
        .port(port)
        .username(username)
        .ssl_mode(if use_ssl == Some(false) {
            MySqlSslMode::Disabled
        } else {
            MySqlSslMode::Preferred
        });
    // sqlx only skips the auth-response scramble (as required for a truly
    // passwordless MySQL account) when the options' password is `None`.
    // Calling `.password("")` sets `Some("")`, which still runs SHA1("")
    // through the scramble algorithm and sends a bogus 20-byte response —
    // rejected by the server as "using password: YES" even though the user
    // typed nothing.
    if !password.is_empty() {
        opts = opts.password(password);
    }
    if let Some(db) = database.filter(|d| !d.is_empty()) {
        opts = opts.database(db);
    }
    MySqlPoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .map_err(connect_error)
}

/// What a failed `connect` says, which is not what a failed query says.
///
/// It does not go through `map_error`: a failure here is "could not connect", never "the
/// connection was lost", and the real reason (connection refused, a host name that does not
/// resolve) is in `message`.
///
/// `Configuration` is the one variant worth its own sentence. sqlx-mysql raises it when the server
/// asks for full authentication — `caching_sha2_password` on a cache miss — over a link that is
/// neither encrypted nor able to encrypt the password with the server's RSA key. The `mysql-rsa`
/// feature in `Cargo.toml` is what keeps that second path open, so this should now be unreachable;
/// it is mapped anyway because the day that feature is dropped from the dependency line, what the
/// user reads is a sentence about a Cargo feature flag with the way out — tick Use SSL — nowhere
/// in it.
fn connect_error(e: sqlx::Error) -> AppError {
    match e {
        sqlx::Error::Configuration(_) => err!("error.mysqlSecureAuthRequired"),
        e => err!("error.mysql", message = e),
    }
}

pub async fn query(
    pool: &MySqlPool,
    sql: &str,
    database: Option<&str>,
) -> Result<Vec<Map<String, Value>>, AppError> {
    // Querying a &Pool directly requires 'static query text (sqlx's Executor
    // impl for &Pool boxes the acquire+execute future); acquiring a connection
    // first lets us run borrowed, non-'static SQL text instead.
    let mut conn = pool.acquire().await.map_err(map_error)?;
    if let Some(db) = database.filter(|d| !d.is_empty()) {
        let use_sql = format!("USE {}", quote_ident(db));
        // Text protocol: `USE` is one of the statements MySQL will not accept as a prepared one.
        sqlx::raw_sql(sqlx::AssertSqlSafe(use_sql))
            .execute(&mut *conn)
            .await
            .map_err(map_error)?;
    }
    // AssertSqlSafe opts out of sqlx's SQL-injection speed bump: this client
    // runs arbitrary, user-authored SQL by design, not app-embedded queries.
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await
        .map_err(map_error)?;

    Ok(rows.iter().map(row_to_json).collect())
}

/// Backtick-quotes an identifier (database/table name) for interpolation into
/// SQL text, doubling embedded backticks — MySQL's own escaping rule.
pub(super) fn quote_ident(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}

/// The server's own id for this session, which is what `KILL QUERY` names.
pub async fn thread_id(conn: &mut sqlx::MySqlConnection) -> Result<u64, AppError> {
    sqlx::query_scalar("SELECT CONNECTION_ID()")
        .fetch_one(conn)
        .await
        .map_err(map_error)
}

/// Asks the server to stop whatever session `thread_id` is running.
///
/// `KILL QUERY` rather than `KILL`: it ends the statement and leaves the session itself open, so
/// what comes back is an error the script can report against the statement it stopped, rather than
/// a dropped connection reported as "connection lost" against the whole run.
///
/// Runs on a connection of its own out of the same pool, since the one being killed is busy. A
/// server that no longer has that session (the query finished first) answers `1094`, which is not
/// a failure worth showing: the user asked for it to stop, and it has.
///
/// By number and not by the message. `"Unknown thread id"` is the English text of one server
/// build: a server running under another `lc_messages`, or a MariaDB wording it differently, says
/// the same thing in words this would not recognise — and the user would be shown an error for a
/// query that had already finished. `mysql_script::problem` reads numbers for the same reason.
pub async fn kill_query(pool: &MySqlPool, thread_id: u64) -> Result<(), AppError> {
    match sqlx::query(sqlx::AssertSqlSafe(format!("KILL QUERY {thread_id}")))
        .execute(pool)
        .await
    {
        Ok(_) => Ok(()),
        Err(e) if error_number(&e) == ER_NO_SUCH_THREAD => Ok(()),
        Err(e) => Err(map_error(e)),
    }
}

/// "Unknown thread id" — the session named is not on the server any more.
const ER_NO_SUCH_THREAD: u16 = 1094;

/// The server's own number for an error, or 0 when it did not come from the server.
fn error_number(error: &sqlx::Error) -> u16 {
    error
        .as_database_error()
        .and_then(|db| db.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>())
        .map_or(0, |e| e.number())
}

/// Reads what the header shows about the server. The compile variables are the only place a
/// MySQL connection names its machine: the platform it was built for, and the architecture it
/// was built for — "Linux x86_64".
pub async fn server_info(pool: &MySqlPool) -> Result<ServerInfo, AppError> {
    let mut conn = pool.acquire().await.map_err(map_error)?;
    let rows = sqlx::query(
        "SHOW VARIABLES WHERE Variable_name IN \
         ('version', 'version_compile_os', 'version_compile_machine')",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(map_error)?;

    let mut version = String::new();
    let mut platform = String::new();
    let mut machine = String::new();
    for row in &rows {
        let name: String = row.get("Variable_name");
        let value: String = row.get("Value");
        match name.as_str() {
            "version" => version = value,
            "version_compile_os" => platform = value,
            "version_compile_machine" => machine = value,
            _ => {}
        }
    }

    // Either half can be missing — an older server has no `version_compile_machine` — and
    // whichever is left should still reach the header on its own.
    let os = [platform.trim(), machine.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(ServerInfo { version, os })
}

/// Whether a server that speaks the MySQL protocol is in fact MariaDB.
///
/// The two share a wire protocol and nearly all of their SQL, but they do not describe a column's
/// DEFAULT the same way, and MariaDB has none of the histogram tables an 8.0 `mysqldump` reads by
/// default — so which one is on the other end has to be known before either is touched. The
/// version string is the only place MariaDB says so: `11.8.8-MariaDB-ubu2404`.
pub fn is_mariadb(version: &str) -> bool {
    version.to_ascii_lowercase().contains("mariadb")
}

/// Asks the server which of the two it is.
///
/// Read once, when the connection is opened: it cannot change under a live connection, and it is
/// needed on paths — reading a table's structure — that have no other reason to ask the server
/// about itself. A server that will not answer is taken for MySQL, which is what this client
/// assumed before it asked at all.
pub async fn detect_mariadb(pool: &MySqlPool) -> bool {
    sqlx::query_scalar::<_, String>("SELECT VERSION()")
        .fetch_one(pool)
        .await
        .is_ok_and(|version| is_mariadb(&version))
}

pub async fn list_databases(pool: &MySqlPool) -> Result<Vec<String>, AppError> {
    let mut conn = pool.acquire().await.map_err(map_error)?;
    let rows = sqlx::query("SHOW DATABASES")
        .fetch_all(&mut *conn)
        .await
        .map_err(map_error)?;
    Ok(rows.iter().map(|r| r.get::<String, _>(0)).collect())
}

pub async fn list_tables(pool: &MySqlPool, database: &str) -> Result<Vec<String>, AppError> {
    let mut conn = pool.acquire().await.map_err(map_error)?;
    let sql = format!("SHOW TABLES FROM {}", quote_ident(database));
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await
        .map_err(map_error)?;
    Ok(rows.iter().map(|r| r.get::<String, _>(0)).collect())
}

/// The row a foreign key column points at: what it references, not what it is declared as.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKey {
    pub table: String,
    pub column: String,
}

/// What `SHOW COLUMNS` knows about one column beyond its name — everything a new row has to
/// respect: the type it is written as, whether it may be left NULL, and what it falls back to
/// when an INSERT leaves it out.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    /// The declared type as MySQL spells it, e.g. `varchar(255)` or `int unsigned`.
    pub data_type: String,
    pub nullable: bool,
    /// The column's DEFAULT, or `None` when it has none. An expression default
    /// (`CURRENT_TIMESTAMP`, `(uuid())`) is reported here too, unquoted and by itself
    /// indistinguishable from a literal — `extra` is what tells the two apart.
    pub default_value: Option<String>,
    /// `SHOW COLUMNS`' Extra: `auto_increment`, `DEFAULT_GENERATED`, `STORED GENERATED`, ...
    pub extra: String,
    /// What this column references, when it is part of a foreign key. A column in a composite
    /// foreign key reports its own half of it; a column under more than one constraint reports
    /// the first one `information_schema` lists.
    pub foreign_key: Option<ForeignKey>,
}

/// Which columns of `table` are foreign keys, and what each one points at.
///
/// Read from `information_schema`, which only ever shows the constraints the connected user has
/// privileges on — so an empty result means "none visible to you", not necessarily "none". A
/// failure here is swallowed by the caller for the same reason: the FK markers are decoration on
/// top of the grid, and losing them must not cost the user the rows themselves.
///
/// A key may point into another schema, and those are left out: what reaches the frontend is a
/// bare table name, opened inside the database the grid is already on. Reported, such a key would
/// name a table that database does not have — an error where a row was promised, and a marker
/// claiming a relationship to the wrong table of the same name where it happens to exist.
async fn foreign_keys(
    conn: &mut sqlx::MySqlConnection,
    database: &str,
    table: &str,
) -> Result<BTreeMap<String, ForeignKey>, AppError> {
    let rows = sqlx::query(
        "SELECT COLUMN_NAME, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME
         FROM information_schema.KEY_COLUMN_USAGE
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND REFERENCED_TABLE_NAME IS NOT NULL
           AND REFERENCED_TABLE_SCHEMA = TABLE_SCHEMA
         ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
    )
    .bind(database)
    .bind(table)
    .fetch_all(&mut *conn)
    .await
    .map_err(map_error)?;

    let mut keys = BTreeMap::new();
    for row in &rows {
        keys.entry(row.get::<String, _>("COLUMN_NAME"))
            .or_insert_with(|| ForeignKey {
                table: row.get::<String, _>("REFERENCED_TABLE_NAME"),
                column: row.get::<String, _>("REFERENCED_COLUMN_NAME"),
            });
    }
    Ok(keys)
}

/// One condition on the rows a page is cut out of — the grid's filter bar sends a list of these,
/// and they are ANDed together. `value` carries whatever the user typed, as text: the operator is
/// what says how to read it (a single value, a comma-separated list, a pair), and operators like
/// `isNull` ignore it entirely.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub column: String,
    pub operator: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// Turns the filter rows into a WHERE clause (leading space included, empty when nothing filters)
/// and the values to bind into its placeholders, in order.
///
/// Every value reaches MySQL as a bound parameter, never as SQL text — the column name is the one
/// part that has to be interpolated, which is why it is checked against the table's own columns
/// first. A row whose operator wants a value it wasn't given is dropped rather than matched
/// literally: the bar's opening `id =` row must not become `WHERE id = ''` before anything is
/// typed into it.
fn build_where(filters: &[Filter], columns: &[String]) -> Result<(String, Vec<String>), AppError> {
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    for filter in filters {
        if !columns.iter().any(|c| c == &filter.column) {
            return Err(err!("error.unknownFilterColumn", column = &filter.column));
        }
        let col = quote_ident(&filter.column);
        let value = filter.value.as_deref().unwrap_or("");
        let operator = filter.operator.as_str();

        let (clause, mut values): (String, Vec<String>) = match operator {
            "eq" => (format!("{col} = ?"), vec![value.to_string()]),
            "ne" => (format!("{col} <> ?"), vec![value.to_string()]),
            "gt" => (format!("{col} > ?"), vec![value.to_string()]),
            "gte" => (format!("{col} >= ?"), vec![value.to_string()]),
            "lt" => (format!("{col} < ?"), vec![value.to_string()]),
            "lte" => (format!("{col} <= ?"), vec![value.to_string()]),
            "contains" => (
                format!("{col} LIKE ?"),
                vec![format!("%{}%", escape_like(value))],
            ),
            "notContains" => (
                format!("{col} NOT LIKE ?"),
                vec![format!("%{}%", escape_like(value))],
            ),
            "startsWith" => (
                format!("{col} LIKE ?"),
                vec![format!("{}%", escape_like(value))],
            ),
            "endsWith" => (
                format!("{col} LIKE ?"),
                vec![format!("%{}", escape_like(value))],
            ),
            "like" => (format!("{col} LIKE ?"), vec![value.to_string()]),
            "notLike" => (format!("{col} NOT LIKE ?"), vec![value.to_string()]),
            "regexp" => (format!("{col} REGEXP ?"), vec![value.to_string()]),
            "notRegexp" => (format!("{col} NOT REGEXP ?"), vec![value.to_string()]),
            "in" | "notIn" => {
                let items = split_list(value);
                if items.is_empty() {
                    continue;
                }
                let placeholders = vec!["?"; items.len()].join(", ");
                let sql_op = if operator == "in" { "IN" } else { "NOT IN" };
                (format!("{col} {sql_op} ({placeholders})"), items)
            }
            "between" | "notBetween" => {
                let items = split_list(value);
                // Two bounds or nothing — one of them alone says nothing about a range.
                if items.len() < 2 {
                    continue;
                }
                let sql_op = if operator == "between" {
                    "BETWEEN"
                } else {
                    "NOT BETWEEN"
                };
                (format!("{col} {sql_op} ? AND ?"), items[..2].to_vec())
            }
            "isNull" => (format!("{col} IS NULL"), Vec::new()),
            "isNotNull" => (format!("{col} IS NOT NULL"), Vec::new()),
            "isEmpty" => (format!("{col} = ''"), Vec::new()),
            "isNotEmpty" => (format!("{col} <> ''"), Vec::new()),
            other => return Err(err!("error.unknownFilterOperator", operator = other)),
        };

        clauses.push(clause);
        binds.append(&mut values);
    }

    if clauses.is_empty() {
        return Ok((String::new(), binds));
    }
    Ok((format!(" WHERE {}", clauses.join(" AND ")), binds))
}

/// Which page of a table is wanted, and what it is cut out of: the page itself, the order the
/// whole table is put in first, and the filters that narrow it down before either.
///
/// One value rather than five loose arguments — `page` and `page_size` are both `i64` and
/// `sort_desc` is a bare `bool`, so a positional call is one transposition away from silently
/// reading the wrong page in the wrong order.
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
    /// Keyed by column name; `columns` is what carries their order.
    pub column_meta: BTreeMap<String, ColumnMeta>,
    pub primary_key: Vec<String>,
    /// The AUTO_INCREMENT column, if the table has one. Only such a table has a
    /// counter worth offering to reset after a delete.
    pub auto_increment_column: Option<String>,
    pub rows: Vec<Map<String, Value>>,
    pub total: i64,
}

/// Reads one page of a table. `query.sort_column` orders the whole table before the page is cut
/// out of it — sorting is the server's job, not the page's, or each page would only be sorted
/// within itself. It is ignored unless it names a real column of this table, which is also what
/// keeps it out of the SQL text unchecked.
///
/// `query.filters` narrows the table down first, ANDed together; `total` counts what is left after
/// them, so the pager measures the filtered table rather than the whole one.
/// `mariadb` says which of the two servers answered: only MySQL marks an expression default in
/// `Extra`, and a row started off without that marker is written with the expression's text in it —
/// see [`super::mysql_structure::mariadb_expression_defaults`].
pub async fn table_data(
    pool: &MySqlPool,
    mariadb: bool,
    database: &str,
    table: &str,
    query: &PageQuery,
) -> Result<TablePage, AppError> {
    let mut conn = pool.acquire().await.map_err(map_error)?;
    let qualified = format!("{}.{}", quote_ident(database), quote_ident(table));

    // SHOW COLUMNS gives us the column list (in table order) even when the
    // table has zero rows, which a `SELECT * ... LIMIT n` result set can't.
    let columns_sql = format!("SHOW COLUMNS FROM {qualified}");
    let column_rows = sqlx::query(sqlx::AssertSqlSafe(columns_sql))
        .fetch_all(&mut *conn)
        .await
        .map_err(map_error)?;
    let columns: Vec<String> = column_rows
        .iter()
        .map(|r| r.get::<String, _>("Field"))
        .collect();
    let mut foreign_keys = foreign_keys(&mut conn, database, table)
        .await
        .unwrap_or_default();
    // Swallowed like the foreign keys above, and for the same reason: a second reading of the same
    // table's metadata that fails where the first succeeded is not worth the rows themselves.
    let expression_defaults = if mariadb {
        super::mysql_structure::mariadb_expression_defaults(&mut conn, database, table)
            .await
            .unwrap_or_default()
    } else {
        Default::default()
    };

    let column_meta: BTreeMap<String, ColumnMeta> = column_rows
        .iter()
        .map(|r| {
            let field = r.get::<String, _>("Field");
            let foreign_key = foreign_keys.remove(&field);
            let mut extra = r.get::<String, _>("Extra");
            // The marker MariaDB does not write, put where MySQL would have written it, so that
            // everything downstream reads one vocabulary.
            if expression_defaults.contains(&field) {
                if !extra.is_empty() {
                    extra.push(' ');
                }
                extra.push_str("DEFAULT_GENERATED");
            }
            (
                field,
                ColumnMeta {
                    data_type: r.get::<String, _>("Type"),
                    nullable: r.get::<String, _>("Null") == "YES",
                    default_value: r.try_get::<Option<String>, _>("Default").unwrap_or(None),
                    extra,
                    foreign_key,
                },
            )
        })
        .collect();
    let primary_key: Vec<String> = column_rows
        .iter()
        .filter(|r| r.get::<String, _>("Key") == "PRI")
        .map(|r| r.get::<String, _>("Field"))
        .collect();
    let auto_increment_column: Option<String> = column_rows
        .iter()
        .find(|r| r.get::<String, _>("Extra").contains("auto_increment"))
        .map(|r| r.get::<String, _>("Field"));

    let (where_clause, binds) = build_where(&query.filters, &columns)?;

    let count_sql = format!("SELECT COUNT(*) FROM {qualified}{where_clause}");
    let mut count_query = sqlx::query_scalar(sqlx::AssertSqlSafe(count_sql));
    for value in &binds {
        count_query = count_query.bind(value.as_str());
    }
    let total: i64 = count_query.fetch_one(&mut *conn).await.map_err(map_error)?;

    // The ceiling is the largest page size the grid offers. Anything lower silently shortens the
    // page while the grid still counts its pages by the size it asked for, putting the rows past
    // the ceiling out of reach.
    let page_size = query.page_size.clamp(1, 5000);
    let offset = query.page.max(0).saturating_mul(page_size);
    // Only a column the table actually has can reach the SQL text, so a stale or made-up sort
    // column is dropped rather than turned into an error the user can't act on.
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
    let data_sql = format!(
        "SELECT * FROM {qualified}{where_clause}{order_by} LIMIT {page_size} OFFSET {offset}"
    );
    let mut data_query = sqlx::query(sqlx::AssertSqlSafe(data_sql));
    for value in &binds {
        data_query = data_query.bind(value.as_str());
    }
    let rows = data_query.fetch_all(&mut *conn).await.map_err(map_error)?;

    Ok(TablePage {
        columns,
        column_meta,
        primary_key,
        auto_increment_column,
        rows: rows.iter().map(row_to_json).collect(),
        total,
    })
}

/// Binds a JSON value (always `Null` or `String` — the frontend only ever
/// sends edited text or an explicit null) as an `Option<&str>`, letting MySQL
/// coerce the text to the column's real type on write.
fn bind_value<'q>(
    query: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    value: &'q Value,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
    match value {
        Value::String(s) => query.bind(Some(s.as_str())),
        _ => query.bind(None::<&str>),
    }
}

/// Updates exactly one row, identified by `key` (primary key columns, or —
/// when a table has none — every column as a fallback). Runs inside a
/// transaction that first verifies the key predicate matches exactly one
/// row, so the no-PK fallback can't silently clobber a duplicate row.
pub async fn update_row(
    pool: &MySqlPool,
    database: &str,
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

    let qualified = format!("{}.{}", quote_ident(database), quote_ident(table));
    let set_clause = updates
        .keys()
        .map(|c| format!("{} = ?", quote_ident(c)))
        .collect::<Vec<_>>()
        .join(", ");
    // `<=>` is MySQL's NULL-safe equality operator, so a key column that is
    // itself NULL still matches (plain `=` never matches NULL).
    let where_clause = key
        .keys()
        .map(|c| format!("{} <=> ?", quote_ident(c)))
        .collect::<Vec<_>>()
        .join(" AND ");

    let mut tx = pool.begin().await.map_err(map_error)?;

    let count_sql = format!("SELECT COUNT(*) FROM {qualified} WHERE {where_clause}");
    let mut count_query = sqlx::query(sqlx::AssertSqlSafe(count_sql));
    for v in key.values() {
        count_query = bind_value(count_query, v);
    }
    let matched: i64 = count_query
        .fetch_one(&mut *tx)
        .await
        .map_err(map_error)?
        .get::<i64, _>(0);
    if matched != 1 {
        tx.rollback().await.map_err(map_error)?;
        return Err(err!("error.rowsMatched", matched = matched));
    }

    let update_sql = format!("UPDATE {qualified} SET {set_clause} WHERE {where_clause}");
    let mut update_query = sqlx::query(sqlx::AssertSqlSafe(update_sql));
    for v in updates.values() {
        update_query = bind_value(update_query, v);
    }
    for v in key.values() {
        update_query = bind_value(update_query, v);
    }
    update_query.execute(&mut *tx).await.map_err(map_error)?;

    tx.commit().await.map_err(map_error)?;
    Ok(())
}

/// Inserts `rows`, one map per new row, all in a single transaction: if any one of them is
/// rejected, none of them land.
///
/// A row only carries the columns it has something to say about — a column left out of the map
/// is left out of that row's INSERT too, so the table's own DEFAULT (or AUTO_INCREMENT, or a
/// generated expression) is what fills it. That is also why each row is its own statement
/// rather than one multi-VALUES INSERT: rows may fill in different sets of columns, and the
/// error a rejected row produces can then say which row it was.
pub async fn insert_rows(
    pool: &MySqlPool,
    database: &str,
    table: &str,
    rows: &[Map<String, Value>],
) -> Result<(), AppError> {
    if rows.is_empty() {
        return Ok(());
    }

    let qualified = format!("{}.{}", quote_ident(database), quote_ident(table));
    let mut tx = pool.begin().await.map_err(map_error)?;

    for (i, row) in rows.iter().enumerate() {
        // `() VALUES ()` is MySQL's way of spelling "a row that is nothing but defaults".
        let sql = if row.is_empty() {
            format!("INSERT INTO {qualified} () VALUES ()")
        } else {
            let columns = row
                .keys()
                .map(|c| quote_ident(c))
                .collect::<Vec<_>>()
                .join(", ");
            let placeholders = vec!["?"; row.len()].join(", ");
            format!("INSERT INTO {qualified} ({columns}) VALUES ({placeholders})")
        };
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for v in row.values() {
            query = bind_value(query, v);
        }
        if let Err(e) = query.execute(&mut *tx).await {
            tx.rollback().await.map_err(map_error)?;
            return Err(err!("error.rowFailed", index = i + 1).caused_by(map_error(e)));
        }
    }

    tx.commit().await.map_err(map_error)?;
    Ok(())
}

/// Deletes rows identified by `keys` (each map is one row's primary key columns,
/// or — when a table has none — every column as a fallback), or every row in the
/// table when `all` is set. `reset_auto_increment` afterwards puts the table's
/// AUTO_INCREMENT counter back to 1, so the next insert starts from 1 again.
///
/// The deletes run in one transaction: if any of them fails, none of them land.
/// Each per-row DELETE carries `LIMIT 1` so that the no-primary-key fallback can
/// only ever remove the one row the user selected, never its duplicates.
pub async fn delete_rows(
    pool: &MySqlPool,
    database: &str,
    table: &str,
    keys: &[Map<String, Value>],
    all: bool,
    reset_auto_increment: bool,
) -> Result<(), AppError> {
    if !all && keys.is_empty() {
        return Ok(());
    }

    let qualified = format!("{}.{}", quote_ident(database), quote_ident(table));
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
            // `<=>` is MySQL's NULL-safe equality operator, so a key column that
            // is itself NULL still matches (plain `=` never matches NULL).
            let where_clause = key
                .keys()
                .map(|c| format!("{} <=> ?", quote_ident(c)))
                .collect::<Vec<_>>()
                .join(" AND ");
            let sql = format!("DELETE FROM {qualified} WHERE {where_clause} LIMIT 1");
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
            for v in key.values() {
                query = bind_value(query, v);
            }
            query.execute(&mut *tx).await.map_err(map_error)?;
        }
    }

    tx.commit().await.map_err(map_error)?;

    // Outside the transaction on purpose: ALTER TABLE forces an implicit commit
    // in MySQL, so running it inside would end the transaction behind our back.
    if reset_auto_increment {
        let sql = format!("ALTER TABLE {qualified} AUTO_INCREMENT = 1");
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(pool)
            .await
            .map_err(map_error)?;
    }

    Ok(())
}

fn row_to_json(row: &MySqlRow) -> Map<String, Value> {
    let mut obj = Map::new();
    for (i, col) in row.columns().iter().enumerate() {
        obj.insert(col.name().to_string(), column_value(row, i));
    }
    obj
}

// sqlx has no single "decode as any type" API, so we try common Rust types in
// order of likelihood and fall back to a lossy string/base64 representation.
//
// i64/u64 are tried before bool: sqlx-mysql's `bool::compatible()` accepts
// *any* MySQL integer column type (TINYINT, INT, BIGINT, ...), not just
// TINYINT(1), because the wire protocol doesn't distinguish them. Checking
// bool first would decode every non-zero id/FK integer column as `true`.
pub(super) fn column_value(row: &MySqlRow, i: usize) -> Value {
    if let Ok(v) = row.try_get::<Option<i64>, _>(i) {
        return v.map(Value::from).unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<u64>, _>(i) {
        return v.map(Value::from).unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<bool>, _>(i) {
        return v.map(Value::from).unwrap_or(Value::Null);
    }
    // DECIMAL/NUMERIC is explicitly excluded from f64 by sqlx (different
    // rounding semantics) and needs its own decoder. Rendered as a string to
    // preserve exact precision instead of going through a lossy f64 roundtrip.
    if let Ok(v) = row.try_get::<Option<rust_decimal::Decimal>, _>(i) {
        return v
            .map(|d| Value::String(d.to_string()))
            .unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(i) {
        return v
            .and_then(|n| serde_json::Number::from_f64(n).map(Value::Number))
            .unwrap_or(Value::Null);
    }
    // `NaiveDateTime` only declares itself compatible with MySQL's DATETIME
    // column type, not TIMESTAMP (a distinct wire type) — so TIMESTAMP
    // columns like created_at/updated_at would fall through to Null. Decode
    // as `DateTime<Utc>` instead, which sqlx-mysql accepts for both.
    if let Ok(v) = row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(i) {
        return v
            .map(|d| Value::String(d.naive_utc().to_string()))
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
    // `Json<T>::compatible()` also matches plain VARCHAR/TEXT columns (MySQL
    // has historically transmitted JSON as CHAR), so it's gated on the
    // column's actual reported type name instead of just trying it broadly —
    // otherwise a TEXT column that merely *contains* JSON-encoded text (e.g.
    // PHP's json_encode) would get silently reparsed into a structured value,
    // reordering its keys (this app's serde_json has no `preserve_order`).
    if row.column(i).type_info().name() == "JSON" {
        if let Ok(v) = row.try_get::<Option<sqlx::types::Json<Value>>, _>(i) {
            return v.map(|j| j.0).unwrap_or(Value::Null);
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

/// The parts of this module that decide what SQL is written, rather than what a server answers:
/// how a filter row becomes a condition, and how an identifier is quoted before it is pasted into
/// statement text. Both are what stands between user input and the SQL that reaches MySQL.
#[cfg(test)]
mod tests {
    use super::{build_where, connect_error, lost_connection, map_error, quote_ident, Filter};

    fn filter(column: &str, operator: &str, value: Option<&str>) -> Filter {
        Filter {
            column: column.to_string(),
            operator: operator.to_string(),
            value: value.map(str::to_string),
        }
    }

    fn columns() -> Vec<String> {
        ["id", "name"].iter().map(|c| c.to_string()).collect()
    }

    fn build(filters: &[Filter]) -> (String, Vec<String>) {
        build_where(filters, &columns()).unwrap()
    }

    /// Every value reaches the server as a bound parameter; only the column name is interpolated,
    /// and only after being matched against the table's own columns.
    #[test]
    fn values_are_bound_and_never_written_into_the_sql() {
        let (clause, binds) = build(&[filter("id", "eq", Some("1' OR '1'='1"))]);
        assert_eq!(clause, " WHERE `id` = ?");
        assert_eq!(binds, ["1' OR '1'='1"]);
    }

    #[test]
    fn conditions_are_anded_together() {
        let (clause, binds) = build(&[
            filter("id", "gte", Some("10")),
            filter("name", "ne", Some("bob")),
        ]);
        assert_eq!(clause, " WHERE `id` >= ? AND `name` <> ?");
        assert_eq!(binds, ["10", "bob"]);
    }

    /// The wildcards in a value the user meant literally are escaped, so searching for `50%` finds
    /// the text and not "anything starting with 50".
    #[test]
    fn pattern_operators_escape_the_value_they_wrap() {
        let (clause, binds) = build(&[filter("name", "contains", Some("50%_x"))]);
        assert_eq!(clause, " WHERE `name` LIKE ?");
        assert_eq!(binds, [r"%50\%\_x%"]);

        let (_, binds) = build(&[filter("name", "startsWith", Some("a_b"))]);
        assert_eq!(binds, [r"a\_b%"]);
    }

    /// `like` is the operator for handing MySQL a pattern of one's own, so its value is the one
    /// that is *not* escaped.
    #[test]
    fn like_passes_the_users_own_pattern_through() {
        let (_, binds) = build(&[filter("name", "like", Some("a%b"))]);
        assert_eq!(binds, ["a%b"]);
    }

    #[test]
    fn lists_become_one_placeholder_per_item() {
        let (clause, binds) = build(&[filter("id", "in", Some("1, 2 ,3"))]);
        assert_eq!(clause, " WHERE `id` IN (?, ?, ?)");
        assert_eq!(binds, ["1", "2", "3"]);
    }

    #[test]
    fn between_takes_the_first_two_bounds() {
        let (clause, binds) = build(&[filter("id", "between", Some("1,9,99"))]);
        assert_eq!(clause, " WHERE `id` BETWEEN ? AND ?");
        assert_eq!(binds, ["1", "9"]);
    }

    /// A row whose operator wants a value it wasn't given is dropped rather than matched
    /// literally: the bar's opening `id =` row must not become `WHERE id IN ()`.
    #[test]
    fn a_row_without_enough_of_a_value_is_dropped() {
        assert_eq!(build(&[filter("id", "in", Some(""))]).0, "");
        assert_eq!(build(&[filter("id", "between", Some("1"))]).0, "");
        assert_eq!(build(&[]).0, "");
    }

    #[test]
    fn operators_that_stand_on_their_own_bind_nothing() {
        let (clause, binds) = build(&[
            filter("id", "isNull", None),
            filter("name", "isNotEmpty", None),
        ]);
        assert_eq!(clause, " WHERE `id` IS NULL AND `name` <> ''");
        assert!(binds.is_empty());
    }

    /// The column is the one part that has to be interpolated, so a name the table does not have
    /// is refused outright rather than quoted and sent.
    #[test]
    fn an_unknown_column_or_operator_is_refused() {
        assert!(build_where(
            &[filter("id`; DROP TABLE x; --", "eq", Some("1"))],
            &columns()
        )
        .is_err());
        assert!(build_where(&[filter("id", "sqli", Some("1"))], &columns()).is_err());
    }

    /// MySQL's own escaping rule for an identifier: a backtick inside one is doubled.
    #[test]
    fn identifiers_are_backtick_quoted() {
        assert_eq!(quote_ident("users"), "`users`");
        assert_eq!(quote_ident("we`ird"), "`we``ird`");
    }

    /// `Database(_)` cannot be built here — `sqlx::error::DatabaseError` is a trait and MySQL's
    /// implementation is not public — so the two "the server answered" variants used instead are
    /// `RowNotFound` and `Protocol`. What has to hold is still the same: an error the server
    /// answered with must never be turned into "connection lost", because then a read command
    /// would be rerun for nothing and the user would be told the wrong story about what happened.
    #[test]
    fn only_a_broken_pipe_counts_as_a_lost_connection() {
        let eof = sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "expected to read 4 bytes, got 0 bytes at EOF",
        ));
        assert!(lost_connection(&eof));
        assert!(lost_connection(&sqlx::Error::PoolTimedOut));
        assert!(lost_connection(&sqlx::Error::PoolClosed));

        assert!(!lost_connection(&sqlx::Error::RowNotFound));
        assert!(!lost_connection(&sqlx::Error::Protocol(
            "unexpected packet".into()
        )));
    }

    #[test]
    fn a_lost_connection_gets_its_own_code_and_carries_no_driver_text() {
        let eof = sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "expected to read 4 bytes, got 0 bytes at EOF",
        ));
        let error = map_error(eof);
        assert_eq!(error.code, "error.connectionLost");
        // sqlx's verbatim text here is the library's internal business, not the server speaking —
        // so it does not qualify to be kept verbatim under the convention `error.rs` describes.
        assert!(error.params.is_empty());

        let other = map_error(sqlx::Error::RowNotFound);
        assert_eq!(other.code, "error.mysql");
        assert!(other.params.contains_key("message"));
    }

    /// The failure a server asking for full authentication produces on a link that can neither
    /// encrypt the channel nor encrypt the password. The `mysql-rsa` feature should keep it from
    /// ever being raised — this pins what the user reads if it is raised anyway, because the
    /// driver's own words there name a Cargo feature and not the checkbox that gets them
    /// connected.
    #[test]
    fn an_unauthenticatable_link_names_the_ssl_checkbox_not_the_driver() {
        let error = connect_error(sqlx::Error::Configuration(
            "RSA auth backend disabled; enable feature `mysql-rsa` (or `rsa` if using sqlx-mysql \
             directly) or use TLS."
                .into(),
        ));
        assert_eq!(error.code, "error.mysqlSecureAuthRequired");
        assert!(error.params.is_empty());
    }

    /// Everything else a failed connect can be — refused, unresolved, rejected credentials — still
    /// travels with the driver's own text, which is the part worth searching for.
    #[test]
    fn every_other_connect_failure_keeps_the_drivers_words() {
        let error = connect_error(sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "connection refused",
        )));
        assert_eq!(error.code, "error.mysql");
        assert!(error.params.contains_key("message"));
    }
}
