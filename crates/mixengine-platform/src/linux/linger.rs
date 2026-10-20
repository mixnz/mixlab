//! The account's systemd linger: whether its user manager, and with it the daemon's unit, starts at
//! boot and stays after logout.
//!
//! **As the account and never elevated.** systemd's own polkit policy lets a session's owner linger
//! itself (`org.freedesktop.login1.set-self-linger`), so `loginctl` is run as the user; a policy
//! that says no is answered with the command a person can run with `sudo`, by the daemon.
//!
//! **The uid is always named.** `loginctl enable-linger` with no argument means the user *of the
//! caller's session*, and a daemon started by its unit at boot is in no session at all.
//!
//! **Read through `loginctl` and not from `/var/lib/systemd/linger/`**, which is where logind keeps
//! its state today rather than its interface.

use std::process::Command;

use crate::{BootStart, Error, Result};

/// The tool. Off the `PATH`, as `systemctl` is beside it.
const LOGINCTL: &str = "loginctl";

/// What logind says about an account with no session and no linger — which is "not lingering".
const NOT_LINGERING: &str = "not logged in or lingering";

/// What this account's linger reads now. Never fails: a machine that cannot say has no boot half.
pub(crate) fn state() -> BootStart {
    let uid = current_uid().to_string();

    let Ok(output) = Command::new(LOGINCTL)
        .args(["show-user", &uid, "--property=Linger", "--value"])
        .output()
    else {
        return BootStart::Unsupported;
    };

    reading(
        output.status.success(),
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
    )
}

/// Turn this account's linger on or off, then read it back.
///
/// # Errors
///
/// [`Error::Command`] when `loginctl` cannot be started or refuses — the latter is polkit saying the
/// account may not linger itself here.
pub(crate) fn set(on: bool) -> Result<BootStart> {
    let output = Command::new(LOGINCTL)
        .args(arguments(on, current_uid()))
        .output()
        .map_err(|source| Error::Command {
            command: LOGINCTL,
            path: None,
            status: "could not be started".to_owned(),
            output: source.to_string(),
        })?;

    if !output.status.success() {
        return Err(Error::Command {
            command: LOGINCTL,
            path: None,
            status: output.status.to_string(),
            output: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    Ok(state())
}

/// `show-user`'s answer, as on, off, or nothing this code can rely on.
fn reading(success: bool, stdout: &str, stderr: &str) -> BootStart {
    match (success, stdout.trim()) {
        (true, "yes") => BootStart::On,
        (true, "no") => BootStart::Off,
        (false, _) if stderr.contains(NOT_LINGERING) => BootStart::Off,
        _ => BootStart::Unsupported,
    }
}

/// The argument vector for one change, with the uid in it.
fn arguments(on: bool, uid: u32) -> [String; 2] {
    let verb = match on {
        true => "enable-linger",
        false => "disable-linger",
    };

    [verb.to_owned(), uid.to_string()]
}

/// The uid this process runs as.
#[expect(
    unsafe_code,
    reason = "getuid is the POSIX way to ask, takes no arguments, touches no memory and cannot \
              fail; there is no safe binding for it in std"
)]
fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yes_and_no_are_on_and_off() {
        assert_eq!(reading(true, "yes\n", ""), BootStart::On);
        assert_eq!(reading(true, "no\n", ""), BootStart::Off);
    }

    /// The daemon outlived the SSH session that started it, and the account does not linger.
    #[test]
    fn an_account_logind_does_not_know_is_one_that_does_not_linger() {
        assert_eq!(
            reading(
                false,
                "",
                "Failed to get user: User ID 1001 is not logged in or lingering\n"
            ),
            BootStart::Off
        );
    }

    #[test]
    fn anything_else_is_no_boot_half_rather_than_a_failed_status() {
        assert_eq!(reading(true, "maybe\n", ""), BootStart::Unsupported);
        assert_eq!(
            reading(
                false,
                "",
                "Failed to connect to bus: No such file or directory\n"
            ),
            BootStart::Unsupported
        );
    }

    #[test]
    fn the_command_names_the_uid_and_never_leaves_it_out() {
        assert_eq!(arguments(true, 1001), ["enable-linger", "1001"]);
        assert_eq!(arguments(false, 1001), ["disable-linger", "1001"]);
    }

    /// The whole cycle against the real logind, as this account.
    ///
    /// `#[ignore]` **and** `MIXENGINE_SYSTEM_TESTS`, per `docs/standards/testing.md` rule 1: this
    /// changes when the account of whoever runs it gets a user manager. Run it from a transient user
    /// unit as well as from a shell — `systemd-run --user --wait` puts it in no session, which is
    /// the case naming the uid exists for.
    ///
    /// **It never turns off a linger it did not turn on**: an account that already lingers is
    /// skipped, and says so.
    #[test]
    #[ignore = "turns this account's systemd linger on and off"]
    fn this_account_lingers_and_stops_lingering() {
        if std::env::var_os("MIXENGINE_SYSTEM_TESTS").is_none() {
            eprintln!("skipped: MIXENGINE_SYSTEM_TESTS is not set");
            return;
        }

        match state() {
            BootStart::Unsupported => {
                eprintln!("skipped: this machine has no logind to ask");
                return;
            }
            BootStart::On => {
                eprintln!(
                    "skipped: this account already lingers, and this suite never turns that off"
                );
                return;
            }
            BootStart::Off => {}
        }

        assert_eq!(
            set(true).expect("enable-linger as the account"),
            BootStart::On
        );
        assert_eq!(
            set(false).expect("disable-linger as the account"),
            BootStart::Off
        );
    }
}
