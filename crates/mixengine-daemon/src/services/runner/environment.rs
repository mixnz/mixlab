//! The surroundings a service's commands run in: the working directory and environment a
//! spawn, a stop command or a reload command is handed, and the keyring reads that fill it,
//! each bounded by a deadline a start or a shutdown can afford.

use std::collections::BTreeMap;
use std::sync::Arc;

use mixengine_proto::{EnvValue, ServiceId};
use mixengine_supervisor::Surroundings;

use super::{ENVIRONMENT, OnFailure, Resolved, Runner, START_ENVIRONMENT};

impl Runner {
    /// Where this service's own commands are run — its directory, and the environment it is running
    /// with.
    ///
    /// The cached one whenever there is one, which is every life of a process this daemon spawned.
    /// **A service it adopted has none**, and that is the case worth spelling out: the environment
    /// was built by a daemon that is gone, so a stop command for a survivor resolves one here — once,
    /// at the moment it is stopped, which is the only moment an adopted service runs a command at
    /// all.
    ///
    /// An environment that cannot be resolved is not a reason to skip the stop: a spec whose
    /// credential the keyring no longer holds still names a `mariadb-admin shutdown` that is far
    /// better than a kill, and the alternative to trying it with what is available is a database
    /// recovering on its next start. It is said once, in `daemon.log`, and never in the event.
    ///
    /// **What is dropped is the entry that failed, and only that entry.** The whole environment
    /// would be the wrong thing to throw away for one unreadable credential: a `mariadb-admin`
    /// run without the `HOME` the spec declares as a literal cannot find its defaults file or its
    /// socket, so a stop that was meant to survive a locked keyring would fail a second time for a
    /// reason nobody chose.
    ///
    /// **And an environment that will not *arrive* is the same answer**, which is why [`ENVIRONMENT`]
    /// bounds the read. The uncached path is only ever taken while a service is being stopped, and a
    /// keyring that is waiting for somebody to type a password would otherwise hold a `mix stop` —
    /// or a whole daemon shutdown — open indefinitely. Giving up on the read leaves the blocking task
    /// where it is, still waiting on the store; what it does not do is make the stop wait with it —
    /// and the literals are still known here, without the task that has stopped answering.
    ///
    /// **Bounded by whichever of that and the shutdown's remainder is smaller**, because "only ever
    /// taken while a service is being stopped" is the whole reason: this is a wait a stop makes, it
    /// is per service, and it is spent *before* the grace period the budget was already dividing. A
    /// daemon stopping three adopted services against a locked keyring spent nine seconds here that
    /// no budget had agreed to, which on Windows is the process being terminated in the middle of
    /// the WAL checkpoint it stopped its services in order to make. Zero is a real answer and means
    /// the literals, which is what the arm below already does with a read that did not finish.
    pub(super) async fn where_commands_run(&self) -> Surroundings {
        if let Some(place) = &self.surroundings {
            return place.clone();
        }

        let literals = || {
            self.spec
                .env()
                .iter()
                .filter_map(|(name, value)| match value {
                    EnvValue::Literal { value } => Some((name.clone(), value.clone())),
                    EnvValue::Keyring { .. } => None,
                })
                .collect::<BTreeMap<String, String>>()
        };

        // **Inside the shutdown's budget and not beside it** — see `Runner::within_budget`, which is
        // why this is not `ENVIRONMENT` outright. It is the one wait a stop makes *before* its grace
        // period, and it is paid by every service this daemon adopted: those are the runners a spawn
        // never filled `surroundings` in for, so a whole-daemon shutdown resolves their environment
        // here, once each, three seconds at a time against a Windows budget of two and a half.
        let env = match tokio::time::timeout(
            self.within_budget(ENVIRONMENT),
            self.walk_environment(OnFailure::Record),
        )
        .await
        {
            Ok(Ok((env, failed))) => {
                for (name, error) in failed {
                    tracing::warn!(
                        service = self.spec.id().as_str(),
                        entry = name.as_str(),
                        error = %error,
                        "cannot resolve one entry of the environment this service is running with; \
                         its own commands will be run without that entry"
                    );
                }

                env
            }

            Ok(Err(error)) => {
                tracing::warn!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "cannot resolve the environment this service is running with; its own commands \
                     will be run with the entries the spec states outright"
                );

                literals()
            }

            Err(_) => {
                tracing::warn!(
                    service = self.spec.id().as_str(),
                    after = ?ENVIRONMENT,
                    "the environment this service is running with did not resolve in time; a \
                     locked OS keyring is the usual reason; its own commands will be run with the \
                     entries the spec states outright"
                );

                literals()
            }
        };

