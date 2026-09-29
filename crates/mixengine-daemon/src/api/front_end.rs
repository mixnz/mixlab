//! `service.set_front_end`: moving a home from one web server to the other.
//!
//! Roadmap task **T97**, designed by
//! [ADR 0026](https://github.com/mixnz/mixlab/blob/master/docs/decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md).
//! Its own file rather than more of [`super::create`], which is already two methods read forwards
//! and backwards, and beside it rather than in [`super::rpc`] because everything here is those two
//! methods composed: a switch is a stop, a delete, a create and a start, and its whole difficulty is
//! what happens when one of them does not work.
//!
//! # The grant is asked for before anything is touched
//!
//! **T42 writes the port-80 grant into the `security.capability` attribute of the binary**, so a home
//! that switches from Caddy to nginx is a home whose new front end has no grant. macOS redirects by
//! port and carries over; Windows reserves nothing. The outcome ADR 0026 forbids is *a home whose
//! sites are rendered for a server that cannot answer* — so the question is asked while there is
//! still nothing to undo, and a machine that will not grant leaves the home **where it was**.
//!
//! **The rule is *do not make it worse*, not *require a grant*.** A Linux home where nobody ever
//! granted has a front end that cannot bind 80 and therefore does not start; refusing to move it
//! would trap somebody on a server that cannot answer in order to protect them from a server that
//! cannot answer. So the old binary is probed too, and only a switch that would lose a grant the
//! home already had is refused.
//!
//! # Delete before create, and put the old row back
//!
//! The order is ADR 0026's: creating first would make `front_end::held_by`'s "exactly one" briefly
//! false and would need an exception carved into the refusal for its own caller. The cost is a
//! window with no front end, and the step inside it can fail — a front end renders through its own
//! checker (`nginx -t`, `caddy validate`), so a configuration the new program refuses is caught
//! *there*. That is the good news: it is the last step that is still undoable, and the old row's
//! [`Declaration`] was read before it was deleted precisely so that it can be.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mixengine_core::services::{self, Declaration};
use mixengine_proto::privileged::PrivilegedOp;
use mixengine_proto::{
    Error, ErrorCode, FrontEndOutcome, FrontEndReport, FrontEndSwitch, JobFilter, JobKind,
    JobState, JobSummary, PackageVersion, ResourceLimits, ServiceCreate, ServiceFailure, ServiceId,
    ServiceState, ServiceWalk, rpc,
};

use super::Api;
use super::target;
use crate::error::ToWire as _;

/// How many running jobs the conflict check looks at. [`super::rpc`]'s constant, for its reason.
const JOBS_LOOKED_AT: u32 = 5;

/// The front end this home is on, as it was before anything moved.
///
/// **Read whole, and read early.** Every field here is either something the new row inherits or
/// something the rollback needs, and both are gone the moment the row is deleted.
#[derive(Debug)]
struct Standing {
    /// Which service it is.
    id: ServiceId,

    /// Everything its row said, in the shape [`services::create`] takes — the rollback material.
    declaration: Declaration,

    /// Was it up? A switch starts the new front end only if the old one was running.
    running: bool,

    /// What did not travel with the switch, one sentence each.
    not_carried: Vec<String>,
}

impl Api {
    /// `service.set_front_end` — one job, and the three refusals that happen before it exists.
    ///
    /// **Nothing is checked twice for the sake of it.** The package has to be installed or there is
    /// no binary to grant and no version to create with; a second switch already running would race
    /// this one through the same rows; and a site that declares the front end refuses the *delete*
    /// this walk is about to make, which is a refusal far better delivered before the front end has
    /// been stopped. The delete makes it again from inside, which is where it cannot be forgotten.
    ///
    /// # Errors
    ///
    /// `precondition_failed` when no version of that package is installed, when a named version is
    /// not, and when a site declares the front end this would replace; `conflict` when another
    /// switch is running; and the wire error of a home whose rows could not be read.
    pub(crate) async fn service_set_front_end(
        self: &Arc<Self>,
        switch: FrontEndSwitch,
    ) -> Result<JobSummary, Error> {
        let package = switch.server.package();
        let install = self.installed(package, switch.version.as_ref()).await?;

        self.no_other_switch().await?;
        self.no_site_declares_the_front_end().await?;

        let api = Arc::clone(self);

        self.jobs
            .begin(
                &JobKind::parse(rpc::method::SERVICE_SET_FRONT_END).expect("a valid kind"),
                move |handle| async move {
                    let report = api.switch_to(&switch, &install, &handle).await?;

                    serde_json::to_value(report).map_err(|error| {
                        Error::new(
                            ErrorCode::Internal,
                            format!("a front-end report could not be encoded: {error}"),
                        )
                    })
                },
            )
            .await
    }

