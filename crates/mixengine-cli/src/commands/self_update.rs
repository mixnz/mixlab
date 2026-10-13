//! `mix self-update` — the headless distribution's updater.

use std::io::Write as _;
use std::process::ExitCode;

use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    Error, ErrorCode, UpdateApplied, UpdateApply, UpdateCheck, UpdateDecide, UpdateDecision,
    UpdateFinish, UpdateHandOver, UpdateHandedOver, UpdatePlacement, UpdateStatus, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::confirm::unanswered;
use crate::output::{emit, rendered};
use crate::{confirm, render};

/// What `mix self-update` was asked to do: its three flags.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SelfUpdateAsk {
    pub(crate) check: bool,
    pub(crate) yes: bool,
    pub(crate) finish: bool,
}

/// `mix self-update` — roadmap task **T88**.
///
/// **The daemon does the update and this waits for it.** `mix` may depend on `mixengine-platform`
/// and `mixengine-proto` and on nothing else, and verifying a signature, unpacking an archive and
/// swapping files are all `mixengine-core`'s. What has to outlive the daemon is the *client*, and
/// what it has to do afterwards is exactly one thing — start the new one — which is
/// [`Autostart::run`].
///
/// The sequence, and which process performs each step:
///
/// | # | Step | Who |
/// | --- | --- | --- |
/// | 1 | take the lock, so two of these cannot interleave | `mix` |
/// | 2 | `update.check` → version, notes, size, what will be restarted | daemon |
/// | 3 | prompt: *install / skip this version / remind me later* | `mix` |
/// | 4 | `update.apply`: download, verify, smoke-test, stop, swap, answer, exit | daemon |
/// | 5 | wait for the endpoint to stop answering | `mix` |
/// | 6 | `Autostart::run` — start the new daemon and return when it listens | `mix` |
/// | 7 | start the services that were stopped; clean up `.old` | new daemon |
///
/// **Step 6 deliberately does not open a [`Client`] afterwards.** The new daemon may speak a newer
/// protocol than the `mix` still running from the old image, and a protocol-mismatch error at the
/// end of a successful update would be the worst possible last line. `--detach` exiting zero *is*
/// the readiness probe, which is what `autostart.rs` already documents.
pub(crate) async fn self_update(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    asked: SelfUpdateAsk,
) -> Result<ExitCode, Error> {
    let SelfUpdateAsk { check, yes, finish } = asked;

    // **Held for the whole of this and not for the call**, because what must not interleave is the
    // swap and the relaunch: two of these racing would have the second find `.old` files written by
    // the first and a daemon that is in the middle of being replaced. `platform::lock` is the
    // mechanism the daemon already uses for its own single-instance guarantee.
    let _lock = match check {
        true => None,
        false => update_lock()?,
    };

    let mut client = Client::connect(endpoint, autostart).await?;

    // The second half of a `.pkg` update — T88f. Refused by the daemon, in a sentence, until
    // Installer.app has put the new binaries on disk.
    if finish {
        let applied: UpdateApplied = ask(
            &mut client,
            rpc::method::UPDATE_FINISH,
            encode(&UpdateFinish {}),
        )
        .await?;

        return relaunch(client, endpoint, autostart, json, &applied).await;
    }

    let status: UpdateStatus = ask(
        &mut client,
        rpc::method::UPDATE_CHECK,
        encode(&UpdateCheck { force: true }),
    )
    .await?;

    if check {
        emit(&rendered(json, &status, || render::update_status(&status)))?;
        return Ok(ExitCode::SUCCESS);
    }

    // **One document per run under `--json`, and the offer is not it.** The status is printed here
    // so a person can read what they are about to agree to; a caller reading JSON is not being asked
    // anything, and two objects on one stdout is not JSON at all — `mix uninstall`'s rule, and its
    // reason. What a `--json` run gets instead is whichever of the two below turns out to be the
    // answer.
    if !json {
        emit(&render::update_status(&status))?;
    }

    // **Neither of these is a failure.** A machine that is up to date and a copy of MixEngine that
    // `apt` installed are both perfectly healthy, and the reason has just been printed — or is about
    // to be, as the one document a `--json` caller gets.
    //
    // A copy the `.pkg` installed reads `managed` and carries an installer, and is not refused (T88f).
    let refused = !status.offered
        || (matches!(status.placement, UpdatePlacement::Managed { .. })
            && status.installer.is_none());

    if refused || status.available.is_none() {
        if json {
            emit(&rendered(json, &status, || render::update_status(&status)))?;
        }

        return Ok(ExitCode::SUCCESS);
    }

    let release = status.available.clone().expect("checked one line above");

    if !yes {
        let answer = chosen_update(&release.version, json)?;

        if let Some(decision) = answer.decision() {
            let answered: UpdateStatus = ask(
                &mut client,
                rpc::method::UPDATE_DECIDE,
                encode(&UpdateDecide {
                    version: release.version.clone(),
                    decision,
                }),
            )
            .await?;

            emit(&rendered(json, &answered, || {
                render::update_status(&answered)
            }))?;
            return Ok(ExitCode::SUCCESS);
        }
    }

    // The `.pkg` path: the daemon downloads, checks and opens it, and keeps running. What finishes
    // the update is `--finish`, once the person has been through Installer.app (T88f, D8).
    if status.installer.is_some() {
        let handed: UpdateHandedOver = ask(
            &mut client,
            rpc::method::UPDATE_HAND_OVER,
            encode(&UpdateHandOver {
                version: release.version.clone(),
            }),
        )
        .await?;

        emit(&rendered(json, &handed, || {
            render::update_handed_over(&handed)
        }))?;
        return Ok(ExitCode::SUCCESS);
    }

    let applied: UpdateApplied = ask(
        &mut client,
        rpc::method::UPDATE_APPLY,
        encode(&UpdateApply {
            version: release.version.clone(),
        }),
    )
    .await?;

    relaunch(client, endpoint, autostart, json, &applied).await
}

