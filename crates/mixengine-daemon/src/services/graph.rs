//! What this home declares, as the registry sees it — roadmap task **T207d** gave it a file.
//!
//! The graph every walk starts from, what a walk remembers out of it (first-run rituals, how a
//! service's databases are provisioned, which running service has to re-read its configuration),
//! and the questions answered by rendering it: the front end's program and TLS port, and where a
//! stopped service would be woken. The state it reads is the [`Registry`]'s.

use std::collections::HashMap;

use mixengine_core::generate::Generated;
use mixengine_core::services::{self, ServiceGraph};
use mixengine_proto::ServiceId;

use super::spec::catalogue;
use super::{Registry, Undeclarable, activate, lock, reset, spec};

impl Registry {
    /// Bring every service's configuration up to date, and hand any change to whatever is running.
    ///
    /// [`graph`](Self::graph) without the graph, for the caller that changed something a *front end*
    /// renders rather than something about a service: a site written, updated, deleted, started or
    /// stopped. The rendering is the whole of what it wants — `mix site create` has no use for a
    /// start order.
    ///
    /// **A failure fails the caller**, which is deliberate and is not the hosts queue's behaviour.
    /// A hosts entry that has not been granted yet is a want with a person on the other end of it; a
    /// configuration the server refused is a defect, and a `site.create` that returned success while
    /// the site was unreachable would send whoever typed it to look in the wrong place. Nothing was
    /// installed either way — `document::install` stages first.
    ///
    /// # Errors
    ///
    /// [`Undeclarable`], exactly as [`graph`](Self::graph) reports it.
    pub(crate) async fn reconfigure(&self) -> Result<(), Undeclarable> {
        self.graph().await.map(drop)
    }

    /// The declared services, checked and ordered.
    ///
    /// Asked afresh rather than cached, because the set is a *rendering* of state that changes —
    /// a service created, a port edited, a package upgraded — and a graph held from startup would
    /// answer for a home that no longer exists.
    ///
    /// **It is also where a running service learns its configuration moved**, which is roadmap task
    /// **T31** and is here because this is the only place both halves are visible: the source knows
    /// what it rewrote, and this map knows what is running. Asking anywhere else would mean either a
    /// generator that reaches into the registry or a walk that re-reads files it has already
    /// overwritten. See [`Registry::hand_over`].
    ///
    /// # Errors
    ///
    /// [`Undeclarable`], which keeps the two apart on purpose: a source that failed is the daemon's
    /// problem, and a set that is not a graph is the user's declaration and belongs in
    /// `invalid_argument`.
    pub(crate) async fn graph(&self) -> Result<ServiceGraph, Undeclarable> {
        let generated = self
            .specs
            .declared()
            .await
            .map_err(|error| Undeclarable::Unavailable(Box::new(error)))?;

        self.hand_over(&generated);
        self.remember_rituals(&generated);
        self.remember_provisioning(&generated);

        let graph = ServiceGraph::new(
            generated
                .into_iter()
                .map(|one| one.spec)
                .collect::<Vec<_>>(),
        )
        .map_err(|error| Undeclarable::Invalid(Box::new(mixengine_core::Error::Graph(error))))?;

        *lock(&self.last_graph) = Some((tokio::time::Instant::now(), graph.clone()));

        Ok(graph)
    }

    /// The last graph a walk produced if it is younger than `max_age`, and a new walk otherwise —
    /// roadmap task **T190b**, for the idle sweep and nothing else.
    ///
    /// **Stale on purpose, and bounded twice.** Every change to this home walks, so the graph here
    /// is behind only when a change skipped the walk, and `max_age` bounds even that. And the sweep
    /// never *acts* on it: a stop is decided on a fresh [`graph`](Self::graph).
    ///
    /// # Errors
    ///
    /// [`Undeclarable`], exactly as [`graph`](Self::graph) reports it, when a walk was needed.
    pub(crate) async fn graph_for_sweep(
        &self,
        max_age: std::time::Duration,
    ) -> Result<ServiceGraph, Undeclarable> {
        let kept = lock(&self.last_graph)
            .as_ref()
            .filter(|(taken, _)| taken.elapsed() < max_age)
            .map(|(_, graph)| graph.clone());

        match kept {
            Some(graph) => Ok(graph),
            None => self.graph().await,
        }
    }

