//! A daemon going away.
//!
//! The [`Budget`] a shutdown grants every stop still to come, the flag that refuses a start once one
//! has begun, and the wait for everything still supervised. The state it reads and writes is the
//! [`Registry`]'s.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mixengine_proto::ServiceId;
use tokio::time::Instant;

use super::{Registry, Running, Stopping, lock};

/// The moment every stop now in flight has to be finished by.
///
/// **One deadline for the whole daemon, not one per service, and that is the point.** A
/// [`ServiceSpec`] says how long *its* service needs in order to shut down cleanly, which is a
/// statement about MariaDB and stays true however many other services there are; what nothing owned
/// until this budget is the sum. Eight of them each asking for ten seconds is eighty seconds a user
/// is sitting through after typing `mix daemon stop`, and a `StopBehaviour::Command` that really
/// runs a program is what turned that from arithmetic into something that happens.
///
/// So a shutdown sets this once and every runner reads it as it stops: each service gets what its
/// spec asks for or what is left, whichever is less. The rule is the one a stop command already
/// follows a level down, inside a single grace period — whatever is allowed, minus what has already
/// been spent.
///
/// [`None`] is the ordinary state of the daemon and means *no ceiling but the spec's*: a
/// `mix service stop mariadb` on a running machine is not a shutdown and has no total to divide.
///
/// Shared rather than passed, because the two things that need to agree about it are at opposite
/// ends of the process — the API handler that grants the budget, and a runner task started an hour
/// earlier that will spend it. A `std` mutex holding a `Copy` value: nothing awaits while it is held.
///
/// [`ServiceSpec`]: mixengine_proto::ServiceSpec
#[derive(Debug, Clone, Default)]
pub(crate) struct Budget(Arc<Mutex<Option<Deadlines>>>);

/// The two moments a shutdown keeps: when every stop has to be finished, and the one after it.
///
/// The second exists for [`Budget::reprieve`] and is [`CONFIRMATION_REPRIEVE`](crate::CONFIRMATION_REPRIEVE)
/// past the first. Held together rather than in two locks because they are narrowed as one decision.
#[derive(Debug, Clone, Copy)]
struct Deadlines {
    /// What every stop now in flight has to be finished by.
    stops: Instant,

    /// What the one wait a spent budget still permits has to be finished by.
    reprieve: Instant,
}

impl Budget {
    /// Give everything that stops from now on until `deadline`, and no longer.
    ///
    /// **Narrowing only.** A shutdown that arrives while another is running — a `daemon.shutdown`
    /// answered by a client, and the console event that closed the window a moment later — must not
    /// have its clock extended by the second one: the OS ceiling that motivated the tighter of the
    /// two is not something the daemon may grant itself more of. Both deadlines narrow together, so
    /// the reprieve of a shutdown that has been tightened is the tighter one's.
    pub(super) fn narrow_to(&self, deadline: Instant) {
        let asked = Deadlines {
            stops: deadline,

            // Saturating in the only direction that matters: a deadline so far out that adding a
            // quarter second to it overflows is one nothing was ever going to reach anyway, and the
            // reprieve then grants exactly what the deadline itself does.
            reprieve: deadline
                .checked_add(crate::CONFIRMATION_REPRIEVE)
                .unwrap_or(deadline),
        };

        let mut held = lock(&self.0);

        *held = Some(held.map_or(asked, |existing| Deadlines {
            stops: existing.stops.min(asked.stops),
            reprieve: existing.reprieve.min(asked.reprieve),
        }));
    }

    /// How much of it is left, or [`None`] when nothing is counting.
    ///
    /// Zero once the deadline has passed, which is a real answer rather than a missing one: a
    /// service reached after the budget ran out is killed at once instead of starting a fresh grace
    /// period of its own.
    pub(super) fn remaining(&self) -> Option<Duration> {
        lock(&self.0).map(|held| held.stops.saturating_duration_since(Instant::now()))
    }

