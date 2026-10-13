//! What the last daemon left behind — roadmap task **T18**, given a file by **T207d**.
//!
//! Adopting the processes that survived it, stopping the ones nothing can supervise, and clearing
//! the rows whose process is gone. The state it reads and writes is the [`Registry`]'s.

use mixengine_core::services::{self, ServiceGraph};
use mixengine_platform::process::{Adopted, StartTime};
use mixengine_proto::{ServiceId, ServiceState, StateReason};

use super::runner::gone;
use super::{Registry, Undeclarable, lock, record};

/// What one boot's reconciliation found — roadmap task **T18**.
///
/// Lists rather than counts, because the one caller that is not a test writes them into
/// `daemon.log`: "adopted mariadb@main" is the line somebody reads a week later to understand why a
/// database had been up for longer than the daemon watching it, and a number would not answer that.
///
/// **Every row this touched is in exactly one of them**, including the ones it could not finish
/// with. A reconciliation that reported only its successes would let the summary line say nothing
/// happened in the very boot where a survivor refused to die — which is the boot somebody is reading
/// the log for.
#[derive(Debug, Default)]
pub(crate) struct Recovery {
    /// Services whose process survived and is now supervised again.
    pub(crate) adopted: Vec<ServiceId>,

    /// Survivors this daemon stopped rather than adopt: nothing declares them, or they were left in
    /// a state adoption cannot resume.
    pub(crate) stopped: Vec<ServiceId>,

    /// Rows that claimed a process which was not there. Nothing was signalled for these.
    pub(crate) cleared: Vec<ServiceId>,

    /// Survivors this daemon meant to stop and could not, whose rows are therefore **left as they
    /// were found** — still naming the process, still in a supervised state.
    ///
    /// Not a failure of the boot: the daemon serves clients either way, and the next one meets the
    /// same case and tries again. It is here because it is the one outcome that leaves the machine
    /// holding a port nothing supervises, and it must not be reported as quiet.
    pub(crate) refused: Vec<ServiceId>,
}

impl Recovery {
    /// Whether there was anything to reconcile at all, which is the ordinary case.
    pub(crate) fn is_empty(&self) -> bool {
        self.adopted.is_empty()
            && self.stopped.is_empty()
            && self.cleared.is_empty()
            && self.refused.is_empty()
    }
}

