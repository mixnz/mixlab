//! Finding what a reference names: a project, a site's holder, the services it links, and one site
//! as the wire describes it.

use mixengine_core::extensions::manifest::Body;
use mixengine_core::extensions::store as extension_store;
use mixengine_core::{manifest, projects, resolve, services, sites};
use mixengine_proto::{
    Error, ErrorCode, ProjectRef, ServiceId, SiteDetail, SiteKind, SitePool, SiteServiceLink,
};

use crate::error::ToWire as _;

use super::{Holder, Sites, doc_root_full, owned_by_an_extension, summary};

impl Sites {
    /// The project a reference names, or `not_found`.
    pub(super) async fn project(
        &self,
        reference: &ProjectRef,
    ) -> Result<projects::ProjectRecord, Error> {
        projects::find(&self.store, reference)
            .await
            .map_err(|error| error.to_wire())?
            .ok_or_else(|| {
                let (said, hint) = match reference {
                    ProjectRef::Name(name) => (
                        format!("no such project: {name}"),
                        "`mix project list` shows what does exist".to_owned(),
                    ),
                    ProjectRef::Path(path) => (
                        format!("no project is registered at or above {path}"),
                        format!("`mix project create {path}` registers it"),
                    ),
                };

                Error::new(ErrorCode::NotFound, said).with_hint(hint)
            })
    }

