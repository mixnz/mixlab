//! What this module keeps between two commands: exactly one open event stream.
//!
//! **One per window** since T168a: the tray panel is a second webview with its own `daemonWatch.ts`,
//! and a single slot meant the panel opening its stream closed the main window's — the Dashboard
//! would silently stop updating whenever somebody clicked the tray icon.
//!
//! One stream per tab is wrong. MixEngine's event bus is a shared bus holding 1024 messages, and
//! two MixEngine tabs open at once would be two `/events` connections reading it. One stream, with
//! every tab listening on the same `Channel`, is enough for this phase — logs have their own stream
//! per service ([MixEngine's ADR 0009]: logs are never events), kept in `LogsState` just below,
//! entirely separate from `MixEngineState`.

use std::collections::HashMap;
use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

/// One cancellable stream per webview label — what `MixEngineState` and `MetricsState` both keep,
/// since the tray panel (T168) opens each of them beside the main window's.
#[derive(Default)]
struct PerWindow {
    open: Mutex<HashMap<String, CancellationToken>>,
}

impl PerWindow {
    fn keep(&self, window: &str, token: CancellationToken) {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = open.insert(window.to_owned(), token) {
            previous.cancel();
        }
    }

    fn stop(&self, window: &str) {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(token) = open.remove(window) {
            token.cancel();
        }
    }
}

#[derive(Default)]
pub struct MixEngineState {
    /// Keyed by webview label (`main`, `tray`).
    streams: PerWindow,
}

impl MixEngineState {
    /// Cancels this window's open stream, if any, and keeps the new one. Another window's stream
    /// is left alone.
    pub fn keep(&self, window: &str, token: CancellationToken) {
        self.streams.keep(window, token);
    }

    /// Closes this window's open stream. Calling it twice is harmless.
    pub fn stop(&self, window: &str) {
        self.streams.stop(window);
    }
}

/// Exactly one open log stream — separate from `MixEngineState`, because `/events` and
/// `/logs/service/{id}` are two connections at once, not one replacing the other. The same
/// `keep`/`stop` shape, a separate struct because Tauri keys state by type: merging them would be
/// two streams sharing one lock, and opening Logs would close the `/events` open for the Dashboard.
#[derive(Default)]
pub struct LogsState {
    open: Mutex<Option<CancellationToken>>,
}

impl LogsState {
    pub fn keep(&self, token: CancellationToken) {
        let mut slot = self.open.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = slot.replace(token) {
            previous.cancel();
        }
    }

    pub fn stop(&self) {
        let mut slot = self.open.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(token) = slot.take() {
            token.cancel();
        }
    }
}

/// Exactly one open `/metrics` stream — separate from `MixEngineState`/`LogsState`, because the
/// Dashboard holds `/events` **and** `/metrics` at once: merging with either would let one take the
/// other's lock. `LogsState` is not reused for this even though it is also "one stream" —
/// `/logs/{id}` and `/metrics` can be open at the same time when Logs and the Dashboard are both in
/// the already-visited state (`mountedScreens` keeps every screen in the DOM).
///
/// **Closing this stream means something different from closing the other two.** `/events` and
/// `/logs` close because nobody reads them any more; closing `/metrics` also changes the daemon's
/// behaviour — opening this connection makes the daemon sample at 1 Hz, and closing it returns the
/// daemon to once a minute. `stop()` here must be called exactly when the Dashboard is no longer
/// `active`, not only on unmount.
///
/// **One per window since T168.** The tray panel shows the daemon's own CPU and memory while it is
/// open; with a single slot, opening it closed the Dashboard's stream, and closing it left the
/// Dashboard with none. The daemon samples at 1 Hz while *any* of them is open.
#[derive(Default)]
pub struct MetricsState {
    streams: PerWindow,
}

impl MetricsState {
    pub fn keep(&self, window: &str, token: CancellationToken) {
        self.streams.keep(window, token);
    }

    pub fn stop(&self, window: &str) {
        self.streams.stop(window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opening a second one closes the first — otherwise a reopened tab would leave behind an
    /// `/events` connection nobody reads, running until the app exits.
    #[test]
    fn a_second_stream_cancels_the_first() {
        let state = MixEngineState::default();
        let first = CancellationToken::new();
        state.keep("main", first.clone());
        assert!(!first.is_cancelled());

        let second = CancellationToken::new();
        state.keep("main", second.clone());
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());

        state.stop("main");
        assert!(second.is_cancelled());
    }

    /// The tray panel opening and closing its stream must never touch the main window's.
    #[test]
    fn each_window_keeps_its_own_stream() {
        let state = MixEngineState::default();
        let main = CancellationToken::new();
        let tray = CancellationToken::new();
        state.keep("main", main.clone());
        state.keep("tray", tray.clone());
        assert!(!main.is_cancelled());
        assert!(!tray.is_cancelled());

        state.stop("tray");
        assert!(tray.is_cancelled());
        assert!(!main.is_cancelled());

        let tray_again = CancellationToken::new();
        state.keep("tray", tray_again.clone());
        assert!(!main.is_cancelled());
    }

    /// The panel's metrics opening and closing must not touch the Dashboard's.
    #[test]
    fn metrics_are_kept_per_window_too() {
        let state = MetricsState::default();
        let main = CancellationToken::new();
        let tray = CancellationToken::new();
        state.keep("main", main.clone());
        state.keep("tray", tray.clone());
        state.stop("tray");
        assert!(tray.is_cancelled());
        assert!(!main.is_cancelled());
    }

    /// Closing with nothing open, and closing twice, must both not panic: `mixengine_unwatch` runs
    /// from an effect's cleanup and effects run twice under StrictMode.
    #[test]
    fn stopping_nothing_is_harmless() {
        let state = MixEngineState::default();
        state.stop("main");
        state.stop("main");
        state.stop("a window that never watched");
    }
}