    /// How much is left of the window a wait may reach into once [`Budget::remaining`] is zero.
    ///
    /// **For the one wait that zero answers wrongly** — the poll that asks whether a killed survivor
    /// has left the process table, where no window at all reports a process that stopped correctly
    /// as one that would not go. See [`CONFIRMATION_REPRIEVE`](crate::CONFIRMATION_REPRIEVE) for why
    /// that question is not like the other waits the budget shortens, and
    /// [`Runner::seeing_it_go`](super::runner::Runner) for the only caller.
    ///
    /// **Shared across the walk**, because it is a moment and not an allowance: two survivors reached
    /// after the budget ran out look into the same window rather than into one each, which is what
    /// keeps this out of the ceiling arithmetic.
    ///
    /// [`None`] when nothing is counting, and the caller then has no reason to ask — there is no
    /// spent budget for it to be making up for.
    pub(super) fn reprieve(&self) -> Option<Duration> {
        lock(&self.0).map(|held| held.reprieve.saturating_duration_since(Instant::now()))
    }
}

impl Registry {
    /// Everything that stops from now on has until `now + grace`, however long its own spec asks
    /// for.
    ///
    /// Called by both ways a daemon stops and by nothing else: `daemon.shutdown` before it walks the
    /// stop plan, and the accept loop before it cancels the root token on a signal. The two differ
    /// only in the number they pass, which is where "one budget, two ceilings" lives — see
    /// [`Budget`] for why a second call can only ever shorten it.
    ///
    /// **A `grace` this clock cannot name leaves the budget where it was, and that is arithmetic
    /// rather than a fallback.** `Instant + Duration` panics on overflow, and this is the one path
    /// where a panic costs more than the request it is in: the signal half runs on the main task, so
    /// the unwinding goes straight past `Store::close` and leaves the write-ahead log
    /// uncheckpointed — a `-wal` sidecar holding the newest commits, for a number somebody typed
    /// into `config.toml`. `mixengine-core` clamps `shutdown_grace_seconds` on the way in and this
    /// deliberately does not rest on that: one clamp is a decision about a config file, and this is
    /// the shutdown path, which must be unable to panic on whatever reaches it.
    ///
    /// Skipping the call is what the arithmetic would have done anyway. A deadline centuries out
    /// loses every `min` in [`Budget::narrow_to`] and bounds nothing, which is exactly what
    /// [`Budget`]'s [`None`] already means — *no ceiling but the spec's* — and a grace that large is
    /// a request for precisely that. What it must not also mean is "no shutdown", which is why the
    /// flag below is set before the arithmetic is attempted rather than after it succeeds.
    pub(crate) fn stopping_within(&self, grace: Duration) {
        self.shutting_down.store(true, Ordering::Relaxed);

        let Some(deadline) = Instant::now().checked_add(grace) else {
            tracing::warn!(
                ?grace,
                "this shutdown was granted more time than this machine's clock can name; every \
                 service gets the grace period its own spec asks for and nothing bounds the sum"
            );

            return;
        };

        self.budget.narrow_to(deadline);
    }

