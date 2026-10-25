//! Raising the administrator prompt on the one-shot helper.
//!
//! `do shell script … with administrator privileges`, which is Apple's remaining supported way to
//! run one program elevated from a program that is not: `AuthorizationExecuteWithPrivileges` has
//! been deprecated since 10.7, and `SMJobBless` installs a **persistent** privileged helper, which
//! ADR 0005 rules out.
//!
//! **The script is a compile-time constant and the two paths arrive as `argv`** — see
//! `crate::prompt::macos::SCRIPT`. Interpolating a path into an AppleScript string literal would be
//! the single most dangerous line in the elevation path on any of the three systems, and `quoted
//! form of` is AppleScript's own answer to it.

use std::path::Path;
use std::process::Command;

use mixengine_proto::privileged::ElevationOutcome;

use crate::prompt::{self, macos as decide};
use crate::{Elevation, ElevationSupport, Raised, Result};

/// Part of the base system since Mac OS X 10.0, and named absolutely so that the daemon's own `PATH`
/// cannot decide what runs.
const OSASCRIPT: &str = "/usr/bin/osascript";

#[derive(Debug)]
pub(crate) struct Prompt;

/// `launchctl`, by its absolute path for [`OSASCRIPT`]'s reason.
const LAUNCHCTL: &str = "/bin/launchctl";

/// Why this process's session shows no administrator prompt, read once; `None` when it does, or
/// when `launchctl` will not say, which is answered as before this was asked.
fn session_outside_the_screen() -> Option<String> {
    static ANSWER: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

    ANSWER
        .get_or_init(|| {
            let output = Command::new(LAUNCHCTL).arg("managername").output().ok()?;
            if !output.status.success() {
                return None;
            }

            decide::outside_the_screen(&String::from_utf8_lossy(&output.stdout))
        })
        .clone()
}

impl Elevation for Prompt {
    fn probe(&self) -> ElevationSupport {
        if !Path::new(OSASCRIPT).is_file() {
            return ElevationSupport::Unavailable {
                reason: format!(
                    "{OSASCRIPT} is not on this machine, so no administrator prompt can be raised"
                ),
            };
        }

        // Which login session this process is in decides whether macOS shows its prompt at all,
        // and it cannot change under a running process, so it is asked once.
        match session_outside_the_screen() {
            Some(reason) => ElevationSupport::Unavailable { reason },
            None => ElevationSupport::Available,
        }
    }

    fn needs_the_screen(&self, op: &mixengine_proto::privileged::PrivilegedOp) -> Option<String> {
        decide::needs_the_screen(op)
    }

    fn run(&self, helper: &Path, request: &Path) -> Result<Raised> {
        prompt::usable("run as the elevation helper", helper)?;
        prompt::usable("hand to the elevation helper", request)?;

        let mut osascript = Command::new(OSASCRIPT);
        for line in decide::SCRIPT {
            osascript.arg("-e").arg(line);
        }

        // Both are absolute, so neither can be mistaken for an option by osascript's own argument
        // parsing, and both are quoted by AppleScript rather than by us.
        osascript.arg(helper).arg(request);

        let ran = match osascript.output() {
            Ok(output) => output,
            Err(source) => {
                return Ok(Raised::from(ElevationOutcome::Unavailable {
                    reason: format!("{OSASCRIPT} could not be started ({source})"),
                }));
            }
        };

        let complaint = String::from_utf8_lossy(&ran.stderr);
        tracing::debug!(
            code = ?ran.status.code(),
            stderr = %complaint.trim(),
            "osascript ended"
        );

        // The helper's words only when the helper ran: on the other two answers the stream is
        // osascript's, and `Unavailable` already carries it as its reason.
        let outcome = decide::outcome(ran.status.code(), &complaint);
        let said = match outcome {
            ElevationOutcome::Completed => decide::said(&complaint),
            _ => None,
        };

        Ok(Raised { outcome, said })
    }
}
