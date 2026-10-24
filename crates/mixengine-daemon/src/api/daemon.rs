//! `daemon.status`, `daemon.version`, `daemon.doctor`, `daemon.bundle`,
//! `daemon.set_credential_store` and `daemon.shutdown`: what this daemon says about itself, and how
//! it stops.
//!
//! The ordered stop a shutdown makes is the one `update.apply` and `update.finish` make too, so
//! they call it here.

use mixengine_proto::{
    BundleReport, DaemonShutdown, DaemonStatus, DaemonVersion, Error, ErrorCode, ServiceWalk,
    Uptime,
};

use super::Api;
use super::service::walked;
use crate::error::ToWire as _;

impl Api {
    /// `daemon.doctor` — every check the doctor makes, and the line about where this home keeps its
    /// passwords, which only the API knows: the store is decided in `main` and described from here.
    /// A note and never a problem, so `daemon.doctor_repair` has nothing to read in it.
    pub(super) async fn doctor_report(&self) -> mixengine_proto::DoctorReport {
        let mut report = self.doctor.report().await;
        report.checks.push(crate::credentials::check(
            self.credentials.store,
            self.paths.credentials_file(),
        ));
        report
    }

    /// `daemon.set_credential_store`. Records the store the next start uses; the running daemon
    /// keeps the host it was built with.
    ///
    /// # Errors
    ///
    /// `precondition_failed` with the sentence [`crate::credentials::switch`] gives, or the wire
    /// error of a database that could not be read or written.
    pub(super) async fn set_credential_store(
        &self,
        asked: mixengine_proto::CredentialStoreSet,
    ) -> Result<mixengine_proto::CredentialStoreChange, Error> {
        let release = mixengine_platform::RELEASE;
        let to = crate::credentials::Store::from(asked.store);
        let from = crate::credentials::recorded(&self.store)
            .await?
            .unwrap_or(crate::credentials::Store::default_for(release));

        if from != to {
            crate::credentials::check_switch(
                &self.store,
                self.paths.credentials_file(),
                release,
                std::env::consts::OS,
                from,
                to,
            )
            .await
            .map_err(|why| Error::new(ErrorCode::PreconditionFailed, why))?;
            crate::credentials::record(&self.store, to).await?;
        }

        Ok(mixengine_proto::CredentialStoreChange {
            recorded: to.into(),
            running: self.credentials.store.into(),
        })
    }

    /// `daemon.status` — every fact this build actually has.
    ///
    /// **Fallible since the elevation queue**, and `daemon.version` is what stays infallible: how
    /// many operations are waiting is a read of the queue, and there is no honest number to report
    /// when that read fails. Reporting zero would be the stale-clear failure the queue exists to
    /// prevent — a healthy machine that is missing its hosts entries — and the handshake a client
    /// makes before it trusts anything here is `daemon.version`, which touches nothing.
    ///
    /// # Errors
    ///
    /// The wire error of a queue that could not be read.
    pub(super) async fn status(&self) -> Result<DaemonStatus, Error> {
        Ok(DaemonStatus {
            version: self.version.to_owned(),
            protocol: self.protocol,
            pid: self.pid,
            home: self.home.clone(),
            endpoint: self.endpoint.clone(),
            database: self.database.clone(),
            started_at: self.started.at(),
            uptime: Uptime::from_duration(self.started.elapsed()),
            // Always `Some` from a daemon that has the members at all — they are `Option` for the
            // wire, so an older daemon's answer decodes, and never to say "this build did not
            // manage to find out". A queue that cannot be read fails the call above.
            elevation: Some(self.elevation.summary().await?),
            dns: Some(self.dns.status()),
            update: self.updates.offer().await,
            credentials: Some(mixengine_proto::CredentialsStatus {
                store: self.credentials.store.into(),
                choosable: self.credentials.choosable,
            }),
        })
    }

    /// `daemon.bundle` — one diagnostics archive, written into this home.
    ///
    /// **The status is read here and handed over, its failure included.** `status` is fallible and a
    /// bundle is wanted exactly when things are failing, so a queue that would not answer becomes an
    /// omission inside the archive rather than the end of the call.
    ///
    /// # Errors
    ///
    /// The wire error of an archive that could not be written.
    pub(super) async fn bundle(&self) -> Result<BundleReport, Error> {
        let report = self.doctor_report().await;
        let status = self.status().await;

        self.bundles.take(&report, status, self.version()).await
    }

