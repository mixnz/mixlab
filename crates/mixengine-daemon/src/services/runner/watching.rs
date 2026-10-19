//! Watching a running service: for the daemon asking it to stop, for it ending, for it going
//! sick, and for new limits or a reload to hand it.

use std::ops::ControlFlow;

use mixengine_platform::process::{Exit, Supervised};
use mixengine_proto::{ResourceLimits, ServiceState};
use mixengine_supervisor::logs::Capture;
use mixengine_supervisor::{Health, Restarts, Surroundings, Verdict};
use tokio::time::Instant;

use super::{After, Readiness, Runner, WATCH, fold, worth_recording};

impl Runner {
    /// Watch a running service: for the daemon asking it to stop, for it ending, for it going sick.
    ///
    /// One timer rather than two, and it is the cheaper one that sets the pace: the health check's
    /// interval is measured in seconds and the liveness poll in milliseconds, so the loop wakes on
    /// whichever is next and asks only the question that is due.
    #[warn(clippy::cognitive_complexity)]
    pub(super) async fn supervise(
        &mut self,
        mut supervised: Supervised,
        capture: Capture,
        restarts: &mut Restarts,
    ) -> After {
        let mut watch = Watch::starting(self.spec.health());

        // Taken once, before the loop: the borrow checker's reason is that the loop reaches `&mut
        // self`, and the better one is that a probe every ten seconds must not be a keyring read
        // every ten seconds. Only `HealthProbe::Command` reads it.
        let place = self.where_commands_run().await;

        loop {
            match self.woken(watch.due).await {
                Woken::Cancelled => return self.stop(supervised, capture).await,

                Woken::Reload => {
                    self.reload(&place, &supervised).await;

                    continue;
                }

                Woken::Limits(asked) => {
                    self.apply_limits(&supervised, asked);

                    continue;
                }

                Woken::Tick => {}
            }

            if let Some(exit) = self.exit_if_ended(&mut supervised) {
                return self
                    .ended_while_watched(supervised, capture, &exit, restarts)
                    .await;
            }

            self.fold_into_state(&mut watch).await;

            if self
                .probe_if_due(&mut watch, &place, restarts)
                .await
                .is_break()
            {
                return self.stop(supervised, capture).await;
            }
        }
    }

    /// Wait until something the watch loop has to act on happens.
    #[warn(clippy::cognitive_complexity)]
    async fn woken(&mut self, due: Option<Instant>) -> Woken {
        let watch = Instant::now() + WATCH;
        let wake = due.map_or(watch, |due| due.min(watch));

        tokio::select! {
            // Biased towards the stop: a service the daemon has been asked to shut down should not
            // spend one more health interval being probed.
            biased;

            () = self.cancel.cancelled() => Woken::Cancelled,

            // Ahead of the timer and behind the stop. A configuration that has already been written
            // is one the next liveness poll can wait for, and a daemon on its way out should not
            // spend a reload's patience on a service it is about to stop.
            () = self.asked_to_reload.notified() => Woken::Reload,

            // Beside the reload and for its reason: something has been written down and the running
            // process has not been told yet. Cheaper than a reload — no patience, no signal, no
            // command — because both mechanisms accept a rewrite with processes already inside
            // them.
            Ok(()) = self.limits_asked.changed() => Woken::Limits(*self.limits_asked.borrow_and_update()),

            // **Nothing is done here, deliberately.** This arm exists to wake the loop; what to do
            // about the new value is the same decision a health probe reaches, and it is made once,
            // from both inputs, by `fold_into_state`.
            Ok(()) = self.over_memory.changed() => Woken::Tick,

            () = tokio::time::sleep_until(wake) => Woken::Tick,
        }
    }

    /// Tell the running process about limits that were written down after it started.
    #[warn(clippy::cognitive_complexity)]
    fn apply_limits(&self, supervised: &Supervised, asked: ResourceLimits) {
        if let Err(error) = supervised.set_limits(&super::super::limits::from_proto(asked)) {
            // **Logged, and the service is left running.** A mechanism that refused a value is not
            // a service that has stopped working, and a daemon that killed a healthy database
            // because a cap could not be written would be doing more damage than the uncapped
            // service ever could.
            tracing::warn!(
                service = self.spec.id().as_str(),
                error = %error,
                "cannot apply this service's new limits to the running process"
            );
        }
    }

