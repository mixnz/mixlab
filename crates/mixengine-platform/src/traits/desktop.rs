//! Finding this install's window and starting it.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::Result;

/// An application this machine has, as it would be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    /// The executable — never a bundle directory, never a bare name.
    pub program: PathBuf,
}

/// What looking for an application found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// It is here.
    Installed(InstalledApp),

    /// It is not, and this is where this install looked — phrased for a person.
    NotInstalled {
        /// Where.
        searched: String,
    },
}

/// What became of a started application after one second.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// Still up.
    Running {
        /// Its process id.
        pid: u32,
    },

    /// Exited successfully inside the judgement: a single-instance application forwarding to the
    /// copy already running, which is the common case and not a failure.
    HandedOn,

    /// Exited otherwise inside the judgement. `status` is how, rendered for a person.
    Failed {
        /// How it exited.
        status: String,
    },
}

/// Finding this install's window, and starting it.
///
/// **The one capability that starts a process the daemon does not supervise**, and the reason it is
/// a capability rather than a free function in [`crate::process`]: what a test of the handoff has
/// to see is *which program, which arguments, which variable names* — a recorder — while the OS
/// mechanism underneath (`spawn_detached`) is proved once in `tests/desktop.rs` against a shell.
///
/// # Blocking
///
/// `launch` blocks for its one-second judgement and is called through `spawn_blocking`.
pub trait DesktopApps: std::fmt::Debug + Send + Sync {
    /// The desktop application **this MixEngine install** has, if it has one.
    ///
    /// `executable` and `bundle` are what `packaging/common.sh` declares as `MIX_WINDOW` and
    /// `MIX_WINDOW_APP`; two arguments and not zero, for
    /// [`crate::install::application_file_name`]'s reason — a name packaging owns is not one this
    /// crate may hold.
    ///
    /// It looks where MixEngine's own installer writes, relative to the program that is asking, so
    /// the answer is *this install's window* and never some other install's.
    ///
    /// # Errors
    ///
    /// None today; "not installed" is a [`Located`] and not an error.
    fn locate_window(&self, executable: &str, bundle: &str) -> Result<Located>;

    /// Start `app` with `args`, `env` added to this process's environment, detached, and judged for
    /// one second.
    ///
    /// **`env` is where a credential goes** and it goes nowhere else: not into `args`, not into a
    /// log, and — for the mock — not into what is recorded.
    ///
    /// # Errors
    ///
    /// [`Error::Io`](crate::Error::Io) naming the program when it could not be started at all. An
    /// application that started and died is [`Started::Failed`], not an error.
    fn launch(
        &self,
        app: &InstalledApp,
        args: &[OsString],
        env: &BTreeMap<String, String>,
    ) -> Result<Started>;
}
