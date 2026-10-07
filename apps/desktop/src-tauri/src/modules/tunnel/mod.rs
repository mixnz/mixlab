//! The `tunnel` module — T203a. Shares an address on this machine on the internet through a
//! Cloudflare quick tunnel. Dials nothing of MixEngine's: ADR 0056.

pub mod binary;
pub mod commands;
pub mod process;
pub mod state;
pub mod target;

/// Puts the running list in the app.
pub fn register<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.manage(state::Tunnels::default())
}
