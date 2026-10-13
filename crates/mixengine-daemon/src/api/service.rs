//! `service.list`, `service.status` and the walks — `service.start`, `service.stop`,
//! `service.restart` and `service.reset_credential`; roadmap tasks **T19a** and **T125**.
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
    /// An empty list is a real answer and not a failure: until T30 renders a `services` row into a
    /// runnable spec, this build declares nothing at all.
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
    /// **Three subjects, and the third is the daemon answering a question no client may** — roadmap
    /// task **T125**. A named service is the transitive set below it; nothing named is every
    /// declared service; and [`ServiceTarget::project`] is what one project needs, which is
    /// [`Self::project_roots`]'s to work out. Until T125 a client that wanted the third asked for
    /// the second, so applying a blueprint on a home with four PHP versions and three databases
    /// started all of them.
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

    /// Every service one project needs, for [`ServiceTarget::project`] — roadmap task **T125**.
    ///
    /// **Two readings, because the front end is not a service any site declares.**
    /// [`mixengine_core::sites::services_of`] answers what the project's own sites named — the database and the
    /// cache `blueprint.apply` wrote into `site_service_links`, and the php-fpm pool — and
    /// [`front_end::held_by`] answers which program those sites are reached *through*. Starting the
    /// first set without the second is a project whose parts are all running and whose address
    /// answers nothing.
    ///
    /// **A root the graph does not know is passed over rather than refused.** That is T122's shape:
    /// `service.delete` can take a pool row while a site still names it, and a project that cannot
    /// be started at all because one link dangles is a worse answer than one started without it —
    /// the start plan is the transitive set below what is left, which is still everything that can
    /// run.
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
    /// **A stop can fail**, and since T18 there is exactly one way: a process that outlived a
    /// previous daemon, was adopted by this one, and would not die. What comes back then names it,
    /// with no reason attached — see [`Registry::stop`](crate::services::Registry::stop).
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
    /// **When the stop fails, that is what comes back instead, and the start never happens.** The one
    /// way it can (T18) is a survivor this daemon adopted and could not kill — a process still
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
                // watchdog since T71a: what is left here is the reporting this method owes a client
                // and the target semantics only an RPC has.
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
    /// **The named service always ends running, whatever state it was in** — the design's D7. The
    /// proof that a reset worked is the readiness check, which for all three of these recipes is an
    /// authenticated query; a repair that ended with the service stopped would be reporting an exit
    /// code and calling it a repair, and `postgres --single` exits 0 on a syntax error.
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
    /// **A walk that is not waited for is still bounded by the daemon's own life.** It is cancelled
    /// by the root token rather than detached, per the rule in `docs/standards/rust.md` against a
    /// task that outlives shutdown: the services it started keep their runners, which are the
    /// registry's and were never this task's to hold.
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

        tokio::spawn(async move {
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

/// The refusal for a request naming a service *and* a project — roadmap task **T125**.
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

/// The refusal `service.stop` and `service.restart` give a project-scoped request — **T125**.
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
/// `record` is [`None`] for a service that is declared and has no `services` row. That is not a case
/// a finished MixEngine reaches — from T30 a declaration is rendered *from* a row — and it is
/// reported rather than smoothed into `stopped`, because a service that claims to be stopped and
/// then refuses to start explains nothing to whoever declared it.
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
        // The row's column, on `port`'s rule: a service with no row has no setting to report,
        // exactly as it has no state — and `false` is the reading that cannot be acted on, which is
        // the safer of the two for something a boot walk reads.
        autostart: record.is_some_and(|record| record.autostart),
        // Only about a service that *is* stopped (T167d): the column keeps the last word after a
        // start, and a running service reported as "stopped by a person" would be a client drawing
        // a state that is over.
        stopped_by: record
            .filter(|record| record.state == mixengine_proto::ServiceState::Stopped)
            .map(|record| record.stopped_by.into()),
        // The row's parent's version (T183), on `port`'s rule: no row, no version to report.
        version: record.and_then(|record| record.version.clone()),
        // Only about a service that *is* failed (T200b, D5), on `stopped_by`'s rule above.
        last_failure: record
            .filter(|record| record.state == mixengine_proto::ServiceState::Failed)
            .and_then(|record| record.last_failure.clone()),
    }
}

