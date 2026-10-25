//! Asks the database module to bring its saved connections along after a service moved port.
//!
//! The `open_terminal.rs` path: build the state, call `launch::request`. The `db` module owns its
//! saved connections and is the only one that may read or change them, so this hands it the move
//! and nothing else; the tab finds which connections were made for that service and asks first.

use serde_json::json;
use tauri::AppHandle;

use crate::error::AppError;
use crate::launch::{self, TabRequest};

/// Opens a Database tab that offers to move `service`'s saved connections from `from` to `to`.
#[tauri::command]
pub async fn mixengine_saved_connections_follow(
    app: AppHandle,
    service: String,
    from: u16,
    to: u16,
) -> Result<(), AppError> {
    launch::request(
        &app,
        TabRequest {
            module_id: "db",
            state: json!({ "serviceMoved": { "service": service, "from": from, "to": to } }),
        },
    );
    Ok(())
}
