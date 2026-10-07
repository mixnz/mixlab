//! The tunnel module's commands — T203a.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, State};

use super::binary::{self, Found};
use super::state::{records_path, TunnelInfo, Tunnels};
use super::target;
use crate::error::AppError;
use crate::platform::{app_data_dir, in_background};

/// The download this system would get.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    version: String,
    size: u64,
}

/// Which cloudflared a tunnel would run, and what downloading one would fetch.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryStatus {
    found: Option<Found>,
    download: Option<Download>,
}

fn configured_path(configured: Option<String>) -> Option<PathBuf> {
    configured
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
}

#[tauri::command]
pub fn tunnel_binary(app: AppHandle, configured: Option<String>) -> Result<BinaryStatus, AppError> {
    let data = app_data_dir(&app)?;
    Ok(BinaryStatus {
        found: binary::find(configured_path(configured).as_deref(), &data),
        download: binary::source().map(|source| Download {
            version: binary::VERSION.to_owned(),
            size: source.size,
        }),
    })
}

/// Downloads the pinned cloudflared, reporting `{ done, total }` on `tunnel://download`.
#[tauri::command]
pub async fn tunnel_download(app: AppHandle) -> Result<String, AppError> {
    let data = app_data_dir(&app)?;
    let reporter = app.clone();
    in_background(move || {
        binary::install(&data, &|done, total| {
            let _ = reporter.emit(
                "tunnel://download",
                serde_json::json!({ "done": done, "total": total }),
            );
        })
        .map(|path| path.display().to_string())
    })
    .await
}

#[tauri::command]
pub async fn tunnel_start(
    app: AppHandle,
    state: State<'_, Tunnels>,
    target: String,
    configured: Option<String>,
) -> Result<TunnelInfo, AppError> {
    let target = target::parse(&target)?;
    let data = app_data_dir(&app)?;
    let found = binary::find(configured_path(configured).as_deref(), &data)
        .ok_or_else(|| err!("error.tunnelNoBinary"))?;
    let _ = std::fs::create_dir_all(data.join("tunnel"));
    state.start(&app, &found.path, target, &records_path(&data))
}

#[tauri::command]
pub fn tunnel_stop(app: AppHandle, state: State<'_, Tunnels>, id: u32) {
    state.stop(&app, id);
}

#[tauri::command]
pub fn tunnel_list(state: State<'_, Tunnels>) -> Vec<TunnelInfo> {
    state.list()
}