    /// How the process ended, if it has.
    #[warn(clippy::cognitive_complexity)]
    fn exit_if_ended(&self, supervised: &mut Supervised) -> Option<Exit> {
        match supervised.exited() {
            Ok(exit) => exit,

            // The OS will not say. Nothing to decide on, and the next tick asks again.
            Err(error) => {
                tracing::warn!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "cannot tell whether this service is still running"
                );

                None
            }
        }
    }

    /// The process ended while it was being watched: let the restart policy decide.
    #[warn(clippy::cognitive_complexity)]
    async fn ended_while_watched(
        &self,
        supervised: Supervised,
        capture: Capture,
        exit: &Exit,
        restarts: &mut Restarts,
    ) -> After {
        // Said before the kill rather than after, and this is the only place a readiness is
        // published without a row behind it. Between a process ending and `after_exit` persisting
        // what that meant lie a drain bounded by `FLUSH` and a write, and a walk arriving inside
        // that window would otherwise read the `Up` this service stopped being — and start the tier
        // below against a database that has gone. `Deciding` and not `Down` because what happens
        // next is the restart policy's to say: whoever is waiting waits a moment longer for the
        // answer rather than being handed a failure that a restart is about to contradict.
        self.readiness.send_replace(Readiness::Deciding);

        let capture = self.kill(supervised, capture).await;
        let decision = restarts.ended(exit, std::time::Instant::now(), &capture);

        // No port diagnosis here, and the difference from a failed *start* is the whole reason:
        // this process had the port. Whatever ended it, nobody took it away — and if something
        // claims it before the restart, that start is where it will be met and named.
        self.after_exit(decision, exit.code(), None).await
    }

    /// Fold health and size into one state, and record it if it changed.
    ///
    /// **Folded before the health guard in [`Runner::probe_if_due`], not inside it.** A service
    /// whose recipe declares no `HealthCheck` never reaches that guard, and a fold living there
    /// would watch such a service's ceiling and never say a word about it.
    #[warn(clippy::cognitive_complexity)]
    async fn fold_into_state(&mut self, watch: &mut Watch) {
        let over = *self.over_memory.borrow_and_update();
        let folded = fold(watch.healthy, over);

        if worth_recording(watch.state, folded.0) && self.move_to(folded.0, folded.1).await {
            watch.state = folded.0;
        }
    }

    /// Make the health probe if one is due. `Break` if the daemon asked the service to stop
    /// meanwhile.
    #[warn(clippy::cognitive_complexity)]
    async fn probe_if_due(
        &self,
        watch: &mut Watch,
        place: &Surroundings,
        restarts: &mut Restarts,
    ) -> ControlFlow<()> {
        let Some(watching) = watch.health.as_mut() else {
            return ControlFlow::Continue(());
        };

        if watch.due.is_some_and(|due| Instant::now() < due) {
            return ControlFlow::Continue(());
        }

        // Raced against the stop for the same reason the wait in `woken` is: the probe is a program
        // the spec named or an HTTP request, and both are bounded by `HealthCheck::timeout` rather
        // than by anything short. A `mariadb-admin ping` against a database that has stopped
        // answering hangs for the whole of it, and awaiting that outright would make a `mix stop`
        // arriving mid-probe wait out a deadline set for judging health, not for shutting down.
        // Dropping the future is the cancellation: `run_once` kills the child it spawned, and the
        // fold this verdict would have gone into belongs to a loop that is ending anyway.
        let examined = tokio::select! {
            biased;

            () = self.cancel.cancelled() => return ControlFlow::Break(()),

            examined = watching.examine(place) => examined,
        };

        let interval = watching.interval();

        match probe_action(&examined, watch.complained) {
            // **Recorded, not applied.** The verdict changes what this loop believes about the
            // service's health; what state that adds up to is the fold's, at the top of the next
            // turn, because size has a say in it too.
            ProbeAction::Degraded => watch.healthy = false,

            ProbeAction::Recovered => {
                // The backoff, not the failure history: a service that recovers between crashes is
                // still crashing, and `Restarts` is what remembers that.
                restarts.recovered();
                watch.healthy = true;
            }

            ProbeAction::Nothing => {}

            ProbeAction::RetryLater { say } => {
                if say && let Err(error) = &examined {
                    tracing::warn!(
                        service = self.spec.id().as_str(),
                        error = %error,
                        "this service's health probe could not be made; it will be tried again at \
                         the next interval"
                    );
                }

                watch.complained = true;
                watch.due = Some(Instant::now() + interval);

                return ControlFlow::Continue(());
            }

            ProbeAction::StopProbing => {
                if let Err(error) = &examined {
                    tracing::warn!(
                        service = self.spec.id().as_str(),
                        error = %error,
                        "this service cannot be health-checked here; it will be watched for \
                         exiting and nothing else"
                    );
                }

                watch.health = None;
                watch.due = None;

                return ControlFlow::Continue(());
            }
        }

        // Cleared by any probe that was actually made, so a fault that comes back after an hour of
        // working is reported again rather than swallowed by the first one.
        watch.complained = false;
        watch.due = Some(Instant::now() + interval);

        ControlFlow::Continue(())
    }
}

