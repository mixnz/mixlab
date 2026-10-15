//! ClickHouse, read side: connecting over its HTTP interface and reading what is on it.
//!
//! No sqlx driver exists for ClickHouse, so this module talks the HTTP interface directly through
//! `reqwest` — already in the tree for the REST module — rather than adding a client crate built
//! around rows of a fixed, compile-time shape. `clickhouse` (the crate ClickHouse Inc. publishes)
//! is exactly that: it wants a Rust struct per table, which is the wrong shape for a browser that
//! opens a table it has never seen before. Every query here is sent as the request body with
//! `FORMAT JSON` appended; the response carries its own column names and types in `meta` and the
//! rows as JSON objects in `data`, which is already the shape `row_to_json` builds by hand for
//! MySQL and PostgreSQL.
//!
//! v1 was read-only throughout — see the plan this was built from
//! (`2026-09-04-clickhouse-db-kind.md`, a local plan, never committed). Row writes (insert/update/delete)
//! shipped after it — see `docs/specs/2026-09-04-clickhouse-row-writes-design.md`. DDL,
//! dump/restore and the Query tab's own writes are still closed.

pub mod ddl;
pub mod dump;
pub mod script;
pub mod structure;

use super::filters::{escape_like, split_list};
use crate::error::AppError;
use crate::modules::db::models::ServerInfo;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// One live ClickHouse connection: the server's base URL and the credentials sent with every
/// request. Cheap to clone — `reqwest::Client` is an `Arc` inside, and the rest is two short
/// strings — which is what lets `commands::handle` hand it out without holding the connection map.
#[derive(Clone)]
pub struct Connection {
    client: Client,
    base_url: String,
    user: String,
    password: String,
}

/// Turns a failed request or a response the server refused into the one error code every caller
/// reports through — the counterpart of `mysql::map_error`, without a "lost connection" case:
/// there is no session to lose, only a request that either lands or does not.
fn map_error(message: impl std::fmt::Display) -> AppError {
    err!("error.clickhouse", message = message)
}

/// Opens a connection, and proves it: `SELECT 1` is run here rather than left for the first real
/// query, so that a wrong password or an unreachable host is reported by the Connect button and
/// not by the sidebar a moment later — the same reasoning as `postgres::connect`.
///
/// `use_ssl` chooses the scheme outright — `Some(true)` for `https://`, anything else for
/// `http://` — rather than the "try TLS, fall back to plaintext" MySQL and PostgreSQL do. Those
/// two negotiate TLS *inside* one TCP connection, so a server that turns out not to offer it can
/// still be reached in plaintext. HTTP and HTTPS are two different endpoints from the first byte;
/// there is no connection to fall back within, so leaving `use_ssl` unset defaults to the scheme
/// the box on the connection form itself defaults to — plain HTTP.
pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    use_ssl: Option<bool>,
) -> Result<Connection, AppError> {
    let scheme = if use_ssl == Some(true) {
        "https"
    } else {
        "http"
    };
    let base_url = format!("{scheme}://{host}:{port}/");
    let client = Client::builder().build().map_err(map_error)?;
    let conn = Connection {
        client,
        base_url,
        user: username.to_string(),
        password: password.to_string(),
    };
    query(&conn, "SELECT 1").await?;
    Ok(conn)
}

/// What one call to the server answers back, in the shape `FORMAT JSON` always gives: the rows it
/// found, each one already a JSON object keyed by column name, since that is what `FORMAT JSON`
/// writes them as.
///
/// `FORMAT JSON` also carries a `meta` array — the columns' names and types of *this* result —
/// which nothing here reads: a column's type is always looked up from `system.columns` before a
/// `SELECT` naming it is built (see [`table_columns`]), because the decision `is_decodable` makes
/// has to be baked into that `SELECT`'s own text, not read back after the fact from a query that
/// already assumed an answer.
#[derive(Debug, Deserialize)]
pub struct QueryResult {
    #[serde(default)]
    pub data: Vec<Map<String, Value>>,
}

/// Sends one statement with no parameters — see [`query_with_params`], which this calls with none.
pub async fn query(conn: &Connection, sql: &str) -> Result<QueryResult, AppError> {
    query_with_params(conn, sql, &[]).await
}

