//! `runtime.upgrade_plan` and `runtime.upgrade` — roadmap task **T193b**, the design's D5.

use std::sync::Arc;

use mixengine_core::extensions::manifest::Body;
use mixengine_core::upgrade::{self, ManifestPin, Moved, PinRewrite};
use mixengine_proto::{
    Error, ErrorCode, ExtensionId, JobSummary, OldVersion, PackageVersion, RuntimeKind,
    RuntimeTarget, RuntimeUninstall, RuntimeUpgrade, RuntimeUpgradeQuery, ServiceId, SiteKind,
    UpgradeEntry, UpgradeItem, UpgradeOutcome, UpgradePlan, VersionConstraint, rpc,
};

use super::{Resolved, Standing, resolve, settle};
use crate::api::Api;
use crate::error::ToWire as _;

/// A `web-app` pool on `from`, and whether it moves.
#[derive(Debug, Clone)]
struct ExtensionPool {
    pool: ServiceId,
    moves: bool,
    requires: Option<VersionConstraint>,
}

/// Everything about this home an update of `from` reads, once, for the plan and for the walk.
#[derive(Debug, Clone)]
struct Survey {
    /// For PHP, the shared pools `(from, to)`.
    pools: Option<(ServiceId, ServiceId)>,
    /// Each PHP site on `from`'s pool, by primary domain.
    sites: Vec<String>,
    extension_pools: Vec<ExtensionPool>,
    dropped: Vec<String>,
    pins: Vec<PinRewrite>,
    default: bool,
    manifests: Vec<ManifestPin>,
    tools: Vec<String>,
}

impl Survey {
    /// What keeps `from` installed whatever is asked — D5 step 9.
    fn reasons_to_keep(&self, kind: RuntimeKind, from: &PackageVersion) -> Vec<String> {
        let mut because: Vec<String> = self
            .manifests
            .iter()
            .map(|pin| {
                format!(
                    "{} pins {kind} {} in {}",
                    pin.project, pin.constraint, pin.path
                )
            })
            .collect();
        because.extend(
            self.tools
                .iter()
                .map(|tool| format!("{tool} is installed only in {kind} {from}")),
        );
        because.extend(
            self.extension_pools
                .iter()
                .filter(|pool| !pool.moves)
                .map(|pool| match &pool.requires {
                    Some(requires) => format!("{} requires {kind} {requires}", pool.pool),
                    None => format!("{} stays on {kind} {from}", pool.pool),
                }),
        );
        because
    }
}

/// What one runtime upgrade has gathered so far, read by the steps after the one that set it.
struct Upgrade<'a> {
    asked: &'a RuntimeUpgrade,
    kind: RuntimeKind,
    from: &'a PackageVersion,
    resolved: Resolved,

    /// Set by `survey_for`.
    survey: Option<Survey>,

    /// Set by `survey_for`; settled as the walk goes.
    plan: Option<UpgradePlan>,

    /// How `from`'s pool stood before the update, set by `carry_pool` for a runtime with pools.
    was: Option<Standing>,

    /// The extension pools that move with the runtime, set by `move_to_new`.
    moving: Vec<ServiceId>,

    /// What the move changed, set by `move_to_new`, so a refusal can put it back.
    moved: Option<Moved>,
}

impl<'a> Upgrade<'a> {
    fn new(asked: &'a RuntimeUpgrade, resolved: Resolved) -> Self {
        Self {
            asked,
            kind: asked.query.kind,
            from: &asked.query.from,
            resolved,
            survey: None,
            plan: None,
            was: None,
            moving: Vec::new(),
            moved: None,
        }
    }

    fn survey(&self) -> &Survey {
        self.survey.as_ref().expect("survey_for sets the survey")
    }

    fn plan_mut(&mut self) -> &mut UpgradePlan {
        self.plan.as_mut().expect("survey_for sets the plan")
    }
}

/// Where a runtime upgrade is — D5's numbered steps.
enum UpgradeStep {
    Install,
    Survey,
    CarryPool,
    Move,
    Render,

    /// The front end refused the sites on `to`; this is what it said.
    MoveBack(Error),

