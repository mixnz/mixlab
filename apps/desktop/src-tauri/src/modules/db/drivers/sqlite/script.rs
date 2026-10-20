//! Running whatever SQL the user typed, against SQLite — the counterpart of `mysql/script.rs` and
//! `postgres/script.rs`, and two of their three jobs: split a script, and run it statement by
//! statement.
//!
//! The third, stopping a statement that is already running, has no counterpart here. There is no
//! session on a server to reach in and kill: the statement runs inside this process against a file.
//! The Query tab's Cancel button is closed on this engine rather than left to be pressed and do
//! nothing — see `cancellable` on the dialect.
//!
//! The script is split by the shared splitter, with SQLite's rules in `sql::syntax::SQLITE`.

use super::{column_value, map_error};
use crate::error::AppError;
use crate::modules::db::drivers::sql::{statements::split_statements, syntax::SQLITE};
use crate::modules::db::models::{SqlProblem, StatementResult};
use futures_util::StreamExt;
use serde_json::Value;
use sqlx::{Column, Either, Executor, Row, SqlitePool};
use std::time::Instant;

/// How many rows of one result set are read back — as on the other two, a ceiling rather than a
/// promise.
const MAX_ROWS: usize = 10_000;

/// The statements whose point is the number of rows they changed, so that a count of zero is still
/// worth reporting as a count.
///
/// `REPLACE` is SQLite's own, and is `INSERT OR REPLACE` under a shorter name. There is no
/// `TRUNCATE` and no `MERGE`.
fn is_write_verb(verb: &str) -> bool {
    matches!(verb, "INSERT" | "UPDATE" | "DELETE" | "REPLACE")
}

/// Asks SQLite what it makes of one statement, **without running it**.
///
/// The statement is prepared and the prepared statement immediately thrown away, so nothing it
/// would do happens — this is safe to fire at a half-typed `DELETE`.
///
/// SQLite reports a bad name and bad syntax under the same error code, so which of the two it is
/// has to be read off the message. `near "…": syntax error` is the only thing it says that is
/// certainly the user's mistake; everything else is reported as a warning, because a temporary
/// table or a `PRAGMA` from earlier in the script is invisible from a prepare that runs on its own
/// connection.
pub async fn validate(pool: &SqlitePool, sql: &str) -> Result<Option<SqlProblem>, AppError> {
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
    let message = db.message().to_string();
    let syntax = message.contains("syntax error");

    Ok(Some(SqlProblem {
        message,
        number: 0,
        /* SQLite reports the offending token but not where it is, so there is no line to point at.
        The editor draws the warning across the whole statement when this is `None`. */
        line: None,
        severity: if syntax { "error" } else { "warning" }.to_string(),
    }))
}

