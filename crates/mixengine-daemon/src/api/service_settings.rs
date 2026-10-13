//! What may be set on a service, and its readings: `service.limits` and `service.set_limits`
//! (roadmap task **T68**, the watchdog **T71a**), `service.idle` and `service.set_idle` (**T69**),
//! `service.save_resources` and `service.set_save_resources` (**T167b**), and
//! `service.set_autostart` (**T112**).
//!
//! Each writes a row and starts or stops nothing; `service.set_limits` alone also pushes what it
//! wrote at a running process.

use mixengine_core::services::GraphError;
use mixengine_proto::{
    Enforcement, Error, ErrorCode, IdleReport, IdleSource, LimitSupport, MemoryWatchdog,
    ResourceLimits, SaveResources, SaveResourcesSet, ServiceAutostartSet, ServiceId,
    ServiceIdleSet, ServiceLimitsReport, ServiceLimitsSet, ServiceSpec, ServiceSummary,
    ServiceTarget,
};

use super::Api;
use crate::error::ToWire as _;

impl Api {
    /// `service.limits` — what this service may take, and what this machine will do about it.
    ///
    /// **Both halves in one answer**, because neither is worth having alone: a `memory_mb` of 512
    /// means one thing where it is a commit charge enforced by a failed allocation and another where
    /// it is charged pages enforced by the OOM killer — and a third where it is stored and enforced
    /// by nothing. The T68 design, D2.
    pub(super) async fn service_limits(
        &self,
        target: &ServiceTarget,
    ) -> Result<ServiceLimitsReport, Error> {
        let id = self.named_service(target.service.as_ref())?;
        let spec = self.spec_of(&id).await?;

        let support = self.elevation.host().resource_control().support();

        let watchdog = watchdog_of(&support, spec.limits(), &spec, self.memory_over_minutes);

        Ok(ServiceLimitsReport {
            service: id,
            limits: spec.limits(),
            support,
            watchdog,
        })
    }

    /// `service.idle` — when this service would be stopped for being unused, and what is stopping
    /// that from happening right now.
    ///
    /// **Four answers rather than one**, which is the whole reason this method exists beside the
    /// setting it reports: no policy, a policy switched off here, a running dependent, and a
    /// keep-warm project all look identical from outside — a service that stays running. Only two
    /// of them are settings anybody can change, and telling a person to go and change the wrong one
    /// is worse than saying nothing. Roadmap task **T69**.
    pub(super) async fn service_idle(&self, target: &ServiceTarget) -> Result<IdleReport, Error> {
        let id = self.named_service(target.service.as_ref())?;
        let spec = self.spec_of(&id).await?;

        let column = mixengine_core::services::idle_minutes(&self.store, &id)
            .await
            .map_err(|error| error.to_wire())?;

        // The recipe's own default, asked of the same catalogue the generator renders with and
        // through the same rule — its saving number while the home saves resources (T167b) — so
        // this report and the policy actually in force cannot disagree about what "unset" means.
        let saving = mixengine_core::services::save_resources::get(&self.store)
            .await
            .map_err(|error| error.to_wire())?;
        let recipe_default = crate::services::catalogue()
            .recipe(id.name())
            .and_then(|recipe| {
                recipe
                    .idle_default()
                    .or_else(|| saving.then(|| recipe.idle_when_saving()).flatten())
            });

        Ok(IdleReport {
            source: IdleSource::of(column, recipe_default, spec.idle()),
            policy: spec.idle().cloned(),
            exempt: self.exemptions_for(&id).await,
            service: id,
        })
    }

    /// `service.set_idle` — replace how long this service may look idle before it is stopped.
    ///
    /// **Nothing is applied here, unlike `service.set_limits` beside it.** A limit is a property of
    /// a running process and has to reach one; an idle policy is a statement about a future sweep,
    /// and the next one reads the row. So a service already past its new policy is not stopped by
    /// the call that set it — it is stopped by the sweep that follows, which is also what a person
    /// can watch happen.
    pub(super) async fn service_set_idle(
        &self,
        asked: &ServiceIdleSet,
    ) -> Result<IdleReport, Error> {
        let id = asked.service.clone();

        // Refused for a service nothing declares before anything is written, on `set_limits`'
        // reasoning: a setting accepted for a name nobody has is a row nobody can read back.
        let _ = self.spec_of(&id).await?;

        mixengine_core::services::set_idle(&self.store, &id, asked.minutes)
            .await
            .map_err(|error| error.to_wire())?;

        self.service_idle(&ServiceTarget {
            service: Some(id),
            project: None,
            wait: false,
        })
        .await
    }