/// Sends one statement and reads back whatever it returns, decoded as JSON.
///
/// `params` are ClickHouse's own parameterized-query values: the statement names each one as
/// `{name:Type}` and the value is sent as `param_name=value` on the URL, never interpolated into
/// the SQL text. This is what keeps a filter value the user typed from being SQL — the same job
/// `sqlx`'s `.bind()` does for the other engines, done here by hand because there is no `sqlx`
/// driver for this one to lean on.
///
/// `FORMAT JSON` is appended on a line of its own rather than asked for as a query parameter, so
/// that a statement ending in its own `-- comment` does not swallow it. Credentials go as headers
/// (`X-ClickHouse-User` / `X-ClickHouse-Key`) instead of Basic Auth in the URL, so they never sit
/// on a request line any HTTP logging in between would keep.
pub async fn query_with_params(
    conn: &Connection,
    sql: &str,
    params: &[(String, String)],
) -> Result<QueryResult, AppError> {
    let mut url = url::Url::parse(&conn.base_url).map_err(map_error)?;
    if !params.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (name, value) in params {
            pairs.append_pair(&format!("param_{name}"), value);
        }
    }
    let body = format!("{sql}\nFORMAT JSON");
    let mut request = conn.client.post(url).body(body);
    if !conn.user.is_empty() {
        request = request
            .header("X-ClickHouse-User", &conn.user)
            .header("X-ClickHouse-Key", &conn.password);
    }
    let response = request.send().await.map_err(map_error)?;
    let status = response.status();
    let text = response.text().await.map_err(map_error)?;
    if !status.is_success() {
        // ClickHouse's own error text ("Code: 516. DB::Exception: ... Authentication failed") is
        // plain text on a failure, not JSON — the same words the CLI client would show.
        return Err(map_error(text.trim()));
    }
    serde_json::from_str(&text).map_err(map_error)
}

/// Sends one statement of user-authored text — the Query tab's, not this module's own generated
/// SQL — scoped to `database` when one is given, so an unqualified table name in it resolves the
/// way the sidebar's own choice of database means it to.
///
/// A separate function from [`query_with_params`] rather than that one widened with another
/// parameter: every other caller in this module builds its own SQL text and already qualifies
/// every table it names (see [`qualified`]), so none of them has a use for this — and `{name:Type}`
/// parameters have no part to play in a script the user typed by hand.
///
/// Checked against the test server: `?database=x` on the URL scopes an unqualified table the same
/// way `USE x` would, without a session to run `USE` inside — the HTTP interface has none.
pub(super) async fn query_in_database(
    conn: &Connection,
    sql: &str,
    database: Option<&str>,
) -> Result<QueryResult, AppError> {
    let mut url = url::Url::parse(&conn.base_url).map_err(map_error)?;
    if let Some(database) = database.filter(|d| !d.is_empty()) {
        url.query_pairs_mut().append_pair("database", database);
    }
    let body = format!("{sql}\nFORMAT JSON");
    let mut request = conn.client.post(url).body(body);
    if !conn.user.is_empty() {
        request = request
            .header("X-ClickHouse-User", &conn.user)
            .header("X-ClickHouse-Key", &conn.password);
    }
    let response = request.send().await.map_err(map_error)?;
    let status = response.status();
    let text = response.text().await.map_err(map_error)?;
    if !status.is_success() {
        return Err(map_error(text.trim()));
    }
    serde_json::from_str(&text).map_err(map_error)
}

/// Sends one statement and hands back the raw response for the caller to read as a stream, rather
/// than buffering it into a `String` first the way [`query_with_params`]/[`query_in_database`] do —
/// for `clickhouse/dump.rs`'s data export, where the whole point is never holding a table's worth of
/// output in memory at once. Checks the status before handing the response back: a failure's body
/// is still small (ClickHouse's own error text), so reading it here is cheaper than making every
/// caller re-implement the same check.
pub(super) async fn query_streaming(
    conn: &Connection,
    sql: &str,
    database: Option<&str>,
) -> Result<reqwest::Response, AppError> {
    let mut url = url::Url::parse(&conn.base_url).map_err(map_error)?;
    if let Some(database) = database.filter(|d| !d.is_empty()) {
        url.query_pairs_mut().append_pair("database", database);
    }
    let mut request = conn.client.post(url).body(sql.to_string());
    if !conn.user.is_empty() {
        request = request
            .header("X-ClickHouse-User", &conn.user)
            .header("X-ClickHouse-Key", &conn.password);
    }
    let response = request.send().await.map_err(map_error)?;
    let status = response.status();
    if status.is_success() {
        Ok(response)
    } else {
        let text = response.text().await.map_err(map_error)?;
        Err(map_error(text.trim()))
    }
}

/// Sends one statement with no `FORMAT` appended, and reports only whether the server accepted it
/// — for `EXPLAIN AST`, whose own output is a plain-text tree rather than anything `FORMAT JSON`
/// would turn into rows. Checked against the test server: `EXPLAIN AST select 1\nFORMAT JSON`
/// parses as `EXPLAIN AST` of the query `select 1 FORMAT JSON` — the appended format is read as
/// part of what is *being explained*, not as a format for the explanation itself — so the two
/// have to stay apart, unlike every other statement `query_in_database` runs.
pub(super) async fn execute_check(
    conn: &Connection,
    sql: &str,
    database: Option<&str>,
) -> Result<(), AppError> {
    let mut url = url::Url::parse(&conn.base_url).map_err(map_error)?;
    if let Some(database) = database.filter(|d| !d.is_empty()) {
        url.query_pairs_mut().append_pair("database", database);
    }
    let mut request = conn.client.post(url).body(sql.to_string());
    if !conn.user.is_empty() {
        request = request
            .header("X-ClickHouse-User", &conn.user)
            .header("X-ClickHouse-Key", &conn.password);
    }
    let response = request.send().await.map_err(map_error)?;
    let status = response.status();
    let text = response.text().await.map_err(map_error)?;
    if status.is_success() {
        Ok(())
    } else {
        Err(map_error(text.trim()))
    }
}

