//! Hands a Terminal tab state to the shell's launch queue — roadmap task **T205**, D11.
//!
//! The `explore_data.rs` path, one module along: build the state, call `launch::request`. The
//! Terminal module validates what arrives exactly as it validates a restored tab, so nothing here
//! needs to know its shape.

use serde_json::Value;
use tauri::AppHandle;

use crate::error::AppError;
use crate::launch::{self, TabRequest};

/// Opens a Terminal tab with `state` as the tab's restored state.
#[tauri::command]
pub async fn mixengine_open_terminal(app: AppHandle, state: Value) -> Result<(), AppError> {
    launch::request(
        &app,
        TabRequest {
            module_id: "terminal",
            state,
        },
    );
    Ok(())
}
