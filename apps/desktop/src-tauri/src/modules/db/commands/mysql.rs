//! Every MySQL and MariaDB command. Which of the two answered is a property of the
//! connection, read once when it was opened - see `drivers::mysql::detect_mariadb`.

use super::handle;
use super::{in_background, mysql_pool, reporter, sql_endpoint, tools_dir};
use super::{RunningQuery, Transfer};
use crate::error::AppError;
use crate::modules::db::drivers::{dump, mysql, tools};
use crate::modules::db::models::DbKind;
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use crate::modules::db::state::DbState;
use serde_json::{Map, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn mysql_query(
    state: State<'_, DbState>,
    id: String,
    sql: String,
    database: Option<String>,
) -> Result<Vec<Map<String, Value>>, AppError> {
    let pool = mysql_pool(&state, &id).await?;
    mysql::query(&pool, &sql, database.as_deref()).await
}

#[tauri::command]
pub async fn mysql_list_databases(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<String>, AppError> {
    handle(&state, &id).await?.sql()?.list_databases().await
}

#[tauri::command]
pub async fn mysql_server_info(
    state: State<'_, DbState>,
    id: String,
) -> Result<ServerInfo, AppError> {
    handle(&state, &id).await?.sql()?.server_info().await
}

#[tauri::command]
pub async fn mysql_list_tables(
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
pub async fn mysql_table_stats(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<mysql::structure::TableStats>, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_stats(database)
        .await
}

#[tauri::command]
pub async fn mysql_table_data(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    query: mysql::PageQuery,
) -> Result<mysql::TablePage, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_data(database, table, query)
        .await
}

#[tauri::command]
pub async fn mysql_update_row(
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
pub async fn mysql_insert_rows(
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

#[tauri::command]
pub async fn mysql_delete_rows(
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
pub async fn mysql_table_structure(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
) -> Result<mysql::structure::TableStructure, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .table_structure(database, table)
        .await
}

/// Every table and column of one database, for the Query tab's completion. One read covers the
/// whole database, so the editor never asks per table as the Structure tab does.
#[tauri::command]
pub async fn mysql_schema_outline(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<mysql::structure::SchemaOutline, AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .schema_outline(database)
        .await
}

/// The collations this server has, for the column editor's picker. A property of the server rather
/// than of any one table, so the frontend reads it once per connection.
#[tauri::command]
pub async fn mysql_collations(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<mysql::structure::Collation>, AppError> {
    handle(&state, &id).await?.sql()?.collations().await
}

/// Writes a database out as SQL. `mode` is `structure`, `data` or `all`.
///
/// Long enough on a real database to need saying how far along it is, which it does on
/// `transfer://progress` — an estimate built from what mysqldump says it is doing, so the command
/// returning is still what says the dump is done.
#[tauri::command]
pub async fn mysql_dump(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    mode: String,
    path: String,
) -> Result<(), AppError> {
    let mode = dump::DumpMode::parse(&mode)?;
    let tool = tools::require(tools::Tool::MysqlDump, &tools_dir(&app)?)?;
    // Read through the pool, before the endpoint: the character set the dump is transferred in and
    // the server's own version are properties of the server, not of how the tool is invoked.
    let pool = mysql_pool(&state, &id).await?;
    let charset = mysql::structure::dump_charset(&pool, &database).await?;
    let version = mysql::server_info(&pool).await.map(|info| info.version);
    // Only MySQL 8.0 and up has the histogram table an 8.0 mysqldump reads by default. MariaDB has
    // it at no version — it numbers itself 10.x and 11.x, so it cannot be told from a modern MySQL
    // by the number alone — and asking it for one is an error that stops the dump outright. A
    // version that could not be read is treated as old, which costs a dump nothing but the
    // histograms.
    let column_statistics = version
        .as_deref()
        .is_ok_and(|version| !mysql::is_mariadb(version) && !version.starts_with('5'));
    // What each table weighs, for the progress the dump reports. A server that will not say —
    // `information_schema` shows a user only what they have privileges on — leaves the dump to run
    // with a bar that moves without a number, which is not worth refusing to dump over.
    let tables: Vec<(String, u64)> = mysql::structure::table_stats(&pool, &database)
        .await
        .map(|tables| {
            tables
                .into_iter()
                .map(|table| (table.name, table.data_size))
                .collect()
        })
        .unwrap_or_default();
    let endpoint = sql_endpoint(&state, &id, DbKind::Mysql).await?;
    let report = reporter(&app, &id);
    /* Registered for the length of the run and taken out however it ends, so the tab closing or
    the Cancel button has something to reach. */
    let transfer = Transfer::start(&state, &id);
    let cancelled = transfer.flag();
    in_background(move || {
        mysql::dump::dump(
            &tool,
            &endpoint.host,
            endpoint.port,
            &endpoint.user,
            &endpoint.password,
            &database,
            &charset,
            mode,
            column_statistics,
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

/// Replays a SQL file. `database` is the default one for statements that do not name their own.
///
/// Reports on `transfer://progress` as the file goes in, which for a restore is a count rather
/// than an estimate — the file is of a known size and the client is fed it byte by byte.
#[tauri::command]
pub async fn mysql_restore(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    database: String,
    path: String,
) -> Result<(), AppError> {
    let tool = tools::require(tools::Tool::MysqlClient, &tools_dir(&app)?)?;
    let endpoint = sql_endpoint(&state, &id, DbKind::Mysql).await?;
    let report = reporter(&app, &id);
    /* Registered for the length of the run and taken out however it ends, so the tab closing or
    the Cancel button has something to reach. */
    let transfer = Transfer::start(&state, &id);
    let cancelled = transfer.flag();
    in_background(move || {
        mysql::dump::restore(
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

/// Drops a database and every table in it.
#[tauri::command]
pub async fn mysql_drop_database(
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

/// Creates a database, for the header's database picker.
#[tauri::command]
pub async fn mysql_create_database(
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

/// Creates an empty table — one `id` column and its primary key — for the sidebar's add button.
#[tauri::command]
pub async fn mysql_create_table(
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

/// Renames a table, for the sidebar's context menu.
#[tauri::command]
pub async fn mysql_rename_table(
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

/// Drops a table and everything in it, for the sidebar's context menu.
#[tauri::command]
pub async fn mysql_drop_table(
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
pub async fn mysql_add_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: mysql::structure::ColumnSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .add_column(database, table, spec)
        .await
}

#[tauri::command]
pub async fn mysql_modify_column(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: mysql::structure::ColumnSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .modify_column(database, table, name, spec)
        .await
}

#[tauri::command]
pub async fn mysql_drop_column(
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
pub async fn mysql_add_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    spec: mysql::structure::IndexSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .add_index(database, table, spec)
        .await
}

#[tauri::command]
pub async fn mysql_modify_index(
    state: State<'_, DbState>,
    id: String,
    database: String,
    table: String,
    name: String,
    spec: mysql::structure::IndexSpec,
) -> Result<(), AppError> {
    handle(&state, &id)
        .await?
        .sql()?
        .modify_index(database, table, name, spec)
        .await
}

#[tauri::command]
pub async fn mysql_drop_index(
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

/// Runs the Query tab's editor contents. `database` is the one selected in the header, applied as a
/// `USE` before the script so that unqualified table names resolve the way the rest of the
/// workspace resolves them.
#[tauri::command]
pub async fn mysql_run_script(
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

/// Asks MySQL to parse one statement without running it, for the editor's error checking.
///
/// Read-only in the strongest sense available: the statement is prepared and the plan thrown away,
/// so not even a `DELETE` handed to this does anything. What comes back is `null` when the server
/// had nothing to say — see `mysql::script::validate` for why most of what it *does* say is a
/// warning rather than an error.
#[tauri::command]
pub async fn mysql_validate_sql(
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

/// Stops the run named by `run_id`, if it is still running.
///
/// `run_id` and not just the connection: the button belongs to one editor, and what it means is
/// "stop the script *I* started" — see [`DbState::running_queries`].
///
/// Asking to cancel what has already finished is not an error: the button is pressed while the
/// results are on their way back often enough, and the user's intent — that it not still be
/// running — is satisfied either way.
#[tauri::command]
pub async fn mysql_cancel_query(
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