/// Runs a script, statement by statement, on one connection.
///
/// The connection is closed when the script is done rather than going back to the pool carrying
/// whatever the script left on it: an unfinished `BEGIN` holds a write lock on the file, and every
/// later statement from any tab would then wait on it until the busy timeout gave up. A `PRAGMA` a
/// script sets survives a returned connection too.
///
/// There is no `announce` and no pid, unlike the other two: nothing here can be cancelled from
/// outside.
pub async fn run(pool: &SqlitePool, sql: &str) -> Result<Vec<StatementResult>, AppError> {
    let statements = split_statements(sql, &SQLITE);
    if statements.is_empty() {
        return Err(err!("error.nothingToRun"));
    }

    let mut conn = pool.acquire().await.map_err(map_error)?;
    conn.close_on_drop();

    let mut results: Vec<StatementResult> = Vec::new();

    for statement in statements {
        let started = Instant::now();
        let mut columns: Vec<String> = Vec::new();
        let mut rows: Vec<Vec<Value>> = Vec::new();
        let mut truncated = false;
        let mut rows_affected = 0u64;
        let mut last_insert_id: Option<u64> = None;
        let mut failure: Option<String> = None;

        // Scoped so the stream lets go of the connection before the next statement takes it.
        {
            let mut stream =
                sqlx::raw_sql(sqlx::AssertSqlSafe(statement.text.clone())).fetch_many(&mut *conn);
            while let Some(item) = stream.next().await {
                match item {
                    Ok(Either::Right(row)) => {
                        if columns.is_empty() {
                            columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                        }
                        if rows.len() < MAX_ROWS {
                            rows.push(
                                (0..row.columns().len())
                                    .map(|i| column_value(&row, i))
                                    .collect(),
                            );
                        } else {
                            truncated = true;
                        }
                    }
                    Ok(Either::Left(done)) => {
                        rows_affected = done.rows_affected();
                        /* The rowid of the row this statement inserted. Reported only for an
                        INSERT: SQLite keeps the last one on the connection, so after an UPDATE
                        it would still hold whatever the INSERT before it left there. */
                        if statement.verb == "INSERT" || statement.verb == "REPLACE" {
                            let rowid = done.last_insert_rowid();
                            if rowid > 0 {
                                last_insert_id = Some(rowid as u64);
                            }
                        }
                    }
                    Err(e) => {
                        failure = Some(
                            e.as_database_error()
                                .map(|db| db.message().to_string())
                                .unwrap_or_else(|| e.to_string()),
                        );
                        break;
                    }
                }
            }
        }

        let kind = if failure.is_some() {
            "error"
        } else if !columns.is_empty() {
            "rows"
        } else if is_write_verb(&statement.verb) {
            "affected"
        } else {
            "ok"
        };
        let failed = failure.is_some();
        results.push(StatementResult {
            statement: statement.text,
            verb: statement.verb,
            kind: kind.to_string(),
            columns,
            rows,
            truncated,
            rows_affected,
            last_insert_id,
            duration_ms: started.elapsed().as_millis() as u64,
            error: failure,
        });
        // A failed statement stops the script, the way it would in a command-line client.
        if failed {
            break;
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(sql: &str) -> Vec<(String, String)> {
        split_statements(sql, &SQLITE)
            .into_iter()
            .map(|s| (s.verb, s.text))
            .collect()
    }

    #[test]
    fn a_semicolon_inside_a_string_does_not_split() {
        let statements = split("select 'a;b'; select 2");
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].1, "select 'a;b'");
    }

    #[test]
    fn a_backslash_before_a_quote_does_not_escape_it() {
        /* MySQL's splitter would read `'a\'` as an unterminated string and swallow the rest. In
        SQLite a backslash is an ordinary character and the literal ends at that quote, so this
        is two statements. */
        let statements = split(r"select 'a\'; select 2");
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[1].0, "SELECT");
    }

    #[test]
    fn a_doubled_quote_is_an_escaped_one() {
        let statements = split("select 'it''s; here'; select 2");
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].1, "select 'it''s; here'");
    }

    #[test]
    fn both_identifier_quotes_are_understood() {
        // SQLite takes the standard double quote and MySQL's backtick alike, so a semicolon inside
        // either has to be left where it is.
        assert_eq!(split(r#"select "a;b" from t; select 2"#).len(), 2);
        assert_eq!(split("select `a;b` from t; select 2").len(), 2);
    }

    #[test]
    fn a_dash_comment_needs_no_space_after_it() {
        // Unlike MySQL, where `--` only opens a comment when whitespace follows.
        let statements = split("select 1 --3;\nselect 2");
        assert_eq!(statements.len(), 1);
        assert_eq!(statements[0].1, "select 1 --3;\nselect 2");
    }

    #[test]
    fn a_block_comment_does_not_nest() {
        // The first close ends it, so the `;` after it separates two statements.
        let statements = split("select /* a /* b */ 1; select 2");
        assert_eq!(statements.len(), 2);
    }

    #[test]
    fn comments_are_kept_and_a_run_of_them_alone_is_not_a_statement() {
        assert!(split("-- nothing here\n").is_empty());
        assert!(split("  ;  ; ").is_empty());
        let statements = split("/* keep me */ select 1");
        assert_eq!(statements[0].1, "/* keep me */ select 1");
    }

    #[test]
    fn the_verb_is_the_opening_word() {
        let statements = split("  insert into t values (1)");
        assert_eq!(statements[0].0, "INSERT");
    }
}

#[cfg(test)]
mod run_tests {
    use super::super::tests::Fixture;
    use super::*;

    #[tokio::test]
    async fn a_script_runs_statement_by_statement_and_reports_each() {
        let (_fixture, pool) = Fixture::open().await;
        let results = run(
            &pool,
            "select 1 as n; update post set views = 0; select count(*) from post",
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].kind, "rows");
        assert_eq!(results[0].columns, vec!["n"]);
        // A write reports what it changed, even when that is zero.
        assert_eq!(results[1].kind, "affected");
        assert_eq!(results[1].rows_affected, 3);
        assert_eq!(results[2].kind, "rows");
    }

    #[tokio::test]
    async fn an_insert_reports_the_rowid_it_created() {
        let (_fixture, pool) = Fixture::open().await;
        let results = run(&pool, "insert into author (name) values ('Barbara')")
            .await
            .unwrap();
        assert_eq!(results[0].last_insert_id, Some(3));
    }

    #[tokio::test]
    async fn an_update_does_not_report_the_insert_before_it() {
        let (_fixture, pool) = Fixture::open().await;
        // SQLite keeps the last rowid on the connection, so an UPDATE that reported it would be
        // reporting whatever the statement before it left there.
        let results = run(
            &pool,
            "insert into author (name) values ('Barbara'); update author set bio = 'x'",
        )
        .await
        .unwrap();
        assert_eq!(results[0].last_insert_id, Some(3));
        assert_eq!(results[1].last_insert_id, None);
    }

    #[tokio::test]
    async fn a_failed_statement_stops_the_script_and_keeps_what_came_before() {
        let (_fixture, pool) = Fixture::open().await;
        let results = run(&pool, "select 1; select * from nowhere; select 2")
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].kind, "error");
        assert!(results[1].error.is_some());
    }

    #[tokio::test]
    async fn an_empty_script_says_so() {
        let (_fixture, pool) = Fixture::open().await;
        assert_eq!(
            run(&pool, "  -- nothing\n")
                .await
                .expect_err("nothing to run")
                .code,
            "error.nothingToRun"
        );
    }

    #[tokio::test]
    async fn a_transaction_left_open_does_not_follow_the_connection_back_to_the_pool() {
        let (_fixture, pool) = Fixture::open().await;
        run(&pool, "begin; update post set views = 1")
            .await
            .unwrap();

        /* The script's connection is closed rather than returned, so the write lock that unfinished
        BEGIN is holding goes with it. Without that, this next write would sit on the busy
        timeout and then fail — from another tab, for no reason the user could see. */
        let results = run(&pool, "update post set views = 2").await.unwrap();
        assert_eq!(results[0].rows_affected, 3);
    }

    #[tokio::test]
    async fn bad_syntax_is_an_error_and_a_bad_name_is_only_a_warning() {
        let (_fixture, pool) = Fixture::open().await;

        let syntax = validate(&pool, "selec 1")
            .await
            .unwrap()
            .expect("a problem");
        assert_eq!(syntax.severity, "error");

        /* A name the prepare cannot see may still exist by the time the script reaches it — a
        temporary table made by an earlier statement, say — so this is the softer of the two. */
        let name = validate(&pool, "select * from nowhere")
            .await
            .unwrap()
            .expect("a problem");
        assert_eq!(name.severity, "warning");

        assert!(validate(&pool, "select * from post")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn checking_a_statement_does_not_run_it() {
        let (_fixture, pool) = Fixture::open().await;
        validate(&pool, "delete from post").await.unwrap();
        let left: i64 = sqlx::query_scalar("select count(*) from post")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 3);
    }
}