    /// The version to create with and the binary a grant would be written against.
    ///
    /// **The newest installed unless one is named** — nobody choosing a web server is choosing a
    /// patch release. A switch installs nothing, so a package with no installed version is refused
    /// with the command that would install one.
    async fn installed(
        &self,
        package: &str,
        wanted: Option<&PackageVersion>,
    ) -> Result<Install, Error> {
        let records = mixengine_core::packages::records(&self.store, Some(package))
            .await
            .map_err(|error| error.to_wire())?;

        let chosen = match wanted {
            Some(version) => records.into_iter().find(|one| &one.version == version),
            None => records
                .into_iter()
                .max_by(|left, right| left.version.cmp_precedence(&right.version)),
        };

        let Some(chosen) = chosen else {
            let named = wanted.map_or_else(String::new, |version| format!(" {version}"));

            return Err(Error::new(
                ErrorCode::PreconditionFailed,
                format!("{package}{named} is not installed, and a switch installs nothing"),
            )
            .with_hint(format!(
                "`mix package list-available {package}` names the versions, and `mix package \
                 install {package} <version>` puts one here"
            )));
        };

        Ok(Install {
            // The one spelling of that join, shared with the recipe that will run it — see
            // `mixengine_core::generate::program`.
            binary: mixengine_core::generate::program(Path::new(&chosen.path), package),
            version: chosen.version,
        })
    }

    /// Refuse a second switch, naming the one that is running.
    ///
    /// **A lock would make the second caller wait instead**, which is worse: a switch stops a web
    /// server, and somebody whose command is silently queued behind another one has no way to tell
    /// that from a command that is slow. This is `elevation.grant`'s one-slot refusal, at a
    /// different scale.
    async fn no_other_switch(&self) -> Result<(), Error> {
        let running = self
            .jobs
            .list(&JobFilter {
                state: Some(JobState::Running),
                limit: JOBS_LOOKED_AT,
            })
            .await?;

        let Some(other) = running
            .into_iter()
            .find(|job| job.kind.as_str() == rpc::method::SERVICE_SET_FRONT_END)
        else {
            return Ok(());
        };

        Err(Error::new(
            ErrorCode::Conflict,
            format!("{} is already changing this home's front end", other.id),
        )
        .with_hint(format!(
            "`mix job status {}` says how far it has got; there is one front end, so there is one \
             switch at a time",
            other.id
        )))
    }

    /// The delete's own refusal, made before the front end has been stopped.
    ///
    /// **A switch does not force.** `service.delete --force` exists because somebody who has been
    /// shown the sites is entitled to overrule the declaration; a switch has shown nobody anything,
    /// so a site with an explicit link to the front end is a refusal and not a warning.
    async fn no_site_declares_the_front_end(&self) -> Result<(), Error> {
        let Some(id) = self.front_end_row().await? else {
            return Ok(());
        };

        let declared = mixengine_core::sites::declaring(&self.store, &id)
            .await
            .map_err(|error| error.to_wire())?;

        if declared.is_empty() {
            return Ok(());
        }

        Err(Error::new(
            ErrorCode::PreconditionFailed,
            format!("{id} is declared by {}", declared.join(", ")),
        )
        .with_hint(format!(
            "`mix site update <site> --service …` drops it; a switch deletes {id}, and it will not \
             overrule a declaration somebody made on purpose"
        )))
    }

    /// The service this home's front end is, by what its package is *for*.
    async fn front_end_row(&self) -> Result<Option<ServiceId>, Error> {
        let held = services::front_end::held_by(&self.store, &crate::services::catalogue())
            .await
            .map_err(|error| error.to_wire())?;

        // An id a `services` row holds that this build cannot parse belongs to no recipe here, so it
        // is not a front end — which is the answer `held_by` itself gives such a row.
        Ok(held.and_then(|id| ServiceId::parse(id).ok()))
    }

