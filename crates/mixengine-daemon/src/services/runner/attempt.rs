//! One life of a service's process: spawning it, relaying its output, waiting for it to be
//! ready, watching it while it runs, and deciding what its end means — a restart, a stop,
//! or giving up.

use std::ffi::OsString;
use std::sync::Arc;

use mixengine_core::services;
use mixengine_platform::process::{self, Supervised};
use mixengine_proto::{ServiceState, StateReason};
use mixengine_supervisor::logs::Capture;
use mixengine_supervisor::restart::TAIL_LINES;
use mixengine_supervisor::{Decision, Health, Restarts, Surroundings, Verdict, ready};
use tokio::sync::broadcast;
use tokio::time::Instant;

use super::super::now;
use super::{After, Readiness, Runner, WATCH, fold, worth_recording};

impl Runner {
    /// One life of the process: spawn it, wait for readiness, then watch it until it ends.
    /// Put everything this capture reads onto the log clients are connected to — roadmap task
    /// **T16b**.
    ///
    /// **A relay and not a fourth sink inside the capture**, deliberately. The reader threads run
    /// outside the runtime and their one obligation is to drain a pipe the service blocks on; making
    /// them also reach into a daemon-side structure would put the daemon's locks on the path of
    /// every line a service prints, where a moment's contention stalls the process itself. What this
    /// task does instead costs the reader threads nothing — the send they already make has one more
    /// subscriber — and everything after that happens on the runtime, where being slow is only slow.
    ///
    /// **It ends on its own**, when the last sender inside the capture is dropped: that is the
    /// capture going away with this run of the process, or the last reader thread reaching end of
    /// file. Neither needs the runner to remember it, which is what keeps this out of every stop
    /// path — a restart makes a new capture and a new relay, and the log they both write into is the
    /// same one the client has been reading since before either existed.
    ///
    /// A relay that falls behind is a gap in the client's stream and says so. It should not happen:
    /// this loop does nothing but move a line from one channel to another, so falling behind means
    /// the runtime itself is starved — but a hole nobody mentions is exactly what
    /// [`LogFrame::Gap`](mixengine_proto::LogFrame::Gap) exists to prevent, and the alternative is a
    /// log panel quietly missing the lines that explain a failure.
    ///
    /// **What the capture already holds is taken here, and not inside the task** — roadmap task
    /// **T16c**. `Capture::start` puts the reader threads on the pipes before it returns, and the
    /// task below is spawned rather than run, so a service can print before the runtime first polls
    /// it. A subscription delivers nothing that was sent before it existed, so every line in that
    /// window used to reach `current.log` and never the ring — permanently, not until something
    /// caught up, and those are a start's first lines. [`Capture::read`] hands over the tail and the
    /// subscription under one lock, so what is taken here is complete and holds no line twice; both
    /// are moved into the task, which records the tail before it pumps anything.
    fn relay(&self, capture: &Capture) {
        let (already_said, mut lines) = capture.read();
        let log = Arc::clone(&self.log);
        let service = self.spec.id().clone();

        tokio::spawn(async move {
            for line in already_said {
                log.record(line);
            }

            loop {
                match lines.recv().await {
                    Ok(line) => log.record(line),

                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::warn!(
                            service = service.as_str(),
                            missed,
                            "this daemon fell behind a service's output; the lines it lost are \
                             reported as a gap to everything reading its log"
                        );

                        log.missed(missed);
                    }

                    // Every sender is gone: this run of the process is over and its pipes have been
                    // read to the end.
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    pub(super) async fn attempt(&mut self, restarts: &mut Restarts) -> After {
        let env = match self.environment().await {
            Ok(Some(env)) => env,

            // Asked to stop while the keyring was still being read. Nothing was started, so this
            // goes straight through `Stopping`, as a stop during a backoff does.
            Ok(None) => {
                self.stopped_before_running().await;

                return After::Done;
            }

            // A credential the spec names and the keyring does not hold, or one it will not answer
            // for inside `START_ENVIRONMENT`. The process was never started, which is exactly what
            // `SpawnFailed` says. The event stays the reason alone; the sentence naming the entry —
            // its address, never its value — goes on the row (T200b, D5), which is what lets this
            // machine's own MixLab and `mix` say why instead of `daemon.log` alone.
            Err(error) => {
                tracing::error!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "cannot resolve the environment this service is to be started with"
                );

                return self
                    .give_up(StateReason::SpawnFailed, Some(format!("{error:#}")))
                    .await;
            }
        };

        let args: Vec<OsString> = self.spec.args().iter().map(OsString::from).collect();

        let mut supervised = match process::spawn_supervised(
            self.spec.program(),
            &args,
            self.spec.cwd(),
            &env,
            &super::super::limits::from_proto(self.spec.limits()),
        ) {
            Ok(supervised) => supervised,

            Err(error) => {
                tracing::error!(
                    service = self.spec.id().as_str(),
                    program = %self.spec.program().display(),
                    error = %error,
                    "cannot start this service"
                );

                return self
                    .give_up(StateReason::SpawnFailed, Some(format!("{error:#}")))
                    .await;
            }
        };

        // Kept for the life of this process rather than rebuilt: what a health probe and a shutdown
        // command need is the environment the service is *running* with, and resolving it again
        // would be an OS keyring read every ten seconds for as long as the service is up.
        self.surroundings = Some(Surroundings::new(self.spec.cwd(), env));

        // Cloned rather than borrowed out of `self`, because the wait below also holds `&mut
        // supervised` and the two would be a borrow of `self` and a borrow inside it at once. Two
        // owned fields, cloned once per start, against a ready check that may run for a minute.
        let place = self
            .surroundings
            .clone()
            .expect("the surroundings were just set");

        // Before anything waits on the process: a pipe nobody drains stops the service writing to
        // it, and a ready check that matches a log pattern has nothing to match against until this
        // exists.
        let capture = Capture::start(
            &mut supervised,
            self.spec.id().as_str(),
            self.spec.logs(),
            Some(&self.directory),
        );

        self.relay(&capture);

        // The three columns nothing wrote before T19, and the pair T18 adopts on. The start time is
        // read while the handle is still held, which is what makes it this child's: an unreaped
        // child keeps its pid reserved on Unix, and on Windows this process holds a handle to it, so
        // the number cannot have been given away in between. A reading that fails leaves the column
        // null rather than guessing — the process is supervised either way, and what a null costs is
        // only that a daemon restart will not adopt it.
        let started_at = match supervised.started_at() {
            Ok(started_at) => started_at,

            Err(error) => {
                tracing::warn!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "cannot read when this service's process began; a daemon restart will not adopt \
                     it"
                );

                None
            }
        };

        if let Err(error) = services::started(
            &self.store,
            self.spec.id(),
            supervised.pid(),
            started_at.map(process::StartTime::stored),
            now(),
        )
        .await
        {
            tracing::error!(
                service = self.spec.id().as_str(),
                error = %error,
                "cannot record the process this service is running as; it is supervised but a \
                 daemon restart will not adopt it"
            );
        }

        // Raced against the stop for the same reason the watch loop is, and it matters more here:
        // a ready check is allowed tens of seconds, and a daemon that only looked at its token
        // after `ready::wait` returned would sit through all of them on a service it is shutting
        // down. The future is dropped rather than resumed, so nothing here needs to be cancel safe.
        let outcome = tokio::select! {
            biased;

            () = self.cancel.cancelled() => None,

            outcome = ready::wait(self.spec.ready(), &mut supervised, &capture, &place) => Some(outcome),
        };

        // Asked to stop before it was ever ready. Through the same stop the spec asks for: the
        // process is up, `Starting` reaches `Stopping`, and a service that is mid-start is exactly
        // the one whose data directory should not be left to a destructor that kills.
        let Some(outcome) = outcome else {
            return self.stop(supervised, capture).await;
        };

        match outcome {
            Ok(ready::Ready::Ready) => {
                if !self
                    .move_to(ServiceState::Running, StateReason::Ready)
                    .await
                {
                    self.kill(supervised, capture).await;
                    self.record_exit(None).await;

                    return After::Done;
                }

                self.answered_by_this_start();

                self.supervise(supervised, capture, restarts).await
            }

            // The most common way a service fails to start, and the reason `ready::wait` races the
            // exit rather than polling the probe: this is the same path a crash an hour from now
            // takes, and it is the restart policy that decides between them.
            Ok(ready::Ready::Exited(exit)) => {
                // Asked before the process is reaped, and only about a failure: an exit of zero is
                // a service that did what it was told, and nothing about a port could improve on
                // that sentence.
                let conflict = if exit.is_success() {
                    None
                } else {
                    self.port_conflict(Some(supervised.pid())).await
                };

                let capture = self.kill(supervised, capture).await;
                let decision = restarts.ended(&exit, std::time::Instant::now(), &capture);

                self.after_exit(decision, exit.code(), conflict).await
            }

            // Running, and never going to be usable. Killed rather than left: the next attempt would
            // collide with the port and the data directory this one is holding.
            Ok(ready::Ready::TimedOut) => {
                let after = self.spec.ready().timeout();
                // Asked while the process is still up, so that its own pid is there to be
                // recognised: a service that bound its port and then failed its ready check is
                // holding that port itself, and is not in conflict with anybody.
                let conflict = self.port_conflict(Some(supervised.pid())).await;

                // Kept rather than dropped since roadmap task **T127a**: `kill` is also the drain,
                // so what comes back holds the server's last words instead of leaving them in a
                // pipe.
                let capture = self.kill(supervised, capture).await;
                let refused = self.refused_superuser(&capture);

                // **And what it last said, in the daemon's own log.** The service's own log file
                // holds these lines as well, but a ready timeout is the failure a reader meets
                // furthest from that file — in a CI log, in a bug report, in a screenshot — and
                // "not ready within 15s" on its own says only that something was slow. Measured on
                // run 35470533602, where a php-fpm pool timed out on Windows and the report held
                // no line of what the pool had printed.
                let said = capture.recent(TAIL_LINES);
                if !said.is_empty() {
                    tracing::warn!(
                        service = self.spec.id().as_str(),
                        said = said
                            .iter()
                            .map(|line| line.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" / "),
                        "this service was not ready in time; these are its last lines"
                    );
                }

                self.record_exit(None).await;

                // **A conflict outranks a refusal, and the order is not arbitrary.** If another
                // program holds the port, the client this check runs is talking to *that program* —
                // a refusal it printed is not this service's, and who holds the port is the fact
                // worth reporting.
                self.give_up(
                    conflict
                        .or(refused)
                        .unwrap_or(StateReason::ReadyTimeout { after }),
                    None,
                )
                .await
            }

            // A spec this build or this machine cannot check. Not a timeout, and reported as what it
            // is — see `StateReason::Uncheckable`.
            Err(error) => {
                let reason = uncheckable(&error);
                tracing::error!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "this service cannot be checked for readiness here"
                );

                self.kill(supervised, capture).await;
                self.record_exit(None).await;

                self.give_up(reason, None).await
            }
        }
    }

    /// Watch a running service: for the daemon asking it to stop, for it ending, for it going sick.
    ///
    /// One timer rather than two, and it is the cheaper one that sets the pace: the health check's
    /// interval is measured in seconds and the liveness poll in milliseconds, so the loop wakes on
    /// whichever is next and asks only the question that is due.
    async fn supervise(
        &mut self,
        mut supervised: Supervised,
        capture: Capture,
        restarts: &mut Restarts,
    ) -> After {
        let mut health = self.spec.health().map(Health::watching);
        let mut due = health
            .as_ref()
            .map(|health| Instant::now() + health.interval());

        // Whether the current run of unmakeable probes has already been reported. See the `Err` arm
        // below: a transient fault is worth one line, not one line every interval.
        let mut complained = false;

        // **The two halves of the state this loop owns** — roadmap task T71a. `healthy` starts true
        // because a service with no health check is one nothing has ever said is ill, and `state`
        // starts `Running` because `supervise` is entered from the `Running`/`Ready` transition
        // immediately above it.
        let mut healthy = true;
        let mut state = ServiceState::Running;

        // Taken once, before the loop: the borrow checker's reason is that the loop reaches `&mut
        // self`, and the better one is that a probe every ten seconds must not be a keyring read
        // every ten seconds. Only `HealthProbe::Command` reads it.
        let place = self.where_commands_run().await;

        loop {
            let watch = Instant::now() + WATCH;
            let wake = due.map_or(watch, |due| due.min(watch));

            tokio::select! {
                // Biased towards the stop: a service the daemon has been asked to shut down should
                // not spend one more health interval being probed.
                biased;

                () = self.cancel.cancelled() => {
                    return self.stop(supervised, capture).await;
                }

                // Ahead of the timer and behind the stop. A configuration that has already been
                // written is one the next liveness poll can wait for, and a daemon on its way out
                // should not spend a reload's patience on a service it is about to stop.
                () = self.asked_to_reload.notified() => {
                    self.reload(&place, &supervised).await;

                    continue;
                }

                // Beside the reload and for its reason: something has been written down and the
                // running process has not been told yet. Cheaper than a reload — no patience, no
                // signal, no command — because both mechanisms accept a rewrite with processes
                // already inside them.
                Ok(()) = self.limits_asked.changed() => {
                    let asked = *self.limits_asked.borrow_and_update();

                    if let Err(error) = supervised.set_limits(&super::super::limits::from_proto(asked)) {
                        // **Logged, and the service is left running.** A mechanism that refused a
                        // value is not a service that has stopped working, and a daemon that killed
                        // a healthy database because a cap could not be written would be doing more
                        // damage than the uncapped service ever could.
                        tracing::warn!(
                            service = self.spec.id().as_str(),
                            error = %error,
                            "cannot apply this service's new limits to the running process"
                        );
                    }

                    continue;
                }

                // **Nothing is done here, deliberately.** This arm exists to wake the loop; what
                // to do about the new value is the same decision a health probe reaches, and it is
                // made once, below, from both inputs.
                Ok(()) = self.over_memory.changed() => {}

                () = tokio::time::sleep_until(wake) => {}
            }

            match supervised.exited() {
                Ok(Some(exit)) => {
                    // Said before the kill rather than after, and this is the only place a readiness
                    // is published without a row behind it. Between a process ending and
                    // `after_exit` persisting what that meant lie a drain bounded by `FLUSH` and a
                    // write, and a walk arriving inside that window would otherwise read the `Up`
                    // this service stopped being — and start the tier below against a database that
                    // has gone. `Deciding` and not `Down` because what happens next is the restart
                    // policy's to say: whoever is waiting waits a moment longer for the answer
                    // rather than being handed a failure that a restart is about to contradict.
                    self.readiness.send_replace(Readiness::Deciding);

                    let capture = self.kill(supervised, capture).await;
                    let decision = restarts.ended(&exit, std::time::Instant::now(), &capture);

                    // No port diagnosis here, and the difference from a failed *start* is the whole
                    // reason: this process had the port. Whatever ended it, nobody took it away —
                    // and if something claims it before the restart, that start is where it will be
                    // met and named.
                    return self.after_exit(decision, exit.code(), None).await;
                }

                Ok(None) => {}

                // The OS will not say. Nothing to decide on, and the next tick asks again.
                Err(error) => tracing::warn!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "cannot tell whether this service is still running"
                ),
            }

            // **Folded before the health guard below, not inside it** — roadmap task T71a. A
            // service whose recipe declares no `HealthCheck` never reaches that branch, and a fold
            // living there would watch such a service's ceiling and never say a word about it.
            let over = *self.over_memory.borrow_and_update();
            let folded = fold(healthy, over);

            if worth_recording(state, folded.0) && self.move_to(folded.0, folded.1).await {
                state = folded.0;
            }

            let Some(watching) = health.as_mut() else {
                continue;
            };

            if due.is_some_and(|due| Instant::now() < due) {
                continue;
            }

            // Raced against the stop for the same reason the sleep above is: the probe is a program
            // the spec named or an HTTP request, and both are bounded by `HealthCheck::timeout`
            // rather than by anything short. A `mariadb-admin ping` against a database that has
            // stopped answering hangs for the whole of it, and awaiting that outright would make a
            // `mix stop` arriving mid-probe wait out a deadline set for judging health, not for
            // shutting down. Dropping the future is the cancellation: `run_once` kills the child it
            // spawned, and the fold this verdict would have gone into belongs to a loop that is
            // ending anyway.
            let examined = tokio::select! {
                biased;

                () = self.cancel.cancelled() => {
                    return self.stop(supervised, capture).await;
                }

                examined = watching.examine(&place) => examined,
            };

            match examined {
                // **Recorded, not applied** — roadmap task T71a. The verdict changes what this loop
                // believes about the service's health; what state that adds up to is the fold's, at
                // the top of the next turn, because size has a say in it too.
                Ok(Some(Verdict::Degraded)) => {
                    healthy = false;
                }

                Ok(Some(Verdict::Recovered)) => {
                    // The backoff, not the failure history: a service that recovers between crashes
                    // is still crashing, and `Restarts` is what remembers that.
                    restarts.recovered();
                    healthy = true;
                }

                Ok(None) => {}

                // A probe that could not be made *this time*: the binary was being replaced by an
                // upgrade, the machine was out of process slots. **Not a verdict about the service**
                // — nothing was measured, so degrading it would report a bad moment as a sick
                // database — and not a reason to stop asking either, because the next interval is
                // entitled to a different answer. Said once and then only counted, so a fault that
                // lasts an hour is one line in `daemon.log` rather than three hundred.
                Err(error) if error.might_work_later() => {
                    if !complained {
                        complained = true;

                        tracing::warn!(
                            service = self.spec.id().as_str(),
                            error = %error,
                            "this service's health probe could not be made; it will be tried again \
                             at the next interval"
                        );
                    }

                    due = Some(Instant::now() + watching.interval());

                    continue;
                }

                // A probe this build or this machine cannot make. **The service is left alone**,
                // deliberately: it is running and its readiness was proved, and degrading it for a
                // check nobody can make would report a fault in the spec as a fault in the service.
                // Said once, because the answer will not change, and then never probed again.
                Err(error) => {
                    tracing::warn!(
                        service = self.spec.id().as_str(),
                        error = %error,
                        "this service cannot be health-checked here; it will be watched for exiting \
                         and nothing else"
                    );

                    health = None;
                    due = None;

                    continue;
                }
            }

            // Cleared by any probe that was actually made, so a fault that comes back after an hour
            // of working is reported again rather than swallowed by the first one.
            complained = false;

            due = Some(Instant::now() + watching.interval());
        }
    }

