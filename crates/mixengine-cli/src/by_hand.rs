//! Raising the elevation prompt from this terminal, through `sudo` — ADR 0065.
//!
//! **What is decided here is only the route, never what runs as root.** The daemon writes the
//! request and waits for its report; this side runs `sudo` on it because this process is the one
//! holding the person's terminal. The program it runs is the helper installed where
//! [`mixengine_platform::install::helper_path`] says, owned by root — never a path the daemon
//! names, so a compromised daemon cannot choose what a person types their password for.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mixengine_proto::{
    ByHand, ElevationStatus, Error, ErrorCode, JobId, JobQuery, JobSummary, JobWait, Millis, rpc,
};

use crate::client::{Client, ask, encode};

/// How a grant's one prompt is to be raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// The daemon raises the operating system's dialog, as before ADR 0065.
    Dialog,

    /// This terminal runs `sudo` on the request the daemon writes.
    ByHand,
}

impl Route {
    /// The `by_hand` flag the daemon is sent.
    pub(crate) fn by_hand(self) -> bool {
        self == Self::ByHand
    }
}

/// What this process knows about where it is running.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Terminal {
    /// Is standard input a terminal a person can answer `sudo` on?
    pub(crate) tty: bool,

    /// Is that person behind SSH, where a dialog would land on a screen they cannot see?
    pub(crate) ssh: bool,

    /// Is there a `sudo` to run? Never on Windows, where UAC has no terminal form.
    pub(crate) unix: bool,
}

impl Terminal {
    /// This process, now.
    pub(crate) fn here() -> Self {
        use std::io::IsTerminal as _;

        Self {
            tty: std::io::stdin().is_terminal(),
            ssh: std::env::var_os("SSH_CONNECTION").is_some()
                || std::env::var_os("SSH_TTY").is_some(),
            unix: cfg!(unix),
        }
    }
}

/// Which way to ask, decided **before** anybody is asked `continue? [y/N]`.
///
/// A terminal behind SSH goes by hand even where the daemon could raise a dialog: on macOS that
/// dialog appears on the Mac's own screen. `--json` never does — it has nobody to type a password.
///
/// # Errors
///
/// `privileged_required` when neither a dialog nor this terminal can ask, naming `rerun`: the
/// command that works from a terminal.
pub(crate) fn route(
    status: &ElevationStatus,
    terminal: Terminal,
    json: bool,
    rerun: &str,
) -> Result<Route, Error> {
    let can_type = terminal.unix && terminal.tty && !json;

    if can_type && (terminal.ssh || !status.can_prompt) {
        return Ok(Route::ByHand);
    }

    if status.can_prompt {
        return Ok(Route::Dialog);
    }

    Err(Error::new(
        ErrorCode::PrivilegedRequired,
        "this machine cannot raise an elevation prompt, and this command has no terminal to ask in",
    )
    .with_hint(format!("run it again from a terminal:  {rerun}")))
}

/// Refuse to put `helper` behind `sudo` unless it is at one of the `allowed` places, and root's.
///
/// **Both, and the first is not optional.** Root owns `/usr/bin/bash` too, and handed a request it
/// would run it as a script. The places are the ones this machine compiles in: where the system
/// runs the helper, and where a release ships a copy beside the daemon, which the daemon falls back
/// to for a batch the installed helper cannot read.
///
/// # Errors
///
/// `privileged_required`, naming where a helper would be trusted.
pub(crate) fn trusted(
    helper: &str,
    allowed: &[PathBuf],
    root_owns: impl Fn(&Path) -> bool,
) -> Result<(), Error> {
    let helper = Path::new(helper);

    if !allowed.iter().any(|place| place == helper) {
        let places: Vec<String> = allowed
            .iter()
            .map(|place| place.display().to_string())
            .collect();

        return Err(Error::new(
            ErrorCode::PrivilegedRequired,
            format!(
                "a terminal runs only a helper a release installed ({}), and this daemon would use {}",
                places.join(", "),
                helper.display()
            ),
        )
        .with_hint("install a MixEngine release, or run this where a dialog can be raised"));
    }

    if !root_owns(helper) {
        return Err(Error::new(
            ErrorCode::PrivilegedRequired,
            format!(
                "the helper at {} is not root's alone, so it is not run with sudo",
                helper.display()
            ),
        ));
    }

    Ok(())
}