impl Registry {
    /// Reconcile what the daemon before this one left behind — roadmap task **T18**.
    ///
    /// Called once, before anything is served, and it is the only thing in this registry that reads
    /// a `services` row it did not write. A daemon that was killed — or a machine that lost power —
    /// leaves rows claiming a supervisor that no longer exists, and every one of them is one of two
    /// things:
    ///
    /// - **a survivor**: the pid is still there *and* the process bearing it began at the moment
    ///   that was recorded. It is adopted, and from here on it is supervised like anything else. The
    ///   pair is the whole check, because a pid on its own is reused within minutes and signalling
    ///   somebody else's program is the one accident this product cannot have.
    /// - **gone**: nothing has that pid, or what does is a different process. The row is cleared and
    ///   the service is recorded as `stopped` with [`StateReason::Vanished`]. **Nothing is
    ///   signalled**, which is the point of failing the identity check rather than trusting the
    ///   number.
    ///
    /// A survivor is not always adoptable, and the third outcome is the one that stops it: a service
    /// nothing declares any more cannot be supervised at all, and one whose row says `starting`,
    /// `stopping` or `restarting` cannot be resumed from where it was — a readiness that was never
    /// decided cannot be re-decided without the pipes that went with the old daemon. Leaving either
    /// running would leave the port and the data directory held by a process the next start collides
    /// with, so they are stopped, and the reason says which it was.
    ///
    /// A survivor that will not go is the fourth outcome and the only one that leaves work undone:
    /// its row is left exactly as it was found, and it is reported in [`Recovery::refused`] so the
    /// boot does not summarise itself as quiet. The next daemon meets the same row and tries again.
    ///
    /// **A stopped survivor is killed rather than asked**, unlike every other stop in this crate.
    /// A boot is not the moment to spend a `StopBehaviour`'s grace period per service on processes
    /// this daemon has already decided it cannot supervise — the daemon would answer no client until
    /// the last of them had gone. For a database that means recovery on its next start, which is the
    /// same cost a stop command that fails carries — see [`Runner::ask_to_stop`][runner] — and it is
    /// stated in the row rather than hidden.
    ///
    /// Adoption writes **no transition** for the service it takes over: nothing happened to it. Its
    /// row said `running` before this process existed and says `running` still.
    ///
    /// [runner]: super::runner::Runner
    pub(crate) async fn recover(&self) -> Recovery {
        let mut recovery = Recovery::default();

        // **First, and before any row is read** — roadmap task **T68**. On Linux a supervised group
        // is a *directory*, so a daemon that was killed leaves one behind for every service it was
        // capping. Nothing on the other two systems, where a group is a kernel object that goes when
        // the last handle to it closes.
        //
        // Safe to run before adoption rather than after, and that is the point: an empty cgroup is
        // removable and a non-empty one is not, so this takes exactly the ones whose processes are
        // already gone and leaves the ones belonging to survivors this reconciliation is about to
        // adopt. It needs no list of what to expect because the kernel is the list.
        mixengine_platform::process::sweep_stale_groups();

        let records = match services::records(&self.store).await {
            Ok(records) => records,

            // Nothing can be reconciled and nothing is: the daemon carries on with an empty registry
            // rather than refusing to start, because a `services` table that cannot be read is a
            // problem every later request will report for itself.
            Err(error) => {
                tracing::error!(
                    %error,
                    "cannot read what the last daemon left behind; no service will be adopted"
                );

                return recovery;
            }
        };

        // Asked once for the whole reconciliation rather than per row. A source that cannot answer
        // is not a reason to leave survivors running — it only means none of them can be adopted,
        // which is what an empty graph then says for every one of them.
        let declared = match self.graph().await {
            Ok(graph) => Some(graph),

            Err(error) => {
                let reason: &dyn std::fmt::Display = match &error {
                    Undeclarable::Unavailable(why) => why,
                    Undeclarable::Invalid(why) => why,
                };

                tracing::error!(
                    %reason,
                    "cannot tell which services are declared; anything that outlived the last \
                     daemon will be stopped rather than adopted"
                );

                None
            }
        };

        for (stored, record) in records {
            // A row in `stopped` or `failed` with no process named is already telling the truth
            // about a machine with no daemon on it. Everything else is either claiming a supervisor
            // or naming a pid, and both are this function's business.
            if !record.state.is_supervised() && record.pid.is_none() {
                continue;
            }

            let service = match ServiceId::parse(&stored) {
                Ok(service) => service,

                Err(error) => {
                    tracing::error!(
                        service = stored,
                        %error,
                        "the services table holds an id this build cannot read; leaving the row \
                         alone"
                    );

                    continue;
                }
            };

            self.reconcile(&service, &record, declared.as_ref(), &mut recovery)
                .await;
        }

        recovery
    }

