//! The four commands the *Remove MixLab* dialog calls — roadmap task **T182a**, spec D4.

use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, State};

use crate::error::AppError;
use crate::modules::mixengine::for_uninstall;
use crate::updater::placement::{self, PKG_RECEIPT};

use super::menu::UninstallMenu;
use super::{finished, offered, own_directories, read_back, Outcome};

/// How long a daemon removing its home is given to end: `mix uninstall` allows the same (T182b, D8).
const GONE_WITHIN: Duration = Duration::from_secs(120);

/// Whether this window offers the removal at all.
pub fn available() -> bool {
    offered(
        std::env::consts::OS,
        cfg!(debug_assertions),
        &placement::read(),
    )
}

#[tauri::command]
pub fn uninstall_available() -> bool {
    available()
}

/// The plan, with the two choices as they stand. Starts the daemon when none runs: the one job the
/// window starts MixEngine for on its own, because only the daemon knows its traces (spec D4).
#[tauri::command]
pub async fn uninstall_plan(keep_home: bool, keep_relocated: bool) -> Result<Value, AppError> {
    if !available() {
        return Err(err!("error.uninstallUnavailable"));
    }
    for_uninstall::ensure_daemon().await?;
    for_uninstall::plan(keep_home, keep_relocated).await
}

/// The whole removal. Returns only when the window is still here to say why; a finished one ends
/// the process without writing anything (spec D4, steps 3 to 6).
#[tauri::command]
pub async fn uninstall_run(
    app: AppHandle,
    keep_home: bool,
    keep_relocated: bool,
) -> Result<Outcome, AppError> {
    if !available() {
        return Err(err!("error.uninstallUnavailable"));
    }

    // The login entry is MixLab's own, not a row of MixEngine's plan, and left behind it would
    // start a bundle that no longer exists at every login. Unsupported is nothing to turn off.
    let _ = crate::login_item::login_item_set(app.clone(), false);

    let daemon = for_uninstall::ensure_daemon().await?;
    let report = for_uninstall::run(keep_home, keep_relocated).await?;
    // A declined prompt or a row left behind keeps the daemon up for the next run: nothing to wait for.
    let gone = finished(&report) && for_uninstall::wait_gone(&daemon, GONE_WITHIN).await;

    let bundle = crate::relaunch::origin()
        .map(|origin| origin.root.clone())
        .ok_or_else(|| err!("error.relaunchNoExecutable"))?;
    let read = read_back(&bundle, PKG_RECEIPT, &own_directories(&report));

    if gone && !read.bundle_left && !read.receipt_left && read.left.is_empty() {
        // **`std::process::exit`, not `app.exit`.** The window-state plugin and the exit hooks
        // would write into the directories just removed. The single-instance endpoint is the one
        // thing worth taking back, and `launch::stop` only removes.
        crate::launch::stop(&app);
        std::process::exit(0);
    }

    Ok(Outcome {
        report,
        gone,
        bundle_left: read.bundle_left,
        receipt_left: read.receipt_left,
        left: read.left,
    })
}

/// The frontend's translated text onto the menu item, which Rust built before any string was
/// loaded. Nothing to do where there is no item.
#[tauri::command]
pub fn uninstall_menu_label(menu: State<'_, UninstallMenu>, label: String) -> Result<(), AppError> {
    menu.set_text(&label)
        .map_err(|e| err!("error.uninstallFailed", message = e))
}