    /// `daemon.version` — the handshake, cheap enough to answer while everything else is still
    /// coming up.
    pub(super) fn version(&self) -> DaemonVersion {
        DaemonVersion {
            version: self.version.to_owned(),
            protocol: self.protocol,
        }
    }

    /// `daemon.shutdown` — stop every supervised service in reverse dependency order, then stop.
    ///
    /// **The order is the reason this is a method and not a cancelled token**, which is what a signal
    /// already is. A root token cancelled outright stops every runner at once, and a site that is
    /// still serving requests against a database that has gone is exactly what dependency order
    /// exists to prevent — for the whole set it is only a few hundred milliseconds of difference, and
    /// those are the milliseconds a user reads in a log as `connection refused`.
    ///
    /// **The budget is granted before the plan is built and it is the total**, not each service's:
    /// the specs are the services' own statements about what they need, and dividing what is left
    /// among them is `Runner::grace_for`'s, one level down. Nothing here
    /// takes a grace from the caller — a shutdown budget is a property of the machine, and a client
    /// that could ask for thirty seconds could ask for thirty minutes.
    ///
    /// **Asked a second time, it grants nothing at all** — the same escalation `main.rs` performs
    /// when a second console event arrives during a stop that is still going, and for the same
    /// reason: somebody asking again is somebody who has stopped being willing to wait for the
    /// polite stop. What that reaches is every runner that has not begun its stop, which then goes
    /// straight to the kill; a service already inside a grace period keeps the one it was granted,
    /// exactly as it does on the signal path. Two clients asking at the same instant get the same
    /// treatment, which is the cost of not being able to tell them apart from one person asking
    /// twice — the same cost two Ctrl-Cs in quick succession already carry.
    ///
    /// **Without it one platform has no escalation at all.** Every console control event needs a
    /// console, and a `--detach`ed daemon on Windows — the one `mix` autostarts — has none, so this
    /// method is the only thing that can ask it anything. `Registry::stopping_within` narrows and
    /// never extends, so a second request that passed the configured grace again would be discarded
    /// by its own `min` and change nothing.
    ///
    /// **The token is cancelled after the walk and before this answers**, which is the ordering the
    /// whole method rests on. Cancelling first would stop the services out of order through the
    /// signal path, and answering first would mean writing a response into a connection this daemon
    /// is about to stop waiting for. What the client then sees is the answer, followed by the
    /// connection closing — which *is* the shutdown, not a failure of it.
    ///
    /// **It is cancelled by a guard and not by a statement, because this future is not guaranteed to
    /// reach one.** A client that goes away mid-walk takes the handler with it, and the first thing
    /// done here latches the registry shut for good — so a cancellation that lived on a line at the
    /// end would be skipped on exactly the paths that leave a daemon nobody can start anything with.
    /// See [`Going`](super::Going). Both facts above still hold: it drops after the walk, and the
    /// answer is encoded by the caller.
    ///
    /// **A daemon that cannot say what it declares still stops.** An `Undeclarable` here is a
    /// `extension.toml` somebody is in the middle of editing, and refusing to shut down over it
    /// would leave them with a daemon they can only kill. It is reported, the ordered walk is
    /// skipped, and the cancellation on the way out stops every runner the untidy way — which is
    /// what a signal would have done anyway.
    ///
    /// **Reported to the client and not only to `daemon.log`**, which is what
    /// [`DaemonShutdown::unordered`] is for: the walk that goes out in that case is empty and
    /// complete, and a client reading that alone cannot tell a skipped order from a home with
    /// nothing to stop. What it carries is the wire error [`ToWire`](crate::error::ToWire) already
    /// writes for these same declarations, so `mix daemon stop` and `mix service list` say the same
    /// sentence about the same half-written file rather than two.
    pub(super) async fn daemon_shutdown(&self) -> DaemonShutdown {
        // Read before anything here latches it, or every shutdown would look like a second one.
        let asked_again = self.services.is_shutting_down();

        // Taken before the registry is latched, so that from the moment this daemon refuses to start
        // anything there is no way of leaving it running.
        let _going = self.shutdown.begun();

        self.services.stopping_within(match asked_again {
            true => std::time::Duration::ZERO,
            false => self.shutdown.grace(),
        });

        let (services, unordered) = match self.stop_everything().await {
            Ok(walk) => (walk, None),

            Err(error) => {
                tracing::warn!(
                    %error,
                    "cannot work out the order to stop services in; stopping them all at once"
                );

                (
                    ServiceWalk {
                        planned: Vec::new(),
                        complete: true,
                        reached: Vec::new(),
                        failed: None,
                        blocked: Vec::new(),
                    },
                    Some(error),
                )
            }
        };

        tracing::info!(
            stopped = services.reached.len(),
            refused = services
                .failed
                .as_ref()
                .map(|failure| failure.service.as_str()),
            ordered = unordered.is_none(),
            // The only record that a stop was cut short, and the sentence somebody reads when they
            // go looking for why a database recovered on its next start.
            hurried = asked_again,
            "a client asked this daemon to stop"
        );

        DaemonShutdown {
            services,
            unordered,
        }
    }