    /// Reconcile only the rows this registry holds no runner for — roadmap task **T47b**.
    ///
    /// **[`recover`](Self::recover) with the set narrowed, and narrowed for a reason that is not
    /// tidiness.** That function walks *every* row, which is right at a boot where nothing is
    /// supervised yet and wrong on a running daemon: it would adopt or stop services this process is
    /// already watching. The rows a live daemon may decide anything about are the ones it holds no
    /// runner for.
    ///
    /// The decision per row is [`reconcile`](Self::reconcile)'s and is not restated here — adopt when
    /// the pid *and* the recorded start time both match, stop what cannot be resumed or is no longer
    /// declared, and never signal on a pid alone.
    pub(crate) async fn reconcile_stranded(&self) -> Recovery {
        let mut recovery = Recovery::default();

        let records = match services::records(&self.store).await {
            Ok(records) => records,

            Err(error) => {
                tracing::error!(
                    %error,
                    "cannot read the services table; nothing stranded can be reconciled"
                );

                return recovery;
            }
        };

        let declared = match self.graph().await {
            Ok(graph) => Some(graph),

            Err(error) => {
                let reason: &dyn std::fmt::Display = match &error {
                    Undeclarable::Unavailable(why) => why,
                    Undeclarable::Invalid(why) => why,
                };

                tracing::error!(
                    %reason,
                    "cannot tell which services are declared; anything stranded will be stopped \
                     rather than adopted"
                );

                None
            }
        };

        let held = self.supervised();

        for (stored, record) in records {
            if !record.state.is_supervised() && record.pid.is_none() {
                continue;
            }

            let service = match ServiceId::parse(&stored) {
                Ok(service) => service,

                Err(error) => {
                    tracing::error!(
                        service = stored,
                        %error,
                        "the services table holds an id this build cannot read; leaving the row \
                         alone"
                    );

                    continue;
                }
            };

            // The whole of what makes this safe on a running daemon.
            if held.contains(&service) {
                continue;
            }

            self.reconcile(&service, &record, declared.as_ref(), &mut recovery)
                .await;
        }

        recovery
    }

    /// What to do about one row the last daemon left behind. See [`Registry::recover`].
    async fn reconcile(
        &self,
        service: &ServiceId,
        record: &services::ServiceRecord,
        declared: Option<&ServiceGraph>,
        recovery: &mut Recovery,
    ) {
        let survivor = match survivor(service, record) {
            Ok(survivor) => survivor,

            // The OS has the answer and would not give it, which is neither "it is there" nor "it is
            // gone". Treated as gone, because the alternative is a row left claiming a supervisor
            // for ever — and because the only thing this branch forgoes is *adopting*: nothing is
            // signalled on a pid whose identity was never confirmed.
            Err(error) => {
                tracing::warn!(
                    service = service.as_str(),
                    error = %error,
                    "cannot tell whether this service's process outlived the daemon that started \
                     it; treating it as gone"
                );

                None
            }
        };

        let Some(adopted) = survivor else {
            if self
                .discard(service, record, None, StateReason::Vanished)
                .await
            {
                recovery.cleared.push(service.clone());
            }

            return;
        };

        let spec = declared.and_then(|graph| graph.spec(service));

        // Only a service that was *up* can be taken over as it is. The mid-flight states have a
        // process and no way to resume what was being done to it: a `starting` service was never
        // proved ready and cannot be re-checked without the pipes that went with the old daemon, and
        // a `stopping` one is halfway through a stop somebody asked for.
        let resumable = matches!(record.state, ServiceState::Running | ServiceState::Degraded);

        let stopped = match (spec, resumable) {
            (Some(spec), true) => {
                self.supervise(
                    spec,
                    &mut lock(&self.running),
                    Some(adopted),
                    StateReason::Requested,
                );
                recovery.adopted.push(service.clone());

                return;
            }

            // **Two different sentences, because they are two different people's problem.** A
            // service nothing declares is one somebody removed and the answer is that its process
            // goes with it; a daemon that could not be told what is declared has stopped a service
            // that may be perfectly well declared, and a row saying "nothing declares this" would
            // send its owner looking for a declaration that is there. The log line `recover` writes
            // says the same thing once for the whole boot; this is what `mix service list` shows
            // for each service afterwards.
            (None, _) => {
                let reason = if declared.is_some() {
                    "nothing declares this service any more, so nothing could supervise the \
                     process it left behind"
                } else {
                    "this daemon could not read which services are declared, so it had nothing to \
                     supervise the process it left behind against"
                };

                self.discard(
                    service,
                    record,
                    Some(adopted),
                    StateReason::Unadopted {
                        reason: reason.to_owned(),
                    },
                )
                .await
            }

            (Some(_), false) => {
                self.discard(
                    service,
                    record,
                    Some(adopted),
                    StateReason::Unadopted {
                        reason: format!(
                            "the daemon supervising it went away while it was {}, which is not a \
                             state another daemon can take over",
                            record.state
                        ),
                    },
                )
                .await
            }
        };

        if stopped {
            recovery.stopped.push(service.clone());
        } else {
            recovery.refused.push(service.clone());
        }
    }

