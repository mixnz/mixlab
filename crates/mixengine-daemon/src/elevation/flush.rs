//! Carrying the queue out: what a grant reserves, one flush through the helper, and what its answer
//! means for each operation.

use std::sync::Arc;
use std::time::SystemTime;

use mixengine_core::ElevationError;
use mixengine_proto::privileged::ElevationOutcome;
use mixengine_proto::{Error, ErrorCode, GrantOutcome, JobKind, JobSummary, Timestamp, rpc};

use crate::error::ToWire as _;

use super::{
    Afterwards, Chosen, Elevation, Released, Route, Slot, may_have_changed_the_helper,
    put_the_helper_first,
};

impl Elevation {
    /// `elevation.grant` — spend one prompt on everything that is waiting.
    ///
    /// **A job, and the exception `service.start` earns does not transfer.** What this waits on is a
    /// person reading a dialog: `Elevation::run` blocks with no deadline, and there is no declared
    /// ready timeout to bound it with. So the row exists the moment a client asks, and the work runs
    /// on `spawn_blocking` exactly as the trait's own documentation anticipates.
    ///
    /// **Cancellation is checked before the prompt and after it, and never during.** A cancellation
    /// token cannot close a UAC dialog, and pretending otherwise would report a job as cancelled
    /// while the person at the machine was still looking at a prompt with MixEngine's name on it.
    ///
    /// # Errors
    ///
    /// `precondition_failed` when nothing is waiting — the helper refuses an empty batch outright,
    /// so raising a prompt to discover that would be a dialog for nothing. `dependency_missing` when
    /// there is no helper beside this daemon. `privileged_required`, carrying `probe()`'s reason,
    /// when this machine cannot raise a prompt at all. `conflict`, naming the job already running,
    /// when a grant is in flight.
    ///
    /// [`Route::ByHand`] is not refused on a machine with no dialog: it raises none (ADR 0065).
    pub(crate) async fn grant(self: &Arc<Self>, route: Route) -> Result<JobSummary, Error> {
        let (waiting, helper) = self.preflight(None, route).await?;

        let elevation = Arc::clone(self);
        let started = self
            .jobs
            .begin(
                &JobKind::parse(rpc::method::ELEVATION_GRANT).expect("a valid kind"),
                move |handle| async move {
                    elevation
                        .flush(&handle, helper, waiting, Afterwards::Reconcile, route)
                        .await
                },
            )
            .await;

        match started {
            Ok(summary) => {
                // Only while the slot is still `Reserved`: the work may already have finished and
                // released it, and writing the id over a free slot would wedge every later grant.
                let mut state = self
                    .state
                    .lock()
                    .expect("the elevation slot is not held across an await");
                if state.slot == Slot::Reserved {
                    state.slot = Slot::Running(summary.id);
                }

                Ok(summary)
            }
            Err(error) => {
                self.release();
                Err(error)
            }
        }
    }

    /// Take the one grant slot, or say who has it.
    pub(super) fn reserve(&self) -> Result<(), Error> {
        let mut state = self
            .state
            .lock()
            .expect("the elevation slot is not held across an await");

        match state.slot {
            Slot::Free => {
                state.slot = Slot::Reserved;
                Ok(())
            }
            Slot::Reserved => Err(Error::new(
                ErrorCode::Conflict,
                "a grant is already starting",
            )),
            // Waiting for a terminal, the command is what a person needs, not the job.
            Slot::Running(job) if state.waiting.is_some() => Err(Error::new(
                ErrorCode::Conflict,
                format!("job {job} is waiting for a terminal to run the helper"),
            )
            .with_hint("`mix elevation status` shows the command it is waiting for")),
            Slot::Running(job) => Err(Error::new(
                ErrorCode::Conflict,
                format!("job {job} is already asking for permission"),
            )
            .with_hint(format!(
                "`mix job wait {job}` follows the one that is running"
            ))),
        }
    }

    /// Give the slot back.
    pub(super) fn release(&self) {
        self.state
            .lock()
            .expect("the elevation slot is not held across an await")
            .slot = Slot::Free;
    }

