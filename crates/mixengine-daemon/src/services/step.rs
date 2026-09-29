//! Running one [`Step`], and removing whatever credential file it needed — roadmap task **T77a**.
//!
//! Lifted out of [`first_run`](super::first_run) beside the core-side move, and for the same
//! reason: a bootstrap step and a database provisioning step are run the same way, and the way is
//! not obvious enough to be written twice. The secret file is written before, removed after
//! **whatever happened**, and a non-zero exit becomes an error carrying what the program said —
//! because a database server's own message is the only thing that explains a statement it refused.

use mixengine_core::generate::step::Step;
use mixengine_platform::process::Ran;
use mixengine_proto::{Error, ErrorCode};

use crate::error::ToWire as _;

/// Run one step, and fail if it did not succeed.
///
/// **What it printed goes into the failure, and — for a step handed no credential — into the log in
/// full.** A step's output can hold a bootstrap server's own SQL error, which is the only thing that
/// says why a data directory would not be made — `mariadb-install-db`'s own summary never contains
/// it, and the runner it ran on is thrown away. The failure carries the last line, which is what a
/// client shows; but a `mariadbd` that would not start ends on `[ERROR] Aborting` and says why on
/// the line above, and CI run 34220602983 is where that one line proved to be no diagnosis at all.
/// The whole stream goes to `daemon.log`, capped, from the steps that were given nothing to leak: a
/// step fed a statement on `stdin` or through a [`SecretFile`] can have the server quote that
/// statement back, credential included, and stays at the one line ADR 0006 already tolerates.
///
/// [`SecretFile`]: mixengine_core::generate::first_run::SecretFile
pub(crate) async fn run(step: &Step) -> Result<Ran, Error> {
    let args: Vec<std::ffi::OsString> = step.args.iter().map(std::ffi::OsString::from).collect();
    let patience = step.timeout.as_duration();

    // **Written here and removed below, whatever happens in between** — roadmap task T34c. A recipe
    // that wrote it would be a recipe touching the disk, and a *step* that removed it would be a
    // cleanup that does not run when the step before it failed, which is precisely the case a
    // credential must not survive. `run/` is one of the directories `Paths::bootstrap` restricts to
    // this account on every start, which is what makes the file's exposure the keyring's own floor.
    if let Some(file) = &step.secret_file {
        tokio::fs::write(&file.path, &file.content)
            .await
            .map_err(|source| {
                Error::new(
                    ErrorCode::Internal,
                    format!(
                        "{} needs a file at {} and it could not be written: {source}",
                        step.label,
                        file.path.display()
                    ),
                )
            })?;
    }

    let ran = match &step.stdin {
        None => {
            mixengine_platform::process::run_once(
                &step.program,
                &args,
                &step.cwd,
                &step.env,
                patience,
            )
            .await
        }
        Some(input) => {
            mixengine_platform::process::run_once_with_input(
                &step.program,
                &args,
                &step.cwd,
                &step.env,
                patience,
                input,
            )
            .await
        }
    };

    if let Some(file) = &step.secret_file {
        // Best effort by construction: the step's own answer is what the caller is waiting for, and
        // a removal that failed must not replace it. What it leaves behind in that case is a file in
        // an owner-only directory, which the next attempt overwrites.
        if let Err(error) = tokio::fs::remove_file(&file.path).await {
            tracing::warn!(path = %file.path.display(), %error, "a first-run credential file could not be removed");
        }
    }

    let ran = ran.map_err(|error| error.to_wire())?;

    if ran.succeeded() {
        return Ok(ran);
    }

    if step.stdin.is_none() && step.secret_file.is_none() {
        tracing::error!(
            step = %step.label,
            program = %step.program.display(),
            said = %last_lines(ran.complaints(), SAID_AT_MOST),
            "a first-run step did not succeed; this is the end of what it said"
        );
    }

    Err(Error::new(
        ErrorCode::Internal,
        format!(
            "{} failed while {}: {}{}",
            step.program.display(),
            step.label,
            ran.exit().map_or_else(
                || "it ran out of time".to_owned(),
                |exit| format!("it exited with {exit}")
            ),
            ran.complaint()
                .map_or_else(String::new, |said| format!(": {said}")),
        ),
    ))
}