    /// What happens after the process ended by itself.
    /// Who else is on a port this service was going to listen on — roadmap task **T38**.
    ///
    /// On a blocking task because every implementation of the capability is a synchronous read of
    /// the machine, and one of them starts a process to do it (see `mixengine_platform`'s macOS
    /// module). A join that fails is treated as no diagnosis, which is the same answer every other
    /// failure to ask gets.
    async fn port_conflict(&self, ours: Option<u32>) -> Option<StateReason> {
        if self.spec.ports().is_empty() {
            return None;
        }

        let host = Arc::clone(&self.host);
        let ports = self.spec.ports().to_vec();

        tokio::task::spawn_blocking(move || {
            super::super::ports::conflict(host.as_ref(), &ports, ours)
        })
        .await
        .ok()
        .flatten()
    }

    /// What this service's last words say about a refused superuser, if anything — task **T127a**.
    ///
    /// **The same shape as [`Runner::port_conflict`], and asked at the same moment**: a start that
    /// has already failed, and a question that can turn an honest but useless reason into a useful
    /// one. Nothing here can fail a start that was going to succeed, which is the whole reason this
    /// reads a failure rather than racing one — a reader that aborted a start on one log line could
    /// take down a cluster somebody's application was merely polling with a stale password.
    ///
    /// The capture must already be drained — [`Runner::kill`] is what does that — or the server's
    /// last line is still sitting in a pipe.
    fn refused_superuser(&self, capture: &Capture) -> Option<StateReason> {
        let superuser = self.superuser?;
        let recent = capture.recent(TAIL_LINES);

        super::super::databases::refusal_in_log(
            recent.iter().map(|line| line.text.as_str()),
            superuser,
        )
        .map(|said| StateReason::SuperuserRefused { said })
    }

