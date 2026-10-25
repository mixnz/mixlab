//! Starting this window again.
//!
//! Two callers: the ErrorBoundary's *Restart app*, and MixLab's own updater (`crate::updater`),
//! coming back on the new version after it has replaced this executable underneath the running
//! process.
//!
//! # Three things here are not obvious
//!
//! **The path is read once, at startup.** On Linux `std::env::current_exe` reads `/proc/self/exe`,
//! which follows the *inode* and not the name: after the swap has renamed `mixlab` to `mixlab.old`
//! and written a new `mixlab`, a running window asking afterwards is told its own path is
//! `…/mixlab.old`. Relaunching that is relaunching the version the user just replaced, with nothing
//! anywhere to say so. So [`remember`] runs on the first line of `run()`, before an update can have
//! happened, and [`origin`] is what everything else reads.
//!
//! **A relaunched copy must not hand its start back to the copy it is replacing.** `run()`'s second
//! act is `launch::forward`, which finds the still-running predecessor and exits — so a naive
//! spawn-then-exit produces no window at all. The parent sets [`ENV`] on the child; the child sees
//! it, skips `forward`, and waits for the predecessor's endpoint to go quiet instead.
//!
//! **[`ENV`] is read and removed on the first line.** Left in the environment it is inherited by
//! every terminal tab, every dump tool, and by the *next* relaunch's child — which would then skip
//! `forward` for ever. Taken the way `launch::Opening` takes the handoff credential, and for the
//! same reason.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Runtime};

use crate::error::AppError;
use crate::instance;

/// What the parent sets on the copy it is starting in its place.
///
/// Its value is never read: what matters is that it is there, and that the child removes it.
const ENV: &str = "MIXLAB_RELAUNCH";

/// How long a relaunched copy waits for its predecessor to let go of the endpoint.
///
/// A predecessor still there after this is a process that is stuck, and opening anyway is the right
/// answer: `instance::serve` finds the endpoint taken, says so on stderr, and this copy runs without
/// the single-instance listener — the state a race between two ordinary starts already produces, and
/// a window on the new version rather than no window at all.
const HANDOVER_TIMEOUT: Duration = Duration::from_secs(15);

/// How often it asks. Short enough that the handover is not felt, long enough not to spin.
const HANDOVER_POLL: Duration = Duration::from_millis(100);

/// Where this window was started from.
#[derive(Debug, Clone)]
pub struct Origin {
    /// The file to start again. Inside the bundle, on macOS.
    pub executable: PathBuf,

    /// The thing an installer placed: the bundle on macOS, the executable everywhere else.
    ///
    /// This is what an update replaces, and what `UpdateApplied::directory` is the parent of.
    pub root: PathBuf,
}

static ORIGIN: OnceLock<Option<Origin>> = OnceLock::new();

/// Read this process's own executable, once, before anything can have replaced it.
///
/// Called from the first line of `run()`. Answers `false` on a machine whose operating system will
/// not name this process's executable at all — nothing here can restart anything there, and saying
/// so is better than guessing around it.
pub fn remember() -> bool {
    ORIGIN
        .get_or_init(|| {
            let executable = std::env::current_exe().ok()?;
            let root = mixengine_platform::install::application_root(&executable);
            Some(Origin { executable, root })
        })
        .is_some()
}

/// Where this window was started from, or `None` on a machine that would not say.
pub fn origin() -> Option<&'static Origin> {
    ORIGIN.get().and_then(Option::as_ref)
}

/// Whether this process is the copy another one started in its place — and forget it either way.
///
/// Read *and removed* on the first line of `run()`: see this module's third paragraph.
pub fn taking_over() -> bool {
    let present = std::env::var_os(ENV).is_some();
    std::env::remove_var(ENV);
    present
}

/// Block until the copy this one is replacing has let go of the endpoint, or until it is clear it
/// will not.
///
/// Runs on Tauri's own runtime, before the builder, exactly as `launch::forward` does.
pub fn wait_for_predecessor(identifier: &str) {
    let endpoint = instance::Endpoint::for_app(identifier);
    let deadline = Instant::now() + HANDOVER_TIMEOUT;

    tauri::async_runtime::block_on(async move {
        while Instant::now() < deadline {
            if !instance::listening(&endpoint).await {
                return;
            }
            tokio::time::sleep(HANDOVER_POLL).await;
        }
        eprintln!("mixlab: the copy being replaced is still holding the endpoint; opening anyway");
    });
}

/// Start this window again and end this process.
///
/// **The spawn comes first and the exit second**, always: a spawn that failed with the exit already
/// requested is a user left with no window and a message they never saw.
pub fn restart<R: Runtime>(app: &AppHandle<R>) -> Result<(), AppError> {
    let origin = origin().ok_or_else(|| err!("error.relaunchNoExecutable"))?;

    let mut command = std::process::Command::new(&origin.executable);
    command.env(ENV, "1");
    crate::platform::hide_console(&mut command);
    command
        .spawn()
        .map_err(|e| err!("error.relaunchFailed", message = e))?;

    app.exit(0);
    Ok(())
}

/// The ErrorBoundary's *Restart app*, and anything else that wants this window started again.
#[tauri::command]
pub fn relaunch_app(app: AppHandle) -> Result<(), AppError> {
    restart(&app)
}
