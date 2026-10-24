//! `runtime.*`: what the index offers, what this machine has, and the job that turns one into the
//! other.
//!
//! **This is the job system's first producer.** Everything under it existed before this file did
//! and none of it could be reached: the index client verified a signed index nobody asked for, the
//! download pipeline installed an artifact nobody named, and the job system ran jobs nobody
//! started. What is added here is a method in front of each — which is why the wiring is an `impl`
//! and not an adapter, since [`mixengine_core::install::Watcher`] was shaped after [`JobHandle`] on
//! purpose.
//!
//! # Five of the six methods answer inline, and one returns a job
//!
//! The split is the download and nothing else. `docs/architecture/daemon-and-ipc.md` says a long
//! operation returns a job rather than holding a call open; removing a directory, reading a table
//! and moving a default are none of them long, and making every one of them return a job would make
//! a client learn a second protocol to hear an answer that was ready before it asked.
//!
//! `resolve` is the newest of the five and the only one a shim will *not* use: it calls
//! [`mixengine_core::resolve`] in-process instead, because a `php` that needs a running daemon to
//! start is a `php` that stops working when the daemon does. What the method is for is every client
//! that is already talking to one — `mix`, the GUI panel — and the answer is the same either way,
//! which is the whole reason the order lives in `core` rather than here.
//!
//! # An install that is already running is answered with the job that is running it
//!
//! Rather than started twice or refused. Two `runtime.install` calls for one version is what two
//! terminals or a double-clicked button produce, and the second is asking for the same outcome —
//! but the two would share a `.part` file in `cache/downloads/`, named after the artifact's hash and
//! appended to by both, which is a download that can only fail its checksum. So the second call is
//! handed the first call's [`JobSummary`], which is what it would have been handed if it had asked a
//! moment earlier.
//!
//! [`JobHandle`]: crate::jobs::JobHandle
//! [`JobSummary`]: mixengine_proto::JobSummary

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mixengine_core::index::{self, Index, Package, Selection, Target};
use mixengine_core::install::Installer;
use mixengine_core::upgrade::kept::{self, Family};
use mixengine_core::{Paths, Store, resolve, runtimes};
use mixengine_proto::{
    CatalogueGap, Error, ErrorCode, Execution, JobId, PackageVersion, Requirements,
    ResolvedRuntime, RuntimeCatalogue, RuntimeFilter, RuntimeKind, RuntimeList, RuntimeQuestion,
    RuntimeRelease, RuntimeSummary, RuntimeTarget, RuntimeUpdate, VersionConstraint,
};

use crate::error::ToWire as _;
use crate::jobs::Jobs;
use crate::requirements;

mod install;
mod uninstall;

/// Where the package index is read from, and which key it has to be signed by.
///
/// **Both, or neither.** A team hosting its own mirror cannot sign with our private key, so an index
/// URL that could be pointed elsewhere while the key stayed compiled in would be a setting that can
/// only ever fail — which is why `docs/operations/runtime-packaging.md` promises the pair and not
/// the URL alone.
///
/// Overriding them is trusting a different publisher, and that is a decision only somebody who
/// already controls how this daemon starts can make: the values arrive as arguments to `mixengined`,
/// from the command line or its environment, and nothing below `main` reads either.
#[derive(Debug, Clone)]
pub(crate) struct IndexSource {
    /// The document's URL.
    pub(crate) url: String,

    /// The base64 minisign public key every fetch is verified against.
    pub(crate) public_key: String,
}

impl IndexSource {
    /// Where the *extension registry* is, given where the package index is.
    ///
    /// **Derived rather than a second setting.** The two documents are published side by side under
    /// one tag and verified with one key, so a mirror that serves the index serves this beside it;
    /// a `--registry-url` of its own would be a second thing to point somewhere and a second way to
    /// point only half of a mirror. The key is the same value for the same reason it is the same
    /// key: an extension has the package index's blast radius exactly.
    ///
    /// The last path segment is replaced, so `https://mirror/mixengine/index.json` becomes
    /// `https://mirror/mixengine/extensions.json`; a URL with nothing to replace gets the name
    /// appended.
    pub(crate) fn registry_url(&self) -> String {
        match self.url.rsplit_once('/') {
            Some((base, _)) => {
                format!("{base}/{}", mixengine_core::extensions::registry::FILE_NAME)
            }
            None => format!(
                "{}/{}",
                self.url,
                mixengine_core::extensions::registry::FILE_NAME
            ),
        }
    }
}

