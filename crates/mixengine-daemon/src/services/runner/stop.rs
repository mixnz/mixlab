//! Asking a service to stop, within the grace period its spec gives and the budget a
//! shutdown has left, and killing what does not — a child of this process or an adopted
//! survivor of an earlier daemon alike.

use std::time::Duration;

use mixengine_platform::process::{Adopted, CAN_ASK_TO_STOP, Exit, Supervised};
use mixengine_proto::{ServiceState, StateReason, StopBehaviour};
use mixengine_supervisor::logs::Capture;
use tokio::time::Instant;

use super::{After, FLUSH, GONE, GONE_FLOOR, POLL, Runner, gone};

impl Runner {
    /// Stop a service this daemon adopted, the way its spec asks.
    ///
    /// The shape of [`Runner::stop`], with the one difference adoption forces: this process cannot
    /// *wait* for a process it is not the parent of, so where the supervised path blocks in the
    /// kernel this one polls the identity until it stops answering.
    ///
    /// **A survivor that will not go leaves its row where it is**, which is the only honest answer
    /// available and is also self-healing. Recording `Stopped` for a process that is still holding
    /// the port would be the orphan this whole task exists to prevent, written down as a fact; so
    /// the row keeps its `stopping` and its pid, and the daemon that starts next meets exactly the
    /// case crash recovery already handles — a supervised state with a live process behind it — and
    /// stops it again.
    ///
    /// That row is also what answers the person who asked. This task ends either way, so
    /// [`Registry::stop_one`](super::super::Registry) reads the row afterwards rather than the task's
    /// ending, and a walk that could not take the service down says so instead of reporting a stop
    /// that did not happen.
    pub(super) async fn stop_adopted(&self, mut adopted: Adopted) -> After {
        let reason = self.stopping_because();

        self.move_to(ServiceState::Stopping, reason.clone()).await;

        // `GONE` and not `FLUSH`: what this path does after the request is watch a process it is not
        // the parent of leave the table, and that is the tail its grace period has to leave room for.
        self.ask_to_stop(&mut adopted, GONE).await;

        // Killed whatever the polite half achieved, on the same reasoning as the supervised path:
        // the leader exiting is not the workers exiting. On Unix this reaches the group the survivor
        // still leads; on Windows it reaches the one process, the job object having gone with the
        // daemon that made it.
        if let Err(error) = adopted.stop() {
            tracing::warn!(
                service = self.spec.id().as_str(),
                pid = adopted.pid(),
                error = %error,
                "cannot stop the adopted process"
            );
        }

        // **Capped here and not inside [`gone`], which is the one place the budget does not reach.**
        // That function is shared with `Registry::discard`, which polls a survivor during crash
        // recovery — before the first client is served, and so before any shutdown exists to have a
        // budget — so a parameter threaded through it would be one that caller has nothing honest to
        // pass. Dropping the poll says exactly what its own deadline passing says: it is not known to
        // have gone, so the row keeps its `stopping` and its pid for the next daemon. The inner
        // `GONE` still bounds the caller that has no budget, and `seeing_it_go` is what stops a whole
        // walk being five seconds longer than it was allowed for one survivor that will not go —
        // without shortening the question to nothing, which would answer it wrongly.
        let went = tokio::time::timeout(self.seeing_it_go(), gone(self.spec.id(), &adopted))
            .await
            .unwrap_or(false);

        if !went {
            tracing::error!(
                service = self.spec.id().as_str(),
                pid = adopted.pid(),
                "this adopted process did not go when it was stopped; its row is left saying so, \
                 for the next daemon to stop it again"
            );

            return After::Done;
        }

        self.record_exit(None).await;

        self.move_to(ServiceState::Stopped, reason).await;

        After::Done
    }

