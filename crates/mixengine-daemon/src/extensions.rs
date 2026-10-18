//! `extension.*` — roadmap tasks **T80** and **T81**.
//!
//! **Not [`crate::php_extensions`]**, which turns a *PHP* extension on for one installed runtime.
//! These are MixEngine's own: Mailpit, phpMyAdmin, Adminer.
//!
//! A façade and nothing more — every decision here is `mixengine_core::extensions`', because
//! `CLAUDE.md` puts no business logic in a client and the daemon is the client's server rather than
//! a second place for rules. What belongs *here* is what only the daemon has: the home's paths, its
//! store, the registry client and the job runner.
//!
//! **Consent is checked here rather than trusted.** A client sends the plan it showed somebody
//! ([`ExtensionConsent`]), and this compares it against the manifest it is about to install — the
//! shape `[scaffold]` consent already has, and for its reason: the registry can be refreshed between
//! the reading and the sending, and a consent naming what was shown is the only kind that cannot be
//! spent on something else.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mixengine_core::extensions::manifest::ExtensionManifest;
use mixengine_core::extensions::registry::Registry;
use mixengine_core::extensions::store::Source;
use mixengine_core::extensions::{install, manifest, store as extension_store, uninstall};
use mixengine_core::index::Client;
use mixengine_core::{Paths, Store};
use mixengine_proto::{
    Error, ErrorCode, ExtensionAvailable, ExtensionCatalogue, ExtensionConsent, ExtensionId,
    ExtensionInspect, ExtensionInspection, ExtensionInstall, ExtensionOffer, ExtensionOrigin,
    ExtensionPlan, ExtensionPlanRequest, ExtensionRemoval, ExtensionSummary, ExtensionUninstall,
    JobKind, JobSummary, PortWish, ServiceId, Timestamp, rpc,
};

use crate::error::ToWire as _;

mod configure;

/// Where an extension's own credentials live inside the keyring's `mixengine` namespace.
///
/// `<home-id>/extensions/<id>/…`, beside the `<home-id>/<service-id>/<key>` a recipe's credential
/// takes (`generate::recipe::Context::secret_address`) — one namespace, two shapes that cannot
/// collide because a service id has no `/` in it.
///
/// **The home in front is roadmap task T126**, and it is the same sentence a service's address
/// carries: the credential store is one per user, `MIXENGINE_HOME` means a user may have several
/// homes, and two homes that each installed phpMyAdmin were signing their session cookies with one
/// shared value.
const EXTENSION_SECRET_PREFIX: &str = "extensions/";

/// What `{secret}` is stored under, for the one extension-owned secret there is.
const CONFIG_SECRET_KEY: &str = "config";

/// How many characters it has — [`generate::databases::SECRET_LENGTH`]'s number, for its reason.
///
/// [`generate::databases::SECRET_LENGTH`]: mixengine_core::generate::databases::SECRET_LENGTH
const CONFIG_SECRET_LENGTH: usize = 32;

/// Everything `extension.*` needs.
#[derive(Debug)]
pub(crate) struct Extensions {
    /// The home, for the directories an install uses.
    paths: Paths,

    /// The rows.
    store: Store,

    /// The signed registry, cached under the home's cache directory.
    registry: Client<Registry>,

    /// What runs an install, which is long enough to be a job.
    jobs: Arc<crate::jobs::Jobs>,

    /// This system, for the port allocator's bind probe.
    host: Arc<dyn mixengine_platform::Host>,

    /// The sites, for what a `web-app` install does after its row — roadmap task **T81b**, the
    /// design's D7. Held rather than reached for, the reason `Domains` holds it: every path that
    /// writes a site has to ask for the hosts file, issue the certificate and regenerate, and a
    /// mechanism the caller has to remember is one the caller eventually forgets.
    sites: Arc<crate::sites::Sites>,

    /// The supervisor, for the one thing a `web-app`'s uninstall now has to do first — roadmap task
    /// **T82a**, its design's D11.
    ///
    /// `mixengine_core::extensions::uninstall`'s note says supervision belongs to the daemon and the
    /// order it walks is stop-then-this; until that task a `web-app` had no process to stop.
    services: Arc<crate::services::Registry>,
}

