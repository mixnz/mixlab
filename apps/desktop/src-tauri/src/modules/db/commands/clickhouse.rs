use super::{clickhouse_connection, reporter, Transfer};
use crate::error::AppError;
use crate::modules::db::drivers::{
    clickhouse, clickhouse_ddl, clickhouse_dump, clickhouse_script, clickhouse_structure, dump,
};
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
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse::server_info(&conn).await
}

#[tauri::command]
pub async fn clickhouse_list_databases(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<String>, AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse::list_databases(&conn).await
}

#[tauri::command]
pub async fn clickhouse_list_tables(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<String>, AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse::list_tables(&conn, &database).await
}

#[tauri::command]
pub async fn clickhouse_table_data(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    query: clickhouse::PageQuery,
) -> Result<clickhouse::TablePage, AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse::table_data(&conn, &database, &table, &query).await
}

#[tauri::command]
pub async fn clickhouse_table_structure(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<clickhouse_structure::TableStructure, AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_structure::table_structure(&conn, &database, &table).await
}

#[tauri::command]
pub async fn clickhouse_table_stats(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<clickhouse_structure::TableStats>, AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_structure::table_stats(&conn, &database).await
}

#[tauri::command]
pub async fn clickhouse_schema_outline(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<clickhouse_structure::SchemaOutline, AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_structure::schema_outline(&conn, &database).await
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
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_script::run(&conn, &sql, database.as_deref()).await
}

#[tauri::command]
pub async fn clickhouse_validate_sql(
    state: State<'_, DbState>,
    id: String,
    sql: String,
    database: Option<String>,
) -> Result<Option<SqlProblem>, AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_script::validate(&conn, &sql, database.as_deref()).await
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
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse::update_row(&conn, &database, &table, &updates, &key).await
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
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse::delete_rows(&conn, &database, &table, &keys, all, reset_auto_increment).await
}

#[tauri::command]
pub async fn clickhouse_insert_rows(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    rows: Vec<Map<String, Value>>,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse::insert_rows(&conn, &database, &table, &rows).await
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
    clickhouse_ddl::create_database(&conn, &name).await
}

#[tauri::command]
pub async fn clickhouse_drop_database(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::drop_database(&conn, &database).await
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
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::create_table(&conn, &database, &table, &engine).await
}

#[tauri::command]
pub async fn clickhouse_rename_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    new_name: String,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::rename_table(&conn, &database, &table, &new_name).await
}

#[tauri::command]
pub async fn clickhouse_drop_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::drop_table(&conn, &database, &table).await
}

#[tauri::command]
pub async fn clickhouse_add_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: clickhouse_ddl::ColumnSpec,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::add_column(&conn, &database, &table, &spec).await
}

#[tauri::command]
pub async fn clickhouse_drop_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::drop_column(&conn, &database, &table, &name).await
}

#[tauri::command]
pub async fn clickhouse_modify_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: clickhouse_ddl::ColumnSpec,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::modify_column(&conn, &database, &table, &name, &spec).await
}

#[tauri::command]
pub async fn clickhouse_add_skip_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: clickhouse_ddl::SkipIndexSpec,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::add_skip_index(&conn, &database, &table, &spec).await
}

#[tauri::command]
pub async fn clickhouse_modify_skip_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: clickhouse_ddl::SkipIndexSpec,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::modify_skip_index(&conn, &database, &table, &name, &spec).await
}

#[tauri::command]
pub async fn clickhouse_drop_skip_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
) -> Result<(), AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::drop_skip_index(&conn, &database, &table, &name).await
}

#[tauri::command]
pub async fn clickhouse_rebuild_order_by(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    columns: Vec<String>,
) -> Result<Option<String>, AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::rebuild_order_by(&conn, &database, &table, &columns).await
}

#[tauri::command]
pub async fn clickhouse_row_count(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<u64, AppError> {
    let conn = clickhouse_connection(&state, &id).await?;
    clickhouse_ddl::row_count(&conn, &database, &table).await
}

/// Writes `database` out as SQL — `mode` is `structure`, `data` or `all`. See
/// `docs/specs/2026-09-04-clickhouse-dump-restore-design.md`.
#[tauri::command]
pub async fn clickhouse_dump(
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
        clickhouse_dump::dump_structure(&conn, &database, &path, &watch).await?;
    }
    if mode != dump::DumpMode::Structure {
        let append = mode == dump::DumpMode::All;
        clickhouse_dump::dump_data(&conn, &database, &path, append, &watch).await?;
    }
    Ok(())
}

/// Replays a dump file into `database`.
#[tauri::command]
pub async fn clickhouse_restore(
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
    clickhouse_dump::restore(&conn, &database, &path, &watch).await
}