    /// `service.save_resources` — whether this home stops services nobody is using (T167b).
    pub(super) async fn service_save_resources(&self) -> Result<SaveResources, Error> {
        let on = mixengine_core::services::save_resources::get(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        Ok(SaveResources { on })
    }

    /// `service.set_save_resources` — turn it on or off.
    ///
    /// **Nothing is stopped or started here**, on [`Self::service_set_idle`]'s reasoning: the next
    /// idle sweep rebuilds its graph from the rows and reads the switch there, so a service already
    /// past its saving policy is stopped by that sweep and not by this call.
    ///
    /// **Which is why the kept graph is forgotten** — roadmap task T190b. The sweep looks through
    /// the last walk's graph, and this changes every recipe's default policy without walking.
    pub(super) async fn service_set_save_resources(
        &self,
        asked: SaveResourcesSet,
    ) -> Result<SaveResources, Error> {
        mixengine_core::services::save_resources::set(&self.store, asked.on)
            .await
            .map_err(|error| error.to_wire())?;

        self.services.forget_graph();

        self.service_save_resources().await
    }

    /// `service.set_autostart` — replace whether this service starts when the daemon does.
    ///
    /// **Nothing is applied here**, on [`Self::service_set_idle`]'s reasoning: what this changes is
    /// what the walk at the next daemon start covers, and starting the service now would answer a
    /// different question than the one asked.
    ///
    /// Answers the service as it now is rather than a report of its own, unlike `set_idle` beside
    /// it: an idle policy has four readings that all look alike from outside and needs a type to
    /// tell them apart, where this one is a column with two values and a [`ServiceSummary`] already
    /// carries it. Roadmap task **T112**.
    pub(super) async fn service_set_autostart(
        &self,
        asked: &ServiceAutostartSet,
    ) -> Result<ServiceSummary, Error> {
        let id = asked.service.clone();

        // Refused for a service nothing declares before anything is written, on `set_idle`'s
        // reasoning: a setting accepted for a name nobody has is a row nobody can read back.
        let _ = self.spec_of(&id).await?;

        mixengine_core::services::set_autostart(&self.store, &id, asked.autostart)
            .await
            .map_err(|error| error.to_wire())?;

        self.service_status(&id).await
    }

    /// What is holding `id` open right now, whatever its policy says.
    ///
    /// **Reads the same two things the sweeper reads, through the same function**, so the report
    /// and the decision cannot drift: a client told nothing exempts a service, and a sweeper that
    /// then declines to stop it, would be two answers to one question.
    ///
    /// An empty answer on a failure to read either, which is the one place this differs from the
    /// sweeper: there, an unreadable keep-warm table skips the whole sweep because acting on it
    /// could stop the wrong service; here nothing is being acted on, and a report that failed
    /// entirely because one of its four answers was unavailable would be less useful than one
    /// missing that answer.
    async fn exemptions_for(&self, id: &ServiceId) -> Vec<mixengine_proto::IdleExemption> {
        let Ok(graph) = self.services.graph().await else {
            return Vec::new();
        };

        let Ok(warm) = mixengine_core::projects::kept_warm(&self.store).await else {
            return Vec::new();
        };

        crate::services::idle::exemptions(&graph, id, &self.services.supervised(), &warm)
    }

    /// The one service a `service.limits` call is about, or a refusal naming what is missing.
    ///
    /// **A limit belongs to one service and there is no sensible answer for "all of them"**, which
    /// is `service.status`'s reasoning: a call with no subject is one that was typed wrongly, and
    /// answering it as a list would hide that.
    fn named_service(&self, service: Option<&ServiceId>) -> Result<ServiceId, Error> {
        service.cloned().ok_or_else(|| {
            Error::new(
                ErrorCode::InvalidArgument,
                "which service's limits? name one",
            )
            .with_hint("`mix service limits <service>`")
        })
    }

    /// The declared spec for `id`, which is where a service's limits actually live.
    ///
    /// **The spec and not the `services` row**, although `limits_json` is a column on that row: the
    /// row is the *input* to configuration generation and the spec is what came out of it, so the
    /// spec is what the supervisor will actually apply. Reading the column directly would be a
    /// second path to one answer.
    async fn spec_of(&self, id: &ServiceId) -> Result<ServiceSpec, Error> {
        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;

        graph.spec(id).cloned().ok_or_else(|| {
            mixengine_core::Error::Graph(GraphError::NoSuchService { id: id.clone() }).to_wire()
        })
    }

    /// `service.set_limits` — replace what this service may take, and apply it now.
    ///
    /// Four steps, in an order each of which is why the next one is safe:
    ///
    /// 1. **Refuse what is wrong on any machine.** `ResourceLimits::validate`, beside the type.
    /// 2. **Refuse what is wrong on *this* machine** — a `cpu_percent` above `100 × cores`. Here and
    ///    not in `mixengine-proto`, because the number it is measured against is a property of the
    ///    machine and proto has no host to ask. The T68 design, D10.
    ///
    ///    **It guards small machines and nothing else, and that is worth knowing before reading it
    ///    as more than it is**: `cpu_percent` is a `u8`, so the largest value anybody can express is
    ///    255, which is already below the ceiling on any machine with three cores or more. What this
    ///    catches is a one-core VM or a constrained container being asked for two cores' worth. The
    ///    rule stays because it is correct and costs one comparison — not because it fires often.
    /// 3. **Write the row**, which is what a stopped service's next spawn will read.
    /// 4. **Push it at the runner**, which writes it into the live process. A service that is not
    ///    running skips this and has lost nothing — step 3 already said everything.
    pub(super) async fn service_set_limits(
        &self,
        asked: &ServiceLimitsSet,
    ) -> Result<ServiceLimitsReport, Error> {
        let id = asked.service.clone();

        // Refused before anything is written, and refused for a service that does not exist before
        // that: a limit accepted for a name nothing declares would be a row nobody could read back.
        // The spec is kept since T71a: whether a watchdog would restart this service is its answer.
        let spec = self.spec_of(&id).await?;

        asked.limits.validate().map_err(|reason| {
            Error::new(ErrorCode::InvalidArgument, reason)
                .with_hint("leave `memory_mb` unset to run this service uncapped")
        })?;

        let support = self.elevation.host().resource_control().support();
        let ceiling = support.cores.saturating_mul(100);

        if let Some(percent) = asked.limits.cpu_percent
            && u32::from(percent) > ceiling
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "`cpu_percent` is a percentage of one core, and this machine has \
                     {}, so {ceiling} is the whole of it",
                    support.cores
                ),
            )
            .with_hint("a ceiling above the machine's own is not a ceiling"));
        }

        mixengine_core::services::set_limits(&self.store, &id, asked.limits)
            .await
            .map_err(|error| error.to_wire())?;

        self.services.set_limits(&id, asked.limits);

        let watchdog = watchdog_of(&support, asked.limits, &spec, self.memory_over_minutes);

        Ok(ServiceLimitsReport {
            service: id,
            limits: asked.limits,
            support,
            watchdog,
        })
    }
}

/// What is watching this service's ceiling, if anything — roadmap task **T71a**.
///
/// **[`None`] on two different machines, and it is the same answer for both**: one whose kernel
/// holds the ceiling itself, and one where this service declared no ceiling at all. In each case
/// there is no loop to describe, and a client drawing a watchdog would be describing something that
/// never runs.
///
/// `restarts` is the *spec's* answer, which is the recipe's: a person may set `memory_mb` on
/// anything, and what happens at the end of the count is a property of the program.
fn watchdog_of(
    support: &LimitSupport,
    limits: ResourceLimits,
    spec: &ServiceSpec,
    after_minutes: u32,
) -> Option<MemoryWatchdog> {
    let watched = limits.memory_mb.is_some() && !matches!(support.memory, Enforcement::Hard { .. });

    watched.then(|| MemoryWatchdog {
        after_minutes,
        restarts: spec.restart_over_memory(),
    })
}