/// How many lines of a failed step's output the log keeps.
///
/// Forty is more than a `mariadbd` prints between its first `[ERROR]` and `Aborting` — measured at
/// nine for the InnoDB failures of T33a — and less than a system-table load that fails on every
/// statement, which would otherwise be a thousand lines in a log read by eye.
const SAID_AT_MOST: usize = 40;

/// The last `count` lines of `text`, as one string.
///
/// Whole lines from the end rather than a byte count from the end, because the line a reader wants
/// is the *first* of the interesting ones and a cut through the middle of it is the cut that loses
/// the error code.
fn last_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(count);
    lines[from..].join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use mixengine_core::generate::first_run;
    use mixengine_proto::Millis;

    use super::*;

    /// The program that prints a file, on this system.
    ///
    /// A step reading its own [`SecretFile`] is the only way to assert the file was *there* when it
    /// ran: a program that could not open it exits non-zero, and [`run`] answers with the failure.
    fn printing_a_file(path: &Path) -> (std::path::PathBuf, Vec<String>) {
        if cfg!(windows) {
            (
                std::path::PathBuf::from(r"C:\Windows\System32\cmd.exe"),
                vec![
                    "/c".to_owned(),
                    "type".to_owned(),
                    path.display().to_string(),
                ],
            )
        } else {
            (
                std::path::PathBuf::from("/bin/cat"),
                vec![path.display().to_string()],
            )
        }
    }

    /// A step's secret file exists while it runs, and is gone the moment it is over.
    ///
    /// **Both halves are the point** — roadmap task **T34c**. MySQL takes the statement that sets
    /// its root password as a *path*, so the file has to exist; and a generated credential left
    /// lying in the home afterwards would be the thing ADR 0006 exists to prevent.
    #[tokio::test]
    async fn a_secret_file_is_there_for_the_step_and_gone_after_it() {
        let home = tempfile::tempdir().expect("a directory");
        let path = home.path().join("init.sql");
        let (program, args) = printing_a_file(&path);

        run(&Step {
            label: "set the root password".to_owned(),
            program,
            args,
            stdin: None,
            secret_file: Some(first_run::SecretFile {
                path: path.clone(),
                content: "ALTER USER 'root'@'localhost' IDENTIFIED BY 'hunter2';
"
                .to_owned(),
            }),
            env: BTreeMap::new(),
            cwd: home.path().to_path_buf(),
            timeout: Millis::from_secs(30),
        })
        .await
        .expect("the program read the file it was given");

        assert!(
            !path.exists(),
            "a file holding a generated password outlived the step that needed it"
        );
    }

    /// And gone after a step that failed, which is the case a cleanup step could not cover.
    #[tokio::test]
    async fn a_secret_file_is_removed_even_when_the_step_fails() {
        let home = tempfile::tempdir().expect("a directory");
        let path = home.path().join("init.sql");
        let (program, _) = printing_a_file(&path);
        let (_, missing) = printing_a_file(&home.path().join("nothing-here.sql"));

        run(&Step {
            label: "set the root password".to_owned(),
            program,
            args: missing,
            stdin: None,
            secret_file: Some(first_run::SecretFile {
                path: path.clone(),
                content: "ALTER USER 'root'@'localhost' IDENTIFIED BY 'hunter2';
"
                .to_owned(),
            }),
            env: BTreeMap::new(),
            cwd: home.path().to_path_buf(),
            timeout: Millis::from_secs(30),
        })
        .await
        .expect_err("the program was pointed at a file that is not there");

        assert!(!path.exists(), "the failure left the credential on disk");
    }
}
