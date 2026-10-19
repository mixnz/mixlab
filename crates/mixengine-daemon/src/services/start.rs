//! Starting services — roadmap task **T207d** gave it a file.
//!
//! The walk over a start plan, the one service at a time it is made of, the registration of a
//! runner under the lock a shutdown drains, and how a walk waits for that runner to decide. The
//! state it reads and writes is the [`Registry`]'s.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use mixengine_core::services::{Plan, ServiceGraph};
use mixengine_platform::process::Adopted;
use mixengine_proto::{LogSubject, ServiceId, ServiceSpec, ServiceState, StateReason};
use tokio::sync::{Notify, watch};

use super::runner::{Readiness, Runner};
use super::{Fault, Registry, Running, Start, Walk, databases, first_run, lock, record};

impl Registry {
    /// Bring `id` up if it is not already, and say nothing if it was — roadmap task **T77a**.
    ///
    /// **Not a second start path**: it builds the same graph and the same plan `service.start`
    /// builds and hands them to the same walk, so a dependency comes up first and a first-run ritual
    /// is performed, exactly as it would be for somebody typing `mix service start`. What differs is
    /// only the answer — a caller about to run a statement wants "it is up" or a refusal, not a walk
    /// to render.
    ///
    /// # Errors
    ///
    /// A graph that will not build, no such service, or a service that would not start — carrying
    /// the persisted reason where the walk recorded one, because "mariadb@main did not start" with
    /// no cause is a sentence that sends somebody to look in the wrong place.
    pub(crate) async fn ensure_running(
        &self,
        id: &ServiceId,
    ) -> Result<(), mixengine_proto::Error> {
        use crate::error::ToWire as _;

        let graph = self.graph().await.map_err(|error| error.to_wire())?;
        let plan = graph
            .start_plan([id])
            .map_err(|error| mixengine_core::Error::Graph(error).to_wire())?;

        match self.start(&graph, &plan).await.failed {
            None => Ok(()),

            Some((failed, fault)) => {
                // **The one start failure this daemon can do better than "read the log"** — roadmap
                // task **T127a**. T127 built the repair and reached it from the provisioning probe,
                // which is a *running* server; this is the same sentence for a database that never
                // got there.
                let hint = match &fault.reason {
                    Some(StateReason::SuperuserRefused { .. }) => databases::repair_hint(&failed),
                    _ => format!("`mix service logs {failed}` has what it printed"),
                };

                // The runner's sentence first (T202a, D3): what it wrote beside the reason is what
                // a person acts on, and the state machine's word is the fallback.
                Err(mixengine_proto::Error::new(
                    mixengine_proto::ErrorCode::PreconditionFailed,
                    fault.said(&failed),
                )
                .with_hint(hint))
            }
        }
    }

    /// Start everything in `plan`, in its order, waiting for each to be ready before the next.
    ///
    /// A service that is **already up** is counted as reached rather than restarted: `mix service
    /// start` on something that is up is a request for it to be up. One that is merely already
    /// *supervised* — in a restart backoff, or mid-start for another walk — is not the same thing: it
    /// is asked to start now and waited for. Both decisions are [`Registry::begin`]'s, because the
    /// first has to be taken under the same lock as the registration. See the note there.
    ///
    /// **Every service in the plan is asked, not only the one the caller named.** A plan is already
    /// the transitive set, this walks it one service at a time, and a `db` in its fourth crash is
    /// exactly what a person typing `mix service start web` needs unstuck — a walk that woke the top
    /// of the plan and left its dependencies sitting out their backoffs would fail, and tell them to
    /// go and start `db` by hand.
    pub(crate) async fn start(&self, graph: &ServiceGraph, plan: &Plan) -> Walk {
        self.start_because(graph, plan, StateReason::Requested)
            .await
    }

