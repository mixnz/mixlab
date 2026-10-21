//! A grant raised from a terminal: the request is written, and this daemon waits for the helper's
//! report to appear beside it — ADR 0065.
//!
//! **Nothing is spawned here.** The daemon has no terminal for `sudo` to ask on; `mix` has, and runs
//! the installed helper itself. What this side owns is the wait, and the three ways it ends: a
//! report, a cancel, or nobody running the command before [`BY_HAND_WAIT`](super::BY_HAND_WAIT).
//! The last two are a decline, which ADR 0005 already calls a normal outcome: every row stays.

use std::path::Path;
use std::time::Duration;

use mixengine_platform::Raised;
use mixengine_proto::ByHand;
use mixengine_proto::privileged::{ElevationOutcome, PrivilegedResponse, RESPONSE_FILE_NAME};
use tokio::time::Instant;

use super::Elevation;

/// How often the request directory is looked at.
const POLL: Duration = Duration::from_millis(250);

/// How long a report that is there but does not parse is given to finish being written.
///
/// The helper creates the file and then writes it, so a look in between sees it empty. After this
/// long it is judged as it stands, and `read_report` says what is wrong with it.
const SETTLING: Duration = Duration::from_secs(2);

impl Elevation {
    /// Publish the waiting command, wait for the report, and take the command down again.
    pub(super) async fn by_hand(
        &self,
        handle: &crate::jobs::JobHandle,
        helper: &Path,
        request: &mixengine_core::elevation::Request,
    ) -> Raised {
        self.set_waiting(Some(ByHand {
            job: handle.id(),
            helper: helper.display().to_string(),
            request: request.path().display().to_string(),
        }));
        // Taken down however this ends, `Released`'s reasoning: a status still naming a command
        // nobody is waiting for would send a person to run it.
        let _published = Unpublished(self);

        handle
            .progress(40, "waiting for the command to be run in a terminal")
            .await;

        let outcome = wait_for_report(
            &request.directory().join(RESPONSE_FILE_NAME),
            handle,
            self.by_hand_wait,
        )
        .await;

        if outcome == ElevationOutcome::Declined {
            tracing::info!("nobody ran the helper from a terminal; nothing was applied");
        }

        Raised::from(outcome)
    }

    fn set_waiting(&self, waiting: Option<ByHand>) {
        self.state
            .lock()
            .expect("the elevation slot is not held across an await")
            .waiting = waiting;
    }
}

/// Clears the waiting command when the wait ends, including by a panic or a shutdown.
struct Unpublished<'a>(&'a Elevation);

impl Drop for Unpublished<'_> {
    fn drop(&mut self) {
        self.0.set_waiting(None);
    }
}

/// `Completed` once a report is there and whole, `Declined` on a cancel or the deadline.
///
/// `Completed` means what it means after a dialog: the helper ran, and `judge` reads what it said.
async fn wait_for_report(
    report: &Path,
    handle: &crate::jobs::JobHandle,
    deadline: Duration,
) -> ElevationOutcome {
    let gives_up = Instant::now() + deadline;
    let mut seen: Option<Instant> = None;

    loop {
        if let Ok(bytes) = tokio::fs::read(report).await {
            let whole = serde_json::from_slice::<PrivilegedResponse>(&bytes).is_ok();
            let first = *seen.get_or_insert_with(Instant::now);

            if whole || first.elapsed() >= SETTLING {
                return ElevationOutcome::Completed;
            }
        }

        if Instant::now() >= gives_up {
            return ElevationOutcome::Declined;
        }

        tokio::select! {
            () = handle.cancelled() => return ElevationOutcome::Declined,
            () = tokio::time::sleep(POLL) => {}
        }
    }
}