    /// Drop the kept graph, so the next sweep renders — for a change that does not walk (T190b).
    pub(crate) fn forget_graph(&self) {
        *lock(&self.last_graph) = None;
    }

    /// The absolute path of the program this home's front end runs, or [`None`].
    ///
    /// **By what a package is for, not by what it is called** — [`services::front_end::held_by`]
    /// answers that, by role rather than by name, so a third front end added later is found without
    /// a string being added to a list here.
    ///
    /// Rendering the graph is what turns a row into a program path, and [`recover`](Self::recover)
    /// has already done it a moment before this is called at start-up: a second rendering identical
    /// to the first writes nothing and reloads nothing.
    ///
    /// [`None`] for every failure as well as for a home with no front end. The caller is a start-up
    /// step that must not fail a start, and "we could not find out" and "there is nothing to find"
    /// lead to the same next move.
    pub(crate) async fn front_end_program(&self) -> Option<std::path::PathBuf> {
        let id = services::front_end::held_by(&self.store, &catalogue())
            .await
            .inspect_err(|error| {
                tracing::warn!(%error, "cannot tell which service is this home's front end");
            })
            .ok()
            .flatten()?;

        let id = ServiceId::parse(&id).ok()?;

        let graph = self
            .graph()
            .await
            .inspect_err(|error| {
                tracing::warn!(
                    ?error,
                    "cannot render this home's services, so the front end has no path"
                );
            })
            .ok()?;

        Some(graph.spec(&id)?.program().to_path_buf())
    }

    /// The port this home's front end serves TLS on, or [`None`] — roadmap task **T53**.
    ///
    /// **From the settings a rendering would be made with**, which is the only source that cannot
    /// come to disagree with the configuration on disk. The other two are both refused: a second
    /// `Settings::merge` here could drift from the generator's, and reading the number out of the
    /// rendered file is what `CLAUDE.md` forbids outright — generated configuration is
    /// disposable and is never parsed back into state.
    ///
    /// **`settings` and never `generate`.** [`generate`](mixengine_core::generate::Generator::generate)
    /// installs, so reaching the port that way would have `mix cert status` — a command whose whole
    /// guarantee is that it writes nothing — rewrite this home's configuration and possibly reload
    /// a running server as a side effect of being asked a question.
    ///
    /// [`None`] for a home with no front end and for every failure alike, on
    /// [`front_end_program`](Self::front_end_program)'s reasoning: the caller reports "nothing is
    /// serving this", and "we could not find out" leads to the same sentence.
    pub(crate) async fn front_end_tls_port(&self) -> Option<u16> {
        let id = services::front_end::held_by(&self.store, &catalogue())
            .await
            .inspect_err(|error| {
                tracing::warn!(%error, "cannot tell which service is this home's front end");
            })
            .ok()
            .flatten()?;

        let id = ServiceId::parse(&id).ok()?;

        let settings = self
            .specs
            .settings(&id)
            .await
            .inspect_err(|error| {
                tracing::warn!(
                    %error,
                    "cannot read this home's front end settings, so it has no TLS port"
                );
            })
            .ok()
            .flatten()?;

        u16::try_from(settings.number("https_port"))
            .ok()
            .filter(|port| *port != 0)
    }

    /// Keep what each declared service still has to have done to it once — roadmap task **T33**.
    ///
    /// **Replaced rather than merged**, so a service that stopped declaring a ritual — an override
    /// pointed at a different data directory, a recipe that no longer has one — stops carrying it.
    /// The whole map is at most one entry per declared service, and each holds a cloned
    /// [`Context`](mixengine_core::generate::Context).
    fn remember_rituals(&self, generated: &[Generated]) {
        let remembered: HashMap<_, _> = generated
            .iter()
            .filter_map(|one| {
                one.first_run
                    .clone()
                    .map(|ritual| (one.spec.id().clone(), ritual))
            })
            .collect();

        *lock(&self.rituals) = remembered;
    }