    RestartExtensionPools,
    StopOld,
    RemoveOld,
    Done,
}

/// The extension pools on `from` that move with it.
#[warn(clippy::cognitive_complexity)]
fn moving_pools(survey: &Survey) -> Vec<ServiceId> {
    survey
        .extension_pools
        .iter()
        .filter(|pool| pool.moves)
        .map(|pool| pool.pool.clone())
        .collect()
}

/// `from` kept without trying to uninstall it: because it was asked to be, or because something
/// still needs it. `None` means nothing keeps it, and the uninstall is tried.
#[warn(clippy::cognitive_complexity)]
fn kept_without_trying(keep: bool, because: Vec<String>) -> Option<OldVersion> {
    if keep {
        return Some(OldVersion::Kept {
            because: vec!["you asked to keep it".to_owned()],
        });
    }

    (!because.is_empty()).then_some(OldVersion::Kept { because })
}

/// A core error as the wire error a job ends with.
fn wire(error: mixengine_core::Error) -> Error {
    error.to_wire()
}

/// The plan a survey describes, every entry `planned`.
fn plan_of(
    kind: RuntimeKind,
    from: &PackageVersion,
    resolved: &Resolved,
    survey: &Survey,
) -> UpgradePlan {
    let planned = |item| UpgradeEntry {
        item,
        outcome: UpgradeOutcome::Planned {},
    };
    let mut entries = Vec::new();

    if survey.default {
        entries.push(planned(UpgradeItem::Default {}));
    }
    if let Some((from_pool, to_pool)) = &survey.pools {
        for site in &survey.sites {
            entries.push(planned(UpgradeItem::Site {
                site: site.clone(),
                from: from_pool.clone(),
                to: to_pool.clone(),
            }));
        }
    }
    for pool in &survey.extension_pools {
        entries.push(planned(UpgradeItem::ExtensionPool {
            pool: pool.pool.clone(),
            moves: pool.moves,
            requires: pool.requires.clone(),
        }));
    }
    for name in &survey.dropped {
        entries.push(planned(UpgradeItem::DroppedExtension {
            name: name.clone(),
        }));
    }
    for pin in &survey.pins {
        entries.push(planned(UpgradeItem::Pin {
            project: pin.project.clone(),
            from: pin.from.clone(),
            to: VersionConstraint::from(resolved.to.clone()),
        }));
    }
    for pin in &survey.manifests {
        entries.push(planned(UpgradeItem::Manifest {
            project: pin.project.clone(),
            path: pin.path.clone(),
            constraint: pin.constraint.clone(),
        }));
    }
    for tool in &survey.tools {
        entries.push(planned(UpgradeItem::Tool { name: tool.clone() }));
    }

    let because = survey.reasons_to_keep(kind, from);

    UpgradePlan {
        subject: kind.as_str().to_owned(),
        from: from.clone(),
        to: resolved.to.clone(),
        to_installed: resolved.to_installed,
        bytes: resolved.bytes,
        stale: resolved.stale,
        needs: resolved.needs.clone(),
        entries,
        old: match because.is_empty() {
            true => OldVersion::WillBeRemoved {},
            false => OldVersion::WillBeKept { because },
        },
    }
}

/// A pool id, which a version always makes.
fn pool_id(version: &PackageVersion) -> Result<ServiceId, Error> {
    ServiceId::parse(format!("php-fpm@{version}"))
        .map_err(|error| Error::new(ErrorCode::Internal, format!("a pool id: {error}")))
}

impl Api {
    /// The installed versions of `kind`.
    async fn installed_runtimes(&self, kind: RuntimeKind) -> Result<Vec<PackageVersion>, Error> {
        Ok(mixengine_core::runtimes::records(&self.store, Some(kind))
            .await
            .map_err(|error| error.to_wire())?
            .into_iter()
            .map(|row| row.version)
            .collect())
    }

