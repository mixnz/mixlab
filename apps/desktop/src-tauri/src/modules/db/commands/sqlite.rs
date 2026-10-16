//! SQLite
//!
//! Every one of these takes a `database` argument the way the MySQL and PostgreSQL commands do, and
//! for the same reason — the workspace above hands one down — but there is only ever one value it
//! can hold: `drivers::sqlite::MAIN_DATABASE`. It is accepted and ignored rather than removed, so
//! that the shared `SqlApi` on the frontend keeps one shape across all three engines.
//!
//! `retry_read!` is not used here. It exists for a pooled connection that died between two
//! statements; a file that has gone away is not something a second attempt fixes.
//!
//! The unused arguments carry `#[allow(unused_variables)]` rather than a leading underscore. The
//! name of a command's argument is what the frontend's `invoke` has to send, and whether Tauri's
//! macro strips a `_` on the way to camelCase is a detail nothing in this repo depended on before
//! — a wrong guess there is a call that fails at runtime with "invalid args" and at build time not
//! at all. The attribute keeps the name exactly as it is sent.

use crate::error::AppError;
use crate::modules::db::drivers::{dump, sqlite};
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use crate::modules::db::state::DbState;
use serde_json::{Map, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

use super::handle;
use super::{reporter, sqlite_pool, RunningQuery, Transfer};

/// Creates an empty database file, for the New button on the connection form.
///
/// Takes no connection id: there is nothing open yet, which is the whole point — the form calls
/// this and then puts the path in its own box for the user to connect with.
#[tauri::command]
pub async fn sqlite_create_file(path: String) -> Result<(), AppError> {
    sqlite::create_file(&path).await
}

#[tauri::command]
pub async fn sqlite_server_info(
    state: State<'_, DbState>,
    id: String,
) -> Result<ServerInfo, AppError> {
    handle(&state, &id).await?.sql()?.server_info().await
}

/// The one database a file holds. Answered without touching it — see `sqlite::list_databases`.
#[tauri::command]
pub async fn sqlite_list_databases(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<String>, AppError> {
    handle(&state, &id).await?.sql()?.list_databases().await
}

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_list_tables(
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

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_table_data(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    query: sqlite::PageQuery,
) -> Result<sqlite::TablePage, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_data(database, table, query)
        .await
}

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_table_structure(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<sqlite::structure::TableStructure, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_structure(database, table)
        .await
}

#[tauri::command]
pub async fn sqlite_schema_outline(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<sqlite::structure::SchemaOutline, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .schema_outline(database)
        .await
}

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_table_stats(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<sqlite::structure::TableStats>, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_stats(database)
        .await
}

#[tauri::command]
pub async fn sqlite_collations(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<sqlite::structure::Collation>, AppError> {
    handle(&state, &id).await?.sql()?.collations().await
}

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_update_row(
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

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_insert_rows(
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

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_delete_rows(
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

/// `collation` is accepted and ignored: a SQLite table carries none of its own — see
/// `sqlite::ddl::create_table`.
#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_create_table(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    collation: Option<String>,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .create_table(database, table, collation, None)
        .await
}

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_rename_table(
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

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_drop_table(
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

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_add_column(
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

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_modify_column(
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

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_drop_column(
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

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_add_index(
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

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_modify_index(
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

#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_drop_index(
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

/// Runs the user's script.
///
/// `run_id` is accepted for the shape the other two engines have and is not remembered anywhere:
/// nothing here can be cancelled from outside, so there is no pid to file under it — see
/// `sqlite::script`.
#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_run_script(
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

/// Asks SQLite to prepare one statement without running it, for the editor's error checking.
#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_validate_sql(
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

/// Writes the schema and/or the rows to `path`, depending on `mode`.
#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_dump(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    mode: String,
    path: String,
) -> Result<(), AppError> {
    let mode = dump::DumpMode::parse(&mode)?;
    let pool = sqlite_pool(&state, &id).await?;
    let report = reporter(&app, &id);
    let transfer = Transfer::start(&state, &id);
    let cancelled = transfer.flag();
    let watch = dump::Watch {
        report: &report,
        cancel: &|| cancelled.load(Ordering::Relaxed),
    };
    let path = std::path::Path::new(&path);

    if mode != dump::DumpMode::Data {
        sqlite::dump::dump_structure(&pool, path, &watch).await?;
    }
    if mode != dump::DumpMode::Structure {
        let append = mode == dump::DumpMode::All;
        sqlite::dump::dump_data(&pool, path, append, &watch).await?;
    }
    Ok(())
}

/// Replays a dump file into the open database. No progress in between — see A5 of the design
/// spec — only one reading before `sqlite::script::run` starts, so the overlay shows movement
/// rather than sitting frozen on an empty bar.
#[allow(unused_variables)]
#[tauri::command]
pub async fn sqlite_restore(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    path: String,
) -> Result<(), AppError> {
    let pool = sqlite_pool(&state, &id).await?;
    let report = reporter(&app, &id);
    let _transfer = Transfer::start(&state, &id);
    report(dump::Progress::default());
    sqlite::dump::restore(&pool, std::path::Path::new(&path)).await
}
