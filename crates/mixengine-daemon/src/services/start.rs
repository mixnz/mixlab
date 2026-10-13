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
mod tests {
    use mixengine_proto::{Backoff, DaemonEvent, Millis, ReadyCheck, RestartPolicy};
    use mixengine_testkit::FakeService;

    use super::super::fixture::{
        Declared, EVENTUALLY, SILENCE, arguments, home, registry, registry_on, row, service, spec,
    };
    use super::*;

    /// A service that dies before it is ever ready, under a policy that never gives up.
    ///
    /// The backoff is far longer than either test that uses this needs, deliberately: what both
    /// assert about is the gap between two attempts, and a short wait would race them into asserting
    /// against a service that had moved on to the next one.
    fn crash_looping(id: &str) -> ServiceSpec {
        let broken = FakeService::new().never_ready().exit_after(50).exit_code(3);

        spec(id)
            .args(arguments(&broken))
            .restart(RestartPolicy::Always {
                backoff: Backoff {
                    initial: Millis::from_secs(30),
                    max: Millis::from_secs(30),
                    ..Backoff::default()
                },
            })
            .build()
            .expect("a usable spec")
    }

    /// A service that comes up, stays up for a moment and then dies, under the same never-give-up
    /// policy and the same long backoff.
    ///
    /// **The slow ready is the point.** Between the registration and `Running` lies the one window in
    /// which [`Registry::begin`] can ask a runner that is not in a backoff, and what a test needs to
    /// be able to do is land a second walk inside it. A second is far longer than the two database
    /// writes a walk takes to get there, and the exit is late enough to be unambiguously after it.
    fn crashes_after_coming_up(id: &str) -> ServiceSpec {
        let brittle = FakeService::new()
            .ready_after(1_000)
            .exit_after(3_000)
            .exit_code(3);

        spec(id)
            .args(arguments(&brittle))
            .restart(RestartPolicy::Always {
                backoff: Backoff {
                    initial: Millis::from_secs(30),
                    max: Millis::from_secs(30),
                    ..Backoff::default()
                },
            })
            .build()
            .expect("a usable spec")
    }

    /// A provisioning statement goes to a *running* server, so the method that provisions starts one
    /// — and starting one that is already up is not a failure to report to anybody.
    ///
    /// Roadmap task **T77a**. What this is really asserting is the second call: `ensure_running` is
    /// reached on every `database.create`, including the ones on a home where everything is already
    /// up, and a version of it that answered "already started" as an error would make the ordinary
    /// case the failing one.
    #[tokio::test]
    async fn ensuring_a_service_is_running_is_idempotent() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        registry
            .ensure_running(&service("caddy"))
            .await
            .expect("it comes up");
        registry
            .ensure_running(&service("caddy"))
            .await
            .expect("and it is still up");