    /// Read everything an update of `from` touches.
    async fn survey(
        &self,
        kind: RuntimeKind,
        from: &PackageVersion,
        resolved: &Resolved,
    ) -> Result<Survey, Error> {
        let wire = |error: mixengine_core::Error| error.to_wire();
        let to = &resolved.to;

        let pools = match kind {
            RuntimeKind::Php => Some((pool_id(from)?, pool_id(to)?)),
            _ => None,
        };

        let mut sites = Vec::new();
        let mut extension_pools = Vec::new();
        let mut dropped = Vec::new();

        if let Some((from_pool, _)) = &pools {
            for site in mixengine_core::sites::records(&self.store, None)
                .await
                .map_err(wire)?
            {
                if matches!(&site.kind, SiteKind::PhpFpm { pool: Some(pool) } if pool == from_pool)
                {
                    sites.push(
                        site.domains
                            .first()
                            .cloned()
                            .unwrap_or(site.doc_root.clone()),
                    );
                }
            }

            for pool in mixengine_core::services::pools::of_runtime(&self.store, kind, from)
                .await
                .map_err(wire)?
                .into_iter()
                .filter(|pool| pool != from_pool)
            {
                let requires = match pool.instance().map(ExtensionId::parse) {
                    Some(Ok(extension)) => {
                        mixengine_core::extensions::store::get(&self.store, &extension)
                            .await
                            .map_err(wire)?
                            .and_then(|installed| match installed.manifest.body {
                                Body::WebApp(app) => Some(app.runtime.requires),
                                _ => None,
                            })
                    }
                    _ => None,
                };

                extension_pools.push(ExtensionPool {
                    moves: requires
                        .as_ref()
                        .is_some_and(|requires| requires.matches(to)),
                    requires,
                    pool,
                });
            }

            let choices = mixengine_core::runtimes::extensions::state(&self.store, kind, from)
                .await
                .map_err(wire)?
                .choices;
            let offered = match resolved.to_installed {
                true => Some(
                    mixengine_core::runtimes::extensions::state(&self.store, kind, to)
                        .await
                        .map_err(wire)?
                        .offered,
                ),
                false => resolved.extensions.clone(),
            };
            if let Some(offered) = offered {
                dropped = upgrade::carry_choices(&choices, &offered).1;
            }
        }

        let default = mixengine_core::runtimes::records(&self.store, Some(kind))
            .await
            .map_err(wire)?
            .into_iter()
            .any(|row| &row.version == from && row.default);

        Ok(Survey {
            pools,
            sites,
            extension_pools,
            dropped,
            pins: upgrade::pins_to_rewrite(&self.store, kind, from, to)
                .await
                .map_err(wire)?,
            default,
            manifests: upgrade::manifest_pins_needing(&self.store, kind, from, to)
                .await
                .map_err(wire)?,
            tools: upgrade::tools_only_in(&self.store, kind, from, to)
                .await
                .map_err(wire)?,
        })
    }

    /// `runtime.upgrade_plan` — what an update would do, changing nothing.
    ///
    /// # Errors
    ///
    /// What [`resolve`] refuses, and the wire error of a table that could not be read.
    pub(crate) async fn runtime_upgrade_plan(
        &self,
        query: &RuntimeUpgradeQuery,
    ) -> Result<UpgradePlan, Error> {
        let installed = self.installed_runtimes(query.kind).await?;
        let resolved = resolve(
            self.runtimes.fetcher(),
            query.kind.as_str(),
            &installed,
            &query.from,
            query.to.as_ref(),
        )
        .await?;
        let survey = self.survey(query.kind, &query.from, &resolved).await?;

        Ok(plan_of(query.kind, &query.from, &resolved, &survey))
    }

