//! `runtime.upgrade_plan` and `runtime.upgrade` — roadmap task **T193b**, the design's D5.

use mixengine_core::extensions::manifest::Body;
use mixengine_core::upgrade::{self, ManifestPin, PinRewrite};
use mixengine_proto::{
    Error, ErrorCode, ExtensionId, OldVersion, PackageVersion, RuntimeKind, RuntimeUpgradeQuery,
    ServiceId, SiteKind, UpgradeEntry, UpgradeItem, UpgradeOutcome, UpgradePlan, VersionConstraint,
};

use super::{Resolved, resolve};
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
}