/// Where the window's bundle is named on macOS — `mixengine_core::window::BUNDLE`, spelled here
/// because this crate reaches no further into the workspace than the platform layer.
const BUNDLE: &str = "MixLab.app";

/// [`trusted`], asked of this machine.
///
/// # Errors
///
/// As [`trusted`], and when this machine will not name its installed helper.
pub(crate) fn trusted_here(helper: Option<&str>) -> Result<(), Error> {
    let installed = mixengine_platform::install::helper_path().map_err(|error| {
        Error::new(
            ErrorCode::PrivilegedRequired,
            format!("this machine will not name its installed helper: {error}"),
        )
    })?;

    // Beside this `mix`, which a release puts beside `mixengined`: the same list the daemon's
    // fallback is read from.
    let mut allowed = vec![installed];
    if let Ok(program) = std::env::current_exe() {
        allowed.extend(mixengine_platform::install::helper_sources(
            &program, BUNDLE,
        ));
    }

    trusted(helper.unwrap_or("no helper at all"), &allowed, root_alone)
}

/// Does root own `path`, with nobody else able to write it or the directory it is in?
fn root_alone(path: &Path) -> bool {
    use mixengine_platform::elevated::{others_can_write, owner_of};

    let roots = |path: &Path| owner_of(path).is_ok_and(|owner| owner.is_superuser());
    let closed = |path: &Path| others_can_write(path).is_ok_and(|open| !open);

    roots(path)
        && closed(path)
        && path
            .parent()
            .is_some_and(|directory| roots(directory) && closed(directory))
}

/// What a person would type: the line `mix elevation status` shows while a grant waits.
pub(crate) fn command(waiting: &ByHand) -> String {
    format!("sudo -- {} {}", waiting.helper, waiting.request)
}

/// What is said just before `sudo` asks: that it will, and the line it is running.
fn announcement(waiting: &ByHand) -> String {
    format!(
        "sudo will ask for your password, running:\n  {}",
        command(waiting)
    )
}

/// How long a grant is given to publish what it is waiting for.
const PUBLISHED_WITHIN: Duration = Duration::from_secs(30);

/// How long, after `sudo` failed, the daemon is given to find a report the helper did leave.
const REPORT_WITHIN: Duration = Duration::from_secs(3);

/// Run the helper on the request `job` is waiting for, through `sudo`, in this terminal.
///
/// Returns once the daemon has the report, with [`None`] — the job is still to be followed — or
/// with the job itself when it ended without asking for anything, which an uninstall with nothing
/// outside the home does. A `sudo` that failed without the helper leaving a report, and a Ctrl-C,
/// cancel the job: the daemon counts that as a decline and keeps every row.
///
/// # Errors
///
/// What [`trusted_here`] refuses; `sudo` that could not start or did not run the helper.
pub(crate) async fn run_waiting(
    client: &mut Client,
    job: JobId,
) -> Result<Option<JobSummary>, Error> {
    let waiting = match published(client, job).await? {
        Published::Waiting(waiting) => waiting,
        Published::Ended(summary) => return Ok(summary),
    };

    if let Err(error) = trusted_here(Some(&waiting.helper)) {
        cancel(client, job).await;
        return Err(error);
    }

    let _ = writeln!(std::io::stderr(), "{}", announcement(&waiting));

    let mut sudo = tokio::process::Command::new("sudo");
    sudo.arg("--").arg(&waiting.helper).arg(&waiting.request);

    let ran = tokio::select! {
        ran = sudo.status() => ran,
        _ = tokio::signal::ctrl_c() => {
            cancel(client, job).await;
            return Err(Error::new(
                ErrorCode::Conflict,
                "interrupted; nothing was applied, and everything is still waiting",
            ));
        }
    };

    let status = match ran {
        Ok(status) => status,
        Err(source) => {
            cancel(client, job).await;
            return Err(Error::new(
                ErrorCode::PrivilegedRequired,
                format!("sudo could not be started ({source}); nothing was applied"),
            ));
        }
    };

    if status.success() || !still_waiting(client, job, REPORT_WITHIN).await? {
        return Ok(None);
    }

    cancel(client, job).await;

    Err(Error::new(
        ErrorCode::PrivilegedRequired,
        format!(
            "sudo did not run the helper to the end ({status}); nothing was applied, and \
             everything is still waiting"
        ),
    ))
}

