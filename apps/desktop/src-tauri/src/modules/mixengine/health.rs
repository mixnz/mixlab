//! Whether the daemon is there, and if not, starting it.
//!
//! **Three states, not two**, and the UI draws three different things: *not running* (cannot dial,
//! but `mixengined` is on the machine), *not responding* (dials, but `/health` does not finish),
//! *no MixEngine* (the program was not found). Folding all three into "error" makes the user guess
//! whether they have to install, start or wait.
//!
//! `/health` needs no authentication — that is exactly why it exists on MixEngine's side: so a
//! client can decide whether to start the daemon itself.

use std::ffi::OsString;
use std::process::Command;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::error::AppError;

use super::rpc;

/// The daemon's bare name — an entry of `MIX_BINARIES`, in the form
/// `mixengine_platform::install::program_path` takes, with no executable extension.
const DAEMON: &str = "mixengined";

/// The program to start a daemon with: where this machine really has it, otherwise the bare name
/// for `PATH`.
///
/// **The list this module used to keep for itself now belongs to `mixengine-platform`** — roadmap
/// task **T107**. The measurement that justified it still holds and went over with it: on the
/// machine this module was written on, MixEngine lives in `%LOCALAPPDATA%\Programs\MixEngine` and
/// is **not** on this process's `PATH`, because the Windows installer is a per-user one that edits
/// the user's `PATH`, while a running process carries the `PATH` it inherited when it opened. What
/// is new is that the daemon, the packaging scripts and this window now read **one** answer instead
/// of three.
///
/// The bare name stays as the last resort: a `PATH` entry that appeared after this process started
/// is still worth one spawn, and a failed spawn is reported in words.
fn program() -> OsString {
    mixengine_platform::install::program_path(DAEMON).map_or_else(|| DAEMON.into(), OsString::from)
}

/// What state the daemon is in, seen from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Presence {
    Running,
    NotAnswering,
    NotRunning,
    NotInstalled,
}

/// What the tab's gate draws, and when nothing was found, where we looked — roadmap task **T111**.
///
/// `searched` is the list [`mixengine_platform::install::program_path`] went through, in order, and
/// is only filled for [`Presence::NotInstalled`]: the other three states search nothing, so an
/// empty list says exactly that instead of a list the other side has to remember to ignore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceReport {
    pub presence: Presence,
    pub searched: Vec<String>,
}

/// How long before the daemon counts as not responding.
///
/// `/health` is a read that touches no disk on the other end; two seconds is generous enough that
/// only a truly stuck daemon hits it, and short enough that the tab's gate does not freeze.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

/// What state the daemon is in.
///
/// **Exactly one dial.** The previous version dialled once just to ask "is anyone there", threw the
/// connection away, then dialled again for `/health` — and on Windows that wasted dial ate exactly
/// the pipe instance that was waiting, so the second one met a daemon that had not yet set up its
/// replacement. The result was "daemon not responding" on a machine where the daemon was running
/// normally. The question "is anyone there" is already inside `/health`'s answer, so asking it
/// separately adds nothing but a bug.
///
/// **Since T111 the answer carries where we looked for the daemon**, when it was not found.
pub async fn presence() -> PresenceReport {
    let presence =
        match tokio::time::timeout(HEALTH_TIMEOUT, rpc::request("GET", "/health", None)).await {
            Ok(Ok(_)) => Presence::Running,
            // Cannot reach the endpoint: not running, or not installed. Those are two different
            // answers.
            Ok(Err(error)) if error.code == "error.mixengineUnreachable" => {
                if installed() {
                    Presence::NotRunning
                } else {
                    Presence::NotInstalled
                }
            }
            // Reachable but does not finish: a stuck daemon, or a pipe belonging to another
            // account.
            _ => Presence::NotAnswering,
        };

    let searched = if presence == Presence::NotInstalled {
        searched()
    } else {
        Vec::new()
    };

    PresenceReport { presence, searched }
}

/// The directories the search went through, in the form the tab's gate prints them.
fn searched() -> Vec<String> {
    mixengine_platform::install::program_search_dirs()
        .iter()
        .map(|dir| dir.display().to_string())
        .collect()
}

/// Whether `mixengined` is on this machine. Only asked once we know we cannot dial.
///
/// **A few disk reads, not a process** — roadmap task **T107**. The previous version ran
/// `mixengined --version` and threw the output away: one process creation, one console window to
/// hide by hand, and up to a second while the tab opens, to answer a question a single `stat`
/// answers — and `program_path` asks `PATH` by reading it, not by running a program on it.
fn installed() -> bool {
    mixengine_platform::install::program_path(DAEMON).is_some()
}

/// The four directories the user chose a location for, in the form the start command takes —
/// roadmap task **T146**.
///
/// **Only the keys someone actually changed**, not all four every time: the daemon treats a value
/// equal to what `config.toml` already holds as a silent no-op, but sending four keys for a
/// one-key change is saying three things we do not mean.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChosenPaths {
    pub runtimes: Option<String>,
    pub packages: Option<String>,
    pub data: Option<String>,
    pub logs: Option<String>,
}