    /// [`Registry::start`], naming why — roadmap task **T113**.
    ///
    /// The one caller that passes anything else is the boot walk, which starts what a home's
    /// `autostart` settings asked for: nobody requested those, and an event saying somebody did
    /// would make the setting a way to forge a request. Everything else about the walk is
    /// identical, including that a service already up is counted as reached without being
    /// restarted — so a service recovery adopted is not re-explained.
    pub(crate) async fn start_because(
        &self,
        graph: &ServiceGraph,
        plan: &Plan,
        because: StateReason,
    ) -> Walk {
        let mut walk = Walk::default();

        for id in plan.flat() {
            let Some(spec) = graph.spec(id) else {
                // Unreachable through the API, where the plan is built from this same graph. Worth
                // a line rather than a panic: it would take the daemon down for one bad request.
                tracing::error!(
                    service = id.as_str(),
                    "the plan names a service the graph does not hold"
                );

                walk.failed = Some((id.clone(), Fault::unexplained()));
                break;
            };

            match self.begin(spec, because.clone()).await {
                Start::Ready => walk.reached.push(id.clone()),

                Start::Failed(fault) => {
                    walk.failed = Some((id.clone(), fault));
                    break;
                }
            }
        }

        // **Nothing is marked when the daemon is on its way out**, which is the one case where the
        // sentence this would persist is not true. A walk that `begin` refused did not meet a
        // service that failed, so writing `DependencyFailed` under its name would leave every
        // dependent recorded as broken by a database that was stopping perfectly — and leave it that
        // way on disk, for whoever opens `mix service list` after the next boot.
        if let Some((failed, _)) = &walk.failed
            && !self.is_shutting_down()
        {
            walk.blocked = self.block(graph, plan, failed).await;
        }

        walk
    }

