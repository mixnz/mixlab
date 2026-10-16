//! PostgreSQL
//!
//! Every one of these takes the database as an argument the way the MySQL commands do, but it means
//! something different: there it names a database to reach into from the one connection, here it
//! picks which pool the command runs on. See `postgres_pool`.

use super::{in_background, postgres_pool, postgres_pools, reporter, sql_endpoint, tools_dir};
use super::{RunningQuery, Transfer};
use crate::error::AppError;
use crate::modules::db::drivers::{dump, postgres, tools};
use crate::modules::db::models::DbKind;
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use crate::modules::db::state::DbState;
use serde_json::{Map, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn postgres_list_databases(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<String>, AppError> {
    retry_read!({
        let pool = postgres_pool(&state, &id, "").await?;
        postgres::list_databases(&pool).await
    })
}

#[tauri::command]
pub async fn postgres_server_info(
    state: State<'_, DbState>,
    id: String,
) -> Result<ServerInfo, AppError> {
    retry_read!({
        let pool = postgres_pool(&state, &id, "").await?;
        postgres::server_info(&pool).await
    })
}

#[tauri::command]
pub async fn postgres_list_tables(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<String>, AppError> {
    retry_read!({
        let pool = postgres_pool(&state, &id, &database).await?;
        postgres::list_tables(&pool).await
    })
}

#[tauri::command]
pub async fn postgres_table_stats(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<postgres::structure::TableStats>, AppError> {
    retry_read!({
        let pool = postgres_pool(&state, &id, &database).await?;
        postgres::structure::table_stats(&pool).await
    })
}

#[tauri::command]
pub async fn postgres_table_data(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    query: postgres::PageQuery,
) -> Result<postgres::TablePage, AppError> {
    retry_read!({
        let pool = postgres_pool(&state, &id, &database).await?;
        postgres::table_data(&pool, &table, &query).await
    })
}

#[tauri::command]
pub async fn postgres_update_row(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    updates: Map<String, Value>,
    key: Map<String, Value>,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::update_row(&pool, &table, &updates, &key).await
}

#[tauri::command]
pub async fn postgres_insert_rows(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    rows: Vec<Map<String, Value>>,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::insert_rows(&pool, &table, &rows).await
}

/// `reset_auto_increment` keeps MySQL's name because it is the frontend's name for the same
/// checkbox; what it resets here is the table's identity or `serial` sequence.
#[tauri::command]
pub async fn postgres_delete_rows(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    keys: Vec<Map<String, Value>>,
    all: bool,
    reset_auto_increment: bool,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::delete_rows(&pool, &table, &keys, all, reset_auto_increment).await
}

#[tauri::command]
pub async fn postgres_table_structure(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<postgres::structure::TableStructure, AppError> {
    retry_read!({
        let pool = postgres_pool(&state, &id, &database).await?;
        postgres::structure::table_structure(&pool, &table).await
    })
}

#[tauri::command]
pub async fn postgres_collations(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<postgres::structure::Collation>, AppError> {
    retry_read!({
        let pool = postgres_pool(&state, &id, "").await?;
        postgres::structure::collations(&pool).await
    })
}

#[tauri::command]
pub async fn postgres_query(
    state: State<'_, DbState>,
    id: String,
    sql: String,
    database: Option<String>,
) -> Result<Vec<serde_json::Map<String, serde_json::Value>>, AppError> {
    let pool = postgres_pool(&state, &id, database.as_deref().unwrap_or("")).await?;
    postgres::query(&pool, &sql).await
}

#[tauri::command]
pub async fn postgres_schema_outline(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<postgres::structure::SchemaOutline, AppError> {
    retry_read!({
        let pool = postgres_pool(&state, &id, &database).await?;
        postgres::structure::schema_outline(&pool, &database).await
    })
}

#[tauri::command]
pub async fn postgres_run_script(
    state: State<'_, DbState>,
    id: String,
    run_id: String,
    sql: String,
    database: Option<String>,
) -> Result<Vec<StatementResult>, AppError> {
    let pool = postgres_pool(&state, &id, database.as_deref().unwrap_or("")).await?;
    // However it ended, dropping this is what forgets the pid.
    let _running = RunningQuery::start(&state, &run_id);
    postgres::script::run(&pool, &sql, |pid| {
        state
            .running_queries
            .lock()
            .unwrap()
            .insert(run_id.clone(), pid);
    })
    .await
}

/// Asks PostgreSQL to parse one statement without running it, for the editor's error checking.
#[tauri::command]
pub async fn postgres_validate_sql(
    state: State<'_, DbState>,
    id: String,
    sql: String,
    database: Option<String>,
) -> Result<Option<SqlProblem>, AppError> {
    let pool = postgres_pool(&state, &id, database.as_deref().unwrap_or("")).await?;
    postgres::script::validate(&pool, &sql).await
}

/// Stops the run named by `run_id`, if it is still running — see [`DbState::running_queries`].
///
/// The cancel goes out on a connection of its own, since the one being cancelled is busy — and to
/// the same database, because a backend pid is only cancellable from the server it belongs to.
#[tauri::command]
pub async fn postgres_cancel_query(
    state: State<'_, DbState>,
    id: String,
    run_id: String,
    database: Option<String>,
) -> Result<(), AppError> {
    let pid = state.running_queries.lock().unwrap().get(&run_id).copied();
    let Some(pid) = pid else {
        return Ok(());
    };
    let pool = postgres_pool(&state, &id, database.as_deref().unwrap_or("")).await?;
    postgres::script::cancel(&pool, pid).await
}

/// Creates a database, for the header's database picker. `collation` is accepted and ignored: see
/// `postgres::ddl::create_database`.
#[tauri::command]
pub async fn postgres_create_database(
    state: State<'_, DbState>,
    id: String,
    name: String,
    #[allow(unused_variables)] collation: Option<String>,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, "").await?;
    postgres::ddl::create_database(&pool, &name).await
}

/// Drops a database and every table in it. Takes the whole connection rather than a pool, since the
/// pool on the database being dropped is what has to be closed first.
#[tauri::command]
pub async fn postgres_drop_database(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<(), AppError> {
    let pools = postgres_pools(&state, &id).await?;
    postgres::ddl::drop_database(&pools, &database).await
}

/// Creates an empty table — one `id` column and its primary key — for the sidebar's add button.
/// `collation` is accepted and ignored: a PostgreSQL table has none of its own.
#[tauri::command]
pub async fn postgres_create_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    #[allow(unused_variables)] collation: Option<String>,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::ddl::create_table(&pool, &table).await
}

#[tauri::command]
pub async fn postgres_rename_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    new_name: String,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::ddl::rename_table(&pool, &table, &new_name).await
}

#[tauri::command]
pub async fn postgres_drop_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::ddl::drop_table(&pool, &table).await
}

#[tauri::command]
pub async fn postgres_add_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: crate::modules::db::drivers::sql::types::ColumnSpec,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::ddl::add_column(&pool, &table, &spec.into()).await
}

#[tauri::command]
pub async fn postgres_modify_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: crate::modules::db::drivers::sql::types::ColumnSpec,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::ddl::modify_column(&pool, &table, &name, &spec.into()).await
}

#[tauri::command]
pub async fn postgres_drop_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::ddl::drop_column(&pool, &table, &name).await
}

#[tauri::command]
pub async fn postgres_add_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: crate::modules::db::drivers::sql::types::IndexSpec,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::ddl::add_index(&pool, &table, &spec.into()).await
}

#[tauri::command]
pub async fn postgres_modify_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: crate::modules::db::drivers::sql::types::IndexSpec,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::ddl::modify_index(&pool, &table, &name, &spec.into()).await
}

