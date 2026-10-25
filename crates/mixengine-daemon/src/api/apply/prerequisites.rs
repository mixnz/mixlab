//! What a blueprint needs installed before it can be applied, and installing it.

use std::collections::BTreeMap;

use mixengine_proto::{
    BlueprintPlan, Disposition, Error, ErrorCode, PackageVersion, PlanAction, Requirement,
};

use super::super::Api;
use crate::error::ToWire as _;
use crate::jobs::JobHandle;
use crate::requirements;

use super::steps;

impl Api {
    /// Every version this apply will install, decided before it writes anything.
    ///
    /// **Nothing is written until every version is known.** A plan reads this home's tables and
    /// never the index, so it holds a [`VersionConstraint`](mixengine_proto::VersionConstraint)
    /// where a release belongs. Turning one into a release needs the index, which needs the network
    /// — so it happens here, once, at the top of the job: a constraint nothing satisfies fails
    /// while the ledger is still empty and there is nothing to take back.
    pub(super) async fn resolve(
        &self,
        plan: &BlueprintPlan,
        handle: &JobHandle,
    ) -> Result<BTreeMap<String, PackageVersion>, Error> {
        let mut resolved = BTreeMap::new();

        for step in &plan.steps {
            if !matches!(step.disposition, Disposition::Create) {
                continue;
            }

            match &step.action {
                PlanAction::InstallRuntime { kind, wanted } => {
                    handle
                        .progress(
                            0,
                            format!("looking up {} {}", kind.as_str(), wanted.as_str()),
                        )
                        .await;

                    resolved.insert(
                        steps::runtime_key(*kind),
                        self.runtimes.newest_satisfying(*kind, wanted).await?,
                    );
                }

                PlanAction::InstallPackage { package, wanted } => {
                    handle.progress(0, format!("looking up {package}")).await;

                    resolved.insert(
                        steps::package_key(package),
                        self.packages
                            .newest_satisfying(package, wanted.as_ref())
                            .await?,
                    );
                }

                _ => {}
            }
        }

        Ok(resolved)
    }

    /// The release every `Create` install step resolves to, without reporting progress.
    ///
    /// [`Self::resolve`]'s lookups for a caller with no job: the dry run.
    async fn install_targets(
        &self,
        plan: &BlueprintPlan,
    ) -> Result<Vec<(String, PackageVersion)>, Error> {
        let mut targets = Vec::new();

        for step in &plan.steps {
            if !matches!(step.disposition, Disposition::Create) {
                continue;
            }

            match &step.action {
                PlanAction::InstallRuntime { kind, wanted } => targets.push((
                    kind.as_str().to_owned(),
                    self.runtimes.newest_satisfying(*kind, wanted).await?,
                )),
                PlanAction::InstallPackage { package, wanted } => targets.push((
                    package.clone(),
                    self.packages
                        .newest_satisfying(package, wanted.as_ref())
                        .await?,
                )),
                _ => {}
            }
        }

        Ok(targets)
    }

    /// What those releases lack on this machine, merged.
    ///
    /// [`None`] when anything needed to judge could not be had: a dry run prints its plan regardless.
    pub(super) async fn blueprint_needs(&self, plan: &BlueprintPlan) -> Option<Vec<Requirement>> {
        let targets = self.install_targets(plan).await.ok()?;
        let names: Vec<&str> = targets.iter().map(|(kind, _)| kind.as_str()).collect();
        let catalogue = self.runtimes.fetcher().index.kinds(&names).await.ok()?;
        let facts = requirements::facts();

        Some(mixengine_core::requirements::merged(
            targets.iter().flat_map(|(kind, version)| {
                requirements::of(&catalogue.index, kind, version.as_str(), &facts)
            }),
        ))
    }

    /// Refuse, or install what was agreed and judge again — before the first step writes anything.
    ///
    /// # Errors
    ///
    /// `dependency_missing` for a lack nothing agreed to fixes, and whatever
    /// [`requirements::satisfy`] refuses with.
    pub(super) async fn meet_requirements(
        &self,
        plan: &BlueprintPlan,
        install_prerequisites: bool,
        handle: &JobHandle,
    ) -> Result<(), Error> {
        // A plan that installs nothing asks the index nothing — an apply that worked offline before
        // requirements were checked still does.
        let targets = self.install_targets(plan).await?;
        if targets.is_empty() {
            return Ok(());
        }

        // **An index that cannot be read refuses nothing here**, on `requirements::gate`'s reasoning:
        // the install steps read it too, and report that in their own words.
        let fetcher = self.runtimes.fetcher();
        let names: Vec<&str> = targets.iter().map(|(kind, _)| kind.as_str()).collect();
        let Ok(catalogue) = fetcher.index.kinds(&names).await else {
            return Ok(());
        };

        let judge = || {
            let facts = requirements::facts();
            mixengine_core::requirements::merged(targets.iter().flat_map(|(kind, version)| {
                requirements::of(&catalogue.index, kind, version.as_str(), &facts)
            }))
        };
        let subject = requirements::Subject {
            name: "this blueprint".to_owned(),
            install: None,
        };

        let unmet = judge();
        if let Some(refused) = requirements::refusal(&subject, &unmet, install_prerequisites) {
            return Err(refused);
        }
        if !mixengine_core::requirements::needs_consent(&unmet) {
            return Ok(());
        }

        requirements::satisfy(fetcher, &unmet, handle).await?;
        requirements::refusal(&subject, &judge(), false).map_or(Ok(()), Err)
    }

    /// The installed version of a package that satisfies what the blueprint asked for.
    ///
    /// The newest of them, which is the same rule the plan used — applied to what is on disk *now*,
    /// because the install step just before this one may be what put it there.
    pub(super) async fn installed_package(
        &self,
        package: &str,
        wanted: Option<&mixengine_proto::VersionConstraint>,
    ) -> Result<mixengine_proto::PackageVersion, Error> {
        let installed = mixengine_core::packages::records(&self.store, Some(package))
            .await
            .map_err(|error| error.to_wire())?;

        installed
            .into_iter()
            .filter(|record| wanted.is_none_or(|wanted| wanted.matches(&record.version)))
            .map(|record| record.version)
            .max_by(mixengine_proto::PackageVersion::cmp_precedence)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PreconditionFailed,
                    match wanted {
                        Some(wanted) => {
                            format!("no installed {package} satisfies {}", wanted.as_str())
                        }
                        None => format!("no {package} is installed"),
                    },
                )
                .with_hint("`mix package available` lists what the index offers")
            })
    }
}