    /// Have `spec` supervised if it is not already, and wait until it is decided whether it is up.
    ///
    /// **Already supervised is answered in here, under the lock that registers.** Asking first and
    /// spawning afterwards would be two decisions where there is one: the daemon's runtime is
    /// multi-threaded, so two `service.start` for the same service would both find nothing running
    /// and both spawn, and the second registration would overwrite the first — leaving a process
    /// holding the port and the data directory that no `stop` and no shutdown can still name.
    ///
    /// **What that lock decides is whether to spawn, and nothing about the service.** A runner is
    /// alive through a restart backoff, through a stop and through a start that has not finished, so
    /// the answer comes from the [`Readiness`] it publishes rather than from its task being alive:
    /// `mix service start` on something genuinely up is a request for it to be up and is reached,
    /// while on something in its fourth crash it is not, and the tier below must not be started
    /// against it.
    ///
    /// **A runner that is not up is asked rather than only read** — T19c, and the case a person is
    /// most likely to type `mix service start` at. Reading a service crash-looping under
    /// [`RestartPolicy::Always`](mixengine_proto::RestartPolicy::Always) can only ever report the
    /// attempt that just failed: its runner never deregisters, nothing in this path could shorten the
    /// backoff it is sitting in, and every start therefore re-walked the tier, emitted two more
    /// events and spawned nothing. So the request goes *in*, and what this then waits for is the
    /// attempt that request causes rather than the one before it.
    ///
    /// **A daemon on its way out starts nothing, and that answer is taken under the registration
    /// lock like the other two.** `api/rpc.rs` runs every handler in a `tokio::spawn` nothing joins,
    /// so a `service.start` outlives the connection that asked for it: one landing beside a
    /// `mix daemon stop` used to walk past the place the shutdown had already been, register a fresh
    /// runner into a map [`Registry::shut_down`] had already drained, and spawn a process nothing
    /// was left to wait for — killed a moment later by
    /// [`Supervised`](mixengine_platform::process::Supervised)'s destructor, leaving a row mid-write
    /// for the next boot's crash recovery to clean up. Reading the answer outside the lock would
    /// leave that same window open a few instructions narrower; reading it under the lock that
    /// [`Registry::shut_down`] drains under closes it, because then the two are one order.
    ///
    /// It is refused for a service that is already up as well, which is the one case that could
    /// arguably be answered [`Start::Ready`]. A shutdown that has begun is going to stop that
    /// service too, and answering a client that it is up — moments before the daemon takes it
    /// down — would be true for less time than it takes to render.
    /// `because` is why this start is happening, and reaches the move into `Starting` — roadmap
    /// task **T113**.
    async fn begin(&self, spec: &ServiceSpec, because: StateReason) -> Start {
        let id = spec.id().clone();

        // **Before the ritual and before the lock, on every path into a start** — roadmap task
        // **T70a**. The addresses this may be holding on the service's behalf are the ones the
        // process is about to bind, and a release that happened after the spawn would be a database
        // that fails to start reporting a taken address without being able to say what took it.
        // `running` below is a `std` mutex and this awaits, which is the second reason it is here
        // rather than lower.
        self.holder.release(&id).await;

        // **Before the spawn, and outside the lock** — roadmap task T33. A ritual is minutes of
        // work, and holding a `std` mutex across it would stop every other `service.*` call for the
        // whole of a bootstrap. Nothing is registered yet, so a second start arriving mid-ritual
        // finds nothing supervised and asks the same question again — which reads the markers, finds
        // our own in-progress one, and clears and redoes rather than colliding.
        //
        // The refusal below persists nothing, exactly as the shutting-down one does not: no row
        // moved and no event was published, and the durable account is the job row, which holds the
        // step that failed and what it printed.
        if let Some(plan) = self.ritual_for(&id)
            && let Err(error) = first_run::ensure(&self.host, &self.jobs, &plan).await
        {
            tracing::error!(
                service = id.as_str(),
                error = %error,
                "this service has never been started here and its first run did not finish"
            );

            return Start::Failed(Fault {
                reason: Some(StateReason::FirstRunFailed {
                    detail: error.message.clone(),
                }),
                detail: None,
            });
        }

        let (mut readiness, asked) = {
            // Held across the spawn as well, so that a runner which ends immediately cannot
            // deregister an entry that has not been made yet. Nothing awaits while it is held.
            let mut running = lock(&self.running);

            if self.is_shutting_down() {
                // `None`, because nothing was persisted about this and nothing should be: no row
                // moved, no event was published, and the service is where it was. What a client
                // renders is the walk naming the service it did not start, and the sentence saying
                // why is this line in `daemon.log`.
                tracing::warn!(
                    service = id.as_str(),
                    "refusing to start this service: the daemon is shutting down"
                );

                return Start::Failed(Fault::unexplained());
            }

            let supervised = running
                .get(&id)
                .filter(|entry| !entry.task.is_finished())
                .map(|entry| (entry.readiness.clone(), Arc::clone(&entry.asked_to_start)));

            if let Some((mut readiness, asked_to_start)) = supervised {
                // Read and marked seen in the same breath as the request, which is what makes the
                // wait below sound: whatever the runner publishes after this — including a start that
                // is up again before this function is next polled — is a change this receiver has not
                // seen, so it cannot be missed and the value it replaces cannot be mistaken for it.
                let before = readiness.borrow_and_update().clone();

                match before {
                    // Already where the caller wants it. Nothing is asked for, and nothing must be:
                    // a request left as an unconsumed permit would cut short the backoff of some
                    // crash an hour from now that nobody asked about.
                    Readiness::Up => (readiness, None),

                    _ => {
                        asked_to_start.notify_one();

                        (readiness, Some(before))
                    }
                }
            } else {
                (self.supervise(spec, &mut running, None, because), None)
            }
        };

        match asked {
            Some(before) => settled_after_asking(&mut readiness, before).await,
            None => settled(&mut readiness).await,
        }
    }

