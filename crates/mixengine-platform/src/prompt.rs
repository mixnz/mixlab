//! The half of the three launchers that is a decision rather than a call.
//!
//! `windows/prompt.rs`, `macos/prompt.rs` and `linux/prompt.rs` are mapped onto `sys` by `#[path]`,
//! so each is compiled on its own system alone — and so is any test beside it. The part of a
//! launcher most likely to be wrong is its table: which exit code means the person said no, which
//! means there was nobody to ask, how an argument is quoted. Those live here, as pure functions over
//! a code and a string, in a module compiled on all three systems, so that each system's table is
//! tested on every one of them. What stays in `sys::prompt` is the call that can be made nowhere
//! else. See the T40a design, "Testing".

#![allow(
    dead_code,
    reason = "each table here is compiled on all three systems and used on one, which is the \
              module's whole purpose: on Windows the `macos` and `linux` tables are read by the \
              tests below and by nothing else, and the same holds on the other two"
)]

use std::ffi::OsString;
use std::io;
use std::path::Path;

use mixengine_proto::privileged::ElevationOutcome;

use crate::{Error, Result};

/// Refuse a path this crate is not willing to hand to a mechanism that will run it as root.
///
/// Absolute, and an existing file. Checked in one place rather than in each launcher: three
/// implementations of one check is two chances to forget it, and what is being checked is about to
/// become a root process's command line.
///
/// # Errors
///
/// [`Error::Io`] naming the path, with `action` completing "cannot …".
pub(crate) fn usable(action: &'static str, path: &Path) -> Result<()> {
    let complaint = if path.is_absolute() {
        if path.is_file() {
            None
        } else {
            Some(io::Error::new(
                io::ErrorKind::NotFound,
                "there is no file there",
            ))
        }
    } else {
        Some(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is not absolute",
        ))
    };

    complaint.map_or(Ok(()), |source| {
        Err(Error::Io {
            action,
            path: path.to_path_buf(),
            source,
        })
    })
}

/// Windows' table, and the one argument its mechanism has to be given as a string.
pub(crate) mod windows {
    use super::{Error, OsString, Path, Result, io};

    /// `ERROR_CANCELLED`, which is what `ShellExecuteExW` reports when the person dismisses UAC.
    ///
    /// Spelled out rather than imported, so the table can be read and tested on a machine that is
    /// not Windows. `windows/prompt.rs` asserts it against `windows_sys`' own constant, which is
    /// what stops the two from drifting.
    pub(crate) const ERROR_CANCELLED: u32 = 1223;

    /// The single string `ShellExecuteExW` takes as the child's parameters: the request path, quoted.
    ///
    /// **The quoting is provable here rather than conventional.** `"` is not a legal character in a
    /// Windows path name, so no path can close the quotation early — and a path carrying one anyway
    /// is refused, which is what turns that from a belief into a check. See the T40a design, D4.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the path contains a quotation mark.
    pub(crate) fn parameters(request: &Path) -> Result<OsString> {
        if request.as_os_str().to_string_lossy().contains('"') {
            return Err(Error::Io {
                action: "quote for the elevation prompt",
                path: request.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a quotation mark is not a legal character in a Windows path name",
                ),
            });
        }

        let mut quoted = OsString::from("\"");
        quoted.push(request.as_os_str());
        quoted.push("\"");

        Ok(quoted)
    }
}

/// The most of what a helper said that reaches an error message, in bytes.
///
/// A refusal is one line; the bound is for a helper that panicked and printed a backtrace, where the
/// reason is at the end and a page of frames above it is not a sentence anybody reads — T166, D5.
pub(crate) const SAID_LIMIT: usize = 1024;

/// A helper's stderr as it goes into an error: trimmed, `None` when there is nothing, and the
/// **last** [`SAID_LIMIT`] bytes when there is too much, cut on a character boundary.
pub(crate) fn trimmed(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let mut start = text.len().saturating_sub(SAID_LIMIT);
    while !text.is_char_boundary(start) {
        start += 1;
    }

    Some(text[start..].trim_start().to_owned())
}

/// macOS' table, and the script that is a constant.
pub(crate) mod macos {
    use super::ElevationOutcome;