/// The `written_rows` field of an `X-ClickHouse-Summary` response header, or `0` when the header
/// is missing, is not JSON, or has no such field — a synchronous write has already succeeded by
/// the time this is read, so a header that cannot be parsed is not a reason to fail the call.
///
/// The field is a JSON *string*, not a number — checked against the test server: `written_rows`
/// comes back as `"2"`, not `2`. Everything else in the header follows the same convention (large
/// integers as JSON strings, the same reason ClickHouse's `FORMAT JSON` does it for query results).
pub(super) fn parse_written_rows(summary_header: &str) -> u64 {
    serde_json::from_str::<Value>(summary_header)
        .ok()
        .and_then(|v| {
            v.get("written_rows")
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0)
}

/// Sends one statement with no `FORMAT` appended — same wire shape as [`execute_check`] — and
/// reads back how many rows ClickHouse reports having written, from the `X-ClickHouse-Summary`
/// response header. This is the only place that count is available: `FORMAT JSON` cannot be
/// appended to ask for it the way a `SELECT`'s can be, since a synchronous `INSERT` has no result
/// set of its own to format (see `clickhouse/script.rs`'s module doc — `INSERT ... FORMAT JSON` is
/// parsed as a different statement, not a request for JSON output, checked against the test
/// server).
pub(super) async fn execute_with_written_rows(
    conn: &Connection,
    sql: &str,
    database: Option<&str>,
) -> Result<u64, AppError> {
    let mut url = url::Url::parse(&conn.base_url).map_err(map_error)?;
    if let Some(database) = database.filter(|d| !d.is_empty()) {
        url.query_pairs_mut().append_pair("database", database);
    }
    let mut request = conn.client.post(url).body(sql.to_string());
    if !conn.user.is_empty() {
        request = request
            .header("X-ClickHouse-User", &conn.user)
            .header("X-ClickHouse-Key", &conn.password);
    }
    let response = request.send().await.map_err(map_error)?;
    let status = response.status();
    let written_rows = response
        .headers()
        .get("X-ClickHouse-Summary")
        .and_then(|v| v.to_str().ok())
        .map(parse_written_rows)
        .unwrap_or(0);
    let text = response.text().await.map_err(map_error)?;
    if status.is_success() {
        Ok(written_rows)
    } else {
        Err(map_error(text.trim()))
    }
}

/// Backtick-quotes an identifier for interpolation into SQL text, backslash-escaping an embedded
/// backtick.
///
/// Backslash rather than doubling — unlike MySQL's own `quote_ident` — because that is what
/// ClickHouse's lexer actually does with one: checked against the test server, `` SELECT 1 AS
/// `a``b` `` is refused (400) while `` SELECT 1 AS `a\`b` `` parses and names the column `` a`b ``.
pub(super) fn quote_ident(ident: &str) -> String {
    format!("`{}`", ident.replace('\\', "\\\\").replace('`', "\\`"))
}

/// A database and table addressed together, both quoted — how a table is written into SQL text.
pub(super) fn qualified(database: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(database), quote_ident(table))
}

/// Backslash-escapes and single-quotes a value for splicing into ClickHouse SQL text as a string
/// literal — the counterpart of `quote_ident` for values instead of identifiers. Checked against
/// the same backslash convention `quote_ident` uses rather than doubling.
///
/// Used only for building `ALTER TABLE ... UPDATE/DELETE` and `INSERT` text: unlike a read, that
/// text has to be matched back against `system.mutations.command` afterwards (see
/// `run_mutation_and_wait`), so it is spliced in literally rather than sent as a `{name:Type}`
/// bound parameter — a resolved literal round-trips there, a parameter placeholder's resolved form
/// is not something to rely on.
pub(super) fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// One column's `WHERE` fragment identifying a row by its current value: direct equality for a
/// column `is_decodable`, `toString(...)` for one that is not (the design's D2 — direct equality
/// keeps the sparse primary index usable and avoids `Float`/`Decimal` round-trip mismatches that a
/// blanket `toString()` would risk), and `IS NULL` for a value that is itself null, since
/// ClickHouse has no `<=>`.
fn key_clause(name: &str, type_name: &str, value: &Value) -> String {
    let ident = quote_ident(name);
    match value {
        Value::Null => format!("{ident} IS NULL"),
        Value::String(s) if is_decodable(type_name) => format!("{ident} = {}", quote_literal(s)),
        Value::String(s) => format!("toString({ident}) = {}", quote_literal(s)),
        // The frontend's `SqlApi` types this as `string | null` — anything else reaching here is
        // impossible through it, but a defensive value beats a panic if that ever changes.
        _ => format!("{ident} IS NULL"),
    }
}