    /// `runtime.upgrade` — one job, and the refusals that come before it.
    ///
    /// # Errors
    ///
    /// What [`resolve`] refuses; `dependency_missing` when this machine lacks what `to` needs and
    /// nothing was agreed; `conflict` when `to` is being installed or either version is being
    /// updated by another job.
    pub(crate) async fn runtime_upgrade(
        self: &Arc<Self>,
        asked: RuntimeUpgrade,
    ) -> Result<JobSummary, Error> {
        let kind = asked.query.kind;
        let installed = self.installed_runtimes(kind).await?;
        let resolved = resolve(
            self.runtimes.fetcher(),
            kind.as_str(),
            &installed,
            &asked.query.from,
            asked.query.to.as_ref(),
        )
        .await?;

        if let Some(job) = self.runtimes.installing(kind, &resolved.to).await {
            return Err(Error::new(
                ErrorCode::Conflict,
                format!("job {job} is installing {kind} {}", resolved.to),
            ));
        }

        let target = RuntimeTarget {
            kind,
            version: resolved.to.clone(),
        };
        if !resolved.to_installed && !asked.ignore_requirements {
            crate::requirements::gate(
                self.runtimes.fetcher(),
                kind.as_str(),
                resolved.to.as_str(),
                &crate::runtimes::subject_of(&target),
                asked.install_prerequisites,
            )
            .await?;
        }

        let api = Arc::clone(self);
        let (from, to) = (asked.query.from.clone(), resolved.to.clone());
        self.begin_upgrade(
            rpc::method::RUNTIME_UPGRADE,
            kind.as_str(),
            &from,
            &to,
            move |handle| async move { api.runtime_walk(&asked, resolved, &handle).await },
        )
        .await
    }

    /// The walk — D5, steps 1 to 9, in order. **The old version is untouched until the new one is
    /// proven**: nothing points at `to` before its pool has started, and `from` goes last.
    #[warn(clippy::cognitive_complexity)]
    async fn runtime_walk(
        &self,
        asked: &RuntimeUpgrade,
        resolved: Resolved,
        handle: &crate::jobs::JobHandle,
    ) -> Result<UpgradePlan, Error> {
        let mut upgrade = Upgrade::new(asked, resolved);
        let mut step = UpgradeStep::Install;

        loop {
            step = match step {
                UpgradeStep::Install => self.install_to(&upgrade, handle).await?,
                UpgradeStep::Survey => self.survey_for(&mut upgrade).await?,
                UpgradeStep::CarryPool => self.carry_pool(&mut upgrade, handle).await?,
                UpgradeStep::Move => self.move_to_new(&mut upgrade, handle).await?,
                UpgradeStep::Render => self.render_again(handle).await,
                UpgradeStep::MoveBack(refused) => {
                    return Err(self.move_back(&upgrade, refused).await);
                }
                UpgradeStep::RestartExtensionPools => {
                    self.restart_extension_pools(&upgrade).await?
                }
                UpgradeStep::StopOld => self.stop_old(&mut upgrade, handle).await?,
                UpgradeStep::RemoveOld => self.remove_old(&mut upgrade, handle).await,
                UpgradeStep::Done => {
                    return Ok(upgrade.plan.take().expect("survey_for sets the plan"));
                }
            };
        }
    }

    /// 1. Install `to`.
    #[warn(clippy::cognitive_complexity)]
    async fn install_to(
        &self,
        upgrade: &Upgrade<'_>,
        handle: &crate::jobs::JobHandle,
    ) -> Result<UpgradeStep, Error> {
        if upgrade.resolved.to_installed {
            return Ok(UpgradeStep::Survey);
        }

        let (kind, to) = (upgrade.kind, &upgrade.resolved.to);
        let target = RuntimeTarget {
            kind,
            version: to.clone(),
        };
        if upgrade.asked.install_prerequisites && !upgrade.asked.ignore_requirements {
            crate::requirements::prepare(
                self.runtimes.fetcher(),
                kind.as_str(),
                to.as_str(),
                &crate::runtimes::subject_of(&target),
                handle,
            )
            .await?;
        }
        handle.progress(5, &format!("installing {kind} {to}")).await;
        self.runtimes.perform(&target, handle).await?;

        Ok(UpgradeStep::Survey)
    }

    /// Read what the update moves, and the plan it reports.
    #[warn(clippy::cognitive_complexity)]
    async fn survey_for(&self, upgrade: &mut Upgrade<'_>) -> Result<UpgradeStep, Error> {
        // Read after the install, so `to` is a row every query below can see.
        let survey = self
            .survey(
                upgrade.kind,
                upgrade.from,
                &Resolved {
                    to_installed: true,
                    ..upgrade.resolved.clone()
                },
            )
            .await?;
        upgrade.plan = Some(plan_of(
            upgrade.kind,
            upgrade.from,
            &upgrade.resolved,
            &survey,
        ));

        let next = match survey.pools {
            Some(_) => UpgradeStep::CarryPool,
            None => UpgradeStep::Move,
        };
        upgrade.survey = Some(survey);

        Ok(next)
    }

