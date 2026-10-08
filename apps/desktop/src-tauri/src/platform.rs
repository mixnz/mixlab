//! The things every corner of the app needed and each wrote out for itself.
//!
//! None of them is about a database, a terminal or a request — they are about the machine MixLab is
//! running on and the runtime it is running in, which is why they are here at the crate root rather
//! than in whichever module happened to want them first.

use std::path::PathBuf;
use std::process::Command;

use tauri::{AppHandle, Manager};

use crate::error::AppError;

/// Runs blocking work off the async runtime, turning a panicked or cancelled task into an error.
///
/// Everything that reaches the OS credential store or a `std::process::Command` goes through here:
/// both block, and blocking a Tauri command's thread blocks the webview's answer to every other
/// command with it.
pub async fn in_background<T, F>(work: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| err!("error.backgroundTaskFailed", message = e))?
}

/// Where MixLab keeps what it remembers between runs: the tools it downloaded, and the SSH host
/// keys it has seen.
pub fn app_data_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, AppError> {
    app.path()
        .app_data_dir()
        .map_err(|e| err!("error.noAppDataDir", message = e))
}

/// Keeps a child process from opening a console window of its own.
///
/// A release build has no console — `windows_subsystem = "windows"` — so Windows gives every child
/// in the console subsystem a new one, which is a real black window. A tool that runs for a few
/// tens of milliseconds shows up as a strange window flashing over the app, and looks exactly like
/// something running behind the user's back.
///
/// Does nothing anywhere else, so callers need no `cfg` of their own.
pub fn hide_console(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// Not in `std`, and `windows-sys` is not a dependency for one integer.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Whether the window-state plugin last saved `label` as maximized.
///
/// Read from the plugin's own file, for the one window whose state it is told not to put back
/// itself (`lib.rs`, and `launch::bring_to_front` for why). A file that is missing or unreadable
/// is a window that was not maximized: the plugin itself treats it the same way.
#[cfg(windows)]
pub fn saved_maximized<R: tauri::Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    use tauri_plugin_window_state::AppHandleExt;

    let Ok(dir) = app.path().app_config_dir() else {
        return false;
    };
    std::fs::read(dir.join(app.filename()))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|saved| saved.get(label)?.get("maximized")?.as_bool())
        .unwrap_or(false)
}

/// Lets the next child process handle Ctrl+C — roadmap task **T205c**.
///
/// A launcher that starts MixLab with `CREATE_NEW_PROCESS_GROUP` sets "ignore Ctrl+C" on it, and
/// every child inherits that: a local PowerShell tab then ignored the key while Git Bash, whose
/// runtime turns the byte into its own signal, still stopped. Clearing it is harmless here, since a
/// GUI process has no console for a Ctrl+C to arrive on, and cheap enough to do before every spawn.
///
/// Does nothing anywhere else, so callers need no `cfg` of their own.
pub fn process_ctrl_c() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn SetConsoleCtrlHandler(handler: *const std::ffi::c_void, add: i32) -> i32;
        }
        // SAFETY: a null handler with FALSE only clears this process's "ignore Ctrl+C" flag.
        unsafe { SetConsoleCtrlHandler(std::ptr::null(), 0) };
    }
}

/// Shows `window` maximized in a single `ShowWindow`, so nothing smaller or emptier reaches the
/// screen first.
///
/// On the main thread, as every call on a window's handle must be; queued there behind anything
/// already asked of the window, so a `show` that follows it still comes after it. tao reads the
/// maximized state back from the `WM_SIZE` this causes, so its own record stays true.
#[cfg(windows)]
pub fn show_maximized<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    /// Not in `std`, and `windows-sys` is not a dependency for one call.
    const SW_SHOWMAXIMIZED: i32 = 3;
    #[link(name = "user32")]
    extern "system" {
        fn ShowWindow(hwnd: *mut std::ffi::c_void, command: i32) -> i32;
    }

    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(hwnd) = target.hwnd() {
            // SAFETY: the handle is this process's own live window, and this is its thread.
            unsafe { ShowWindow(hwnd.0, SW_SHOWMAXIMIZED) };
        }
    });
}
