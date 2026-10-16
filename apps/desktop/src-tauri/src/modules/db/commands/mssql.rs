//! SQL Server
//!
//! `database` here means what it means on MySQL and not what it means on PostgreSQL: a database to
//! reach into over the one pool, never a pool to pick. See `mssql_pool`.

use crate::error::AppError;
use crate::modules::db::drivers::{dump, mssql};
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use crate::modules::db::state::DbState;
use serde_json::{Map, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

use super::handle;
use super::{mssql_pool, reporter, RunningQuery, Transfer};

#[tauri::command]
pub async fn mssql_list_databases(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<String>, AppError> {
    handle(&state, &id).await?.sql()?.list_databases().await
}

#[tauri::command]
pub async fn mssql_server_info(
    state: State<'_, DbState>,
    id: String,
) -> Result<ServerInfo, AppError> {
    handle(&state, &id).await?.sql()?.server_info().await
}

#[tauri::command]
pub async fn mssql_list_tables(
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

/// Not `retry_read!`: these three write, and `retry_read!` only bothers to bound because a SELECT
/// tried again after failing halfway is harmless — an INSERT/UPDATE/DELETE tried again the same way
/// could write twice. `postgres_update_row`/`mysql_update_row` skip it for the same reason.
#[tauri::command]
pub async fn mssql_update_row(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    updates: Map<String, Value>,
    key: Map<String, Value>,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .update_row(database, table, updates, key)
        .await
}

#[tauri::command]
pub async fn mssql_insert_rows(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    rows: Vec<Map<String, Value>>,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .insert_rows(database, table, rows)
        .await
}

/// `reset_auto_increment` keeps MySQL's name, the way `commands::postgres` does too — it is the
/// frontend's name for the checkbox; what it resets here is the table's IDENTITY seed.
#[tauri::command]
pub async fn mssql_delete_rows(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    keys: Vec<Map<String, Value>>,
    all: bool,
    reset_auto_increment: bool,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .delete_rows(database, table, keys, all, reset_auto_increment)
        .await
}

#[tauri::command]
pub async fn mssql_table_data(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    query: mssql::PageQuery,
) -> Result<mssql::TablePage, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_data(database, table, query)
        .await
}

#[tauri::command]
pub async fn mssql_table_structure(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<mssql::structure::TableStructure, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_structure(database, table)
        .await
}

#[tauri::command]
pub async fn mssql_table_stats(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<mssql::structure::TableStats>, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_stats(database)
        .await
}

#[tauri::command]
pub async fn mssql_collations(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<mssql::structure::Collation>, AppError> {
    handle(&state, &id).await?.sql()?.collations().await
}

#[tauri::command]
pub async fn mssql_schema_outline(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<mssql::structure::SchemaOutline, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .schema_outline(database)
        .await
}

/// Runs the Query tab's text against SQL Server, batch by batch and statement by statement within
/// each — see `mssql::script::run`. `USE database` runs before the first statement so an unqualified
/// table name resolves the way it does everywhere else in the workspace, matching MySQL's connection
/// model (D2): one pool for the whole server, not one per database.
#[tauri::command]
pub async fn mssql_run_script(
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

/// Stops the run named by `run_id`, if it is still running — see `mssql::script::cancel` for why
/// this ends the whole session rather than just the statement (D8), and `DbState::running_queries`
/// for the map this reads.
#[tauri::command]
pub async fn mssql_cancel_query(
    state: State<'_, DbState>,
    id: String,
    run_id: String,
) -> Result<(), AppError> {
    let session = state.running_queries.lock().unwrap().get(&run_id).copied();
    let Some(session) = session else {
        return Ok(());
    };
    handle(&state, &id)
        .await?
        .sql()?
        .cancel_query(session)
        .await
}

/// Asks SQL Server to parse one statement without running it, for the editor's error checking.
#[tauri::command]
pub async fn mssql_validate_sql(
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
pub async fn mssql_create_database(
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
pub async fn mssql_drop_database(
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
pub async fn mssql_create_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    #[allow(unused_variables)] collation: Option<String>,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .create_table(database, table, collation, None)
        .await
}

#[tauri::command]
pub async fn mssql_rename_table(
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
pub async fn mssql_drop_table(
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
pub async fn mssql_add_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: crate::modules::db::drivers::sql::types::ColumnSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .add_column(database, table, spec)
        .await
}

#[tauri::command]
pub async fn mssql_drop_column(
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

#[tauri::command]
pub async fn mssql_modify_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: crate::modules::db::drivers::sql::types::ColumnSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .modify_column(database, table, name, spec)
        .await
}

#[tauri::command]
pub async fn mssql_add_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: crate::modules::db::drivers::sql::types::IndexSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .add_index(database, table, spec)
        .await
}

#[tauri::command]
pub async fn mssql_modify_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: crate::modules::db::drivers::sql::types::IndexSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .modify_index(database, table, name, spec)
        .await
}

#[tauri::command]
pub async fn mssql_drop_index(
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

/// Writes `database` out as SQL — `mode` is `structure`, `data` or `all`. See
/// `docs/specs/2026-09-05-mssql-support-design.md`'s D10 and
/// `2026-09-05-mssql-plan-7-dump-restore.md` (a local plan, never committed).
#[tauri::command]
pub async fn mssql_dump(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    mode: String,
    path: String,
) -> Result<(), AppError> {
    let mode = dump::DumpMode::parse(&mode)?;
    let pool = mssql_pool(&state, &id).await?;
    let report = reporter(&app, &id);
    let transfer = Transfer::start(&state, &id);
    let cancelled = transfer.flag();
    let watch = dump::Watch {
        report: &report,
        cancel: &|| cancelled.load(Ordering::Relaxed),
    };

    if mode != dump::DumpMode::Data {
        mssql::dump::dump_structure(&pool, &database, &path, &watch).await?;
    }
    if mode != dump::DumpMode::Structure {
        let append = mode == dump::DumpMode::All;
        mssql::dump::dump_data(&pool, &database, &path, append, &watch).await?;
    }
    Ok(())
}

/// Replays a dump file into `database`. No progress in between — see the plan's Task 5 — only one
/// reading before `mssql::script::run` starts, so the overlay shows movement rather than sitting
/// frozen on an empty bar, the same as `sqlite_restore`.
#[allow(unused_variables)]
#[tauri::command]
pub async fn mssql_restore(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    path: String,
) -> Result<(), AppError> {
    let pool = mssql_pool(&state, &id).await?;
    let report = reporter(&app, &id);
    let _transfer = Transfer::start(&state, &id);
    report(dump::Progress::default());
    mssql::dump::restore(&pool, &database, &path).await
}
