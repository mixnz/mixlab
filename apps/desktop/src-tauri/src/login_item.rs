//! MixLab at login — ADR 0042.
//!
//! `tauri-plugin-autostart` writes the entry (a LaunchAgent, an `HKCU\...\Run` value, an XDG
//! autostart file) with [`crate::launch::HIDDEN`], so a login start brings up the tray and no
//! window. Its own JavaScript commands are not granted to any window: Settings reaches it through
//! the two below, which answer with what the operating system holds rather than what was last set.

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::AutoLaunchManager;

use crate::error::AppError;

/// What the Settings switch draws.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginItem {
    /// `false` in a development build, which never registers an entry: its name would be the
    /// release's, and the release's is the one a person ticked (ADR 0024, ADR 0040).
    pub supported: bool,
    /// Whether the entry is there now — read from the system every time.
    pub enabled: bool,
    /// Whether this session can show a tray icon. Without one a login start opens the window, and
    /// Settings says so under the switch.
    pub tray_host: bool,
}

#[tauri::command]
pub fn login_item_status(app: AppHandle) -> LoginItem {
    status(&app)
}

#[tauri::command]
pub fn login_item_set(app: AppHandle, enabled: bool) -> Result<LoginItem, AppError> {
    let Some(manager) = app.try_state::<AutoLaunchManager>() else {
        return Err(err!("error.loginItemUnsupported"));
    };
    let written = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    written.map_err(|e| err!("error.loginItemFailed", message = e))?;
    Ok(status(&app))
}

fn status(app: &AppHandle) -> LoginItem {
    let manager = app.try_state::<AutoLaunchManager>();
    LoginItem {
        supported: manager.is_some(),
        enabled: manager
            .map(|manager| manager.is_enabled().unwrap_or(false))
            .unwrap_or(false),
        tray_host: crate::tray::has_tray_host(app),
    }
}