/// What follows an update's answer, whichever way the binaries were replaced: wait for the old
/// daemon to go, and start the new one.
async fn relaunch(
    client: Client,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    applied: &UpdateApplied,
) -> Result<ExitCode, Error> {
    // The daemon answered and is on its way out. From here the connection is worthless: it belongs
    // to a process that has already committed to exiting.
    drop(client);

    emit(&rendered(json, applied, || render::update_applied(applied)))?;

    // **Bounded.** A daemon that has answered `update.apply` has already stopped its services and
    // cancelled its token, so anything longer than this is a supervised process refusing to die —
    // which does not stop the relaunch. Past the timeout the new daemon is started anyway: the
    // endpoint is a socket it will rebind or a pipe it will re-create, and a user left with no
    // daemon at all is worse than a second one failing to start and saying so.
    if !Client::gone(endpoint, RELAUNCH_WAIT).await {
        let _ = writeln!(
            std::io::stderr(),
            "the old daemon has not stopped answering yet; starting the new one anyway"
        );
    }

    let Some(autostart) = autostart else {
        // `--no-autostart` on the one command whose whole ending is starting a daemon. Said rather
        // than silently skipped: the update happened, and the machine is one command short of
        // having a daemon again.
        let _ = writeln!(
            std::io::stderr(),
            "MixEngine {} is installed. --no-autostart was given, so start the daemon yourself: \
             mix status",
            applied.to
        );

        return Ok(ExitCode::SUCCESS);
    };

    if let Err(error) = autostart.run() {
        report_update_rollback(applied, &error);
        return Ok(ExitCode::FAILURE);
    }

    Ok(ExitCode::SUCCESS)
}

/// How long `mix self-update` waits for the daemon it just replaced to stop answering.
const RELAUNCH_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// The lock `mix self-update` and MixLab's updater both take — T187, spec D7.
const UPDATE_LOCK: &str = "update.lock";

/// Take the update lock beside the binaries, or say who has it.
///
/// **Beside the binaries, not in the home**, since T187. A lock belongs to what is being changed,
/// and what an update changes is the directory holding `mixengined`, which several homes can share
/// — and which MixLab's own updater, knowing nothing of homes, swaps too.
fn update_lock() -> Result<Option<mixengine_platform::lock::Lock>, Error> {
    let directory = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .ok_or_else(|| Error::new(ErrorCode::Io, "cannot tell which directory mix runs from"))?;
    update_lock_in(&directory)
}