        Surroundings::new(self.spec.cwd(), env)
    }

    /// The environment the child is given: the spec's, with every named credential fetched.
    ///
    /// A named credential that is not there fails the start rather than being passed as an empty
    /// string: a MariaDB started with no root password is a worse outcome than one that did not
    /// start. The first entry that would not resolve is the error, named — the rest are neither
    /// worth listing nor worth asking for, which is why the walk stops there ([`OnFailure::Stop`]).
    ///
    /// **An environment that will not *arrive* is an environment that is not there**, which is why
    /// [`START_ENVIRONMENT`] bounds this. The answer is the same one a missing credential gets and
    /// for the same reason: what the spec asked for is not available, so the process is not
    /// started. It reaches the user as [`StateReason::SpawnFailed`], which says precisely that, and
    /// the reason it did not arrive is named in `daemon.log` where a locked keyring can be acted on.
    ///
    /// **[`None`] is a stop that arrived first.** The runner's cancellation is waited on beside the
    /// read, so a `mix stop` or a daemon shutdown is not held for as long as a person takes to
    /// answer the OS's dialog. The caller owes that stop its `Stopped`, as a backoff does.
    ///
    /// Giving up on the read leaves the blocking task where it is, still waiting on the store — see
    /// [`Runner::where_commands_run`], which pays the same price for the same reason. What it does
    /// not do is make a whole daemon's shutdown wait with it.
    ///
    /// **And the next attempt joins that task rather than starting another**, which is the whole of
    /// why this takes `&mut self` — see [`Runner::reading`]. A start is retried, so a price the stop
    /// path pays once per service was being paid once per attempt here, and against a keyring that
    /// never answers that is tokio's blocking pool filling up with reads nobody is waiting for.
    ///
    /// [`StateReason::SpawnFailed`]: mixengine_proto::StateReason::SpawnFailed
    pub(super) async fn environment(&mut self) -> anyhow::Result<Option<BTreeMap<String, String>>> {
        // Taken rather than borrowed: whichever arm below runs, this handle is either finished with
        // or put back, and a `take` is what stops a `?` in between leaving a live read behind.
        let mut reading = match self.reading.take() {
            Some(reading) => reading,
            None => self.spawn_environment(OnFailure::Stop),
        };

        // `&mut` so that the timeout drops the borrow and not the handle — the read this gives up on
        // is the one the next attempt is going to wait for.
        let walked = tokio::select! {
            biased;

            () = self.cancel.cancelled() => {
                self.reading = Some(reading);

                return Ok(None);
            }

            walked = tokio::time::timeout(START_ENVIRONMENT, &mut reading) => walked,
        };

        let (env, failed) = match walked {
            Ok(joined) => joined?,

            // Worded to stand on its own: the caller logs this with `%error`, which prints the top
            // of the chain and not the chain.
            Err(_) => {
                self.reading = Some(reading);

                anyhow::bail!(
                    "no answer within {START_ENVIRONMENT:?}; a locked OS keyring, or a keyring \
                     dialog nobody answered, is the usual reason"
                )
            }
        };

        match failed.into_iter().next() {
            // **The cause in the message itself**, because the caller logs this with `%error`,
            // which prints the top of a chain and not the chain — a `.context` here hid *why* from
            // `daemon.log`.
            Some((name, error)) => Err(anyhow::anyhow!("the environment entry {name}: {error:#}")),
            None => Ok(Some(env)),
        }
    }

    /// Walk the spec's environment: every entry that resolves, and the error of each that did not.
    ///
    /// Off the runtime because a keyring read blocks — on Linux on a D-Bus round trip to a daemon
    /// that may be prompting the user to unlock it.
    ///
    /// `on_failure` is what lets the two callers differ: a start refuses anything less than the
    /// whole environment and so has nothing to gain from the entries past the first failure, a stop
    /// command runs with whatever there is. The [`Err`] here is neither — it is the blocking task
    /// itself not finishing, which says nothing about any entry.
    ///
    /// The awaiting half of [`Runner::spawn_environment`], for the caller that has nowhere to keep a
    /// read it gave up on: [`Runner::where_commands_run`] is reached once per stop of one service,
    /// where the start path is reached once per attempt.
    async fn walk_environment(&self, on_failure: OnFailure) -> anyhow::Result<Resolved> {
        Ok(self.spawn_environment(on_failure).await?)
    }

    /// Start the walk, without waiting for it.
    ///
    /// Split out from [`Runner::walk_environment`] for the one caller that has to hold the handle
    /// across a timeout it may not survive — see [`Runner::reading`]. The task itself is the same
    /// either way, and so is what abandoning it costs.
    fn spawn_environment(&self, on_failure: OnFailure) -> tokio::task::JoinHandle<Resolved> {
        let named: Vec<(String, EnvValue)> = self
            .spec
            .env()
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();

        let host = Arc::clone(&self.host);
        let own = self.spec.id().clone();

        tokio::task::spawn_blocking(move || {
            let mut env = BTreeMap::new();
            let mut failed = Vec::new();

            for (name, value) in named {
                let resolved = match value {
                    EnvValue::Literal { value } => Ok(value),

                    // **Through `crate::secrets` and not the keyring directly.** This is the read
                    // that hands a server its own superuser password at spawn, so it is the one
                    // that has to find an entry written before addresses named their home —
                    // otherwise upgrading a build would leave every database unable to authenticate
                    // against itself.
                    EnvValue::Keyring { service, key } => {
                        { crate::secrets::secret_blocking(host.as_ref(), &service, &key) }
                            .map_err(anyhow::Error::from)
                            .and_then(|secret| {
                                secret.ok_or_else(|| {
                                    // What the store holds around the miss, for `daemon.log` only:
                                    // the sentence on the row stays the one a person acts on.
                                    tracing::warn!(
                                        service = own.as_str(),
                                        address = %format!("{service}/{key}"),
                                        store = %crate::secrets::after_a_miss(
                                            host.as_ref(),
                                            &service,
                                            &key
                                        ),
                                        "a credential this service is started with was not found"
                                    );

                                    anyhow::anyhow!(
                                        "no credential is stored at {service}/{key}{}",
                                        repair_for(&own, &key)
                                    )
                                })
                            })
                    }
                };

                match resolved {
                    Ok(value) => {
                        env.insert(name, value);
                    }

                    Err(error) => {
                        failed.push((name, error));

                        if matches!(on_failure, OnFailure::Stop) {
                            break;
                        }
                    }
                }
            }

            (env, failed)
        })
    }
}