    /// The work: write the batch, raise the one prompt, apply what came back.
    pub(super) async fn flush(
        &self,
        handle: &crate::jobs::JobHandle,
        helper: Chosen,
        mut waiting: Vec<mixengine_proto::PendingOp>,
        afterwards: Afterwards,
        route: Route,
    ) -> Result<serde_json::Value, Error> {
        // Released however this ends — including through a panic the RPC layer contains, which is
        // the whole reason it is not a line at the bottom.
        let _slot = Released(self);

        // **The helper's own operation first** — the T182b design, D2. The batch is run by the
        // helper installed when it starts, so anything after a replacement would still be answered
        // by the old one. A stable sort keeps every other operation in the order it was queued.
        put_the_helper_first(&mut waiting);

        if handle.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "the grant was cancelled before any prompt was raised",
            ));
        }

        // A fresh single-use directory per grant: `response.json`'s existence is the anti-replay
        // check, so a directory that has been answered is finished (T40/D10).
        let directory = self.elevate.join(
            mixengine_platform::generate_secret(16)
                .map_err(|error| mixengine_core::Error::Platform(error).to_wire())?,
        );

        // The lower of what this daemon speaks and what the installed helper answered its handshake
        // with — roadmap task T88a, and `speaks` is where that is decided.
        let request = mixengine_core::elevation::write_request(
            &directory,
            &self.home,
            &waiting,
            helper.speaks,
        )
        .map_err(|error| error.to_wire())?;

        handle.progress(20, "asking for permission").await;

        let raised = match route {
            Route::Dialog => {
                let path = request.path().to_path_buf();
                let machine = Arc::clone(&self.host);
                tokio::task::spawn_blocking(move || machine.elevation().run(&helper.path, &path))
                    .await
                    .map_err(|join| {
                        Error::new(
                            ErrorCode::Internal,
                            format!("the elevation prompt could not be waited on: {join}"),
                        )
                    })?
            }
            Route::ByHand { .. } => Ok(self.by_hand(handle, &helper.path, &request).await),
        };

        let answer = self.judge(handle, &request, raised, &waiting).await;

        // **D8.** The machine may have just been wired, and a daemon that only learned that at its
        // next start would go on writing hosts entries while the user watched their grant do
        // nothing. Unconditional rather than "only if a resolver operation was in the batch": the
        // helper is the authority on what it applied, and a re-read costs one file or one registry
        // key.
        self.dns.reprobe(self.host.as_ref());

        // And the block a hosts-only home accumulated is now redundant, so it is cleared by the
        // grant that made it so rather than by a second prompt a week later. A failure here is
        // logged and not returned: the grant itself succeeded, and reporting it as failed because
        // the follow-up could not be queued would be a worse answer than the truth.
        //
        // **Not after an uninstall's grant** (T182b, D6): that prompt takes MixEngine off the
        // machine, and asking again for the block a site still wants would put back what it removed.
        if afterwards == Afterwards::Reconcile
            && let Err(error) = self.require_hosts().await
        {
            tracing::warn!(%error, "the hosts block could not be reconciled after the grant");
        }

        // On every branch, including the failing ones. The directory is single-use by construction,
        // and leaving one behind would make the next grant's fresh directory the only thing keeping
        // that true — a property worth having in two places rather than one.
        //
        // **`tokio::fs` and not `std::fs`**, which is the rule the rest of this crate keeps without
        // exception: a synchronous removal here runs on the worker thread that is serving every
        // other request. It is `tokio::fs` rather than the explicit `spawn_blocking` the certificate
        // and log paths use because there is one call to move, and that is exactly what `tokio::fs`
        // is — the same hand-off, without a closure to read past.
        if let Err(error) = tokio::fs::remove_dir_all(request.directory()).await {
            tracing::warn!(
                directory = %request.directory().display(),
                %error,
                "a single-use elevation request directory could not be removed"
            );
        }

        answer
    }

    /// Turn what the prompt answered into a job result, and apply it to the queue.
    async fn judge(
        &self,
        handle: &crate::jobs::JobHandle,
        request: &mixengine_core::elevation::Request,
        raised: mixengine_platform::Result<mixengine_platform::Raised>,
        waiting: &[mixengine_proto::PendingOp],
    ) -> Result<serde_json::Value, Error> {
        let mixengine_platform::Raised { outcome, said } =
            raised.map_err(|error| mixengine_core::Error::Platform(error).to_wire())?;
        let asked = waiting.len();
        let mut problems: Vec<String> = Vec::new();

        let (applied, still_pending) = match &outcome {
            // Every row kept, and the job **succeeds**: ADR 0005 says a declined prompt is a normal
            // outcome, never an error, and a failed job would put a red line in `mix job list` for
            // somebody exercising a choice the design offers them.
            ElevationOutcome::Declined => {
                tracing::info!("the elevation prompt was declined; nothing was applied");
                (0, asked)
            }

            // Kept too, but this one is a failure: nothing was asked and nothing can be until the
            // machine changes. The reason is the answer — on Linux it is a command to type.
            ElevationOutcome::Unavailable { reason } => {
                let error = Error::new(
                    ErrorCode::PrivilegedRequired,
                    format!("this machine cannot raise an elevation prompt: {reason}"),
                );
                self.remember(handle, &outcome, 0, asked, Vec::new());
                return Err(error);
            }

            // The helper **ran**. Whether it left a report is the next question, and "no report" is
            // a real state: T40a is explicit that `Completed` does not promise a file.
            ElevationOutcome::Completed => {
                handle.progress(70, "reading what the helper did").await;

                let report = match mixengine_core::elevation::read_report(request) {
                    Ok(report) => report,
                    // What the helper said is the only account of why it left nothing — a refusal
                    // writes no file beside a request it does not trust (T166, D6). `warn`, because
                    // until now it reached nothing above `debug`.
                    Err(mixengine_core::Error::Elevation(
                        ElevationError::ElevateReportMissing { path, .. },
                    )) => {
                        tracing::warn!(
                            said = said.as_deref().unwrap_or(""),
                            "the elevation helper ran and left no report"
                        );
                        self.remember(handle, &outcome, 0, asked, Vec::new());
                        return Err(mixengine_core::Error::Elevation(
                            ElevationError::ElevateReportMissing { path, said },
                        )
                        .to_wire());
                    }
                    Err(error) => {
                        self.remember(handle, &outcome, 0, asked, Vec::new());
                        return Err(error.to_wire());
                    }
                };

                let results: Vec<_> = request
                    .ids()
                    .iter()
                    .copied()
                    .zip(report.results.iter().cloned())
                    .collect();

                let settled = mixengine_core::elevation::settle(&self.store, &results)
                    .await
                    .map_err(|error| error.to_wire())?;

                // What this home now holds, so the producer can tell a plan already applied from a
                // new one — roadmap task T180.
                self.remember_the_firewall(waiting, &results).await;

                for (id, reason) in &settled.refused {
                    tracing::warn!(
                        %id,
                        reason,
                        "a privileged operation will not succeed as written and was dropped"
                    );
                }

                tracing::info!(
                    applied = settled.applied,
                    kept = settled.failed.len(),
                    refused = settled.refused.len(),
                    elevated = report.elevated,
                    helper = report.elevate_version,
                    "an elevated batch was applied"
                );

                // A batch that installed or replaced the helper is one whose answer the daemon no
                // longer holds: the one it knows about is the one it asked at start. **Only those two
                // operations, and not "any batch that changed the machine"**, which was the first
                // draft and undid an uninstall on CI's Windows runner (2026-09-07): the batch removed
                // the audit log last, the probe that followed ran under the runner's already-elevated
                // token, and an elevated helper writes a line for what it did — so the log the batch
                // had just removed was back before the uninstall read the machine a second time, and
                // the home was kept for it. A probe is only ever spent on what could have changed the
                // thing it asks about.
                if settled.applied > 0 && may_have_changed_the_helper(waiting) {
                    self.learn_installed_helper().await;
                }

                // **Every client that drew the list is told it shrank**, not only the one whose
                // "Allow" started this grant: the daemon raises a prompt of its own after an update
                // changed the helper (T182b, D2), while the window already shows the same row.
                if (settled.applied > 0 || !settled.refused.is_empty())
                    && let Err(error) = self.announce_what_is_left().await
                {
                    tracing::warn!(%error, "could not announce what is left in the queue");
                }

                // **The sentences, named by the operation they are about** — roadmap task
                // **T147**. Composed here rather than by a client: what an operation is called and
                // what it said are both the daemon's to know, and a client joining the two would be
                // a second place the wording lives.
                problems = settled
                    .refused
                    .iter()
                    .chain(settled.failed.iter())
                    .map(|(id, reason)| {
                        let what = waiting
                            .iter()
                            .find(|pending| pending.id == *id)
                            .map_or_else(
                                || format!("operation {id}"),
                                |pending| pending.op.name().to_owned(),
                            );

                        format!("{what}: {reason}")
                    })
                    .collect();

                (settled.applied, settled.failed.len())
            }
        };

        let grant = self.remember(handle, &outcome, applied, still_pending, problems);

        serde_json::to_value(&grant).map_err(|source| {
            Error::new(
                ErrorCode::Internal,
                format!("the grant's outcome could not be encoded: {source}"),
            )
        })
    }

    /// Record what this grant did, for `elevation.status`, and hand it back for the job's result.
    fn remember(
        &self,
        handle: &crate::jobs::JobHandle,
        outcome: &ElevationOutcome,
        applied: usize,
        still_pending: usize,
        problems: Vec<String>,
    ) -> GrantOutcome {
        let grant = GrantOutcome {
            job: handle.id(),
            at: Timestamp::from_system_time(SystemTime::now()),
            outcome: outcome.clone(),
            applied,
            still_pending,
            problems,
        };

        self.state
            .lock()
            .expect("the elevation slot is not held across an await")
            .last = Some(grant.clone());

        grant
    }
}