    /// Stop the service the way its spec asks, then record that it is stopped.
    ///
    /// Reached from either side of readiness — from the watch loop, and from a stop that arrived
    /// while the ready check was still running. A walk waiting on this service is answered by the
    /// `Stopping` this begins with rather than by anything at the end: it learns that the service
    /// will not be coming up at the moment that becomes true, instead of sitting through the grace
    /// period of a stop it did not ask for.
    pub(super) async fn stop(&self, mut supervised: Supervised, capture: Capture) -> After {
        let reason = self.stopping_because();

        self.move_to(ServiceState::Stopping, reason.clone()).await;

        // `FLUSH` is what the kill below still needs after this returns, and saying so here is what
        // keeps the whole of this stop inside a shutdown's budget rather than the polite half of it.
        let exit = self.ask_to_stop(&mut supervised, FLUSH).await;

        // Whatever the polite half achieved, the group is killed afterwards: the leader exiting is
        // not the workers exiting, and a php-fpm pool left holding the port is what the next start
        // collides with.
        self.kill(supervised, capture).await;
        self.record_exit(exit.and_then(|exit| exit.code())).await;

        self.move_to(ServiceState::Stopped, reason).await;

        After::Done
    }

    /// Ask the group to leave on its own, and wait as long as the spec says. `None` if it did not.
    ///
    /// Written against [`Stoppable`] rather than against [`Supervised`] because a service this
    /// daemon adopted is stopped by the same `StopBehaviour` as one it started: the spec is the
    /// user's statement about what the *service* needs in order to shut down cleanly, and it does
    /// not become less true because the daemon that spawned the process was killed. What differs
    /// between the two is only what the request travels on, which is the trait's whole surface.
    ///
    /// `tail` is how long the caller's own kill still needs after this returns — [`FLUSH`] for the
    /// supervised path, [`GONE`] for the adopted one. It is a parameter and not a constant here
    /// because those two really are different amounts of work, and reserving the larger of them for
    /// both would take five seconds off every supervised stop to pay for a drain that costs two. See
    /// [`Runner::grace_for`], which is where it is spent.
    async fn ask_to_stop(&self, process: &mut dyn Stoppable, tail: Duration) -> Option<Exit> {
        // Started before the request rather than after it, which only `Command` can tell the
        // difference: a signal is sent in microseconds, while running `mariadb-admin shutdown` is
        // itself part of what the spec's grace period was written to cover. The rule is T9a's, one
        // level down — whatever the spec allows, minus what has already been spent. The `Command`
        // arm moves it once more, for the one thing that is neither.
        let mut began = Instant::now();

        let grace = match self.spec.stop() {
            // Nothing to ask. Honest rather than a grace period spent on a request nobody sent.
            StopBehaviour::Kill => return None,

            StopBehaviour::Signal { grace } => {
                // ADR 0008: Windows has no request a daemon can send to a process it gave no console
                // to, so the grace period is not spent pretending otherwise.
                if !CAN_ASK_TO_STOP {
                    return None;
                }

                if let Err(error) = process.ask_to_stop() {
                    tracing::warn!(
                        service = self.spec.id().as_str(),
                        error = %error,
                        "cannot ask this service to stop; it will be killed"
                    );

                    return None;
                }

                self.grace_for(*grace, tail)
            }

            // The polite stop for a service that has something to flush, and on Windows the *only*
            // one there is (ADR 0008). What the command does is ask; what proves it worked is the
            // process going, which is the wait below — a `mariadb-admin shutdown` returns as soon as
            // the server has accepted the instruction, not once it has finished acting on it.
            StopBehaviour::Command {
                program,
                args,
                grace,
            } => {
                let place = self.where_commands_run().await;

                // **The clock starts after this, not before it.** Resolving the environment is the
                // daemon's own preparation rather than any part of the service shutting down, and
                // for an adopted service it is a keyring read allowed the whole of `ENVIRONMENT`.
                // Charged to the grace period it is spent before the request is even sent: a
                // three-second read and a `mariadb-admin shutdown` that returns in two would use up
                // a five-second grace outright, and the service would be killed mid-flush — the
                // "recovery on its next start" the arms below exist to avoid.
                began = Instant::now();

                // Clamped here rather than above for the same reason the clock starts here: what a
                // whole-daemon shutdown has left is smaller after a three-second keyring read than
                // it was before one, and the command is what should be given the difference.
                let allowed = self.grace_for(*grace, tail);

                // **Zero is a kill, not a command run with no patience.** `run` would spawn the
                // program, find the deadline already past on the next poll and kill it — a process
                // started, two pipes made and a `mariadb-admin` given no chance to say anything, at
                // the one moment the daemon has run out of the time it was given. Nothing is left to
                // hear an answer in, so the question is not asked; `ended_meanwhile` is still read,
                // because a service that went by itself must not be recorded as one that was killed.
                if allowed.is_zero() {
                    tracing::info!(
                        service = self.spec.id().as_str(),
                        program = %program.display(),
                        "nothing is left of the shutdown budget; this service's stop command is not \
                         run and it is killed at once, which may leave it to recover on its next \
                         start"
                    );

                    return self.ended_meanwhile(process);
                }

                match place.run(program, args, allowed).await {
                    Ok(ran) if ran.succeeded() => allowed,

                    // It ran and refused, or it ran out of the whole grace period. Either way the
                    // service has not been asked successfully and waiting longer buys nothing, so
                    // this falls through to the kill — loudly, and carrying whatever the program
                    // said, because for a database that kill is a recovery on its next start and
                    // `ERROR 1045: Access denied` is the whole of what the user has to act on.
                    Ok(ran) => {
                        if let Some(exit) = self.ended_meanwhile(process) {
                            tracing::info!(
                                service = self.spec.id().as_str(),
                                program = %program.display(),
                                timed_out = ran.timed_out(),
                                complaint = ran.complaint().unwrap_or("it said nothing"),
                                "this service's stop command did not report success, but the \
                                 service stopped anyway"
                            );

                            return Some(exit);
                        }

                        tracing::error!(
                            service = self.spec.id().as_str(),
                            program = %program.display(),
                            timed_out = ran.timed_out(),
                            complaint = ran.complaint().unwrap_or("it said nothing"),
                            "this service's stop command did not work; killing it instead, which \
                             may leave it to recover on its next start"
                        );

                        return None;
                    }

                    // The program a spec names is not on this machine, or cannot be started. A
                    // spec to fix rather than a service to blame, and the service still has to stop.
                    Err(error) => {
                        let ended = self.ended_meanwhile(process);

                        // Said either way: a program a spec names and this machine does not have is
                        // a spec to fix, and it stays broken whether or not the service happened to
                        // go by itself this time.
                        tracing::error!(
                            service = self.spec.id().as_str(),
                            program = %program.display(),
                            error = %error,
                            killing = ended.is_none(),
                            "cannot run this service's stop command"
                        );

                        return ended;
                    }
                }
            }

            // `StopBehaviour` is `#[non_exhaustive]`. A behaviour this build does not know is not a
            // licence to invent one, and the service still has to stop.
            other => {
                tracing::warn!(
                    service = self.spec.id().as_str(),
                    behaviour = ?other,
                    "unknown stop behaviour; killing the service"
                );

                return None;
            }
        };

        let deadline = began + grace;

        loop {
            match process.exited() {
                Ok(Some(exit)) => return Some(exit),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        service = self.spec.id().as_str(),
                        error = %error,
                        "cannot tell whether this service has stopped; it will be killed"
                    );

                    return None;
                }
            }

