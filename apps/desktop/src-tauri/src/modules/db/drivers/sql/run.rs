//! Running a split script statement by statement on one sqlx connection, for the engines whose
//! statements each come back as one result: PostgreSQL and SQLite.
//!
//! MySQL runs its own loop, because a stored procedure answers one statement with several result
//! sets and MySQL reports each as a result of its own. SQL Server runs on tiberius and ClickHouse
//! over HTTP, so neither has a sqlx connection to hand this.

use super::statements::Statement;
use crate::modules::db::models::StatementResult;
use futures_util::StreamExt;
use serde_json::Value;
use sqlx::{Column, Database, Either, Executor, Row};
use std::time::Instant;

/// How many rows of one result set are read back. A query without a LIMIT can name more rows than
/// there is memory for, so the client stops here and says that it did.
///
/// Ten thousand rather than the thousand this was: the results grid holds only the rows on screen
/// now, so the cost of a large set is what it takes to decode and hand over, not what it takes to
/// draw. This is still a ceiling and not a promise; the auto-LIMIT in the Query tab is what most
/// scripts actually stop at. Every SQL engine holds a script's result set to it.
pub const MAX_ROWS: usize = 10_000;

/// What an engine supplies to [`run_statements`]: the parts of reading a result that differ.
pub struct ScriptRows<DB: Database> {
    /// One cell of a row, as JSON.
    pub column_value: fn(&DB::Row, usize) -> Value,
    /// The verbs whose point is the number of rows they changed, so that a count of zero is still
    /// worth reporting as a count.
    pub is_write_verb: fn(&str) -> bool,
    /// Rows affected and the generated key, read off a finished statement given its verb. A hook
    /// because sqlx has no trait method for either on `QueryResult`.
    pub done: fn(&DB::QueryResult, &str) -> (u64, Option<u64>),
}

/// Runs `statements` one after another on `conn` and reports each one's outcome.
///
/// A statement that fails stops the script, the way it would in a command-line client: its own
/// result carries the server's message, and the results before it are kept.
pub async fn run_statements<DB>(
    conn: &mut DB::Connection,
    statements: Vec<Statement>,
    rows: &ScriptRows<DB>,
) -> Vec<StatementResult>
where
    DB: Database,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
{
    let mut results: Vec<StatementResult> = Vec::new();

    for statement in statements {
        let started = Instant::now();
        let mut columns: Vec<String> = Vec::new();
        let mut cells: Vec<Vec<Value>> = Vec::new();
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
                        if cells.len() < MAX_ROWS {
                            cells.push(
                                (0..row.columns().len())
                                    .map(|i| (rows.column_value)(&row, i))
                                    .collect(),
                            );
                        } else {
                            truncated = true;
                        }
                    }
                    Ok(Either::Left(done)) => {
                        (rows_affected, last_insert_id) = (rows.done)(&done, &statement.verb);
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
        } else if (rows.is_write_verb)(&statement.verb) {
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
            rows: cells,
            truncated,
            rows_affected,
            last_insert_id,
            duration_ms: started.elapsed().as_millis() as u64,
            error: failure,
        });
        if failed {
            break;
        }
    }

    results
}