    pub(super) async fn after_exit(
        &self,
        decision: Decision,
        code: Option<i32>,
        conflict: Option<StateReason>,
    ) -> After {
        self.record_exit(code).await;

        match decision {
            // It did what it was asked to. Through `Stopping`, which is the only edge the machine
            // has into `Stopped` — a service that exited cleanly is stopping for exactly as long as
            // these two writes take.
            Decision::Rest { reason } => {
                self.move_to(ServiceState::Stopping, reason.clone()).await;
                self.move_to(ServiceState::Stopped, reason).await;

                After::Done
            }

            // **A conflict replaces an exit and never a crash loop.** `StateReason::Exited` says
            // only what the OS said, and a port held by somebody else is the same fact with the
            // useful half attached; `StateReason::CrashLoop` carries its own count and the lines
            // the service printed, which is more than this could add.
            Decision::GiveUp { reason } => {
                let reason = match (reason, conflict) {
                    (StateReason::Exited { .. }, Some(conflict)) => conflict,
                    (reason, _) => reason,
                };

                self.give_up(reason, None).await
            }

            // The `Restarting` is what answers a walk that is waiting on this service, which is why
            // a failure to persist it ends the task: a runner that went on restarting from a row
            // nobody could read would leave that walk with nothing to wait for.
            Decision::Restart { after, attempt } => {
                if !self
                    .move_to(
                        ServiceState::Restarting,
                        conflict.unwrap_or(StateReason::Exited { code }),
                    )
                    .await
                {
                    return After::Done;
                }

                After::Again { after, attempt }
            }
        }
    }