            if Instant::now() >= deadline {
                tracing::info!(
                    service = self.spec.id().as_str(),
                    grace = ?grace,
                    "this service did not stop when asked; killing it"
                );

                return None;
            }

            tokio::time::sleep(POLL).await;
        }
    }

    /// Why the stop about to happen is happening.
    ///
    /// [`StateReason::Requested`] unless somebody set [`Runner::stopping_because`] first, which is
    /// the idle sweeper and nothing else. Read rather than subscribed to: there is one moment this
    /// matters, and it is the moment a stop begins.
    pub(super) fn stopping_because(&self) -> StateReason {
        self.stopping_because
            .borrow()
            .clone()
            .unwrap_or(StateReason::Requested)
    }

    /// How long this service may actually take to stop: what its spec asks for, or what a shutdown
    /// has left once the kill after it is paid for, whichever is less — roadmap task **T9a**.
    ///
    /// **The two numbers answer different questions and neither is wrong.** A spec's grace period is
    /// what *this* service needs in order to shut down cleanly, and it stays true whether the daemon
    /// is stopping one service or twelve; the budget is what somebody who typed `mix daemon stop`
    /// is waiting for, and until this task nothing owned it — twelve services each allowed ten
    /// seconds was two minutes nobody had agreed to.
    ///
    /// **`tail` is what makes the budget a bound on the total rather than on the polite half of it.**
    /// Asking a service to stop is not the last thing a stop does: the kill after it drains the
    /// service's pipes ([`FLUSH`]), or watches a survivor leave the process table ([`GONE`]), and
    /// both are per service and both come *after* the grace period. A grace period clamped to the
    /// whole of what is left therefore overran the budget by that tail once per service — three
    /// services on Windows was a budget of two and a half seconds and a walk of eight and a half,
    /// against an OS ceiling of five. So what is kept back here is exactly
    /// [`CEILING_RESERVE`](crate::serve::CEILING_RESERVE) one level down: a shutdown subtracts from an OS
    /// ceiling what it still has to do after the last service stops, and a service subtracts from
    /// the budget what it still has to do after its own request to stop.
    ///
    /// Outside a shutdown there is no total to divide and the spec is the whole answer, which is what
    /// [`Budget::remaining`](super::super::Budget::remaining)'s [`None`] means — **`tail` is subtracted from
    /// nothing there**, because a `mix service stop mariadb` on a running machine has no total for
    /// the kill to have to fit inside. Inside one, a service reached after the budget is spent gets
    /// zero and is killed at once — stated in its row and in the log line above, because for a
    /// database that is a recovery on its next start rather than a clean stop, and the honest report
    /// is the whole of what makes a shorter budget somebody's choice instead of a surprise.
    fn grace_for(&self, asked: mixengine_proto::Millis, tail: Duration) -> Duration {
        let asked = asked.as_duration();

        let Some(left) = self.budget.remaining() else {
            return asked;
        };

        let allowed = left.saturating_sub(tail);

        if allowed >= asked {
            return asked;
        }

        tracing::info!(
            service = self.spec.id().as_str(),
            asked = ?asked,
            left = ?left,
            tail = ?tail,
            allowed = ?allowed,
            "this service is being stopped inside a shutdown with less time left than its spec asks \
             for; it gets what is left, less what killing it afterwards still needs"
        );

        allowed
    }

    /// What is left of a shutdown's budget for something that wants `wants` — roadmap task **T9a**.
    ///
    /// The other half of [`Runner::grace_for`], for the parts of a stop that are not a grace period
    /// at all: the log drain in [`Runner::kill`], the watch in [`Runner::stop_adopted`], and the
    /// environment read in [`Runner::where_commands_run`] that an adopted service's stop command
    /// waits on before any of it begins. Those are constants rather than anything the spec asks for,
    /// and a constant charged outside the budget is a constant the budget does not bound — which is
    /// the whole of what `docs/architecture/daemon-and-ipc.md` promises when it says the budget
    /// bounds the total.
    ///
    /// [`None`] is the ordinary non-shutdown state and means the constant entire: nothing is
    /// counting, and there is no total for it to have to fit inside.
    pub(super) fn within_budget(&self, wants: Duration) -> Duration {
        self.budget
            .remaining()
            .map_or(wants, |left| wants.min(left))
    }

    /// How long the poll after the kill in [`Runner::stop_adopted`] gets — roadmap task **T9a**.
    ///
    /// [`Runner::within_budget`] with a floor under it, and the floor is the whole of what this adds:
    /// see [`GONE_FLOOR`] for why zero is the one answer this particular wait cannot be given, and
    /// [`Budget::reprieve`](super::super::Budget) for where the floor is paid from. Everything above the
    /// floor is the budget's answer unchanged — a shutdown with seconds left still watches for
    /// [`GONE`], and one with fifty milliseconds left still watches for fifty rather than for a
    /// hundred, because a budget that has not run out is a budget that is still being kept.
    ///
    /// **What the reprieve running out means is what a spent budget always means here**: the poll is
    /// short, the survivor is not known to have gone, and the row says so. The floor makes the
    /// question answerable in the ordinary case; it does not make the answer up.
    fn seeing_it_go(&self) -> Duration {
        let allowed = self.within_budget(GONE);

        // `None` is the ordinary non-shutdown state, where `allowed` is `GONE` entire and this `max`
        // changes nothing. Inside a shutdown it is what is left of the one window the whole walk
        // shares, so the floor is granted to as many survivors as fit inside it and no more.
        let floor = self
            .budget
            .reprieve()
            .map_or(GONE_FLOOR, |left| GONE_FLOOR.min(left));

        allowed.max(floor)
    }

    /// Whether the service ended by itself while it was being asked to stop.
    ///
    /// **The one thing a failed stop request must not lose.** Running `mariadb-admin shutdown` is a
    /// whole grace period's worth of time, and a server that took the instruction and exited inside
    /// that window has stopped exactly as it was asked to — even if the program carrying the
    /// instruction then returned non-zero, or ran out of patience waiting for a server that had
    /// already gone. Answering [`None`] there records no exit code at all and reports a kill that
    /// never happened, on a service that shut down cleanly.
    ///
    /// The wait loop below reads this every `POLL`; the arms that return early have to read it once
    /// themselves, because they are the paths that do not reach the loop.
    fn ended_meanwhile(&self, process: &mut dyn Stoppable) -> Option<Exit> {
        match process.exited() {
            Ok(exit) => exit,

            // Not knowing is treated as still running, which is the safe half: the caller kills, and
            // killing a process that has already gone costs nothing.
            Err(error) => {
                tracing::warn!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "cannot tell whether this service has stopped"
                );

                None
            }
        }
    }

    /// Kill whatever is left of the group and collect the last lines it printed.
    ///
    /// In that order: killing first is what makes the drain finish, because a worker still holding a
    /// copy of the service's stdout keeps the pipe open long after the leader has gone.
    ///
    /// Off the runtime, because both halves block — `docs/standards/rust.md` requires it of
    /// anything that waits. A blocking task that panicked leaves an empty capture rather than taking
    /// the supervisor of every other service down with it.
    ///
    /// **The drain is inside a shutdown's budget and not after it** — see [`Runner::within_budget`].
    /// This runs once per service and after that service's grace period is spent, and it is really
    /// reached: killing the group does not reach a grandchild that left it, and one of those still
    /// holding a copy of the service's stdout keeps the pipe open for the whole of [`FLUSH`]. Left
    /// uncapped, a walk of three services would spend six seconds past a total somebody was told —
    /// which on Windows is the daemon being terminated in the middle of `Store::close`, the one
    /// outcome [`CEILING_RESERVE`](crate::serve::CEILING_RESERVE) exists to prevent.
    pub(super) async fn kill(&self, supervised: Supervised, mut capture: Capture) -> Capture {
        let service = self.spec.id().clone();
        let flush = self.within_budget(FLUSH);

        tokio::task::spawn_blocking(move || {
            // Its `Drop` is the kill: the group goes, whether or not the leader had already exited.
            drop(supervised);

            if !capture.finish(flush) {
                tracing::warn!(
                    service = service.as_str(),
                    "the last lines of this service were not read before it was let go"
                );
            }

            capture
        })
        .await
        .unwrap_or_else(|error| {
            tracing::error!(%error, "the task stopping a service did not finish");

            Capture::detached()
        })
    }
}