#[tauri::command]
pub async fn postgres_drop_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
) -> Result<(), AppError> {
    let pool = postgres_pool(&state, &id, &database).await?;
    postgres::ddl::drop_index(&pool, &table, &name).await
}

/// Writes a PostgreSQL database out as SQL. `mode` is `structure`, `data` or `all`.
///
/// Reports on `transfer://progress` as pg_dump names each table it reaches, the same estimate the
/// MySQL dump gives — so the command returning is still what says the dump is done.
#[tauri::command]
pub async fn postgres_dump(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    mode: String,
    path: String,
) -> Result<(), AppError> {
    let mode = dump::DumpMode::parse(&mode)?;
    let tool = tools::require(tools::Tool::PgDump, &tools_dir(&app)?)?;
    // What each table weighs, for the progress the dump reports. A server that will not say leaves
    // the dump to run with a bar that moves without a number, which is not worth refusing over.
    let pool = postgres_pool(&state, &id, &database).await?;
    let tables: Vec<(String, u64)> = postgres::structure::table_stats(&pool)
        .await
        .map(|tables| {
            tables
                .into_iter()
                .map(|table| (table.name, table.data_size))
                .collect()
        })
        .unwrap_or_default();
    let endpoint = sql_endpoint(&state, &id, DbKind::Postgres).await?;
    let report = reporter(&app, &id);
    /* Registered for the length of the run and taken out however it ends, so the tab closing or
    the Cancel button has something to reach. */
    let transfer = Transfer::start(&state, &id);
    let cancelled = transfer.flag();
    in_background(move || {
        postgres::dump::dump(
            &tool,
            &endpoint.host,
            endpoint.port,
            &endpoint.user,
            &endpoint.password,
            &database,
            mode,
            &path,
            &tables,
            &dump::Watch {
                report: &report,
                cancel: &|| cancelled.load(Ordering::Relaxed),
            },
        )
    })
    .await
}

/// Replays a SQL file through psql, into `database`.
#[tauri::command]
pub async fn postgres_restore(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    path: String,
) -> Result<(), AppError> {
    let tool = tools::require(tools::Tool::PsqlClient, &tools_dir(&app)?)?;
    let endpoint = sql_endpoint(&state, &id, DbKind::Postgres).await?;
    let report = reporter(&app, &id);
    /* Registered for the length of the run and taken out however it ends, so the tab closing or
    the Cancel button has something to reach. */
    let transfer = Transfer::start(&state, &id);
    let cancelled = transfer.flag();
    in_background(move || {
        postgres::dump::restore(
            &tool,
            &endpoint.host,
            endpoint.port,
            &endpoint.user,
            &endpoint.password,
            &database,
            &path,
            &dump::Watch {
                report: &report,
                cancel: &|| cancelled.load(Ordering::Relaxed),
            },
        )
    })
    .await
}
