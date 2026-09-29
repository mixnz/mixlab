//! `package.*`: what the index offers of the servers this build can run, what this machine has, and
//! the job that turns one into the other.
//!
//! Roadmap task **T31a**. [`crate::runtimes`] one namespace across, and deliberately built out of
//! the same pieces — the same [`Fetcher`], the same [`Jobs`], the same install-then-record ordering.
//! What is *not* repeated is the reasoning: why an install returns a job, why an install already
//! running is answered with its job rather than started twice, and why the row is written after the
//! rename are all written next door and are true here for the same reasons.
//!
//! # Only what this build can run is offered
//!
//! A [`Recipe`](mixengine_core::generate::Recipe) is what turns an unpacked directory into a service
//! that starts: which binary, which template, how to tell it is up. A package with no recipe is a
//! download that ends in a directory nothing can use — so it is not listed, and installing it is
//! refused before a byte is spent rather than at `service.create`, where the disk is already gone.
//!
//! Two MixEngine versions reading one index therefore answer differently, which is correct: the
//! question a person is asking is *what can I run*, and each of T33–T37 unlocks its own kind by
//! shipping the recipe in the same commit.
//!
//! # Runtimes are not packages
//!
//! PHP is installed through [`crate::runtimes`], into `runtimes/`, with a default version and a shim
//! that reads it. A second door to the same room would either duplicate all of that or produce a PHP
//! the shim cannot see, so the catalogue filter here is what keeps them apart: no runtime has a
//! recipe, so no runtime is offered.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use mixengine_core::generate::Catalogue;
use mixengine_core::{Paths, Store, packages, paths};
use mixengine_proto::{
    Error, ErrorCode, JobId, JobKind, JobSummary, PackageCatalogue, PackageFilter, PackageInstall,
    PackageList, PackageRelease, PackageRemoval, PackageSummary, PackageTarget, PackageUpdate,
    PackageVersion, Requirements, Timestamp, VersionConstraint, rpc,
};

use crate::error::ToWire as _;
use crate::jobs::{JobHandle, Jobs};
use crate::requirements;
use crate::runtimes::Fetcher;

/// Everything `package.*` needs, and the only thing that starts a package install.
#[derive(Debug)]
pub(crate) struct Packages {
    /// Where a package lands.
    paths: Paths,

    /// Where the row goes.
    store: Store,

    /// What turns the work into a job, and the only thing that can end one.
    jobs: Arc<Jobs>,

    /// The index that offers versions, and the pipeline that downloads them.
    fetcher: Arc<Fetcher>,

    /// What this build knows how to configure and run, which is what it is willing to install.
    ///
    /// The very set [`crate::services`] renders from, and not a second one: a home that could
    /// install something it then refused to configure would be a home whose two answers to "can I
    /// run this" disagree.
    catalogue: Catalogue,

    /// The installs this daemon is running, by what they are installing.
    ///
    /// A `tokio` mutex for [`crate::runtimes`]' reason: it is held across the `await` that starts
    /// the job, so that "is this already running" and "start one" are one decision.
    running: tokio::sync::Mutex<BTreeMap<(String, PackageVersion), JobId>>,
}

impl Packages {
    /// Point the package methods at an index and the pipeline that downloads from it.
    pub(crate) fn new(
        paths: &Paths,
        store: &Store,
        jobs: Arc<Jobs>,
        fetcher: Arc<Fetcher>,
    ) -> Arc<Self> {
        Arc::new(Self {
            paths: paths.clone(),
            store: store.clone(),
            jobs,
            fetcher,
            catalogue: crate::services::catalogue(),
            running: tokio::sync::Mutex::new(BTreeMap::new()),
        })
    }

    /// The install job running for this version, if there is one — T193c.
    pub(crate) async fn installing(
        &self,
        package: &str,
        version: &PackageVersion,
    ) -> Option<JobId> {
        self.running
            .lock()
            .await
            .get(&(package.to_owned(), version.clone()))
            .copied()
    }

