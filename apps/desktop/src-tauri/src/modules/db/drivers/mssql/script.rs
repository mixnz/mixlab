//! Running whatever T-SQL the user typed in the Query tab. The counterpart of `mysql/script.rs`/
//! `postgres/script.rs`, and the same three jobs — split a script, run it statement by statement,
//! and answer what the server makes of one statement without running it — plus a fourth SQL Server
//! alone needs: carving `GO` batches out before any of that starts (D9).
//!
//! Kept in step with `src/modules/db/sql/syntax.ts`'s `MSSQL_SYNTAX` and
//! `src/modules/db/sql/statements.ts`'s `splitStatements` by a parallel test suite below — same
//! input, same statement count — not by a shared shape: Rust has no `SqlSyntax`, and the frontend
//! splitter runs before any command reaches this file at all (Plan 4).
//!
//! **`tiberius` has no call that returns both rows and a rows-affected count, unlike `sqlx`.**
//! `Client::query`/`simple_query` return a `QueryStream` whose `into_results()` only collects `Row`
//! and result-set `Metadata` tokens — a plain `UPDATE` with no result set comes back as an empty
//! `Vec<Vec<Row>>`, its `Done` token silently dropped. `Client::execute` reads the opposite way: it
//! collects only `Done`/`DoneProc`/`DoneInProc` tokens into an `ExecuteResult`, discarding any row
//! data those same tokens might have accompanied. `sqlx::raw_sql(...).fetch_many()`, which
//! `mysql::script`/`postgres::script` use for both at once via `Either<QueryResult, Row>`, has no
//! `tiberius` equivalent — confirmed by reading `tiberius-0.12.3`'s `result.rs`/`client.rs`, not
//! assumed.
//!
//! Worse, `execute`/`query` are not a free choice between the two even taken separately: both send
//! their SQL through `sp_executesql` (`RpcProcId::ExecuteSQL`), and SQL Server gives an
//! `sp_executesql` call its own scope — a local temp table (`#t`) created inside one is dropped the
//! instant that call returns, invisible to the very next statement even on the same `Connection`.
//! Found live against the test server, not from source alone: a `CREATE TABLE #t` sent through
//! `execute`, followed by an `INSERT INTO #t` through another `execute` call on the same session,
//! failed with "Invalid object name '#t'". So [`run_statement`] never uses `execute`/`query` at
//! all — everything goes through `simple_query`, a plain TDS batch with no scope of its own, and the
//! row count `simple_query` cannot give directly is read back with an appended `SELECT @@ROWCOUNT`
//! in the same batch instead. See [`run_statement`] for the detail.

use super::{column_value, map_error, quote_ident, Connection, Pool};
use crate::error::AppError;
use crate::modules::db::models::{SqlProblem, StatementResult};
use deadpool::managed::Object;
use serde_json::Value;
use std::time::Instant;
use tiberius::Row;

/// One statement carved out of the editor's text, already past its `GO` batch boundary.
struct Statement {
    text: String,
    /// The keyword it opens with, upper-cased. Empty for a run of nothing but comments or a lone
    /// `GO` line, which is how either is recognised as not being a statement at all.
    verb: String,
}

/// A line holding nothing but `GO`, optionally with a repeat count — case-insensitive, mirroring
/// `src/modules/db/sql/statements.ts`'s `GO_SEPARATOR`. `GO` is a client-side convention (`sqlcmd`,
/// SSMS), not a keyword: sent to the server as text it is a syntax error, so it has to be carved out
/// before the `;`-splitter below ever sees it as code.
fn is_go_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 2 || !trimmed.is_char_boundary(2) {
        return trimmed.eq_ignore_ascii_case("go");
    }
    let (head, rest) = trimmed.split_at(2);
    if !head.eq_ignore_ascii_case("go") {
        return false;
    }
    // `\bgo\b` in spirit: nothing but whitespace and an optional digit run may follow, so
    // `GOOD`/`GO3`/`GO_TABLE` are rejected — a real repeat count needs a space before its digits.
    let rest = rest.trim();
    rest.is_empty()
        || rest
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_whitespace())
}