    /// Put a task in charge of this service and register it. The readiness it will publish on.
    ///
    /// **The caller holds the lock**, which is what the `running` argument says rather than
    /// documents: registering has to happen in the same critical section as the decision to spawn,
    /// or two `service.start` for one service would each find nothing running, each spawn, and leave
    /// a process holding the port that no stop can still name.
    ///
    /// `adopted` is the difference between the two ways a runner begins: [`None`] spawns the process
    /// itself, and [`Some`] takes over one that survived the daemon that started it (roadmap task
    /// T18). Everything after the first life of the process is the same code either way, which is
    /// the reason this is one function and not two.
    ///
    /// `because` is why this service's first life is beginning, and is the reason published with its
    /// move into `Starting` — roadmap task **T113**. It is ignored on the `adopted` path, where
    /// there is no first life to explain: the process was already running when this daemon started.
    pub(super) fn supervise(
        &self,
        spec: &ServiceSpec,
        running: &mut HashMap<ServiceId, Running>,
        adopted: Option<Adopted>,
        because: StateReason,
    ) -> watch::Receiver<Readiness> {
        let id = spec.id().clone();
        let cancel = self.shutdown.child_token();
        let generation = self.generations.fetch_add(1, Ordering::Relaxed);
        let (published, readiness) = watch::channel(Readiness::Deciding);
        let asked_to_start = Arc::new(Notify::new());
        let asked_to_reload = Arc::new(Notify::new());

        // Seeded with what the spec already says, so a runner that never hears from a client still
        // has the right value to apply — and so the first `changed()` is a real change.
        let (limits, limits_asked) = watch::channel(spec.limits());

        // Seeded with `None`: unless the sweeper says otherwise before it cancels, a stop is a stop
        // somebody asked for, which is what every stop before T69 was.
        let (stopping_because, because_asked) = watch::channel(None);

        // Seeded with `None`: a service nothing has measured yet is a service under its ceiling as
        // far as anything here knows, and the first finished minute is a real change.
        let (over_memory, over_memory_asked) = watch::channel(None);

        let runner = Runner {
            spec: spec.clone(),
            store: self.store.clone(),
            directory: self.paths.service_logs(&id),
            // Sized here rather than inside the runner, because this is where a policy meets the
            // log that outlives the runner reading it: the same spec's `ring_lines`, applied to
            // both rings, so what a client is served and what a crash-loop quotes agree about how
            // much of a service is worth keeping.
            log: self.logs.feeding(
                &LogSubject::Service { id: id.clone() },
                usize::from(spec.logs().ring_lines),
            ),
            host: Arc::clone(&self.host),
            // **T127a.** [`None`] for everything that is not a database — the same map
            // `database.create` reads, filled by the [`Registry::graph`] every start goes through.
            superuser: self
                .provisioning_for(&id)
                .map(|provisioning| provisioning.root_user()),
            events: self.events.clone(),
            cancel: cancel.clone(),
            asked_to_start: Arc::clone(&asked_to_start),
            asked_to_reload: Arc::clone(&asked_to_reload),
            limits_asked,
            stopping_because: because_asked,
            over_memory: over_memory_asked,
            budget: self.budget.clone(),
            // Built by the first spawn of this life, or on demand by a service this runner adopted.
            surroundings: None,
            reading: None,
            readiness: published,
        };

        let deregister = Arc::clone(&self.running);
        let logs = Arc::clone(&self.logs);
        let named = id.clone();

        let task = tokio::spawn(async move {
            match adopted {
                Some(adopted) => runner.adopt(adopted).await,
                None => runner.run(because).await,
            }

            let mut running = lock(&deregister);
            if running
                .get(&named)
                .is_some_and(|entry| entry.generation == generation)
            {
                running.remove(&named);
            }

            // After the deregistration and under the same tidy-up, because they answer the same
            // question about the same moment: nothing is supervising this service any more. A
            // client that still has a `follow` open keeps the log — which is what lets its stream
            // carry on when the service is started again — and one that has gone leaves the daemon
            // holding a ring for a service nobody is watching.
            logs.forget_if_unwatched(&LogSubject::Service { id: named });
        });

        running.insert(
            id,
            Running {
                cancel,
                asked_to_start,
                asked_to_reload,
                limits,
                stopping_because,
                over_memory,
                task,
                generation,
                readiness: readiness.clone(),
            },
        );

        readiness
    }