    /// Take the request this start has just answered, so that no later crash is released by it.
    ///
    /// **A permit is only ever consumed by [`Runner::wait_out`], and a runner that is mid-start is
    /// not in one.** A request that arrives while `ready::wait` is running — two walks sharing a
    /// dependency, which is the ordinary case and not the rare one — is kept by the [`Notify`] until
    /// something waits on it. If this start then *succeeds*, nothing does for as long as the service
    /// stays up: the permit outlives the request entirely, and the next crash — hours later and
    /// asked for by nobody — leaves its backoff the instant it enters it, with the ladder reset by
    /// [`Restarts::recovered`] and the move published as [`StateReason::Requested`].
    ///
    /// Reaching `Running` is what makes such a request *answered* rather than dropped: whoever asked
    /// for this service to be started now has it started now, which is the whole of what they asked
    /// for. A request that arrives after this is about the life that follows, and [`Runner::wait_out`]
    /// is where it belongs.
    ///
    /// [`Notify`]: tokio::sync::Notify
    fn answered_by_this_start(&self) {
        let asked = std::pin::pin!(self.asked_to_start.notified());

        // `enable` is how a stored permit is taken without waiting for one: it registers this
        // future's interest and says whether there was already something to receive. The future is
        // dropped on the next line, which is what makes this a read and not a wait.
        if asked.enable() {
            tracing::debug!(
                service = self.spec.id().as_str(),
                "a start asked for while this service was starting is answered by that start"
            );
        }
    }

