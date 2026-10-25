//! `service.list`, `service.status` and the walks — `service.start`, `service.stop`,
//! `service.restart` and `service.reset_credential`.
//!
//! The walk itself is the registry's ([`crate::services`]). What is here is which services a
//! request names, the plan that reaches them, and the shape a client renders — which is also how
//! [`super::create`] reports the service it made or removed.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;

use mixengine_core::services::{GraphError, Plan, ServiceGraph, ServiceRecord, front_end};
use mixengine_proto::{
    Error, ErrorCode, ProjectRef, ResetCredential, ServiceFailure, ServiceId, ServiceList,
    ServiceRole, ServiceSummary, ServiceTarget, ServiceWalk, StateReason,
};

use super::Api;
use crate::error::ToWire as _;
use crate::services;

impl Api {
    /// `service.list` — every declared service and what it is doing.
    ///
    /// **Three readings composed, and each keeps its own authority.** The *set* comes from the
    /// declarations, so a listing cannot name something `service.start` would not find; the *state*
    /// comes from the `services` row, which is the same value `ServiceStateChanged` announced; and
    /// whether a task is supervising it comes from the registry, which is a different question and
    /// is reported as one. Nothing here re-derives a fact another layer already owns.
    ///
    /// An empty list is a real answer and not a failure: a home that declares no service has
    /// nothing to list.
    pub(super) async fn service_list(&self) -> Result<ServiceList, Error> {
        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;
        let rows = mixengine_core::services::records(&self.store)
            .await
            .map_err(|error| error.to_wire())?;
        let supervised = self.services.supervised();

        // Built once for the listing and not once per row: what a package is for is the same
        // answer for every service in it.
        let catalogue = crate::services::catalogue();

        let services = graph
            .ids()
            .map(|id| summary(&graph, &catalogue, id, rows.get(id.as_str()), &supervised))
            .collect();

        Ok(ServiceList { services })
    }

    /// `service.status` — the same sentence about one of them.
    ///
    /// A separate read of the one row rather than a filtered [`Api::service_list`]: the question is
    /// about one service, and answering it by reading every row would make a home with forty of them
    /// pay for thirty-nine it did not ask about.
    pub(super) async fn service_status(&self, id: &ServiceId) -> Result<ServiceSummary, Error> {
        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;

        if graph.spec(id).is_none() {
            return Err(
                mixengine_core::Error::Graph(GraphError::NoSuchService { id: id.clone() })
                    .to_wire(),
            );
        }

        // A declared service with no row is reported rather than refused — see `summary`.
        let record = match mixengine_core::services::record(&self.store, id).await {
            Ok(record) => Some(record),
            Err(mixengine_core::Error::NotFound { .. }) => None,
            Err(error) => return Err(error.to_wire()),
        };

        Ok(summary(
            &graph,
            &crate::services::catalogue(),
            id,
            record.as_ref(),
            &self.services.supervised(),
        ))
    }

    /// `service.start` — bring a service up, and everything it depends on with it.
    ///
    /// **Three subjects, and the third is the daemon answering a question no client may.** A named
    /// service is the transitive set below it; nothing named is every declared service; and
    /// [`ServiceTarget::project`] is what one project needs, which is [`Self::project_roots`]'s to
    /// work out. Before the third existed a client that wanted it asked for the second, so applying
    /// a blueprint on a home with four PHP versions and three databases started all of them.
    pub(super) async fn service_start(&self, target: &ServiceTarget) -> Result<ServiceWalk, Error> {
        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;

        let roots = match (&target.service, &target.project) {
            (Some(_), Some(_)) => return Err(two_subjects()),
            (None, Some(project)) => Some(self.project_roots(project, &graph).await?),
            (service, None) => service.as_ref().map(|id| vec![id.clone()]),
        };

        let plan = start_plan(&graph, roots.as_deref())?;

        self.walk(
            target.wait,
            plan.flat().cloned().collect(),
            move |services| async move {
                let walk = services.start(&graph, &plan).await;

                (walk, "start")
            },
        )
        .await
    }

