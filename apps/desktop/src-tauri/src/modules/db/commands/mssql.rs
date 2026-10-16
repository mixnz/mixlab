//! SQL Server
//!
//! `database` here means what it means on MySQL and not what it means on PostgreSQL: a database to
//! reach into over the one pool, never a pool to pick. See `mssql_pool`.

use crate::error::AppError;
use crate::modules::db::drivers::{dump, mssql};
use crate::modules::db::state::DbState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

use super::{mssql_pool, reporter, Transfer};

/// Writes `database` out as SQL — `mode` is `structure`, `data` or `all`. See
/// `docs/specs/2026-09-05-mssql-support-design.md`'s D10 and
/// `2026-09-05-mssql-plan-7-dump-restore.md` (a local plan, never committed).
pub(super) async fn dump(
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
/// frozen on an empty bar, the same as SQLite's restore.
#[allow(unused_variables)]
pub(super) async fn restore(
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
