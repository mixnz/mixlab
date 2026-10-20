//! Editing a table's rows from the grid, for the engines that reach their server through sqlx:
//! MySQL, PostgreSQL and SQLite. Each engine writes the SQL of the statements, as a [`RowSql`];
//! this is where they run, inside one transaction, so that a save either lands whole or not at
//! all.
//!
//! Every value is bound the same way: the grid only ever sends the text someone typed or an
//! explicit null, bound as `Option<&str>`, and the server brings it to the column's type. The order
//! of the binds is fixed here, and each engine's SQL is written to it: a count binds the key's
//! values, an update the updates' then the key's, an insert the row's, a delete the key's.
//!
//! What stays with each engine is what it reads before the transaction, the calls that have nothing
//! to do, and putting a table's counter back after a delete. SQL Server runs on tiberius and has
//! its own `mssql/editing.rs`.

use crate::error::AppError;
use serde_json::{Map, Value};
use sqlx::{ColumnIndex, Database, Decode, Encode, Executor, IntoArguments, Pool, Type};

#[cfg(test)]
mod statements;

/// The SQL an engine writes for the grid's edits to one table. Each method's doc says which values
/// its statement binds, in order.
pub trait RowSql: Sync {
    type Database: Database;

    /// `SELECT COUNT(*)` of the rows `key` matches; binds the key's values.
    fn count(&self, key: &Map<String, Value>) -> String;
    /// The `UPDATE`; binds the updates' values, then the key's.
    fn update(&self, updates: &Map<String, Value>, key: &Map<String, Value>) -> String;
    /// One row's `INSERT`, `row` possibly empty; binds the row's values.
    fn insert(&self, row: &Map<String, Value>) -> String;
    /// The `DELETE` of the one row `key` names; binds the key's values.
    fn delete_one(&self, key: &Map<String, Value>) -> String;
    /// `DELETE FROM` the table, binding nothing.
    fn delete_all(&self) -> String;
    /// The engine's own `map_error`.
    fn map_error(&self, error: sqlx::Error) -> AppError;
}

/// What is bound for one value: its text, or a null for anything that is not a string.
fn text(value: &Value) -> Option<&str> {
    match value {
        Value::String(s) => Some(s.as_str()),
        _ => None,
    }
}

/// Writes `updates` into the one row `key` names.
///
/// The row is counted first, inside the same transaction as the write: a key that matches two rows
/// — which only a table with no primary key can produce — would otherwise update both, and the
/// grid would show one row changing while another silently changed with it.
pub async fn update_row<R, DB>(
    pool: &Pool<DB>,
    sql: &R,
    updates: &Map<String, Value>,
    key: &Map<String, Value>,
) -> Result<(), AppError>
where
    R: RowSql<Database = DB>,
    DB: Database,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
    for<'t> Option<&'t str>: Encode<'t, DB> + Type<DB>,
    DB::Arguments: IntoArguments<DB>,
    for<'r> i64: Decode<'r, DB> + Type<DB>,
    usize: ColumnIndex<DB::Row>,
{
    let mut tx = pool.begin().await.map_err(|e| sql.map_error(e))?;

    let mut count_query = sqlx::query_scalar::<DB, i64>(sqlx::AssertSqlSafe(sql.count(key)));
    for value in key.values() {
        count_query = count_query.bind(text(value));
    }
    let matched = count_query
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| sql.map_error(e))?;
    if matched != 1 {
        tx.rollback().await.map_err(|e| sql.map_error(e))?;
        return Err(err!("error.rowsMatched", matched = matched));
    }

    let mut update_query = sqlx::query::<DB>(sqlx::AssertSqlSafe(sql.update(updates, key)));
    for value in updates.values().chain(key.values()) {
        update_query = update_query.bind(text(value));
    }
    if let Err(e) = update_query.execute(&mut *tx).await {
        tx.rollback().await.map_err(|e| sql.map_error(e))?;
        return Err(sql.map_error(e));
    }

    tx.commit().await.map_err(|e| sql.map_error(e))?;
    Ok(())
}

/// Inserts `rows`, one statement each, all in one transaction: one rejected row means none of them
/// land, and the error says which row it was.
///
/// A row only carries the columns it has something to say about; a column left out of its map is
/// left out of its `INSERT`, so the table's own default fills it. That is also why each row is its
/// own statement rather than one multi-`VALUES` insert: rows may name different sets of columns.
pub async fn insert_rows<R, DB>(
    pool: &Pool<DB>,
    sql: &R,
    rows: &[Map<String, Value>],
) -> Result<(), AppError>
where
    R: RowSql<Database = DB>,
    DB: Database,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
    for<'t> Option<&'t str>: Encode<'t, DB> + Type<DB>,
    DB::Arguments: IntoArguments<DB>,
{
    let mut tx = pool.begin().await.map_err(|e| sql.map_error(e))?;

    for (i, row) in rows.iter().enumerate() {
        let mut query = sqlx::query::<DB>(sqlx::AssertSqlSafe(sql.insert(row)));
        for value in row.values() {
            query = query.bind(text(value));
        }
        if let Err(e) = query.execute(&mut *tx).await {
            tx.rollback().await.map_err(|e| sql.map_error(e))?;
            return Err(err!("error.rowFailed", index = i + 1).caused_by(sql.map_error(e)));
        }
    }

    tx.commit().await.map_err(|e| sql.map_error(e))?;
    Ok(())
}

/// Deletes the rows `keys` names, or every row when `all` is set, in one transaction: if any of
/// them fails, none of them land. A key with no columns is refused, and the deletes before it are
/// rolled back with it.
pub async fn delete_rows<R, DB>(
    pool: &Pool<DB>,
    sql: &R,
    keys: &[Map<String, Value>],
    all: bool,
) -> Result<(), AppError>
where
    R: RowSql<Database = DB>,
    DB: Database,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
    for<'t> Option<&'t str>: Encode<'t, DB> + Type<DB>,
    DB::Arguments: IntoArguments<DB>,
{
    let mut tx = pool.begin().await.map_err(|e| sql.map_error(e))?;

    if all {
        sqlx::query::<DB>(sqlx::AssertSqlSafe(sql.delete_all()))
            .execute(&mut *tx)
            .await
            .map_err(|e| sql.map_error(e))?;
    } else {
        for key in keys {
            if key.is_empty() {
                tx.rollback().await.map_err(|e| sql.map_error(e))?;
                return Err(err!("error.deleteWithoutKey"));
            }
            let mut query = sqlx::query::<DB>(sqlx::AssertSqlSafe(sql.delete_one(key)));
            for value in key.values() {
                query = query.bind(text(value));
            }
            query
                .execute(&mut *tx)
                .await
                .map_err(|e| sql.map_error(e))?;
        }
    }

    tx.commit().await.map_err(|e| sql.map_error(e))?;
    Ok(())
}
