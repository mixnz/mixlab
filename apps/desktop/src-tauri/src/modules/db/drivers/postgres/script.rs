//! Running whatever SQL the user typed, against PostgreSQL — the counterpart of `mysql/script.rs`,
//! and the same three jobs: split a script, run it statement by statement, and answer what the
//! server makes of one statement without running it.
//!
//! Three things differ from MySQL, and each is the reason for a piece of code below:
//!
//! * **Dollar quoting.** `$$ ... $$` and `$tag$ ... $tag$` hold text that may contain anything,
//!   semicolons included, and a function body is normally written that way — so a splitter that
//!   does not know about it carves every `CREATE FUNCTION` into pieces.
//! * **No `USE`.** The database is decided by which pool this is handed, not by a statement at the
//!   top of the script — see `postgres::Pools`.
//! * **Parse rather than PREPARE.** Checking a statement without running it is a protocol message
//!   here, not a SQL statement, so the user's text is never interpolated into SQL to check it.

use super::{column_value, map_error};
use crate::error::AppError;
use crate::modules::db::drivers::sql::run::{run_statements, ScriptRows};
use crate::modules::db::drivers::sql::{statements::split_statements, syntax::POSTGRES};
use crate::modules::db::models::{SqlProblem, StatementResult};
use sqlx::{Executor, PgPool, Postgres};

/// What the shared loop needs to read PostgreSQL's results. There is no generated key to report:
/// PostgreSQL has no per-statement one, and `RETURNING` is how a script asks for it.
const ROWS: ScriptRows<Postgres> = ScriptRows {
    column_value,
    is_write_verb,
    done: |result, _verb| (result.rows_affected(), None),
};

/// The text could not be parsed. The one thing a checker can be sure is wrong.
const SYNTAX_ERROR: &str = "42601";
/// The user may not do this, or is not who they say. Both say nothing about the text itself: the
/// statement is well-formed, this login simply may not run it — and running it is not what was
/// asked for.
const NOT_ALLOWED: [&str; 2] = ["42501", "28000"];

/// The line a character offset falls on, counted from the start of the statement.
fn line_of(sql: &str, position: usize) -> u32 {
    let upto = position.min(sql.chars().count());
    1 + sql.chars().take(upto).filter(|c| *c == '\n').count() as u32
}

/// Asks PostgreSQL what it makes of one statement, **without running it**.
///
/// The statement is sent as a Parse message — the first half of the extended query protocol — which
/// makes the server parse and plan it and nothing more. Nothing is bound and nothing is executed,
/// so this is safe to fire at a half-typed `DELETE`. Unlike MySQL's `PREPARE`, no SQL text is built
/// here at all: the user's statement travels as a protocol field, never as part of a statement.
///
/// This runs on a pooled connection rather than on the session the script runs on, which is why
/// most of what comes back is a warning: a temporary table or a `SET` from earlier in the script is
/// invisible from here, so "relation does not exist" may well mean "not yet".
pub async fn validate(pool: &PgPool, sql: &str) -> Result<Option<SqlProblem>, AppError> {
    if sql.trim().is_empty() {
        return Ok(None);
    }
    let mut conn = pool.acquire().await.map_err(map_error)?;

    let statement = sqlx::SqlSafeStr::into_sql_str(sqlx::AssertSqlSafe(sql));
    let Err(error) = conn.prepare(statement).await else {
        return Ok(None);
    };
    let Some(db) = error.as_database_error() else {
        return Ok(None);
    };
    let code = db.code().unwrap_or_default().to_string();
    if NOT_ALLOWED.contains(&code.as_str()) {
        return Ok(None);
    }
    let line = db
        .try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
        .and_then(|e| e.position())
        .and_then(|p| match p {
            sqlx::postgres::PgErrorPosition::Original(at) => Some(line_of(sql, at)),
            // A position in *internal* text — inside a function the statement called — points at
            // something the user did not write, so there is nowhere in the editor to put it.
            _ => None,
        });
    Ok(Some(SqlProblem {
        message: db.message().to_string(),
        number: 0,
        line,
        severity: if code == SYNTAX_ERROR {
            "error"
        } else {
            "warning"
        }
        .to_string(),
    }))
}

/// The statements whose point is the number of rows they changed, so that a count of zero is still
/// worth reporting as a count.
fn is_write_verb(verb: &str) -> bool {
    matches!(
        verb,
        "INSERT" | "UPDATE" | "DELETE" | "TRUNCATE" | "COPY" | "MERGE"
    )
}