    /// Every service one project needs, for [`ServiceTarget::project`].
    ///
    /// **Two readings, because the front end is not a service any site declares.**
    /// [`mixengine_core::sites::services_of`] answers what the project's own sites named — the database and the
    /// cache `blueprint.apply` wrote into `site_service_links`, and the php-fpm pool — and
    /// [`front_end::held_by`] answers which program those sites are reached *through*. Starting the
    /// first set without the second is a project whose parts are all running and whose address
    /// answers nothing.
    ///
    /// **A root the graph does not know is passed over rather than refused.** `service.delete` can
    /// take a pool row while a site still names it, and a project that cannot be started at all
    /// because one link dangles is a worse answer than one started without it — the start plan is
    /// the transitive set below what is left, which is still everything that can run.
    ///
    /// An empty set is not an error either: a project with no sites yet needs nothing, and a walk
    /// over nothing is a truthful empty answer.
    async fn project_roots(
        &self,
        reference: &ProjectRef,
        graph: &ServiceGraph,
    ) -> Result<Vec<ServiceId>, Error> {
        let project = mixengine_core::projects::find(&self.store, reference)
            .await
            .map_err(|error| error.to_wire())?
            .ok_or_else(|| {
                let said = match reference {
                    ProjectRef::Name(name) => format!("no such project: {name}"),
                    ProjectRef::Path(path) => {
                        format!("no project is registered at or above {path}")
                    }
                };

                Error::new(ErrorCode::NotFound, said)
                    .with_hint("`mix project list` shows what does exist")
            })?;

        let mut roots = mixengine_core::sites::services_of(&self.store, project.id)
            .await
            .map_err(|error| error.to_wire())?;

        let front_end = front_end::held_by(&self.store, &crate::services::catalogue())
            .await
            .map_err(|error| error.to_wire())?
            .and_then(|id| ServiceId::parse(id).ok());

        if let Some(front_end) = front_end {
            roots.push(front_end);
        }

        roots.retain(|id| graph.spec(id).is_some());
        roots.sort();
        roots.dedup();

        Ok(roots)
    }

    /// `service.stop` — take a service down, and everything that depends on it first.
    ///
    /// **A stop can fail**, and since crash recovery there is exactly one way: a process that
    /// outlived a previous daemon, was adopted by this one, and would not die. What comes back then
    /// names it, with no reason attached — see [`Registry::stop`](crate::services::Registry::stop).
    pub(super) async fn service_stop(&self, target: &ServiceTarget) -> Result<ServiceWalk, Error> {
        refuse_project_scope(target, "stop")?;

        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;
        let plan = stop_plan(&graph, target.service.as_ref())?;

        self.walk(
            target.wait,
            plan.flat().cloned().collect(),
            move |services| async move {
                let walk = services.stop(&plan).await;

                (walk, "stop")
            },
        )
        .await
    }

    /// `service.restart` — take it down, and put back exactly what went down with it.
    ///
    /// **The two halves name different sets, and that asymmetry is the whole of this method.**
    /// Restarting MariaDB stops everything that depends on it, so starting MariaDB again would leave
    /// `php-fpm` where the stop left it: down, on behalf of a user who asked for a restart and got
    /// half of one. So what is started is what the stop *took down*, in start order — which
    /// [`ServiceGraph::start_plan`] computes for a set as readily as for one service, and which also
    /// pulls in anything that set depends on and was not already up.
    ///
    /// **Took down, not covered**: the two differ, and reading the stop plan as the start's would
    /// make a restart of MariaDB start every dependent a user had deliberately stopped. See
    /// [`services::restarted`].
    ///
    /// What comes back describes the *start*, in the ordinary case where the stop reached everything
    /// it was asked to; the plan reported is then the one that was walked second.
    ///
    /// **When the stop fails, that is what comes back instead, and the start never happens.** The
    /// one way it can is a survivor this daemon adopted and could not kill — a process still
    /// holding the port and the data directory — and starting the service again on top of it would
    /// put a second one there to collide with the first. A restart that could not take the service
    /// down has not restarted it, and says so.
    pub(super) async fn service_restart(
        &self,
        target: &ServiceTarget,
    ) -> Result<ServiceWalk, Error> {
        refuse_project_scope(target, "restart")?;

        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;
        let down = stop_plan(&graph, target.service.as_ref())?;

        // Read before anything is stopped, because afterwards nothing is supervised and every
        // service would look like one that had been down all along.
        let roots =
            services::restarted(target.service.as_ref(), &down, &self.services.supervised());

        // Cannot fail: every id in it came out of this same graph. Mapped rather than unwrapped all
        // the same, because a panic here would be one bad request taking the daemon with it.
        let up = graph
            .start_plan(roots.iter())
            .map_err(|error| mixengine_core::Error::Graph(error).to_wire())?;

        self.walk(
            target.wait,
            up.flat().cloned().collect(),
            move |services| async move {
                // **The walk itself is `services::stop_then_start`**, shared with the memory
                // watchdog: what is left here is the reporting this method owes a client and the
                // target semantics only an RPC has.
                (
                    services::stop_then_start(&services, &graph, &down, &up).await,
                    "restart",
                )
            },
        )
        .await
    }

