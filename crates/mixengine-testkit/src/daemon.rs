//! A `mixengined` a test started, and ends when it is done with it.
//!
//! **The binary is an argument.** `CARGO_BIN_EXE_mixengined` reaches only a test of the package that
//! builds the daemon, so this crate cannot name it: each suite passes its own `env!` in. That is the
//! whole reason the suites used to carry a copy of this type each.
//!
//! Nothing here waits for the endpoint. The suites do not agree on when to: most wait right after
//! the start, some declare services first, and the CLI's waits through `mix status`. A start returns
//! as soon as the OS has a process, and [`Home::wait_until_listening`] is the wait.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::Home;
use crate::home::SHUTDOWN;

/// A `mixengined` this test started and is still holding, ended when it drops.
///
/// Ended by [`end_daemon`](crate::end_daemon): asked to stop first, killed only if it does not go.
/// Asked because on macOS a killed daemon leaves the services it started running for good; killed
/// in the end because a test that failed halfway must still not leave a process holding the
/// temporary home open.
#[derive(Debug)]
pub struct Daemon(Child);

impl Daemon {
    /// `binary --home <home>`, stdout and stderr to null, not yet spawned.
    ///
    /// For a suite that adds arguments, points stderr at a file or removes a variable before the
    /// start. Silenced rather than inherited: a passing test prints nothing, and the daemon's own
    /// log inside the home is the better record.
    #[must_use]
    pub fn command(binary: impl AsRef<Path>, home: &Home) -> Command {
        let mut command = Command::new(binary.as_ref());
        command
            .arg("--home")
            .arg(home.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    /// Spawn what [`Daemon::command`] built.
    ///
    /// # Panics
    ///
    /// If the binary cannot be run.
    #[must_use]
    pub fn spawn(mut command: Command) -> Self {
        Self(command.spawn().expect("the daemon binary runs"))
    }

    /// Start `binary` against `home`, with nothing added.
    ///
    /// # Panics
    ///
    /// If the binary cannot be run.
    #[must_use]
    pub fn start(binary: impl AsRef<Path>, home: &Home) -> Self {
        Self::spawn(Self::command(binary, home))
    }

    /// The process it is running as.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.0.id()
    }

    /// Wait for it to end **by itself**, and say whether it did.
    ///
    /// What proves a shutdown happened is this process leaving: an answer to a stop arrives while
    /// the daemon is still there, on purpose. Polled rather than `wait`ed on, because a daemon that
    /// never goes has to fail its test rather than hang the suite.
    pub fn wait_until_gone(&mut self) -> bool {
        let deadline = std::time::Instant::now() + SHUTDOWN;

        while std::time::Instant::now() < deadline {
            if self.0.try_wait().is_ok_and(|exit| exit.is_some()) {
                return true;
            }

            std::thread::sleep(Duration::from_millis(25));
        }

        false
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        crate::end_daemon(&mut self.0);
    }
}