    /// Whether this daemon has begun going away, by either of the two routes it has.
    ///
    /// **Two, because the flag and the token are set at different moments and neither is early
    /// enough on its own.** `daemon.shutdown` grants a budget and *then* walks the stop plan in
    /// dependency order, cancelling the root token only once that walk is done — so for the whole
    /// of the walk, the token says nothing has happened. A signal cancels the token, and a test or
    /// a future caller may cancel it without granting anything. Either one is this daemon on its
    /// way out, and [`Registry::begin`] has to refuse from the first of them.
    ///
    /// Also read by `daemon.shutdown`, which is what makes it `pub(crate)`: a request to stop that
    /// arrives when one of these is already true is somebody asking a second time, and what that
    /// means is [`Registry::stopping_within`] with nothing left in it.
    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Relaxed) || self.shutdown.is_cancelled()
    }

    /// Wait for every supervised service to have stopped.
    ///
    /// **The order is deliberately not this function's.** By the time the daemon calls it the root
    /// token has been cancelled, so every runner is already performing the stop its spec asks for;
    /// what this adds is the *waiting*, so the process does not exit while a database is still
    /// flushing and leave the job to [`Supervised`](mixengine_platform::process::Supervised)'s
    /// destructor, which kills rather than asks.
    ///
    /// Stopping in reverse dependency order is `daemon.shutdown`, which walks [`Registry::stop`]
    /// *before* anything cancels the root token. A signal cancels everything at once and there is
    /// no order left to impose.
    ///
    /// **The two things this takes besides the entries are what keep it from colliding with a walk
    /// that is still going.** A signal arriving mid-`daemon.shutdown` gets here while the handler is
    /// still stepping through the stop plan, and both halves of that were a bug the shape of a false
    /// report: a `begin` that had passed its check would register a runner into a map this has
    /// already emptied, and the walk's next [`Registry::stop_one`] would find its entry gone, read a
    /// row still saying `Stopping`, and call a service that is stopping perfectly a stop that
    /// failed. So the flag and every stop in flight are claimed in the same critical section as the
    /// drain — see [`Registry::stopping`] for why the order of the two locks is the pairing rather
    /// than only the deadlock.
    pub(crate) async fn shut_down(&self) {
        let running: Vec<(ServiceId, Running, Option<Stopping>)> = {
            let mut running = lock(&self.running);

            self.shutting_down.store(true, Ordering::Relaxed);

            running
                .drain()
                .map(|(id, entry)| {
                    // `None` is a stop somebody else claimed before this drain reached it, and it is
                    // theirs to release: that caller took its entry out of the map under this same
                    // lock, so it is not one of the entries here.
                    let claimed = self.claim_stop(&id).ok();

                    (id, entry, claimed)
                })
                .collect()
        };

        if running.is_empty() {
            return;
        }

        tracing::info!(
            services = running.len(),
            "waiting for supervised services to stop"
        );

        // Each claim is dropped as its service's turn ends, which is what releases a walk waiting on
        // that one while this is still working through the rest.
        for (id, entry, _stopping) in running {
            entry.cancel.cancel();

            if let Err(error) = entry.task.await {
                tracing::warn!(
                    service = id.as_str(),
                    %error,
                    "the task supervising this service did not finish cleanly"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use mixengine_proto::{Millis, ReadyCheck, ServiceState, StateReason, StopBehaviour};
    use mixengine_testkit::FakeService;

    use super::super::Fault;
    use super::super::fixture::{
        Declared, EVENTUALLY, arguments, home, registry, row, service, spec,
    };
    use super::*;

    /// **The budget bounds the sum, which is what nothing owned before there was one.**
    ///
    /// Two services each asking for a minute to stop in, and neither of them able to use it: the
    /// stop command runs and never returns, which is what a `mariadb-admin shutdown` against a
    /// server that has stopped answering looks like. Unbudgeted that is two minutes; the whole point
    /// is that somebody who typed `mix daemon stop` waits for what they were told rather than for
    /// the sum of what the specs happen to say.
    ///
    /// **`Command` and not `Signal`, which is what makes this the same test on all three systems.**
    /// Windows sends no request to stop at all (ADR 0008), so a `Signal` spec spends no grace period
    /// there and the assertion would pass without the clamp existing — a green test proving nothing
    /// on the one system whose console clock motivated the task. A stop command really runs
    /// everywhere, and the budget is visible as the deadline it is given.
    ///
    /// **The elapsed time is the assertion and the timeout is its floor.** Either the clamp applies
    /// or the walk sits in the first service's minute; the margin between the two is two orders of
    /// magnitude, so there is no third outcome for a loaded runner to land in.
    #[tokio::test]
    async fn a_shutdown_budget_bounds_every_service_left_to_stop_and_not_each_one() {
        let (_home, paths, store) = home(&["db", "web"]).await;

        // Asks, and never comes back. Nothing here creates the file the service would exit for, so
        // what ends each service is the kill after its grace period — whenever that turns out to be.
        let unanswering = FakeService::new();

        let stubborn = |id| {
            spec(id)
                .args(arguments(&FakeService::new().ignoring_stop()))
                .stop(StopBehaviour::Command {
                    program: FakeService::program(),
                    args: arguments(&unanswering),
                    grace: Millis::from_secs(60),
                })
                .build()
                .expect("a usable spec")
        };

        let declared = Declared(vec![stubborn("db"), stubborn("web")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("two declared services");
        let started = registry.start(&graph, &graph.start_order()).await;
        assert_eq!(started.reached.len(), 2, "{started:?}");

        registry.stopping_within(Duration::from_millis(500));

        let began = Instant::now();
        let stopped = tokio::time::timeout(EVENTUALLY, registry.stop(&graph.stop_order()))
            .await
            .expect("a stop that is bounded by the budget rather than by the specs");
        let took = began.elapsed();

        assert_eq!(stopped.reached.len(), 2, "both stopped: {stopped:?}");
        assert!(
            took < Duration::from_secs(10),
            "the two services took {took:?}, which is the specs' sixty seconds each rather than \
             the budget's half-second for both"
        );
    }

    /// The half of [`Budget`] that is a rule rather than arithmetic.
    #[test]
    fn a_second_shutdown_can_shorten_a_budget_and_never_extend_one() {
        let budget = Budget::default();

        assert_eq!(
            budget.remaining(),
            None,
            "the ordinary state of a daemon: no total to divide, and a spec's grace is the whole \
             answer"
        );

        budget.narrow_to(Instant::now() + Duration::from_secs(30));
        budget.narrow_to(Instant::now() + Duration::from_secs(2));

        let left = budget.remaining().expect("a shutdown is under way");
        assert!(
            left <= Duration::from_secs(2),
            "a console event arriving during a `daemon.shutdown` brings an OS clock with it, and \
             the daemon may not grant itself more of it: {left:?}"
        );

        // And the other direction, which is the one an accidental `max` would get wrong.
        budget.narrow_to(Instant::now() + Duration::from_secs(30));
        assert!(budget.remaining().expect("still under way") <= Duration::from_secs(2));
    }

    /// **A grace no clock can name is a shutdown that bounds nothing, and not one that panics.**
    ///
    /// `Instant + Duration` panics on overflow, and the signal half of a shutdown runs on the
    /// daemon's own main task: the unwinding would go straight past `Store::close` and leave the
    /// write-ahead log uncheckpointed — the next boot opening a database without its newest
    /// commits, because of a number somebody put in `config.toml`. `mixengine-core`'s loader clamps
    /// the setting and this is the second answer to the same number, which is the right number of
    /// answers for a panic that costs a database.
    ///
    /// **The two halves are asserted separately because they used to be one value.** A deadline
    /// that cannot be expressed leaves the budget where it was, which is exactly `Budget`'s `None`
    /// — no ceiling but each spec's — and that must not also mean "no shutdown is happening", which
    /// is what the refused start below is the observable half of.
    #[tokio::test]
    async fn a_shutdown_grace_no_clock_can_hold_bounds_nothing_rather_than_panicking() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        registry.stopping_within(Duration::MAX);

        assert_eq!(
            registry.budget.remaining(),
            None,
            "a deadline further away than this machine's clock can hold bounds nothing, and a \
             grace that large is a request for precisely that"
        );

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("caddy")]).expect("a plan");
        let walk = registry.start(&graph, &plan).await;

        assert!(
            matches!(&walk.failed, Some((id, fault)) if id == &service("caddy") && fault.reason.is_none()),
            "a shutdown whose budget could not be expressed is still a shutdown, and it starts \
             nothing: {walk:?}"
        );
        assert_eq!(
            row(&store, &service("caddy")).await.1,
            None,
            "a process was spawned by a daemon that is shutting down"
        );
    }

    /// A stop that arrives mid-start must not be held by the ready check it interrupts.
    #[tokio::test]
    async fn a_stop_during_a_ready_check_does_not_wait_the_check_out() {
        let (_home, paths, store) = home(&["slow"]).await;

        let fake = FakeService::new().never_ready();
        let declared = Declared(vec![
            spec("slow")
                .args(arguments(&fake))
                .ready(ReadyCheck::LogPattern {
                    regex: mixengine_testkit::service::READY_LINE.to_owned(),
                    // Far longer than this test may take, on purpose: a runner that only looked at
                    // its token after `ready::wait` returned would be reported by the timeout below
                    // rather than by an assertion that has to guess at a threshold.
                    timeout: Millis::from_secs(600),
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = Arc::new(registry(&paths, &store, Arc::new(declared)));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("slow")]).expect("a plan");

        let walking = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.start(&graph, &plan).await })
        };

        // A recorded pid is the runner having spawned the process and entered the ready check,
        // which is the only moment this test is about.
        let deadline = tokio::time::Instant::now() + EVENTUALLY;
        while row(&store, &service("slow")).await.1.is_none() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the service never spawned"
            );

            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        registry.shutdown.cancel();
        tokio::time::timeout(EVENTUALLY, registry.shut_down())
            .await
            .expect("the stop was not held by the ready check it interrupted");

        let walk = tokio::time::timeout(EVENTUALLY, walking)
            .await
            .expect("the walk was told how the start ended")
            .expect("the walk did not panic");

        assert!(
            matches!(
                &walk.failed,
                Some((id, Fault { reason: Some(StateReason::Requested), .. })) if id == &service("slow")
            ),
            "a service stopped before it was ready did not come up, and says why: {walk:?}"
        );

        let (state, pid) = row(&store, &service("slow")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(pid, None);
    }

    /// **A daemon on its way out starts nothing** — the other half of the same collision, and the
    /// one that leaves a process behind rather than a wrong sentence.
    ///
    /// `api/rpc.rs` runs every handler in the daemon's `Background`, which outlives the connection,
    /// so a `service.start` that lands while `mix daemon stop` is walking outlives the connection
    /// that asked for it. It used
    /// to walk past the place the shutdown had already been, register a runner into a map
    /// `shut_down` had already drained, and spawn a process nothing was left to wait for: killed by
    /// `Supervised`'s destructor as the runtime came down, leaving a row mid-write for the next
    /// boot's crash recovery to sort out.
    ///
    /// **Both routes into it are asserted, because they begin at different moments.** Granting the
    /// budget is the first thing `daemon.shutdown` does and the token is cancelled only after its
    /// ordered walk, so for the whole of that walk the token says nothing has happened; a signal is
    /// the other way round and may grant no budget this registry can hold. The refusal is the same
    /// either way, and so is the evidence for it: no row moved, and no process was spawned.
    #[tokio::test]
    async fn a_start_that_arrives_once_a_shutdown_has_begun_is_refused() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let granted = registry(&paths, &store, Arc::new(declared));

        let graph = granted.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("caddy")]).expect("a plan");

        // What `daemon.shutdown` does before it walks anything, and the whole of the window this
        // test is about: from here the daemon is going away, and the root token says nothing yet.
        granted.stopping_within(Duration::from_secs(5));

        let walk = tokio::time::timeout(EVENTUALLY, granted.start(&graph, &plan))
            .await
            .expect("the start was refused rather than waited on");

        assert!(walk.reached.is_empty(), "{walk:?}");
        assert!(
            matches!(&walk.failed, Some((id, fault)) if id == &service("caddy") && fault.reason.is_none()),
            "the walk names the service it would not start, with nothing to quote as a reason: \
             {walk:?}"
        );

        let (state, pid) = row(&store, &service("caddy")).await;
        assert_eq!(
            state,
            ServiceState::Stopped,
            "a service that was refused is where it was; a refusal is not a start that failed"
        );
        assert_eq!(
            pid, None,
            "a process was spawned by a daemon that is shutting down, and nothing is left to wait \
             for it"
        );
        assert!(
            lock(&granted.running).is_empty(),
            "a runner was registered into a map the shutdown is about to drain"
        );

        // The other route, and the one a signal takes: the root token is cancelled with no budget
        // ever granted, which is still this daemon going away.
        let signalled = registry(
            &paths,
            &store,
            Arc::new(Declared(vec![
                spec("caddy").build().expect("a usable spec"),
            ])),
        );
        signalled.shutdown.cancel();

        let walk = tokio::time::timeout(EVENTUALLY, signalled.start(&graph, &plan))
            .await
            .expect("the start was refused rather than waited on");

        assert!(
            matches!(&walk.failed, Some((id, fault)) if id == &service("caddy") && fault.reason.is_none()),
            "a cancelled root token is a daemon that starts nothing: {walk:?}"
        );
        assert!(lock(&signalled.running).is_empty(), "{walk:?}");
    }

    /// **A signal arriving mid-walk does not turn the rest of that walk into failures.**
    ///
    /// `daemon.shutdown` stops services in dependency order and a Ctrl-C in the middle of it breaks
    /// the accept loop, which cancels the root token and calls `shut_down` — so the whole `running`
    /// map is drained underneath a walk that is still stepping through it. The walk's next service
    /// then had no entry to wait for and read a row while the shutdown was still waiting for that
    /// very runner, reporting a stop that was going perfectly as one that failed. The budget half of
    /// this case was handled from the start; the walk half was not.
    ///
    /// Staged so the overlap is not a coincidence: `db` is given a stop three times longer than
    /// `web`'s, so the walk reaches it while the shutdown is unambiguously still inside it.
    #[tokio::test]
    async fn a_shutdown_draining_the_map_under_a_walk_does_not_fail_the_rest_of_it() {
        let (home, paths, store) = home(&["db", "web"]).await;

        // One file per service: the stop command creates it as it starts, which is both what the
        // service is waiting to exit for and what this test watches to know where the walk is.
        let asked = |id: &str| home.path().join(format!("{id}-was-asked-to-stop"));

        let slow_to_stop = |id: &str, millis: u64| {
            let stop = FakeService::new().touch(asked(id)).exit_after(millis);

            spec(id)
                .args(arguments(
                    &FakeService::new().ignoring_stop().exit_when(asked(id)),
                ))
                .stop(StopBehaviour::Command {
                    program: FakeService::program(),
                    args: arguments(&stop),
                    grace: Millis::from_secs(30),
                })
        };

        let declared = Declared(vec![
            slow_to_stop("db", 1_500).build().expect("a usable spec"),
            slow_to_stop("web", 400)
                .depends_on(service("db"))
                .build()
                .expect("a usable spec"),
        ]);
        let registry = Arc::new(registry(&paths, &store, Arc::new(declared)));

        let graph = registry.graph().await.expect("two declared services");
        assert_eq!(
            registry.start(&graph, &graph.start_order()).await.reached,
            vec![service("db"), service("web")]
        );

        let walking = {
            let registry = Arc::clone(&registry);
            let order = graph.stop_order();

            tokio::spawn(async move { registry.stop(&order).await })
        };

        // `web`'s stop command having started is the walk being inside its first service, which is
        // the only moment a signal can arrive in for this test to be about anything.
        let deadline = tokio::time::Instant::now() + EVENTUALLY;
        while !asked("web").is_file() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the walk never reached the first service's stop"
            );

            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        // The signal: everything released at once, and the map emptied under the walk.
        registry.shutdown.cancel();
        tokio::time::timeout(EVENTUALLY, registry.shut_down())
            .await
            .expect("every runner finished");

        let walk = tokio::time::timeout(EVENTUALLY, walking)
            .await
            .expect("the walk was answered")
            .expect("the walk did not panic");

        assert_eq!(
            walk.reached,
            vec![service("web"), service("db")],
            "a service the shutdown was stopping was reported as one that would not stop: {walk:?}"
        );
        assert!(walk.failed.is_none(), "{walk:?}");

        assert_eq!(row(&store, &service("db")).await.0, ServiceState::Stopped);
        assert_eq!(row(&store, &service("web")).await.0, ServiceState::Stopped);
    }

    #[tokio::test]
    async fn a_daemon_on_its_way_out_waits_for_what_it_supervises() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("caddy")]).expect("a plan");
        registry.start(&graph, &plan).await;

        // What a signal does: the root token every runner hangs off is cancelled, and then the
        // daemon waits rather than dropping the tasks on the floor.
        registry.shutdown.cancel();
        tokio::time::timeout(EVENTUALLY, registry.shut_down())
            .await
            .expect("every runner finished");

        let (state, pid) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(pid, None);
        assert!(lock(&registry.running).is_empty());
    }
}
