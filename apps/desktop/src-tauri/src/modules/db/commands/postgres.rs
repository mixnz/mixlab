//! PostgreSQL's dump and restore, which `sql_dump` and `sql_restore` call once they know the
//! engine. Every other SQL operation is a `sql_*` command in `sql.rs`, answered by
//! `drivers::postgres::engine`.

use super::Transfer;
use super::{in_background, postgres_pool, reporter, sql_endpoint, tools_dir};
use crate::error::AppError;
use crate::modules::db::drivers::{dump, postgres, tools};
use crate::modules::db::models::DbKind;
use crate::modules::db::state::DbState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

/// Writes a PostgreSQL database out as SQL. `mode` is `structure`, `data` or `all`.
///
/// Reports on `transfer://progress` as pg_dump names each table it reaches, the same estimate the
/// MySQL dump gives — so the command returning is still what says the dump is done.
pub(super) async fn dump(
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
pub(super) async fn restore(
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
