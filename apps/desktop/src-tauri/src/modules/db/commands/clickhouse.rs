use super::{clickhouse_connection, reporter, Transfer};
use crate::error::AppError;
use crate::modules::db::drivers::{clickhouse, dump};
use crate::modules::db::state::DbState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

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