/// A process the runner can ask to stop and then watch for.
///
/// **Two implementations, and the trait exists to keep one `StopBehaviour` reading rather than two.**
/// A `Supervised` child is this daemon's own and a survivor it adopted (roadmap task T18) is not,
/// but the grace period a spec asks for is about the *service* — the seconds MariaDB needs to flush
/// — and is the same either way. What differs is only what a request travels on and where the answer
/// comes from: a status the kernel keeps for a child of ours, and the process's identity for one
/// that was somebody else's.
///
/// Deliberately the two calls [`Runner::ask_to_stop`] makes and no more. The kill afterwards is not
/// here because the two are genuinely different — a group whose ownership this process holds, and a
/// pid it can only signal — and folding them together would hide the one place adoption is weaker.
trait Stoppable: Send {
    /// Ask it to stop, the way this system asks. See [`process::CAN_ASK_TO_STOP`].
    ///
    /// [`process::CAN_ASK_TO_STOP`]: mixengine_platform::process::CAN_ASK_TO_STOP
    fn ask_to_stop(&self) -> mixengine_platform::Result<()>;

    /// Whether it has ended, without waiting for it either way.
    fn exited(&mut self) -> mixengine_platform::Result<Option<Exit>>;
}

impl Stoppable for Supervised {
    fn ask_to_stop(&self) -> mixengine_platform::Result<()> {
        Self::ask_to_stop(self)
    }

