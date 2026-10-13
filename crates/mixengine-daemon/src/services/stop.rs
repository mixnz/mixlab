//! Stopping services — roadmap task **T207d** gave it a file.
//!
//! The walk over a stop plan, and the claim that makes two stops of one service one stop. The
//! state it reads and writes is the [`Registry`]'s.

use std::sync::Arc;

use mixengine_core::services::{self, Plan};
use mixengine_proto::{ServiceId, StateReason};
use tokio::sync::watch;

use super::{Fault, Registry, Stopping, Walk, lock};

impl Registry {
    /// Stop everything in `plan`, in its order, waiting for each to have gone before the next.
    ///
    /// A service that is not running is already where the caller wants it, so nearly every stop
    /// reaches everything it was asked to. **Nearly, and not always — since T18.** A survivor this
    /// daemon adopted and could not kill keeps its row in `stopping` and keeps holding the port, and
    /// a walk that reported it as reached would tell somebody their database is down while it is
    /// answering queries. So the row is what decides, and a service that is still supervised when its
    /// runner has finished stops the walk.
    ///
    /// **Stopping there rather than carrying on is the stop order doing its job.** A plan is
    /// dependents first — `web` before the `db` it talks to — precisely so that nothing is left
    /// pointed at a service that is going away; going on to stop `db` because `web` would not die
    /// would produce exactly the arrangement the order exists to prevent.
    pub(crate) async fn stop(&self, plan: &Plan) -> Walk {
        let mut walk = Walk::default();

        for id in plan.flat() {
            if !self.stop_one(id).await {
                // No reason to carry: what a client would render here is the state the row is still
                // in, which it can already read, and the sentence saying why is the runner's own
                // `error!` in `daemon.log`.
                walk.failed = Some((id.clone(), Fault::unexplained()));
                break;
            }

            walk.reached.push(id.clone());
        }

        walk
    }

    /// [`Registry::stop`], saying why — roadmap task **T123**.
    ///
    /// **The stop half of [`Registry::start_because`]**, and it exists for what the row keeps rather
    /// than for what the event says. `daemon.shutdown` and a person's `service.stop` walk the same
    /// [`Registry::stop`], so both left `stopped_by = 'person'` behind — and on-demand activation,
    /// which may not undo a person's stop, then refused to wake anything at all after a restart.
    ///
    /// **Set before the walk rather than inside it**, on the idle sweeper's reasoning: a runner
    /// reads its reason at the moment it enters `Stopping`, so a value written per service as the
    /// walk reaches it would be in time — and one written after the walk would not. What the sweeper
    /// also does, and this deliberately does not, is take the reason back on a stop that did not
    /// happen: this is called by a daemon on its way out, and there is no next stop to mislabel.
    pub(crate) async fn stop_because(&self, plan: &Plan, reason: &StateReason) -> Walk {
        for id in plan.flat() {
            self.stopping_because(id, Some(reason.clone()));
        }

        self.stop(plan).await
    }