    /// The AppleScript, as the three `-e` lines it is given in.
    ///
    /// **A compile-time constant with no value of ours anywhere in it.** The two paths arrive as
    /// `argv` and are quoted by AppleScript's own `quoted form of`, which is the one operator on
    /// that system whose entire job is being right about spaces, quotes and newlines in a shell
    /// word. Interpolating a path into this string instead is the line T40/D9 singles out as the
    /// single most dangerous one in the elevation path on any of the three systems — T40a, D3.
    pub(crate) const SCRIPT: [&str; 3] = [
        "on run argv",
        "do shell script (quoted form of (item 1 of argv)) & \" \" \
         & (quoted form of (item 2 of argv)) with administrator privileges",
        "end run",
    ];

    /// The AppleScript error code trailing `stderr`, when osascript left one.
    ///
    /// `execution error: User canceled. (-128)` — the number in the last parentheses is the whole of
    /// what this system hands back. `do shell script` raises an error rather than returning a
    /// status, so the helper's own exit code arrives here too, as a positive number.
    pub(crate) fn error_code(stderr: &str) -> Option<i32> {
        let tail = stderr.trim_end().strip_suffix(')')?;
        let open = tail.rfind('(')?;

        tail[open + 1..].parse().ok()
    }

    /// The helper's own words, out of osascript's account of them.
    ///
    /// `do shell script` reports a non-zero status as an AppleScript error, which wraps whatever the
    /// helper wrote to stderr as `<n>:<n>: execution error: <stderr> (<status>)`. The framing is
    /// AppleScript's and says nothing to a person; what is inside it is the helper's sentence. Only
    /// the **last** parenthesised integer goes — `(os error 1)` inside the sentence is the helper's.
    pub(crate) fn said(stderr: &str) -> Option<String> {
        let text = stderr.trim();
        let text = text
            .split_once(": execution error: ")
            .map_or(text, |(_, rest)| rest);

        let text = match error_code(text) {
            Some(_) => text
                .strip_suffix(')')
                .and_then(|tail| tail.rfind('(').map(|open| &tail[..open]))
                .unwrap_or(text),
            None => text,
        };

        super::trimmed(text)
    }

    /// What osascript's answer means, in the three words the trait speaks.
    ///
    /// `-128` is `userCanceledErr`, and it is why T40/D11 said a declined prompt cannot be an exit
    /// code of the helper's. A **positive** code is the helper's own status, reported through an
    /// AppleScript error because that is how `do shell script` reports one — the helper ran, so the
    /// answer is `Completed` and whatever it thought is beside the request. Any other negative code
    /// is AppleScript refusing to do it at all; so is a run that left no code to read, because there
    /// is then nothing to say it ever started.
    pub(crate) fn outcome(code: Option<i32>, stderr: &str) -> ElevationOutcome {
        if code == Some(0) {
            return ElevationOutcome::Completed;
        }

        match error_code(stderr) {
            Some(-128) => ElevationOutcome::Declined,
            // What osascript says from a session with no screen, worded as a wrong password.
            Some(-60007) => ElevationOutcome::Unavailable {
                reason: OUTSIDE_THE_SCREEN.to_owned(),
            },
            Some(status) if status > 0 => ElevationOutcome::Completed,
            _ => ElevationOutcome::Unavailable {
                reason: format!(
                    "macOS would not raise an administrator prompt: {}",
                    stderr.trim()
                ),
            },
        }
    }
    /// What a daemon outside the screen's login session is told about its prompts.
    const OUTSIDE_THE_SCREEN: &str = "this daemon was started over SSH, outside this Mac's screen, \
         and macOS shows no administrator prompt from there. Run `mix daemon stop` at the Mac, \
         then any `mix` command there starts it again";

    /// Why no prompt can be raised from a process in the session `launchctl managername` names,
    /// or `None` for `Aqua`, the session that owns the screen.
    ///
    /// `mix` over SSH starts the daemon by forking, so it lives in SSH's `Background` session, and
    /// osascript from there fails with -60007 without showing anything.
    pub(crate) fn outside_the_screen(manager: &str) -> Option<String> {
        (manager.trim() != "Aqua").then(|| OUTSIDE_THE_SCREEN.to_owned())
    }