    fn exited(&mut self) -> mixengine_platform::Result<Option<Exit>> {
        Self::exited(self)
    }
}

impl Stoppable for Adopted {
    fn ask_to_stop(&self) -> mixengine_platform::Result<()> {
        Self::ask_to_stop(self)
    }

    /// Takes `&mut self` although the underlying question does not, because the other implementation
    /// needs it: `Supervised::exited` reaps a child, which is a mutation of the handle.
    fn exited(&mut self) -> mixengine_platform::Result<Option<Exit>> {
        Self::exited(self)
    }
}

#[cfg(test)]
mod characterization;

#[cfg(test)]
mod tests {
    use mixengine_proto::{Millis, ServiceSpec};
    use mixengine_testkit::FakeService;

    use super::super::super::fixture::{arguments, home, spec};
    use super::super::tests::{SLACK, runner};
    use super::*;

    /// How long a test waits before reading a file a stop command would have left behind.
    ///
    /// Far longer than a `fakeservice --touch` needs — it writes the file before it does anything
    /// else at all — so a run that got as far as starting has had every chance to leave its trace.
    const SETTLE: Duration = Duration::from_millis(500);

    /// The budget these tests hand a runner that is being shut down.
    ///
    /// Bigger than [`GONE`] by a second, which is the whole arrangement: the tail these stops
    /// reserve is `GONE`, so a grace period clamped correctly is one second and one clamped to the
    /// whole of what is left is six. There is no third answer for a slow runner to land on.
    const BUDGET: Duration = Duration::from_secs(6);