    /// The project a row points at.
    ///
    /// `internal` when it is gone, which the cascade makes impossible and which is therefore a bug
    /// rather than a user's mistake.
    async fn project_by_id(&self, id: i64) -> Result<projects::ProjectRecord, Error> {
        projects::records(&self.store)
            .await
            .map_err(|error| error.to_wire())?
            .into_iter()
            .find(|project| project.id == id)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Internal,
                    format!("site {id} belongs to a project that is not there"),
                )
                .with_hint("deleting a project takes its sites, so this is a bug in MixEngine")
            })
    }

    /// The holder a row names — roadmap task **T81b**.
    ///
    /// `internal` when it is gone: both foreign keys cascade, so a missing owner is a bug.
    pub(super) async fn holder_of(&self, site: &sites::SiteRecord) -> Result<Holder, Error> {
        match &site.owner {
            sites::SiteOwner::Project(id) => Ok(Holder::Project(self.project_by_id(*id).await?)),

            sites::SiteOwner::Extension(id) => {
                let installed = extension_store::get(&self.store, id)
                    .await
                    .map_err(|error| error.to_wire())?
                    .ok_or_else(|| {
                        Error::new(
                            ErrorCode::Internal,
                            format!("site {} belongs to an extension that is not there", site.id),
                        )
                        .with_hint(
                            "forgetting an extension takes its site, so this is a bug in MixEngine",
                        )
                    })?;

                let Body::WebApp(app) = &installed.manifest.body else {
                    return Err(Error::new(
                        ErrorCode::Internal,
                        format!("site {} belongs to {id}, which is not a web-app", site.id),
                    ));
                };

                Ok(Holder::Extension {
                    id: id.clone(),
                    install_dir: installed.install_dir.clone(),
                    requires: app.runtime.requires.clone(),
                })
            }
        }
    }

    /// The project a site may be edited under, or the refusal for a site nobody edits by hand —
    /// roadmap task **T81b**, the design's D6.
    ///
    /// **Here and not in `mix`**: `blueprint.apply` and `domain.add` reach [`Self::update`] without
    /// going through a CLI, and a refusal they could cross is no refusal.
    pub(super) fn editable<'a>(
        &self,
        site: &sites::SiteRecord,
        holder: &'a Holder,
    ) -> Result<&'a projects::ProjectRecord, Error> {
        match holder {
            Holder::Project(project) => Ok(project),
            Holder::Extension { id, .. } => Err(owned_by_an_extension(
                site.domains.first().map_or("", String::as_str),
                id,
            )),
        }
    }

    /// Everything `site.create` does after its row, for a site written by somebody else — roadmap
    /// task **T81b**, the design's D7. The three calls, in the order and with the logging policy
    /// `create` gives each.
    ///
    /// # Errors
    ///
    /// Whatever the walk reports — a hosts want and a certificate refusal are logged, never
    /// returned.
    pub(crate) async fn now_declares(&self, site: &sites::SiteRecord) -> Result<(), Error> {
        self.wants_the_hosts_file().await;
        self.now_has_a_certificate(site).await;
        self.now_serves_what_it_declares().await
    }

    /// What `site.delete` does after its row, for a site removed by somebody else — the T81b
    /// design's D8.
    ///
    /// # Errors
    ///
    /// Whatever the walk reports.
    pub(crate) async fn no_longer_declares(&self) -> Result<(), Error> {
        self.wants_the_hosts_file().await;
        self.now_serves_what_it_declares().await
    }

    /// Every id checked to exist, answered in the order it was given.
    ///
    /// A miss is `not_found` carrying `` `mix service create <id>` declares it ``, on an
    /// uninstalled runtime's precedent: the refusal names the command that fixes it.
    pub(super) async fn existing(&self, wanted: &[ServiceId]) -> Result<Vec<ServiceId>, Error> {
        for service in wanted {
            services::record(&self.store, service).await.map_err(|_| {
                Error::new(ErrorCode::NotFound, format!("no such service: {service}"))
                    .with_hint(format!("`mix service create {service}` declares it"))
            })?;
        }

        Ok(wanted.to_vec())
    }

    /// The manifest's `[[services]]` as ids.
    ///
    /// **An absent `instance` is a lookup, not a second identity**: the bare `name` when a service
    /// by that id exists — which is what a single-instance package such as `caddy` is called — and
    /// `name@main` otherwise.
    ///
    /// A `version` nothing installed satisfies is **not** refused (spec D8): refusing an import
    /// because this machine has MariaDB 11.5 would break the clean-machine case the import path
    /// exists for. It is written to `daemon.log` at `info` and no further — there is no field on
    /// [`SiteDetail`] for it, and inventing one here would be an API this spec did not agree.
    pub(super) async fn linked(
        &self,
        manifest: &manifest::Manifest,
    ) -> Result<Vec<ServiceId>, Error> {
        let mut linked = Vec::with_capacity(manifest.services.len());

        for declared in &manifest.services {
            let id = self
                .lookup(&declared.name, declared.instance.as_deref())
                .await?;

            if let Some(wanted) = &declared.version {
                tracing::info!(
                    service = %id,
                    constraint = %wanted.as_str(),
                    "a manifest asks for a version of a service; this build records the link and \
                     nothing about the version"
                );
            }

            linked.push(id);
        }

        Ok(linked)
    }

    /// A site entry's own `services = [...]`, as ids this home has — roadmap task **T204**.
    pub(super) async fn resolved(&self, wanted: &[ServiceId]) -> Result<Vec<ServiceId>, Error> {
        let mut linked = Vec::with_capacity(wanted.len());

        for id in wanted {
            linked.push(self.lookup(id.name(), id.instance()).await?);
        }

        Ok(linked)
    }

    /// One service the manifest names, looked up the way `[[services]]` always has been: an
    /// absent instance tries the bare name, then `name@main`.
    async fn lookup(&self, name: &str, instance: Option<&str>) -> Result<ServiceId, Error> {
        let spelled = match instance {
            Some(instance) => vec![format!("{name}@{instance}")],
            None => vec![name.to_owned(), format!("{name}@main")],
        };

        for candidate in spelled {
            let Ok(id) = ServiceId::parse(candidate) else {
                continue;
            };

            if services::record(&self.store, &id).await.is_ok() {
                return Ok(id);
            }
        }

        Err(Error::new(
            ErrorCode::NotFound,
            format!("the manifest declares {name}, which is not here"),
        )
        .with_hint(format!(
            "`mix service create {name}` declares it, or drop it from mixengine.toml"
        )))
    }

    /// One record as the wire describes it: the summary, the two pool answers, and the links with
    /// each service's current state.
    pub(super) async fn detail(
        &self,
        site: &sites::SiteRecord,
        holder: &Holder,
    ) -> Result<SiteDetail, Error> {
        let full = doc_root_full(holder.root(), &site.doc_root);

        let pool = match &site.kind {
            SiteKind::PhpFpm { pool } => {
                // Asked again rather than remembered, because the point of showing both is that
                // they can differ: the row was frozen at create and the resolver moves on. For an
                // extension's site the question is its constraint alone — roadmap task **T81b**.
                let resolved = resolve::runtime(&self.store, &holder.question())
                    .await
                    .ok()
                    .and_then(|resolved| {
                        ServiceId::parse(format!("php-fpm@{}", resolved.runtime.version)).ok()
                    });

                Some(SitePool {
                    declared: pool.clone(),
                    resolved,
                })
            }

            _ => None,
        };

        let mut linked = Vec::with_capacity(site.services.len());

        for service in &site.services {
            let state = services::record(&self.store, service)
                .await
                .map_err(|error| error.to_wire())?
                .state;

            linked.push(SiteServiceLink {
                service: service.clone(),
                state,
            });
        }

        Ok(SiteDetail {
            site: summary(site, holder, self.web_port().await?, &self.mdns),
            root: holder.root().display().to_string(),
            doc_root_full: full.display().to_string(),
            doc_root_exists: full.is_dir(),
            domains: site.domains.clone(),
            pool,
            services: linked,
        })
    }
}