/// The `WHERE` clause identifying one row by every column named in `key` — ClickHouse has no
/// primary key, so the frontend always sends every column (the design's D1). Each name must be a
/// real column: an unknown one means the schema drifted out from under an open tab, not something
/// to guess past.
fn build_key_where(
    columns: &BTreeMap<String, String>,
    key: &Map<String, Value>,
) -> Result<String, AppError> {
    let mut clauses = Vec::new();
    for (name, value) in key {
        let Some(type_name) = columns.get(name) else {
            return Err(err!("error.unknownFilterColumn", column = name));
        };
        clauses.push(key_clause(name, type_name, value));
    }
    Ok(clauses.join(" AND "))
}

/// The `WHERE` clause for a multi-row delete: every key's own clause, parenthesised, joined by
/// `OR` — one mutation for the whole batch (the design's D3) rather than one per key, since
/// ClickHouse's mutation has no `LIMIT` to make looping any safer than a single combined statement
/// whose match count is checked once, for the whole batch, before anything runs.
fn combined_key_where(
    columns: &BTreeMap<String, String>,
    keys: &[Map<String, Value>],
) -> Result<String, AppError> {
    let mut groups = Vec::new();
    for key in keys {
        if key.is_empty() {
            return Err(err!("error.deleteWithoutKey"));
        }
        groups.push(format!("({})", build_key_where(columns, key)?));
    }
    Ok(groups.join(" OR "))
}

/// Whether every row names the same set of columns — order does not matter, only which names are
/// present. `INSERT INTO t (a, b) VALUES (...), (...)` needs one column list for the whole
/// statement, so rows that disagree on which columns they fill in cannot go into the same one.
fn same_columns(rows: &[Map<String, Value>]) -> bool {
    let Some(first) = rows.first() else {
        return true;
    };
    let expected: BTreeSet<&str> = first.keys().map(String::as_str).collect();
    rows[1..]
        .iter()
        .all(|row| row.keys().map(String::as_str).collect::<BTreeSet<&str>>() == expected)
}

/// Whether `row`'s `is_done` reads as ClickHouse's true — `1` as either the JSON number or the
/// string `FORMAT JSON` may write a `UInt8` as.
fn mutation_is_done(row: &Map<String, Value>) -> bool {
    match row.get("is_done") {
        Some(Value::Number(n)) => n.as_i64() == Some(1),
        Some(Value::String(s)) => s == "1",
        _ => false,
    }
}

/// `system.mutations.latest_fail_reason` for `row`, or empty when there is none.
fn mutation_fail_reason(row: &Map<String, Value>) -> String {
    row.get("latest_fail_reason")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Among mutation rows just read from `system.mutations`, the one this call itself submitted: its
/// `mutation_id` was not in `baseline_ids`, read before submitting. `None` when zero or more than
/// one such row exists — see `run_mutation_and_wait`'s use of this, which keeps polling rather than
/// guess in that case.
///
/// Matching is by `mutation_id` alone, not by comparing `command` text against what was sent:
/// checked against a real server, `system.mutations.command` is ClickHouse's own reformatting of
/// the statement (identifiers unquoted, clauses reparenthesised — `` `id` = '2' `` submitted comes
/// back as `(id = '2')`), not the text this module built. Reproducing that formatting to compare
/// against would be reproducing ClickHouse's own SQL printer; `mutation_id` set membership carries
/// the same information without needing to.
fn find_new_mutation<'a>(
    rows: &'a [Map<String, Value>],
    baseline_ids: &std::collections::HashSet<String>,
) -> Option<&'a Map<String, Value>> {
    let mut matches = rows.iter().filter(|row| {
        row.get("mutation_id")
            .and_then(Value::as_str)
            .map(|id| !baseline_ids.contains(id))
            .unwrap_or(false)
    });
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

/// How long a poll waits before giving up on a mutation ever finishing, and how often it checks —
/// named rather than inline so a later tune (see the plan's Task 5 note) is a one-line change.
const MUTATION_POLL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const MUTATION_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