    /// Let go of a service the last daemon left behind: stop whatever survived, clear the pid it
    /// named, and record where that leaves it. `false` if the survivor would not go.
    ///
    /// **In that order, and the order is the whole of the guarantee**: a row is only cleared once the
    /// process it named is no longer running, so a daemon killed in the middle of this leaves the next
    /// one exactly the case it already knows how to handle. The corollary is that a survivor which
    /// will not go leaves its row untouched — still claiming the pid, still in the state it was found
    /// in — rather than being written down as stopped while it holds the port. That is the same rule
    /// [`Runner::stop_adopted`][runner] follows, and it is why this can report a failure at all.
    ///
    /// [`runner`]: super::runner::Runner
    /// [runner]: super::runner::Runner
    async fn discard(
        &self,
        service: &ServiceId,
        row: &services::ServiceRecord,
        survivor: Option<Adopted>,
        reason: StateReason,
    ) -> bool {
        if let Some(adopted) = survivor {
            tracing::info!(
                service = service.as_str(),
                pid = adopted.pid(),
                %reason,
                "stopping a process that outlived the daemon supervising it"
            );

            if let Err(error) = adopted.stop() {
                tracing::error!(
                    service = service.as_str(),
                    pid = adopted.pid(),
                    error = %error,
                    "cannot stop it; its row is left naming it, for the next daemon to try again"
                );

                return false;
            }

            if !gone(service, &adopted).await {
                tracing::error!(
                    service = service.as_str(),
                    pid = adopted.pid(),
                    "this process did not go when it was stopped; its row is left naming it, for \
                     the next daemon to try again"
                );

                return false;
            }
        }

        if row.pid.is_some()
            && let Err(error) = services::ended(&self.store, service, None).await
        {
            tracing::error!(
                service = service.as_str(),
                error = %error,
                "cannot clear the process this service's row names; the next daemon will meet the \
                 same pid again"
            );
        }

        if !row.state.is_supervised() {
            // A row that already says `stopped` or `failed` and merely held a stale pid. There is
            // no move to make and nothing to announce: it was where it says it is.
            return true;
        }

        // Through `Stopping`, which is the only edge into `Stopped` — and which the machine already
        // means: this is the last thing anybody did to the process. A row that was *already*
        // stopping is skipped rather than asked to move to where it is, which is not an event.
        if row.state != ServiceState::Stopping {
            record(
                &self.store,
                &self.events,
                service,
                ServiceState::Stopping,
                reason.clone(),
            )
            .await;
        }

        record(
            &self.store,
            &self.events,
            service,
            ServiceState::Stopped,
            reason,
        )
        .await;

        true
    }
}

