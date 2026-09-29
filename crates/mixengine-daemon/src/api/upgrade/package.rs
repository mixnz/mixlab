//! `package.upgrade_plan` and `package.upgrade` — roadmap task **T193c**, the design's D6.
//!
//! **One instance at a time, and each one proven before the next.** An instance that does not
//! start on `to` is put back on `from` and started again, and `from` is removed only when every
//! instance has moved.

use std::path::Path;
use std::sync::Arc;

use mixengine_core::{lines, upgrade};
use mixengine_proto::{
    Error, ErrorCode, JobSummary, OldVersion, PackageTarget, PackageUpgrade, PackageUpgradeQuery,
    PackageVersion, ServiceId, UpgradeEntry, UpgradeItem, UpgradeOutcome, UpgradePlan, rpc,
};

use super::{Resolved, resolve, settle};
use crate::api::Api;
use crate::error::ToWire as _;

/// One instance of `from`, as the update found it.
#[derive(Debug, Clone)]
struct Instance {
    service: ServiceId,
    running: bool,
    front_end: bool,
}

/// The plan for moving `instances`, every entry `planned`.
fn package_plan(
    package: &str,
    from: &PackageVersion,
    resolved: &Resolved,
    instances: &[Instance],
) -> UpgradePlan {
    let planned = |item| UpgradeEntry {
        item,
        outcome: UpgradeOutcome::Planned {},
    };
    let mut entries = Vec::new();

    for instance in instances {
        entries.push(planned(UpgradeItem::Instance {
            service: instance.service.clone(),
            restarts: instance.running,
        }));
        if instance.front_end && instance.running {
            entries.push(planned(UpgradeItem::FrontEndRestart {
                service: instance.service.clone(),
            }));
        }
        // **D6 step 5.** Within 8.0 a newer patch upgrades the data dictionary on first start.
        if package == "mysql" && lines::line_of(package, from) == "8.0" {
            entries.push(planned(UpgradeItem::NoDowngrade {
                service: instance.service.clone(),
            }));
        }
    }

    UpgradePlan {
        subject: package.to_owned(),
        from: from.clone(),
        to: resolved.to.clone(),
        to_installed: resolved.to_installed,
        bytes: resolved.bytes,
        stale: resolved.stale,
        needs: resolved.needs.clone(),
        entries,
        old: OldVersion::WillBeRemoved {},
    }
}

/// Set the outcome of `service`'s `instance` entry.
fn mark(plan: &mut UpgradePlan, service: &ServiceId, outcome: UpgradeOutcome) {
    for entry in &mut plan.entries {
        if matches!(&entry.item, UpgradeItem::Instance { service: moved, .. } if moved == service) {
            entry.outcome = outcome.clone();
        }
    }
}

impl Api {
    /// The installed versions of `package`.
    async fn installed_packages(&self, package: &str) -> Result<Vec<PackageVersion>, Error> {
        Ok(
            mixengine_core::packages::records(&self.store, Some(package))
                .await
                .map_err(|error| error.to_wire())?
                .into_iter()
                .map(|row| row.version)
                .collect(),
        )
    }

    /// Every instance of `package from`, and whether each is running and is the front end.
    async fn instances_of(
        &self,
        package: &str,
        from: &PackageVersion,
    ) -> Result<Vec<Instance>, Error> {
        let wire = |error: mixengine_core::Error| error.to_wire();
        let front = mixengine_core::services::front_end::held_by(
            &self.store,
            &crate::services::catalogue(),
        )
        .await
        .map_err(wire)?;

        let mut instances = Vec::new();
        for service in mixengine_core::packages::holders(&self.store, package, from)
            .await
            .map_err(wire)?
        {
            let standing = self.standing_of(&service).await?;
            instances.push(Instance {
                front_end: front.as_deref() == Some(service.as_str()),
                running: standing.running,
                service,
            });
        }

        Ok(instances)
    }

    /// `package.upgrade_plan` — what an update would do, changing nothing.
    ///
    /// # Errors
    ///
    /// What [`resolve`] refuses, and the wire error of a table that could not be read.
    pub(crate) async fn package_upgrade_plan(
        &self,
        query: &PackageUpgradeQuery,
    ) -> Result<UpgradePlan, Error> {
        let installed = self.installed_packages(&query.package).await?;
        let resolved = resolve(
            self.runtimes.fetcher(),
            &query.package,
            &installed,
            &query.from,
            query.to.as_ref(),
        )
        .await?;
        let instances = self.instances_of(&query.package, &query.from).await?;

        Ok(package_plan(
            &query.package,
            &query.from,
            &resolved,
            &instances,
        ))
    }

    /// `package.upgrade` — one job, and the refusals that come before it.
    ///
    /// # Errors
    ///
    /// As `runtime_upgrade`'s.
    pub(crate) async fn package_upgrade(
        self: &Arc<Self>,
        asked: PackageUpgrade,
    ) -> Result<JobSummary, Error> {
        let package = asked.query.package.clone();
        let installed = self.installed_packages(&package).await?;
        let resolved = resolve(
            self.runtimes.fetcher(),
            &package,
            &installed,
            &asked.query.from,
            asked.query.to.as_ref(),
        )
        .await?;

        if let Some(job) = self.packages.installing(&package, &resolved.to).await {
            return Err(Error::new(
                ErrorCode::Conflict,
                format!("job {job} is installing {package} {}", resolved.to),
            ));
        }

        if !resolved.to_installed && !asked.ignore_requirements {
            crate::requirements::gate(
                self.runtimes.fetcher(),
                &package,
                resolved.to.as_str(),
                &subject(&package, &resolved.to),
                asked.install_prerequisites,
            )
            .await?;
        }

        let api = Arc::clone(self);
        let (from, to) = (asked.query.from.clone(), resolved.to.clone());
        self.begin_upgrade(
            rpc::method::PACKAGE_UPGRADE,
            &package,
            &from,
            &to,
            move |handle| async move { api.package_walk(&asked, resolved, &handle).await },
        )
        .await
    }