    /// A process that takes every request to stop and acts on none of them.
    ///
    /// The whole of what these tests need from a [`Stoppable`]: what is under test is how long the
    /// runner is *willing* to wait, and a process that ended would answer the wait instead of
    /// letting it reach its deadline. Deliberately not a real one — the thing being timed is a
    /// budget, and a second child would put its own spawn inside every measurement.
    struct NeverStops;

    impl Stoppable for NeverStops {
        fn ask_to_stop(&self) -> mixengine_platform::Result<()> {
            Ok(())
        }

        fn exited(&mut self) -> mixengine_platform::Result<Option<Exit>> {
            Ok(None)
        }
    }

    /// A spec whose stop command is a `fakeservice` that runs until something kills it.
    ///
    /// **`Command` and not `Signal`, for the reason T9a's own budget test gives**: Windows sends no
    /// request to stop at all (ADR 0008), so a `Signal` spec spends no grace period there and every
    /// assertion below would pass without the clamp existing — green on the one system whose
    /// console clock motivated the task.
    fn stopped_by_a_command(id: &str, grace: Millis, command: &FakeService) -> ServiceSpec {
        spec(id)
            .stop(StopBehaviour::Command {
                program: FakeService::program(),
                args: arguments(command),
                grace,
            })
            .build()
            .expect("a usable spec")
    }