    /// The walk: stop, swap the row, and start — with the grant asked for before any of it.
    async fn switch_to(
        &self,
        switch: &FrontEndSwitch,
        install: &Install,
        handle: &crate::jobs::JobHandle,
    ) -> Result<FrontEndReport, Error> {
        let catalogue = crate::services::catalogue();
        let _guard = self.front_end.lock().await;

        handle
            .progress(5, "reading what this home is reached through")
            .await;

        let package = switch.server.package();
        let wanted = ServiceId::parse(package).map_err(|error| {
            Error::new(
                ErrorCode::Internal,
                format!("{package} is not a service id: {error}"),
            )
        })?;

        let was = self.front_end_row().await?;

        // **Whatever version is named.** This method switches which *program* a home is reached
        // through; moving between versions of one program is a different operation with different
        // consequences, and pretending one method did both would let a mistyped patch number delete
        // and re-create somebody's front end.
        if was.as_ref().is_some_and(|id| id.name() == package) {
            return Ok(self
                .report(
                    was.clone(),
                    was,
                    FrontEndOutcome::Unchanged {},
                    None,
                    Vec::new(),
                )
                .await);
        }

        handle.progress(15, "reading the front end it is on").await;
        let standing = match &was {
            Some(id) => Some(self.standing(id).await?),
            None => None,
        };

        handle
            .progress(
                25,
                "asking whether this machine will let it answer on 80 and 443",
            )
            .await;

        if let Some(because) = self
            .would_lose_the_grant(switch, install, &was, handle)
            .await
        {
            return Ok(self
                .report(
                    was.clone(),
                    was,
                    FrontEndOutcome::NotGranted { because },
                    None,
                    Vec::new(),
                )
                .await);
        }

        handle.progress(40, "stopping the front end it is on").await;
        if let Some(standing) = &standing
            && standing.running
        {
            let walk = self.service_stop(&target(&standing.id)).await?;

            if let Some(failed) = walk.failed {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    format!(
                        "{} could not be stopped, so nothing was changed",
                        failed.service
                    ),
                )
                .with_hint(
                    "a front end that will not stop is one this switch would have to delete out \
                     from under a live process",
                ));
            }
        }

        handle
            .progress(55, "taking its row and its configuration")
            .await;
        let kept_data = match &standing {
            Some(standing) => match self.delete_declared(&catalogue, &standing.id, false).await {
                Ok(removal) => removal.data_kept,
                Err(error) => {
                    // Nothing has been deleted, so the home is as it was but for a service this walk
                    // stopped. Putting that back is the whole of the repair.
                    self.start_again(standing).await;

                    return Err(error);
                }
            },
            None => None,
        };

        handle
            .progress(70, "rendering this home for its new front end")
            .await;
        let created = self
            .create_declared(
                &catalogue,
                &ServiceCreate {
                    id: wanted.clone(),
                    version: install.version.clone(),
                    // A front end's ports are its own settings and never a column — the recipes name
                    // no preferred port, so there is nothing to carry and nothing to allocate.
                    port: None,
                    // The two things that are about *the home's front end* rather than about the
                    // program that was doing the job.
                    bind_addr: standing
                        .as_ref()
                        .and_then(|standing| standing.declaration.bind_addr.clone()),
                    data_dir: None,
                    autostart: standing
                        .as_ref()
                        .map(|standing| standing.declaration.autostart),
                    overrides: None,
                },
            )
            .await;

        if let Err(error) = created {
            return Ok(self.put_back(standing, was, error).await);
        }

        handle.progress(85, "starting it").await;
        let started = match standing.as_ref().is_some_and(|standing| standing.running) {
            true => Some(self.start_now(&wanted).await),
            false => None,
        };

        Ok(self
            .report(
                was,
                Some(wanted),
                FrontEndOutcome::Switched { started },
                kept_data,
                standing
                    .map(|standing| standing.not_carried)
                    .unwrap_or_default(),
            )
            .await)
    }

    /// A report, with `answering` measured rather than assumed.
    ///
    /// **One place reads the machine**, which is what keeps the member's promise true: it is about
    /// the front end named by `now`, whatever the outcome was and whichever branch built it.
    async fn report(
        &self,
        was: Option<ServiceId>,
        now: Option<ServiceId>,
        outcome: FrontEndOutcome,
        kept_data: Option<String>,
        not_carried: Vec<String>,
    ) -> FrontEndReport {
        FrontEndReport {
            answering: self.front_end_may_answer().await,
            was,
            now,
            outcome,
            kept_data,
            not_carried,
        }
    }

    /// The front end as it stands, and what about it will not survive the switch.
    async fn standing(&self, id: &ServiceId) -> Result<Standing, Error> {
        let declaration = services::declaration(&self.store, id)
            .await
            .map_err(|error| error.to_wire())?;
        let record = services::record(&self.store, id)
            .await
            .map_err(|error| error.to_wire())?;

        let running = self.services.supervised().contains(id)
            || matches!(
                record.state,
                ServiceState::Running
                    | ServiceState::Starting
                    | ServiceState::Restarting
                    | ServiceState::Degraded
            );

        Ok(Standing {
            not_carried: self.what_will_not_travel(id, &declaration).await,
            id: id.clone(),
            declaration,
            running,
        })
    }

    /// Everything the old row held that belonged to the *program* rather than to the job it did.
    ///
    /// **Named rather than silently dropped.** An override written in one server's configuration
    /// language is a syntax error in the other's, a ceiling was measured against the program that is
    /// going, and a data directory is that program's files — none of them can travel. What a person
    /// can do about that is put them back, and they can only do that if they are told.
    async fn what_will_not_travel(&self, id: &ServiceId, declaration: &Declaration) -> Vec<String> {
        let mut left = Vec::new();

        if !is_empty_document(&declaration.overrides) {
            left.push(format!(
                "the settings {id} was overriding, which are written in its own configuration \
                 language"
            ));
        }

        if let Some(data) = &declaration.data_dir {
            left.push(format!("the data directory {id} was pointed at, {data}"));
        }

        if self
            .spec_limits(id)
            .await
            .is_some_and(|limits| limits != ResourceLimits::default())
        {
            left.push(format!("the limits set on {id}"));
        }

        if matches!(
            services::idle_minutes(&self.store, id).await,
            Ok(Some(minutes)) if minutes > 0
        ) {
            left.push(format!("the idle policy set on {id}"));
        }

        left
    }

    /// What a service is capped at, as the rendering holds it, or [`None`] for every failure alike.
    ///
    /// The caller is building a sentence for a person; a limit that could not be read is one that
    /// goes unmentioned rather than one that fails a switch.
    async fn spec_limits(&self, id: &ServiceId) -> Option<ResourceLimits> {
        let graph = self.services.graph().await.ok()?;

        Some(graph.spec(id)?.limits())
    }

    /// Ask this machine for the new front end, and answer with the reason to stay where we are.
    ///
    /// [`None`] means go: the grant is there, this system grants nothing at all, or the front end
    /// the home is on cannot answer either — in which case the switch takes nothing away.
    async fn would_lose_the_grant(
        &self,
        switch: &FrontEndSwitch,
        install: &Install,
        was: &Option<ServiceId>,
        handle: &crate::jobs::JobHandle,
    ) -> Option<String> {
        if self.may_answer(&install.binary) {
            return None;
        }

        // Derived from the method rather than from a `#[cfg]`, which is why `PortAccessState`
        // carries one. `None` is a system that grants nothing — Windows — and is not a refusal.
        let plan = self
            .elevation
            .host()
            .port_access()
            .probe(&install.binary, &crate::elevation::Elevation::ANSWERING)
            .ok()?
            .plan(&install.binary)?;

        if let Err(error) = self
            .elevation
            .enqueue(&PrivilegedOp::PortAccessGrant { plan })
            .await
        {
            tracing::warn!(%error, "a front end's port grant could not be put in the queue");
        }

        // **Only when asked.** What is about to be allowed is read before it is allowed (T64), so a
        // caller that did not ask for the prompt gets the operation left waiting and the reason —
        // and `mix elevation grant` followed by the same command works.
        // **Inside this job and not as one of its own**, which is what `grant_within` exists for:
        // whether the switch may go ahead depends on what the machine says *after* the prompt has
        // been answered, and there is no hook between one job ending and another beginning.
        if switch.grant
            && let Err(error) = self.elevation.grant_within(handle).await
        {
            tracing::info!(code = ?error.code, "the prompt a switch raised did not run");
        }

        if self.may_answer(&install.binary) {
            return None;
        }

        // The rule is *do not make it worse*. A home whose front end cannot answer either has
        // nothing to lose, and refusing would trap it there.
        if was.is_some() && !self.front_end_may_answer().await {
            return None;
        }

        Some(format!(
            "this machine has not been asked to let {} answer on 80 and 443, so the front end it \
             is on was kept",
            install.binary.display()
        ))
    }

    /// May this binary bind 80 and 443?
    ///
    /// **A probe that cannot be read is not a refusal.** `require_port_access` treats one as *ask
    /// for nothing*, and treating it here as *this machine says no* would refuse a switch on a
    /// machine that said nothing at all.
    fn may_answer(&self, binary: &Path) -> bool {
        match self
            .elevation
            .host()
            .port_access()
            .probe(binary, &crate::elevation::Elevation::ANSWERING)
        {
            Ok(state) => state.granted,
            Err(_) => true,
        }
    }

    /// The same question about the front end this home is on **now**.
    ///
    /// `false` for a home with no front end at all: nothing is answering, which is what a client
    /// renders and is also what makes the "would this make it worse" comparison below correct — a
    /// home with nothing to lose loses nothing.
    async fn front_end_may_answer(&self) -> bool {
        match self.services.front_end_program().await {
            Some(binary) => self.may_answer(&binary),
            None => false,
        }
    }

    /// Put the old row back after a create that would not render.
    async fn put_back(
        &self,
        standing: Option<Standing>,
        was: Option<ServiceId>,
        because: Error,
    ) -> FrontEndReport {
        let Some(standing) = standing else {
            // There was no front end to put back: the home had none, and it has none now.
            return self
                .report(
                    was,
                    None,
                    FrontEndOutcome::Failed {
                        because: because.to_string(),
                    },
                    None,
                    Vec::new(),
                )
                .await;
        };

        match self.restore(&standing).await {
            Ok(()) => {
                if standing.running {
                    self.start_again(&standing).await;
                }

                self.report(
                    was,
                    Some(standing.id.clone()),
                    FrontEndOutcome::RolledBack {
                        because: because.to_string(),
                    },
                    None,
                    Vec::new(),
                )
                .await
            }

            Err(failed) => {
                tracing::error!(
                    front_end = standing.id.as_str(),
                    %failed,
                    "the front end this switch replaced could not be put back; this home has none"
                );

                self.report(
                    was,
                    self.front_end_row().await.ok().flatten(),
                    FrontEndOutcome::Failed {
                        because: format!(
                            "{because}, and it could not be put back either: {failed}"
                        ),
                    },
                    None,
                    Vec::new(),
                )
                .await
            }
        }
    }

    /// Write the old row again, exactly, and render this home for it.
    ///
    /// **The row first and the rendering second**, which is `service.create`'s own order: the
    /// generator renders from what is declared, so a row that is not there yet renders nothing.
    async fn restore(&self, standing: &Standing) -> Result<(), Error> {
        services::create(&self.store, self.services.host(), &standing.declaration)
            .await
            .map_err(|error| error.to_wire())?;

        self.services
            .reconfigure()
            .await
            .map_err(|error| error.to_wire())
    }

    /// Start a service, and describe a start that could not even be planned as the failure it is.
    async fn start_now(&self, id: &ServiceId) -> ServiceWalk {
        match self.service_start(&target(id)).await {
            Ok(walk) => walk,
            Err(error) => {
                tracing::warn!(service = id.as_str(), %error, "the new front end did not start");

                ServiceWalk {
                    planned: vec![id.clone()],
                    complete: true,
                    reached: Vec::new(),
                    // No reason: this is the daemon's own failure and not a transition anything
                    // watched, which is exactly the case `ServiceFailure::reason` documents `None`
                    // for.
                    failed: Some(ServiceFailure {
                        service: id.clone(),
                        reason: None,
                    }),
                    blocked: Vec::new(),
                }
            }
        }
    }

    /// Put a service this walk stopped back where it found it. Best effort, and logged loudly.
    async fn start_again(&self, standing: &Standing) {
        if let Err(error) = self.service_start(&target(&standing.id)).await {
            tracing::error!(
                front_end = standing.id.as_str(),
                %error,
                "the front end this switch stopped could not be started again"
            );
        }
    }
}

/// The version a switch creates with, and the file a grant would be written against.
#[derive(Debug)]
struct Install {
    /// The program itself, spelled the way this OS spells one.
    binary: PathBuf,

    /// Which installed version it belongs to.
    version: PackageVersion,
}

/// Is this overrides column the empty document every uncustomised service has?
///
/// A string comparison against `{}` would be right for every row this build writes and wrong for one
/// written with whitespace in it, which is why it is parsed.
pub(super) fn is_empty_document(overrides: &str) -> bool {
    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(overrides)
        .is_ok_and(|document| document.is_empty())
}
