use super::handle;
use super::{clickhouse_connection, reporter, RunningQuery, Transfer};
use crate::error::AppError;
use crate::modules::db::drivers::{clickhouse, dump};
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use crate::modules::db::state::DbState;
use serde_json::{Map, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn clickhouse_server_info(
    state: State<'_, DbState>,
    id: String,
) -> Result<ServerInfo, AppError> {
    handle(&state, &id).await?.sql()?.server_info().await
}

#[tauri::command]
pub async fn clickhouse_list_databases(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<String>, AppError> {
    handle(&state, &id).await?.sql()?.list_databases().await
}

#[tauri::command]
pub async fn clickhouse_list_tables(
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

#[tauri::command]
pub async fn clickhouse_table_data(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    query: clickhouse::PageQuery,
) -> Result<clickhouse::TablePage, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_data(database, table, query)
        .await
}

#[tauri::command]
pub async fn clickhouse_table_structure(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<clickhouse::structure::TableStructure, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_structure(database, table)
        .await
}

#[tauri::command]
pub async fn clickhouse_table_stats(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<clickhouse::structure::TableStats>, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_stats(database)
        .await
}

#[tauri::command]
pub async fn clickhouse_schema_outline(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<clickhouse::structure::SchemaOutline, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .schema_outline(database)
        .await
}

/// `run_id` is taken, like the other three engines', but never stored: there is no
/// `clickhouse_cancel_query` to look it up for, since `cancellable: false` on the dialect closes
/// the button that would call one. Kept in the signature so the frontend has one shape to call
/// through whichever engine it is talking to.
#[tauri::command]
#[allow(unused_variables)]
pub async fn clickhouse_run_script(
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

#[tauri::command]
pub async fn clickhouse_validate_sql(
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
pub async fn clickhouse_update_row(
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
pub async fn clickhouse_delete_rows(
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
pub async fn clickhouse_insert_rows(
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

/// Creates a database, for the header's database picker. `collation` is taken and dropped:
/// ClickHouse has no collation, and `SqlApi.createDatabase` is one signature shared by four engines.
#[tauri::command]
#[allow(unused_variables)]
pub async fn clickhouse_create_database(
    state: State<'_, DbState>,
    id: String,
    name: String,
    collation: Option<String>,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse::ddl::create_database(&conn, &name).await
}

#[tauri::command]
pub async fn clickhouse_drop_database(
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

/// Creates an empty table — one `id UInt64` column and an empty sorting key — for the sidebar's
/// add button. `engine` is the only thing the dialog asks for beyond the name, and the only one of
/// a table's properties ClickHouse cannot change afterwards.
#[tauri::command]
pub async fn clickhouse_create_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    engine: String,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .create_table(database, table, None, Some(engine))
        .await
}

#[tauri::command]
pub async fn clickhouse_rename_table(
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
pub async fn clickhouse_drop_table(
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
pub async fn clickhouse_add_column(
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
pub async fn clickhouse_drop_column(
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
pub async fn clickhouse_modify_column(
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
pub async fn clickhouse_add_skip_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: clickhouse::ddl::SkipIndexSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .add_skip_index(database, table, spec)
        .await
}

#[tauri::command]
pub async fn clickhouse_modify_skip_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: clickhouse::ddl::SkipIndexSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .modify_skip_index(database, table, name, spec)
        .await
}

#[tauri::command]
pub async fn clickhouse_drop_skip_index(
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
pub async fn clickhouse_rebuild_order_by(
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
pub async fn clickhouse_row_count(
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

/// Writes `database` out as SQL — `mode` is `structure`, `data` or `all`. See
/// `docs/specs/2026-09-04-clickhouse-dump-restore-design.md`.
pub(super) async fn dump(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    mode: String,
    path: String,
) -> Result<(), AppError> {
    let mode = dump::DumpMode::parse(&mode)?;
    let conn = clickhouse_connection(&state, &id).await?;
    let report = reporter(&app, &id);
    let transfer = Transfer::start(&state, &id);
    let cancelled = transfer.flag();
    let watch = dump::Watch {
        report: &report,
        cancel: &|| cancelled.load(Ordering::Relaxed),
    };

    if mode != dump::DumpMode::Data {
        clickhouse::dump::dump_structure(&conn, &database, &path, &watch).await?;
    }
    if mode != dump::DumpMode::Structure {
        let append = mode == dump::DumpMode::All;
        clickhouse::dump::dump_data(&conn, &database, &path, append, &watch).await?;
    }
    Ok(())
}

/// Replays a dump file into `database`.
pub(super) async fn restore(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    path: String,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    let report = reporter(&app, &id);
    let transfer = Transfer::start(&state, &id);
    let cancelled = transfer.flag();
    let watch = dump::Watch {
        report: &report,
        cancel: &|| cancelled.load(Ordering::Relaxed),
    };
    clickhouse::dump::restore(&conn, &database, &path, &watch).await
}