/// The second half of a missing-credential sentence, for the one entry
/// `mix service reset-credential` can write back.
///
/// **This service's own entry, by its address.** A recipe's ritual declares one secret of its own
/// (`root`, `postgres`), at `<home>/<this service>/<account>`; every other `Keyring` entry a spec
/// names is another service's — phpMyAdmin reading its database's superuser — and
/// `reset-credential` on *this* service would be refused. The address is the rule because the
/// runner holds the spec and not the recipe, and the spec's id is in every address of its own.
fn repair_for(own: &ServiceId, key: &str) -> String {
    let names_itself = mixengine_core::services::handoff::secret_key_before_homes(key)
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(service, _)| service == own.as_str());

    if !names_itself {
        return String::new();
    }

    format!(
        "; `mix service reset-credential {own}` generates one for this home and writes it into \
         the data directory, keeping every database"
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::Instant;

    use super::super::super::fixture::{home, spec};
    use super::super::tests::{SLACK, adopted_runner};
    use super::*;

    /// **The environment a stop reads is inside the shutdown's budget, like every other wait a stop
    /// makes** — the third of them, and the one the first implementation left out.
    ///
    /// `FLUSH` and `GONE` are clamped by [`Runner::within_budget`] because a constant charged after
    /// the budget is a constant the budget does not bound. This read is the same thing and was not:
    /// it is paid per service, on the stop path, by every adopted service, and `ENVIRONMENT` is
    /// three seconds — so on Windows, where the whole signalled budget is two and a half and the
    /// slack over the WAL checkpoint is one, a single locked keyring put the daemon past the clock
    /// the OS is running and the checkpoint is what it lost.
    ///
    /// A locked store rather than a missing one, because they are different failures: a missing
    /// store answers at once and no deadline is ever reached.
    #[tokio::test]
    async fn an_environment_a_stop_waits_for_is_bounded_by_what_the_shutdown_has_left() {
        let (_home, paths, store) = home(&["mariadb"]).await;

        // Longer than `ENVIRONMENT` so that what ends this read is always a deadline and never the
        // store — whichever deadline it turns out to be is then the whole of what is measured — and
        // no longer than it has to be, because dropping a runtime waits for the blocking task this
        // sleep is on and every second past the deadline is a second added to the test.
        let never = ENVIRONMENT + Duration::from_secs(1);
        let left = Duration::from_millis(500);

        let runner = adopted_runner(
            spec("mariadb")
                .env("MARIADB_DATA", "/var/lib/mysql")
                .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb@main/root")
                .build()
                .expect("a usable spec"),
            &paths,
            &store,
            never,
        );

        runner.budget.narrow_to(Instant::now() + left);

        let began = Instant::now();
        let place = runner.where_commands_run().await;
        let took = began.elapsed();

        assert!(
            took < Duration::from_secs(2),
            "the read was given {took:?} by a shutdown with {left:?} left to spend; unclamped it is \
             {ENVIRONMENT:?}, once per service, after which the budget is already over"
        );

        // What it does about giving up is unchanged and is the point of giving up at all: the stop
        // goes on with the entries the spec states outright, because a `mariadb-admin shutdown`
        // short one entry still beats a kill. `Surroundings` redacts its values and prints its keys,
        // which is exactly as much as this needs to say.
        let resolved = format!("{place:?}");
        assert!(
            resolved.contains("MARIADB_DATA"),
            "the entries the spec states outright are what a stop carries on with: {resolved}"
        );
        assert!(
            !resolved.contains("MARIADB_ROOT_PASSWORD"),
            "the keyring never answered, so there is nothing under this name to have carried: \
             {resolved}"
        );
    }

    /// The same read, outside a shutdown: nothing is counting, so it gets its whole deadline.
    ///
    /// The mistake this guards against is a clamp applied unconditionally, which would quietly take
    /// a shutdown's arithmetic into a machine that is not shutting down — a health probe's
    /// environment, resolved once per life of a process, cut short by a budget that does not exist.
    #[tokio::test]
    async fn without_a_budget_the_environment_read_keeps_its_own_deadline() {
        let (_home, paths, store) = home(&["mariadb"]).await;

        let runner = adopted_runner(
            spec("mariadb")
                .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb@main/root")
                .build()
                .expect("a usable spec"),
            &paths,
            &store,
            ENVIRONMENT + Duration::from_secs(1),
        );

        assert_eq!(
            runner.budget.remaining(),
            None,
            "no shutdown is under way, which is what this test is about"
        );

        let began = Instant::now();
        let _ = runner.where_commands_run().await;
        let took = began.elapsed();

        assert!(
            took >= ENVIRONMENT,
            "the read was cut off after {took:?} with nothing counting against it"
        );
        assert!(
            took < ENVIRONMENT + SLACK,
            "the read ran past its own deadline: {took:?}"
        );
    }

    /// **A start that gave up on a keyring read joins it next time instead of starting another.**
    ///
    /// [`START_ENVIRONMENT`] bounds the read and cannot abort it: the walk is a blocking task,
    /// `spawn_blocking` has no cancellation, and dropping the handle only stops *this* task waiting.
    /// One parked thread is what the bound was worth paying; one per attempt is not — a start is
    /// retried by every restart the policy grants and by every `service.start` a client sends, so a
    /// keyring that never answers filled tokio's blocking pool, at which point [`Runner::kill`] is
    /// also a blocking task and the daemon could no longer stop anything.
    ///
    /// **Asserted by the clock rather than by counting threads**, which is what the mock keyring can
    /// support: the store answers a second after the first attempt has given up, so an attempt that
    /// joined the read in flight hears it a second later and one that started a fresh read waits the
    /// whole of `START_ENVIRONMENT` again. The gap between those two is the test.
    #[tokio::test]
    async fn a_start_that_gave_up_on_a_keyring_read_does_not_start_a_second_one() {
        let (_home, paths, store) = home(&["mariadb"]).await;

        // Longer than `START_ENVIRONMENT`, so the first attempt always ends at its deadline; and only just,
        // so the answer lands while the second attempt is waiting for it.
        let answers_after = START_ENVIRONMENT + Duration::from_secs(1);

        let mut runner = adopted_runner(
            spec("mariadb")
                .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb@main/root")
                .build()
                .expect("a usable spec"),
            &paths,
            &store,
            answers_after,
        );

        let first = runner
            .environment()
            .await
            .expect_err("the keyring has not answered yet");

        assert!(
            format!("{first}").contains("no answer within"),
            "the first attempt ended at its own deadline, which is what leaves a read behind: \
             {first}"
        );
        assert!(
            runner.reading.is_some(),
            "the read this attempt gave up on is the one the next attempt has to join"
        );

        let began = Instant::now();
        let second = runner
            .environment()
            .await
            .expect_err("the mock keyring holds no credential under that name");
        let took = began.elapsed();

        assert!(
            took < START_ENVIRONMENT,
            "the second attempt waited {took:?}, which is a read of its own: the one already in \
             flight answers {:?} after the first attempt gave up",
            answers_after - START_ENVIRONMENT
        );
        assert!(
            format!("{second}").contains("MARIADB_ROOT_PASSWORD"),
            "what came back is the store's answer about the entry, not a second deadline: {second}"
        );
        assert!(
            runner.reading.is_none(),
            "the read finished, so there is nothing left for a third attempt to join"
        );
    }

    /// **A missing credential says so in the line the log prints.**
    ///
    /// The caller logs the error with `%error`, which prints the top of an `anyhow` chain and not
    /// the chain. With the cause behind a `.context`, `daemon.log` read *"the environment entry
    /// MIXENGINE_DB_PASSWORD"* and nothing else, and a phpMyAdmin that could not find its database's
    /// password failed to start without a word about why.
    #[tokio::test]
    async fn a_missing_credential_is_named_in_the_error_a_log_prints() {
        let (_home, paths, store) = home(&["mariadb"]).await;

        let mut runner = adopted_runner(
            spec("mariadb")
                .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb@main/root")
                .build()
                .expect("a usable spec"),
            &paths,
            &store,
            Duration::ZERO,
        );

        let error = runner
            .environment()
            .await
            .expect_err("the mock keyring holds no credential under that name");

        let printed = format!("{error}");
        assert!(printed.contains("MARIADB_ROOT_PASSWORD"), "{printed}");
        assert!(
            printed.contains("no credential is stored at mixengine/mariadb@main/root"),
            "what `%error` prints has to say why, not only which entry: {printed}"
        );
    }

    /// The entry that is this service's own — the one `mix service reset-credential` can write back
    /// — names the repair in the same sentence, so Sites, `mix service list` and the start error
    /// all carry it with no second lookup.
    #[tokio::test]
    async fn a_missing_credential_of_the_services_own_names_the_repair() {
        let (_home, paths, store) = home(&["mariadb"]).await;

        let mut runner = adopted_runner(
            spec("mariadb")
                .env_from_keyring(
                    "MARIADB_ROOT_PASSWORD",
                    "mixengine",
                    "0123456789ab/mariadb/root",
                )
                .build()
                .expect("a usable spec"),
            &paths,
            &store,
            Duration::ZERO,
        );

        let error = runner
            .environment()
            .await
            .expect_err("the mock keyring holds no credential under that name");

        let printed = format!("{error}");
        assert!(
            printed.contains("no credential is stored at mixengine/0123456789ab/mariadb/root"),
            "{printed}"
        );
        assert!(
            printed.contains("`mix service reset-credential mariadb` generates one for this home"),
            "{printed}"
        );
    }

    /// Review focus 5. Another service's entry — phpMyAdmin reading its database's root — is not
    /// one `reset-credential php-fpm@phpmyadmin` could write, so the sentence stops at the address.
    #[tokio::test]
    async fn a_missing_credential_of_another_service_names_no_repair() {
        let (_home, paths, store) = home(&["mariadb"]).await;

        let mut runner = adopted_runner(
            spec("mariadb")
                .env_from_keyring(
                    "MIXENGINE_DB_PASSWORD",
                    "mixengine",
                    "0123456789ab/php-fpm@phpmyadmin/root",
                )
                .build()
                .expect("a usable spec"),
            &paths,
            &store,
            Duration::ZERO,
        );

        let error = runner
            .environment()
            .await
            .expect_err("the mock keyring holds no credential under that name");

        let printed = format!("{error}");
        assert!(printed.contains("no credential is stored at"), "{printed}");
        assert!(!printed.contains("reset-credential"), "{printed}");
    }

    /// **A stop does not wait for a start's keyring read.** [`START_ENVIRONMENT`] is long enough for
    /// a person to answer the OS's dialog, which is too long for a `mix stop` or a shutdown to sit
    /// behind; the runner's cancellation is what ends the wait, and the read is kept for the next
    /// attempt to join.
    #[tokio::test]
    async fn a_stop_ends_a_start_still_waiting_on_the_keyring() {
        let (_home, paths, store) = home(&["mariadb"]).await;

        let mut runner = adopted_runner(
            spec("mariadb")
                .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb@main/root")
                .build()
                .expect("a usable spec"),
            &paths,
            &store,
            START_ENVIRONMENT + Duration::from_secs(5),
        );

        let cancel = runner.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            cancel.cancel();
        });

        let began = Instant::now();
        let env = runner
            .environment()
            .await
            .expect("a stop is not a failure to resolve");
        let took = began.elapsed();

        assert!(
            env.is_none(),
            "the stop arrived first, so nothing was resolved"
        );
        assert!(
            took < START_ENVIRONMENT,
            "the start waited {took:?} after a stop at 200ms; the stop should have ended it"
        );
        assert!(
            runner.reading.is_some(),
            "the read the stop interrupted is the one the next attempt has to join"
        );
    }
}