/// What a job turned out to be doing when it was looked at.
enum Published {
    /// Waiting for this command.
    Waiting(ByHand),

    /// Not waiting for anything: ended, with the job as it ended when the daemon still answered,
    /// or still running past [`PUBLISHED_WITHIN`] without asking — either way, followed as usual.
    Ended(Option<JobSummary>),
}

/// What `job` is waiting for, once `elevation.status` says.
///
/// A daemon that stops answering is a job that ended: a finished uninstall takes the daemon with
/// it, and there was nothing to run.
async fn published(client: &mut Client, job: JobId) -> Result<Published, Error> {
    let gives_up = std::time::Instant::now() + PUBLISHED_WITHIN;

    loop {
        let Ok(status) = ask::<ElevationStatus>(client, rpc::method::ELEVATION_STATUS, None).await
        else {
            return Ok(Published::Ended(None));
        };
        if let Some(waiting) = status.waiting.filter(|waiting| waiting.job == job) {
            return Ok(Published::Waiting(waiting));
        }

        let Ok(summary) = waited(client, job).await else {
            return Ok(Published::Ended(None));
        };
        if summary.state.is_finished() {
            return Ok(Published::Ended(Some(summary)));
        }
        if std::time::Instant::now() >= gives_up {
            return Ok(Published::Ended(None));
        }
    }
}

/// Is `job` still waiting for its report after `within`?
async fn still_waiting(client: &mut Client, job: JobId, within: Duration) -> Result<bool, Error> {
    let gives_up = std::time::Instant::now() + within;

    loop {
        let status: ElevationStatus = ask(client, rpc::method::ELEVATION_STATUS, None).await?;
        if status.waiting.is_none_or(|waiting| waiting.job != job) {
            return Ok(false);
        }

        if std::time::Instant::now() >= gives_up {
            return Ok(true);
        }

        waited(client, job).await?;
    }
}

/// `job` after a short `job.wait`, which is also these loops' pause.
async fn waited(client: &mut Client, job: JobId) -> Result<JobSummary, Error> {
    ask(
        client,
        rpc::method::JOB_WAIT,
        encode(&JobWait {
            job,
            timeout: Millis(200),
        }),
    )
    .await
}

/// Ask the daemon to stop waiting. Its failing changes nothing: the wait runs out on its own.
async fn cancel(client: &mut Client, job: JobId) {
    let _ = ask::<JobSummary>(client, rpc::method::JOB_CANCEL, encode(&JobQuery { job })).await;
}

#[cfg(test)]
mod tests {

    use mixengine_proto::{ByHand, ElevationStatus, ErrorCode, JobId};

    use super::*;