    /// The ordered half of [`Api::daemon_shutdown`]: every declared service, dependents first.
    ///
    /// Separate so that the failure to build a plan is a value the caller can go on from rather than
    /// a `?` that would take the shutdown with it.
    pub(super) async fn stop_everything(&self) -> Result<ServiceWalk, Error> {
        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;
        let plan = graph.stop_order();
        let planned = plan.flat().cloned().collect();

        // **Not the same word a person's `service.stop` leaves.** Somebody asked for the *daemon*
        // to stop, which is not a sentence about any one service, and a row that recorded it as one
        // would keep every service unwakeable until somebody started it by hand.
        Ok(walked(
            planned,
            self.services
                .stop_because(&plan, &mixengine_proto::StateReason::Shutdown)
                .await,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use mixengine_proto::rpc;
    use mixengine_proto::{Millis, ServiceState, StopBehaviour};
    use mixengine_testkit::FakeService;
    use serde_json::Value;

    use super::*;
    use crate::api::test_daemon::{call, daemon, undeclared, web_and_db};
    use crate::services::fixture;

    #[tokio::test]
    async fn status_answers_with_what_the_daemon_knows_about_itself() {
        let answer = call(r#"{"jsonrpc":"2.0","method":"daemon.status","id":1}"#).await;

        assert_eq!(answer["jsonrpc"], "2.0");
        assert_eq!(answer["id"], 1);
        assert_eq!(answer["result"]["version"], "0.1.0");
        assert_eq!(answer["result"]["pid"], 4123);
        assert_eq!(answer["result"]["protocol"], 1);
        assert!(answer["result"]["endpoint"].is_string());
    }

    /// One service that ignores a request to stop, with a stop command that never answers.
    ///
    /// A minute of grace it can never use: whatever it actually spends stopping is the grace period
    /// the budget allowed it, which is what makes a stop's *length* the readable thing it is here.
    fn slow_to_stop(id: &str) -> Arc<fixture::Declared> {
        Arc::new(fixture::Declared(vec![
            fixture::spec(id)
                .args(fixture::arguments(&FakeService::new().ignoring_stop()))
                .stop(StopBehaviour::Command {
                    program: FakeService::program(),
                    args: fixture::arguments(&FakeService::new()),
                    grace: Millis::from_secs(60),
                })
                .build()
                .expect("a usable spec"),
        ]))
    }

    /// `daemon.status` carries the offer, and carries nothing when there is none — which is every
    /// daemon that has not managed to read a feed.
    #[tokio::test]
    async fn the_status_line_says_nothing_about_updates_until_there_is_something_to_say() {
        let daemon = daemon(web_and_db(), &[]).await;

        let answer = daemon.ask(rpc::method::DAEMON_STATUS, Value::Null).await;

        assert!(answer["result"].get("update").is_none(), "{answer}");

        daemon.quiet().await;
    }

    /// `daemon.shutdown`'s whole claim, in order: services down, then the token, then the answer.
    #[tokio::test]
    async fn shutting_down_stops_the_services_before_it_cancels_anything() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let _: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, Value::Null).await;
        assert_eq!(daemon.state("web").await, Some(ServiceState::Running));

        let shutdown: DaemonShutdown = daemon
            .expect(rpc::method::DAEMON_SHUTDOWN, Value::Null)
            .await;

        assert_eq!(
            shutdown.services.planned,
            vec![fixture::service("web"), fixture::service("db")],
            "dependents first — the same walk `service.stop` does, and not the root token's \
             everything-at-once: {shutdown:?}"
        );
        assert_eq!(shutdown.services.reached, shutdown.services.planned);
        assert!(shutdown.services.complete);
        assert!(shutdown.services.failed.is_none(), "{shutdown:?}");
        assert!(
            shutdown.unordered.is_none(),
            "the order was kept, and a note about one that was not is a note nobody may print: \
             {shutdown:?}"
        );

        // The rows are what say the stop was performed rather than merely planned, and they were
        // written before this answer existed: cancelling first and reporting afterwards would have
        // produced the same struct with the services still going.
        assert_eq!(daemon.state("db").await, Some(ServiceState::Stopped));
        assert_eq!(daemon.state("web").await, Some(ServiceState::Stopped));

        assert!(
            daemon.api.shutdown.token().is_cancelled(),
            "the accept loop is what actually ends the process, and this is what tells it to"
        );
    }

    /// **A shutdown that was begun is finished by the daemon, whatever becomes of whoever asked.**
    ///
    /// The handler is a future hyper holds, and hyper is built here with its default `half_close`,
    /// which closes a connection the moment a read sees end of file rather than waiting for a
    /// response nobody is left to receive. So a `mix daemon stop` that is interrupted — Ctrl-C, the
    /// GUI window closing, the client killed — drops this future where it stands. So does a panic
    /// anywhere inside the walk.
    ///
    /// What that used to leave is the worst state this daemon has: `Registry::stopping_within` has
    /// already latched `shutting_down`, which is never cleared, so every `service.start` from that
    /// moment on is refused — and the token that ends the process is cancelled on a line the future
    /// never reached. A live daemon, its services stopped, starting nothing, for as long as nobody
    /// notices.
    #[tokio::test]
    async fn a_shutdown_whose_client_went_away_still_takes_the_daemon_with_it() {
        use std::future::Future as _;

        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let _: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, Value::Null).await;
        assert_eq!(daemon.state("web").await, Some(ServiceState::Running));

        {
            // Polled by hand rather than raced against a timer: one poll gets past the point where
            // the shutdown has committed itself and cannot get past the first thing it waits on,
            // which makes "dropped in flight" the fact this test rests on rather than a hope about
            // scheduling. `#[tokio::test]` is a current-thread runtime, so nothing else runs while
            // this poll does and the future cannot finish underneath it.
            let mut shutting = std::pin::pin!(daemon.api.daemon_shutdown());
            let mut polling = std::task::Context::from_waker(std::task::Waker::noop());

            assert!(
                shutting.as_mut().poll(&mut polling).is_pending(),
                "the whole shutdown answered in a single poll, so this test drops nothing and \
                 proves nothing"
            );
        }

        assert!(
            daemon.api.shutdown.token().is_cancelled(),
            "the shutdown latched this daemon shut and then left it running: the accept loop is \
             waiting on a token nothing is going to cancel now"
        );

        // The other half of that state, and the reason the first assertion is worth making: the
        // latch is permanent, so a daemon left like this is one that refuses every start it is ever
        // asked for again. Whoever typed `mix daemon stop` reads the refusal, not the cause.
        let refused: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, Value::Null).await;
        assert!(
            refused.failed.is_some(),
            "a daemon that has begun shutting down starts nothing, which is what makes leaving one \
             running the failure it is: {refused:?}"
        );

        // Waited for by hand, because the drop above landed inside `web`'s stop: `stop_one` takes an
        // entry out of the map before it awaits the task, so the runner it cancelled is one nothing
        // is left holding. It still stops — that is what the cancellation did — and this is what
        // gives it the chance to, since `quiet` below can only drain what is still registered and a
        // `Home` is a temporary directory a live process would keep from being removed.
        daemon.until("web", ServiceState::Stopped).await;
        daemon.quiet().await;
    }

