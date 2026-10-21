//! Raising the OS elevation prompt on the one-shot helper.

use std::path::Path;

use mixengine_proto::privileged::ElevationOutcome;

use crate::Result;

/// Whether this machine can raise an elevation prompt at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevationSupport {
    /// Nothing this layer can see stands in the way.
    Available,

    /// There is no mechanism here, and this is why.
    Unavailable {
        /// What is missing, phrased for a user, with the manual command where one exists.
        reason: String,
    },
}

/// What became of one prompt, and what the helper said on its way out.
///
/// `said` is **a sentence for a person, never something to branch on** — T40a, D7 still holds: the
/// helper's exit code goes no further than a log line, because Windows cannot supply it. What a
/// helper that refused its request wrote to stderr is the only account of *why* there is no report
/// beside it (roadmap task T166, D5), and without it the daemon can say only that there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised {
    /// The prompt's answer — the part that crosses the wire.
    pub outcome: ElevationOutcome,

    /// What the helper wrote to stderr, trimmed and bounded, when this OS let us read it.
    ///
    /// Only ever set on [`ElevationOutcome::Completed`]: on the other two the stream belongs to the
    /// launcher, not to the helper. Always `None` on Windows, where a `runas` child has no stream the
    /// caller owns.
    pub said: Option<String>,
}

impl From<ElevationOutcome> for Raised {
    fn from(outcome: ElevationOutcome) -> Self {
        Self {
            outcome,
            said: None,
        }
    }
}

/// Running `mixengine-elevate` once, under an administrative token.
///
/// **The capability stops at the prompt.** It raises one, waits for the process to end, and answers
/// with one of the three words in [`ElevationOutcome`]. It does not open the response file: reading
/// that is `serde_json` over types `mixengine-proto` already describes, with no operating system
/// anywhere in it, and a mock that had to fabricate a whole `PrivilegedResponse` would be a mock of
/// something that is not the OS. T40b reads it. See the T40a design, D1.
///
/// **Nothing here takes a command or an argument list.** Both paths are the caller's, every
/// invocation is composed from them and from constants belonging to the implementation, and there is
/// no parameter through which caller-supplied text could reach a command line — which makes T40/D9's
/// rule a property of the type rather than something three implementations have to remember.
pub trait Elevation: std::fmt::Debug + Send + Sync {
    /// Can a prompt be raised here?
    ///
    /// Cheap, and spawns nothing: `mix doctor` (T47) and the daemon's degraded mode (T40b) both have
    /// to be able to say "this machine cannot elevate" without raising a prompt to find out.
    ///
    /// **Its honesty is not the same on the three systems, and pretending otherwise would be worse
    /// than the asymmetry.** Windows answers [`ElevationSupport::Available`] unconditionally,
    /// because UAC is part of the OS and an account without administrative rights is asked for
    /// somebody else's credentials rather than not asked at all. macOS answers it when `osascript`
    /// is there, which is nearly a constant. Linux is the system where the question has a real
    /// answer, and where it decides whether anything is spawned at all.
    ///
    /// So this is a cheap way to find out that elevation is impossible, **never a promise that it is
    /// possible**. The authoritative answer is always [`run`](Self::run)'s, and `run` returns
    /// [`ElevationOutcome::Unavailable`] too.
    fn probe(&self) -> ElevationSupport;

    /// Why `op` cannot be granted from a terminal on this system, or `None` when it can.
    ///
    /// A grant raised from a terminal leaves such rows in the queue, for a grant raised at the
    /// machine's own screen. Only macOS has one: its System keychain asks on screen even under
    /// `sudo`.
    fn needs_the_screen(&self, op: &mixengine_proto::privileged::PrivilegedOp) -> Option<String> {
        let _ = op;
        None
    }

    /// Run `helper` once, elevated, with `request` as its only argument.
    ///
    /// **Blocking, and with no deadline**: a person reading a prompt is not a clock the OS gives us.
    /// The caller owns cancellation — T40b runs this on `spawn_blocking`, the way a keyring read
    /// already is.
    ///
    /// [`ElevationOutcome::Completed`] means the helper **ran**, not that it left a report. A helper
    /// that died before writing one is `Completed` with nothing beside the request — a state every
    /// caller has to handle anyway, on every system, because a crash is not a per-OS event. So is a
    /// helper that refused its request, and [`Raised::said`] is where its reason arrives.
    ///
    /// # Errors
    ///
    /// **Not for an absent mechanism.** A machine with no way to prompt is a normal outcome the
    /// daemon degrades around, and it is [`ElevationOutcome::Unavailable`]. `Err` is reserved for a
    /// launcher that could not be *attempted*: [`Error::Io`](crate::Error::Io) for a path that is not
    /// an absolute existing file, or that carries a character the mechanism cannot be given, and
    /// [`Error::Os`](crate::Error::Os) for a system call that failed for a reason that is not the
    /// user.
    fn run(&self, helper: &Path, request: &Path) -> Result<Raised>;
}