/// [`update_lock`] in a directory the caller names, so a test needs no install.
///
/// **A directory this account cannot write holds no lock**, and the answer is `None`: nobody here
/// swaps it (a `.pkg`, a `.deb`), so there is nothing to keep two updates apart over.
fn update_lock_in(
    directory: &std::path::Path,
) -> Result<Option<mixengine_platform::lock::Lock>, Error> {
    let path = directory.join(UPDATE_LOCK);

    match mixengine_platform::lock::Lock::acquire(&path) {
        Ok(mixengine_platform::lock::Acquired::Held(lock)) => Ok(Some(lock)),
        Ok(mixengine_platform::lock::Acquired::Taken(holder)) => Err(Error::new(
            ErrorCode::PreconditionFailed,
            format!("another update is running ({holder})"),
        )
        .with_hint(
            "wait for it to finish, whether it is mix self-update or MixLab; two updates at once \
             would interleave their swaps",
        )),
        Err(mixengine_platform::Error::Io { source, .. })
            if matches!(
                source.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(crate::error::to_wire(&error)),
    }
}

/// What somebody answered to *there is a newer MixEngine*.
///
/// Three answers and not two, because *skip this version* and *remind me later* are different
/// decisions and a yes/no that meant both would be a prompt nobody could answer correctly —
/// `confirm::Choice`'s own reasoning, which T78 added for this shape of question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    /// Install it now.
    Install,
    /// Never offer this version again.
    Skip,
    /// Ask again in a few days.
    Later,
}

impl Choice {
    /// What `update.decide` is told, or [`None`] for the answer that decides nothing because it
    /// installs.
    fn decision(self) -> Option<UpdateDecision> {
        match self {
            Self::Install => None,
            Self::Skip => Some(UpdateDecision::Skip),
            Self::Later => Some(UpdateDecision::Later),
        }
    }
}

/// Ask, and read one line back.
///
/// # Errors
///
/// [`ErrorCode::PreconditionFailed`] when there is nobody to ask — `--json`, or a standard input at
/// end of file — naming the flag that says yes in advance. A script that could not be asked is told
/// what to do rather than defaulted into replacing its own binaries.
fn chosen_update(version: &str, json: bool) -> Result<Choice, Error> {
    if json {
        return Err(unanswered());
    }

    let question = format!("\ninstall MixEngine {version}? [i]nstall / [s]kip / [l]ater: ");

    match confirm::ask_update(&question) {
        Some(choice) => Ok(choice),
        None => Err(unanswered()),
    }
}

/// The one path that leaves a machine worse than it found it, reported so it can be undone by hand.
///
/// **`mix` does not attempt the rollback itself.** The files are the daemon's to place, `mix` may not
/// link `mixengine-core`, and a client that moved binaries around on a failure it does not
/// understand is a client doing something to the machine. What it can do honestly is name the two
/// files and the move that undoes the swap.
fn report_update_rollback(applied: &UpdateApplied, error: &Error) {
    let mut stderr = std::io::stderr();
    let suffix = std::env::consts::EXE_SUFFIX;

    let _ = writeln!(
        stderr,
        "\nMixEngine {} is installed, and the new daemon would not start: {error}",
        applied.to
    );
    let _ = writeln!(
        stderr,
        "the binaries it replaced are still in {}, each under its own name with .old on the end:",
        applied.directory
    );

    for name in &applied.replaced {
        let _ = writeln!(stderr, "  {name}{suffix}.old");
    }

    let _ = writeln!(
        stderr,
        "renaming each of those back over the file beside it puts this machine as it was."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixengine_proto::ErrorCode;

    /// **T187, spec D7.** The lock is the file beside the binaries that MixLab's updater takes too,
    /// and a second taker is refused with the holder named.
    #[test]
    fn the_update_lock_is_the_file_beside_the_binaries() {
        let directory = tempfile::tempdir().unwrap();

        let held = update_lock_in(directory.path()).unwrap();
        assert!(held.is_some());
        assert!(directory.path().join(UPDATE_LOCK).exists());

        let refused = update_lock_in(directory.path()).unwrap_err();
        assert_eq!(refused.code, ErrorCode::PreconditionFailed);
        assert!(refused.message.contains(&std::process::id().to_string()));
    }
}