impl Default for IndexSource {
    /// What MixEngine publishes, verified by the key compiled into this binary.
    fn default() -> Self {
        Self {
            url: index::DEFAULT_URL.to_owned(),
            public_key: index::PUBLIC_KEY.to_owned(),
        }
    }
}

/// The index and the download pipeline, shared by everything that installs anything.
///
/// **One per daemon, and not one per namespace.** `runtime.*` and `package.*` both read the same
/// signed index and both write into the same `cache/`, so two clients would be two processes'
/// worth of refresh racing over one set of cached files and two installers sharing one
/// `downloads/`. The
/// pair is built once, where the public key is checked, and handed to both.
#[derive(Debug)]
pub(crate) struct Fetcher {
    /// The verified package index, cached under `cache/`.
    pub(crate) index: index::PackageIndex,

    /// The download pipeline, with its partial downloads in the same place.
    pub(crate) installer: Installer,
}

impl Fetcher {
    /// Point an index client and an installer at `source`, caching under the home's `cache/`.
    ///
    /// `http` is the daemon's own transport, shared with the extension registry client and the
    /// update feed client rather than built fresh here — the same "one per daemon" rule this type's
    /// own doc comment already states, one layer down.
    ///
    /// # Errors
    ///
    /// The wire error of a public key that is not one, which means a broken build or an unusable
    /// `--index-key` and fails the daemon's start rather than the first call: a daemon that cannot
    /// install anything should say so while somebody is looking at it.
    pub(crate) fn new(
        paths: &Paths,
        source: &IndexSource,
        http: reqwest::Client,
    ) -> Result<Arc<Self>, Error> {
        Ok(Arc::new(Self {
            index: index::PackageIndex::with_transport(
                &source.url,
                &source.public_key,
                paths.cache(),
                http,
            )
            .map_err(|error| error.to_wire())?,
            installer: Installer::new(paths.cache()).map_err(|error| error.to_wire())?,
        }))
    }
}

/// Everything `runtime.*` needs, and the only thing that starts an install.
#[derive(Debug)]
pub(crate) struct Runtimes {
    /// Where a runtime lands.
    paths: Paths,

    /// Where the row goes.
    store: Store,

    /// What turns the work into a job, and the only thing that can end one.
    jobs: Arc<Jobs>,

    /// The index that offers versions, and the pipeline that downloads them.
    fetcher: Arc<Fetcher>,

    /// What a pool created by an install is registered with, so a request can start it.
    ///
    /// Held for one call: `activate::hold_all` needs it, because an activator that accepts a
    /// connection has to be able to make the service run.
    services: Arc<crate::services::Registry>,

    /// The installs this daemon is running, by what they are installing.
    ///
    /// A `tokio` mutex rather than a `std` one because it is held across the `await` that starts the
    /// job — which is the whole point: the check for "is this already running" and the row that
    /// makes it so have to be one decision, or two callers arriving together both find nothing and
    /// both start.
    running: tokio::sync::Mutex<BTreeMap<(RuntimeKind, PackageVersion), JobId>>,

    /// `bin/`, refreshed as a runtime comes or goes. Set once, after both are built; a test that
    /// builds no `Shims` leaves it unset and nothing is refreshed.
    shims: std::sync::OnceLock<Arc<crate::shims::Shims>>,
}

impl Runtimes {
    /// Point the runtime methods at an index and the pipeline that downloads from it.
    pub(crate) fn new(
        paths: &Paths,
        store: &Store,
        jobs: Arc<Jobs>,
        fetcher: Arc<Fetcher>,
        services: Arc<crate::services::Registry>,
    ) -> Arc<Self> {
        Arc::new(Self {
            paths: paths.clone(),
            store: store.clone(),
            jobs,
            fetcher,
            services,
            running: tokio::sync::Mutex::new(BTreeMap::new()),
            shims: std::sync::OnceLock::new(),
        })
    }

    /// Keep `bin/` in step with what is installed. Called once in `main`.
    pub(crate) fn keeps_bin(&self, shims: Arc<crate::shims::Shims>) {
        let _ = self.shims.set(shims);
    }

    /// Refresh `bin/` now: a runtime's names are there only while one of its versions is.
    /// Reported and never fatal — the next start, and the rescan, fill it again.
    async fn refresh_bin(&self) {
        if let Some(shims) = self.shims.get()
            && let Err(error) = shims.runtimes_changed().await
        {
            tracing::warn!(%error, "bin/ could not be refreshed after a runtime came or went");
        }
    }

