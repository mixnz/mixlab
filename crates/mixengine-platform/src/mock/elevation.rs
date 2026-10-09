//! A prompt nobody sees.
//!
//! The surface T40b's tests run against, and the reason `Elevation` is a capability on a `Host`
//! rather than a free function like `lock` or `signal`: a queue whose whole job is to batch pending
//! operations behind **one** prompt has to be testable without one.
//!
//! **Unlike the three real launchers, nothing here refuses a path.** A test asserting what a queue
//! raised should not also have to install a helper binary; the refusals are the launchers' own and
//! are asserted against the real host in `tests/elevation.rs`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mixengine_proto::privileged::ElevationOutcome;

use crate::{Elevation, ElevationSupport, Raised, Result};

/// One attempt to raise a prompt, as the mock recorded it.
///
/// Both paths, because the pair is the assertion worth making: one prompt, on the request that was
/// just written, with the helper the daemon was configured with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The helper that would have been run.
    pub helper: PathBuf,

    /// The request that would have been its only argument.
    pub request: PathBuf,

    /// What that request held when the prompt was raised — the daemon removes the file afterwards,
    /// so this is the only way a test can see which operations a batch carried (T182b, D6).
    pub body: String,
}

#[derive(Debug)]
pub(super) struct Prompts {
    raised: Mutex<Vec<Prompt>>,

    /// What every attempt answers with. One scripted outcome rather than a queue of them: a caller
    /// that behaves differently on the second prompt of a batch is a caller that has broken the rule
    /// the batch exists for.
    answer: ElevationOutcome,

    /// What the helper "said" on stderr, handed back with a `Completed` answer.
    said: Option<String>,

    /// Whether the "helper" leaves a report beside the request, applying every operation in it.
    /// Without one, `Completed` is a helper that ran and wrote nothing — the default, and T40a's
    /// reminder that a crash is not a per-OS event.
    reports: bool,
}

impl Default for Prompts {
    fn default() -> Self {
        Self {
            raised: Mutex::new(Vec::new()),
            answer: ElevationOutcome::Completed,
            said: None,
            reports: false,
        }
    }
}

impl Prompts {
    /// A machine where every prompt is accepted.
    pub(super) fn accepting() -> Self {
        Self::default()
    }

    /// A machine where the person at it says no.
    pub(super) fn declining() -> Self {
        Self {
            answer: ElevationOutcome::Declined,
            ..Self::default()
        }
    }

    /// A machine with no way to raise a prompt at all, with `reason`.
    ///
    /// `&str` and not the `&'static str` its neighbours in this directory take: the wire type owns
    /// its reason, so there is nothing for a lifetime to buy here.
    pub(super) fn refusing(reason: &str) -> Self {
        Self {
            answer: ElevationOutcome::Unavailable {
                reason: reason.to_owned(),
            },
            ..Self::default()
        }
    }

    /// A prompt that is accepted, and a helper that applies everything it was asked to and says so
    /// in a report — the one path on which a grant takes rows out of the queue.
    pub(super) fn applying() -> Self {
        Self {
            reports: true,
            ..Self::default()
        }
    }

    /// A prompt that is accepted, and a helper that wrote `said` to stderr — the helper that refused
    /// its request, when nothing is written beside it (T166).
    pub(super) fn saying(said: &str) -> Self {
        Self {
            said: Some(said.to_owned()),
            ..Self::default()
        }
    }

    /// Every prompt this host was asked to raise, in order.
    pub(super) fn raised(&self) -> Vec<Prompt> {
        self.raised
            .lock()
            .expect("no test panics while holding this")
            .clone()
    }
}

impl Elevation for Prompts {
    fn probe(&self) -> ElevationSupport {
        match &self.answer {
            ElevationOutcome::Unavailable { reason } => ElevationSupport::Unavailable {
                reason: reason.clone(),
            },
            // A machine where the person declines can still raise a prompt, which is the whole
            // distinction `probe` exists to draw — and the one a caller gets wrong by reading
            // "declined" as "impossible".
            ElevationOutcome::Completed | ElevationOutcome::Declined => ElevationSupport::Available,
        }
    }

    fn run(&self, helper: &Path, request: &Path) -> Result<Raised> {
        if self.reports {
            report_everything_applied(request);
        }

        self.raised
            .lock()
            .expect("no test panics while holding this")
            .push(Prompt {
                helper: helper.to_path_buf(),
                request: request.to_path_buf(),
                body: std::fs::read_to_string(request).unwrap_or_default(),
            });

        Ok(Raised {
            outcome: self.answer.clone(),
            said: self.said.clone(),
        })
    }
}

/// Write the report a helper that applied every operation in `request` would leave beside it.
///
/// Panics rather than returning an error: a test that asked for a report and got none would fail
/// later, on an assertion about the queue, a long way from the reason.
fn report_everything_applied(request: &Path) {
    use mixengine_proto::privileged::{
        OpOutcome, PrivilegedOp, PrivilegedRequest, PrivilegedResponse, RESPONSE_FILE_NAME,
    };

    let text = std::fs::read_to_string(request).expect("the request the daemon just wrote");
    let asked: PrivilegedRequest = serde_json::from_str(&text).expect("a request this build reads");

    let response = PrivilegedResponse {
        version: asked.version,
        elevate_version: "0.0.0-mock".to_owned(),
        nonce: asked.nonce,
        elevated: true,
        supported_ops: PrivilegedOp::ALL
            .iter()
            .map(|&name| name.to_owned())
            .collect(),
        audit_log: PathBuf::from("audit.log"),
        results: asked
            .ops
            .iter()
            .map(|_| OpOutcome::Applied {
                detail: "applied by the mock".to_owned(),
            })
            .collect(),
    };

    let beside = request
        .parent()
        .expect("a request lives in its own directory")
        .join(RESPONSE_FILE_NAME);
    std::fs::write(
        beside,
        serde_json::to_vec(&response).expect("a response encodes"),
    )
    .expect("the report is written");
}