/// What a package is *for*, as the catalogue answers it — roadmap task **T97**.
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
mod tests {
    use mixengine_proto::rpc;
    use mixengine_proto::{Millis, ReadyCheck, ServiceState};
    use mixengine_testkit::FakeService;
    use serde_json::Value;

    use super::*;
    use crate::api::test_daemon::{daemon, undeclared, web_and_db};
    use crate::services::fixture;

    #[tokio::test]
    async fn a_home_that_declares_nothing_lists_nothing_rather_than_failing() {
        // The honest answer for this build, and the one the registry, the graph and the walk all
        // handle without a special case: `Undeclared` until T30 renders a row into a spec.
        let list: ServiceList = undeclared()
            .await
            .expect(rpc::method::SERVICE_LIST, Value::Null)
            .await;

        assert!(list.services.is_empty(), "{list:?}");
    }

    #[tokio::test]
    async fn a_listing_names_the_declared_set_with_what_each_row_says() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let list: ServiceList = daemon.expect(rpc::method::SERVICE_LIST, Value::Null).await;

        let ids: Vec<&str> = list.services.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(
            ids,
            ["db", "web"],
            "in id order, which is a listing's order"
        );

        let web = &list.services[1];
        assert_eq!(web.state, Some(ServiceState::Stopped), "what the row says");
        assert!(!web.supervised, "nothing has been started");
        assert_eq!(web.pid, None);
        assert_eq!(
            web.depends_on,
            vec![fixture::service("db")],
            "the graph's edge, which is what makes a start order explicable"
        );
    }

    // The project scope — roadmap task **T125**. What the set *is* belongs to
    // `core::sites::services_of`, which is tested where it lives; what is proved here is the three
    // requests this surface has to answer with a refusal rather than with a walk.

    #[tokio::test]
    async fn a_start_scoped_to_a_project_nothing_answers_to_is_not_found() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let answer = daemon
            .ask(
                rpc::method::SERVICE_START,
                serde_json::json!({"project": {"name": "nope"}}),
            )
            .await;

        assert_eq!(answer["error"]["data"]["code"], "not_found");
        assert!(
            answer["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("nope"),
            "{answer}"
        );
    }

    /// **Two subjects is refused rather than resolved by precedence.** Picking one would carry out
    /// the half the caller did not mean — silently, on somebody's machine.
    #[tokio::test]
    async fn a_start_naming_a_service_and_a_project_is_refused() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let answer = daemon
            .ask(
                rpc::method::SERVICE_START,
                serde_json::json!({"service": "db", "project": {"name": "blog"}}),
            )
            .await;

        assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
        assert_eq!(
            daemon.state("db").await,
            Some(ServiceState::Stopped),
            "a refused request starts nothing"
        );
    }

    /// **A reset names one service, and no service at all is refused by the type** — roadmap task
    /// **T127**.
    ///
    /// T125's finding was that `ServiceTarget::service` is optional and [`None`] means *the whole
    /// home*; a repair that could be asked for with no subject is that mistake with worse
    /// consequences. `ResetCredential::service` is required, so this is a deserialisation refusal
    /// rather than a check somebody could delete.
    #[tokio::test]
    async fn a_credential_reset_requires_a_service() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        for params in [
            serde_json::json!({}),
            serde_json::json!({ "project": { "name": "blog" } }),
        ] {
            let answer = daemon
                .ask(rpc::method::SERVICE_RESET_CREDENTIAL, params.clone())
                .await;

            assert_eq!(
                answer["error"]["data"]["code"], "invalid_argument",
                "{params} {answer}"
            );
        }
    }

    /// **A service that keeps no credential is refused before anything is stopped** — **T127**.
    ///
    /// The second assertion is the one worth having: a repair that refused *after* the stop would
    /// take a web server down to tell somebody they named the wrong thing.
    #[tokio::test]
    async fn a_service_with_no_credential_is_refused_without_being_stopped() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        daemon
            .ask(
                rpc::method::SERVICE_START,
                serde_json::json!({ "service": "web" }),
            )
            .await;

        let answer = daemon
            .ask(
                rpc::method::SERVICE_RESET_CREDENTIAL,
                serde_json::json!({ "service": "web", "wait": true }),
            )
            .await;

        assert_eq!(
            answer["error"]["data"]["code"], "invalid_argument",
            "{answer}"
        );
        assert_eq!(
            daemon.state("web").await,
            Some(ServiceState::Running),
            "a refused reset stopped the service anyway"
        );
    }

    /// **A project has no stop.** The set includes the front end every *other* site on the machine
    /// is reached through, so a project-scoped stop is a machine-wide outage wearing one project's
    /// name — and an accepted request would be exactly that, not an error a user can see.
    #[tokio::test]
    async fn a_stop_and_a_restart_have_no_project_scope() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        for method in [rpc::method::SERVICE_STOP, rpc::method::SERVICE_RESTART] {
            let answer = daemon
                .ask(method, serde_json::json!({"project": {"name": "blog"}}))
                .await;

            assert_eq!(
                answer["error"]["data"]["code"], "invalid_argument",
                "{method} answered {answer}"
            );
        }
    }

    /// **T97.** A listing says what each service is *for*, so a client never has to decide that
    /// `nginx` means "front end" for itself.
    ///
    /// Written with nginx rather than Caddy on purpose, which is `front_end::held_by`'s own test
    /// rule: the answer has to come from the recipe, and a lookup that had happened to be a
    /// comparison against the string `caddy` would pass the other way round and fail this.
    #[tokio::test]
    async fn a_listing_says_what_each_service_is_for() {
        let declared = Arc::new(fixture::Declared(vec![
            fixture::spec("mariadb@main")
                .build()
                .expect("a usable spec"),
            fixture::spec("nginx").build().expect("a usable spec"),
        ]));
        let daemon = daemon(declared, &["mariadb@main", "nginx"]).await;

        let list: ServiceList = daemon.expect(rpc::method::SERVICE_LIST, Value::Null).await;

        let role = |id: &str| {
            list.services
                .iter()
                .find(|one| one.id.as_str() == id)
                .unwrap_or_else(|| panic!("{id} is in the listing"))
                .role
        };

        assert_eq!(
            role("nginx"),
            Some(ServiceRole::FrontEnd {
                server: mixengine_proto::FrontEndServer::Nginx
            }),
            "the program every site is reached through, by what its recipe is for"
        );
        assert_eq!(
            role("mariadb@main"),
            Some(ServiceRole::Other {}),
            "a database is not one of them"
        );
    }

    #[tokio::test]
    async fn a_service_that_is_declared_and_has_no_row_is_reported_without_a_state() {
        // Not a case a finished MixEngine reaches — from T30 a declaration is rendered *from* a row
        // — and reported rather than smoothed into `stopped`, because a service that claims to be
        // stopped and then refuses to start explains nothing to whoever declared it.
        let daemon = daemon(web_and_db(), &["db"]).await;

        let list: ServiceList = daemon.expect(rpc::method::SERVICE_LIST, Value::Null).await;

        assert_eq!(list.services[0].state, Some(ServiceState::Stopped), "db");
        assert_eq!(list.services[1].state, None, "web, which has no row");
    }

    #[tokio::test]
    async fn the_status_of_a_service_nobody_declared_is_not_found() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let answer = daemon
            .ask(
                rpc::method::SERVICE_STATUS,
                serde_json::json!({"service": "mailpit"}),
            )
            .await;

        assert_eq!(answer["error"]["data"]["code"], "not_found");
        assert_eq!(
            answer["error"]["code"], -32000,
            "the method ran and the work did not: an application error, not a protocol one"
        );
    }

    #[tokio::test]
    async fn a_status_with_no_service_is_refused_rather_than_answered_as_a_listing() {
        let answer = undeclared()
            .await
            .ask(rpc::method::SERVICE_STATUS, serde_json::json!({}))
            .await;

        assert_eq!(answer["error"]["code"], -32602, "{answer}");
        assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
    }

    #[tokio::test]
    async fn starting_one_service_starts_what_it_depends_on_and_says_what_it_reached() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let walk: ServiceWalk = daemon
            .expect(
                rpc::method::SERVICE_START,
                serde_json::json!({"service": "web"}),
            )
            .await;

        assert!(walk.complete, "the default is to wait: {walk:?}");
        assert_eq!(
            walk.planned,
            vec![fixture::service("db"), fixture::service("web")],
            "a plan is the transitive set, in the order it was walked"
        );
        assert_eq!(walk.reached, walk.planned, "{walk:?}");
        assert!(walk.failed.is_none(), "{walk:?}");

        let summary: ServiceSummary = daemon
            .expect(
                rpc::method::SERVICE_STATUS,
                serde_json::json!({"service": "db"}),
            )
            .await;
        assert_eq!(summary.state, Some(ServiceState::Running));
        assert!(summary.supervised, "a task is holding it");
        assert!(summary.pid.is_some(), "the process it is running as");

        daemon.quiet().await;
    }

    #[tokio::test]
    async fn a_start_that_is_not_waited_for_answers_with_the_plan_and_walks_behind_it() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let walk: ServiceWalk = daemon
            .expect(
                rpc::method::SERVICE_START,
                serde_json::json!({"service": "web", "wait": false}),
            )
            .await;

        assert!(!walk.complete, "nothing has happened yet: {walk:?}");
        assert_eq!(walk.planned.len(), 2, "the plan is still what was accepted");
        assert!(
            walk.reached.is_empty() && walk.failed.is_none(),
            "an empty walk, and it says so through `complete` rather than by looking finished"
        );

        // The walk carries on inside the daemon, which is the whole of what this mode promises.
        daemon.until("web", ServiceState::Running).await;

        daemon.quiet().await;
    }

    #[tokio::test]
    async fn a_dependency_that_fails_stops_the_walk_and_blocks_what_needed_it() {
        let never = FakeService::new().never_ready();
        let specs = Arc::new(fixture::Declared(vec![
            fixture::spec("db")
                .args(fixture::arguments(&never))
                .ready(ReadyCheck::LogPattern {
                    regex: mixengine_testkit::service::READY_LINE.to_owned(),
                    timeout: Millis(750),
                })
                .build()
                .expect("a usable spec"),
            fixture::spec("web")
                .depends_on(fixture::service("db"))
                .build()
                .expect("a usable spec"),
        ]));
        let daemon = daemon(specs, &["db", "web"]).await;

        let walk: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, Value::Null).await;

        assert!(walk.reached.is_empty(), "{walk:?}");
        let failed = walk.failed.clone().expect("the walk stopped somewhere");
        assert_eq!(failed.service, fixture::service("db"));
        assert!(
            matches!(
                failed.reason,
                Some(mixengine_proto::StateReason::ReadyTimeout { .. })
            ),
            "the reason the transition carried, not one invented here: {failed:?}"
        );
        assert_eq!(
            walk.blocked,
            vec![fixture::service("web")],
            "fail-fast: what needed it was never spawned"
        );

        // And the row says the same thing, with the edge that broke named on it.
        assert_eq!(daemon.state("web").await, Some(ServiceState::Failed));

        daemon.quiet().await;
    }

    #[tokio::test]
    async fn stopping_a_service_takes_down_what_depends_on_it_first() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let _: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, Value::Null).await;

        let walk: ServiceWalk = daemon
            .expect(
                rpc::method::SERVICE_STOP,
                serde_json::json!({"service": "db"}),
            )
            .await;

        assert_eq!(
            walk.planned,
            vec![fixture::service("web"), fixture::service("db")],
            "the opposite walk: a site is never left pointed at a database that is going away"
        );
        assert_eq!(walk.reached, walk.planned);
        assert!(
            walk.failed.is_none(),
            "a stop has no state it fails to reach"
        );

        assert_eq!(daemon.state("db").await, Some(ServiceState::Stopped));
        assert_eq!(daemon.state("web").await, Some(ServiceState::Stopped));

        daemon.quiet().await;
    }

    /// The one that says what `restart` means, and the reason it is not stop-then-start-the-same-id.
    #[tokio::test]
    async fn restarting_a_dependency_puts_back_everything_it_took_down() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let _: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, Value::Null).await;
        let before = daemon
            .expect::<ServiceSummary>(
                rpc::method::SERVICE_STATUS,
                serde_json::json!({"service": "db"}),
            )
            .await
            .pid;

        let walk: ServiceWalk = daemon
            .expect(
                rpc::method::SERVICE_RESTART,
                serde_json::json!({"service": "db"}),
            )
            .await;

        // Stopping `db` takes `web` with it; starting `db` alone would have left it there.
        assert_eq!(
            walk.planned,
            vec![fixture::service("db"), fixture::service("web")],
            "what the stop covered, in start order: {walk:?}"
        );
        assert_eq!(walk.reached, walk.planned, "{walk:?}");

        let after = daemon
            .expect::<ServiceSummary>(
                rpc::method::SERVICE_STATUS,
                serde_json::json!({"service": "db"}),
            )
            .await;

        assert_eq!(after.state, Some(ServiceState::Running));
        assert_ne!(after.pid, before, "a restart is a different process");
        assert_eq!(
            daemon.state("web").await,
            Some(ServiceState::Running),
            "the dependent came back rather than being left where the stop put it"
        );

        daemon.quiet().await;
    }

    /// The other half of what `restart` means: it puts back what it took down, and nothing else.
    #[tokio::test]
    async fn restarting_a_dependency_leaves_a_dependent_that_was_down_where_it_was() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        // Only `db`. A plan for one service pulls in what it depends on, and `web` depends on `db`
        // rather than the other way about, so `web` is deliberately left stopped.
        let _: ServiceWalk = daemon
            .expect(
                rpc::method::SERVICE_START,
                serde_json::json!({"service": "db"}),
            )
            .await;
        assert_eq!(daemon.state("web").await, Some(ServiceState::Stopped));

        let walk: ServiceWalk = daemon
            .expect(
                rpc::method::SERVICE_RESTART,
                serde_json::json!({"service": "db"}),
            )
            .await;

        assert_eq!(
            walk.planned,
            vec![fixture::service("db")],
            "the stop covered `web` and took nothing down there: {walk:?}"
        );
        assert_eq!(
            daemon.state("web").await,
            Some(ServiceState::Stopped),
            "a restart of a dependency is not a request to start its dependents"
        );

        daemon.quiet().await;
    }

    #[tokio::test]
    async fn starting_a_service_that_is_not_declared_is_not_found_rather_than_a_walk() {
        let daemon = daemon(web_and_db(), &["db", "web"]).await;

        let answer = daemon
            .ask(
                rpc::method::SERVICE_START,
                serde_json::json!({"service": "mailpit"}),
            )
            .await;

        assert_eq!(answer["error"]["data"]["code"], "not_found");
        assert!(
            answer["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("mailpit")),
            "{answer}"
        );
    }

    /// **The code is the failure's own, not one this handler picked** — R8.
    ///
    /// It used to be `internal` for every source failure whatever it was, and the fixture is why:
    /// `SpecSource` returned `anyhow::Error`, the wire mapping downcast to find a
    /// `mixengine_core::Error` behind it, and a fixture that raised a bare string had none to find.
    /// The running daemon never took that path — its source renders through `mixengine-core` and so
    /// always carried a code — so what this test was pinning down was the hole and not the product.
    /// With the trait typed, a package that is not installed answers `not_found` and points at
    /// `mix package list`, which is the sentence `error.rs` has argued for since T30: a user who
    /// misspelled a setting must not be sent to file a bug report.
    #[tokio::test]
    async fn a_source_that_cannot_answer_is_reported_with_the_failure_s_own_code() {
        let daemon = daemon(Arc::new(fixture::Unavailable), &[]).await;

        let answer = daemon.ask(rpc::method::SERVICE_LIST, Value::Null).await;

        assert_eq!(answer["error"]["data"]["code"], "not_found");
        assert!(
            answer["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("no such package")),
            "the source's own complaint survives the trip: {answer}"
        );
    }

    #[tokio::test]
    async fn a_target_says_nothing_and_means_every_declared_service() {
        // The three spellings of "no parameters" that `no_params` accepts, on a method that does
        // take them: a client sending any of them is asking about everything.
        for params in [Value::Null, serde_json::json!({}), serde_json::json!([])] {
            let daemon = undeclared().await;
            let walk: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, params).await;

            assert!(walk.planned.is_empty(), "nothing is declared: {walk:?}");
            assert!(walk.complete);
        }
    }
}