    /// **A grace period the budget clamps still leaves room for the kill after it** — the half of
    /// T9a's promise the first implementation did not keep.
    ///
    /// The budget is a deadline for the whole walk, and asking a service to stop is not the whole of
    /// stopping one: the kill afterwards drains the service's pipes, or watches a survivor leave the
    /// process table, per service and *after* the grace period. A clamp to the whole of what is left
    /// therefore overran the total by that tail every time — which on Windows is the daemon being
    /// terminated in the middle of the WAL checkpoint it stopped its services in order to make.
    ///
    /// So the assertion is on the sum and not on the wait: what the ask took plus what its caller
    /// still has to spend has to fit in the budget. The stop command never returns, which is what a
    /// `mariadb-admin shutdown` against a server that has stopped answering looks like, so the ask
    /// spends exactly what it was allowed and the number is the clamp itself.
    #[tokio::test]
    async fn a_grace_the_budget_clamps_leaves_room_for_the_kill_that_follows_it() {
        let (_home, paths, store) = home(&["mariadb"]).await;

        // Asks, and never comes back. Nothing here creates a file it would exit for.
        let unanswering = FakeService::new();
        let runner = runner(
            stopped_by_a_command("mariadb", Millis::from_secs(60), &unanswering),
            &paths,
            &store,
        );

        runner.budget.narrow_to(Instant::now() + BUDGET);

        let began = Instant::now();
        let exit = runner.ask_to_stop(&mut NeverStops, GONE).await;
        let took = began.elapsed();

        assert!(
            exit.is_none(),
            "the service ignored the request, so the caller is told to kill it"
        );

        assert!(
            took + GONE <= BUDGET + SLACK,
            "the ask took {took:?} and the kill after it still needs {GONE:?}, which is more than \
             the {BUDGET:?} the whole stop was given: the grace period was clamped to what was \
             left instead of to what was left minus the tail"
        );

        // The other direction, and the one an over-eager reserve would get wrong: what is left
        // *minus* the tail is still a second of asking, and a stop that skipped it would be a
        // database killed with a shutdown command available and unused.
        assert!(
            took >= Duration::from_millis(500),
            "the stop command was given {took:?}, which is not the second the budget had room for"
        );
    }

    /// **A budget with nothing left kills at once rather than starting a fresh anything.**
    ///
    /// Zero is a real answer and not a missing one — see [`Budget::remaining`] — and what it has to
    /// mean is the kill. A grace period that began again from the spec's number here would be a
    /// service handed sixty more seconds by the very arithmetic that exists to take them away, at
    /// the moment the daemon has none left to give.
    ///
    /// **What is asserted is the decision, and the reason that is not also the spawn is worth
    /// writing down.** The code skips running the command as well, because a `run` with no patience
    /// would start a process and two pipes and kill them on the next poll for an answer nothing can
    /// wait to hear — but no test can watch that happen: the child is dropped microseconds after it
    /// is created, long before the program can load, read its arguments and write anything down, so
    /// the file below is missing under the spawn every bit as reliably as under the skip. It is
    /// still watched, because it can only ever accuse truthfully, and it is the assertions above it
    /// that carry the test.
    #[tokio::test]
    async fn a_budget_that_is_already_spent_kills_at_once_and_starts_no_fresh_grace_period() {
        let (home, paths, store) = home(&["mariadb"]).await;

        let marker = home.path().join("the-stop-command-ran");
        let announcing = FakeService::new().touch(&marker);
        let runner = runner(
            stopped_by_a_command("mariadb", Millis::from_secs(60), &announcing),
            &paths,
            &store,
        );

        // Already elapsed: the walk reached this service with the whole of its budget spent on the
        // ones before it, which is the case T9a's budget exists to bound.
        runner.budget.narrow_to(Instant::now());

        assert_eq!(
            runner.grace_for(Millis::from_secs(60), FLUSH),
            Duration::ZERO,
            "a service reached after the budget ran out gets nothing, and nothing is the kill"
        );

        let began = Instant::now();
        let exit = runner.ask_to_stop(&mut NeverStops, FLUSH).await;
        let took = began.elapsed();

        assert!(exit.is_none(), "nothing was asked, so nothing stopped");
        assert!(
            took < SLACK,
            "a stop with nothing left to spend spent {took:?}; it is meant to fall straight through \
             to the kill"
        );

        tokio::time::sleep(SETTLE).await;

        assert!(
            !marker.exists(),
            "{} was created, so the stop command was run with a grace period of zero",
            marker.display()
        );
    }