        let (state, _) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Running);

        let graph = registry.graph().await.expect("one declared service");
        let stopping = graph.stop_plan([&service("caddy")]).expect("a plan");
        registry.stop(&stopping).await;
    }

    /// A service this home does not declare is a refusal rather than a start.
    #[tokio::test]
    async fn ensuring_a_service_nothing_declares_is_refused() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        registry
            .ensure_running(&service("mariadb"))
            .await
            .expect_err("nothing declares it");
    }

    #[tokio::test]
    async fn a_service_starts_runs_and_stops() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("caddy")]).expect("a plan");

        let walk = registry.start(&graph, &plan).await;

        assert_eq!(walk.reached, vec![service("caddy")], "{walk:?}");
        assert!(walk.failed.is_none(), "{walk:?}");

        let (state, pid) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Running);
        assert!(pid.is_some(), "the pid of a running service is recorded");

        // The log file is the supervisor's, and this is the one assertion that it is being written
        // for a service the *registry* started rather than one a supervisor test spawned.
        let log = paths
            .service_logs(&service("caddy"))
            .join(mixengine_supervisor::logs::CURRENT_LOG_FILE_NAME);
        assert!(log.is_file(), "{} was not written", log.display());

        let stopping = graph.stop_plan([&service("caddy")]).expect("a plan");
        let stopped = registry.stop(&stopping).await;

        assert_eq!(stopped.reached, vec![service("caddy")], "{stopped:?}");
        assert!(stopped.failed.is_none(), "{stopped:?}");

        let (state, pid) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(pid, None, "a stopped service names no process");
    }

    /// T202a, D3. The detail the runner persisted with the reason reaches the walk, so the
    /// sentence a caller prints first is the runner's. A spec that names a credential nothing
    /// holds is the failure that found this.
    #[tokio::test]
    async fn a_walk_carries_the_detail_the_runner_wrote_with_the_reason() {
        let (_home, paths, store) = home(&["mariadb"]).await;
        let declared = Declared(vec![
            spec("mariadb")
                .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb/root")
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("mariadb")]).expect("a plan");
        let walk = registry.start(&graph, &plan).await;

        let (failed, fault) = walk.failed.expect("the start failed");
        assert_eq!(failed, service("mariadb"));
        assert_eq!(fault.reason, Some(StateReason::SpawnFailed));
        assert!(
            fault.detail.as_deref().is_some_and(|detail| {
                detail.contains("no credential is stored at mixengine/mariadb/root")
            }),
            "{fault:?}"
        );
    }

    /// T202a, D3. `ensure_running` says the detail first, and the reason when there is no detail.
    #[tokio::test]
    async fn ensure_running_says_the_runners_detail_before_the_state_machines_reason() {
        let (_home, paths, store) = home(&["mariadb"]).await;
        let declared = Declared(vec![
            spec("mariadb")
                .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb/root")
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let error = registry
            .ensure_running(&service("mariadb"))
            .await
            .expect_err("it cannot start");

        assert!(
            error
                .message
                .starts_with("mariadb did not start: the environment entry"),
            "{}",
            error.message
        );
        assert!(
            !error.message.contains("could not be started at all"),
            "the state machine's sentence is not the first one any more: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn a_service_that_never_becomes_ready_fails_rather_than_waiting_forever() {
        let (_home, paths, store) = home(&["slow"]).await;

        let fake = FakeService::new().never_ready();
        let declared = Declared(vec![
            spec("slow")
                .args(arguments(&fake))
                .ready(ReadyCheck::LogPattern {
                    regex: mixengine_testkit::service::READY_LINE.to_owned(),
                    timeout: Millis(750),
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("slow")]).expect("a plan");

        let walk = registry.start(&graph, &plan).await;

        assert!(
            matches!(
                &walk.failed,
                Some((id, Fault { reason: Some(StateReason::ReadyTimeout { after }), .. }))
                    if id == &service("slow") && *after == Millis(750)
            ),
            "a ready timeout says how long it waited: {walk:?}"
        );

        let (state, pid) = row(&store, &service("slow")).await;
        assert_eq!(state, ServiceState::Failed);
        assert_eq!(
            pid, None,
            "a service that was killed for never becoming ready names no process"
        );
    }

    /// Roadmap task **T38**: the failure a user can do something about, in place of the one they
    /// cannot.
    ///
    /// The same shape as the test above — a service that never becomes ready — with one difference:
    /// the machine says a program MixEngine does not manage is on the port this service declared.
    /// "not ready within 750ms" is true of both and sends the reader to the service's own log,
    /// where a fixture that never announces itself has nothing to say.
    #[tokio::test]
    async fn a_service_whose_port_is_held_by_another_program_says_which_one() {
        let (_home, paths, store) = home(&["slow"]).await;

        let fake = FakeService::new().never_ready();
        let declared = Declared(vec![
            spec("slow")
                .args(arguments(&fake))
                .ports([3306])
                .ready(ReadyCheck::LogPattern {
                    regex: mixengine_testkit::service::READY_LINE.to_owned(),
                    timeout: Millis(750),
                })
                .build()
                .expect("a usable spec"),
        ]);
        let squatter = mixengine_platform::mock::Host::with_a_port_held(
            paths.root(),
            3306,
            mixengine_platform::PortHolder {
                pid: Some(4242),
                name: Some("mysqld.exe".to_owned()),
            },
        );
        let registry = registry_on(&paths, &store, Arc::new(declared), Arc::new(squatter));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("slow")]).expect("a plan");

        let walk = registry.start(&graph, &plan).await;

        assert!(
            matches!(
                &walk.failed,
                Some((id, Fault { reason: Some(StateReason::PortInUse { port, program: Some(program), .. }), .. }))
                    if id == &service("slow") && *port == 3306 && program == "mysqld.exe"
            ),
            "the reason names the program on the port, not the symptom: {walk:?}"
        );

        let (state, _) = row(&store, &service("slow")).await;
        assert_eq!(state, ServiceState::Failed);
    }

    /// The invariant behind the check-and-register being one step: whatever else two starts do,
    /// there is never a second process for a service that already has one.
    #[tokio::test]
    async fn starting_a_service_that_is_already_up_does_not_spawn_a_second_one() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("caddy")]).expect("a plan");

        registry.start(&graph, &plan).await;
        let (_, first) = row(&store, &service("caddy")).await;
        assert!(first.is_some(), "the first start recorded a process");

        let again = registry.start(&graph, &plan).await;

        assert_eq!(again.reached, vec![service("caddy")], "{again:?}");
        assert!(again.failed.is_none(), "{again:?}");
        assert_eq!(
            row(&store, &service("caddy")).await.1,
            first,
            "the process being supervised is still the one the first start spawned"
        );
        assert_eq!(
            lock(&registry.running).len(),
            1,
            "a second runner would be one the registry can no longer name"
        );

        let stopping = graph.stop_plan([&service("caddy")]).expect("a plan");
        registry.stop(&stopping).await;
    }

    #[tokio::test]
    async fn what_depends_on_a_failure_is_never_spawned() {
        let (_home, paths, store) = home(&["db", "web"]).await;

        // Dies without ever announcing itself. **`never_ready` is not decoration**: a service that
        // printed its ready line and then died would be racing the ready check against the exit,
        // which is a real behaviour `ready::wait` biases towards the exit for — but it is not what
        // this test is about, and a test whose subject is decided by a race is a test that passes
        // most of the time.
        let broken = FakeService::new().never_ready().exit_after(50).exit_code(3);

        let declared = Declared(vec![
            spec("db")
                .args(arguments(&broken))
                .build()
                .expect("a usable spec"),
            spec("web")
                .depends_on(service("db"))
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("two declared services");
        let plan = graph.start_plan([&service("web")]).expect("a plan");

        let walk = registry.start(&graph, &plan).await;

        assert!(
            matches!(&walk.failed, Some((id, _)) if id == &service("db")),
            "the walk stops at the service that failed: {walk:?}"
        );
        assert_eq!(walk.blocked, vec![service("web")], "{walk:?}");

        assert_eq!(row(&store, &service("db")).await.0, ServiceState::Failed);
        assert_eq!(row(&store, &service("web")).await.0, ServiceState::Failed);
    }

    /// A walk waits for the attempt in flight and **not** for the policy to give up, because a policy
    /// is allowed never to: `RestartPolicy::Always` has no ceiling, so nothing about a service under
    /// it will ever reach `Failed`, and a walk that waited for that would never come back at all.
    #[tokio::test]
    async fn a_service_whose_policy_never_gives_up_does_not_hold_the_walk_for_ever() {
        let (_home, paths, store) = home(&["db"]).await;
        let registry = registry(
            &paths,
            &store,
            Arc::new(Declared(vec![crash_looping("db")])),
        );

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("db")]).expect("a plan");

        let walk = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
            .await
            .expect("the walk was answered after the first attempt");

        assert!(
            matches!(
                &walk.failed,
                Some((id, Fault { reason: Some(StateReason::Exited { code: Some(3) }), .. }))
                    if id == &service("db")
            ),
            "the walk says how the attempt ended rather than that the policy ran out: {walk:?}"
        );
        assert_eq!(
            row(&store, &service("db")).await.0,
            ServiceState::Restarting,
            "the runner is still putting it back, which is what its policy asks for"
        );

        registry.shutdown.cancel();
        tokio::time::timeout(EVENTUALLY, registry.shut_down())
            .await
            .expect("every runner finished");
    }

    /// The other half of that rule, and the half the default policy lives on: a ceiling is something
    /// a walk *can* wait for, so it does. `OnFailure`'s first crash is a transient the runner
    /// recovers from a backoff later, and a walk that took it for an answer would leave the tier
    /// below `Failed` and unsupervised beside a service that went on to come up by itself.
    #[tokio::test]
    async fn a_walk_waits_out_the_retries_a_bounded_policy_is_allowed() {
        let (_home, paths, store) = home(&["db", "web"]).await;
        let broken = FakeService::new().never_ready().exit_after(50).exit_code(3);

        let declared = Declared(vec![
            spec("db")
                .args(arguments(&broken))
                // One retry, and a backoff short enough that what the test spends its time on is
                // the wait being *taken* rather than the wait itself.
                .restart(RestartPolicy::OnFailure {
                    max_retries: 1,
                    window: Millis::from_secs(300),
                    backoff: Backoff {
                        initial: Millis(50),
                        max: Millis(50),
                        ..Backoff::default()
                    },
                })
                .build()
                .expect("a usable spec"),
            spec("web")
                .depends_on(service("db"))
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("two declared services");
        let plan = graph.start_plan([&service("web")]).expect("a plan");

        let walk = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
            .await
            .expect("the walk was answered once the policy ran out");

        // Two attempts and the crash loop that ended them, not the first `Exited`: that reason here
        // would be a walk that gave up while the runner was still going to try again.
        assert!(
            matches!(
                &walk.failed,
                Some((id, Fault { reason: Some(StateReason::CrashLoop { attempts: 2, .. }), .. }))
                    if id == &service("db")
            ),
            "the walk is answered by the policy running out, not by one crash: {walk:?}"
        );
        assert_eq!(walk.blocked, vec![service("web")], "{walk:?}");
        assert_eq!(row(&store, &service("db")).await.0, ServiceState::Failed);
        assert_eq!(
            row(&store, &service("web")).await.1,
            None,
            "`web` was never spawned"
        );
    }

    /// **The invariant behind reading readiness rather than a task's liveness.** A runner stays alive
    /// for as long as it keeps putting a service back, and a service in the gap between two attempts
    /// is not somewhere a dependent can be started against.
    #[tokio::test]
    async fn a_service_in_a_restart_backoff_is_not_reported_as_up() {
        let (_home, paths, store) = home(&["db", "web"]).await;
        let declared = Declared(vec![
            crash_looping("db"),
            spec("web")
                .depends_on(service("db"))
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("two declared services");
        let plan = graph.start_plan([&service("web")]).expect("a plan");

        let first = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
            .await
            .expect("the first walk was answered");

        assert_eq!(first.blocked, vec![service("web")], "{first:?}");

        // What the second walk arrives to: a runner that is alive, and a service that is not up.
        assert_eq!(
            row(&store, &service("db")).await.0,
            ServiceState::Restarting
        );

        let again = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
            .await
            .expect("the second walk was answered");

        assert!(
            matches!(&again.failed, Some((id, _)) if id == &service("db")),
            "a supervised service that is not up has to stop the walk: {again:?}"
        );
        assert!(
            again.reached.is_empty(),
            "nothing in this plan is up, so nothing was reached: {again:?}"
        );
        assert_eq!(
            lock(&registry.running).len(),
            1,
            "the second walk spawned a second runner for a service that already has one"
        );
        assert_eq!(
            row(&store, &service("web")).await.1,
            None,
            "`web` was never spawned against a database that is between crashes"
        );

        registry.shutdown.cancel();
        tokio::time::timeout(EVENTUALLY, registry.shut_down())
            .await
            .expect("every runner finished");
    }

    /// **The other half of that rule, and T19c.** Reading a crash-looping runner is right about the
    /// service not being up and useless as an answer to somebody who has just *asked* for it to
    /// start: nothing in that path could shorten the backoff the runner is sitting in, so every
    /// attempt re-walked the tier, emitted two more events and spawned nothing. A start now reaches
    /// the runner.
    #[tokio::test]
    async fn an_explicit_start_cuts_short_the_backoff_a_crash_loop_is_sitting_in() {
        let (_home, paths, store) = home(&["db"]).await;
        let registry = registry(
            &paths,
            &store,
            Arc::new(Declared(vec![crash_looping("db")])),
        );

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("db")]).expect("a plan");

        let first = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
            .await
            .expect("the first walk was answered");

        assert!(
            matches!(&first.failed, Some((id, _)) if id == &service("db")),
            "{first:?}"
        );
        assert_eq!(
            row(&store, &service("db")).await.0,
            ServiceState::Restarting,
            "the runner is in the backoff this test is about"
        );

        // Subscribed while that backoff is being served, so the only events on this stream are the
        // ones the second walk causes. Nothing else is running and a 30 second wait is silent.
        let mut watching = registry.events.subscribe();

        // **The timeout is the assertion.** `crash_looping`'s backoff is longer than `EVENTUALLY`, and
        // this walk is not answered until the attempt the request causes has been decided — so a
        // request that did not reach the runner could not be answered here at all.
        let again = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
            .await
            .expect("the request cut the backoff short rather than waiting it out");

        assert!(
            matches!(
                &again.failed,
                Some((id, Fault { reason: Some(StateReason::Exited { code: Some(3) }), .. }))
                    if id == &service("db")
            ),
            "the walk is answered by the attempt the request caused: {again:?}"
        );
        assert_eq!(
            lock(&registry.running).len(),
            1,
            "the service was asked to start again, not given a second runner"
        );

        // And it went back as a *request* and not as the policy coming round, which is the difference
        // `Restarts::recovered` records and the reason somebody reading the log needs.
        let frame = tokio::time::timeout(EVENTUALLY, watching.next())
            .await
            .expect("the stream is not silent")
            .expect("the stream is still open");

        let crate::api::events::Frame::Event(DaemonEvent::ServiceStateChanged(change)) = frame
        else {
            panic!("the first thing the second walk published was not a state change: {frame:?}");
        };

        assert_eq!(change.to, ServiceState::Starting);
        assert_eq!(change.reason, StateReason::Requested);

        registry.shutdown.cancel();
        tokio::time::timeout(EVENTUALLY, registry.shut_down())
            .await
            .expect("every runner finished");
    }

    /// **The limit of that, and the half a request must not outlive.** A runner is only listening for
    /// one while it is sitting out a backoff, so a request that arrives mid-start is *kept* — and if
    /// that start succeeds, nothing consumes it for as long as the service stays up. The crash after
    /// that would then be released the instant it entered its backoff, its ladder reset and its move
    /// published as `Requested`, on behalf of somebody who asked an hour earlier and got what they
    /// asked for. The start that answers a request is what takes it.
    #[tokio::test]
    async fn a_start_asked_for_mid_start_is_taken_by_the_start_that_answers_it() {
        let (_home, paths, store) = home(&["db"]).await;
        let registry = registry(
            &paths,
            &store,
            Arc::new(Declared(vec![crashes_after_coming_up("db")])),
        );

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("db")]).expect("a plan");

        // The two walks a shared dependency produces, and the only way to reach the window this
        // test is about: one of them registers the runner, the other finds it `Deciding` a second
        // short of ready and asks it to start — which is a request no `wait_out` is going to take.
        let (first, second) = tokio::time::timeout(EVENTUALLY, async {
            tokio::join!(registry.start(&graph, &plan), registry.start(&graph, &plan))
        })
        .await
        .expect("both walks were answered");

        assert!(first.failed.is_none(), "{first:?}");
        assert!(second.failed.is_none(), "{second:?}");
        assert_eq!(
            lock(&registry.running).len(),
            1,
            "the second walk asked the first walk's runner rather than spawning another"
        );

        // Subscribed once the service is up, so the only events on this stream are the ones its
        // crash causes — and the request, if it survived, is the only thing that could cause more.
        let mut watching = registry.events.subscribe();

        loop {
            let frame = tokio::time::timeout(EVENTUALLY, watching.next())
                .await
                .expect("the service crashed as the fixture says it does")
                .expect("the stream is still open");

            let crate::api::events::Frame::Event(DaemonEvent::ServiceStateChanged(change)) = frame
            else {
                continue;
            };

            if change.to == ServiceState::Restarting {
                assert_eq!(
                    change.reason,
                    StateReason::Exited { code: Some(3) },
                    "the crash is the fixture's, and nobody asked for it"
                );

                break;
            }
        }

        // **The silence is the assertion.** The backoff this runner has just entered is thirty
        // seconds and nothing has asked for anything since the start that succeeded; a request left
        // over from that start would end it here, within a millisecond, as a `Starting` reading
        // `Requested`.
        let next = tokio::time::timeout(SILENCE, watching.next()).await;

        assert!(
            next.is_err(),
            "the runner left a backoff nobody asked it to leave: {next:?}"
        );

        registry.shutdown.cancel();
        tokio::time::timeout(EVENTUALLY, registry.shut_down())
            .await
            .expect("every runner finished");
    }
}