    /// `package.list` — what is on this machine, and what is holding each of them.
    ///
    /// # Errors
    ///
    /// The wire error of a table that could not be read.
    pub(crate) async fn list(&self, filter: &PackageFilter) -> Result<PackageList, Error> {
        let packages = packages::records(&self.store, filter.package.as_deref())
            .await
            .map_err(|error| error.to_wire())?;

        Ok(PackageList { packages })
    }

    /// `package.list_available` — what the index offers of what this build can run.
    ///
    /// **Composed here rather than by the client**, which is the rule in `CLAUDE.md`: whether a
    /// listed version is installed is a fact about two lists, and leaving a client to cross-reference
    /// them would be two clients able to disagree about what "installed" means.
    ///
    /// # Errors
    ///
    /// `invalid_argument` for a filter naming a package this build cannot run — a filter that
    /// silently matched nothing would look exactly like an index that publishes nothing — and the
    /// wire error of an index that could not be obtained *at all*. A fetch that fails while a cached
    /// index exists is answered from the cache, with [`PackageCatalogue::stale`] set.
    ///
    /// `filter.refresh` reaches the network even if the cache is still fresh — see
    /// [`PackageFilter::refresh`](mixengine_proto::PackageFilter::refresh).
    pub(crate) async fn list_available(
        &self,
        filter: &PackageFilter,
    ) -> Result<PackageCatalogue, Error> {
        let wanted: Vec<String> = match filter.package.as_deref() {
            Some(package) => vec![self.runnable(package)?],
            None => self.catalogue.packages().map(str::to_owned).collect(),
        };

        let catalogue = match filter.refresh {
            true => self.fetcher.index.refresh().await,
            false => self.fetcher.index.catalogue().await,
        }
        .map_err(|error| error.to_wire())?;
        let installed = packages::records(&self.store, filter.package.as_deref())
            .await
            .map_err(|error| error.to_wire())?;
        let facts = requirements::facts();

        let mut offered = Vec::new();
        let mut updates = Vec::new();

        for name in &wanted {
            let name = name.as_str();
            let mut versions = Vec::new();

            for package in catalogue.index.installable(name) {
                // An index that offers a version this build could not make a directory for is one
                // whose entry is skipped rather than one that fails the listing, on
                // `runtimes::list_available`'s reasoning: the other versions are still installable.
                let Ok(version) = PackageVersion::parse(package.version.clone()) else {
                    tracing::warn!(
                        package = name,
                        version = package.version,
                        "the package index offers a version this build cannot use as a directory \
                         name; skipping it"
                    );
                    continue;
                };

                let chosen = catalogue.index.artifact(name, &package.version);

                offered.push(PackageRelease {
                    installed: installed
                        .iter()
                        .any(|have| have.package == name && have.version == version),
                    package: name.to_owned(),
                    line: Some(mixengine_core::lines::line_of(name, &version)),
                    newest_in_line: Some(false),
                    version: version.clone(),
                    channel: package.channel.into(),
                    eol: package.eol.clone(),
                    bytes: chosen.map_or(0, |chosen| chosen.artifact.size),
                    execution: chosen.map(|chosen| chosen.execution),
                    needs: chosen.map(|_| {
                        requirements::of(&catalogue.index, name, &package.version, &facts)
                    }),
                });
                versions.push(version);
            }

            // T193a, D1 — `runtimes::list_available`'s reasoning.
            let newest = mixengine_core::lines::newest_of_each_line(name, versions.iter());
            for release in offered.iter_mut().filter(|release| release.package == name) {
                release.newest_in_line = Some(newest.values().any(|n| *n == release.version));
            }

            for have in installed.iter().filter(|have| have.package == name) {
                let Some(to) =
                    mixengine_core::lines::update_for(name, &have.version, versions.iter())
                else {
                    continue;
                };

                updates.push(PackageUpdate {
                    package: name.to_owned(),
                    to_installed: installed
                        .iter()
                        .any(|other| other.package == name && other.version == to),
                    needs: Some(requirements::of(
                        &catalogue.index,
                        name,
                        to.as_str(),
                        &facts,
                    )),
                    from: have.version.clone(),
                    to,
                });
            }
        }

        Ok(PackageCatalogue {
            packages: offered,
            stale: catalogue.freshness.is_stale(),
            updates: Some(updates),
        })
    }