/// The process this row names, if it is still the one the row was written about.
///
/// **Both halves or nothing.** A row with a pid and no start time is one this build wrote when the
/// OS would not say when the process began, and there is no way to tell now whether the number still
/// means what it meant — so it is treated as gone, which costs an adoption and never a wrong signal.
///
/// # Errors
///
/// Whatever [`Adopted::identify`] could not ask the OS. Not "there is no such process", which is
/// [`None`].
fn survivor(
    service: &ServiceId,
    row: &services::ServiceRecord,
) -> mixengine_platform::Result<Option<Adopted>> {
    let (Some(pid), Some(started)) = (row.pid, row.pid_start_time) else {
        if row.pid.is_some() {
            tracing::warn!(
                service = service.as_str(),
                "this service's row names a process and not when it began, so it cannot be \
                 identified; treating it as gone"
            );
        }

        return Ok(None);
    };

    Adopted::identify(pid, StartTime::from_stored(started))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use mixengine_core::Store;
    use mixengine_proto::{Backoff, DaemonEvent, Millis, RestartPolicy, StopBehaviour};
    use mixengine_testkit::FakeService;

    use super::super::fixture::{
        Declared, EVENTUALLY, SILENCE, Unavailable, arguments, gone, home, registry, row, service,
        spec,
    };
    use super::super::now;
    use super::*;

    /// A `fakeservice` running, and a row that says the *last* daemon started it — T18's subject.
    ///
    /// The row is written the way the runner writes one, through `core`, so what recovery meets is
    /// what a killed daemon really leaves: a supervised state, a pid, and the moment that process
    /// began. Nothing here is supervising it, which is the point.
    async fn left_running(
        store: &Store,
        id: &ServiceId,
        state: ServiceState,
    ) -> mixengine_testkit::service::Running {
        left_running_with(store, id, state, FakeService::new()).await
    }

    /// [`left_running`], for a test that needs the survivor to behave in a particular way.
    async fn left_running_with(
        store: &Store,
        id: &ServiceId,
        state: ServiceState,
        fake: FakeService,
    ) -> mixengine_testkit::service::Running {
        let service = fake.spawn();

        // Waited for, because a spawn returns before the process has parsed its arguments — and a
        // start time read in that window would be right for the wrong reason.
        assert!(
            service.wait_for_stdout(mixengine_testkit::service::READY_LINE, EVENTUALLY),
            "the survivor did not start"
        );

        let pid = service.id();
        let started = mixengine_platform::process::started_at(pid)
            .expect("this system can be asked when a process began")
            .expect("the survivor is running");

        services::transition(
            store,
            id,
            ServiceState::Starting,
            StateReason::Requested,
            now(),
        )
        .await
        .expect("a stopped service can start");

        if state != ServiceState::Starting {
            services::transition(store, id, state, StateReason::Ready, now())
                .await
                .expect("a starting service can reach the state this fixture asks for");
        }

        services::started(store, id, pid, Some(started.stored()), now())
            .await
            .expect("the row takes the process the last daemon started");

        service
    }

    /// **T18, and the case M1 is about.** A process that outlived the daemon which started it is
    /// supervised again by the one that finds it — not restarted, not left running with nothing
    /// watching it, and above all not reported as something it is not.
    ///
    /// The survivor here is this test's own child rather than one a daemon left behind, which is the
    /// only way to produce one on Windows at all: a supervised child there dies with its daemon by
    /// kernel guarantee (ADR 0007), so the process that reaches a real `recover` is the one from the
    /// window that ADR accepts. Nothing in the code under test can tell the difference — it is
    /// handed a row, and it asks the OS about the pid in it.
    #[tokio::test]
    async fn a_service_that_outlived_the_last_daemon_is_supervised_again() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let mut survivor = left_running(&store, &service("caddy"), ServiceState::Running).await;

        // Subscribed before the reconciliation, because half of what is asserted here is a silence:
        // nothing happened to this service, so nothing may be announced about it.
        let mut watching = registry.events.subscribe();

        let recovered = registry.recover().await;

        assert_eq!(recovered.adopted, vec![service("caddy")], "{recovered:?}");
        assert!(recovered.stopped.is_empty(), "{recovered:?}");
        assert!(recovered.cleared.is_empty(), "{recovered:?}");

        assert!(
            registry.supervised().contains(&service("caddy")),
            "the adopted service has nothing supervising it"
        );
        assert_eq!(
            row(&store, &service("caddy")).await.0,
            ServiceState::Running,
            "an adopted service is where it was; adoption is not a start"
        );
        assert!(
            survivor.still_running(),
            "the process was stopped by the daemon that was supposed to take it over"
        );

        let announced = tokio::time::timeout(SILENCE, watching.next()).await;
        assert!(
            announced.is_err(),
            "adopting a service announced a state change, so a client was told a service somebody \
             has been using all along had just moved: {announced:?}"
        );

        // And the other half of being supervised again: a stop reaches it. This is the assertion
        // that separates adoption from a registry entry that merely looks right.
        let graph = registry.graph().await.expect("one declared service");
        let stopping = graph.stop_plan([&service("caddy")]).expect("a plan");
        tokio::time::timeout(EVENTUALLY, registry.stop(&stopping))
            .await
            .expect("the adopted service was stopped");

        let (state, pid) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(pid, None);
        assert!(
            !survivor.still_running(),
            "the adopted process outlived the stop of the service it belongs to"
        );
    }

    /// **One credential the keyring cannot answer for does not take the rest of the environment
    /// with it.**
    ///
    /// The adopted path is the only one that resolves an environment at stop time — see
    /// `Runner::where_commands_run` — and it is the one where a keyring is most likely to refuse: a
    /// machine that has been rebooted since the daemon that started this process. Dropping the whole
    /// environment for that would run `mariadb-admin shutdown` without the `HOME` the spec declares
    /// outright, so the stop that was meant to survive a locked keyring would fail a second time for
    /// a reason nobody chose.
    ///
    /// The mock store holds no credential, which is the same answer a locked one eventually gives.
    #[tokio::test]
    async fn an_adopted_service_stops_with_the_environment_entries_that_did_resolve() {
        let (home, paths, store) = home(&["mariadb"]).await;

        let asked = home.path().join("shutdown-was-asked-for");
        let dumped = home.path().join("stop-command-environment");
        let shutdown = FakeService::new().dump_env(&dumped).touch(&asked);

        let declared = Declared(vec![
            spec("mariadb")
                .env("MIXENGINE_DECLARED", "outright")
                .env_from_keyring("MIXENGINE_SECRET", "mariadb", "root")
                .stop(StopBehaviour::Command {
                    program: FakeService::program(),
                    args: arguments(&shutdown),
                    grace: Millis::from_secs(10),
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        // Deaf to every signal, so the clean exit below is evidence the command ran and nothing
        // else.
        let mut survivor = left_running_with(
            &store,
            &service("mariadb"),
            ServiceState::Running,
            FakeService::new().ignoring_stop().exit_when(&asked),
        )
        .await;

        let recovered = registry.recover().await;
        assert_eq!(recovered.adopted, vec![service("mariadb")], "{recovered:?}");

        let graph = registry.graph().await.expect("one declared service");
        let stopping = graph.stop_plan([&service("mariadb")]).expect("a plan");
        let stopped = tokio::time::timeout(EVENTUALLY, registry.stop(&stopping))
            .await
            .expect("the adopted service was stopped");

        assert_eq!(stopped.reached, vec![service("mariadb")], "{stopped:?}");
        assert!(
            gone(&mut survivor).await,
            "the adopted process outlived the stop of the service it belongs to"
        );

        let environment =
            std::fs::read_to_string(&dumped).expect("the stop command recorded its environment");

        assert!(
            environment
                .lines()
                .any(|line| line == "MIXENGINE_DECLARED=outright"),
            "the entry the spec states outright was thrown away with the one the keyring refused, \
             so the stop command ran without the environment it needs:\n{environment}"
        );
        assert!(
            !environment.contains("MIXENGINE_SECRET"),
            "an entry that did not resolve was passed anyway, which for a credential means an \
             empty password:\n{environment}"
        );
    }

    /// The other half of M1: what did *not* survive is cleaned, and cleaning it signals nothing.
    ///
    /// **The pid in the row is this test process's own**, which is the strongest way to assert the
    /// second half: if the identity check were skipped — if a pid alone were taken for a service —
    /// this test would not fail, it would be killed. What makes the pair not match is a start time
    /// one tick from the real one, which is exactly what a recycled pid looks like from in here.
    #[tokio::test]
    async fn a_row_whose_process_did_not_survive_is_cleared_and_nothing_is_signalled() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let ours = std::process::id();
        let mistaken = mixengine_platform::process::started_at(ours)
            .expect("this system can be asked when a process began")
            .expect("this process is running")
            .stored()
            + 1;

        services::transition(
            &store,
            &service("caddy"),
            ServiceState::Starting,
            StateReason::Requested,
            now(),
        )
        .await
        .expect("a stopped service can start");
        services::transition(
            &store,
            &service("caddy"),
            ServiceState::Running,
            StateReason::Ready,
            now(),
        )
        .await
        .expect("a starting service can be running");
        services::started(&store, &service("caddy"), ours, Some(mistaken), now())
            .await
            .expect("the row takes a process");

        let mut watching = registry.events.subscribe();

        let recovered = registry.recover().await;

        assert_eq!(recovered.cleared, vec![service("caddy")], "{recovered:?}");
        assert!(recovered.adopted.is_empty(), "{recovered:?}");

        let (state, pid) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(
            pid, None,
            "a row that kept a pid would be adopted by the next daemon, and by then it is somebody \
             else's"
        );

        // Through `Stopping`, which is the only edge into `Stopped`, and with the reason a person
        // reading the service list needs to understand why a service they left running is not.
        let mut seen = Vec::new();
        while seen.len() < 2 {
            let frame = tokio::time::timeout(EVENTUALLY, watching.next())
                .await
                .expect("the stream is not silent")
                .expect("the stream is still open");

            if let crate::api::events::Frame::Event(DaemonEvent::ServiceStateChanged(change)) =
                frame
            {
                seen.push(change);
            }
        }

        assert_eq!(seen[0].to, ServiceState::Stopping);
        assert_eq!(seen[1].to, ServiceState::Stopped);
        assert_eq!(seen[1].reason, StateReason::Vanished);
    }

    /// A survivor nothing declares cannot be supervised, and is not left holding the port either.
    ///
    /// The state a service is left in is what a user is told, so it says which of the two things
    /// went wrong — the process was there, and this daemon had no declaration to run it against.
    #[tokio::test]
    async fn a_survivor_nothing_declares_any_more_is_stopped() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let registry = registry(&paths, &store, Arc::new(Declared(Vec::new())));

        let mut survivor = left_running(&store, &service("caddy"), ServiceState::Running).await;

        let recovered = registry.recover().await;

        assert_eq!(recovered.stopped, vec![service("caddy")], "{recovered:?}");
        assert!(recovered.adopted.is_empty(), "{recovered:?}");

        assert!(
            gone(&mut survivor).await,
            "a service nothing declares was left running with nothing supervising it, which is the \
             process the next start collides with"
        );

        let (state, pid) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(pid, None);
        assert!(
            registry.supervised().is_empty(),
            "nothing can be supervising a service that has no declaration"
        );
    }

    /// A daemon that cannot be *told* what is declared stops the same survivors — and must not tell
    /// their owner they were undeclared.
    ///
    /// The two look identical from inside the reconciliation and are opposite problems outside it:
    /// one service was removed and its process goes with it, the other is declared perfectly well by
    /// a source this daemon could not read. `mix service list` is where a person meets the
    /// difference, and a row saying "nothing declares this" would send them looking for a
    /// declaration that is sitting right there.
    #[tokio::test]
    async fn a_survivor_stopped_because_the_declarations_could_not_be_read_says_so() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let registry = registry(&paths, &store, Arc::new(Unavailable));

        let mut survivor = left_running(&store, &service("caddy"), ServiceState::Running).await;

        let mut watching = registry.events.subscribe();

        let recovered = registry.recover().await;

        assert_eq!(recovered.stopped, vec![service("caddy")], "{recovered:?}");
        assert!(
            gone(&mut survivor).await,
            "a survivor was left running by a daemon that could not tell whether it was declared"
        );

        let stopped = loop {
            let frame = tokio::time::timeout(EVENTUALLY, watching.next())
                .await
                .expect("the stream is not silent")
                .expect("the stream is still open");

            if let crate::api::events::Frame::Event(DaemonEvent::ServiceStateChanged(change)) =
                frame
                && change.to == ServiceState::Stopped
            {
                break change;
            }
        };

        let StateReason::Unadopted { reason } = &stopped.reason else {
            panic!("a survivor this daemon stopped is not {}", stopped.reason);
        };

        assert!(
            reason.contains("could not read which services are declared"),
            "the reason does not say why this daemon had nothing to supervise the process \
             against: {reason}"
        );
        assert!(
            !reason.contains("nothing declares"),
            "a service whose declarations could not be read was reported as undeclared: {reason}"
        );
    }

    /// A reconciliation that could not finish is not a quiet boot.
    ///
    /// The one outcome that leaves the machine as it found it — a survivor that would not go, whose
    /// row still names it — has to reach the summary `mixengined` writes, or the line a person opens
    /// `daemon.log` for says nothing happened in exactly the boot where something did. Asserted on
    /// the value rather than through a process, because a process that survives `SIGKILL` and
    /// `TerminateProcess` is not something a test can arrange on three operating systems.
    #[test]
    fn a_survivor_that_would_not_stop_is_not_reported_as_nothing_to_do() {
        let refused = Recovery {
            refused: vec![service("caddy")],
            ..Recovery::default()
        };

        assert!(!refused.is_empty(), "{refused:?}");
        assert!(Recovery::default().is_empty());
    }

    /// A process is not enough: a service left mid-start cannot be resumed, so it is stopped.
    ///
    /// Its readiness was never decided and cannot be decided now — the ready check most specs use
    /// matches a log pattern, and the pipes it would read went with the daemon that died. Adopting
    /// it as though it were up would route traffic to a service nothing ever proved was listening.
    #[tokio::test]
    async fn a_service_left_mid_start_is_stopped_rather_than_taken_for_ready() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let mut survivor = left_running(&store, &service("caddy"), ServiceState::Starting).await;

        let recovered = registry.recover().await;

        assert_eq!(recovered.stopped, vec![service("caddy")], "{recovered:?}");
        assert!(
            gone(&mut survivor).await,
            "a service that was still starting was adopted rather than stopped"
        );
        assert_eq!(
            row(&store, &service("caddy")).await.0,
            ServiceState::Stopped
        );
    }

    /// Adoption ends where an ordinary life begins: the moment the survivor exits, its policy has
    /// it, and what the policy starts is a child of *this* daemon — pipes, group and log capture
    /// restored.
    #[tokio::test]
    async fn an_adopted_service_that_ends_is_put_back_as_this_daemon_s_own() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![
            spec("caddy")
                .restart(RestartPolicy::Always {
                    backoff: Backoff {
                        initial: Millis(50),
                        max: Millis(50),
                        ..Backoff::default()
                    },
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        // A survivor that is going to end by itself shortly after being adopted.
        let mut survivor = left_running_with(
            &store,
            &service("caddy"),
            ServiceState::Running,
            FakeService::new().exit_after(750),
        )
        .await;
        let first = survivor.id();

        registry.recover().await;

        // The row naming a *different* process is the whole assertion: the exit was noticed, the
        // policy was asked, and what it started was spawned here rather than adopted.
        let deadline = tokio::time::Instant::now() + EVENTUALLY;
        loop {
            let (state, pid) = row(&store, &service("caddy")).await;

            if state == ServiceState::Running && pid.is_some_and(|pid| pid != i64::from(first)) {
                break;
            }

            assert!(
                tokio::time::Instant::now() < deadline,
                "the adopted service was not put back by its policy: it is {state} with pid {pid:?}"
            );

            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        assert!(
            !survivor.still_running(),
            "the process that was adopted is somehow still running"
        );

        registry.shutdown.cancel();
        tokio::time::timeout(EVENTUALLY, registry.shut_down())
            .await
            .expect("every runner finished");
    }
}