/// Splits a script into batches on a line holding nothing but `GO` (or `GO n`), the first of the two
/// tiers D9 calls for. Each batch is handed whole to [`split_statements`] next, so a semicolon inside
/// one still separates statements the normal way.
///
/// Reads line by line rather than tracking open strings/comments/brackets the way
/// `src/modules/db/sql/statements.ts`'s `matchBatchSeparator` does, because this runs first, on the
/// raw text, before anything else has a chance to. A line holding nothing but `GO` inside a string or
/// a multi-line comment would be cut here as if it were a real separator — a real gap from the JS
/// version, and a narrow enough one to accept for v1 (a multi-line string or comment whose own text
/// is exactly a `GO` line is vanishingly rare in real T-SQL). If this ever needs to be exact, the fix
/// is to give this the same open-string/comment/bracket tracking `split_statements` already has,
/// not to special-case `GO` inside it.
fn split_batches(sql: &str) -> Vec<String> {
    sql.lines().fold(vec![String::new()], |mut batches, line| {
        if is_go_line(line) {
            batches.push(String::new());
        } else {
            let current = batches
                .last_mut()
                .expect("the fold starts with one batch and only ever adds more");
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
        batches
    })
}

/// Splits one `GO` batch into the statements sent one at a time. Only a semicolon outside a string,
/// a bracketed or double-quoted identifier, and a comment separates two — the same set of rules
/// `postgres::script::split_statements` reads for PostgreSQL, minus dollar quoting and plus SQL
/// Server's own two quirks:
///
/// * **`"` is always an identifier, never a string** — `QUOTED_IDENTIFIER ON` is the default every
///   client driver sets, `tiberius` included, so unlike MySQL/PostgreSQL there is no double-quoted
///   string branch to fall back to here at all.
/// * **`[name]` is a second identifier quote, asymmetric** — `]` doubled escapes one inside the
///   name, `[` doubled means nothing (the name is already open by the time a second `[` is read).
///
/// Block comments do not nest (like MySQL, unlike PostgreSQL); `--` always opens a comment whatever
/// follows it (like PostgreSQL, unlike MySQL); there is no `#` comment (`#` opens a temp table name).
fn split_statements(sql: &str) -> Vec<Statement> {
    let chars: Vec<char> = sql.chars().collect();
    let mut statements: Vec<Statement> = Vec::new();
    let mut current = String::new();
    let mut verb = String::new();
    let mut verb_done = false;
    let mut i = 0;

    fn push(
        statements: &mut Vec<Statement>,
        current: &mut String,
        verb: &mut String,
        verb_done: &mut bool,
    ) {
        let text = current.trim().to_string();
        current.clear();
        *verb_done = false;
        let verb = std::mem::take(verb);
        // No opening keyword means there was no statement here — only whitespace, or a comment
        // sitting between two separators.
        if verb.is_empty() {
            return;
        }
        statements.push(Statement { text, verb });
    }

    while i < chars.len() {
        let c = chars[i];

        // `--` always opens a comment in T-SQL, whatever follows it — unlike MySQL, which wants
        // whitespace after it so `5--3` stays arithmetic.
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }
        // T-SQL's block comments do not nest: a second `/*` inside one is just text, and the first
        // `*/` closes the whole thing — like MySQL, unlike PostgreSQL.
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            current.push('/');
            current.push('*');
            i += 2;
            while i < chars.len() {
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    current.push('*');
                    current.push('/');
                    i += 2;
                    break;
                }
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }

        if c == '\'' || c == '"' || c == '[' {
            // `"` and `[` both open an identifier; `'` opens a string. Neither kind escapes with a
            // backslash here — doubling the close character is the only escape T-SQL has, for both.
            let close = if c == '[' { ']' } else { c };
            current.push(c);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                current.push(ch);
                i += 1;
                if ch == close {
                    // Two of the close character in a row are an escaped one, not the end of the run.
                    if chars.get(i) == Some(&close) {
                        current.push(close);
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }

        if c == ';' {
            push(&mut statements, &mut current, &mut verb, &mut verb_done);
            i += 1;
            continue;
        }

        // Plain code: the first word of it is the statement's keyword.
        if !verb_done {
            if c.is_alphanumeric() || c == '_' {
                verb.extend(c.to_uppercase());
            } else if !verb.is_empty() {
                verb_done = true;
            }
        }
        current.push(c);
        i += 1;
    }

    push(&mut statements, &mut current, &mut verb, &mut verb_done);
    statements
}

/// Splits a whole script into the statements `run` sends one at a time, `GO` batches carved out
/// first (D9). Comments and whitespace between two `GO`s, or before the first one, produce no
/// statement — the same "empty verb is not a statement" rule `split_statements` already applies
/// inside a batch.
fn split_script(sql: &str) -> Vec<Statement> {
    split_batches(sql)
        .iter()
        .flat_map(|batch| split_statements(batch))
        .collect()
}

/// The four verbs whose point is the number of rows they changed, so that a count of zero is still
/// worth reporting as a count — the same list `postgres::script::is_write_verb` reads, minus MySQL's
/// `REPLACE`/`LOAD` and plus `MERGE`, which T-SQL has and MySQL does not.
fn is_write_verb(verb: &str) -> bool {
    matches!(verb, "INSERT" | "UPDATE" | "DELETE" | "MERGE")
}

/// How many rows of one result set are read back. A query without a `TOP` can name more rows than
/// there is memory for, so the client stops here and says that it did — the same ceiling
/// `mysql::script`/`postgres::script` use, for the same reason: the grid only ever shows the rows on
/// screen now.
const MAX_ROWS: usize = 10_000;

/// The server's own words for one failed statement — never routed through [`map_error`], which
/// produces a translation code for the *command's* own `Result`, not the per-statement text this
/// app has always shown the way the server worded it (see `mysql::script`/`postgres::script`, which
/// keep the same distinction).
fn statement_error(e: tiberius::error::Error) -> String {
    match &e {
        tiberius::error::Error::Server(token) => token.message().to_string(),
        _ => e.to_string(),
    }
}

/// Whether `text` is a `CREATE`/`ALTER`/`CREATE OR ALTER` `VIEW`/`PROCEDURE`/`PROC`/`FUNCTION`/
/// `TRIGGER` — the class of statement SQL Server refuses to run unless it is the *only* statement in
/// its batch. [`run_statement`] otherwise appends `;SELECT @@ROWCOUNT AS n` to every statement it
/// sends, which is itself a second statement in the same batch — a rule broken by the very technique
/// that reads back a row count. Found live: restoring a dump whose structure included a view
/// (`CREATE VIEW ... AS SELECT ...`) failed with a bare "Incorrect syntax near the keyword 'SELECT'"
/// rather than the clearer "must be the first statement in a query batch" message SSMS shows for the
/// same mistake — the appended text lands mid-parse of the view's own body, not after it.
///
/// Checked on the statement text rather than `Statement.verb`, which is only the first word
/// (`"CREATE"` for both `CREATE TABLE` and `CREATE VIEW` alike) — telling them apart needs the
/// second word too.
fn requires_own_batch(text: &str) -> bool {
    let mut words = text.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    if !first.eq_ignore_ascii_case("create") && !first.eq_ignore_ascii_case("alter") {
        return false;
    }
    let mut next = words.next().unwrap_or("");
    if next.eq_ignore_ascii_case("or") {
        if !words.next().unwrap_or("").eq_ignore_ascii_case("alter") {
            return false;
        }
        next = words.next().unwrap_or("");
    }
    matches!(
        next.to_ascii_uppercase().as_str(),
        "VIEW" | "PROCEDURE" | "PROC" | "FUNCTION" | "TRIGGER"
    )
}

fn failed(statement: &Statement, started: Instant, message: String) -> StatementResult {
    StatementResult {
        statement: statement.text.clone(),
        verb: statement.verb.clone(),
        kind: "error".to_string(),
        columns: Vec::new(),
        rows: Vec::new(),
        truncated: false,
        rows_affected: 0,
        last_insert_id: None,
        duration_ms: started.elapsed().as_millis() as u64,
        error: Some(message),
    }
}

/// Runs one statement and reports its outcome.
///
/// Always through `simple_query`, never `execute`/`query` — found live, against the test server,
/// not from reading `tiberius`'s source alone: both of those call SQL Server's `sp_executesql`
/// under the hood (`RpcProcId::ExecuteSQL` — see this module's top-level doc), and SQL Server gives
/// an `sp_executesql` call its own nested scope. A local temp table (`#t`) created inside one is
/// dropped the instant that call returns — invisible to the very next statement even on the same
/// connection, session and SPID. `CREATE TABLE #t` through `execute`, then `INSERT INTO #t` through
/// another `execute` call, failed with "Invalid object name '#t'" the first time this ran against
/// the real server, despite both calls sharing one `Connection`. `simple_query` sends a plain TDS
/// batch instead, with no such scope of its own — the same one every other statement in a [`run`]
/// already shares.
///
/// The cost: `simple_query`'s stream exposes rows and result-set boundaries only, never a `Done`
/// token's row count (this module's top-level doc, again). So `@@ROWCOUNT` — SQL Server's own
/// answer to "how many rows did the statement just above this one touch" — is read back by
/// appending `SELECT @@ROWCOUNT AS n` to the same batch, on its own line so a statement ending in an
/// un-newlined `--` comment cannot swallow it, and taking it as the *last* result set. This also
/// means every statement, whatever its verb, is asked the same way — no more guessing ahead of time
/// whether one might return rows, and no more `EXEC` blind spot: a stored procedure's own last
/// `INSERT`/`UPDATE` sets `@@ROWCOUNT` too, the same approximation SSMS's own "(N rows affected)"
/// banner relies on for an ad hoc batch.
///
/// One limitation this does not solve: a result set with columns but zero rows reads its column
/// list off the first row collected, so a genuinely empty `SELECT` reports `"ok"` with no columns
/// rather than `"rows"` with an empty grid. Narrow enough to accept for now — `tiberius`'s
/// `QueryStream::columns()` could recover it, but only by giving up `into_results()`'s single call
/// and driving the stream by hand across two result sets, and no query this app runs today is
/// expected to be both column-bearing and empty.
async fn run_statement(
    client: &mut Connection,
    statement: &Statement,
    started: Instant,
) -> StatementResult {
    if requires_own_batch(&statement.text) {
        return match client.simple_query(statement.text.clone()).await {
            Ok(stream) => match stream.into_results().await {
                Ok(_) => StatementResult {
                    statement: statement.text.clone(),
                    verb: statement.verb.clone(),
                    kind: "ok".to_string(),
                    columns: Vec::new(),
                    rows: Vec::new(),
                    truncated: false,
                    rows_affected: 0,
                    last_insert_id: None,
                    duration_ms: started.elapsed().as_millis() as u64,
                    error: None,
                },
                Err(e) => failed(statement, started, statement_error(e)),
            },
            Err(e) => failed(statement, started, statement_error(e)),
        };
    }

    let combined = format!("{}\n;SELECT @@ROWCOUNT AS n", statement.text);
    let mut results = match client.simple_query(combined).await {
        Ok(stream) => match stream.into_results().await {
            Ok(results) => results,
            Err(e) => return failed(statement, started, statement_error(e)),
        },
        Err(e) => return failed(statement, started, statement_error(e)),
    };

    let rows_affected = results
        .pop()
        .and_then(|rowcount_set| rowcount_set.into_iter().next())
        .and_then(|row| row.get::<i32, _>("n"))
        .unwrap_or(0)
        .max(0) as u64;

    match results.into_iter().next() {
        Some(rows) => {
            let columns: Vec<String> = rows
                .first()
                .map(|row: &Row| row.columns().iter().map(|c| c.name().to_string()).collect())
                .unwrap_or_default();
            let truncated = rows.len() > MAX_ROWS;
            let values: Vec<Vec<Value>> = rows
                .iter()
                .take(MAX_ROWS)
                .map(|row| row.cells().map(|(_, data)| column_value(data)).collect())
                .collect();
            StatementResult {
                statement: statement.text.clone(),
                verb: statement.verb.clone(),
                kind: if columns.is_empty() { "ok" } else { "rows" }.to_string(),
                columns,
                rows: values,
                truncated,
                rows_affected,
                last_insert_id: None,
                duration_ms: started.elapsed().as_millis() as u64,
                error: None,
            }
        }
        None => {
            // A count of zero is still reported as `"affected"` for one of the four write verbs —
            // the same rule `mysql::script`'s callers use: `UPDATE ... 0 rows` says something,
            // `SET @x = 1 ... 0 rows` does not.
            let kind = if is_write_verb(&statement.verb) || rows_affected > 0 {
                "affected"
            } else {
                "ok"
            };
            StatementResult {
                statement: statement.text.clone(),
                verb: statement.verb.clone(),
                kind: kind.to_string(),
                columns: Vec::new(),
                rows: Vec::new(),
                truncated: false,
                rows_affected,
                last_insert_id: None,
                duration_ms: started.elapsed().as_millis() as u64,
                error: None,
            }
        }
    }
}

/// The session's own SPID, which is what [`cancel`] names to `KILL` — the counterpart of
/// `mysql::thread_id`/`postgres::script::backend_pid`.
async fn session_id(client: &mut Connection) -> Result<u64, AppError> {
    let row = client
        .simple_query("SELECT @@SPID")
        .await
        .map_err(map_error)?
        .into_row()
        .await
        .map_err(map_error)?
        .ok_or_else(|| err!("error.mssql", message = "the server reported no @@SPID"))?;
    Ok(row.get::<i16, _>(0).unwrap_or(0).max(0) as u64)
}

/// Runs the editor's text batch by batch (D9) and statement by statement within each batch,
/// reporting each statement's outcome. Everything runs on one connection, so a `USE`, a `SET`, a
/// temporary table or a transaction opened by one statement is still in force for the next — a
/// script reads the way it would in `sqlcmd`.
///
/// The connection is detached from the pool with [`Object::take`] before the first statement runs,
/// and simply dropped — closing the TDS session — when `run` returns or is dropped early. Handing it
/// back to the pool instead would carry whatever the script left set: another database (`USE`), an
/// open transaction, a temp table, a `SET` no other borrower expects. `Manager::recycle`'s
/// `SELECT 1` would not catch any of that — only a dead connection fails it. Same reasoning as
/// `mysql::script::run`'s identical `close_on_drop`.
///
/// A statement that fails stops the script: its own result carries the error, and the results
/// before it are still returned. `announce` is handed the session's SPID once, before the first
/// statement runs — the only handle another connection has on it, and what [`cancel`] needs to stop
/// it.
pub async fn run(
    pool: &Pool,
    sql: &str,
    database: Option<&str>,
    announce: impl FnOnce(u64),
) -> Result<Vec<StatementResult>, AppError> {
    let statements = split_script(sql);
    if statements.is_empty() {
        return Err(err!("error.nothingToRun"));
    }

    let guard = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let mut client = Object::take(guard);

    if let Some(db) = database.filter(|d| !d.is_empty()) {
        client
            .simple_query(format!("USE {}", quote_ident(db)))
            .await
            .map_err(map_error)?;
    }

    announce(session_id(&mut client).await?);

    let mut results = Vec::new();
    for statement in &statements {
        let started = Instant::now();
        let result = run_statement(&mut client, statement, started).await;
        let failed = result.kind == "error";
        results.push(result);
        if failed {
            break;
        }
    }

    Ok(results)
}

/// SQL Server has no Attention/cancel API `tiberius` exposes publicly — checked against
/// `tiberius-0.12.3`'s source, not assumed: `AttentionSignal`/`Attention` only appear as internal
/// packet/token kinds the client reads, with no `pub fn` anywhere that sends one (D8). The only way
/// left to stop a session from another connection is `KILL <spid>`, which ends the **whole session**,
/// not just the statement in flight — losing its transaction and any temp table, unlike MySQL's
/// `KILL QUERY` or PostgreSQL's `pg_cancel_backend`. That is a real, sharper cost than the other two
/// engines pay, and `mssqlDialect.cancellable`'s own doc comment says so plainly.
///
/// `KILL` also needs `ALTER ANY CONNECTION` (or `sysadmin`/`processadmin`) — a login without it
/// cannot even kill its own session. The test server only has `sa`, which always has it, so this
/// cannot be measured against an ordinary login (D8 leaves this open rather than resolved against a
/// login that always passes). Rather than gate the Cancel button on an untested permission probe, it
/// stays open and any permission error SQL Server gives comes back to the user exactly as the server
/// worded it — extending `sql_cancel_query`'s "asking to cancel what already finished is not an
/// error" with "asking to cancel what you may not kill is the server's answer to give, not this
/// app's to guess at".
///
/// A SPID the server no longer has is not a failure worth showing: error 6101 ("Session ID %d is
/// not valid") is what the test server actually answers `KILL 99999` with (found live — public
/// write-ups of this error more often name 6106/6107, kept here too since the exact number is a
/// documented engine-version quirk, not a stable contract), swallowed the same way
/// `mysql::kill_query`'s `ER_NO_SUCH_THREAD` and `postgres::script::cancel`'s "a pid the server no
/// longer has answers false" are. Any other error — permission denied among them — is returned
/// as-is.
pub async fn cancel(pool: &Pool, session_id: u64) -> Result<(), AppError> {
    const NO_SUCH_PROCESS: [u32; 3] = [6101, 6106, 6107];
    let guard = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let mut client = Object::take(guard);
    let result = client.simple_query(format!("KILL {session_id}")).await;
    match result {
        Ok(_) => Ok(()),
        Err(tiberius::error::Error::Server(e)) if NO_SUCH_PROCESS.contains(&e.code()) => Ok(()),
        Err(e) => Err(map_error(e)),
    }
}

/// Asks SQL Server what it makes of one statement, **without running it**.
///
/// `SET PARSEONLY ON` makes every statement after it parse-only until `SET PARSEONLY OFF` — the
/// closest T-SQL equivalent to MySQL's `PREPARE`/PostgreSQL's `Parse` message. It catches less than
/// either: PARSEONLY does not resolve table or column names, only syntax — so unlike
/// `mysql::script`/`postgres::script`, there is no "looks wrong now, might not be once a temp table
/// exists" case to downgrade to a warning here. Everything this returns is a genuine syntax error.
///
/// Runs on a pooled connection kept for reuse across a debounce, like `mysql::script::validate` — not
/// the session a script runs on, so `PARSEONLY` never leaks onto a connection [`run`] might later
/// borrow (each call turns it back `OFF` before returning, on success or failure alike).
pub async fn validate(
    pool: &Pool,
    sql: &str,
    database: Option<&str>,
) -> Result<Option<SqlProblem>, AppError> {
    if sql.trim().is_empty() {
        return Ok(None);
    }

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;

    if let Some(db) = database.filter(|d| !d.is_empty()) {
        // A database that cannot be entered ends the check rather than failing it: whatever is
        // wrong is wrong with the header, not with the statement being asked about. Chained
        // straight onto `.await` rather than kept in a named `Result` — the `QueryStream` an `Ok`
        // would hold borrows `client`, and every branch here needs to call `simple_query` on it
        // again next.
        if client
            .simple_query(format!("USE {}", quote_ident(db)))
            .await
            .is_err()
        {
            return Ok(None);
        }
    }

    if client.simple_query("SET PARSEONLY ON").await.is_err() {
        return Ok(None);
    }

    let outcome = match client.simple_query(sql.to_string()).await {
        Ok(_) => None,
        Err(tiberius::error::Error::Server(e)) => Some(SqlProblem {
            message: e.message().to_string(),
            number: e.code().try_into().unwrap_or(0),
            line: Some(e.line()),
            severity: "error".to_string(),
        }),
        Err(_) => None,
    };

    // Best-effort, like `mysql::script::validate`'s DEALLOCATE: the connection goes back to the pool
    // either way, and a failure to turn PARSEONLY back off is not the user's statement's problem —
    // `Manager::recycle`'s `SELECT 1` is what actually decides whether this connection is trustworthy
    // enough to hand out again.
    let _ = client.simple_query("SET PARSEONLY OFF").await;

    Ok(outcome)
}

/// What the splitter has to get right is where one statement ends and one batch ends — a semicolon
/// or `GO` line inside a string, a quoted identifier or a comment is text, not a separator, and
/// sending the halves of a statement separately is a syntax error at best and half an operation at
/// worst. These mirror `src/modules/db/sql/statements.test.ts`'s `MSSQL_SYNTAX` describe blocks
/// (Plan 4) one for one — same input, same statement count — per D9's "kept in step by a parallel
/// test suite" note.
#[cfg(test)]
mod tests {
    use super::{requires_own_batch, split_script};

    fn verbs(sql: &str) -> Vec<String> {
        split_script(sql).into_iter().map(|s| s.verb).collect()
    }

    fn texts(sql: &str) -> Vec<String> {
        split_script(sql).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn requires_own_batch_covers_create_and_alter_of_the_batch_alone_kinds() {
        assert!(requires_own_batch("CREATE VIEW v AS SELECT 1"));
        assert!(requires_own_batch("ALTER VIEW v AS SELECT 1"));
        assert!(requires_own_batch("CREATE PROCEDURE p AS SELECT 1"));
        assert!(requires_own_batch("CREATE PROC p AS SELECT 1"));
        assert!(requires_own_batch(
            "CREATE FUNCTION f() RETURNS int AS BEGIN RETURN 1 END"
        ));
        assert!(requires_own_batch(
            "CREATE TRIGGER t ON a AFTER INSERT AS BEGIN END"
        ));
        assert!(requires_own_batch("CREATE OR ALTER VIEW v AS SELECT 1"));
    }

    #[test]
    fn requires_own_batch_leaves_every_other_statement_alone() {
        assert!(!requires_own_batch("CREATE TABLE t (id int)"));
        assert!(!requires_own_batch("ALTER TABLE t ADD c int"));
        assert!(!requires_own_batch("SELECT * FROM v"));
        assert!(!requires_own_batch("INSERT INTO t VALUES (1)"));
        assert!(!requires_own_batch(""));
    }

    #[test]
    fn bracket_identifier_hides_its_semicolon() {
        assert_eq!(
            texts("SELECT * FROM [Order;Details]; SELECT 1"),
            ["SELECT * FROM [Order;Details]", "SELECT 1"]
        );
    }

    #[test]
    fn doubled_close_bracket_is_one_literal_bracket() {
        assert_eq!(texts("SELECT * FROM [a]]b]"), ["SELECT * FROM [a]]b]"]);
    }

    #[test]
    fn double_quote_is_always_an_identifier() {
        assert_eq!(
            texts(r#"SELECT * FROM "Order;Details""#),
            [r#"SELECT * FROM "Order;Details""#]
        );
    }

    #[test]
    fn go_ends_the_previous_statement_without_becoming_one() {
        assert_eq!(verbs("SELECT 1\nGO\nSELECT 2"), ["SELECT", "SELECT"]);
    }

    #[test]
    fn semicolons_still_split_inside_each_batch() {
        assert_eq!(
            verbs("SELECT 1; SELECT 2\nGO\nSELECT 3"),
            ["SELECT", "SELECT", "SELECT"]
        );
    }

    #[test]
    fn go_accepts_a_repeat_count() {
        assert_eq!(verbs("SELECT 1\nGO 3\nSELECT 2"), ["SELECT", "SELECT"]);
    }

    #[test]
    fn go_is_case_insensitive() {
        assert_eq!(verbs("SELECT 1\ngo\nSELECT 2"), ["SELECT", "SELECT"]);
    }

    #[test]
    fn two_gos_in_a_row_add_no_empty_statement() {
        assert_eq!(verbs("SELECT 1\nGO\nGO\nSELECT 2"), ["SELECT", "SELECT"]);
    }

    #[test]
    fn go_needs_the_line_to_itself() {
        assert_eq!(verbs("SELECT 1 GO\nSELECT 2"), ["SELECT"]);
        // Read as one statement, not two — `GO` here is just two more words of the SELECT list.
        assert_eq!(texts("SELECT 1 GO\nSELECT 2"), ["SELECT 1 GO\nSELECT 2"]);
    }

    #[test]
    fn a_name_merely_starting_with_go_is_not_a_separator() {
        assert_eq!(verbs("SELECT good_column FROM t"), ["SELECT"]);
    }

    #[test]
    fn is_not_fooled_by_a_comment_that_merely_contains_the_word() {
        assert_eq!(
            texts("SELECT 1 -- go\nSELECT 2"),
            ["SELECT 1 -- go\nSELECT 2"]
        );
    }
}