impl Extensions {
    /// Build it.
    ///
    /// **The registry client is built by the caller** — `main.rs`, beside the `Fetcher`, so that a
    /// compiled-in key that is not a key fails the start rather than the first install — and this
    /// is built after `Sites`, which it holds (roadmap task **T81b**).
    pub(crate) fn new(
        paths: Paths,
        store: Store,
        jobs: Arc<crate::jobs::Jobs>,
        host: Arc<dyn mixengine_platform::Host>,
        registry: Client<Registry>,
        sites: Arc<crate::sites::Sites>,
        services: Arc<crate::services::Registry>,
    ) -> Self {
        Self {
            paths,
            store,
            registry,
            jobs,
            host,
            sites,
            services,
        }
    }

    /// Read a manifest and say what installing it here would produce.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for a path that is not absolute, and whatever
    /// [`mixengine_core::extensions::inspect`] raises about the file itself.
    pub(crate) fn inspect(&self, asked: &ExtensionInspect) -> Result<ExtensionInspection, Error> {
        let path = absolute(&asked.path)?;

        mixengine_core::extensions::inspect(&self.paths, &path).map_err(|error| error.to_wire())
    }

    /// What this home has installed.
    ///
    /// # Errors
    ///
    /// Whatever reading the tables costs.
    pub(crate) async fn list(&self) -> Result<mixengine_proto::InstalledExtensions, Error> {
        let installed = extension_store::all(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        // One read for the whole listing: the domain of every extension-owned site, by extension —
        // roadmap task **T81b**.
        let served: std::collections::BTreeMap<ExtensionId, String> =
            mixengine_core::sites::records(&self.store, None)
                .await
                .map_err(|error| error.to_wire())?
                .into_iter()
                .filter_map(|site| match site.owner {
                    mixengine_core::sites::SiteOwner::Extension(id) => {
                        Some((id, site.domains.first().cloned().unwrap_or_default()))
                    }
                    mixengine_core::sites::SiteOwner::Project(_) => None,
                })
                .collect();

        Ok(mixengine_proto::InstalledExtensions {
            extensions: installed
                .iter()
                .map(|one| summary(one, served.get(&one.id).cloned()))
                .collect(),
        })
    }

    /// What the signed registry publishes.
    ///
    /// **`asked.refresh` skips a still-fresh cache** — `RuntimeFilter::refresh`'s reason: someone
    /// who just watched an extension get published should not have to wait out the registry
    /// client's own freshness window before this home notices.
    ///
    /// # Errors
    ///
    /// Whatever obtaining the registry costs when there is no usable cache either — a signature
    /// that does not verify, a document from before the cached one, a server that cannot be
    /// reached.
    pub(crate) async fn available(
        &self,
        asked: &ExtensionAvailable,
    ) -> Result<ExtensionCatalogue, Error> {
        let catalogue = match asked.refresh {
            true => self.registry.refresh().await,
            false => self.registry.catalogue().await,
        }
        .map_err(|error| error.to_wire())?;
        let listing = catalogue.index.listing();

        let installed = extension_store::all(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        let extensions = listing
            .extensions
            .iter()
            .map(|manifest| ExtensionOffer {
                id: manifest.extension.id.clone(),
                name: manifest.extension.name.clone(),
                version: manifest.extension.version.clone(),
                kind: manifest.extension.kind,
                description: manifest.extension.description.clone(),
                installed: installed.iter().any(|one| one.id == manifest.extension.id),
                artifact: mixengine_core::extensions::availability(manifest),
            })
            .collect();

        Ok(ExtensionCatalogue {
            extensions,
            unreadable: listing.unreadable,
            stale: catalogue.freshness.is_stale(),
        })
    }

    /// What installing something would do here.
    ///
    /// # Errors
    ///
    /// Whatever reading the manifest reports, and [`ErrorCode::NotFound`] for an id the registry
    /// does not list.
    pub(crate) async fn plan(&self, asked: &ExtensionPlanRequest) -> Result<ExtensionPlan, Error> {
        let (manifest, signed) = self.manifest(&asked.source).await?;

        let plan = install::plan(&self.store, &self.paths, &manifest, signed)
            .await
            .map_err(|error| error.to_wire())?;

        Ok(ExtensionPlan {
            id: plan.id,
            name: plan.name,
            version: plan.version,
            kind: plan.kind,
            description: plan.description,
            homepage: manifest.extension.homepage.clone(),
            signed: plan.signed,
            permissions: plan.permissions,
            ports: plan.ports,
            install_dir: plan.install_dir.display().to_string(),
            data_dir: plan.data_dir.display().to_string(),
            site: plan.site.map(|site| mixengine_proto::PlannedSite {
                domain: site.domain,
                pool: site.pool,
                database: site.database,
                signs_in: site.signs_in,
            }),
        })
    }

    /// Install one, as a job.
    ///
    /// # Errors
    ///
    /// Everything [`plan`](Self::plan) reports, [`ErrorCode::PreconditionFailed`] when the consent
    /// does not describe what is about to be installed, and whatever starting a job costs.
    pub(crate) async fn install(
        self: &Arc<Self>,
        asked: &ExtensionInstall,
    ) -> Result<JobSummary, Error> {
        let (manifest, signed) = self.manifest(&asked.source).await?;

        agrees(&asked.consent, &manifest, signed)?;

        // Refused before the job exists, so a client that asked for something impossible is told
        // now rather than handed a job id that fails a second later.
        install::plan(&self.store, &self.paths, &manifest, signed)
            .await
            .map_err(|error| error.to_wire())?;

        let kind = JobKind::parse(rpc::method::EXTENSION_INSTALL)
            .expect("`extension.install` is a method name, which is what a job kind is");

        let extensions = Arc::clone(self);
        let source = asked.source.clone();

        self.jobs
            .begin(&kind, move |handle| async move {
                extensions
                    .perform(&source, &manifest, signed, &handle)
                    .await
            })
            .await
    }

    /// The work behind an install.
    async fn perform(
        &self,
        source: &ExtensionOrigin,
        manifest: &ExtensionManifest,
        signed: bool,
        handle: &crate::jobs::JobHandle,
    ) -> Result<serde_json::Value, Error> {
        let id = manifest.extension.id.as_str();
        tracing::info!(job = %handle.id(), extension = %id, "installing an extension");

        handle.progress(0, "reading the manifest").await;

        let from = match source {
            ExtensionOrigin::Path { path } => Some(absolute(path)?),
            ExtensionOrigin::Registry { .. } => None,
        };

        let source = match signed {
            true => Source::Registry,
            false => Source::Path,
        };
        let at = Timestamp::from_system_time(std::time::SystemTime::now());

        // **The front end judges the fragment before a byte is fetched** — roadmap task **T81c**,
        // the design's D5. A `[[recipe.front_end]]` this home's front end will not parse is refused
        // here, where nothing has been downloaded, unpacked or written; running it afterwards would
        // leave the choice between a wedged front end and an uninstall in an error path.
        self.would_be_served(manifest, source, at).await?;

        let installed = install::install(
            &self.store,
            &self.paths,
            self.host.as_ref(),
            install::Request {
                manifest,
                source,
                from: from.as_deref(),
                at,
            },
            handle,
        )
        .await
        .map_err(|error| error.to_wire())?;

        // **What `site.create` does after its row**, for the site the install just wrote — roadmap
        // task **T81b**, the design's D7. A `service` needs none of this: `extension.start` walks
        // `service.start` and regenerates on the way. A site has nothing to walk.
        let site = mixengine_core::sites::of_extension(&self.store, &installed.id)
            .await
            .map_err(|error| error.to_wire())?;

        if let Some(site) = &site {
            // **Before the site is declared**, so the first file the front end reads already names
            // the activator after the pool — T200, D1.
            handle.progress(86, "making its pool reachable").await;
            self.make_pool_reachable().await;

            handle.progress(88, "writing its configuration").await;
            self.configure_one(&installed).await?;

            handle.progress(90, "declaring the site").await;
            self.sites.now_declares(site).await?;
        } else if carries_a_fragment(manifest) {
            // **A `recipe` extension has neither a service to start nor a site to declare, and
            // still changed what the front end reads** — roadmap task **T81c**. Nothing else on
            // this path would regenerate, so the fragment would sit in the table until the next
            // thing that happened to render.
            handle.progress(90, "regenerating the front end").await;
            self.sites.now_serves_what_it_declares().await?;
        }

        // **And every managed PHP's `conf.d`, if this extension added to it** — roadmap task
        // **T82**, found by installing the real Mailpit.
        //
        // T81 wired `[recipe] php_ini` and left it written by `refresh_all` at boot and by a runtime
        // install, neither of which happens here — so `sendmail_path` appeared only after the daemon
        // was restarted, and the acceptance criterion says *with no manual php.ini edit*, not *after
        // a restart*. This is T81c's own lesson arriving for the other half of `[recipe]`: nothing
        // else on this path would regenerate.
        if carries_php_ini(manifest) {
            handle.progress(94, "telling every managed PHP").await;
            self.refresh_ini_sets().await;
        }

        serde_json::to_value(summary(
            &installed,
            site.and_then(|site| site.domains.first().cloned()),
        ))
        .map_err(|source| {
            Error::new(
                ErrorCode::Internal,
                format!("an installed extension could not be described: {source}"),
            )
        })
    }

    /// Would this home's front end accept what installing this would give it? — roadmap task
    /// **T81c**, the design's D5.
    ///
    /// **Here rather than in `mixengine_core::extensions::install`**, and the reason is where a
    /// [`Generator`] can be built: it needs the recipe catalogue and this system's port mapping,
    /// both of which this daemon has already assembled in [`crate::services::spec::generator`].
    /// Threading one into a core function to check one field would move that assembly into core.
    ///
    /// The `Installed` handed over is the row this install *would* write, with the ports the
    /// manifest asked for rather than the ones it will hold — which is what a judgement before the
    /// allocation can see, and is argued in [`Generator::would_serve`].
    ///
    /// # Errors
    ///
    /// [`ErrorCode::PreconditionFailed`] carrying the front end's own complaint, with the
    /// extension named: which of the two documents was refused is the first thing the reader needs.
    ///
    /// [`Generator`]: mixengine_core::generate::Generator
    /// [`Generator::would_serve`]: mixengine_core::generate::Generator::would_serve
    async fn would_be_served(
        &self,
        manifest: &ExtensionManifest,
        source: Source,
        at: Timestamp,
    ) -> Result<(), Error> {
        if !carries_a_fragment(manifest) {
            return Ok(());
        }

        let id = manifest.extension.id.clone();
        let pending = extension_store::Installed {
            install_dir: extension_store::install_dir(&self.paths, &id),
            data_dir: extension_store::data_dir(&self.paths, &id),
            ports: manifest.ports.clone(),
            manifest: manifest.clone(),
            source,
            signed: matches!(source, Source::Registry),
            installed_at: at,
            id,
        };

        crate::services::spec::generator(
            &self.paths,
            &self.store,
            self.host.as_ref(),
            self.services.welcome(),
        )
            .would_serve(&pending)
            .await
            .map_err(|error| {
                Error::new(
                    ErrorCode::PreconditionFailed,
                    format!(
                        "{} declares a front-end fragment this home's front end will not accept: {error}",
                        pending.id
                    ),
                )
                .with_hint("`mix extension inspect <path>` shows the fragment as it would be rendered")
            })
    }

    /// Remove one.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::NotFound`] when nothing is installed under that id, and whatever removing it
    /// costs — including the refusal a running service holds.
    pub(crate) async fn uninstall(
        &self,
        asked: &ExtensionUninstall,
    ) -> Result<ExtensionRemoval, Error> {
        // **Stop before removing** — roadmap task **T82a**, its design's D11, and the order
        // `mixengine_core::extensions::uninstall`'s own note states for a `service` extension's
        // process. A `web-app` is served rather than run and had nothing to stop until it was given
        // a pool of its own; a row deleted from under a live php-fpm would leave a master with no
        // configuration and nothing left that knows about it.
        self.stop_what_it_runs(&asked.id).await?;

        // **Read before the row is gone, and used after it is** — roadmap task **T81c**. Whether
        // this extension put anything in the front end's configuration is a fact about its manifest,
        // and the manifest lives in the row this is about to delete.
        let manifest = extension_store::get(&self.store, &asked.id)
            .await
            .map_err(|error| error.to_wire())?
            .map(|installed| installed.manifest);
        let had_a_fragment = manifest.as_ref().is_some_and(carries_a_fragment);
        // The same question for the other half of `[recipe]` — roadmap task **T82**. Read here for
        // the reason above: the manifest lives in the row this is about to delete.
        let had_php_ini = manifest.as_ref().is_some_and(carries_php_ini);

        // **The rows go first and the regeneration second, and that order is the escape hatch** —
        // roadmap task **T81c**, the design's D6. A fragment that was accepted at install can be
        // refused later — the front-end package is upgraded, or this home switches to the other
        // front end and its fragment was never judged — and in that state nothing regenerates. The
        // way out is this call, and it works only because by the time anything renders, the row
        // carrying the fragment is not in the table. Swapping these two lines would remove the only
        // exit from a home in that state; the suites in `tests/caddy.rs` and `tests/nginx.rs` are
        // what would notice.
        let removed = uninstall::uninstall(&self.store, &self.paths, &asked.id, asked.delete_data)
            .await
            .map_err(|error| error.to_wire())?;

        // **The activator goes with the pool** — T200, D2. After the row, so nothing can wake a
        // service that no longer exists in between.
        if let Some(pool) = &removed.pool {
            crate::services::activate::release(pool).await;
        }

        // **After the row, so a failed uninstall does not lose a secret it still needs** — roadmap
        // task **T82**, the design's D7. Idempotent, so an uninstall of something that never had
        // one costs a keyring call and no error.
        self.forget_config_secret(&asked.id).await;

        // What `site.delete` does after its row — roadmap task **T81b**, the design's D8.
        if removed.site.is_some() {
            self.sites.no_longer_declares().await?;
        } else if had_a_fragment {
            self.sites.now_serves_what_it_declares().await?;
        }

        // And the line it added to every managed PHP goes with it — roadmap task **T82**. After the
        // row, so what is rewritten is a home this extension is no longer in.
        if had_php_ini {
            self.refresh_ini_sets().await;
        }

        Ok(ExtensionRemoval {
            id: removed.id,
            service: removed.service,
            data_dir_kept: removed.data_dir_kept.map(|path| path.display().to_string()),
            site: removed.site,
            pool: removed.pool,
        })
    }

    /// The service an extension runs as.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::NotFound`] when nothing is installed under that id, and
    /// [`ErrorCode::PreconditionFailed`] for a kind that runs no process — which is an answer about
    /// the extension rather than a failure of the call.
    pub(crate) async fn service_of(&self, id: &ExtensionId) -> Result<ServiceId, Error> {
        let installed = extension_store::get(&self.store, id)
            .await
            .map_err(|error| error.to_wire())?
            .ok_or_else(|| {
                Error::new(ErrorCode::NotFound, format!("{id} is not installed"))
                    .with_hint("`mix extension list` says what is")
            })?;

        match uninstall::service_of(&installed) {
            Some(service) => Ok(service),

            // A web-app is served, not run — roadmap task **T81b**, the design's D10.
            None if installed.kind() == mixengine_proto::ExtensionKind::WebApp => {
                let domain = mixengine_core::sites::of_extension(&self.store, id)
                    .await
                    .map_err(|error| error.to_wire())?
                    .and_then(|site| site.domains.first().cloned())
                    .unwrap_or_default();

                Err(served_as_a_site(id, &domain))
            }

            None => Err(Error::new(
                ErrorCode::PreconditionFailed,
                format!(
                    "{id} is a {} extension and runs no process",
                    installed.kind()
                ),
            )),
        }
    }

    /// The manifest behind a source, and whether anything vouches for it.
    async fn manifest(&self, source: &ExtensionOrigin) -> Result<(ExtensionManifest, bool), Error> {
        match source {
            ExtensionOrigin::Registry { id } => {
                let catalogue = self
                    .registry
                    .catalogue()
                    .await
                    .map_err(|error| error.to_wire())?;

                let manifest = catalogue.index.find(id.as_str()).ok_or_else(|| {
                    Error::new(ErrorCode::NotFound, format!("{id} is not in the registry"))
                        .with_hint("`mix extension available` lists what is")
                })?;

                // **Signed because the document was.** The signature covers the whole registry, so
                // an entry either arrived inside something the compiled-in key vouched for or the
                // document was refused before this line.
                Ok((manifest, true))
            }

            ExtensionOrigin::Path { path } => {
                let directory = absolute(path)?;
                let file = match directory.is_file() {
                    true => directory,
                    false => directory.join(manifest::FILE_NAME),
                };

                let text = std::fs::read_to_string(&file).map_err(|source| {
                    Error::new(
                        ErrorCode::NotFound,
                        format!("{} could not be read: {source}", file.display()),
                    )
                })?;

                let manifest = manifest::read(&file, &text).map_err(|error| error.to_wire())?;

                Ok((manifest, false))
            }
        }
    }
}

/// Whether a consent describes the manifest that is about to be installed.
///
/// **Compared rather than believed** — [`ScaffoldConsent`](mixengine_proto::ScaffoldConsent)'s rule.
/// The registry can be refreshed between the plan a person read and the install a client sent, so
/// what is checked is that the version, the signature and the reach they were shown are still the
/// ones about to be installed. Disagreement in either direction refuses.
fn agrees(
    consent: &ExtensionConsent,
    manifest: &ExtensionManifest,
    signed: bool,
) -> Result<(), Error> {
    let refuse = |what: &str| {
        Err(Error::new(
            ErrorCode::PreconditionFailed,
            format!("this is not the {what} you were shown; read the plan again"),
        )
        .with_hint("`mix extension plan` shows what would be installed now"))
    };

    if consent.id != manifest.extension.id {
        return refuse("extension");
    }

    if consent.version != manifest.extension.version {
        return refuse("version");
    }

    if consent.signed != signed {
        return refuse("signature");
    }

    if consent.network != manifest.permissions.network {
        return refuse("network reach");
    }

    Ok(())
}

/// Whether this manifest puts anything in the front end's configuration — roadmap task **T81c**.
///
/// Asked three times on two paths — before an install is judged, after one has written its rows,
/// and before an uninstall removes them — and it is one function because the three have to agree:
/// an install that regenerated where a judgement had not looked would be a fragment reaching the
/// front end unjudged, and an uninstall that did not regenerate where the install had would leave
/// the file behind.
fn carries_a_fragment(manifest: &ExtensionManifest) -> bool {
    manifest
        .recipe
        .as_ref()
        .is_some_and(|recipe| !recipe.front_end.is_empty())
}

/// Whether this extension adds anything to a managed PHP's `conf.d` — roadmap task **T82**.
///
/// [`carries_a_fragment`]'s twin, and one function for the install and the uninstall for its reason:
/// an install that regenerated where an uninstall did not would leave the line behind.
fn carries_php_ini(manifest: &ExtensionManifest) -> bool {
    manifest
        .recipe
        .as_ref()
        .is_some_and(|recipe| !recipe.php_ini.is_empty())
}

/// Whether this extension declared a database, which is what makes an unwritten configuration worth
/// a warning rather than silence — roadmap task **T82**.
fn needs_a_database(installed: &extension_store::Installed) -> bool {
    matches!(
        &installed.manifest.body,
        manifest::Body::WebApp(app) if app.config.is_some() && app.database.is_some()
    )
}

/// A `web-app` is served, not run — roadmap task **T81b**, the design's D10.
fn served_as_a_site(id: &ExtensionId, domain: &str) -> Error {
    Error::new(
        ErrorCode::PreconditionFailed,
        format!("{id} is a web-app extension and is served as a site"),
    )
    .with_hint(format!(
        "`mix site start {domain}` and `mix site stop {domain}` control it"
    ))
}

/// One installed extension as the wire describes it, with the domain it is served on where it is.
fn summary(
    installed: &mixengine_core::extensions::store::Installed,
    site: Option<String>,
) -> ExtensionSummary {
    ExtensionSummary {
        id: installed.id.clone(),
        name: installed.name().to_owned(),
        version: installed.version().clone(),
        kind: installed.kind(),
        signed: installed.signed,
        service: uninstall::service_of(installed),
        ports: installed
            .ports
            .iter()
            .map(|(name, port)| PortWish {
                name: name.clone(),
                wanted: *port,
            })
            .collect(),
        site,
        description: Some(installed.manifest.extension.description.clone()),
        // `127.0.0.1` whatever the network reach: it is where the person's browser is (T200a, D1).
        ui: installed.manifest.ui.as_ref().and_then(|ui| {
            installed
                .ports
                .get(&ui.port)
                .map(|port| format!("http://127.0.0.1:{port}{}", ui.path))
        }),
    }
}

/// A path the daemon can act on: absolute, because this daemon has no idea what the client's
/// current directory is and a relative path here would be resolved against the wrong one — which
/// reads the wrong file rather than failing.
fn absolute(given: &str) -> Result<PathBuf, Error> {
    let path = Path::new(given);

    match path.is_absolute() {
        true => Ok(path.to_path_buf()),
        false => Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{given} is not an absolute path"),
        )
        .with_hint("the client resolves a path against its own directory before sending it")),
    }
}

#[cfg(test)]
mod tests;