    /// Why `op` cannot be granted from a terminal on macOS, or `None` when it can.
    ///
    /// `add-trusted-cert -d` writes the admin trust domain, and macOS asks for it in a SecurityAgent
    /// dialog on the Mac's own screen even when root runs it — see `macos::trust::apply`. Over SSH
    /// nobody at the terminal can answer that, so the row waits for a grant raised at the screen.
    pub(crate) fn needs_the_screen(
        op: &mixengine_proto::privileged::PrivilegedOp,
    ) -> Option<String> {
        matches!(
            op,
            mixengine_proto::privileged::PrivilegedOp::TrustCaInstall { .. }
        )
        .then(|| {
            "adding MixEngine's certificate authority to the System keychain needs this Mac's own \
             screen, where macOS asks for it"
                .to_owned()
        })
    }
}

/// Linux's table, and the environment question the other two systems do not have.
pub(crate) mod linux {
    use super::ElevationOutcome;

    /// What `pkexec` needs on this machine, as far as the environment will say.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct Session {
        /// Is `pkexec` on `PATH`?
        pub(crate) pkexec: bool,

        /// Is there a session bus for polkit to reach an authentication agent over?
        pub(crate) bus: bool,

        /// Is there a graphical session for an agent to draw a prompt in?
        pub(crate) graphical: bool,
    }

    /// What a person reads after any Linux reason: the way that works where a dialog cannot.
    ///
    /// Not a `pkexec` command: an SSH session has no agent for `pkexec` to ask through either, and
    /// a request exists only while a grant waits for it. `mix` in a terminal raises the prompt
    /// through `sudo` itself (ADR 0065).
    pub(crate) const FROM_A_TERMINAL: &str =
        "From a terminal on this machine, run: mix elevation grant";

    /// What is missing, or `None` when a prompt can be raised.
    pub(crate) fn missing(session: Session) -> Option<String> {
        let absent = if !session.pkexec {
            // Since Ubuntu 24.04 `pkexec` is a package of its own, and `polkitd` runs without it.
            "pkexec is not installed (on Ubuntu 24.04 and later it is a package of its own)"
        } else if !session.bus {
            "there is no session bus ($DBUS_SESSION_BUS_ADDRESS is unset and $XDG_RUNTIME_DIR has \
             no bus socket), so polkit cannot reach an authentication agent"
        } else if !session.graphical {
            "there is no graphical session ($DISPLAY and $WAYLAND_DISPLAY are both unset), so no \
             authentication agent can show a password prompt"
        } else {
            return None;
        };

        Some(format!("{absent}. {FROM_A_TERMINAL}"))
    }

    /// What `pkexec`'s exit code means, in the three words the trait speaks.
    ///
    /// `126` is a dismissed dialog and `127` is "not authorised, or something went wrong" — the two
    /// numbers T40/D2 kept every exit code of the helper's below 125 to stay clear of, and this is
    /// the task that spends them. Anything else is the helper's own status: it ran, and what it
    /// thought is beside the request. A run with no code at all was ended by a signal, which is not
    /// a report that it started.
    pub(crate) fn outcome(code: Option<i32>) -> ElevationOutcome {
        match code {
            Some(126) => ElevationOutcome::Declined,
            Some(127) => ElevationOutcome::Unavailable {
                reason:
                    "pkexec would not authorise this; either no authentication agent answered, \
                         or the request was refused"
                        .to_owned(),
            },
            Some(_) => ElevationOutcome::Completed,
            None => ElevationOutcome::Unavailable {
                reason: "pkexec was killed before it could say anything".to_owned(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path that is absolute and is a file, on whichever system this is.
    fn a_real_file() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::TempDir::new().expect("the temporary directory is writable");
        let path = directory.path().join("mixengine-elevate");
        std::fs::write(&path, b"not really a binary").expect("a file in a temporary directory");

        (directory, path)
    }

    #[test]
    fn an_absolute_existing_file_is_usable() {
        let (_directory, path) = a_real_file();

        assert!(usable("run as the elevation helper", &path).is_ok());
    }

    #[test]
    fn a_relative_path_is_refused_before_anything_is_run() {
        let error = usable(
            "run as the elevation helper",
            Path::new("mixengine-elevate"),
        )
        .expect_err("a relative path is not run as root");

        assert!(
            error.to_string().contains("run as the elevation helper"),
            "{error}"
        );
    }

    #[test]
    fn a_path_with_no_file_at_it_is_refused() {
        let (directory, _path) = a_real_file();

        assert!(
            usable(
                "run as the elevation helper",
                &directory.path().join("absent")
            )
            .is_err()
        );
    }

    #[test]
    fn windows_quotes_the_request_path() {
        let quoted = windows::parameters(Path::new(r"C:\Program Files\MixEngine\request.json"))
            .expect("a path with a space is exactly what the quoting is for");

        assert_eq!(
            quoted,
            OsString::from("\"C:\\Program Files\\MixEngine\\request.json\"")
        );
    }

    /// D4: the guarantee is enforced rather than assumed. `"` cannot occur in a Windows path name,
    /// and a path carrying one anyway never reaches a command line.
    #[test]
    fn windows_refuses_a_path_carrying_a_quotation_mark() {
        let error = windows::parameters(Path::new("C:\\a\"b\\request.json"))
            .expect_err("a quotation mark would end the quoting early");

        assert!(
            error.to_string().contains("quote for the elevation prompt"),
            "{error}"
        );
    }

    #[test]
    fn macos_reads_the_code_out_of_an_applescript_error() {
        assert_eq!(
            macos::error_code("execution error: User canceled. (-128)"),
            Some(-128)
        );
        assert_eq!(
            macos::error_code("execution error: something went wrong. (65)\n"),
            Some(65)
        );
        assert_eq!(macos::error_code("no code here at all"), None);
    }

    #[test]
    fn macos_keeps_the_helpers_sentence_and_drops_applescripts_framing() {
        // The stderr of the incident T166 comes from, verbatim but for the path.
        assert_eq!(
            macos::said(
                "7:134: execution error: mixengine-elevate: cannot read /Volumes/SSD/home/run/\
                 elevate/x/request.json: Operation not permitted (os error 1) (65)\n"
            )
            .as_deref(),
            Some(
                "mixengine-elevate: cannot read /Volumes/SSD/home/run/elevate/x/request.json: \
                 Operation not permitted (os error 1)"
            ),
        );
        assert_eq!(
            macos::said("mixengine-elevate: no framing at all").as_deref(),
            Some("mixengine-elevate: no framing at all"),
        );
        assert_eq!(macos::said("  \n"), None);
    }

    #[test]
    fn a_long_complaint_keeps_its_end() {
        let long = format!("{}the reason", "é".repeat(SAID_LIMIT));
        let kept = trimmed(&long).expect("something to keep");

        assert!(kept.len() <= SAID_LIMIT, "{}", kept.len());
        assert!(kept.ends_with("the reason"), "{kept}");
        assert_eq!(trimmed("  a line \n").as_deref(), Some("a line"));
        assert_eq!(trimmed(""), None);
    }

    #[test]
    fn macos_maps_its_three_cases() {
        assert_eq!(macos::outcome(Some(0), ""), ElevationOutcome::Completed);
        assert_eq!(
            macos::outcome(Some(1), "execution error: User canceled. (-128)"),
            ElevationOutcome::Declined
        );
        assert_eq!(
            macos::outcome(Some(1), "execution error: refused. (65)"),
            ElevationOutcome::Completed,
            "a positive code is the helper's own status, so the helper ran"
        );
        assert!(matches!(
            macos::outcome(Some(1), "execution error: Not authorized. (-1743)"),
            ElevationOutcome::Unavailable { .. }
        ));
        assert!(matches!(
            macos::outcome(None, ""),
            ElevationOutcome::Unavailable { .. }
        ));
    }

    /// D5, all four states of the probe.
    /// A daemon started over SSH lives in a session macOS will not show an administrator prompt
    /// from; the one at the screen is `Aqua`.
    #[test]
    fn macos_says_a_daemon_outside_the_screens_session_cannot_prompt() {
        assert_eq!(macos::outside_the_screen("Aqua"), None);

        let reason = macos::outside_the_screen("Background").expect("not the screen's session");
        assert!(reason.contains("over SSH"), "{reason}");
        assert!(reason.contains("mix daemon stop"), "{reason}");
    }

    /// `-60007` is what osascript says from such a session, as if the password had been wrong.
    #[test]
    fn macos_does_not_call_a_session_without_a_screen_a_wrong_password() {
        let answer = macos::outcome(
            Some(1),
            "7:134: execution error: The administrator user name or password was incorrect. \
             (-60007)",
        );

        let ElevationOutcome::Unavailable { reason } = answer else {
            panic!("{answer:?}");
        };
        assert!(reason.contains("over SSH"), "{reason}");
    }

    /// `add-trusted-cert -d` raises SecurityAgent on the Mac's own screen even under `sudo`,
    /// so a terminal cannot grant it. Nothing else the helper does on macOS needs the screen.
    #[test]
    fn macos_says_only_the_trust_install_needs_the_screen() {
        let install = mixengine_proto::privileged::PrivilegedOp::TrustCaInstall {
            plan: mixengine_proto::privileged::TrustPlan::SystemKeychain { der: vec![1, 2, 3] },
        };

        let reason = macos::needs_the_screen(&install).expect("the System keychain asks on screen");
        assert!(reason.contains("screen"), "{reason}");
        assert_eq!(
            macos::needs_the_screen(&mixengine_proto::privileged::PrivilegedOp::Probe {}),
            None
        );
    }

    #[test]
    fn linux_names_whichever_piece_of_polkit_is_missing() {
        let complete = linux::Session {
            pkexec: true,
            bus: true,
            graphical: true,
        };

        assert_eq!(linux::missing(complete), None);

        let reason = linux::missing(linux::Session {
            pkexec: false,
            ..complete
        })
        .expect("no pkexec, no prompt");
        assert!(reason.contains("pkexec"), "{reason}");

        let reason = linux::missing(linux::Session {
            bus: false,
            ..complete
        })
        .expect("no bus, no agent");
        assert!(reason.contains("session bus"), "{reason}");

        let reason = linux::missing(linux::Session {
            graphical: false,
            ..complete
        })
        .expect("no graphical session, nowhere to draw a prompt");
        assert!(reason.contains("graphical session"), "{reason}");
    }

    /// Ubuntu 24.04 ships `pkexec` as a package of its own, so its absence says nothing about polkit.
    #[test]
    fn linux_without_pkexec_does_not_say_polkit_is_missing() {
        let reason = linux::missing(linux::Session {
            pkexec: false,
            bus: true,
            graphical: true,
        })
        .expect("no pkexec, no prompt");

        assert!(reason.contains("pkexec is not installed"), "{reason}");
        assert!(!reason.contains("polkit is not installed"), "{reason}");
        assert!(reason.contains("mix elevation grant"), "{reason}");
    }

    /// The fallback ADR 0065 gives: a machine that cannot prompt says what works instead.
    #[test]
    fn linux_points_at_the_terminal_route_whatever_is_missing() {
        let reason = linux::missing(linux::Session {
            pkexec: true,
            bus: true,
            graphical: false,
        })
        .expect("no graphical session");

        assert!(reason.ends_with(linux::FROM_A_TERMINAL), "{reason}");
    }

    #[test]
    fn linux_maps_the_two_numbers_pkexec_reserves() {
        assert_eq!(linux::outcome(Some(126)), ElevationOutcome::Declined);
        assert!(matches!(
            linux::outcome(Some(127)),
            ElevationOutcome::Unavailable { .. }
        ));
        assert_eq!(linux::outcome(Some(0)), ElevationOutcome::Completed);
        assert_eq!(
            linux::outcome(Some(65)),
            ElevationOutcome::Completed,
            "65 is the helper's own refusal code, and the helper having run is the answer here"
        );
        assert!(matches!(
            linux::outcome(None),
            ElevationOutcome::Unavailable { .. }
        ));
    }
}