    /// `service.reset_credential` — the repair for a server whose password nothing knows any more.
    ///
    /// **[`Self::service_restart`] with steps in the middle**, and deliberately the same two plans:
    /// the stop takes down everything that depends on this service, and what is started again is
    /// what the stop *took down* rather than everything it covered. A repair that started the
    /// covered set would put back dependents somebody had deliberately stopped.
    ///
    /// **The named service always ends running, whatever state it was in.** The proof that a reset
    /// worked is the readiness check, which for all three of these recipes is an authenticated
    /// query; a repair that ended with the service stopped would be reporting an exit code and
    /// calling it a repair, and `postgres --single` exits 0 on a syntax error.
    ///
    /// **The stop carries [`StateReason::CredentialReset`]**, which `StoppedBy::of` reads as a
    /// person's stop — so nothing holds this service's addresses while a step is writing into its
    /// data directory, and a connection arriving mid-repair is refused rather than answered by a
    /// server started on top of the work.
    ///
    /// **Where the steps fail, nothing is started.** A database whose credential is half re-set is
    /// not one to put back in front of an application; what is left is a stopped service and a job
    /// row saying why.
    ///
    /// # Errors
    ///
    /// `invalid_argument` for a service that keeps no credential of its own — asked **before**
    /// anything is stopped, so naming a web server costs nothing. Then whatever the repair reports.
    pub(super) async fn service_reset_credential(
        &self,
        reset: &ResetCredential,
    ) -> Result<ServiceWalk, Error> {
        if !self.services.has_credential_reset(&reset.service) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "{} keeps no superuser credential of its own, so there is nothing here to \
                     re-set",
                    reset.service
                ),
            )
            .with_hint(
                "only the database servers keep one; `mix service list` shows what this home runs",
            ));
        }

        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;
        let down = stop_plan(&graph, Some(&reset.service))?;

        // Read before anything is stopped, for `service_restart`'s reason: afterwards nothing is
        // supervised and every service would look like one that had been down all along.
        let roots = services::restarted(Some(&reset.service), &down, &self.services.supervised());

        // Cannot fail: every id in it came out of this same graph. Mapped rather than unwrapped, so
        // that a bug here is one bad request rather than the daemon.
        let up = graph
            .start_plan(roots.iter())
            .map_err(|error| mixengine_core::Error::Graph(error).to_wire())?;

        let service = reset.service.clone();

        self.walk(
            reset.wait,
            up.flat().cloned().collect(),
            move |services| async move {
                let stopped = services
                    .stop_because(&down, &StateReason::CredentialReset)
                    .await;

                if stopped.failed.is_some() {
                    return (stopped, "reset");
                }

                if let Err(error) = services.reset_credential(&service).await {
                    tracing::error!(
                        service = service.as_str(),
                        %error,
                        "a credential reset did not finish; this service is left stopped"
                    );

                    return (stopped, "reset");
                }

                (services.start(&graph, &up).await, "reset")
            },
        )
        .await
    }

    /// Run a walk here, or behind the answer — the whole of what [`ServiceTarget::wait`] chooses.
    ///
    /// **A walk that is not waited for is still bounded by the daemon's own life.** It runs in the
    /// daemon's `Background`, which the shutdown drains before the store closes, and is cut short by
    /// the root token, per the rule in `docs/standards/rust.md` against a task that outlives
    /// shutdown: the services it started keep their runners, which are the registry's and were never
    /// this task's to hold.
    ///
    /// Its outcome goes to `daemon.log`, because nobody is waiting to be told: a client that asked
    /// not to wait is reading `ServiceStateChanged`, where every move in the walk appears as it
    /// happens — the walk's own summary is the one thing that stream does not carry.
    async fn walk<F, W>(
        &self,
        wait: bool,
        planned: Vec<ServiceId>,
        walking: F,
    ) -> Result<ServiceWalk, Error>
    where
        F: FnOnce(Arc<services::Registry>) -> W + Send + 'static,
        W: Future<Output = (services::Walk, &'static str)> + Send + 'static,
    {
        let services = Arc::clone(&self.services);

        if wait {
            let (walk, _) = walking(services).await;

            return Ok(walked(planned, walk));
        }

        let shutdown = self.shutdown.token().clone();
        let accepted = planned.clone();

        self.shutdown.background().spawn("unwaited walk", async move {
            tokio::select! {
                () = shutdown.cancelled() => {
                    tracing::info!("a walk that was not waited for was cut short by the shutdown");
                }

                (walk, what) = walking(services) => {
                    tracing::info!(
                        %what,
                        reached = walk.reached.len(),
                        failed = walk.failed.as_ref().map(|(id, _)| id.as_str()),
                        blocked = walk.blocked.len(),
                        "a walk nobody was waiting for has finished"
                    );
                }
            }
        });

        Ok(ServiceWalk {
            planned: accepted,
            complete: false,
            reached: Vec::new(),
            failed: None,
            blocked: Vec::new(),
        })
    }
}