    /// The ritual this service still has to have performed, if it declared one.
    pub(super) fn ritual_for(&self, id: &ServiceId) -> Option<mixengine_core::generate::FirstRun> {
        lock(&self.rituals).get(id).cloned()
    }

    /// Whether this service declares a credential a repair could re-set — roadmap task **T127**.
    ///
    /// **Asked before anything is stopped.** The repair is a stop, some steps and a start; refusing
    /// a service that has no credential *after* the stop would take a web server down to tell
    /// somebody they named the wrong thing.
    pub(crate) fn has_credential_reset(&self, id: &ServiceId) -> bool {
        self.ritual_for(id).is_some_and(|plan| plan.has_reset())
    }

    /// Re-set this service's superuser credential in place — roadmap task **T127**.
    ///
    /// **The caller has already stopped it**, and the caller is what starts it again: this is the
    /// middle of `service.reset_credential` and nothing else. It is a method here rather than a call
    /// in the API for [`first_run::ensure`]'s reason — the two things a repair needs, the OS keyring
    /// and the job system, are the registry's fields.
    ///
    /// # Errors
    ///
    /// A service this home does not declare or whose recipe has no repair; and then whatever
    /// [`reset::perform`] reports.
    ///
    /// [`first_run::ensure`]: super::first_run::ensure
    pub(crate) async fn reset_credential(
        &self,
        id: &ServiceId,
    ) -> Result<(), mixengine_proto::Error> {
        let plan = self.ritual_for(id).ok_or_else(|| {
            mixengine_proto::Error::new(
                mixengine_proto::ErrorCode::InvalidArgument,
                format!("{id} has no first-run ritual, so it keeps no credential of its own"),
            )
        })?;

        reset::perform(&self.host, &self.jobs, &plan).await
    }

    /// Keep how each declared service's databases are made — roadmap task **T77a**.
    ///
    /// Replaced rather than merged, on [`Registry::remember_rituals`]'s reasoning: a service that
    /// stopped declaring one — a recipe replaced, an override pointed elsewhere — stops carrying it.
    fn remember_provisioning(&self, generated: &[Generated]) {
        let remembered: HashMap<_, _> = generated
            .iter()
            .filter_map(|one| {
                one.provisioning
                    .clone()
                    .map(|provisioning| (one.spec.id().clone(), provisioning))
            })
            .collect();

        *lock(&self.provisioning) = remembered;
    }

    /// How this service's databases are administered, if it declared a way — roadmap task **T77a**.
    ///
    /// A walk fills the map, so this answers [`None`] until one has happened. Every caller reaches
    /// it through `database.create`, which asks [`Registry::graph`] first.
    pub(crate) fn provisioning_for(
        &self,
        id: &ServiceId,
    ) -> Option<mixengine_core::generate::Provisioning> {
        lock(&self.provisioning).get(id).cloned()
    }

    /// Tell every service that is running and whose configuration just changed to re-read it.
    ///
    /// **A notification and not a command**, which is the whole design: what runs the reload is the
    /// runner, in the surroundings the service itself was started with, exactly as its health probe
    /// and its stop command are run. Doing it from here would mean a second, lesser copy of
    /// [`Surroundings`](mixengine_supervisor::Surroundings) — and it would put a subprocess with a
    /// thirty-second patience inside `service.list`, which is a call a GUI makes on a timer.
    ///
    /// **Only what is up.** A service that is stopped will read the new file when it is started, and
    /// one in a restart backoff will read it on the attempt after this. Neither is a reload, and
    /// asking for one would be a command sent to an address nothing is listening on.
    ///
    /// A permit left with [`Notify`] rather than a message is what makes this safe to do on every
    /// walk: a runner busy in a health probe collects it at the top of its next turn, and two walks
    /// that both find a change while it is busy collapse into the one reload that was needed. The
    /// service is left alone if it has no [`ReloadBehaviour`](mixengine_proto::ReloadBehaviour) —
    /// the runner says so once, because a configuration that changed and reached nothing is worth a
    /// line in `daemon.log`.
    ///
    /// [`Notify`]: tokio::sync::Notify
    fn hand_over(&self, generated: &[Generated]) {
        let running = lock(&self.running);

        for one in generated.iter().filter(|one| one.changed()) {
            let Some(entry) = running.get(one.spec.id()) else {
                continue;
            };

            tracing::debug!(
                service = one.spec.id().as_str(),
                "this service's configuration changed while it is running; asking it to re-read it"
            );

            entry.asked_to_reload.notify_one();
        }
    }