/// Submits an `ALTER TABLE ... UPDATE/DELETE` and waits for the mutation it starts to finish, so
/// the `Result` this returns means "written" the way every other engine's does — `ALTER TABLE`
/// itself only *enqueues* the mutation.
///
/// `command_sql` is sent as-is, values already spliced in as literals (`quote_literal`) rather than
/// as a `{name:Type}` parameter — not because anything reads it back (see `find_new_mutation`'s own
/// doc for why matching dropped that idea), but because `matched_count`'s pre-check and this
/// statement have to agree on which rows they mean, and a parameter is one more thing that could
/// resolve differently between the two calls.
///
/// Never assumes success without finding the mutation and seeing `is_done`: if no row (yet) matches
/// — including the "more than one matched" case `find_new_mutation` refuses to pick between — this
/// keeps polling rather than guessing, until the timeout turns that into an explicit error.
pub(super) async fn run_mutation_and_wait(
    conn: &Connection,
    database: &str,
    table: &str,
    command_sql: &str,
) -> Result<(), AppError> {
    let mutations_sql = "SELECT mutation_id, is_done, latest_fail_reason \
         FROM system.mutations WHERE database = {database:String} AND table = {table:String}";
    let params = [
        ("database".to_string(), database.to_string()),
        ("table".to_string(), table.to_string()),
    ];

    let baseline = query_with_params(conn, mutations_sql, &params).await?;
    let baseline_ids: std::collections::HashSet<String> = baseline
        .data
        .iter()
        .filter_map(|row| {
            row.get("mutation_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();

    execute_check(conn, command_sql, None).await?;

    let deadline = std::time::Instant::now() + MUTATION_POLL_TIMEOUT;
    loop {
        let result = query_with_params(conn, mutations_sql, &params).await?;
        if let Some(row) = find_new_mutation(&result.data, &baseline_ids) {
            let reason = mutation_fail_reason(row);
            if !reason.is_empty() {
                return Err(map_error(reason));
            }
            if mutation_is_done(row) {
                return Ok(());
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(err!("error.clickhouseMutationTimeout"));
        }
        tokio::time::sleep(MUTATION_POLL_INTERVAL).await;
    }
}

/// How many rows `where_clause` matches right now — the pre-check every write runs before touching
/// anything, since ClickHouse has neither a transaction to roll back nor a `LIMIT` on a mutation to
/// cap the damage of a `WHERE` that turned out to match more than one row (the design's D3).
async fn matched_count(
    conn: &Connection,
    table_ref: &str,
    where_clause: &str,
) -> Result<i64, AppError> {
    let sql = format!("SELECT count() AS total FROM {table_ref} WHERE {where_clause}");
    let result = query(conn, &sql).await?;
    Ok(result
        .data
        .first()
        .and_then(|row| row.get("total"))
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_i64())
        })
        .unwrap_or(0))
}

/// Updates exactly one row, identified by every column of `key` (the design's D1) — refuses if that
/// does not match exactly one row (D3), then runs the update as a mutation and waits for it (D4).
/// Values in `updates` are spliced in as literal text without inspecting the column's type (D8): a
/// value that does not fit the column is ClickHouse's own error to report, not this function's to
/// predict.
pub async fn update_row(
    conn: &Connection,
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

    let column_rows = table_columns(conn, database, table).await?;
    let types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|c| (c.name.clone(), c.type_name.clone()))
        .collect();

    let table_ref = qualified(database, table);
    let where_clause = build_key_where(&types, key)?;

    let matched = matched_count(conn, &table_ref, &where_clause).await?;
    if matched != 1 {
        return Err(err!("error.rowsMatched", matched = matched));
    }

    let mut set_parts = Vec::new();
    for (name, value) in updates {
        if !types.contains_key(name) {
            return Err(err!("error.unknownFilterColumn", column = name));
        }
        let ident = quote_ident(name);
        let rhs = match value {
            Value::String(s) => quote_literal(s),
            _ => "NULL".to_string(),
        };
        set_parts.push(format!("{ident} = {rhs}"));
    }

    let sql = format!(
        "ALTER TABLE {table_ref} UPDATE {} WHERE {where_clause}",
        set_parts.join(", ")
    );
    run_mutation_and_wait(conn, database, table, &sql).await
}

/// Deletes rows on a ClickHouse table. `all` truncates — synchronous, and sidesteps the mutation
/// wait entirely (the design's D5) — the common case of clearing a table. Otherwise every key in
/// `keys` must match, combined into one mutation (`combined_key_where`) whose match count is
/// checked against `keys.len()` before it runs (D3): fewer matches means a row already moved out
/// from under the selection, more means duplicates on every column are about to lose more rows than
/// were selected — either way, refuse rather than guess.
///
/// `reset_auto_increment` is accepted and ignored: ClickHouse has no such column to reset.
pub async fn delete_rows(
    conn: &Connection,
    database: &str,
    table: &str,
    keys: &[Map<String, Value>],
    all: bool,
    reset_auto_increment: bool,
) -> Result<(), AppError> {
    let _ = reset_auto_increment;
    let table_ref = qualified(database, table);

    if all {
        let sql = format!("TRUNCATE TABLE {table_ref}");
        return execute_check(conn, &sql, None).await;
    }
    if keys.is_empty() {
        return Ok(());
    }

    let column_rows = table_columns(conn, database, table).await?;
    let types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|c| (c.name.clone(), c.type_name.clone()))
        .collect();
    let where_clause = combined_key_where(&types, keys)?;

    let matched = matched_count(conn, &table_ref, &where_clause).await?;
    if matched != keys.len() as i64 {
        return Err(err!("error.rowsMatched", matched = matched));
    }

    let sql = format!("ALTER TABLE {table_ref} DELETE WHERE {where_clause}");
    run_mutation_and_wait(conn, database, table, &sql).await
}

