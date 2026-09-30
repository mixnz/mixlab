//! Database: the module MixLab started as.
//!
//! Everything about reaching a server, browsing it and writing to it lives under here. The app
//! above knows only [`register`] and this module's block of the command list in
//! [`super::handler`] — no type declared in here reaches the shell.

pub mod commands;
pub mod drivers;
pub mod handoff;
pub mod models;
pub mod state;

/// Puts this module's own state in the app. Called once, from `lib.rs`.
///
/// Tauri keys managed state by type, so a second module manages its own struct through a call of
/// its own here without ever meeting [`state::DbState`].
pub fn register<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .manage(state::DbState::default())
        // Connections handed over by another program, waiting for their tab — see `handoff.rs`.
        .manage(handoff::HandoffState::default())
}

/// Deletes what a tool download that never finished left behind — see
/// [`drivers::tools::sweep_staging`].
///
/// Takes the app handle rather than a path because where MixLab keeps its data is the app's to say.
/// Silent throughout: a sweep that cannot read the directory changes nothing but how much disk is
/// used, and there is nobody to tell at the moment it runs.
pub fn sweep_downloads<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Ok(dir) = crate::platform::app_data_dir(app) {
        drivers::tools::sweep_staging(&dir.join("tools"));
    }
}