    #[tokio::test]
    async fn a_daemon_with_nothing_declared_still_shuts_down() {
        let daemon = undeclared().await;

        let shutdown: DaemonShutdown = daemon
            .expect(rpc::method::DAEMON_SHUTDOWN, Value::Null)
            .await;

        assert!(shutdown.services.planned.is_empty(), "{shutdown:?}");
        assert!(shutdown.services.complete);
        assert!(
            shutdown.unordered.is_none(),
            "nothing to stop is not a failure to work out how to stop it, and the two answer the \
             same empty walk: {shutdown:?}"
        );
        assert!(daemon.api.shutdown.token().is_cancelled());
    }

    /// A declaration nobody can read is a reason to say so, and not a reason to stay running.
    #[tokio::test]
    async fn a_daemon_that_cannot_say_what_it_declares_still_shuts_down() {
        let daemon = daemon(Arc::new(fixture::Unavailable), &[]).await;

        // Not an error: `service.list` answers `internal` for this same source, because there the
        // question *was* about the services. Here it is about the daemon, and refusing would leave
        // whoever is editing an extension with a daemon they can only kill.
        let shutdown: DaemonShutdown = daemon
            .expect(rpc::method::DAEMON_SHUTDOWN, Value::Null)
            .await;

        assert!(
            shutdown.services.planned.is_empty(),
            "no order could be worked out, and the walk says so rather than claiming one: \
             {shutdown:?}"
        );

        // The half that used to be a line in `daemon.log` and nothing else. Without it this answer
        // is the previous test's — an empty, complete walk — and whoever typed `mix daemon stop`
        // would be told the ordering happened when every runner was cancelled at the same moment.
        let why = shutdown
            .unordered
            .as_ref()
            .unwrap_or_else(|| panic!("the skipped order is reported: {shutdown:?}"));

        assert_eq!(why.code, ErrorCode::NotFound);
        assert!(
            why.message.contains("no such package"),
            "the source's own complaint, which is what `service.list` would have said about the \
             same declarations: {why:?}"
        );

        assert!(
            daemon.api.shutdown.token().is_cancelled(),
            "what stops the services then is the root token, which is what a signal does anyway"
        );
    }

