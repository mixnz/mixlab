use crate::platform::{app_data_dir, in_background};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, State};

use super::models::{LocalShell, Output, OutputSink, TerminalEvent, TerminalSize, TerminalTarget};
use super::state::TerminalState;
use super::{local, remote};
use crate::error::AppError;

/// Which shells this machine can open. Detected by looking at the disk and — on Windows — asking
/// `wsl.exe`, so it runs on a blocking thread rather than holding up the async loop.
#[tauri::command]
pub async fn terminal_local_shells() -> Result<Vec<LocalShell>, AppError> {
    tokio::task::spawn_blocking(local::detect)
        .await
        .map_err(|e| err!("error.terminalSpawnFailed", message = e))
}

/// Opens a session and connects it to `on_event`.
///
/// `Data` travels as raw bytes and `Exit` as JSON, over the same channel — `Channel` numbers every
/// frame and the JS side reorders by that number, so `Exit` cannot overtake the last byte.
///
/// `Exit` is also when the session leaves the map. The frontend only calls `terminal_close` for a
/// tab closed while its session is still alive, so a session that ends by itself — typing `exit`,
/// the server disconnecting — and is not dropped here would stay until the app exits, along with
/// everything the `Session` holds.
#[tauri::command]
pub async fn terminal_open(
    app: AppHandle,
    id: String,
    target: TerminalTarget,
    size: TerminalSize,
    on_event: Channel<InvokeResponseBody>,
    state: State<'_, TerminalState>,
) -> Result<(), AppError> {
    /* Whether the session has already ended, read again after inserting. A shell that dies at once
    — a command that does not exist, a server closing right after its banner — emits `Exit` before
    `spawn` has had time to return, and at that point there is nothing in the map to drop. This flag
    is how the inserting side knows it has just inserted a dead session. */
    let ended = Arc::new(AtomicBool::new(false));
    let sink = output_sink(on_event, ended.clone(), {
        let app = app.clone();
        let id = id.clone();
        move || app.state::<TerminalState>().forget(&id)
    });

    let session = match target {
        /* Off the runtime: opening a pty is ConPTY on Windows and `forkpty` on Unix, both
        blocking, and the shell behind it may be on a network drive or a WSL distribution that
        has to start first. */
        TerminalTarget::Local { shell, args, cwd } => {
            in_background(move || local::spawn(shell, args, cwd, size, sink)).await?
        }
        // Failed authentication, a changed fingerprint, an unreachable server — all of them fail
        // here, before there is any session to put in the map. That is what the frontend brings
        // back to the `ErrorBanner` right at the form.
        TerminalTarget::Ssh(ssh) => remote::spawn(&ssh, &app_data_dir(&app)?, size, sink).await?,
    };

    // Opening the same id twice replaces the old session, and its `Drop` cleans up the rest.
    let dead = {
        let mut sessions = state.sessions.lock().unwrap();
        sessions.insert(id.clone(), session);
        // Died before making it into the map: drop it right away, because `Exit` has already gone
        // past and will not come back.
        if ended.load(Ordering::SeqCst) {
            sessions.remove(&id)
        } else {
            None
        }
    };
    // Outside the lock's scope, for the same reason as `TerminalState::forget`.
    drop(dead);
    Ok(())
}

/// A session's way out: `Data` goes straight as bytes, `Exit` goes as JSON — and `Exit` is also
/// when the session leaves the map, through `forget`.
///
/// `ended` is set *before* `forget` takes the lock, because the inserting side reads that flag
/// *under* the lock. Missing it could only happen if both saw an empty map, which needs the flag
/// set after the inserting side has read it and before it inserts — two things the inserting side
/// does back to back under the same lock.
///
/// Moved out of `terminal_open` so tests can call it: building a fake `AppHandle` costs far more
/// than calling this directly with a `TerminalState` of its own.
fn output_sink(
    on_event: Channel<InvokeResponseBody>,
    ended: Arc<AtomicBool>,
    forget: impl Fn() + Send + Sync + 'static,
) -> OutputSink {
    Arc::new(move |output| match output {
        Output::Data(bytes) => {
            let _ = on_event.send(InvokeResponseBody::Raw(bytes));
        }
        Output::Exit { code, message } => {
            if let Ok(json) = serde_json::to_string(&TerminalEvent::Exit { code, message }) {
                let _ = on_event.send(InvokeResponseBody::Json(json));
            }
            ended.store(true, Ordering::SeqCst);
            forget();
        }
    })
}

