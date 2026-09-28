//! Deciding that a service has nothing to do — roadmap task **T69**.
//!
//! The reading is [`mixengine_supervisor::idle`]'s, on the reasoning written there. What is here is
//! everything the supervisor cannot know: how long this home's owner said to wait, what depends on
//! the service, and whether somebody asked for it to stay warm.
//!
//! # The arithmetic is in observations, never in elapsed time
//!
//! A policy's `after` is spent as *that many consecutive sweeps that saw the service idle*, and the
//! count lives in this struct rather than in a column. The alternative — an `idle_since` timestamp
//! compared against the clock — fails on the one case that matters most: tokio measures from
//! `std::time::Instant`, which counts no time on Linux or macOS while the machine is suspended, so
//! the first tick after a laptop's lid opens can arrive eight hours late. A timestamp comparison
//! concludes the service was idle for eight hours and kills it in the first second of somebody's
//! morning. Counting sweeps concludes it has one observation, which is the truth: nothing was
//! measured while the machine was asleep.
//!
//! A daemon restart therefore forgets, which is also correct — a service just adopted has been
//! observed zero times.
//!
//! # Three refusals before anything is measured
//!
//! A service with no policy, a service that is not running, and a service something exempts are all
//! settled before a probe is taken, because each is cheaper than the reading and none of them can be
//! overruled by one.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use mixengine_core::services::ServiceGraph;
use mixengine_proto::{IdleExemption, IdlePolicy, Millis, ServiceId};
use mixengine_supervisor::Observation;

/// How long the sweep may look through the last graph a walk produced — roadmap task **T190b**.
///
/// Ten minutes: the longest a sweep observes with a policy that has since changed. A change walks,
/// which refreshes the graph long before this runs out, and a stop is decided on a fresh one anyway.
pub(crate) const SWEEP_GRAPH_AGE: Duration = Duration::from_secs(600);

/// What a sweep concluded about one service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Something holds it open regardless of what its policy says.
    Exempt(IdleExemption),

    /// It has been seen idle, and not for long enough yet.
    Watching {
        /// How many consecutive sweeps have seen it idle.
        seen: u32,
        /// How many its policy is worth.
        needed: u32,
    },

    /// Somebody is using it, or the question could not be asked. The count is back to zero.
    Reset,

    /// Its policy is spent: stop it.
    Stop {
        /// How long it was declared idle after, for the transition's reason.
        after: Millis,
    },
}

/// How many consecutive idle sweeps `after` is worth at this period.
///
/// **At least one, always.** A policy shorter than the sweep period — `--after 10s` on a home
/// checking every thirty — would otherwise divide to zero and stop the service on a sweep that took
/// no reading at all. One observation is the least evidence anything may be stopped on.
pub(crate) fn observations(after: Millis, period: Duration) -> u32 {
    let period = u64::try_from(period.as_millis().max(1)).unwrap_or(u64::MAX);
    let sweeps = after.0 / period.max(1);

    u32::try_from(sweeps).unwrap_or(u32::MAX).max(1)
}

/// Why this service would not be stopped right now whatever its policy says.
///
/// **Asked before any probe**, because both answers are cheaper than a reading and neither can be
/// overruled by one.
///
/// A running dependent is the first: a database with no connections underneath a running pool is not
/// a database nobody wants, it is a database between two requests — and stopping it would break the
/// dependency the graph exists to maintain, so the next start walk would put it straight back, once
/// per sweep, for ever.
pub(crate) fn exemptions(
    graph: &ServiceGraph,
    service: &ServiceId,
    running: &BTreeSet<ServiceId>,
    warm: &BTreeMap<ServiceId, String>,
) -> Vec<IdleExemption> {
    let mut found = Vec::new();

    if let Ok(dependents) = graph.dependents_of(service) {
        found.extend(
            dependents
                .iter()
                .filter(|dependent| running.contains(*dependent))
                .map(|dependent| IdleExemption::DependentRunning {
                    service: dependent.clone(),
                }),
        );
    }

    if let Some(project) = warm.get(service) {
        found.push(IdleExemption::ProjectKeptWarm {
            project: project.clone(),
        });
    }

    found
}

/// How many consecutive sweeps have seen each service idle.
///
/// The whole of the sweeper's memory, and deliberately nothing else: no timestamps, no history, no
/// row. See this module's own note on why.
#[derive(Debug, Default)]
pub(crate) struct Tally {
    seen: BTreeMap<ServiceId, u32>,
}