    fn status(can_prompt: bool) -> ElevationStatus {
        ElevationStatus {
            elevated: false,
            can_prompt,
            reason: (!can_prompt).then(|| "no polkit agent".to_owned()),
            helper: Some("/usr/local/libexec/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: Vec::new(),
            last: None,
            waiting: None,
        }
    }

    const OVER_SSH: Terminal = Terminal {
        tty: true,
        ssh: true,
        unix: true,
    };

    const AT_THE_DESK: Terminal = Terminal {
        tty: true,
        ssh: false,
        unix: true,
    };

    const IN_A_SCRIPT: Terminal = Terminal {
        tty: false,
        ssh: false,
        unix: true,
    };

    #[test]
    fn ssh_with_a_tty_goes_by_hand_even_where_a_dialog_could_be_raised() {
        assert_eq!(
            route(&status(true), OVER_SSH, false, "mix elevation grant").unwrap(),
            Route::ByHand
        );
    }

    #[test]
    fn no_dialog_and_a_tty_goes_by_hand() {
        assert_eq!(
            route(&status(false), AT_THE_DESK, false, "mix elevation grant").unwrap(),
            Route::ByHand
        );
    }

    #[test]
    fn a_dialog_and_no_ssh_stays_a_dialog() {
        assert_eq!(
            route(&status(true), AT_THE_DESK, false, "mix elevation grant").unwrap(),
            Route::Dialog
        );
        assert_eq!(
            route(&status(true), IN_A_SCRIPT, false, "mix elevation grant").unwrap(),
            Route::Dialog
        );
    }

    #[test]
    fn no_dialog_and_no_tty_is_refused_with_the_command_to_rerun() {
        let error = route(&status(false), IN_A_SCRIPT, false, "mix uninstall")
            .expect_err("nothing can ask here");

        assert_eq!(error.code, ErrorCode::PrivilegedRequired);
        assert!(
            error.message.contains("no terminal to ask in"),
            "{}",
            error.message
        );
        assert!(error.to_string().contains("mix uninstall"), "{error}");
    }

    #[test]
    fn json_never_goes_by_hand() {
        assert!(route(&status(false), OVER_SSH, true, "mix elevation grant").is_err());
        assert_eq!(
            route(&status(true), OVER_SSH, true, "mix elevation grant").unwrap(),
            Route::Dialog
        );
    }

    #[test]
    fn windows_never_goes_by_hand() {
        let windows = Terminal {
            unix: false,
            ..OVER_SSH
        };

        assert_eq!(
            route(&status(true), windows, false, "mix elevation grant").unwrap(),
            Route::Dialog
        );
        assert!(route(&status(false), windows, false, "mix elevation grant").is_err());
    }

    const INSTALLED: &str = "/usr/local/libexec/mixengine/mixengine-elevate";
    const SHIPPED: &str = "/usr/bin/mixengine-elevate";

    /// The two places a release puts the helper: where the system runs it, and beside the daemon.
    fn allowed() -> Vec<std::path::PathBuf> {
        vec![INSTALLED.into(), SHIPPED.into()]
    }

    #[test]
    fn the_installed_root_owned_helper_is_trusted() {
        assert!(trusted(INSTALLED, &allowed(), |_| true).is_ok());
    }

    /// A batch the installed helper cannot read is run by the copy the package put beside the
    /// daemon, and a package's files are root's as much as the installed one is.
    #[test]
    fn the_root_owned_copy_shipped_beside_the_daemon_is_trusted() {
        assert!(trusted(SHIPPED, &allowed(), |_| true).is_ok());
    }

    #[test]
    fn a_helper_at_neither_place_is_refused() {
        let error = trusted(
            "/home/me/mixlab/target/debug/mixengine-elevate",
            &allowed(),
            |_| true,
        )
        .expect_err("a path from the daemon is not run as root");

        assert_eq!(error.code, ErrorCode::PrivilegedRequired);
        assert!(error.to_string().contains(INSTALLED), "{error}");
    }

    /// Root owning a file is not enough on its own: `/usr/bin/bash` is root's, and handed a request
    /// it would run it as a script.
    #[test]
    fn a_root_owned_program_that_is_not_a_helper_is_refused() {
        assert!(trusted("/usr/bin/bash", &allowed(), |_| true).is_err());
    }

    #[test]
    fn a_helper_root_does_not_own_is_refused() {
        assert!(trusted(INSTALLED, &allowed(), |_| false).is_err());
        assert!(trusted(SHIPPED, &allowed(), |_| false).is_err());
    }

    /// Said once, just before `sudo` asks: what is about to run, without reading as "to run sudo".
    #[test]
    fn the_announcement_names_the_command_being_run() {
        let waiting = ByHand {
            job: JobId(4),
            helper: INSTALLED.to_owned(),
            request: "/home/me/.mixengine/run/elevate/abc/request.json".to_owned(),
        };

        assert_eq!(
            announcement(&waiting),
            format!(
                "sudo will ask for your password, running:\n  sudo -- {INSTALLED} \
                 /home/me/.mixengine/run/elevate/abc/request.json"
            )
        );
    }

    #[test]
    fn the_command_is_sudo_on_the_two_paths() {
        let waiting = ByHand {
            job: JobId(4),
            helper: "/usr/local/libexec/mixengine/mixengine-elevate".to_owned(),
            request: "/home/me/.mixengine/run/elevate/abc/request.json".to_owned(),
        };

        assert_eq!(
            command(&waiting),
            "sudo -- /usr/local/libexec/mixengine/mixengine-elevate \
             /home/me/.mixengine/run/elevate/abc/request.json"
        );
    }
}