impl ChosenPaths {
    /// The four keys and their values, in the order `config.toml` lists them.
    ///
    /// The flag names **are** the key names, so this is one loop rather than four identical blocks
    /// — for the same reason `mixengined` rebuilds the argument list for its child process with a
    /// loop.
    fn entries(&self) -> [(&'static str, Option<&str>); 4] {
        [
            ("runtimes", self.runtimes.as_deref()),
            ("packages", self.packages.as_deref()),
            ("data", self.data.as_deref()),
            ("logs", self.logs.as_deref()),
        ]
    }
}

/// Where the daemon keeps its growing directories, and whether that can still be changed — roadmap
/// task **T146**.
///
/// **Answerable before any daemon exists**, which is the whole reason it runs a process instead of
/// calling JSON-RPC: the screen inviting the user to pick a drive is drawn *before* the first
/// start. `--storage` creates nothing — no home, no `config.toml`, no database — so asking is not
/// what takes the choice away.
///
/// One process per drawing of the gate, and that is why [`installed`] reads the disk instead of
/// running the program: this one pays that price for a screen someone is looking at, not for a
/// polling loop.
///
/// **`Value` rather than a `mixengine-proto` type**, following the rule `commands.rs` states for
/// the other two reads: Rust here reads no field — it forwards — and the contract already has a
/// generated copy in `bindings/` for the frontend to type against. Pulling in another crate just to
/// name something that passes straight through pays a price and buys no check.
pub async fn storage() -> Result<Value, AppError> {
    let output = tauri::async_runtime::spawn_blocking(|| {
        let mut command = Command::new(program());
        command.arg("--storage");
        crate::platform::hide_console(&mut command).output()
    })
    .await
    .map_err(|e| err!("error.mixengineStorageFailed", message = e))?
    .map_err(|e| err!("error.mixengineStorageFailed", message = e))?;

    if !output.status.success() {
        return Err(err!(
            "error.mixengineStorageFailed",
            message = String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| err!("error.mixengineStorageFailed", message = e))
}

/// Starts the daemon and returns the endpoint it prints.
///
/// `--detach` **only returns once the daemon answers on its endpoint** and prints the endpoint to
/// stdout — so there is no backoff loop here. The waiting belongs to the process that knows whether
/// its child is still alive, and that is not this process.
///
/// `chosen` is the four directories the user just picked at the gate, if any — roadmap task
/// **T146**. They go straight onto the command line rather than being written here: whether they
/// are allowed is a question about rows in the database, and this process opens no database.
pub async fn start_daemon(chosen: Option<ChosenPaths>) -> Result<String, AppError> {
    let output = tauri::async_runtime::spawn_blocking(move || {
        let mut command = Command::new(program());
        command.arg("--detach");

        for (key, directory) in chosen.unwrap_or_default().entries() {
            if let Some(directory) = directory {
                command.arg(format!("--{key}")).arg(directory);
            }
        }

        crate::platform::hide_console(&mut command).output()
    })
    .await
    .map_err(|e| err!("error.mixengineStartFailed", message = e))?
    .map_err(|e| err!("error.mixengineStartFailed", message = e))?;

    if !output.status.success() {
        return Err(err!(
            "error.mixengineStartFailed",
            message = String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The daemon is looked for exactly where the platform says MixEngine lives, and the answer on
    /// this machine is either a real path or the bare name — never an empty program.
    ///
    /// **The per-OS half of this assertion has moved.** It used to spell out
    /// `programs\mixengine\mixengined.exe` here, next to a list this module kept for itself; since
    /// T107 that list is `mixengine_platform::install::program_dirs`, and so is the test naming it.
    #[test]
    fn the_daemon_is_looked_for_where_the_platform_says_mixengine_is() {
        let program = program();

        assert!(!program.is_empty());
        match mixengine_platform::install::program_path(DAEMON) {
            Some(found) => assert_eq!(program, OsString::from(found)),
            None => assert_eq!(program, OsString::from(DAEMON)),
        }
    }

    /// The four states go over the wire in camelCase — the frontend compares strings against them,
    /// so changing the spelling here breaks the tab's gate with nothing at build time saying so.
    #[test]
    fn presence_is_camel_cased_for_the_shell() {
        let json = |value: Presence| serde_json::to_string(&value).unwrap();
        assert_eq!(json(Presence::Running), "\"running\"");
        assert_eq!(json(Presence::NotAnswering), "\"notAnswering\"");
        assert_eq!(json(Presence::NotRunning), "\"notRunning\"");
        assert_eq!(json(Presence::NotInstalled), "\"notInstalled\"");
    }

    /// The report goes over the wire as an object with two camelCase fields — the frontend reads
    /// both by name, so changing the spelling here breaks the tab's gate with nothing at build time
    /// saying so.
    #[test]
    fn the_report_is_camel_cased_for_the_shell() {
        let report = PresenceReport {
            presence: Presence::NotInstalled,
            searched: vec!["/somewhere".to_owned()],
        };

        assert_eq!(
            serde_json::to_string(&report).unwrap(),
            r#"{"presence":"notInstalled","searched":["/somewhere"]}"#
        );
    }

    /// The search starts right next to this program — the first step of T107, and also exactly
    /// the directory `npm run dev:app` copies the daemon into (T111).
    #[test]
    fn where_it_looked_begins_beside_this_program() {
        let running = std::env::current_exe().expect("this test has a path");
        let beside = running
            .parent()
            .expect("and a directory")
            .display()
            .to_string();

        assert_eq!(searched().first(), Some(&beside));
    }
}