    /// Cancel one service, wait for its runner to finish, and say where that left it.
    ///
    /// **The answer comes from the row and not from the task having ended**, which is the whole
    /// reason this returns anything at all. Since T18 a runner can finish with the service still up:
    /// a survivor this daemon adopted and could not kill leaves its row in `stopping` on purpose —
    /// see [`Runner::stop_adopted`][runner] — because writing `stopped` for a process that is still
    /// holding the port is the one lie crash recovery exists to prevent. The task ends either way,
    /// so a caller that read only that would report the stop it did not get.
    ///
    /// A row nobody can read is not evidence the service is still running, and neither is one that
    /// is not there: both are answered `true`, because the stop itself was performed and the failure
    /// is the daemon's own — it is in `daemon.log`, and it is not a state a client could render.
    ///
    /// **A second caller is given the first one's stop rather than a race against it.** Taking the
    /// entry out of the map is what used to make two of these two different stops: whoever arrived
    /// second found [`None`], skipped the wait that the answer depends on, and read a row mid-stop —
    /// `Stopping`, which [`ServiceState::is_supervised`] counts, so a service being shut down
    /// perfectly was reported as one that would not stop, and `mix daemon stop` exited non-zero
    /// pointing at a database that was fine. It happens two ways and neither is exotic: two clients,
    /// or a signal whose [`Registry::shut_down`] drains the map underneath a walk that is still
    /// going. So the stop is claimed before the entry is taken, in the same critical section, and a
    /// caller that finds it claimed waits for the claim to be released and then reads the same row
    /// this would have — see [`Stopping`], and [`Registry::stopping`] for the lock order that makes
    /// claim-and-take one decision rather than two.
    ///
    /// [`runner`]: super::runner::Runner
    /// [runner]: super::runner::Runner
    /// [`ServiceState::is_supervised`]: mixengine_proto::ServiceState::is_supervised
    async fn stop_one(&self, id: &ServiceId) -> bool {
        // Bound in a statement of its own so the guards are dropped before the awaits below: an
        // `if let` would hold them across, and a lock held over an await is one no other thread can
        // take for as long as this stop lasts.
        let claimed = {
            let mut running = lock(&self.running);

            self.claim_stop(id)
                .map(|stopping| (stopping, running.remove(id)))
        };

        // Held until the row has been read rather than until the task has ended, so that a third
        // caller arriving in between waits for this answer instead of racing the same read.
        let _stopping = match claimed {
            Ok((stopping, entry)) => {
                if let Some(entry) = entry {
                    entry.cancel.cancel();

                    if let Err(error) = entry.task.await {
                        tracing::warn!(
                            service = id.as_str(),
                            %error,
                            "the task supervising this service did not finish cleanly"
                        );
                    }
                }

                Some(stopping)
            }

            // Somebody else's stop, and what is waited for is the whole of it. The result is
            // discarded on purpose: a sender dropped without sending is a stop whose caller went
            // away, which is the same news as one that finished — it is no longer in flight, and
            // the row below is what says where it left the service either way.
            Err(mut over) => {
                let _ = over.wait_for(|finished| *finished).await;

                None
            }
        };

        // Asked after the task, never before: a runner writes `Stopped` and *then* returns, so this
        // reads what the stop actually persisted rather than what it was about to.
        match services::record(&self.store, id).await {
            Ok(record) => !record.state.is_supervised(),

            Err(error) => {
                tracing::error!(
                    service = id.as_str(),
                    %error,
                    "cannot read where stopping this service left it; reporting it as stopped"
                );

                true
            }
        }
    }

    /// Become the stop of this service, or be handed the one that already is.
    ///
    /// [`Err`] carries a receiver released when the stop in flight is over, however it ended — see
    /// [`Stopping`]. **The caller holds [`Registry::running`]**, which is what the argument-free
    /// signature cannot say and the two callers both do: a claim taken a statement away from the
    /// entry it belongs to is a claim a concurrent [`Registry::shut_down`] can drain out from under.
    pub(super) fn claim_stop(&self, id: &ServiceId) -> Result<Stopping, watch::Receiver<bool>> {
        let mut stopping = lock(&self.stopping);

        if let Some(over) = stopping.get(id) {
            return Err(over.clone());
        }

        let (finished, over) = watch::channel(false);
        stopping.insert(id.clone(), over);

        Ok(Stopping {
            service: id.clone(),
            stopping: Arc::clone(&self.stopping),
            finished,
        })
    }
}

#[cfg(test)]
mod tests {
    use mixengine_core::Store;
    use mixengine_proto::{Millis, ServiceState, StopBehaviour};
    use mixengine_testkit::FakeService;

    use super::super::fixture::{
        Declared, EVENTUALLY, arguments, home, registry, row, service, spec,
    };
    use super::super::now;
    use super::*;