    /// 2. Extension choices; 3. the pool's settings; 4. the new pool in the old one's state.
    #[warn(clippy::cognitive_complexity)]
    async fn carry_pool(
        &self,
        upgrade: &mut Upgrade<'_>,
        handle: &crate::jobs::JobHandle,
    ) -> Result<UpgradeStep, Error> {
        let (kind, from, to) = (upgrade.kind, upgrade.from, &upgrade.resolved.to);
        let (from_pool, to_pool) = upgrade
            .survey()
            .pools
            .clone()
            .expect("survey_for goes here only for a runtime with pools");

        handle
            .progress(40, "carrying the extension choices and the pool's settings")
            .await;
        upgrade::carry_extension_choices(&self.store, &self.paths, kind, from, to)
            .await
            .map_err(wire)?;
        upgrade::copy_pool_settings(&self.store, &from_pool, &to_pool)
            .await
            .map_err(wire)?;
        self.services
            .reconfigure()
            .await
            .map_err(|error| error.to_wire())?;

        let standing = self.standing_of(&from_pool).await?;
        if standing.running {
            self.start_new_pool(upgrade, &to_pool, handle).await?;
        } else if standing.person_stopped {
            upgrade::set_stopped_by_person(&self.store, &to_pool)
                .await
                .map_err(wire)?;
        }
        upgrade.was = Some(standing);

        Ok(UpgradeStep::Move)
    }

    /// Start `to`'s pool, because `from`'s was running.
    #[warn(clippy::cognitive_complexity)]
    async fn start_new_pool(
        &self,
        upgrade: &Upgrade<'_>,
        to_pool: &ServiceId,
        handle: &crate::jobs::JobHandle,
    ) -> Result<(), Error> {
        handle.progress(50, &format!("starting {to_pool}")).await;
        let walk = self.service_start(&crate::api::target(to_pool)).await?;

        // **A failed job, not a plan marked "not done"** — the final review's finding: an update
        // that moved nothing has to end the way a failure ends, or `mix` exits zero and MixLab says
        // nothing.
        if let Some(failed) = walk.failed {
            let (kind, from) = (upgrade.kind, upgrade.from);
            return Err(Error::new(
                ErrorCode::ProcessFailed,
                format!(
                    "{0} did not start, so nothing was moved and {kind} {from} is still \
                     installed; `mix service logs {0}` says why",
                    failed.service
                ),
            ));
        }

        Ok(())
    }

    /// 5–6. One transaction.
    #[warn(clippy::cognitive_complexity)]
    async fn move_to_new(
        &self,
        upgrade: &mut Upgrade<'_>,
        handle: &crate::jobs::JobHandle,
    ) -> Result<UpgradeStep, Error> {
        handle
            .progress(65, "moving the sites, the default and the pins")
            .await;
        let survey = upgrade.survey();
        let moving = moving_pools(survey);
        let moved = upgrade::move_runtime(
            &self.store,
            &upgrade::RuntimeMove {
                kind: upgrade.kind,
                from: upgrade.from.clone(),
                to: upgrade.resolved.to.clone(),
                pools: survey.pools.clone(),
                extension_pools: moving.clone(),
                pins: survey.pins.clone(),
            },
        )
        .await
        .map_err(wire)?;

        upgrade.moving = moving;
        upgrade.moved = Some(moved);

        Ok(UpgradeStep::Render)
    }

    /// 7. Render; a refusal reverses the move.
    #[warn(clippy::cognitive_complexity)]
    async fn render_again(&self, handle: &crate::jobs::JobHandle) -> UpgradeStep {
        handle.progress(75, "rendering the sites again").await;

        match self.sites.now_serves_what_it_declares().await {
            Ok(_) => UpgradeStep::RestartExtensionPools,
            Err(refused) => UpgradeStep::MoveBack(refused),
        }
    }

