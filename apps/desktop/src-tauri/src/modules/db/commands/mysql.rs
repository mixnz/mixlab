//! Every MySQL and MariaDB command. Which of the two answered is a property of the
//! connection, read once when it was opened - see `drivers::mysql::detect_mariadb`.

use super::Transfer;
use super::{in_background, mysql_pool, reporter, sql_endpoint, tools_dir};
use crate::error::AppError;
use crate::modules::db::drivers::{dump, mysql, tools};
use crate::modules::db::models::DbKind;
use crate::modules::db::state::DbState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

/// Writes a database out as SQL. `mode` is `structure`, `data` or `all`.
///
/// Long enough on a real database to need saying how far along it is, which it does on
/// `transfer://progress` — an estimate built from what mysqldump says it is doing, so the command
/// returning is still what says the dump is done.
pub(super) async fn dump(
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
pub(super) async fn restore(
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