    /// The walk — D6, steps 1 to 8.
    async fn package_walk(
        &self,
        asked: &PackageUpgrade,
        resolved: Resolved,
        handle: &crate::jobs::JobHandle,
    ) -> Result<UpgradePlan, Error> {
        let package = asked.query.package.as_str();
        let from = &asked.query.from;
        let to = resolved.to.clone();
        let wire = |error: mixengine_core::Error| error.to_wire();

        // 1. Install `to`.
        if !resolved.to_installed {
            if asked.install_prerequisites && !asked.ignore_requirements {
                crate::requirements::prepare(
                    self.runtimes.fetcher(),
                    package,
                    to.as_str(),
                    &subject(package, &to),
                    handle,
                )
                .await?;
            }
            handle
                .progress(5, &format!("installing {package} {to}"))
                .await;
            self.packages
                .perform(
                    &PackageTarget {
                        package: package.to_owned(),
                        version: to.clone(),
                    },
                    handle,
                )
                .await?;
        }

        let instances = self.instances_of(package, from).await?;
        let mut plan = package_plan(package, from, &resolved, &instances);

        // 2. The Linux port grant, before anything is stopped.
        if instances.iter().any(|instance| instance.front_end) {
            let summary = mixengine_core::packages::record(&self.store, package, &to)
                .await
                .map_err(wire)?;
            let binary = mixengine_core::generate::program(Path::new(&summary.path), package);

            handle
                .progress(
                    30,
                    "asking whether this machine will let it answer on 80 and 443",
                )
                .await;
            // A failed job, as `runtime_walk` says why: nothing has moved.
            if let Some(because) = self
                .would_lose_the_grant(asked.grant, &binary, true, handle)
                .await
            {
                return Err(Error::new(
                    ErrorCode::PrivilegedRequired,
                    format!("{because}; {package} {from} is still in use"),
                ));
            }
        }

        // 3–4. One instance at a time.
        let mut why = Vec::new();
        for instance in &instances {
            handle
                .progress(
                    50,
                    &format!("moving {} to {package} {to}", instance.service),
                )
                .await;
            let outcome = self.move_instance(instance, package, from, &to).await?;
            if let UpgradeOutcome::Failed { because } = &outcome {
                why.push(because.clone());
            }
            mark(&mut plan, &instance.service, outcome);
        }

        settle(&mut plan, &UpgradeOutcome::Done {});

        // 7. Client commands come from `to` now.
        if let Err(error) = self.shims.refresh().await {
            tracing::warn!(%error, "bin/ could not be refreshed after a package was updated");
        }

        // An instance that went back means the update did not happen for it: a failed job, which
        // is what makes `mix` exit non-zero and MixLab say so. The ones that moved stay moved.
        if !why.is_empty() {
            return Err(Error::new(ErrorCode::ProcessFailed, why.join("; ")));
        }

        // 8. Remove `from`, or keep it.
        plan.old = if asked.keep {
            OldVersion::Kept {
                because: vec!["you asked to keep it".to_owned()],
            }
        } else {
            match self
                .packages
                .uninstall(&PackageTarget {
                    package: package.to_owned(),
                    version: from.clone(),
                })
                .await
            {
                Ok(_) => OldVersion::Removed {},
                Err(error) => OldVersion::Kept {
                    because: vec![error.message],
                },
            }
        };

        Ok(plan)
    }

    /// Stop, repoint, render, start — and back again if it does not start (D6 steps 3–4).
    async fn move_instance(
        &self,
        instance: &Instance,
        package: &str,
        from: &PackageVersion,
        to: &PackageVersion,
    ) -> Result<UpgradeOutcome, Error> {
        let id = &instance.service;
        let wire = |error: mixengine_core::Error| error.to_wire();

        if instance.running {
            let walk = self.service_stop(&crate::api::target(id)).await?;
            if let Some(failed) = walk.failed {
                return Ok(UpgradeOutcome::Failed {
                    because: format!(
                        "{} could not be stopped, so it stayed on {package} {from}",
                        failed.service
                    ),
                });
            }
        }

        upgrade::repoint_instance(&self.store, id, package, to)
            .await
            .map_err(wire)?;
        self.services
            .reconfigure()
            .await
            .map_err(|error| error.to_wire())?;

        if !instance.running {
            return Ok(UpgradeOutcome::Done {});
        }

        let walk = self.service_start(&crate::api::target(id)).await?;
        if walk.failed.is_none() {
            return Ok(UpgradeOutcome::Done {});
        }

        // 4. Back to `from`.
        upgrade::repoint_instance(&self.store, id, package, from)
            .await
            .map_err(wire)?;
        self.services
            .reconfigure()
            .await
            .map_err(|error| error.to_wire())?;
        let back = self.service_start(&crate::api::target(id)).await?;

        Ok(UpgradeOutcome::Failed {
            because: match back.failed {
                None => format!(
                    "{id} did not start on {package} {to}; it is running on {from} again, and \
                     `mix service logs {id}` says why"
                ),
                Some(_) => format!(
                    "{id} did not start on {package} {to}, nor on {from} again; it is stopped on \
                     {from}, and `mix service logs {id}` says why"
                ),
            },
        })
    }
}

/// What a requirement refusal calls the package.
fn subject(package: &str, version: &PackageVersion) -> crate::requirements::Subject {
    crate::requirements::Subject {
        name: format!("{package} {version}"),
        install: Some(format!("mix package install {package}")),
    }
}
