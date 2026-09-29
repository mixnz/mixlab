//! Starting a desktop application and judging it — roadmap task **T83**, D8, D9 and D11.
//!
//! One launcher for three systems, over [`crate::process::spawn_detached`]: the application inherits
//! this process's environment — a GUI needs the session, which is the opposite of what a supervised
//! child gets — plus whatever the caller adds, which is one credential or nothing.
//!
//! # The judgement
//!
//! [`JUDGEMENT`] after the spawn, the child is asked whether it has exited. Still up is
//! [`Started::Running`]. A clean exit is [`Started::HandedOn`], because that is what a single-instance
//! application does when a copy is already running: forward `argv` and exit 0. Anything else is
//! [`Started::Failed`].
//!
//! # The reaper
//!
//! On Unix a detached child stays this process's child (`setsid` only), so a daemon that never waits
//! on it leaves a zombie for as long as the daemon runs. One thread, started on the first launch,
//! polls every [`REAP_EVERY`] and drops what has ended. On Windows the same thread closes the handle.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use crate::process::{self, Detached};
use crate::{DesktopApps, InstalledApp, Located, Result, Started};

/// How long a started application is watched before it is called running.
pub(crate) const JUDGEMENT: Duration = Duration::from_secs(1);

/// How often the judgement looks.
const GLANCE: Duration = Duration::from_millis(50);

/// How often the reaper looks.
const REAP_EVERY: Duration = Duration::from_secs(2);

/// Every application started and not yet seen to exit.
static ADOPTED: OnceLock<Mutex<Vec<Detached>>> = OnceLock::new();

/// Start `app`, judge it, and hand what is still running to the reaper.
///
/// # Errors
///
/// [`Error::Io`](crate::Error::Io) naming the program when it cannot be started;
/// [`Error::Os`](crate::Error::Os) when the OS will not say whether it has exited.
pub(crate) fn launch(
    app: &InstalledApp,
    args: &[OsString],
    env: &BTreeMap<String, String>,
) -> Result<Started> {
    // The program's own directory: what Explorer and Finder give it, and never this daemon's home,
    // which the application would then pin for its whole life.
    let directory = app
        .program
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(std::env::temp_dir, std::path::Path::to_path_buf);

    let mut child = process::spawn_detached(&app.program, args, &directory, env)?;
    let pid = child.pid();
    let deadline = Instant::now() + JUDGEMENT;

    loop {
        if let Some(exit) = child.exited()? {
            return Ok(if exit.is_success() {
                Started::HandedOn
            } else {
                Started::Failed {
                    status: exit.to_string(),
                }
            });
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(GLANCE);
    }

    adopt(child);
    tracing::debug!(pid, program = %app.program.display(), "started a desktop application");

    Ok(Started::Running { pid })
}

/// The window this MixEngine install has, if it has one — roadmap task **T107**, the design's D3.
///
/// Called by [`Apps`], the one [`crate::DesktopApps`] every system uses: what differs per system is
/// `sys::install::window_dirs`, and nothing else here does.
///
/// # Errors
///
/// None today. The [`Result`] is the trait method's, kept so a lookup that could fail later does not
/// change the trait.
pub(crate) fn locate_window(executable: &str, bundle: &str) -> Result<Located> {
    let running = std::env::current_exe().ok();
    let directory = running.as_deref().and_then(std::path::Path::parent);

    Ok(window_at(directory, executable, bundle))
}

/// The [`DesktopApps`] every system's host holds — roadmap task **T165**.
///
/// One type and not three: what differs per system is `sys::install::window_dirs`, which
/// [`window_at`] already reaches, and the launcher is [`launch`] everywhere.
#[derive(Debug, Default)]
pub(crate) struct Apps;

impl DesktopApps for Apps {
    fn locate_window(&self, executable: &str, bundle: &str) -> Result<Located> {
        locate_window(executable, bundle)
    }

    fn launch(
        &self,
        app: &InstalledApp,
        args: &[OsString],
        env: &BTreeMap<String, String>,
    ) -> Result<Started> {
        launch(app, args, env)
    }
}

/// The pure half: the window belonging to an install whose programs are in `directory`.
///
/// **Neither `PATH` nor the operating system's tables.** A `mixlab` first on somebody's `PATH`, or a
/// bundle Spotlight knows about, may belong to a different install of MixEngine than the program
/// doing the asking — and then which window a database opens in would depend on the order of a
/// `PATH`.
fn window_at(directory: Option<&std::path::Path>, executable: &str, bundle: &str) -> Located {
    let placed_as = crate::install::application_file_name(executable, bundle);
    let mut roots: Vec<std::path::PathBuf> = Vec::new();

    if let Some(directory) = directory {
        roots.push(directory.to_path_buf());
    }
    roots.extend(crate::sys::install::window_dirs(directory));

    let mut looked = Vec::new();

    for root in roots {
        let placed = root.join(&placed_as);
        let program = crate::install::application_executable(&placed, executable);

        if program.is_file() {
            return Located::Installed(InstalledApp { program });
        }

        looked.push(placed.display().to_string());
    }

    Located::NotInstalled {
        searched: if looked.is_empty() {
            "nowhere; this program cannot say which directory it is running from".to_owned()
        } else {
            looked.join(" and ")
        },
    }
}

/// Hand a running child to the reaper, starting it if this is the first.
fn adopt(child: Detached) {
    let adopted = ADOPTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("mixengine-reaper".to_owned())
            .spawn(reap)
            .expect("a thread can be started");
        Mutex::new(Vec::new())
    });

    adopted
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(child);
}