/// The session's own id, which is what `pg_cancel_backend` names.
pub async fn backend_pid(conn: &mut sqlx::PgConnection) -> Result<u64, AppError> {
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(conn)
        .await
        .map_err(map_error)?;
    Ok(pid.max(0) as u64)
}

/// Asks the server to stop whatever the session `pid` is running.
///
/// `pg_cancel_backend` rather than `pg_terminate_backend`: it ends the statement and leaves the
/// session open, so what comes back is an error the script can report against the statement it
/// stopped, rather than a dropped connection reported as "connection lost" against the whole run.
/// A pid the server no longer has answers false, which is not a failure worth showing — the user
/// asked for it to stop, and it has.
pub async fn cancel(pool: &PgPool, pid: u64) -> Result<(), AppError> {
    sqlx::query_scalar::<_, bool>("SELECT pg_cancel_backend($1)")
        .bind(pid as i32)
        .fetch_optional(pool)
        .await
        .map(|_| ())
        .map_err(map_error)
}

/// Runs the editor's text statement by statement and reports each one's outcome.
///
/// Everything runs on one connection, so a `SET`, a temporary table or a transaction opened by one
/// statement is still in force for the next. A statement that fails stops the script: its own
/// result carries the error, and the results before it still come back. That connection is the
/// script's own and is closed when the script ends, so nothing it left behind — an aborted
/// transaction most of all — is ever seen by the queries the rest of the app runs; the other side
/// of that is that one Run press starts a session and the next one starts another.
///
/// `announce` is handed the session's backend pid before the first statement runs — the only handle
/// another connection has on it, and what {@link cancel} needs to stop a statement in flight.
///
/// There is no `database` argument, unlike the MySQL side: which database this runs against is
/// decided by which pool it is given.
pub async fn run(
    pool: &PgPool,
    sql: &str,
    announce: impl FnOnce(u64),
) -> Result<Vec<StatementResult>, AppError> {
    let statements = split_statements(sql, &POSTGRES);
    if statements.is_empty() {
        return Err(err!("error.nothingToRun"));
    }

    let mut conn = pool.acquire().await.map_err(map_error)?;
    // The script gets this connection to itself, and it is closed when the script is done rather
    // than going back to the pool carrying whatever the script left on it. `BEGIN; SELECT 1/0;`
    // leaves an aborted transaction, and every later statement on that connection comes back
    // `25P02` until someone rolls it back — which means the next `list_tables` or `table_data` to
    // borrow it fails for no reason the user can see. A temporary table, a `SET`, an advisory lock
    // and a `LISTEN` all survive the ping sqlx gives a returned connection too.
    //
    // `DISCARD ALL` would clear all of that, but only from a connection that is not mid-transaction
    // and still answering — which is exactly the case that has gone wrong. Closing needs neither.
    //
    // On drop rather than at the end: an error on the way in, or the whole run being dropped
    // because the tab closed, has to close it too. The pool's permit is held until it does, so this
    // never opens a sixth connection. The cost is one handshake per Run, which is a keypress.
    conn.close_on_drop();
    announce(backend_pid(&mut conn).await?);

    Ok(run_statements(&mut *conn, statements, &ROWS).await)
}

#[cfg(test)]
mod tests {
    use super::line_of;
    use crate::modules::db::drivers::sql::{statements::split_statements, syntax::POSTGRES};

    fn verbs(sql: &str) -> Vec<String> {
        split_statements(sql, &POSTGRES)
            .into_iter()
            .map(|s| s.verb)
            .collect()
    }

    fn texts(sql: &str) -> Vec<String> {
        split_statements(sql, &POSTGRES)
            .into_iter()
            .map(|s| s.text)
            .collect()
    }

    #[test]
    fn splits_on_semicolons_outside_everything_else() {
        assert_eq!(verbs("SELECT 1; UPDATE t SET a = 2"), ["SELECT", "UPDATE"]);
        // Only whitespace or a comment between two semicolons is not a statement.
        assert_eq!(verbs("SELECT 1;; -- done\n"), ["SELECT"]);
        assert_eq!(verbs("   "), Vec::<String>::new());
    }

