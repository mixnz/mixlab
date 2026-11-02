//! The tunnel module's commands.

use std::path::PathBuf;
use std::process::Command;

use tauri::{AppHandle, Emitter, State};

use super::binary::{self, Found, Provider};
use super::state::{records_path, TunnelInfo, Tunnels};
use super::target;
use crate::error::AppError;
use crate::platform::{app_data_dir, hide_console, in_background};

/// The download this system would get.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    version: String,
    size: u64,
}

/// Which program a tunnel would run, and what downloading one would fetch.
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

/// The program `provider` would run, or why there is none.
fn found(
    provider: Provider,
    configured: Option<String>,
    data: &std::path::Path,
) -> Result<Found, AppError> {
    binary::find(provider, configured_path(configured).as_deref(), data)
        .ok_or_else(|| err!("error.tunnelNoBinary", program = provider.program()))
}

#[tauri::command]
pub fn tunnel_binary(
    app: AppHandle,
    provider: Provider,
    configured: Option<String>,
) -> Result<BinaryStatus, AppError> {
    let data = app_data_dir(&app)?;
    Ok(BinaryStatus {
        found: binary::find(provider, configured_path(configured).as_deref(), &data),
        download: binary::source(provider).map(|source| Download {
            version: binary::version(provider).to_owned(),
            size: source.size,
        }),
    })
}

/// Downloads the provider's pinned program, reporting `{ done, total }` on `tunnel://download`.
#[tauri::command]
pub async fn tunnel_download(app: AppHandle, provider: Provider) -> Result<String, AppError> {
    let data = app_data_dir(&app)?;
    let reporter = app.clone();
    in_background(move || {
        binary::install(provider, &data, &|done, total| {
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
    provider: Provider,
    target: String,
    configured: Option<String>,
) -> Result<TunnelInfo, AppError> {
    let target = target::parse(&target)?;
    let data = app_data_dir(&app)?;
    let found = found(provider, configured, &data)?;
    let _ = std::fs::create_dir_all(data.join("tunnel"));
    state.start(&app, provider, &found.path, target, &records_path(&data))
}

/// Hands an authtoken to ngrok with ngrok's own `config add-authtoken`, which writes ngrok's own
/// config file. MixLab keeps nothing: not the token, not a copy of the file, and no error or log
/// line with the token in it.
#[tauri::command]
pub async fn tunnel_ngrok_authtoken(
    app: AppHandle,
    token: String,
    configured: Option<String>,
) -> Result<(), AppError> {
    let token = token.trim().to_owned();
    if token.is_empty() {
        return Err(err!("error.tunnelAuthtokenEmpty"));
    }
    let data = app_data_dir(&app)?;
    let ngrok = found(Provider::Ngrok, configured, &data)?;
    in_background(move || {
        let output = hide_console(Command::new(&ngrok.path).args(["config", "add-authtoken"]))
            .arg(&token)
            .output()
            .map_err(|e| err!("error.tunnelAuthtokenFailed", message = e))?;
        if output.status.success() {
            return Ok(());
        }
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Err(err!(
            "error.tunnelAuthtokenFailed",
            message = redact(said.trim(), &token)
        ))
    })
    .await
}

/// `output` with every occurrence of `token` replaced by `***`.
fn redact(output: &str, token: &str) -> String {
    if token.is_empty() {
        return output.to_owned();
    }
    output.replace(token, "***")
}

#[tauri::command]
pub fn tunnel_stop(app: AppHandle, state: State<'_, Tunnels>, id: u32) {
    state.stop(&app, id);
}

#[tauri::command]
pub fn tunnel_list(state: State<'_, Tunnels>) -> Vec<TunnelInfo> {
    state.list()
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn the_token_never_reaches_an_error() {
        assert_eq!(
            redact("ERROR: authtoken 2abcSECRET is invalid", "2abcSECRET"),
            "ERROR: authtoken *** is invalid"
        );
    }

    #[test]
    fn an_empty_token_redacts_nothing() {
        assert_eq!(redact("text", ""), "text");
    }
}