/// Forever: drop every adopted child that has exited.
fn reap() {
    loop {
        std::thread::sleep(REAP_EVERY);

        if let Some(adopted) = ADOPTED.get() {
            adopted
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain_mut(|child| match child.exited() {
                    Ok(None) => true,
                    Ok(Some(exit)) => {
                        tracing::debug!(pid = child.pid(), %exit, "a desktop application ended");
                        false
                    }
                    Err(error) => {
                        tracing::debug!(
                            pid = child.pid(),
                            %error,
                            "a desktop application cannot be waited on"
                        );
                        false
                    }
                });
        }
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;

    /// A window staged the way this system's installer stages one, beside the program asking.
    ///
    /// Built through `application_file_name` and `application_executable` rather than by spelling a
    /// path, so the same test proves the bundle rule on macOS and the file rule on the other two.
    fn stage(directory: &std::path::Path) -> std::path::PathBuf {
        let placed = directory.join(crate::install::application_file_name(
            "mixlab",
            "MixLab.app",
        ));
        let program = crate::install::application_executable(&placed, "mixlab");
        std::fs::create_dir_all(program.parent().expect("a directory")).expect("the directory");
        std::fs::write(&program, b"x").expect("the program");
        program
    }

    #[test]
    fn a_window_beside_the_running_program_is_this_installs_window() {
        let temp = tempfile::tempdir().expect("a directory");
        let program = stage(temp.path());

        match window_at(Some(temp.path()), "mixlab", "MixLab.app") {
            Located::Installed(app) => {
                assert_eq!(app.program, program);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_install_with_no_window_says_where_it_looked() {
        let temp = tempfile::tempdir().expect("a directory");

        match window_at(Some(temp.path()), "mixlab", "MixLab.app") {
            Located::NotInstalled { searched } => {
                assert!(
                    searched.contains(&temp.path().display().to_string()),
                    "{searched}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    /// A program that cannot say where it is running from still answers, and says so.
    #[test]
    fn no_directory_at_all_is_an_answer_and_not_a_panic() {
        assert!(matches!(
            window_at(None, "mixlab", "MixLab.app"),
            Located::NotInstalled { .. }
        ));
    }

    /// **The rule this lookup exists to enforce**: the window belongs to the install the running
    /// program belongs to. A copy in some other directory — first on `PATH`, or one the operating
    /// system's tables know about — is not this daemon's to start.
    #[test]
    fn a_window_in_an_unrelated_directory_is_not_this_installs_window() {
        let elsewhere = tempfile::tempdir().expect("a directory");
        stage(elsewhere.path());
        let here = tempfile::tempdir().expect("a directory");

        assert!(matches!(
            window_at(Some(here.path()), "mixlab", "MixLab.app"),
            Located::NotInstalled { .. }
        ));
    }
}
