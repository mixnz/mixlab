//! The SQL engines' commands: one per operation, whichever engine the connection reaches
//! (ADR 0062). Each looks its connection up, turns it into a `SqlEngine` with `DbHandle::sql`, and
//! asks it. Dump and restore are the exception: they drive the vendors' tools or a transfer of
//! their own, so they match the connection once and hand it to that engine's orchestration.

use super::{clickhouse, handle, mssql, mysql, postgres, sqlite, RunningQuery};
use crate::error::AppError;
use crate::modules::db::drivers::clickhouse::ddl::SkipIndexSpec;
use crate::modules::db::drivers::sql::engine::Row;
use crate::modules::db::drivers::sql::types::{
    Collation, ColumnSpec, IndexSpec, PageQuery, SchemaOutline, TablePage, TableStats,
    TableStructure,
};
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use crate::modules::db::state::{DbHandle, DbState};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn sql_list_databases(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<String>, AppError> {
    handle(&state, &id).await?.sql()?.list_databases().await
}

#[tauri::command]
pub async fn sql_server_info(
    state: State<'_, DbState>,
    id: String,
) -> Result<ServerInfo, AppError> {
    handle(&state, &id).await?.sql()?.server_info().await
}

#[tauri::command]
pub async fn sql_list_tables(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<String>, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .list_tables(database)
        .await
}

/// What every table in the database weighs, for the workspace's Statistics tab.
#[tauri::command]
pub async fn sql_table_stats(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<TableStats>, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_stats(database)
        .await
}

#[tauri::command]
pub async fn sql_table_data(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    query: PageQuery,
) -> Result<TablePage, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_data(database, table, query)
        .await
}

#[tauri::command]
pub async fn sql_table_structure(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<TableStructure, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_structure(database, table)
        .await
}

/// Every table and column of one database, for the Query tab's completion.
#[tauri::command]
pub async fn sql_schema_outline(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<SchemaOutline, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .schema_outline(database)
        .await
}

#[tauri::command]
pub async fn sql_update_row(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    updates: Row,
    key: Row,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .update_row(database, table, updates, key)
        .await
}

#[tauri::command]
pub async fn sql_insert_rows(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    rows: Vec<Row>,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .insert_rows(database, table, rows)
        .await
}

#[tauri::command]
pub async fn sql_delete_rows(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    keys: Vec<Row>,
    all: bool,
    reset_auto_increment: bool,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .delete_rows(database, table, keys, all, reset_auto_increment)
        .await
}

/// `collation` is read by the engines that have one, `engine` by ClickHouse alone.
#[tauri::command]
pub async fn sql_create_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    collation: Option<String>,
    engine: Option<String>,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .create_table(database, table, collation, engine)
        .await
}

#[tauri::command]
pub async fn sql_rename_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    new_name: String,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .rename_table(database, table, new_name)
        .await
}

#[tauri::command]
pub async fn sql_drop_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .drop_table(database, table)
        .await
}

#[tauri::command]
pub async fn sql_add_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: ColumnSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .add_column(database, table, spec)
        .await
}

#[tauri::command]
pub async fn sql_modify_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: ColumnSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .modify_column(database, table, name, spec)
        .await
}

#[tauri::command]
pub async fn sql_drop_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .drop_column(database, table, name)
        .await
}

/// Asks the server what it makes of one statement without running it, for the editor's error
/// checking.
#[tauri::command]
pub async fn sql_validate_sql(
    state: State<'_, DbState>,
    id: String,
    sql: String,
    database: Option<String>,
) -> Result<Option<SqlProblem>, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .validate_sql(sql, database)
        .await
}

#[tauri::command]
pub async fn sql_create_database(
    state: State<'_, DbState>,
    id: String,
    name: String,
    collation: Option<String>,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .create_database(name, collation)
        .await
}

#[tauri::command]
pub async fn sql_drop_database(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .drop_database(database)
        .await
}

#[tauri::command]
pub async fn sql_collations(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<Collation>, AppError> {
    handle(&state, &id).await?.sql()?.collations().await
}

#[tauri::command]
pub async fn sql_add_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: IndexSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .add_index(database, table, spec)
        .await
}