    /// The front end refused the sites on `to`: put them back on `from`, and say so.
    #[warn(clippy::cognitive_complexity)]
    async fn move_back(&self, upgrade: &Upgrade<'_>, refused: Error) -> Error {
        let moved = upgrade.moved.as_ref().expect("move_to_new sets what moved");
        if let Err(error) = upgrade::move_back(&self.store, moved).await {
            return wire(error);
        }
        if let Err(error) = self.sites.now_serves_what_it_declares().await {
            tracing::error!(%error, "the sites could not be rendered again after an update was reversed");
        }
        if let (Some((_, to_pool)), Some(standing)) = (&upgrade.survey().pools, upgrade.was)
            && standing.running
            && let Err(error) = self.service_stop(&crate::api::target(to_pool)).await
        {
            tracing::warn!(%error, "a pool an update started could not be stopped again");
        }

        let (kind, from, to) = (upgrade.kind, upgrade.from, &upgrade.resolved.to);
        Error::new(
            refused.code,
            format!(
                "the front end refused the sites on {kind} {to}, so they were moved back and \
                 {kind} {from} is still installed: {}",
                refused.message
            ),
        )
    }

    /// Restart every extension pool that moved and was running, so it runs on `to`.
    #[warn(clippy::cognitive_complexity)]
    async fn restart_extension_pools(&self, upgrade: &Upgrade<'_>) -> Result<UpgradeStep, Error> {
        for pool in &upgrade.moving {
            if self.standing_of(pool).await?.running
                && let Err(error) = self.service_restart(&crate::api::target(pool)).await
            {
                tracing::warn!(%pool, %error, "an extension's pool did not restart on the new PHP");
            }
        }

        Ok(UpgradeStep::StopOld)
    }

    /// 8. Stop the old pool.
    #[warn(clippy::cognitive_complexity)]
    async fn stop_old(
        &self,
        upgrade: &mut Upgrade<'_>,
        handle: &crate::jobs::JobHandle,
    ) -> Result<UpgradeStep, Error> {
        if let (Some((from_pool, _)), Some(standing)) = (&upgrade.survey().pools, upgrade.was)
            && standing.running
        {
            handle.progress(85, &format!("stopping {from_pool}")).await;
            self.service_stop(&crate::api::target(from_pool)).await?;
        }

        settle(upgrade.plan_mut(), &UpgradeOutcome::Done {});

        Ok(UpgradeStep::RemoveOld)
    }

    /// 9. Remove `from`, or keep it.
    #[warn(clippy::cognitive_complexity)]
    async fn remove_old(
        &self,
        upgrade: &mut Upgrade<'_>,
        handle: &crate::jobs::JobHandle,
    ) -> UpgradeStep {
        let (kind, from) = (upgrade.kind, upgrade.from);
        handle
            .progress(95, &format!("removing {kind} {from}"))
            .await;
        let because = upgrade.survey().reasons_to_keep(kind, from);
        let old = match kept_without_trying(upgrade.asked.keep, because) {
            Some(kept) => kept,
            None => self.uninstall_old(kind, from).await,
        };

        // Kept, so the listing would offer this same update again for as long as `from` stays.
        // The update itself is done: a note that cannot be written is a warning, not a failure.
        if matches!(old, OldVersion::Kept { .. })
            && let Err(error) = mixengine_core::upgrade::kept::remember(
                &self.store,
                mixengine_core::upgrade::kept::Family::Runtime,
                kind.as_str(),
                from.as_str(),
                upgrade.resolved.to.as_str(),
            )
            .await
        {
            tracing::warn!(%error, "could not remember the version this update kept");
        }

        upgrade.plan_mut().old = old;

        UpgradeStep::Done
    }

    /// Uninstall `from`, or say why it stays.
    #[warn(clippy::cognitive_complexity)]
    async fn uninstall_old(&self, kind: RuntimeKind, from: &PackageVersion) -> OldVersion {
        match self
            .runtimes
            .uninstall(&RuntimeUninstall {
                target: RuntimeTarget {
                    kind,
                    version: from.clone(),
                },
                force: false,
            })
            .await
        {
            Ok(_) => OldVersion::Removed {},
            Err(error) => OldVersion::Kept {
                because: vec![error.message],
            },
        }
    }
}

#[cfg(test)]
mod decisions;