    /// **The ordinary state of a daemon is unchanged: no total to divide, and the spec is the whole
    /// answer.**
    ///
    /// A `mix service stop mariadb` on a running machine is not a shutdown. Nothing is counting
    /// against it, so there is no deadline for the kill afterwards to have to fit inside and the
    /// tail is subtracted from nothing — the mistake this guards against is a reserve applied
    /// unconditionally, which would quietly take five seconds off every grace period on the machine
    /// and leave a database being killed mid-flush outside a shutdown entirely.
    #[tokio::test]
    async fn without_a_budget_a_specs_grace_period_is_honoured_in_full() {
        let (_home, paths, store) = home(&["mariadb"]).await;

        let grace = Millis::from_secs(1);
        let unanswering = FakeService::new();
        let runner = runner(
            stopped_by_a_command("mariadb", grace, &unanswering),
            &paths,
            &store,
        );

        assert_eq!(
            runner.budget.remaining(),
            None,
            "no shutdown is under way, which is what this test is about"
        );
        assert_eq!(
            runner.grace_for(grace, GONE),
            grace.as_duration(),
            "the tail is subtracted from a total, and outside a shutdown there is no total"
        );

        let began = Instant::now();
        let exit = runner.ask_to_stop(&mut NeverStops, GONE).await;
        let took = began.elapsed();

        assert!(exit.is_none(), "the service ignored the request");
        assert!(
            took >= grace.as_duration(),
            "the spec asked for {grace} and the stop command was given {took:?}"
        );
        assert!(
            took <= grace.as_duration() + SLACK,
            "the spec asked for {grace} and the stop took {took:?}"
        );
    }

    /// **A spent budget still leaves long enough to see a killed survivor go** — the one wait for
    /// which zero is a wrong answer rather than a short one.
    ///
    /// Every other clamp in this file is honest at zero: a grace period of nothing is a service
    /// killed at once, a log drain of nothing is a tail nobody was reading. The poll after the kill
    /// in [`Runner::stop_adopted`] is a *question*, and asked with no window at all it is asked
    /// microseconds after the kill — the kernel has not finished with the process, so a survivor
    /// that stopped exactly as it was told to is written down as one that would not go. What follows
    /// is the defect this pins: the row keeps its `stopping`,
    /// [`Registry::stop_one`](super::super::Registry) reads it and reports the stop as failed, and
    /// the walk stops there on the ordering rule with every service after it still running.
    ///
    /// **And the floor is a window the walk shares, not an allowance per service**, which is the
    /// half that keeps it inside the OS ceiling — asserted here by letting the window pass, after
    /// which a second survivor gets what a spent budget always gave.
    #[tokio::test]
    async fn a_spent_budget_still_leaves_long_enough_to_see_a_killed_survivor_go() {
        let (_home, paths, store) = home(&["mariadb", "redis"]).await;

        let first = runner(
            spec("mariadb").build().expect("a usable spec"),
            &paths,
            &store,
        );
        let mut second = runner(
            spec("redis").build().expect("a usable spec"),
            &paths,
            &store,
        );

        // One shutdown, two services in its plan — which is what makes the window below a shared
        // one rather than two.
        second.budget = first.budget.clone();

        assert_eq!(
            first.seeing_it_go(),
            GONE,
            "nothing is counting yet, so the constant is the whole answer"
        );

        // Already elapsed: the walk reached these two with the whole of its budget spent on the ones
        // before them.
        first.budget.narrow_to(Instant::now());

        assert!(
            first.within_budget(GONE).is_zero(),
            "the budget is spent, which is the case this test is about"
        );
        assert_eq!(
            first.seeing_it_go(),
            GONE_FLOOR,
            "a killed survivor was given no time at all to leave the process table, so the poll \
             after the kill can only answer that it had not"
        );
        assert_eq!(
            second.seeing_it_go(),
            GONE_FLOOR,
            "both survivors are inside the one window the shutdown opened"
        );

        tokio::time::sleep(crate::CONFIRMATION_REPRIEVE + SETTLE).await;

        assert_eq!(
            second.seeing_it_go(),
            Duration::ZERO,
            "the reprieve is a moment the whole walk shares and it has passed; a floor granted \
             again here would be one per service, which is the unbounded term the OS ceiling \
             cannot contain"
        );
    }
}
