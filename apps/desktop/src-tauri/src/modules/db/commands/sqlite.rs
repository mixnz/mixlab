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
use crate::modules::db::state::DbState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

use super::{reporter, sqlite_pool, Transfer};

/// Creates an empty database file, for the New button on the connection form.
///
/// Takes no connection id: there is nothing open yet, which is the whole point — the form calls
/// this and then puts the path in its own box for the user to connect with.
#[tauri::command]
pub async fn sqlite_create_file(path: String) -> Result<(), AppError> {
    sqlite::create_file(&path).await
}

/// Writes the schema and/or the rows to `path`, depending on `mode`.
#[allow(unused_variables)]
pub(super) async fn dump(
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
pub(super) async fn restore(
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