/// What must start for `roots` to be running, or for everything to be.
fn start_plan(graph: &ServiceGraph, roots: Option<&[ServiceId]>) -> Result<Plan, Error> {
    match roots {
        Some(roots) => graph
            .start_plan(roots)
            .map_err(|error| mixengine_core::Error::Graph(error).to_wire()),
        None => Ok(graph.start_order()),
    }
}

/// The refusal for a request naming a service *and* a project.
///
/// Two subjects is not a request with a winner, so neither is picked: resolving it by precedence
/// would carry out the half the caller did not mean, silently, on somebody's machine.
fn two_subjects() -> Error {
    Error::new(
        ErrorCode::InvalidArgument,
        "a service and a project are two subjects, and this call acts on one".to_owned(),
    )
    .with_hint("send `service` for one service, `project` for what one project needs, neither for every declared service")
}

/// The refusal `service.stop` and `service.restart` give a project-scoped request.
///
/// **Not an omission to be filled in later without thinking about it.** What a project needs
/// includes the front end every *other* site on the machine is reached through, so a project-scoped
/// stop is a machine-wide outage wearing one project's name. A stop that means something narrower
/// is a different set — one this type cannot express by simply being allowed through.
fn refuse_project_scope(target: &ServiceTarget, verb: &str) -> Result<(), Error> {
    if target.project.is_none() {
        return Ok(());
    }

    Err(Error::new(
        ErrorCode::InvalidArgument,
        format!(
            "`service.{verb}` has no project scope: what a project needs includes the front end \
             every other site is reached through"
        ),
    )
    .with_hint(format!(
        "`mix service {verb} <service>` names one, and leaving it out {verb}s every declared service"
    )))
}

/// The opposite walk — **not** the same one reversed. See [`ServiceGraph::stop_plan`].
fn stop_plan(graph: &ServiceGraph, service: Option<&ServiceId>) -> Result<Plan, Error> {
    match service {
        Some(id) => graph
            .stop_plan([id])
            .map_err(|error| mixengine_core::Error::Graph(error).to_wire()),
        None => Ok(graph.stop_order()),
    }
}