    /// The one that MySQL's splitter would get wrong: a function body is held together by its
    /// dollar quotes however many semicolons are inside it.
    #[test]
    fn a_dollar_quoted_body_is_one_statement() {
        let sql = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN a := 1; RETURN a; END $$ LANGUAGE plpgsql; SELECT 2";
        assert_eq!(verbs(sql), ["CREATE", "SELECT"]);
        assert!(texts(sql)[0].contains("RETURN a;"));
    }

    /// A tagged quote closes only on its own tag, so an inner `$$` does not end it.
    #[test]
    fn a_tagged_body_closes_only_on_its_tag() {
        let sql =
            "CREATE FUNCTION f() RETURNS text AS $body$ SELECT '$$'; $body$ LANGUAGE sql; SELECT 1";
        assert_eq!(verbs(sql), ["CREATE", "SELECT"]);
    }

    /// `$1` is a placeholder. Read as an opening quote it would swallow the rest of the script.
    #[test]
    fn placeholders_are_not_dollar_quotes() {
        assert_eq!(verbs("SELECT $1; SELECT $2"), ["SELECT", "SELECT"]);
    }

    /// A semicolon inside a string or a quoted identifier does not end the statement, and a
    /// backslash is an ordinary character — unlike MySQL, where it escapes.
    #[test]
    fn quotes_hold_a_statement_together() {
        assert_eq!(verbs("SELECT ';'; SELECT 2"), ["SELECT", "SELECT"]);
        assert_eq!(
            verbs(r#"SELECT "a;b" FROM t; SELECT 2"#),
            ["SELECT", "SELECT"]
        );
        assert_eq!(verbs(r"SELECT 'a\'; SELECT 2"), ["SELECT", "SELECT"]);
        // Doubling is how a quote is escaped, and does not end the literal.
        assert_eq!(
            verbs("SELECT 'it''s; here'; SELECT 2"),
            ["SELECT", "SELECT"]
        );
    }

    /// The exception to the rule above: an `E'...'` string does escape with backslashes, so the
    /// quote after one is not the end of it. Checked against the server, which answers
    /// `SELECT E'it\'s; here'` with `it's; here` — one statement holding a semicolon.
    #[test]
    fn an_e_string_escapes_with_backslashes() {
        assert_eq!(
            verbs(r"SELECT E'it\'s; here'; SELECT 2"),
            ["SELECT", "SELECT"]
        );
        assert_eq!(
            texts(r"SELECT E'it\'s; here'; SELECT 2")[0],
            r"SELECT E'it\'s; here'"
        );
        // Lowercase is the same prefix.
        assert_eq!(
            verbs(r"SELECT e'it\'s; here'; SELECT 2"),
            ["SELECT", "SELECT"]
        );
        // A name merely ending in `e` is not one, and neither is one held off by a space.
        assert_eq!(verbs(r"SELECT type'a\'; SELECT 2"), ["SELECT", "SELECT"]);
    }

    /// `$` is legal inside a PostgreSQL identifier, so `a$b$c` is one column name and not a body
    /// opening at `$b$` — the server names the column `a$b$c` and reads `x$$y$$` as a name too.
    /// Read the wrong way round, either eats the rest of the script.
    #[test]
    fn a_dollar_inside_a_name_is_not_a_quote() {
        assert_eq!(verbs("SELECT 1 AS a$b$c; SELECT 2"), ["SELECT", "SELECT"]);
        assert_eq!(verbs("SELECT x$$y$$; SELECT 2"), ["SELECT", "SELECT"]);
        // A real body is still one, wherever it opens.
        assert_eq!(
            verbs("SELECT $tag$a; b$tag$; SELECT 2"),
            ["SELECT", "SELECT"]
        );
    }

    /// PostgreSQL's block comments nest; stopping at the first `*/` would leave code behind.
    #[test]
    fn block_comments_nest() {
        assert_eq!(verbs("/* a /* b */ ; c */ SELECT 1"), ["SELECT"]);
    }

    /// `#` is an operator in PostgreSQL, not the start of a comment.
    #[test]
    fn hash_is_not_a_comment() {
        assert_eq!(verbs("SELECT 1 # 2; SELECT 3"), ["SELECT", "SELECT"]);
    }

    #[test]
    fn counts_the_line_an_offset_falls_on() {
        assert_eq!(line_of("SELECT\nFROM\nWHERE", 0), 1);
        assert_eq!(line_of("SELECT\nFROM\nWHERE", 8), 2);
        assert_eq!(line_of("SELECT\nFROM\nWHERE", 999), 3);
    }
}