    /// Mark everything that can no longer come up, and say which edge broke.
    ///
    /// The dependency named is the **direct** one each service declares rather than the root of the
    /// chain, which is why this accumulates as it walks: a chain of four then reads as four honest
    /// sentences leading to the one service that actually broke, instead of three copies of a name
    /// none of them mention.
    ///
    /// Through `Starting`, because that is the only edge the machine has into `Failed` from where
    /// these services are — and it is true: they were asked to start, and this is how that ended.
    async fn block(&self, graph: &ServiceGraph, plan: &Plan, failed: &ServiceId) -> Vec<ServiceId> {
        let Ok(blocked) = graph.blocked_by(failed) else {
            return Vec::new();
        };

        let mut hopeless: BTreeSet<ServiceId> = std::iter::once(failed.clone()).collect();
        let mut marked = Vec::new();

        for id in plan.flat() {
            if !blocked.contains(id) || self.is_running(id) {
                continue;
            }

            let Ok(dependencies) = graph.dependencies_of(id) else {
                continue;
            };

            let Some(dependency) = dependencies
                .iter()
                .find(|dependency| hopeless.contains(*dependency))
            else {
                continue;
            };

            let reason = StateReason::DependencyFailed {
                dependency: dependency.clone(),
            };

            // Both writes are attempted, and the second is not conditional on the first: if the
            // row is somewhere `Starting` cannot be reached from, `Failed` may still be reachable
            // from where it actually is, and the alternative is a row left claiming a start that
            // nothing is performing.
            let entered = record(
                &self.store,
                &self.events,
                id,
                ServiceState::Starting,
                StateReason::Requested,
            )
            .await;

            if record(&self.store, &self.events, id, ServiceState::Failed, reason).await {
                marked.push(id.clone());
            } else if entered {
                tracing::error!(
                    service = id.as_str(),
                    "this service was recorded as starting and then could not be recorded as \
                     failed; its row now names a start nothing is performing"
                );
            }

            hopeless.insert(id.clone());
        }

        marked
    }
}

/// Wait until a runner has decided whether its service is up, and say so in the walk's terms.
///
/// **Bounded by the restart policy where the policy is bounded, and by one attempt where it is
/// not.** A policy with a ceiling arrives at `Running` or at `Failed` by itself, and this waits
/// through its backoffs for whichever it reached — a first crash under the default `OnFailure` is a
/// transient, not an answer. Only a
/// [`RestartPolicy::Always`](mixengine_proto::RestartPolicy::Always) never reaches `Failed` at all,
/// and there the outcome of the attempt in flight is the only thing there is to wait for. Which of
/// the two applies is [`Readiness::of`](Readiness::of)'s to decide; a service being put back
/// by its policy is meanwhile reported by its events, where a client can see it being tried again.
///
/// A closed channel is a runner that ended without deciding: a task that panicked, or one whose
/// first `Starting` would not persist. Both are in `daemon.log` already, and neither is a state a
/// client could render — which is what [`Start::Failed`]'s [`None`] says.
pub(super) async fn settled(readiness: &mut watch::Receiver<Readiness>) -> Start {
    loop {
        // Taken by value rather than matched in place, so no borrow of the channel is held across the
        // await below. Marking it seen here is also what keeps `changed` from missing the next one.
        let decided = readiness.borrow_and_update().clone();

        if let Some(start) = decided_by(decided) {
            return start;
        }

        if readiness.changed().await.is_err() {
            return Start::Failed(Fault::unexplained());
        }
    }
}

/// The same, for a runner that has just been **asked** to start — T19c.
///
/// What such a runner is publishing at the moment it is asked is the attempt *before* the request:
/// `Down` with the crash the backoff is being served for. Waiting on that would answer the caller
/// with the failure their own request is in the middle of correcting, which is the bug this task
/// exists for, so the first thing waited for here is the next thing the runner says. It will say
/// something: a runner released by a request moves to `Starting`, and one that cannot persist even
/// that ends and closes the channel.
///
/// **Unless it was already ending**, which is the one race worth spending a branch on: a runner in
/// its `Stopping`, or three statements from returning `Failed`, has no backoff left to be released
/// from and the request is simply dropped with it. Then `before` — what it last managed to say — is
/// still the truth about the service, and reporting it keeps the reason a client can render instead
/// of trading it for the [`None`] that means "the daemon's own problem".
async fn settled_after_asking(
    readiness: &mut watch::Receiver<Readiness>,
    before: Readiness,
) -> Start {
    if readiness.changed().await.is_err() {
        return decided_by(before).unwrap_or(Start::Failed(Fault::unexplained()));
    }

    settled(readiness).await
}

/// What a readiness answers a walk, or [`None`] while it answers nothing yet.
fn decided_by(readiness: Readiness) -> Option<Start> {
    match readiness {
        Readiness::Up => Some(Start::Ready),
        Readiness::Down(fault) => Some(Start::Failed(fault)),
        Readiness::Deciding => None,
    }
}

#[cfg(test)]
mod characterization;

#[cfg(test)]
mod tests;