    /// The install job running for this version, if there is one — `runtime.upgrade` refuses to
    /// race it.
    pub(crate) async fn installing(
        &self,
        kind: RuntimeKind,
        version: &PackageVersion,
    ) -> Option<JobId> {
        self.running
            .lock()
            .await
            .get(&(kind, version.clone()))
            .copied()
    }

    /// The index and the download pipeline this daemon installs through — for the blueprint
    /// executor, which judges releases it resolved through this same index.
    pub(crate) fn fetcher(&self) -> &Arc<Fetcher> {
        &self.fetcher
    }

    /// `runtime.list_installed` — what is on this machine.
    ///
    /// # Errors
    ///
    /// The wire error of a table that could not be read.
    pub(crate) async fn list_installed(
        &self,
        filter: &RuntimeFilter,
    ) -> Result<RuntimeList, Error> {
        let runtimes = runtimes::records(&self.store, filter.kind)
            .await
            .map_err(|error| error.to_wire())?;

        Ok(RuntimeList { runtimes })
    }

    /// `runtime.list_available` — what the index offers for this machine, and what is already here.
    ///
    /// **Composed here rather than by the client**, which is the rule in `CLAUDE.md`: whether a
    /// listed version is installed is a fact about two lists, and leaving a client to cross-reference
    /// them would be two clients able to disagree about what "installed" means.
    ///
    /// `filter.refresh` reaches the network even if the cache is still fresh — see
    /// [`RuntimeFilter::refresh`](mixengine_proto::RuntimeFilter::refresh).
    ///
    /// # Errors
    ///
    /// The wire error of an index that could not be obtained *at all* — a fetch that fails while a
    /// cached index exists is answered from the cache, with [`RuntimeCatalogue::stale`] set.
    pub(crate) async fn list_available(
        &self,
        filter: &RuntimeFilter,
    ) -> Result<RuntimeCatalogue, Error> {
        let wanted: &[RuntimeKind] = match &filter.kind {
            Some(kind) => std::slice::from_ref(kind),
            None => &RuntimeKind::ALL,
        };
        let names: Vec<&str> = wanted.iter().map(|kind| kind.as_str()).collect();

        let catalogue = match filter.refresh {
            true => self.fetcher.index.refresh(&names).await,
            false => self.fetcher.index.kinds(&names).await,
        }
        .map_err(|error| error.to_wire())?;
        let installed = runtimes::records(&self.store, filter.kind)
            .await
            .map_err(|error| error.to_wire())?;
        // **A version whose folder is gone is not installed here**: it is offered again, which is
        // how it is put back, and it is the base of no update.
        let installed: Vec<_> = installed
            .into_iter()
            .filter(|have| !have.is_missing())
            .collect();
        let kept = kept::all(&self.store)
            .await
            .map_err(|error| error.to_wire())?;
        let facts = requirements::facts();

        let mut runtimes = Vec::new();
        let mut updates = Vec::new();

        for kind in wanted.iter().copied() {
            let name = kind.as_str();
            let mut offered = Vec::new();

            for package in catalogue.index.installable(name) {
                // An index that offers a version this build could not make a directory for is one
                // whose entry is skipped rather than one that fails the listing: the other versions
                // are still installable, and the alternative is a home that can list nothing because
                // of one malformed row in a document nobody here controls.
                let Ok(version) = PackageVersion::parse(package.version.clone()) else {
                    tracing::warn!(
                        kind = name,
                        version = package.version,
                        "the package index offers a version this build cannot use as a directory \
                         name; skipping it"
                    );
                    continue;
                };

                let chosen = catalogue.index.artifact(name, &package.version);

                runtimes.push(RuntimeRelease {
                    installed: installed
                        .iter()
                        .any(|have| have.kind == kind && have.version == version),
                    kind,
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
                    // **What this cell cannot do**, read off the artifact this machine would take.
                    lacks: chosen
                        .map(|chosen| chosen.artifact.lacks.clone())
                        .unwrap_or_default(),
                });
                offered.push(version);
            }

            // **One release stands for each line.** Marked after the whole kind is read, because
            // "newest" is a fact about the set and not about one row.
            let newest = mixengine_core::lines::newest_of_each_line(name, offered.iter());
            for release in runtimes.iter_mut().filter(|release| release.kind == kind) {
                release.newest_in_line = Some(newest.values().any(|n| *n == release.version));
            }

            for have in installed.iter().filter(|have| have.kind == kind) {
                let Some(to) =
                    mixengine_core::lines::update_for(name, &have.version, offered.iter())
                else {
                    continue;
                };

                let to_installed = installed
                    .iter()
                    .any(|other| other.kind == kind && other.version == to);
                // Already run, and it kept `have` beside `to`: asking again would only repeat it.
                if kept::already_kept(
                    &kept,
                    Family::Runtime,
                    name,
                    have.version.as_str(),
                    to.as_str(),
                    to_installed,
                ) {
                    continue;
                }

                updates.push(RuntimeUpdate {
                    kind,
                    to_installed,
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

        Ok(RuntimeCatalogue {
            runtimes,
            stale: catalogue.freshness.is_stale(),
            updates: Some(updates),
            unavailable: Some(
                catalogue
                    .index
                    .missing()
                    .iter()
                    .map(|missing| CatalogueGap {
                        name: missing.kind.clone(),
                        reason: missing.reason.clone(),
                    })
                    .collect(),
            ),
        })
    }

    /// The newest release the index publishes for this machine that satisfies `wanted`.
    ///
    /// **A constraint becomes a release here, and only here.** A plan holds a *constraint* on
    /// purpose — it reads this home's tables and never the index — and turning one into a release
    /// is a question only the index can answer. An apply asks it before it writes anything, so that
    /// a constraint nothing satisfies fails while there is still nothing to take back.
    ///
    /// # Errors
    ///
    /// `not_found` when the index publishes nothing for this machine that satisfies the constraint,
    /// and the wire error of an index that could not be obtained at all.
    pub(crate) async fn newest_satisfying(
        &self,
        kind: RuntimeKind,
        wanted: &VersionConstraint,
    ) -> Result<PackageVersion, Error> {
        let catalogue = self
            .fetcher
            .index
            .kinds(&[kind.as_str()])
            .await
            .map_err(|error| error.to_wire())?;

        catalogue
            .index
            .installable(kind.as_str())
            .filter_map(|package| PackageVersion::parse(package.version.clone()).ok())
            .filter(|version| wanted.matches(version))
            .max_by(PackageVersion::cmp_precedence)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::NotFound,
                    format!(
                        "the package index publishes no {} for this machine that satisfies {}",
                        kind.as_str(),
                        wanted.as_str()
                    ),
                )
                .with_hint(format!(
                    "`mix runtime available --kind {}` lists what it does publish",
                    kind.as_str()
                ))
            })
    }

    /// `runtime.requirements` — what one version lacks on this machine.
    ///
    /// # Errors
    ///
    /// The refusals [`offered`] gives for a version that is not published here, and the wire error
    /// of an index that could not be obtained at all.
    pub(crate) async fn requirements(&self, target: &RuntimeTarget) -> Result<Requirements, Error> {
        let catalogue = self
            .fetcher
            .index
            .kinds(&[target.kind.as_str()])
            .await
            .map_err(|error| error.to_wire())?;
        offered(
            &catalogue.index,
            target.kind.as_str(),
            target.version.as_str(),
            &format!("mix runtime available --kind {}", target.kind.as_str()),
        )?;

        Ok(Requirements {
            unmet: requirements::of(
                &catalogue.index,
                target.kind.as_str(),
                target.version.as_str(),
                &requirements::facts(),
            ),
        })
    }

    /// `runtime.set_default` — make one installed version the one its kind resolves to.
    ///
    /// # Errors
    ///
    /// `not_found` when that version is not installed, and the wire error of a row that could not be
    /// written.
    pub(crate) async fn set_default(
        &self,
        target: &RuntimeTarget,
    ) -> Result<RuntimeSummary, Error> {
        runtimes::set_default(&self.store, target.kind, &target.version)
            .await
            .map_err(|error| error.to_wire())
    }

    /// `runtime.found` — every runtime directory on disk with no row, and why. Offline: no index,
    /// and nothing recorded.
    ///
    /// # Errors
    ///
    /// The wire error of a table that could not be read.
    pub(crate) async fn found(&self) -> Result<mixengine_proto::RuntimeFoundList, Error> {
        let found = mixengine_core::adopt::walk::unrecorded(&self.store, &self.paths)
            .await
            .map_err(|error| error.to_wire())?;

        Ok(mixengine_proto::RuntimeFoundList {
            found: found
                .iter()
                .filter_map(|one| match &one.subject {
                    mixengine_core::adopt::Subject::Runtime { kind, version } => {
                        Some(mixengine_proto::RuntimeFound {
                            kind: *kind,
                            version: version.clone(),
                            path: one.path.display().to_string(),
                            why: mixengine_core::adopt::walk::examine(one),
                        })
                    }
                    mixengine_core::adopt::Subject::Package { .. } => None,
                })
                .collect(),
        })
    }

    /// `runtime.adopt` — record a version that is on disk without a row.
    ///
    /// A version already recorded answers its row, so asking twice is not an error.
    ///
    /// # Errors
    ///
    /// As [`crate::adopt::by_hand`].
    pub(crate) async fn adopt(&self, target: &RuntimeTarget) -> Result<RuntimeSummary, Error> {
        match runtimes::record(&self.store, target.kind, &target.version).await {
            Ok(summary) => return Ok(summary),
            Err(mixengine_core::Error::NotFound { .. }) => {}
            Err(error) => return Err(error.to_wire()),
        }

        let found = mixengine_core::adopt::walk::Found {
            subject: mixengine_core::adopt::Subject::Runtime {
                kind: target.kind,
                version: target.version.clone(),
            },
            path: runtimes::directory(&self.paths, target.kind, &target.version),
        };

        match crate::adopt::by_hand(&self.store, &found, &self.fetcher.index).await? {
            mixengine_core::adopt::walk::Claimed::Runtime(summary) => {
                self.after_recorded(target.kind, &target.version, summary.clone())
                    .await?;
                Ok(summary)
            }
            mixengine_core::adopt::walk::Claimed::Package(_) => Err(Error::new(
                ErrorCode::Internal,
                "a runtime directory was recorded as a package",
            )),
        }
    }

    /// `runtime.resolve` — which installed version this directory uses, and why that one.
    ///
    /// **Every step of the order happens here** ([`mixengine_core::resolve`]), including the two
    /// that read the filesystem: a client that walked for its own `mixengine.toml` would be a client
    /// deciding something, and two of them walking differently is exactly the disagreement this
    /// method exists to make impossible. What a caller supplies is the pair the daemon cannot know —
    /// the directory the user is in, and what their flag or `MIXENGINE_PHP` said.
    ///
    /// # Errors
    ///
    /// `dependency_missing` when nothing installed satisfies the question, with the command that
    /// would fix it in the hint; `invalid_argument` for a relative directory or a `mixengine.toml`
    /// that does not parse; and the wire error of a table that could not be read.
    pub(crate) async fn resolve(
        &self,
        question: &RuntimeQuestion,
    ) -> Result<ResolvedRuntime, Error> {
        let cwd = question.cwd.as_deref().map(Path::new);

        resolve::runtime(
            &self.store,
            &resolve::Question {
                kind: question.kind,
                cwd,
                explicit: question.version.as_ref(),
            },
        )
        .await
        .map_err(|error| error.to_wire())
    }
}

/// Who a refusal about `target` is about.
pub(crate) fn subject_of(target: &RuntimeTarget) -> requirements::Subject {
    requirements::Subject {
        name: format!("{} {}", target.kind.as_str(), target.version.as_str()),
        install: Some(format!("mix runtime install {}", target.kind.as_str())),
    }
}

/// The package and the artifact the index offers for this kind and version, or the reason it
/// offers neither.
///
/// `listing` is the command whoever reads the message should run next, which differs by namespace:
/// a runtime is listed by `mix runtime available` and a service package by `mix package available`.
///
/// **Three disappointments, told apart**, where [`Index::artifact`] deliberately answers [`None`] to
/// all three: a kind the index has nothing for, a version it does not publish, and a version it
/// publishes for other systems only. They send whoever reads the message to three different places —
/// a typo, a version list, and the fact that upstream ships no build for this machine — and the last
/// one is the one that would otherwise look like a bug in MixEngine.
pub(crate) fn offered<'a>(
    index: &'a Index,
    kind: &str,
    version: &str,
    listing: &str,
) -> Result<(&'a Package, Selection<'a>), Error> {
    // **Asked before "does not publish".** A kind whose file could not be read and a kind that
    // publishes nothing look the same from here on, and they send whoever reads the message to two
    // different places.
    if let Some(missing) = index.unread(kind) {
        return Err(Error::new(
            ErrorCode::Io,
            format!(
                "the package index could not be read for {kind}: {}",
                missing.reason
            ),
        )
        .with_hint(format!("`{listing} --refresh` tries again")));
    }

    let Some(package) = index
        .packages()
        .find(|package| package.kind == kind && package.version == version)
    else {
        return Err(Error::new(
            ErrorCode::NotFound,
            format!("the package index does not publish {kind} {version}"),
        )
        .with_hint(format!("`{listing}` lists every version it does publish")));
    };

    // Read off the target triple this daemon was compiled for rather than asked of the running
    // machine, which is also the answer the caller wants: an x86_64 build running under emulation
    // should install x86_64 artifacts, because that is what it can execute. The reverse holds too
    // and is `Target::runnable`'s business — an ARM64 Windows daemon may install an x86_64
    // artifact, because that machine can execute one and upstream builds it nothing else.
    let Some(target) = Target::host() else {
        return Err(Error::new(
            ErrorCode::UnsupportedPlatform,
            format!(
                "this build of MixEngine runs on a system the package index has no vocabulary \
                 for ({} {})",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        ));
    };

    package
        .select(target)
        .map(|selection| (package, selection))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::UnsupportedPlatform,
                format!("{kind} {version} is not published for this machine"),
            )
            .with_hint(
                "upstream does not build every version for every system; \
                 `mix runtime available` only lists what this one can run",
            )
        })
}