impl Tally {
    /// Fold one observation into the count, and say what follows from it.
    ///
    /// **An exemption clears the count rather than freezing it.** A service somebody kept warm all
    /// afternoon has not been idle all afternoon — it has been unmeasured — and resuming at the
    /// count it had when the exemption began would stop it moments after the exemption lifts.
    pub(crate) fn observe(
        &mut self,
        service: &ServiceId,
        policy: &IdlePolicy,
        period: Duration,
        exempt: Option<IdleExemption>,
        observation: &Observation,
    ) -> Verdict {
        if let Some(exemption) = exempt {
            self.seen.remove(service);
            return Verdict::Exempt(exemption);
        }

        match observation {
            // **Both of these reset, and that is the safety property of the whole task.** Busy is
            // obvious. Unmeasurable is the one worth the arm: treating "I could not ask" as "nobody
            // is connected" stops a database somebody is using because a tool was missing, so an
            // unmeasurable service runs for ever instead.
            Observation::Busy | Observation::Unmeasurable { .. } => {
                self.seen.remove(service);
                Verdict::Reset
            }

            Observation::Idle => {
                let needed = observations(policy.after, period);
                let seen = self.seen.entry(service.clone()).or_default();
                *seen += 1;

                if *seen >= needed {
                    self.seen.remove(service);
                    Verdict::Stop {
                        after: policy.after,
                    }
                } else {
                    Verdict::Watching {
                        seen: *seen,
                        needed,
                    }
                }
            }
        }
    }

    /// Forget a service, because it is no longer one this daemon supervises.
    pub(crate) fn forget(&mut self, service: &ServiceId) {
        self.seen.remove(service);
    }

    /// Every service this tally is counting.
    pub(crate) fn watching(&self) -> impl Iterator<Item = &ServiceId> {
        self.seen.keys()
    }
}

/// What one sweep did.
///
/// **An enum rather than a struct with a count of zero in it**, which is `certs::renewal::Pass`'s
/// reasoning: a sweep that could not read this home's declarations and a sweep that ran and found
/// nothing to stop are different things, and under one shape the test for the first would pass
/// whether or not it had been written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pass {
    /// Nothing was measured, and this is why.
    Skipped {
        /// In words.
        because: String,
    },

    /// The sweep ran.
    Ran {
        /// What it stopped.
        stopped: Vec<ServiceId>,

        /// How many services it took a reading of.
        measured: usize,
    },
}

/// The clock that stops what nothing is using.
pub(crate) struct Sweeper {
    registry: std::sync::Arc<super::Registry>,
    store: mixengine_core::Store,
    period: Duration,
    tally: Tally,
    counters: mixengine_supervisor::Counters,
}

impl Sweeper {
    /// A sweeper over this home.
    pub(crate) fn new(
        registry: std::sync::Arc<super::Registry>,
        store: mixengine_core::Store,
        period: Duration,
    ) -> Self {
        Self {
            registry,
            store,
            period,
            tally: Tally::default(),
            counters: mixengine_supervisor::Counters::new(),
        }
    }

    /// Take one reading of every running service that has a policy, and stop what is spent.
    pub(crate) async fn sweep(&mut self) -> Pass {
        // **The last walk's graph, not a new render** — roadmap task T190b. Rendering installs every
        // service's configuration, and doing that every thirty seconds to learn who has a policy was
        // half of what this daemon spent at rest. A stop still renders first; see `still_stoppable`.
        let graph = match self.registry.graph_for_sweep(SWEEP_GRAPH_AGE).await {
            Ok(graph) => graph,
            // The home's declarations could not be read, so nothing is stopped: an idle policy that
            // cannot be looked up is not an absent one.
            Err(error) => {
                return Pass::Skipped {
                    because: format!("this home's services could not be read: {error:?}"),
                };
            }
        };

        // **Skipped rather than swept against an empty set.** A database that cannot be read is a
        // machine where nothing is known to be kept warm *and* nothing is known not to be, and
        // sweeping on the first reading would stop the pool of the project somebody is working on.
        let warm = match mixengine_core::projects::kept_warm(&self.store).await {
            Ok(warm) => warm,
            Err(error) => {
                return Pass::Skipped {
                    because: format!("this home's keep-warm projects could not be read: {error}"),
                };
            }
        };

        let running = self.registry.supervised();
        self.forget_everything_but(&running);

        let mut stopped = Vec::new();
        let mut measured = 0;

        for id in &running {
            let Some(policy) = graph.spec(id).and_then(|spec| spec.idle()).cloned() else {
                continue;
            };

            let exempt = exemptions(&graph, id, &running, &warm).into_iter().next();

            // The reading is taken only when nothing exempts the service, which is the whole reason
            // exemptions are computed first: on macOS it is also the difference between one process
            // spawned per sweep and none.
            let observation = if exempt.is_some() {
                Observation::Busy
            } else {
                measured += 1;

                mixengine_supervisor::observe(
                    self.registry.host().connections(),
                    id,
                    &policy.probe,
                    &mut self.counters,
                )
                .await
            };

            if let Observation::Unmeasurable { because } = &observation {
                tracing::debug!(
                    service = id.as_str(),
                    %because,
                    "this service could not be measured, so it will not be stopped"
                );
            }

            if let Verdict::Stop { after } =
                self.tally
                    .observe(id, &policy, self.period, exempt, &observation)
                && let Some(fresh) = self.still_stoppable(id, &policy, &running, &warm).await
                && self.stop(&fresh, id, after).await
            {
                stopped.push(id.clone());
            }
        }

        Pass::Ran { stopped, measured }
    }