    #[tokio::test]
    async fn shutting_down_takes_no_parameters() {
        let daemon = undeclared().await;

        // A client that passed a grace period believes it chose one. The budget is the machine's,
        // from `config.toml`, and a method that ignored the argument would be a client quietly not
        // getting what it asked for.
        let answer = daemon
            .ask(
                rpc::method::DAEMON_SHUTDOWN,
                serde_json::json!({"grace_seconds": 30}),
            )
            .await;

        assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
        assert!(!daemon.api.shutdown.token().is_cancelled(), "{answer}");
    }

    /// **Asking again is asking to hurry**, which is what a second signal already means and what a
    /// second `daemon.shutdown` had no way of saying.
    ///
    /// `main.rs` treats a second console event as the person no longer being willing to wait for the
    /// polite stop: it narrows the budget to nothing, so every runner that has not begun its stop
    /// goes straight to the kill. A second request over the API could not do that — `stopping_within`
    /// only ever narrows, and `now + grace` is always *later* than the deadline the first request
    /// set, so the `min` discarded it and the second caller simply queued behind the first.
    ///
    /// **Which left one platform with no way out at all.** A `--detach`ed daemon on Windows has no
    /// console for any of the five events to arrive on — `mix` autostarts exactly that daemon — so
    /// the API is the only thing that can ask it anything, and asking twice did nothing. The escape
    /// from a shutdown wedged on one service was Task Manager.
    ///
    /// The first shutdown is staged as the latch alone rather than as a walk racing this one: what
    /// is under test is what the second request *grants*, and a competing walk would put its own
    /// claims in the way of measuring it.
    ///
    /// **`Command` and not `Signal`, for the reason every other budget test here gives**: Windows
    /// sends no request to stop at all (ADR 0008), so a `Signal` spec spends no grace period there
    /// and this would pass without the escalation existing — green on the one system that needs it
    /// most.
    #[tokio::test]
    async fn a_second_request_to_stop_is_the_hurry_a_second_signal_would_have_been() {
        let daemon = daemon(slow_to_stop("db"), &["db"]).await;

        let _: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, Value::Null).await;
        assert_eq!(daemon.state("db").await, Some(ServiceState::Running));

        // A shutdown already under way, and one whose budget is no help to anybody: ten minutes is
        // the most `config.toml` accepts, so this is the slowest a first request can legitimately be.
        daemon
            .services
            .stopping_within(std::time::Duration::from_secs(600));

        let began = Instant::now();
        let shutdown: DaemonShutdown = daemon
            .expect(rpc::method::DAEMON_SHUTDOWN, Value::Null)
            .await;
        let took = began.elapsed();

        assert_eq!(
            shutdown.services.reached,
            vec![fixture::service("db")],
            "the service still stopped and the answer still says so — an escalation is a shorter \
             stop, not a skipped one: {shutdown:?}"
        );

        // The stop command never returns, so what this measures is the grace period the second
        // request granted. Escalated it is nothing and the service is killed at once; unescalated it
        // is this daemon's own budget less the drain — eight seconds — and the margin between the
        // two is the whole assertion.
        assert!(
            took < std::time::Duration::from_secs(4),
            "the second request to stop waited {took:?} for a service whose stop command never \
             answers; asking again is what says the polite stop is over"
        );

        assert!(daemon.api.shutdown.token().is_cancelled());
    }
}