#[tauri::command]
pub async fn sql_modify_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: IndexSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .modify_index(database, table, name, spec)
        .await
}

#[tauri::command]
pub async fn sql_drop_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .drop_index(database, table, name)
        .await
}

#[tauri::command]
pub async fn sql_add_skip_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: SkipIndexSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .add_skip_index(database, table, spec)
        .await
}

#[tauri::command]
pub async fn sql_modify_skip_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: SkipIndexSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .modify_skip_index(database, table, name, spec)
        .await
}

#[tauri::command]
pub async fn sql_drop_skip_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .drop_skip_index(database, table, name)
        .await
}

#[tauri::command]
pub async fn sql_rebuild_order_by(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    columns: Vec<String>,
) -> Result<Option<String>, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .rebuild_order_by(database, table, columns)
        .await
}

#[tauri::command]
pub async fn sql_row_count(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<u64, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .row_count(database, table)
        .await
}

/// Runs a script, one statement at a time, and keeps the server's session id for the run so that
/// `sql_cancel_query` can stop it.
#[tauri::command]
pub async fn sql_run_script(
    state: State<'_, DbState>,
    id: String,
    run_id: String,
    sql: String,
    database: Option<String>,
) -> Result<Vec<StatementResult>, AppError> {
    let engine = handle(&state, &id).await?.sql()?;
    // However it ended, dropping this is what forgets the session id.
    let _running = RunningQuery::start(&state, &run_id);
    let announce = |session: u64| {
        state
            .running_queries
            .lock()
            .unwrap()
            .insert(run_id.clone(), session);
    };
    engine.run_script(sql, database, &announce).await
}

/// Stops the run named by `run_id`, if it is still running. A run with no session registered has
/// finished, or ran on an engine with nothing to stop, and there is nothing to do.
#[tauri::command]
pub async fn sql_cancel_query(
    state: State<'_, DbState>,
    id: String,
    run_id: String,
) -> Result<(), AppError> {
    let Some(session) = session_of(&state, &run_id) else {
        return Ok(());
    };
    handle(&state, &id)
        .await?
        .sql()?
        .cancel_query(session)
        .await
}

/// The server session `run_id` is running on, while it runs.
fn session_of(state: &DbState, run_id: &str) -> Option<u64> {
    state.running_queries.lock().unwrap().get(run_id).copied()
}

/// Writes a database out to `path`. `mode` is `structure`, `data` or `all`.
#[tauri::command]
pub async fn sql_dump(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    mode: String,
    path: String,
) -> Result<(), AppError> {
    match handle(&state, &id).await? {
        DbHandle::Mysql { .. } => mysql::dump(app, state, id, database, mode, path).await,
        DbHandle::Postgres(_) => postgres::dump(app, state, id, database, mode, path).await,
        DbHandle::Mssql(_) => mssql::dump(app, state, id, database, mode, path).await,
        DbHandle::Clickhouse(_) => clickhouse::dump(app, state, id, database, mode, path).await,
        DbHandle::Sqlite(_) => sqlite::dump(app, state, id, database, mode, path).await,
        DbHandle::Mongo(_) | DbHandle::Redis(_) => Err(err!("error.notSqlConnection")),
    }
}

/// Replays a file written by `sql_dump`, or any SQL file, into `database`.
#[tauri::command]
pub async fn sql_restore(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    path: String,
) -> Result<(), AppError> {
    match handle(&state, &id).await? {
        DbHandle::Mysql { .. } => mysql::restore(app, state, id, database, path).await,
        DbHandle::Postgres(_) => postgres::restore(app, state, id, database, path).await,
        DbHandle::Mssql(_) => mssql::restore(app, state, id, database, path).await,
        DbHandle::Clickhouse(_) => clickhouse::restore(app, state, id, database, path).await,
        DbHandle::Sqlite(_) => sqlite::restore(app, state, id, database, path).await,
        DbHandle::Mongo(_) | DbHandle::Redis(_) => Err(err!("error.notSqlConnection")),
    }
}

#[cfg(test)]
mod tests;