/// The bytes the user types. `data` is a string rather than base64: what xterm's `onData` produces
/// is always a valid string, and its UTF-8 is exactly what needs to be written to the pty.
#[tauri::command]
pub async fn terminal_write(
    id: String,
    data: String,
    state: State<'_, TerminalState>,
) -> Result<(), AppError> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions
        .get(&id)
        .ok_or_else(|| err!("error.terminalUnknownSession"))?;
    session
        .input
        .send(data.into_bytes())
        .map_err(|_| err!("error.terminalUnknownSession"))
}

#[tauri::command]
pub async fn terminal_resize(
    id: String,
    cols: u16,
    rows: u16,
    state: State<'_, TerminalState>,
) -> Result<(), AppError> {
    let sessions = state.sessions.lock().unwrap();
    let session = sessions
        .get(&id)
        .ok_or_else(|| err!("error.terminalUnknownSession"))?;
    session
        .resize
        .send(TerminalSize { cols, rows })
        .map_err(|_| err!("error.terminalUnknownSession"))
}

/// Closes the session. Removing it from the map runs `Drop`, which kills the process — there is no
/// other step. An id not in the map is not an error: a session that died by itself already removed
/// itself from the map when it emitted `Exit`, and the tab closing afterwards is still entitled to
/// call.
#[tauri::command]
pub async fn terminal_close(id: String, state: State<'_, TerminalState>) -> Result<(), AppError> {
    state.sessions.lock().unwrap().remove(&id);
    Ok(())
}

/// The text currently on the system clipboard, read right in this process.
///
/// The webview cannot read the clipboard without raising a permission box, so this is the only
/// path for the Paste item in the right-click menu — the full reason is in `Cargo.toml`, where
/// `arboard` is declared. Writing is not here: `core/clipboard.ts` writes through the webview, and
/// nobody asks for permission to write.
///
/// `in_background` because on Linux reading the clipboard is a round trip with X11 or Wayland, and
/// it has no business happening on the thread drawing the window.
#[tauri::command]
pub async fn terminal_clipboard_text() -> Result<String, AppError> {
    in_background(|| {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| err!("error.terminalClipboardRead", message = e))?;
        clipboard
            .get_text()
            .map_err(|e| err!("error.terminalClipboardRead", message = e))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::{local, output_sink, TerminalState};
    use crate::modules::terminal::models::TerminalSize;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tauri::ipc::Channel;

    /// The path "type `exit` and leave the tab there" takes.
    ///
    /// The frontend does not call `terminal_close` for a tab that has seen `Exit`, so if `Exit` did
    /// not drop the session from the map itself, the `Session` would stay until the app exits — and
    /// with it the two threads waiting on its two channels, or, for an SSH session, the whole TCP
    /// connection and its 15-second keepalive.
    ///
    /// Runs exactly the sink `terminal_open` builds; the only thing not going through here is one
    /// line taking `TerminalState` out of the `AppHandle`. `exit 3` rather than typing `exit` into
    /// an interactive shell, for the same reason as the test in `local.rs`.
    #[tokio::test]
    async fn a_session_that_ends_by_itself_leaves_the_map() {
        let state = Arc::new(TerminalState::default());
        let id = "phien-thu".to_string();

        let sink = output_sink(
            Channel::new(|_| Ok(())),
            Arc::new(AtomicBool::new(false)),
            {
                let state = state.clone();
                let id = id.clone();
                move || state.forget(&id)
            },
        );

        let (shell, args) = if cfg!(windows) {
            (
                "cmd.exe",
                vec!["/c".to_string(), "exit".to_string(), "3".to_string()],
            )
        } else {
            ("/bin/sh", vec!["-c".to_string(), "exit 3".to_string()])
        };
        let session = local::spawn(
            Some(shell.to_string()),
            args,
            None,
            TerminalSize { cols: 80, rows: 24 },
            sink,
        )
        .expect("shell phải mở được");

        // ConPTY asks for the cursor position and waits for an answer before letting the child
        // process run; in the app xterm answers, here nobody does. Answer on its behalf — see
        // `local.rs`.
        session.input.send(b"\x1b[1;1R".to_vec()).unwrap();
        state.sessions.lock().unwrap().insert(id.clone(), session);

        let deadline = Instant::now() + Duration::from_millis(5000);
        while state.sessions.lock().unwrap().contains_key(&id) {
            assert!(
                Instant::now() < deadline,
                "hết hạn mà phiên vẫn còn trong map"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}