    /// Where a connection would have to arrive to wake `service` — roadmap task **T70a**.
    ///
    /// **Asked of the generator rather than of the row**, so the answer is the one a render
    /// computed: a socket path is a function of how deep this home is and a port is a function of
    /// the row, and two places working them out is two chances to disagree about an address the
    /// daemon binds and a server then has to.
    ///
    /// An empty answer is the ordinary one. Most services are not wakeable, and the two front ends
    /// must never be.
    ///
    /// # Errors
    ///
    /// Whatever rendering this home's declarations costs — boxed, because the one caller only logs
    /// it and `mixengine_core::Error` is over 128 bytes (`clippy::result_large_err`).
    pub(crate) async fn wakeable_at(
        &self,
        service: &ServiceId,
    ) -> Result<Vec<mixengine_platform::activation::Listen>, Box<mixengine_core::Error>> {
        let generator = spec::generator(&self.paths, &self.store, self.host.as_ref(), self.welcome);

        Ok(generator
            .held_while_stopped()
            .await?
            .remove(service)
            .unwrap_or_default()
            .iter()
            .map(activate::to_listen)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use mixengine_proto::{Millis, ReloadBehaviour, ServiceState};
    use mixengine_testkit::FakeService;

    use super::super::fixture::{
        Declared, EVENTUALLY, Rerendered, Unavailable, arguments, home, registry, row, service,
        spec,
    };
    use super::super::spec::SpecSource;
    use super::*;

    /// **The idle sweep may look through the last graph while it is young** — roadmap task T190b.
    #[tokio::test]
    async fn the_sweep_reuses_a_young_graph_and_renders_an_old_one() {
        let (_home, paths, store) = home(&["caddy"]).await;
        // Paused only now: the database above is real, and a paused clock times its pool out.
        tokio::time::pause();
        let source = Arc::new(super::super::fixture::Swappable::new(vec![
            spec("caddy").build().expect("a spec"),
        ]));
        let registry = registry(&paths, &store, Arc::clone(&source) as Arc<dyn SpecSource>);
        let age = Duration::from_secs(600);

        registry.graph_for_sweep(age).await.expect("a graph");
        registry.graph_for_sweep(age).await.expect("a graph");
        assert_eq!(
            source.renders(),
            1,
            "the second look reused the first render"
        );

        tokio::time::advance(age).await;
        registry.graph_for_sweep(age).await.expect("a graph");
        assert_eq!(
            source.renders(),
            2,
            "a graph as old as the limit is rendered again"
        );
    }

    /// **Any walk refreshes what the sweep sees**, which is what bounds its staleness by the next
    /// change rather than by the clock — roadmap task T190b.
    #[tokio::test]
    async fn any_walk_refreshes_what_the_sweep_sees() {
        let (_home, paths, store) = home(&["caddy", "redis"]).await;
        tokio::time::pause();
        let source = Arc::new(super::super::fixture::Swappable::new(vec![
            spec("caddy").build().expect("a spec"),
        ]));
        let registry = registry(&paths, &store, Arc::clone(&source) as Arc<dyn SpecSource>);
        let age = Duration::from_secs(600);

        registry.graph_for_sweep(age).await.expect("a graph");
        source.set(vec![
            spec("caddy").build().expect("a spec"),
            spec("redis").build().expect("a spec"),
        ]);
        registry.graph().await.expect("a walk after a change");

        let seen = registry.graph_for_sweep(age).await.expect("a graph");
        assert!(
            seen.spec(&service("redis")).is_some(),
            "the walk's graph, not the older one"
        );
        assert_eq!(source.renders(), 2);
    }

    /// A change that does not walk says so, and the next sweep renders — roadmap task T190b.
    #[tokio::test]
    async fn a_forgotten_graph_is_rendered_again() {
        let (_home, paths, store) = home(&["caddy"]).await;
        tokio::time::pause();
        let source = Arc::new(super::super::fixture::Swappable::new(vec![
            spec("caddy").build().expect("a spec"),
        ]));
        let registry = registry(&paths, &store, Arc::clone(&source) as Arc<dyn SpecSource>);
        let age = Duration::from_secs(600);

        registry.graph_for_sweep(age).await.expect("a graph");
        registry.forget_graph();
        registry.graph_for_sweep(age).await.expect("a graph");

        assert_eq!(source.renders(), 2);
    }

    /// **A configuration that changed under a running service reaches it without a restart** —
    /// roadmap task T31, and the mechanism every site on the machine depends on.
    ///
    /// The claim has two halves and the fixture proves both. The file the reload command creates is
    /// evidence that the command really ran — nothing else in this test writes it — and the pid is
    /// evidence of what the whole thing is *for*: it is the same process before and after, so the
    /// connections it was serving were never dropped.
    ///
    /// The first walk finds a changed rendering too, and is deliberately left in: nothing is running
    /// then, and a reload asked of a service that is down would be a command sent to an address
    /// nothing is listening on.
    #[tokio::test]
    async fn a_rendering_that_changed_reaches_the_process_already_running() {
        let (home, paths, store) = home(&["caddy"]).await;

        let reread = home.path().join("the-configuration-was-read-again");
        let reload = FakeService::new().touch(&reread);

        let declared = Rerendered(vec![
            spec("caddy")
                .reload(ReloadBehaviour::Command {
                    program: FakeService::program(),
                    args: arguments(&reload),
                    patience: Millis::from_secs(20),
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("caddy")]).expect("a plan");
        let walk = registry.start(&graph, &plan).await;
        assert!(walk.failed.is_none(), "{walk:?}");

        let (_, before) = row(&store, &service("caddy")).await;
        assert!(
            !reread.exists(),
            "starting a service is not the same as reloading one"
        );

        // The walk that finds the rendering changed under a service that is up. What it does about
        // it is a permit left with the runner, so the wait below is for the runner's next turn.
        registry.graph().await.expect("a second walk");

        let deadline = std::time::Instant::now() + EVENTUALLY;
        while !reread.exists() && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        assert!(
            reread.is_file(),
            "the reload command was never run: {} is not there",
            reread.display()
        );

        let (state, after) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Running, "a reload is not a restart");
        assert_eq!(
            after, before,
            "the process was replaced rather than reloaded"
        );

        let stopping = graph.stop_plan([&service("caddy")]).expect("a plan");
        assert!(registry.stop(&stopping).await.failed.is_none());
    }

    #[tokio::test]
    async fn a_source_that_cannot_answer_is_kept_apart_from_a_declaration_that_is_wrong() {
        let (_home, paths, store) = home(&[]).await;
        let refusing = registry(&paths, &store, Arc::new(Unavailable));

        let error = refusing.graph().await.expect_err("the source refused");

        let Undeclarable::Unavailable(why) = &error else {
            panic!("a source that failed is the daemon's problem, not the user's: {error:?}");
        };
        assert!(
            // The source's own sentence, kept rather than replaced: this crate does not know what
            // T30 will fail at, so it must not write the failure down in its own words. It is a
            // `mixengine_core::Error` and not an `anyhow::Error` since R8, which is what lets the
            // wire mapping read the code off it instead of downcasting to find one.
            why.to_string().contains("no such package"),
            "{why}"
        );

        // And the other half: a set that is not a graph is what the user declared.
        let cycle = Declared(vec![
            spec("a")
                .depends_on(service("b"))
                .build()
                .expect("a usable spec"),
            spec("b")
                .depends_on(service("a"))
                .build()
                .expect("a usable spec"),
        ]);
        let declaring_a_cycle = registry(&paths, &store, Arc::new(cycle));

        let error = declaring_a_cycle
            .graph()
            .await
            .expect_err("two services in a cycle");

        assert!(
            matches!(
                &error,
                Undeclarable::Invalid(inner) if matches!(**inner, mixengine_core::Error::Graph(_))
            ),
            "{error:?}"
        );
    }
}