    /// Move to `Failed` for `reason`. Whoever is waiting on this service is answered by that move.
    async fn give_up(&self, reason: StateReason, detail: Option<String>) -> After {
        self.move_to_noting(ServiceState::Failed, reason, detail.as_deref())
            .await;

        After::Done
    }
}

/// Turn a supervisor refusal into the reason a user reads.
///
/// [`Error::UnsupportedCheck`](mixengine_supervisor::Error::UnsupportedCheck) already carries both
/// halves in the words the spec's author needs, so they are passed through rather than re-worded.
/// Everything else — a pattern that will not compile, a socket this OS does not have — is described
/// by its own chain, which is where `mixengine-platform` writes what it refused and why.
fn uncheckable(error: &mixengine_supervisor::Error) -> StateReason {
    match error {
        mixengine_supervisor::Error::UnsupportedCheck { check, reason } => {
            StateReason::Uncheckable {
                check: (*check).to_owned(),
                reason: reason.clone(),
            }
        }

        // A pattern that will not compile, a socket this OS does not have. Flattened rather than
        // printed, because these types carry the cause as a `source` precisely so that no layer
        // repeats it — and the layer showing it to a person is the one that has to join it back up.
        other => StateReason::Uncheckable {
            check: "the readiness check this service declares".to_owned(),
            reason: mixengine_proto::flatten(other),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use mixengine_proto::ServiceId;

    use super::super::super::fixture::{home, spec};
    use super::super::tests::adopted_runner;
    use super::*;

    /// **A spawn that failed keeps its sentence on the row** — T200b, D5. Before it, `daemon.log`
    /// was the only place a missing credential was named.
    #[tokio::test]
    async fn a_spawn_that_failed_leaves_its_sentence_on_the_row() {
        let (_home, paths, store) = home(&["mariadb"]).await;
        sqlx::query("UPDATE services SET state = 'starting' WHERE id = 'mariadb'")
            .execute(store.pool())
            .await
            .expect("the row");

        let mut runner = adopted_runner(
            spec("mariadb")
                .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb@main/root")
                .build()
                .expect("a usable spec"),
            &paths,
            &store,
            Duration::ZERO,
        );
        let mut restarts = Restarts::under(runner.spec.restart());

        runner.attempt(&mut restarts).await;

        let note =
            mixengine_core::services::record(&store, &ServiceId::parse("mariadb").expect("an id"))
                .await
                .expect("the row")
                .last_failure
                .expect("a note");
        assert_eq!(note.reason, StateReason::SpawnFailed);
        assert!(
            note.detail
                .contains("no credential is stored at mixengine/mariadb@main/root"),
            "{}",
            note.detail
        );
    }
}