    /// The newest release the index publishes for this machine that satisfies `wanted`.
    ///
    /// [`crate::runtimes::Runtimes::newest_satisfying`]'s twin, and there for the same reason
    /// (roadmap task T78, its design's D9): a plan holds a constraint, and only the index can turn
    /// one into a release. [`None`] takes the newest published, which is what a blueprint naming a
    /// package without pinning it asked for.
    ///
    /// # Errors
    ///
    /// `invalid_argument` for a package this build cannot run; `not_found` when the index publishes
    /// nothing for this machine that satisfies the constraint; and the wire error of an index that
    /// could not be obtained at all.
    pub(crate) async fn newest_satisfying(
        &self,
        package: &str,
        wanted: Option<&VersionConstraint>,
    ) -> Result<PackageVersion, Error> {
        let name = self.runnable(package)?;
        let name = name.as_str();
        let catalogue = self
            .fetcher
            .index
            .catalogue()
            .await
            .map_err(|error| error.to_wire())?;

        catalogue
            .index
            .installable(name)
            .filter_map(|offered| PackageVersion::parse(offered.version.clone()).ok())
            .filter(|version| wanted.is_none_or(|wanted| wanted.matches(version)))
            .max_by(PackageVersion::cmp_precedence)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::NotFound,
                    match wanted {
                        Some(wanted) => format!(
                            "the package index publishes no {name} for this machine that satisfies \
                             {}",
                            wanted.as_str()
                        ),
                        None => {
                            format!("the package index publishes no {name} for this machine at all")
                        }
                    },
                )
                .with_hint(format!(
                    "`mix package available --package {name}` lists what it does publish"
                ))
            })
    }

    /// `package.requirements` — [`crate::runtimes::Runtimes::requirements`] for a service package.
    ///
    /// # Errors
    ///
    /// A package this build cannot run, a version not published here, and the wire error of an
    /// index that could not be obtained at all.
    pub(crate) async fn requirements(&self, target: &PackageTarget) -> Result<Requirements, Error> {
        let package = self.runnable(&target.package)?;
        let catalogue = self
            .fetcher
            .index
            .catalogue()
            .await
            .map_err(|error| error.to_wire())?;
        crate::runtimes::offered(
            &catalogue.index,
            &package,
            target.version.as_str(),
            &format!("mix package available --package {package}"),
        )?;

        Ok(Requirements {
            unmet: requirements::of(
                &catalogue.index,
                &package,
                target.version.as_str(),
                &requirements::facts(),
            ),
        })
    }

    /// `package.install` — start the download, and answer with the job doing it.
    ///
    /// Four things are decided before a job exists, because all four are answers a caller should
    /// have immediately rather than through a job that fails a moment later: a package this build
    /// cannot run, a version that is already installed, a version this daemon is already
    /// installing, and — since T149 — a version this machine certainly lacks something for.
    ///
    /// # Errors
    ///
    /// `invalid_argument` for a package with no recipe, `already_exists` when it is installed,
    /// `dependency_missing` when this machine lacks what it needs, and the wire error of a row that
    /// could not be read or a job that could not be started.
    pub(crate) async fn install(
        self: &Arc<Self>,
        asked: &PackageInstall,
    ) -> Result<JobSummary, Error> {
        let target = &asked.target;
        let package = self.runnable(&target.package)?;
        let package = package.as_str();

        // Held across the whole of this, so that "is one running" and "start one" are one decision.
        let mut running = self.running.lock().await;

        let key = (package.to_owned(), target.version.clone());
        if let Some(job) = running.get(&key).copied() {
            tracing::debug!(
                package,
                version = target.version.as_str(),
                %job,
                "an install of this version is already running; answering with its job"
            );
            return self.jobs.status(job).await;
        }

        match packages::record(&self.store, package, &target.version).await {
            Ok(_) => {
                return Err(mixengine_core::Error::PackageAlreadyRecorded {
                    package: package.to_owned(),
                    version: target.version.clone(),
                }
                .to_wire());
            }
            Err(mixengine_core::Error::NotFound { .. }) => {}
            Err(error) => return Err(error.to_wire()),
        }

        if !asked.ignore_requirements {
            requirements::gate(
                &self.fetcher,
                package,
                target.version.as_str(),
                &requirements::Subject {
                    name: format!("{package} {}", target.version.as_str()),
                    install: Some(format!("mix package install {package}")),
                },
                asked.install_prerequisites,
            )
            .await?;
        }

        let kind = JobKind::parse(rpc::method::PACKAGE_INSTALL)
            .expect("`package.install` is a method name, which is what a job kind is");

        let packages = Arc::clone(self);
        let prepare = asked.install_prerequisites && !asked.ignore_requirements;
        let target = PackageTarget {
            package: package.to_owned(),
            version: target.version.clone(),
        };
        let started = self
            .jobs
            .begin(&kind, move |handle| async move {
                // What the person agreed to happens first, on `runtimes::install`'s reasoning — T150.
                let prepared = match prepare {
                    true => {
                        requirements::prepare(
                            &packages.fetcher,
                            &target.package,
                            target.version.as_str(),
                            &requirements::Subject {
                                name: format!("{} {}", target.package, target.version.as_str()),
                                install: Some(format!("mix package install {}", target.package)),
                            },
                            &handle,
                        )
                        .await
                    }
                    false => Ok(()),
                };

                let outcome = match prepared {
                    Ok(()) => packages.perform(&target, &handle).await,
                    Err(refused) => Err(refused),
                };

                // Released here rather than by the caller, on `runtimes::install`'s reasoning: this
                // future is what owns the install, and it ends by being cancelled as well as by
                // returning.
                packages
                    .running
                    .lock()
                    .await
                    .remove(&(target.package, target.version));

                outcome
            })
            .await?;

        running.insert(key, started.id);

        Ok(started)
    }

    /// The work behind an install: look it up, fetch it, write it down.
    ///
    /// The three steps are in this order for [`crate::runtimes`]' reasons exactly — a failure
    /// anywhere leaves either nothing or a directory with no row, and never a row describing a
    /// package that is not there.
    pub(crate) async fn perform(
        &self,
        target: &PackageTarget,
        handle: &JobHandle,
    ) -> Result<serde_json::Value, Error> {
        let (package, version) = (target.package.as_str(), &target.version);
        tracing::info!(job = %handle.id(), package, version = version.as_str(), "installing a package");

        handle.progress(0, "reading the package index").await;
        let catalogue = self
            .fetcher
            .index
            .catalogue()
            .await
            .map_err(|error| error.to_wire())?;
        let (_, selection) = crate::runtimes::offered(
            &catalogue.index,
            package,
            version.as_str(),
            &format!("mix package available --package {package}"),
        )?;

        if let Some(notice) =
            crate::runtimes::emulation_notice(package, version.as_str(), selection)
        {
            handle.progress(0, &notice).await;
        }

        let into = packages::directory(&self.paths, package, version);
        if let Some(parent) = into.parent() {
            paths::create_dir(parent).map_err(|error| error.to_wire())?;
        }

        // **The recipe's, not this module's.** Which executable proves a Caddy runs is a fact about
        // Caddy, and it belongs beside the template that configures it — where a runtime's is a
        // `match` in `core::runtimes` because a runtime has no recipe to hold it. A package whose
        // recipe names none is installed without one, which is the trait's own default.
        let smoke = self
            .catalogue
            .recipe(package)
            .and_then(|recipe| recipe.smoke_test());

        // Before the install, because the marker it leaves in the directory is this row (T182f).
        let installation = packages::Installation {
            package: package.to_owned(),
            version: version.clone(),
            path: into.clone(),
            bytes: selection.artifact.size,
            url: selection.artifact.url.clone(),
            sha256: selection.artifact.sha256.clone(),
            provides: selection.artifact.provides.clone(),
        };
        let marker = mixengine_core::adopt::marker::Marker::package(&installation).encode();

        let installed = match self
            .fetcher
            .installer
            .install_marked(
                selection.artifact,
                &into,
                smoke.as_ref(),
                mixengine_core::install::NotAnArchive::Refuse,
                Some(&marker),
                handle,
            )
            .await
        {
            Ok(installed) => installed,

            // **T182f, D2**, on `runtimes::perform`'s reasoning: an earlier home installed it.
            Err(mixengine_core::Error::AlreadyInstalled { path }) => {
                let found = mixengine_core::adopt::walk::Found {
                    subject: mixengine_core::adopt::Subject::Package {
                        package: package.to_owned(),
                        version: version.clone(),
                    },
                    path,
                };

                return match crate::adopt::claim_offered(
                    &self.store,
                    &found,
                    &selection.artifact.sha256,
                    handle,
                )
                .await?
                {
                    mixengine_core::adopt::walk::Claimed::Package(summary) => {
                        self.after_recorded(&summary)
                    }
                    mixengine_core::adopt::walk::Claimed::Runtime(_) => Err(Error::new(
                        ErrorCode::Internal,
                        "a package directory was recorded as a runtime",
                    )),
                };
            }

            Err(error) => return Err(error.to_wire()),
        };

        let record = packages::remember(
            &self.store,
            &packages::Installation {
                path: installed.path.clone(),
                bytes: installed.bytes,
                ..installation
            },
            Timestamp::from_system_time(SystemTime::now()),
        )
        .await;

        let summary = match record {
            Ok(summary) => summary,

            // The one place the install-then-record ordering is undone rather than kept, for the
            // reason `runtimes::perform` states: this is the moment we know a directory with no row
            // exists, and leaving it would make the retry fail with `already installed` instead.
            Err(error) => {
                if let Err(cleanup) = packages::discard(&installed.path).await {
                    tracing::warn!(
                        path = %installed.path.display(),
                        %cleanup,
                        "a package whose row could not be written could not be removed either"
                    );
                }
                return Err(error.to_wire());
            }
        };

        self.after_recorded(&summary)
    }

    /// What a newly recorded package answers — shared by an install, an adopt and the start walk
    /// (T182f). A package needs nothing more once its row exists: an instance of it is
    /// `service.create`, which a person asks for.
    ///
    /// # Errors
    ///
    /// Only when the summary cannot be encoded.
    pub(crate) fn after_recorded(
        &self,
        summary: &PackageSummary,
    ) -> Result<serde_json::Value, Error> {
        serde_json::to_value(summary).map_err(|error| {
            Error::new(
                ErrorCode::Internal,
                format!("what the install produced could not be encoded: {error}"),
            )
        })
    }

    /// `package.found` — every package directory on disk with no row, and why — roadmap task
    /// **T182i**. Offline: no index, and nothing recorded.
    ///
    /// # Errors
    ///
    /// The wire error of a table that could not be read.
    pub(crate) async fn found(&self) -> Result<mixengine_proto::PackageFoundList, Error> {
        let found = mixengine_core::adopt::walk::unrecorded(&self.store, &self.paths)
            .await
            .map_err(|error| error.to_wire())?;

        Ok(mixengine_proto::PackageFoundList {
            found: found
                .iter()
                .filter_map(|one| match &one.subject {
                    mixengine_core::adopt::Subject::Package { package, version } => {
                        Some(mixengine_proto::PackageFound {
                            package: package.clone(),
                            version: version.clone(),
                            path: one.path.display().to_string(),
                            why: mixengine_core::adopt::walk::examine(one),
                        })
                    }
                    mixengine_core::adopt::Subject::Runtime { .. } => None,
                })
                .collect(),
        })
    }

    /// `package.adopt` — record a version that is on disk without a row — roadmap task **T182f**.
    ///
    /// A version already recorded answers its row, so asking twice is not an error.
    ///
    /// # Errors
    ///
    /// As [`crate::adopt::by_hand`].
    pub(crate) async fn adopt(&self, target: &PackageTarget) -> Result<PackageSummary, Error> {
        match packages::record(&self.store, &target.package, &target.version).await {
            Ok(summary) => return Ok(summary),
            Err(mixengine_core::Error::NotFound { .. }) => {}
            Err(error) => return Err(error.to_wire()),
        }

        let found = mixengine_core::adopt::walk::Found {
            subject: mixengine_core::adopt::Subject::Package {
                package: target.package.clone(),
                version: target.version.clone(),
            },
            path: packages::directory(&self.paths, &target.package, &target.version),
        };

        match crate::adopt::by_hand(&self.store, &found, &self.fetcher.index).await? {
            mixengine_core::adopt::walk::Claimed::Package(summary) => Ok(summary),
            mixengine_core::adopt::walk::Claimed::Runtime(_) => Err(Error::new(
                ErrorCode::Internal,
                "a package directory was recorded as a runtime",
            )),
        }
    }

    /// `package.uninstall` — refuse while anything is an instance of it, then remove the directory
    /// and the row.
    ///
    /// **The refusal comes first and names the services**, because `services.package_id` is
    /// `ON DELETE RESTRICT`: without it the directory would already be gone by the time SQLite
    /// refused the row, which is the one outcome nothing can repair. What a person does about it is
    /// `service.delete`, and the message says so.
    ///
    /// # Errors
    ///
    /// `not_found` when it is not installed, `precondition_failed` when a service is an instance of
    /// it, and the wire error of a directory that could not be removed — on Windows, most often a
    /// process still running out of it.
    pub(crate) async fn uninstall(&self, target: &PackageTarget) -> Result<PackageRemoval, Error> {
        let removed = packages::record(&self.store, &target.package, &target.version)
            .await
            .map_err(|error| error.to_wire())?;

        if !removed.services.is_empty() {
            let held = removed
                .services
                .iter()
                .map(|service| service.as_str())
                .collect::<Vec<_>>()
                .join(", ");

            return Err(Error::new(
                ErrorCode::PreconditionFailed,
                format!(
                    "{} {} is still what {held} runs",
                    target.package, target.version
                ),
            )
            .with_hint(format!(
                "`mix service delete {}` first; deleting a service keeps its data directory",
                removed.services[0]
            )));
        }

        packages::discard(Path::new(&removed.path))
            .await
            .map_err(|error| error.to_wire())?;

        packages::forget(&self.store, &target.package, &target.version)
            .await
            .map_err(|error| error.to_wire())?;

        tracing::info!(
            package = target.package,
            version = target.version.as_str(),
            "a package was uninstalled"
        );

        Ok(PackageRemoval { removed })
    }

    /// The package's own name, or the refusal that names what this build can run instead.
    ///
    /// The catalogue's own spelling rather than the caller's string, so that everything downstream
    /// holds the name this build knows rather than one that merely compared equal to it. Owned
    /// since T81, when a catalogue stopped being made only of literals — an extension's recipe is
    /// built at run time out of a row.
    fn runnable(&self, package: &str) -> Result<String, Error> {
        self.catalogue
            .packages()
            .find(|known| *known == package)
            .map(str::to_owned)
            .ok_or_else(|| {
                let known = self.catalogue.packages().collect::<Vec<_>>().join(", ");

                Error::new(
                    ErrorCode::InvalidArgument,
                    format!("this build of MixEngine cannot run {package}"),
                )
                .with_hint(format!(
                    "it knows how to configure and run: {known}; a package with no recipe would \
                     unpack into a directory nothing could start"
                ))
            })
    }
}
