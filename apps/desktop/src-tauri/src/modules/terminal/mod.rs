//! Terminal: a shell session, on this machine or on a server over SSH.
//!
//! The difference between the two kinds of session is contained in the function that builds the
//! session; from `commands.rs` upwards there is only one `Session` and one way out.

pub mod commands;
pub mod local;
pub mod models;
pub mod remote;
pub mod state;
pub mod stream;

/// Puts the module's state into the app. Called once, from `lib.rs`.
pub fn register<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.manage(state::TerminalState::default())
}