/// What an install says before it downloads a build this machine cannot run natively.
///
/// **A progress line rather than a column in a table**, because it is a fact about *this* install at
/// the moment it happens; the listing marks the same versions before anybody commits to one. The
/// alternative — remembering it in `runtime_installs` — is a migration and a response member for
/// something re-derivable from the index at any time.
///
/// [`None`] for the native case, so a caller writes no branch of its own. See
/// [ADR 0023](../../../docs/decisions/0023-an-arm64-windows-machine-runs-the-x86_64-build.md),
/// whose whole rule is that this is automatic and never silent.
pub(crate) fn emulation_notice(
    what: &str,
    version: &str,
    selection: Selection<'_>,
) -> Option<String> {
    (selection.execution == Execution::Emulated).then(|| {
        format!(
            "no {} build of {what} {version} is published, so the {} one is being installed and \
             this system will run it under emulation",
            std::env::consts::ARCH,
            selection.artifact.arch.as_str(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One package, published for one target — enough to hand [`emulation_notice`] a selection.
    fn published_for(os: index::Os, arch: index::Arch) -> Package {
        serde_json::from_value(serde_json::json!({
            "kind": "php", "version": "8.3.33", "channel": "stable",
            "artifacts": [{
                "os": os.as_str(), "arch": arch.as_str(),
                "url": "https://example.invalid/php.zip",
                "sha256": "00", "size": 1,
                "provides": { "php": "php.exe" }
            }]
        }))
        .expect("the published shape parses")
    }

    /// The notice is the whole of what an emulated install owes a person.
    ///
    /// It is composed here rather than asserted through a job, because what a job carries is a
    /// string and this is the only place that decides what the string says.
    #[test]
    fn an_emulated_install_says_what_it_is_about_to_do() {
        let package = published_for(index::Os::Windows, index::Arch::X86_64);

        let native = package
            .select(Target::new(index::Os::Windows, index::Arch::X86_64))
            .expect("its own build");
        assert_eq!(
            emulation_notice("php", "8.3.33", native),
            None,
            "a native install has nothing to explain"
        );

        let emulated = package
            .select(Target::new(index::Os::Windows, index::Arch::Aarch64))
            .expect("the x86_64 build is what that machine can run");
        let said = emulation_notice("php", "8.3.33", emulated).expect("an emulated one does");

        assert!(
            said.contains("php 8.3.33"),
            "it names what is being installed: {said}"
        );
        assert!(said.contains("x86_64"), "and which build: {said}");
        assert!(
            said.contains("emulation"),
            "and that this system will emulate it: {said}"
        );
    }

    /// **A mirror is one setting, not two.** The registry sits beside the index it was published
    /// with, so pointing at a mirror points at both.
    #[test]
    fn the_registry_is_the_index_url_with_the_other_document_s_name() {
        let source = IndexSource {
            url: "https://mirror.example/mixengine/index.json".to_owned(),
            public_key: "irrelevant".to_owned(),
        };

        assert_eq!(
            source.registry_url(),
            "https://mirror.example/mixengine/extensions.json"
        );
    }

    /// And what MixEngine publishes is what the compiled-in constant says, rather than something
    /// derived that happens to agree with it today.
    #[test]
    fn the_default_pair_is_what_this_build_publishes() {
        assert_eq!(
            IndexSource::default().registry_url(),
            mixengine_core::extensions::registry::DEFAULT_URL
        );
    }
}
