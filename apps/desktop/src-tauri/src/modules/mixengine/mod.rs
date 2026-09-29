//! Talks to MixEngine's daemon. See
//! `docs/specs/2026-09-06-mixengine-transport-design.md`.

pub mod commands;
pub mod endpoint;
pub mod events;
pub mod for_update;
pub mod health;
pub mod logs;
pub mod metrics;
pub mod open_in_mixdb;
pub mod rpc;
pub mod sse;
pub mod state;
pub mod transport;

/// Puts the module's state into the app. Tauri keys state by type, so it never meets another
/// module's state.
pub fn register<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .manage(state::MixEngineState::default())
        .manage(state::LogsState::default())
        .manage(state::MetricsState::default())
}
