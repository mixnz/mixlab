//! PostgreSQL
//!
//! Every one of these takes the database as an argument the way the MySQL commands do, but it means
//! something different: there it names a database to reach into from the one connection, here it
//! picks which pool the command runs on. See `postgres_pool`.

use super::handle;
use super::{in_background, postgres_pool, reporter, sql_endpoint, tools_dir};
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
    handle(&state, &id).await?.sql()?.list_databases().await
}

#[tauri::command]
pub async fn postgres_server_info(
    state: State<'_, DbState>,
    id: String,
) -> Result<ServerInfo, AppError> {
    handle(&state, &id).await?.sql()?.server_info().await
}

#[tauri::command]
pub async fn postgres_list_tables(
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
pub async fn postgres_table_stats(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<postgres::structure::TableStats>, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_stats(database)
        .await
}

#[tauri::command]
pub async fn postgres_table_data(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    query: postgres::PageQuery,
) -> Result<postgres::TablePage, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_data(database, table, query)
        .await
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
    handle(&state, &id)
        .await?
        .sql()?
        .update_row(database, table, updates, key)
        .await
}

#[tauri::command]
pub async fn postgres_insert_rows(
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
    handle(&state, &id)
        .await?
        .sql()?
        .delete_rows(database, table, keys, all, reset_auto_increment)
        .await
}

#[tauri::command]
pub async fn postgres_table_structure(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<postgres::structure::TableStructure, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_structure(database, table)
        .await
}

#[tauri::command]
pub async fn postgres_collations(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<postgres::structure::Collation>, AppError> {
    handle(&state, &id).await?.sql()?.collations().await
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
    handle(&state, &id)
        .await?
        .sql()?
        .schema_outline(database)
        .await
}

#[tauri::command]
pub async fn postgres_run_script(
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

/// Asks PostgreSQL to parse one statement without running it, for the editor's error checking.
#[tauri::command]
pub async fn postgres_validate_sql(
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

/// Stops the run named by `run_id`, if it is still running — see [`DbState::running_queries`].
///
/// The cancel goes out on a connection of its own, since the one being cancelled is busy — and to
/// the same database, because a backend pid is only cancellable from the server it belongs to.
#[tauri::command]
pub async fn postgres_cancel_query(
    state: State<'_, DbState>,
    id: String,
    run_id: String,
    #[allow(unused_variables)] database: Option<String>,
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

/// Creates a database, for the header's database picker. `collation` is accepted and ignored: see
/// `postgres::ddl::create_database`.
#[tauri::command]
pub async fn postgres_create_database(
    state: State<'_, DbState>,
    id: String,
    name: String,
    #[allow(unused_variables)] collation: Option<String>,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .create_database(name, collation)
        .await
}

/// Drops a database and every table in it. Takes the whole connection rather than a pool, since the
/// pool on the database being dropped is what has to be closed first.
#[tauri::command]
pub async fn postgres_drop_database(
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
    handle(&state, &id)
        .await?
        .sql()?
        .create_table(database, table, collation, None)
        .await
}

#[tauri::command]
pub async fn postgres_rename_table(
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
pub async fn postgres_drop_table(
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
pub async fn postgres_add_column(
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
pub async fn postgres_modify_column(
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
pub async fn postgres_drop_column(
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
pub async fn postgres_add_index(
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
pub async fn postgres_modify_index(
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
pub async fn postgres_drop_index(
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