/// One service, as the three readings that know about it describe it.
///
/// `record` is [`None`] for a service that is declared and has no `services` row. That is not a
/// case a finished MixEngine reaches — a declaration is rendered *from* a row — and it is reported
/// rather than smoothed into `stopped`, because a service that claims to be stopped and then
/// refuses to start explains nothing to whoever declared it.
pub(super) fn summary(
    graph: &ServiceGraph,
    catalogue: &mixengine_core::generate::Catalogue,
    id: &ServiceId,
    record: Option<&ServiceRecord>,
    supervised: &BTreeSet<ServiceId>,
) -> ServiceSummary {
    ServiceSummary {
        id: id.clone(),
        state: record.map(|record| record.state),
        supervised: supervised.contains(id),
        pid: record.and_then(|record| record.pid),
        // The row's column, which is where the number was decided — not the spec's list, which is
        // what that number was rendered into. A service with no row has no port to report, exactly
        // as it has no state.
        port: record.and_then(|record| record.port),
        last_started_at: record.and_then(|record| record.last_started_at),
        last_exit_code: record.and_then(|record| record.last_exit_code),
        // The graph's edges rather than the spec's list: each dependency once, in id order. A
        // service that is in the graph has an entry, so the failure is unreachable — and it is
        // answered with "none declared" rather than a panic, on the same principle as everything
        // else in this module.
        depends_on: graph
            .dependencies_of(id)
            .map(|dependencies| dependencies.iter().cloned().collect())
            .unwrap_or_default(),
        role: Some(role_of(catalogue, id)),
        port_movable: None,
        // The row's column, on `port`'s rule: a service with no row has no setting to report,
        // exactly as it has no state — and `false` is the reading that cannot be acted on, which is
        // the safer of the two for something a boot walk reads.
        autostart: record.is_some_and(|record| record.autostart),
        // Only about a service that *is* stopped: the column keeps the last word after a
        // start, and a running service reported as "stopped by a person" would be a client drawing
        // a state that is over.
        stopped_by: record
            .filter(|record| record.state == mixengine_proto::ServiceState::Stopped)
            .map(|record| record.stopped_by.into()),
        // The row's parent's version, on `port`'s rule: no row, no version to report.
        version: record.and_then(|record| record.version.clone()),
        // Only about a service that *is* failed, on `stopped_by`'s rule above.
        last_failure: record
            .filter(|record| record.state == mixengine_proto::ServiceState::Failed)
            .and_then(|record| record.last_failure.clone()),
    }
}

/// What a package is *for*, as the catalogue answers it.
///
/// **By what the recipe says, never by the name.** That is the whole of what this member exists to
/// stop a client doing, so deriving it here from a string would only move the copy one crate down.
///
/// A row this build has no recipe for is [`ServiceRole::Other`] rather than absent: `None` on the
/// wire means *the daemon predates the member* (ADR 0019), and a service MixEngine cannot configure
/// is certainly not the program every site is reached through — which is the same answer
/// [`front_end::held_by`] already gives such a row
/// when it passes over it.
fn role_of(catalogue: &mixengine_core::generate::Catalogue, id: &ServiceId) -> ServiceRole {
    match catalogue.recipe(id.name()).map(|recipe| recipe.role()) {
        Some(mixengine_core::generate::Role::FrontEnd(server)) => ServiceRole::FrontEnd { server },
        Some(mixengine_core::generate::Role::Other) | None => ServiceRole::Other {},
    }
}

/// A finished walk, in the shape a client renders.
pub(super) fn walked(planned: Vec<ServiceId>, walk: services::Walk) -> ServiceWalk {
    ServiceWalk {
        planned,
        complete: true,
        reached: walk.reached,
        failed: walk.failed.map(|(service, fault)| ServiceFailure {
            service,
            reason: fault.reason,
            detail: fault.detail,
        }),
        blocked: walk.blocked,
    }
}

#[cfg(test)]
mod tests;