/// What the watch loop knows about a running service between two wakes.
struct Watch {
    /// The health check, or `None` if the spec has none or it cannot be made here.
    health: Option<Health>,

    /// When the next probe is due.
    due: Option<Instant>,

    /// Whether the current run of unmakeable probes has already been reported: a transient fault
    /// is worth one line, not one line every interval.
    complained: bool,

    /// **One of the two halves of the state this loop owns.** It starts true because a service
    /// with no health check is one nothing has ever said is ill.
    healthy: bool,

    /// The other half. It starts `Running` because `supervise` is entered from the
    /// `Running`/`Ready` transition.
    state: ServiceState,
}

impl Watch {
    fn starting(check: Option<&mixengine_proto::HealthCheck>) -> Self {
        let health = check.map(Health::watching);
        let due = health
            .as_ref()
            .map(|health| Instant::now() + health.interval());

        Self {
            health,
            due,
            complained: false,
            healthy: true,
            state: ServiceState::Running,
        }
    }
}

/// What woke the watch loop.
enum Woken {
    /// The daemon asked the service to stop.
    Cancelled,

    /// The service's configuration was rewritten and it is to be reloaded.
    Reload,

    /// New limits were written down for it.
    Limits(ResourceLimits),

    /// Time to look at it again: the liveness poll, a probe coming due, or a new memory verdict.
    Tick,
}

/// What one health probe's answer does to the watch loop.
#[derive(Debug, PartialEq, Eq)]
enum ProbeAction {
    /// The service is unhealthy.
    Degraded,

    /// The service is healthy again.
    Recovered,

    /// Nothing changed.
    Nothing,

    /// The probe could not be made this time; try at the next interval, and `say` so only the
    /// first time in a row.
    RetryLater { say: bool },

    /// The probe can never be made here; leave the service alone and stop probing it.
    StopProbing,
}

/// What a probe's answer means, given whether a failure to make one was already reported.
///
/// A probe that could not be made *this time* — the binary was being replaced by an upgrade, the
/// machine was out of process slots — is **not a verdict about the service**: nothing was measured,
/// so degrading it would report a bad moment as a sick database. Nor is it a reason to stop asking,
/// because the next interval is entitled to a different answer. It is said once and then only
/// counted, so a fault that lasts an hour is one line in `daemon.log` rather than three hundred.
///
/// A probe this build or this machine cannot make leaves **the service alone**, deliberately: it
/// is running and its readiness was proved, and degrading it for a check nobody can make would
/// report a fault in the spec as a fault in the service. Said once, because the answer will not
/// change, and then never probed again.
#[warn(clippy::cognitive_complexity)]
fn probe_action(
    examined: &Result<Option<Verdict>, mixengine_supervisor::Error>,
    complained: bool,
) -> ProbeAction {
    match examined {
        Ok(Some(Verdict::Degraded)) => ProbeAction::Degraded,
        Ok(Some(Verdict::Recovered)) => ProbeAction::Recovered,
        Ok(None) => ProbeAction::Nothing,
        Err(error) if error.might_work_later() => ProbeAction::RetryLater { say: !complained },
        Err(_) => ProbeAction::StopProbing,
    }
}

#[cfg(test)]
mod decisions;