    /// **A service whose spec stops it with a command of its own is stopped by running it** —
    /// roadmap task T15a, and the behaviour Phase 3's MariaDB rests on.
    ///
    /// The fixture is what makes this provable rather than merely plausible. The service ignores
    /// every request to stop, so neither a `SIGTERM` nor — on Windows, where there is no such
    /// request at all (ADR 0008) — anything else can end it politely; the only route left is the
    /// file it watches for, and the only thing that creates that file is the program the spec names
    /// as its stop command. So the two assertions are one claim from two sides: the file is there,
    /// and the row records a *clean* exit, which a kill could not have produced.
    #[tokio::test]
    async fn a_service_with_a_stop_command_is_stopped_by_running_it() {
        let (home, paths, store) = home(&["mariadb"]).await;

        let asked = home.path().join("shutdown-was-asked-for");
        let stubborn = FakeService::new().ignoring_stop().exit_when(&asked);
        let shutdown = FakeService::new().touch(&asked);

        let declared = Declared(vec![
            spec("mariadb")
                .args(arguments(&stubborn))
                .stop(StopBehaviour::Command {
                    program: FakeService::program(),
                    args: arguments(&shutdown),
                    grace: Millis::from_secs(10),
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("mariadb")]).expect("a plan");
        let walk = registry.start(&graph, &plan).await;

        assert_eq!(walk.reached, vec![service("mariadb")], "{walk:?}");

        let stopping = graph.stop_plan([&service("mariadb")]).expect("a plan");
        let stopped = registry.stop(&stopping).await;

        assert_eq!(stopped.reached, vec![service("mariadb")], "{stopped:?}");
        assert!(
            asked.is_file(),
            "the stop command was never run: {} is not there",
            asked.display()
        );

        let (state, _) = row(&store, &service("mariadb")).await;
        assert_eq!(state, ServiceState::Stopped);

        let exit: Option<i64> =
            sqlx::query_scalar("SELECT last_exit_code FROM services WHERE id = ?")
                .bind("mariadb")
                .fetch_one(store.pool())
                .await
                .expect("the row");

        assert_eq!(
            exit,
            Some(0),
            "a service that left on its own inside the grace period exits cleanly; a killed one \
             records no code at all"
        );
    }

    /// **A stop command that reported failure does not throw away a service that stopped anyway** —
    /// `Runner::ended_meanwhile`, and the one thing a failed stop request must not lose.
    ///
    /// Running the command is a whole grace period's worth of time, and a server that took the
    /// instruction and exited inside that window has stopped exactly as it was asked to — whatever
    /// the program carrying the instruction then returned. `mariadb-admin shutdown` really does
    /// this: it can return non-zero for a server that has already gone by the time it looks again.
    /// Answering `None` there records no exit code at all and writes an ERROR about a kill that
    /// never happened, on a database that shut down cleanly.
    ///
    /// The fixture stages the window rather than hoping for it. The stop command creates the file
    /// first and only exits `3` four hundred milliseconds later, so the service — which polls for
    /// that file every twenty-five — is reliably gone before the failure is reported. The row is the
    /// assertion: `Some(0)` is a service that left on its own, and `None` is what a kill records.
    #[tokio::test]
    async fn a_service_that_stopped_while_its_stop_command_failed_keeps_its_exit_code() {
        let (home, paths, store) = home(&["mariadb"]).await;

        let asked = home.path().join("shutdown-was-asked-for");
        let stubborn = FakeService::new().ignoring_stop().exit_when(&asked);

        // Asks properly and then reports failure, which is the whole point: the service is already
        // going by the time the runner is told the request did not work.
        let shutdown = FakeService::new()
            .touch(&asked)
            .exit_after(400)
            .exit_code(3);

        let declared = Declared(vec![
            spec("mariadb")
                .args(arguments(&stubborn))
                .stop(StopBehaviour::Command {
                    program: FakeService::program(),
                    args: arguments(&shutdown),
                    grace: Millis::from_secs(10),
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("mariadb")]).expect("a plan");

        assert_eq!(
            registry.start(&graph, &plan).await.reached,
            vec![service("mariadb")]
        );

        let stopping = graph.stop_plan([&service("mariadb")]).expect("a plan");
        let stopped = registry.stop(&stopping).await;

        assert_eq!(stopped.reached, vec![service("mariadb")], "{stopped:?}");
        assert!(
            asked.is_file(),
            "the stop command was never run: {} is not there",
            asked.display()
        );

        let (state, _) = row(&store, &service("mariadb")).await;
        assert_eq!(state, ServiceState::Stopped);

        let exit: Option<i64> =
            sqlx::query_scalar("SELECT last_exit_code FROM services WHERE id = ?")
                .bind("mariadb")
                .fetch_one(store.pool())
                .await
                .expect("the row");

        assert_eq!(
            exit,
            Some(0),
            "the service shut down cleanly while its stop command was still running, and the \
             command's own non-zero status threw that exit code away"
        );
    }

    /// **A stop command that cannot be run does not become a service that cannot be stopped.**
    ///
    /// The failure is loud in `daemon.log` — for a database it means a recovery on its next start —
    /// but the service still has to go, because the alternative is a port and a data directory held
    /// by a process nobody is supervising. Staged with a program that is not there, which is the
    /// ordinary shape of it: a spec written against a package that has since been uninstalled.
    #[tokio::test]
    async fn a_stop_command_that_cannot_be_run_still_leaves_the_service_stopped() {
        let (_home, paths, store) = home(&["mariadb"]).await;

        let stubborn = FakeService::new().ignoring_stop();
        let declared = Declared(vec![
            spec("mariadb")
                .args(arguments(&stubborn))
                .stop(StopBehaviour::Command {
                    program: FakeService::program().with_file_name("mixengine-no-such-program"),
                    args: Vec::new(),
                    grace: Millis::from_secs(10),
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("mariadb")]).expect("a plan");

        assert_eq!(
            registry.start(&graph, &plan).await.reached,
            vec![service("mariadb")]
        );

        let stopping = graph.stop_plan([&service("mariadb")]).expect("a plan");
        let stopped = registry.stop(&stopping).await;

        assert_eq!(stopped.reached, vec![service("mariadb")], "{stopped:?}");

        let (state, pid) = row(&store, &service("mariadb")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(pid, None, "a stopped service names no process");
    }

    /// **A shutdown's stop is not the stop a person makes** — roadmap task **T123**.
    ///
    /// `daemon.shutdown` and `service.stop` walk this same function, so the row is the only place
    /// the difference can be made — and on-demand activation is what reads it at the next boot to
    /// decide whether it may wake anything at all. One test for both stops, because what is under
    /// test is precisely that they come out different.
    #[tokio::test]
    async fn a_shutdown_and_a_person_do_not_leave_the_same_stop_behind() {
        let (_home, paths, store) = home(&["caddy"]).await;

        let fake = FakeService::new();
        let declared = Declared(vec![
            spec("caddy")
                .args(arguments(&fake))
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let up = graph.start_plan([&service("caddy")]).expect("a plan");
        let down = graph.stop_plan([&service("caddy")]).expect("a plan");

        /// What the row says who left this service stopped.
        async fn who(store: &Store, id: &ServiceId) -> mixengine_core::services::StoppedBy {
            services::record(store, id)
                .await
                .expect("the row")
                .stopped_by
        }

        assert!(registry.start(&graph, &up).await.failed.is_none());
        assert!(registry.stop(&down).await.failed.is_none());

        assert_eq!(
            who(&store, &service("caddy")).await,
            mixengine_core::services::StoppedBy::Person,
            "`service.stop` is somebody asking, and nothing may undo it"
        );

        assert!(registry.start(&graph, &up).await.failed.is_none());
        assert!(
            registry
                .stop_because(&down, &StateReason::Shutdown)
                .await
                .failed
                .is_none()
        );

        assert_eq!(
            who(&store, &service("caddy")).await,
            mixengine_core::services::StoppedBy::Daemon,
            "a machine going down was recorded as a decision its owner made"
        );
    }

    /// **A stop that did not take the service down does not report that it did**, which since T18
    /// is a thing that can happen: a survivor this daemon adopted and could not kill keeps its row
    /// in a supervised state on purpose — `Runner::stop_adopted` — because writing `stopped` for a
    /// process still holding the port is the lie crash recovery exists to prevent. The runner's task
    /// ends either way, so a walk that read *that* would report the stop nobody got.
    ///
    /// Arranged through the row rather than through a process that refuses to die, which is not
    /// something a test can stage on three operating systems. It is the row `stop_one` answers from,
    /// and a supervised one with nothing running it is exactly what a refused stop leaves behind —
    /// as well as being what the *second* `mix service stop` after a refused first one meets.
    #[tokio::test]
    async fn a_stop_that_left_the_service_supervised_is_not_reported_as_reached() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        // Through the machine rather than into the column, so that what this leaves is a row the
        // daemon could really have written.
        for state in [
            ServiceState::Starting,
            ServiceState::Running,
            ServiceState::Stopping,
        ] {
            services::transition(
                &store,
                &service("caddy"),
                state,
                StateReason::Requested,
                now(),
            )
            .await
            .expect("a row can be moved to where a refused stop leaves it");
        }

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.stop_plan([&service("caddy")]).expect("a plan");

        let walk = registry.stop(&plan).await;

        assert!(
            walk.reached.is_empty(),
            "a service still claiming a supervisor has not been stopped: {walk:?}"
        );
        assert!(
            matches!(&walk.failed, Some((id, fault)) if id == &service("caddy") && fault.reason.is_none()),
            "the walk names the service it could not take down, with nothing to quote as a \
             reason: {walk:?}"
        );
    }

    /// **Two stops of one service are one stop with two callers**, and the second of them is
    /// answered by the stop that actually happened rather than by a race it lost.
    ///
    /// The shape it arrives in is ordinary: a `mix daemon stop` beside a `mix service stop`, or a
    /// GUI button pressed twice. The caller that got there second used to find the entry already
    /// taken out of the map, skip the wait the answer depends on, and read a row mid-stop — which
    /// says `Running` or `Stopping`, both of which `is_supervised` counts. So a service that was
    /// shutting down perfectly was reported as one that would not stop, `mix daemon stop` exited
    /// non-zero, and somebody went looking for a database that was fine.
    ///
    /// **The stop is staged to take time, or there is no second caller to be wrong.** A stop command
    /// really runs on all three systems — a `Signal` spec spends no grace on Windows at all
    /// (ADR 0008), so the window would close there, on one of the two systems this race is easiest
    /// to hit. Seven hundred milliseconds is far longer than the two database reads the second walk
    /// needs to reach its wrong answer, and far shorter than the grace the spec allows.
    #[tokio::test]
    async fn two_stops_of_one_service_are_both_answered_by_the_stop_that_happened() {
        let (home, paths, store) = home(&["mariadb"]).await;

        let asked = home.path().join("shutdown-was-asked-for");
        let stubborn = FakeService::new().ignoring_stop().exit_when(&asked);

        // Asks properly and stays there, which is what holds the stop open: the file is created as
        // the command starts and the command itself only ends seven hundred milliseconds later.
        let shutdown = FakeService::new().touch(&asked).exit_after(700);

        let declared = Declared(vec![
            spec("mariadb")
                .args(arguments(&stubborn))
                .stop(StopBehaviour::Command {
                    program: FakeService::program(),
                    args: arguments(&shutdown),
                    grace: Millis::from_secs(10),
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("mariadb")]).expect("a plan");

        assert_eq!(
            registry.start(&graph, &plan).await.reached,
            vec![service("mariadb")]
        );

        let stopping = graph.stop_plan([&service("mariadb")]).expect("a plan");

        let (first, second) = tokio::time::timeout(EVENTUALLY, async {
            tokio::join!(registry.stop(&stopping), registry.stop(&stopping))
        })
        .await
        .expect("both stops were answered");

        assert_eq!(first.reached, vec![service("mariadb")], "{first:?}");
        assert!(first.failed.is_none(), "{first:?}");
        assert_eq!(
            second.reached,
            vec![service("mariadb")],
            "the second caller was answered by the stop the first one was performing: {second:?}"
        );
        assert!(
            second.failed.is_none(),
            "a service that stopped exactly as it was asked to was reported as one that would \
             not: {second:?}"
        );

        let (state, pid) = row(&store, &service("mariadb")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(pid, None, "a stopped service names no process");
    }
}