    /// The present, asked once before a stop — roadmap task **T190b**, the spec's D2.
    ///
    /// The sweep counted against a graph up to [`SWEEP_GRAPH_AGE`] old. Stopping is the one thing it
    /// does, so it renders now and stops only if the fresh graph still gives this service a policy
    /// no longer than the one counted, and still does not exempt it. [`None`] in every other case;
    /// the verdict has already cleared the tally, so counting starts again from nothing.
    async fn still_stoppable(
        &self,
        id: &ServiceId,
        counted: &IdlePolicy,
        running: &BTreeSet<ServiceId>,
        warm: &BTreeMap<ServiceId, String>,
    ) -> Option<ServiceGraph> {
        let fresh = match self.registry.graph().await {
            Ok(fresh) => fresh,
            Err(error) => {
                tracing::warn!(
                    service = id.as_str(),
                    ?error,
                    "an idle service was not stopped: this home's services could not be read"
                );
                return None;
            }
        };

        let policy = fresh.spec(id).and_then(|spec| spec.idle())?;

        if observations(policy.after, self.period) > observations(counted.after, self.period) {
            return None;
        }

        if !exemptions(&fresh, id, running, warm).is_empty() {
            return None;
        }

        Some(fresh)
    }

    /// Drop the count of every service this daemon is no longer supervising.
    ///
    /// A service that stopped and was started again begins at zero rather than resuming a count
    /// taken before its restart — the same reasoning that keeps the tally out of a column.
    fn forget_everything_but(&mut self, running: &BTreeSet<ServiceId>) {
        let watched: Vec<ServiceId> = self.tally.watching().cloned().collect();

        for id in watched {
            if !running.contains(&id) {
                self.tally.forget(&id);
            }
        }
    }

    /// Stop one idle service, and say so on its transition rather than in a second event.
    ///
    /// **Takes the graph `still_stoppable` rendered for this stop** — roadmap task T190b. The sweep
    /// looks through a kept graph, and a stop is the one thing it may not decide on one; one render
    /// per stopped service is what that costs, and a stop happens once per idle period at most.
    async fn stop(&self, graph: &ServiceGraph, id: &ServiceId, after: Millis) -> bool {
        // **Set before the plan is walked, never after.** The runner reads the reason at the moment
        // it enters `Stopping`, so a value written afterwards would arrive too late to explain this
        // stop and in good time to mislabel the next one.
        self.registry
            .stopping_because(id, Some(mixengine_proto::StateReason::Idle { after }));

        // **And taken back on every path that does not stop it.** A reason left behind by a stop
        // that did not happen is read by whatever stops the service next — most likely a person
        // running `mix service stop` — and tells them their own request was an idle timeout.
        let plan = match graph.stop_plan([id]) {
            Ok(plan) => plan,
            Err(error) => {
                self.registry.stopping_because(id, None);
                tracing::warn!(service = id.as_str(), %error, "an idle service was not stopped");
                return false;
            }
        };

        if self.registry.stop(&plan).await.failed.is_some() {
            self.registry.stopping_because(id, None);
            tracing::warn!(service = id.as_str(), "an idle service did not stop");
            return false;
        }

        tracing::info!(
            service = id.as_str(),
            %after,
            "nothing was using this service, so it was stopped"
        );

        // **Here, and after the stop succeeded** — roadmap task **T70a**. This is the one stop a
        // *running* daemon makes that may arm a wake: a person's stop and a dependency's leave
        // nothing bound, which is how D8 is answered on this path without reading anything at the
        // moment a connection arrives. The other wakeable stops — a shutdown, a process that went
        // away — are made by a daemon that is going or gone, and are armed by the boot walk in
        // `main` instead (T123).
        //
        // A service with nothing to hold — a pool, which has a permanent activator of its own, or
        // anything with no address at all — is left alone by the row check inside.
        super::hold::hold_if_wakeable(&self.registry, id).await;

        true
    }
}

