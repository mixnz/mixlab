//! The ways asking the privileged helper can fail — what `elevation` reports.
//!
//! One of the enums [`crate::Error`] wraps transparently, so its messages and sources are
//! the ones a caller of this crate always saw.

use std::path::PathBuf;

/// The ways asking the privileged helper can fail
#[derive(Debug, thiserror::Error)]
pub enum ElevationError {
    /// `mixengine-elevate` is neither installed nor beside the program that went looking for it.
    ///
    /// [`RuntimeError::ShimMissing`]'s sibling, but **not the same guarantee**: the shim always
    /// ships beside `mixengined`, while the elevate helper only does on some releases (the NSIS
    /// installer, the portable archives) — a `.pkg`, a `.deb` or an `.rpm` writes it straight to
    /// this system's protected directory instead, so once that installed copy is gone (a `mix
    /// uninstall`, say) there is nothing beside the program to fall back to either. It is **not** a
    /// reason to refuse to start — a daemon with no helper anywhere supervises every service in
    /// this home perfectly well — and it is answered at `elevation.grant`, where somebody can act
    /// on it.
    ///
    /// [`RuntimeError::ShimMissing`]: crate::RuntimeError::ShimMissing
    #[error("the elevation helper is missing from {}", path.display())]
    ElevateMissing {
        /// Where it was expected to be.
        path: PathBuf,
    },

    /// `mixengine-elevate` is installed where it belongs, and that file is not an administrator's.
    ///
    /// **The one state where this refuses rather than falling back.** A copy beside the program is
    /// used when nothing is installed, because that is a development tree and a machine before its
    /// first prompt. A copy that *is* installed and can be written by an ordinary account is not
    /// the same thing at all: it is precisely the arrangement the root-owned directory exists to
    /// prevent, and running it anyway would be doing as root exactly what somebody set up.
    ///
    /// Not [`Error::ElevateMissing`]'s sibling in code either: that one is a broken installation and
    /// the fix is a reinstall, this one is a machine somebody has arranged and the fix is to find
    /// out who. Reported through `elevation.status`' `reason`, so it is on the screen before
    /// anybody clicks Allow.
    ///
    /// [`Error::ElevateMissing`]: crate::ElevationError::ElevateMissing
    #[error("the elevation helper at {} is not an administrator's: {why}", path.display())]
    ElevateUntrusted {
        /// The installed helper this is about.
        path: PathBuf,
        /// Which check it failed, phrased for a person.
        why: String,
    },

    /// A batch with no operations in it.
    ///
    /// The helper refuses one outright — no response file, exit 65 — because giving an empty request
    /// a meaning of its own would be a second way to ask for the report that arrives with every
    /// answer. Refused here so the message says what happened rather than what the exit code was.
    #[error("an elevation request has to carry at least one operation")]
    ElevateRequestEmpty,

    /// The helper ran and left nothing beside the request.
    ///
    /// **A state and not an impossibility.** `ElevationOutcome::Completed` means the helper ran, not
    /// that it wrote a report: a process that died before writing one is exactly this, on every
    /// system, because a crash is not a per-OS event. So is a helper that refused its request — it
    /// writes nothing beside one it does not trust — and then `said` is its reason.
    #[error("the elevation helper left no report beside {}{}", path.display(), said_suffix(said.as_deref()))]
    ElevateReportMissing {
        /// Where one would have been.
        path: PathBuf,
        /// What the helper wrote to stderr, when the OS let the daemon read it. Never on Windows.
        said: Option<String>,
    },

    /// The report is there and is not a document this build can read.
    ///
    /// The response is deliberately tolerant of fields it does not know — the helper is excluded
    /// from auto-update, so one newer than the daemon is routine — which makes this a file that is
    /// not the helper's answer at all.
    #[error("the elevation helper's report at {} cannot be read", path.display())]
    ElevateReportUnreadable {
        /// The report.
        path: PathBuf,
        /// How it failed to parse.
        #[source]
        source: serde_json::Error,
    },

    /// The report is readable and is not an answer to the request it sits beside.
    ///
    /// The nonce, the protocol, or the number of outcomes. Its own variant because none of the three
    /// is a parse failure and all three mean the same thing to a caller: nothing here can be applied
    /// to the queue.
    #[error("the elevation helper's report at {} does not answer this request: {why}", path.display())]
    ElevateReportMismatched {
        /// The report.
        path: PathBuf,
        /// Which of the three checks it failed.
        why: String,
    },

    /// A privileged operation could not be encoded.
    ///
    /// Unreachable: a [`mixengine_proto::privileged::PrivilegedOp`] is one of ours and holds nothing
    /// serde can refuse. Its own variant rather than an `expect`, on
    /// [`JobError::JobOutcomeUnwritable`]'s rule — nothing in this crate panics.
    ///
    /// [`JobError::JobOutcomeUnwritable`]: crate::JobError::JobOutcomeUnwritable
    #[error("a privileged operation could not be written down")]
    OpUnwritable {
        /// How it failed to encode.
        #[source]
        source: serde_json::Error,
    },
}

/// `": <said>"`, or nothing — the tail [`ElevationError::ElevateReportMissing`] gains when the
/// helper said why.
fn said_suffix(said: Option<&str>) -> String {
    said.map_or_else(String::new, |said| format!(": {said}"))
}