/// Inserts `rows` as one `INSERT` statement — which ClickHouse commits as a single atomic block,
/// the closest thing to the "all or nothing" `SqlApi.insertRows` documents (MySQL and PostgreSQL
/// get that guarantee from a real transaction; ClickHouse has none, so one statement is what stands
/// in for it). That only works when every row fills in the same columns (`same_columns`) — rows
/// that disagree are refused outright rather than split into several statements that would each
/// commit or fail on their own, silently breaking the "all or nothing" promise (the design's D7).
///
/// Values are spliced in as literal text without inspecting the column's type (D8), the same as
/// `update_row`.
pub async fn insert_rows(
    conn: &Connection,
    database: &str,
    table: &str,
    rows: &[Map<String, Value>],
) -> Result<(), AppError> {
    if rows.is_empty() {
        return Ok(());
    }
    if !same_columns(rows) {
        return Err(err!("error.clickhouseHeterogeneousInsert"));
    }

    let columns: BTreeSet<&str> = rows[0].keys().map(String::as_str).collect();
    let columns: Vec<&str> = columns.into_iter().collect();
    let table_ref = qualified(database, table);
    let column_list = columns
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let values_list = rows
        .iter()
        .map(|row| {
            let values = columns
                .iter()
                .map(|c| match row.get(*c) {
                    Some(Value::String(s)) => quote_literal(s),
                    _ => "NULL".to_string(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("({values})")
        })
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!("INSERT INTO {table_ref} ({column_list}) VALUES {values_list}");
    execute_check(conn, &sql, None).await
}

/// What the header shows about the server: its version, and the machine it runs on.
///
/// `hostName()` rather than `system.uname` — the latter is not on every version this app can meet
/// (missing outright on the 26.8 test server this was built against), where `hostName()` is a
/// plain SQL function and has been since ClickHouse could be clustered at all.
pub async fn server_info(conn: &Connection) -> Result<ServerInfo, AppError> {
    let version = scalar(conn, "SELECT version()").await?;
    let os = scalar(conn, "SELECT hostName()").await.unwrap_or_default();
    Ok(ServerInfo { version, os })
}

/// The one value a single-row, single-column query answers with, read as a string. ClickHouse's
/// `FORMAT JSON` already writes every scalar as the closest JSON type — including 64-bit integers
/// as JSON strings, to keep the precision an `f64` would round away — so a value already a JSON
/// string is used as it stands, and anything else is rendered rather than assumed absent.
async fn scalar(conn: &Connection, sql: &str) -> Result<String, AppError> {
    let result = query(conn, sql).await?;
    let row = result.data.first().ok_or_else(|| map_error("no rows"))?;
    let value = row.values().next().ok_or_else(|| map_error("no columns"))?;
    Ok(match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// Every database on the server that can be connected to — everything but the three ClickHouse
/// keeps for itself, the same exclusion `clickhouse::isClickhouseSystemDatabase` makes on the
/// frontend.
pub async fn list_databases(conn: &Connection) -> Result<Vec<String>, AppError> {
    let result = query(
        conn,
        "SELECT name FROM system.databases \
         WHERE name NOT IN ('system', 'information_schema', 'INFORMATION_SCHEMA') \
         ORDER BY name",
    )
    .await?;
    Ok(result
        .data
        .iter()
        .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_string))
        .collect())
}

/// Every table and view of `database`, alphabetically.
pub async fn list_tables(conn: &Connection, database: &str) -> Result<Vec<String>, AppError> {
    let result = query_with_params(
        conn,
        "SELECT name FROM system.tables WHERE database = {database:String} ORDER BY name",
        &[("database".to_string(), database.to_string())],
    )
    .await?;
    Ok(result
        .data
        .iter()
        .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_string))
        .collect())
}

/// Whether a column of this type can be trusted to decode as `FORMAT JSON` writes it, rather than
/// needing `toString()` wrapped around it in the `SELECT` list — see the plan's D7.
///
/// `Nullable(X)` is unwrapped first, since it is `X` the question is really about. Everything not
/// matched here — `Array`, `Map`, `Tuple`, `Nested`, `LowCardinality`, `IPv4`/`IPv6`, and the
/// `AggregateFunction`/`SimpleAggregateFunction` states a materialized view rolls up — is read as
/// text instead: the grid draws flat cells, and none of those has one JSON shape worth trusting
/// blind.
fn is_decodable(type_name: &str) -> bool {
    let inner = type_name
        .strip_prefix("Nullable(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(type_name);
    let head = inner.split('(').next().unwrap_or(inner);
    matches!(
        head,
        "UInt8"
            | "UInt16"
            | "UInt32"
            | "UInt64"
            | "UInt128"
            | "UInt256"
            | "Int8"
            | "Int16"
            | "Int32"
            | "Int64"
            | "Int128"
            | "Int256"
            | "Float32"
            | "Float64"
            | "String"
            | "FixedString"
            | "Date"
            | "Date32"
            | "DateTime"
            | "DateTime64"
            | "Decimal"
            | "Decimal32"
            | "Decimal64"
            | "Decimal128"
            | "Decimal256"
            | "UUID"
            | "Enum8"
            | "Enum16"
            | "Bool"
    )
}

/// One column of a table, as `system.columns` reports it.
struct TableColumn {
    name: String,
    type_name: String,
}

async fn table_columns(
    conn: &Connection,
    database: &str,
    table: &str,
) -> Result<Vec<TableColumn>, AppError> {
    let result = query_with_params(
        conn,
        "SELECT name, type FROM system.columns \
         WHERE database = {database:String} AND table = {table:String} \
         ORDER BY position",
        &[
            ("database".to_string(), database.to_string()),
            ("table".to_string(), table.to_string()),
        ],
    )
    .await?;
    Ok(result
        .data
        .iter()
        .filter_map(|row| {
            let name = row.get("name")?.as_str()?.to_string();
            let type_name = row.get("type")?.as_str()?.to_string();
            Some(TableColumn { name, type_name })
        })
        .collect())
}

/// What is known about one column beyond its name — the shape the Structure tab and the grid both
/// read. `foreign_key` is always `None`: ClickHouse has no foreign keys to report.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub extra: String,
    pub foreign_key: Option<Value>,
}

/// One condition on the rows a page is cut out of, as the grid's filter bar sends it — the same
/// shape MySQL and PostgreSQL read, minus the two `regexp` operators: `regexpFilter: false` on the
/// dialect keeps the filter bar from ever offering them for this kind.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub column: String,
    pub operator: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// Turns the filter rows into a `WHERE` clause and the parameters to bind into it, in
/// `{name:Type}` form — see [`query_with_params`].
///
/// Every comparison goes through `toString()` on the column, the value bound as `String`: the
/// frontend sends every filter value as text, and ClickHouse — unlike MySQL — does not coerce a
/// `String` parameter to compare against a `UInt64` column on its own. The ordering operators are
/// the exception, the same way they are on PostgreSQL: `toString()` on a number sorts `10` before
/// `9`, so there the column is left as it is and the parameter is typed to match instead.
fn build_where(
    filters: &[Filter],
    columns: &BTreeMap<String, String>,
) -> Result<(String, Vec<(String, String)>), AppError> {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<(String, String)> = Vec::new();
    let mut next = 0usize;
    let mut placeholder = |params: &mut Vec<(String, String)>, ty: &str, value: String| {
        let name = format!("p{next}");
        next += 1;
        params.push((name.clone(), value));
        format!("{{{name}:{ty}}}")
    };

    for filter in filters {
        let Some(base_type) = columns.get(&filter.column) else {
            return Err(err!("error.unknownFilterColumn", column = &filter.column));
        };
        let col = format!("toString({})", quote_ident(&filter.column));
        let raw = quote_ident(&filter.column);
        let value = filter.value.as_deref().unwrap_or("");
        let operator = filter.operator.as_str();

        let clause = match operator {
            "eq" => format!(
                "{col} = {}",
                placeholder(&mut params, "String", value.to_string())
            ),
            "ne" => format!(
                "{col} <> {}",
                placeholder(&mut params, "String", value.to_string())
            ),
            "gt" => format!(
                "{raw} > {}",
                placeholder(&mut params, base_type, value.to_string())
            ),
            "gte" => format!(
                "{raw} >= {}",
                placeholder(&mut params, base_type, value.to_string())
            ),
            "lt" => format!(
                "{raw} < {}",
                placeholder(&mut params, base_type, value.to_string())
            ),
            "lte" => format!(
                "{raw} <= {}",
                placeholder(&mut params, base_type, value.to_string())
            ),
            "contains" => format!(
                "{col} LIKE {}",
                placeholder(&mut params, "String", format!("%{}%", escape_like(value)))
            ),
            "notContains" => format!(
                "{col} NOT LIKE {}",
                placeholder(&mut params, "String", format!("%{}%", escape_like(value)))
            ),
            "startsWith" => format!(
                "{col} LIKE {}",
                placeholder(&mut params, "String", format!("{}%", escape_like(value)))
            ),
            "endsWith" => format!(
                "{col} LIKE {}",
                placeholder(&mut params, "String", format!("%{}", escape_like(value)))
            ),
            "like" => format!(
                "{col} LIKE {}",
                placeholder(&mut params, "String", value.to_string())
            ),
            "notLike" => format!(
                "{col} NOT LIKE {}",
                placeholder(&mut params, "String", value.to_string())
            ),
            "in" | "notIn" => {
                let items = split_list(value);
                if items.is_empty() {
                    continue;
                }
                let placeholders = items
                    .into_iter()
                    .map(|item| placeholder(&mut params, "String", item))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql_op = if operator == "in" { "IN" } else { "NOT IN" };
                format!("{col} {sql_op} ({placeholders})")
            }
            "between" | "notBetween" => {
                let items = split_list(value);
                if items.len() < 2 {
                    continue;
                }
                let low = placeholder(&mut params, base_type, items[0].clone());
                let high = placeholder(&mut params, base_type, items[1].clone());
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
        return Ok((String::new(), params));
    }
    Ok((format!(" WHERE {}", clauses.join(" AND ")), params))
}

/// Which page of a table is wanted, and what it is cut out of — the same shape MySQL and
/// PostgreSQL take.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageQuery {
    pub page: i64,
    pub page_size: i64,
    #[serde(default)]
    pub sort_column: Option<String>,
    #[serde(default)]
    pub sort_desc: bool,
    #[serde(default)]
    pub filters: Vec<Filter>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TablePage {
    pub columns: Vec<String>,
    pub column_meta: BTreeMap<String, ColumnMeta>,
    pub primary_key: Vec<String>,
    pub auto_increment_column: Option<String>,
    pub rows: Vec<Map<String, Value>>,
    pub total: i64,
}

/// Reads one page of a table. `auto_increment_column` is always `None` — v1 never writes, so
/// nothing needs to know which column a resettable counter would be on.
///
/// `primary_key` is the table's sorting key (`system.tables.sorting_key`) rather than a real
/// constraint — ClickHouse has none — split naively on `, `. For a plain list of columns, which
/// almost every `ORDER BY` is, that is exactly right; for a key built from an expression it shows
/// the expression as if it were one long column name, which is a display quirk and not a
/// correctness problem, since nothing in v1 uses this to identify a row.
pub async fn table_data(
    conn: &Connection,
    database: &str,
    table: &str,
    query: &PageQuery,
) -> Result<TablePage, AppError> {
    let table_ref = qualified(database, table);
    let column_rows = table_columns(conn, database, table).await?;
    let columns: Vec<String> = column_rows.iter().map(|c| c.name.clone()).collect();

    let column_meta: BTreeMap<String, ColumnMeta> = column_rows
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                ColumnMeta {
                    data_type: c.type_name.clone(),
                    nullable: c.type_name.starts_with("Nullable("),
                    default_value: None,
                    extra: String::new(),
                    foreign_key: None,
                },
            )
        })
        .collect();

    let column_types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|c| (c.name.clone(), c.type_name.clone()))
        .collect();
    let (where_clause, params) = build_where(&query.filters, &column_types)?;
    let ch_params: Vec<(String, String)> = params;

    let sorting_key_result = query_with_params(
        conn,
        "SELECT sorting_key FROM system.tables WHERE database = {database:String} AND name = {table:String}",
        &[
            ("database".to_string(), database.to_string()),
            ("table".to_string(), table.to_string()),
        ],
    )
    .await;
    let sorting_key = sorting_key_result
        .ok()
        .and_then(|result| {
            result
                .data
                .first()
                .and_then(|row| row.get("sorting_key")?.as_str().map(str::to_string))
        })
        .unwrap_or_default();
    let primary_key: Vec<String> = if sorting_key.trim().is_empty() {
        Vec::new()
    } else {
        sorting_key.split(", ").map(str::to_string).collect()
    };

    let count_sql = format!("SELECT count() AS total FROM {table_ref}{where_clause}");
    let count_result = query_with_params(conn, &count_sql, &ch_params).await?;
    let total: i64 = count_result
        .data
        .first()
        .and_then(|row| row.get("total"))
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_i64())
        })
        .unwrap_or(0);

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
    let select_list = column_rows
        .iter()
        .map(|c| {
            let ident = quote_ident(&c.name);
            if is_decodable(&c.type_name) {
                ident
            } else {
                format!("toString({ident}) AS {ident}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let data_sql = format!(
        "SELECT {select_list} FROM {table_ref}{where_clause}{order_by} LIMIT {page_size} OFFSET {offset}"
    );
    let data_result = query_with_params(conn, &data_sql, &ch_params).await?;

    Ok(TablePage {
        columns,
        column_meta,
        primary_key,
        auto_increment_column: None,
        rows: data_result.data,
        total,
    })
}

#[cfg(test)]
mod tests;