/// Sweep every `every`, until `shutdown`.
///
/// **The first tick is thrown away**, as `certs::renewal::start` throws its away and for a reason of
/// its own: `tokio::time::interval` completes its first immediately, and a sweep at the moment the
/// daemon finishes recovery would take a reading of services that have been running for no time.
pub(crate) fn start(
    mut sweeper: Sweeper,
    every: Duration,
    shutdown: tokio_util::sync::CancellationToken,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(every);
        ticker.tick().await;

        loop {
            tokio::select! {
                () = shutdown.cancelled() => return,
                _ = ticker.tick() => {}
            }

            match sweeper.sweep().await {
                // Debug and not warn, on `renewal`'s reasoning: this arrives every period, and a
                // home whose services cannot be read already has its own line at start and its own
                // `mix doctor` check. A warning every thirty seconds about something reported twice
                // elsewhere is how a log stops being read.
                Pass::Skipped { because } => {
                    tracing::debug!(%because, "nothing was swept for idleness");
                }

                Pass::Ran { stopped, measured } => {
                    if !stopped.is_empty() {
                        tracing::info!(
                            stopped = stopped.len(),
                            measured,
                            "services nothing was using were stopped"
                        );
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{
        IdleProbe, ReadyCheck, RestartPolicy, ServiceSpec, ServiceSpecBuilder, StopBehaviour,
    };

    use super::*;

    const PERIOD: Duration = Duration::from_secs(30);

    fn id(text: &str) -> ServiceId {
        ServiceId::parse(text).expect("a valid id")
    }

    // ---- The sweep against a real registry — roadmap task T190b. ----

    use std::sync::Arc;

    use mixengine_testkit::FakeService;

    use super::super::SpecSource;
    use super::super::fixture::{self, Swappable, arguments, registry_on};

    /// The port the idle probe asks about; the mock host reports nobody connected to it.
    const PROBED: u16 = 45_123;

    fn idle_spec(fake: &FakeService, after: Millis) -> ServiceSpec {
        fixture::spec("caddy")
            .args(arguments(fake))
            .idle(IdlePolicy {
                after,
                probe: IdleProbe::Connections { port: PROBED },
            })
            .build()
            .expect("a usable spec")
    }

    /// A registry with `caddy` running under `source`, on a host that reports nobody connected.
    async fn running(
        source: &Arc<Swappable>,
    ) -> (
        mixengine_testkit::Home,
        Arc<super::super::Registry>,
        mixengine_core::Store,
    ) {
        let (home, paths, store) = fixture::home(&["caddy"]).await;
        let host = mixengine_platform::mock::Host::with_home(paths.root());
        host.set_connections(PROBED, 0);

        let registry = Arc::new(registry_on(
            &paths,
            &store,
            Arc::clone(source) as Arc<dyn SpecSource>,
            Arc::new(host),
        ));

        let graph = registry.graph().await.expect("a graph");
        let plan = graph.start_plan([&id("caddy")]).expect("a plan");
        assert!(registry.start(&graph, &plan).await.failed.is_none());

        (home, registry, store)
    }

    /// **A sweep with nothing to stop looks through the start's graph and renders nothing.**
    #[tokio::test]
    async fn a_sweep_with_nothing_to_stop_does_not_render() {
        let fake = FakeService::new();
        let source = Arc::new(Swappable::new(vec![idle_spec(
            &fake,
            Millis::from_secs(3600),
        )]));
        let (_home, registry, store) = running(&source).await;
        let before = source.renders();

        let mut sweeper = Sweeper::new(Arc::clone(&registry), store, PERIOD);
        sweeper.sweep().await;
        sweeper.sweep().await;

        assert_eq!(
            source.renders(),
            before,
            "both sweeps looked through the start's graph"
        );
    }

    /// **A stop is decided on a graph rendered for it** — the spec's D2.
    #[tokio::test]
    async fn a_stop_is_decided_on_a_fresh_graph() {
        let fake = FakeService::new();
        let source = Arc::new(Swappable::new(vec![idle_spec(&fake, Millis(1))]));
        let (_home, registry, store) = running(&source).await;
        let before = source.renders();

        let pass = Sweeper::new(Arc::clone(&registry), store, PERIOD)
            .sweep()
            .await;

        assert_eq!(
            pass,
            Pass::Ran {
                stopped: vec![id("caddy")],
                measured: 1
            }
        );
        assert_eq!(source.renders(), before + 1, "one render, for the stop");
    }

    #[tokio::test]
    async fn a_policy_switched_off_since_the_kept_graph_saves_the_service() {
        let fake = FakeService::new();
        let source = Arc::new(Swappable::new(vec![idle_spec(&fake, Millis(1))]));
        let (_home, registry, store) = running(&source).await;
        source.set(vec![
            fixture::spec("caddy")
                .args(arguments(&fake))
                .build()
                .expect("a spec"),
        ]);

        let pass = Sweeper::new(Arc::clone(&registry), store, PERIOD)
            .sweep()
            .await;

        assert_eq!(
            pass,
            Pass::Ran {
                stopped: vec![],
                measured: 1
            }
        );
        assert!(registry.supervised().contains(&id("caddy")));
    }

    #[tokio::test]
    async fn a_policy_lengthened_since_the_kept_graph_saves_the_service() {
        let fake = FakeService::new();
        let source = Arc::new(Swappable::new(vec![idle_spec(&fake, Millis(1))]));
        let (_home, registry, store) = running(&source).await;
        source.set(vec![idle_spec(&fake, Millis::from_secs(3600))]);

        let pass = Sweeper::new(Arc::clone(&registry), store, PERIOD)
            .sweep()
            .await;

        assert_eq!(
            pass,
            Pass::Ran {
                stopped: vec![],
                measured: 1
            }
        );
        assert!(registry.supervised().contains(&id("caddy")));
    }

    #[tokio::test]
    async fn a_fresh_render_that_fails_stops_nothing() {
        let fake = FakeService::new();
        let source = Arc::new(Swappable::new(vec![idle_spec(&fake, Millis(1))]));
        let (_home, registry, store) = running(&source).await;
        source.unavailable(true);

        let pass = Sweeper::new(Arc::clone(&registry), store, PERIOD)
            .sweep()
            .await;

        assert_eq!(
            pass,
            Pass::Ran {
                stopped: vec![],
                measured: 1
            }
        );
        assert!(registry.supervised().contains(&id("caddy")));
    }

    fn policy(minutes: u64) -> IdlePolicy {
        IdlePolicy {
            after: Millis::from_secs(minutes * 60),
            probe: IdleProbe::Connections { port: 3306 },
        }
    }

    /// A spec that runs nothing, because nothing here starts one.
    ///
    /// The root is absolute per OS for the reason the builder refuses otherwise: a spec is never
    /// resolved against a `PATH`, so `/bin/true` is a relative path on Windows and no spec at all.
    fn spec(name: &str, depends_on: &[&str]) -> ServiceSpec {
        let root: std::path::PathBuf = if cfg!(windows) {
            r"C:\MixEngine".into()
        } else {
            "/opt/mixengine".into()
        };

        let mut builder: ServiceSpecBuilder = ServiceSpec::builder(id(name), root.join(name))
            .cwd(root)
            .ready(ReadyCheck::PidAlive { settle: Millis(10) })
            .restart(RestartPolicy::Never)
            .stop(StopBehaviour::Signal { grace: Millis(500) });

        for dependency in depends_on {
            builder = builder.depends_on(id(dependency));
        }

        builder.build().expect("a runnable spec")
    }

    /// `after` is spent in consecutive sweeps, and the last one is what stops it.
    ///
    /// One minute at a thirty-second period is two observations — not "sixty seconds of elapsed
    /// time", which is the distinction this whole module is built around.
    #[test]
    fn a_policy_is_spent_in_consecutive_sweeps() {
        let mut tally = Tally::default();
        let service = id("mariadb@main");

        assert_eq!(
            tally.observe(&service, &policy(1), PERIOD, None, &Observation::Idle),
            Verdict::Watching { seen: 1, needed: 2 }
        );

        assert_eq!(
            tally.observe(&service, &policy(1), PERIOD, None, &Observation::Idle),
            Verdict::Stop {
                after: Millis::from_secs(60)
            }
        );
    }

    /// One busy reading sends it back to nothing. Idle is consecutive or it is not idle.
    #[test]
    fn a_busy_reading_resets_the_count() {
        let mut tally = Tally::default();
        let service = id("mariadb@main");

        tally.observe(&service, &policy(2), PERIOD, None, &Observation::Idle);
        tally.observe(&service, &policy(2), PERIOD, None, &Observation::Idle);

        assert_eq!(
            tally.observe(&service, &policy(2), PERIOD, None, &Observation::Busy),
            Verdict::Reset
        );

        assert_eq!(
            tally.observe(&service, &policy(2), PERIOD, None, &Observation::Idle),
            Verdict::Watching { seen: 1, needed: 4 },
            "the count starts again rather than resuming where it was"
        );
    }

    /// A service that cannot be measured is never stopped, however long that lasts.
    ///
    /// The safety property: a machine with no `lsof` keeps every service running rather than
    /// stopping all of them on a reading nobody took.
    #[test]
    fn an_unmeasurable_service_is_never_stopped() {
        let mut tally = Tally::default();
        let service = id("mariadb@main");

        let broken = Observation::Unmeasurable {
            because: "no lsof on this machine".to_owned(),
        };

        for _ in 0..100 {
            assert_eq!(
                tally.observe(&service, &policy(1), PERIOD, None, &broken),
                Verdict::Reset
            );
        }
    }

    /// An exemption clears the count rather than pausing it.
    #[test]
    fn an_exemption_clears_what_was_counted() {
        let mut tally = Tally::default();
        let service = id("mariadb@main");

        tally.observe(&service, &policy(1), PERIOD, None, &Observation::Idle);

        let exempt = IdleExemption::ProjectKeptWarm {
            project: "shop".to_owned(),
        };

        assert_eq!(
            tally.observe(
                &service,
                &policy(1),
                PERIOD,
                Some(exempt.clone()),
                &Observation::Idle
            ),
            Verdict::Exempt(exempt)
        );

        assert_eq!(
            tally.observe(&service, &policy(1), PERIOD, None, &Observation::Idle),
            Verdict::Watching { seen: 1, needed: 2 },
            "an afternoon spent kept warm is an afternoon unmeasured, not an afternoon idle"
        );
    }

    /// A running dependent exempts, and a stopped one does not.
    #[test]
    fn a_running_dependent_exempts_and_a_stopped_one_does_not() {
        let graph = ServiceGraph::new([
            spec("mariadb@main", &[]),
            spec("php-fpm@8.3", &["mariadb@main"]),
        ])
        .expect("a graph");

        let database = id("mariadb@main");
        let warm = BTreeMap::new();

        let running = BTreeSet::from([id("php-fpm@8.3")]);
        assert_eq!(
            exemptions(&graph, &database, &running, &warm),
            vec![IdleExemption::DependentRunning {
                service: id("php-fpm@8.3")
            }]
        );

        assert!(
            exemptions(&graph, &database, &BTreeSet::new(), &warm).is_empty(),
            "a pool that is stopped holds nothing open"
        );
    }

    /// A keep-warm project exempts the service it reaches.
    #[test]
    fn a_kept_warm_service_exempts() {
        let graph = ServiceGraph::new([spec("php-fpm@8.3", &[])]).expect("a graph");
        let pool = id("php-fpm@8.3");
        let warm = BTreeMap::from([(pool.clone(), "shop".to_owned())]);

        assert_eq!(
            exemptions(&graph, &pool, &BTreeSet::new(), &warm),
            vec![IdleExemption::ProjectKeptWarm {
                project: "shop".to_owned()
            }],
            "the exemption names the project, because that is what a person has to go and change"
        );
    }

    /// A policy shorter than the sweep period is one observation, never zero.
    ///
    /// Zero would stop a service on a sweep that took no reading at all.
    #[test]
    fn a_policy_shorter_than_the_period_is_still_one_observation() {
        assert_eq!(observations(Millis::from_secs(10), PERIOD), 1);
        assert_eq!(observations(Millis::from_secs(30), PERIOD), 1);
        assert_eq!(observations(Millis::from_secs(60), PERIOD), 2);
        assert_eq!(observations(Millis::from_secs(30 * 60), PERIOD), 60);
    }
}
