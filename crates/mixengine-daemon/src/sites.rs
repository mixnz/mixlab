//! `site.*`: what is served out of a project's directory, and at what name.
//!
//! Roadmap task **T39a**. [`crate::projects`]' shape one table down, and nothing more: this task
//! declares, and T43 serves. No config file is rendered here and no process is started — but a
//! declared name has to resolve, so every write asks [`crate::elevation`] for the hosts file this
//! home now needs (T41). Asking is all it does: the operation waits for somebody to allow it.
//!
//! # A create is also the import
//!
//! [`ProjectCreate`](mixengine_proto::ProjectCreate)'s rule, one table down (spec D7): every field
//! but the project falls through — the argument, then `[site]` in the project's manifest, then a
//! default. `site.create { project }` with nothing else typed is therefore how a colleague's
//! checkout gets its site, and there is one code path rather than two for one outcome.
//!
//! # Two answers about a pool, because the row holds one of them
//!
//! A php-fpm site's pool is frozen at create while the project's shell keeps following the default.
//! [`mixengine_proto::SitePool`] reports `declared` — what the row holds — beside
//! `resolved` — what `core::resolve` answers at that root today — so somebody whose site is on
//! 8.3.34 while their shell is on 8.3.35 can see that rather than guess at it.
//!
//! # The order of the checks
//!
//! `api/create.rs`' order and its reasoning: cheapest and most specific first, and nothing is
//! written until every one of them has passed. The project, then the doc root, then the domains,
//! then the kind's payload, then the services, then the pool — and only then the write, whose
//! unique index has the last word about whether a domain was free.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mixengine_core::{Store, domains, manifest, projects, resolve, sites};
use mixengine_proto::{
    Error, ErrorCode, ExtensionId, RuntimeKind, SiteCreate, SiteCreation, SiteDetail, SiteKind,
    SiteList, SiteListQuery, SiteOwner, SiteQuery, SiteRef, SiteRemoval, SiteSharing, SiteState,
    SiteSummary, SiteUpdate, VersionConstraint,
};

use crate::error::ToWire as _;

mod checked;
mod lookup;
pub(crate) mod revoke;
mod sharing;

use checked::natively;
#[cfg(test)]
use checked::{route_list_is_writable, route_path_is_a_prefix, upstream_is_an_address};

/// Everything `site.*` needs: the rows, the queue a name change has to reach, and the registry that
/// renders what a site is served by.
#[derive(Debug)]
pub(crate) struct Sites {
    /// Where a site is written down.
    store: Store,

    /// Where a change to the names this home answers for goes to wait for permission — T41.
    elevation: Arc<crate::elevation::Elevation>,

    /// What turns the rows into the front end's configuration, and tells the running server —
    /// roadmap task **T43**. Every write below ends here.
    services: Arc<crate::services::Registry>,

    /// What signs this site's certificate — roadmap task **T50**.
    ///
    /// **Held here rather than reached for through the API**, so a site gains its certificate
    /// before `site.create` answers: a site that needed a second command to get a padlock is a site
    /// whose padlock is somebody's to remember. `Certificates::issue` takes a row and never a
    /// [`SiteRef`], which is what keeps this a one-way edge — a `Certificates` that resolved
    /// references would need the `Sites` holding it.
    certificates: crate::certs::Certificates,

    /// What advertises a shared site's name on the local network — roadmap task **T75**.
    ///
    /// Held rather than reached for, exactly as `certificates` is: every path that changes a share
    /// has to reconcile the advertisement, and a mechanism the caller has to remember is one the
    /// caller eventually forgets.
    mdns: Arc<crate::mdns::Mdns>,

    /// Where a change to what this home is sharing is announced — roadmap task **T76**.
    ///
    /// Held for `mdns`' reason and beside it: every path that changes a share has to say so, and a
    /// mechanism the caller has to remember is one the caller eventually forgets. That matters more
    /// here than anywhere else in this type, because one of the callers is a clock rather than a
    /// person — a share that ends while nobody is looking is exactly the one somebody has to be
    /// told about.
    events: crate::api::Events,

    /// One share at a time — the T76 design, D9.
    ///
    /// **T74 needed no lock and T76 does, and the difference is who calls.** Sharing arrived only
    /// from a person until the watcher; now *read the rows, work out what this machine should have
    /// open, write* runs on a timer beside an API request doing the same thing. Whole-state
    /// reconciliation absorbs most of that race — the next plan supersedes the last — but not the
    /// part where two passes read the same rows and each writes a plan for a home the other has
    /// already changed.
    ///
    /// A `tokio::sync::Mutex` and not a `std` one: the section it covers awaits at every step — a
    /// database write, a re-render, a certificate, an enqueue.
    sharing: tokio::sync::Mutex<()>,
}

/// Who a site belongs to, with what the daemon needs of them — roadmap task **T81b**.
///
/// The core's [`SiteOwner`](sites::SiteOwner) is a rowid or an id; this is the same answer with the
/// root joined on, the wire shape ready, and the question `core::resolve` is asked for the pool —
/// a directory for a project, the constraint alone for an extension (the design's D5).
#[derive(Debug, Clone)]
pub(crate) enum Holder {
    /// A registered project.
    Project(projects::ProjectRecord),

    /// An installed `web-app` extension.
    Extension {
        /// Which one.
        id: ExtensionId,
        /// Its install directory, which is the site's root.
        install_dir: PathBuf,
        /// `[web-app.runtime].requires`, for `SitePool.resolved`.
        requires: VersionConstraint,
    },
}

impl Holder {
    /// What `doc_root` is relative to.
    fn root(&self) -> &Path {
        match self {
            Self::Project(project) => &project.root,
            Self::Extension { install_dir, .. } => install_dir,
        }
    }

    /// The owner as a listing shows it.
    fn wire(&self) -> SiteOwner {
        match self {
            Self::Project(project) => SiteOwner::Project {
                name: project.name.clone(),
            },
            Self::Extension { id, .. } => SiteOwner::Extension { id: id.clone() },
        }
    }

    /// What the resolver is asked for this site's PHP today.
    fn question(&self) -> resolve::Question<'_> {
        match self {
            Self::Project(project) => resolve::Question {
                kind: RuntimeKind::Php,
                cwd: Some(&project.root),
                explicit: None,
            },
            Self::Extension { requires, .. } => resolve::Question {
                kind: RuntimeKind::Php,
                cwd: None,
                explicit: Some(requires),
            },
        }
    }
}

/// The refusal every edit of an extension-owned site answers with — the T81b design's D6.
fn owned_by_an_extension(domain: &str, id: &ExtensionId) -> Error {
    Error::new(
        ErrorCode::PreconditionFailed,
        format!("{domain} belongs to the {id} extension"),
    )
    .with_hint(format!("`mix extension uninstall {id}` removes it"))
}

/// A manifest that names no single site to fall through to, as the refusal a person reads —
/// roadmap task **T204**, spec D5.
fn unchosen(why: manifest::Unchosen) -> Error {
    match why {
        manifest::Unchosen::NotDeclared { name, declared } => Error::new(
            ErrorCode::NotFound,
            format!("mixengine.toml declares no site called {name}"),
        )
        .with_hint(format!("it declares {}", declared.join(", "))),
        manifest::Unchosen::Ambiguous { declared } => Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "mixengine.toml declares {} sites: {}",
                declared.len(),
                declared.join(", ")
            ),
        )
        .with_hint(format!(
            "`mix site create --from {}` adopts one",
            declared.first().map_or("<domain>", String::as_str)
        )),
        manifest::Unchosen::NamedTwice {
            name,
            first,
            second,
        } => Error::new(
            ErrorCode::InvalidArgument,
            format!("mixengine.toml names {name} in two sites, entries {first} and {second}"),
        )
        .with_hint("a domain belongs to one site; remove it from one of them"),
    }
}

impl Sites {
    /// The one of these the API holds.
    pub(crate) fn new(
        store: &Store,
        elevation: Arc<crate::elevation::Elevation>,
        services: Arc<crate::services::Registry>,
        paths: &mixengine_core::Paths,
        mdns: Arc<crate::mdns::Mdns>,
        events: crate::api::Events,
    ) -> Arc<Self> {
        Arc::new(Self {
            certificates: crate::certs::Certificates::issuing(
                paths,
                elevation.host(),
                store.clone(),
            ),
            store: store.clone(),
            elevation,
            services,
            mdns,
            events,
            sharing: tokio::sync::Mutex::new(()),
        })
    }

    /// Every site in this home, as the rows hold them — roadmap task **T76**.
    ///
    /// Here rather than in [`revoke`] so that the watcher asks this type for
    /// its own rows instead of reaching past it into the store: one door onto a table, which is the
    /// rule `mixengine_core::sites` states about itself.
    ///
    /// # Errors
    ///
    /// Whatever reading the tables reports.
    pub(crate) async fn records(&self) -> Result<Vec<sites::SiteRecord>, Error> {
        sites::records(&self.store, None)
            .await
            .map_err(|error| error.to_wire())
    }

    /// Give this site the certificate its names need — roadmap task **T50**.
    ///
    /// **Before the walk that renders its configuration and never after**, which is the ordering
    /// the T50 design's D1 argues for: `etc/` is disposable and rebuilt from the rows, a certificate
    /// is state that cannot be, so issuance is a precondition of generation rather than a step in
    /// it. T51 is what will make the difference visible.
    ///
    /// **Run after an update as well as a create**, because a site that just gained a domain has a
    /// certificate that no longer covers its names — T50's second reuse question. That is what
    /// `docs/features/tls.md` names as the most common "the padlock broke" report.
    ///
    /// A failure is logged and never returned, exactly as [`Self::wants_the_hosts_file`] is: the
    /// row is already written by the time this runs, a site that exists is worth more than a
    /// certificate that does not, and `mix doctor` reports the gap.
    async fn now_has_a_certificate(&self, site: &sites::SiteRecord) {
        let domain = site.domains.first().cloned().unwrap_or_default();

        match self.certificates.issue(Some(site.clone())).await {
            Ok(report) => {
                for outcome in report.sites {
                    match outcome.outcome {
                        mixengine_proto::IssueOutcome::Refused { because } => {
                            tracing::warn!(%domain, %because, "the site has no certificate yet");
                        }

                        // **Silence and not a warning** — roadmap task **T52**. A site with HTTPS
                        // off asked for no certificate, and saying it has none is a line whoever
                        // reads this log has to work out is not about them. Before T52 split the
                        // outcome, every plaintext site created wrote one.
                        mixengine_proto::IssueOutcome::NotWanted { .. }
                        | mixengine_proto::IssueOutcome::Issued {}
                        | mixengine_proto::IssueOutcome::Reused {} => {}
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%domain, %error, "the site has no certificate yet");
            }
        }
    }

    /// Queue the hosts change this home now needs, and never fail an operation over it.
    ///
    /// The site is already written by the time this runs. Returning an error here would say the
    /// operation failed when it did not, and the queue is a want rather than a step: `mix status`
    /// keeps showing what is waiting until somebody grants it or drops it.
    pub(crate) async fn wants_the_hosts_file(&self) {
        if let Err(error) = self.elevation.require_hosts().await {
            tracing::warn!(
                ?error,
                "the site was written, but nothing was queued for the hosts file"
            );
        }
    }

    /// Render what this home now declares, and tell the front end — roadmap task **T43**, D10.
    ///
    /// **Unlike [`wants_the_hosts_file`](Self::wants_the_hosts_file), a failure here fails the
    /// call**, and the difference is what a failure *means*. A hosts entry that has not been granted
    /// is a want with a person on the other end of it, and `mix status` keeps saying so. A
    /// configuration the server refused is a defect: nothing was installed, the front end is still
    /// reading the configuration that worked, and a `site.create` that answered success would send
    /// whoever typed it to look in the wrong place.
    ///
    /// **The row is already written by the time this runs.** That is deliberate rather than
    /// unavoidable — a site is declared state, and a declaration rolled back because the rendering
    /// failed would leave a person with nothing to fix. What they get is a site they can see in
    /// `mix site list` and an error naming the file the checker complained about.
    ///
    /// A home with no front end renders nothing and this succeeds: there is no set to add sites to,
    /// which is a different thing from a set that was refused.
    ///
    /// **Reachable from `extension.*` as well** — roadmap task **T81c**. An extension carrying a
    /// `[[recipe.front_end]]` changed what the front end reads without touching a `sites` row, and
    /// what has to happen next is this and nothing else: no hosts file, no certificate.
    ///
    /// # Errors
    ///
    /// Whatever the walk reports — a rendering that failed, or a set that is not a graph.
    pub(crate) async fn now_serves_what_it_declares(&self) -> Result<(), Error> {
        self.services
            .reconfigure()
            .await
            .map_err(|error| error.to_wire())
    }

    /// `site.create` — declare a site under a project, taking what it was not told from `[site]`.
    ///
    /// # Errors
    ///
    /// `not_found` for a project matching nothing, a service that is not declared, and a pool that
    /// resolves to nothing; `invalid_argument` for a domain that is not one, a doc root outside the
    /// root, and a kind whose payload is wrong; `already_exists` for a domain another site owns.
    pub(crate) async fn create(&self, create: &SiteCreate) -> Result<SiteCreation, Error> {
        let project = self.project(&create.project).await?;
        let manifest = manifest::read(&manifest::at(&project.root))
            .map_err(|error| error.to_wire())?
            .unwrap_or_default();

        // What an entry with no `domain` answers to, which is the name the default below gives it.
        // A name that slugs to nothing names no entry, and the default refuses it in its own words.
        let default_domain = domains::default_for(&project.name).unwrap_or_default();

        // **Which entry, before any field falls through to it** — roadmap task **T204**, spec D5.
        let chosen = manifest
            .choose(
                create.from.as_deref(),
                create.domains.as_deref(),
                &default_domain,
            )
            .map_err(unchosen)?;
        let declared = chosen.site;

        // The fall-through, in the order spec D7 writes it: the argument, the manifest, the default.
        let domains = match &create.domains {
            Some(domains) => domains.clone(),
            None => match declared.and_then(|site| site.domain.clone()) {
                Some(domain) => std::iter::once(domain)
                    .chain(
                        declared
                            .map(|site| site.aliases.clone())
                            .unwrap_or_default(),
                    )
                    .collect(),
                None => vec![domains::default_for(&project.name).map_err(|e| e.to_wire())?],
            },
        };

        let doc_root = create
            .doc_root
            .clone()
            .or_else(|| declared.and_then(|site| site.doc_root.clone()))
            .unwrap_or_default();

        let https = create
            .https
            .or_else(|| declared.and_then(|site| site.https))
            .unwrap_or(true);

        // No manifest fall-through, unlike `https` above: a blueprint declares what a site *is*,
        // and a redirect is a fact about one home's traffic rather than about the site a blueprint
        // describes — roadmap task **T98**. Off whenever the caller does not ask for it.
        let https_redirect = create.https_redirect.unwrap_or(false);

        let kind = create
            .kind
            .clone()
            .or_else(|| declared.and_then(|site| site.kind.clone()))
            .unwrap_or(SiteKind::PhpFpm { pool: None });

        let services = match &create.services {
            Some(services) => services.clone(),
            None => match chosen.services {
                manifest::ServicesFrom::Listed(ids) => self.resolved(ids).await?,
                manifest::ServicesFrom::Every => self.linked(&manifest).await?,
                manifest::ServicesFrom::Nothing => Vec::new(),
            },
        };

        // **Falls through to `[[site.routes]]`, on `kind`'s rule** — roadmap task **T135**. An
        // import is what this exists for: `project.export` writes the routes into the file, and a
        // `site.create` that ignored them would make the round trip lose half of what a site is.
        let routes = create.routes.clone().or_else(|| {
            declared
                .map(|site| site.routes.clone())
                .filter(|routes| !routes.is_empty())
        });

        let new = sites::NewSite {
            owner: sites::SiteOwner::Project(project.id),
            doc_root: sites::relative_doc_root(&project.root, &doc_root)
                .map_err(|error| error.to_wire())?,
            kind: self.settled(&kind, &project).await?,
            https_enabled: https,
            https_redirect,
            domains: self.checked(&domains, create.accept_risky_tld)?,
            services: self.existing(&services).await?,
            routes: self
                .routes(&project, routes.as_deref())
                .await?
                .unwrap_or_default(),
        };

        let written = sites::create(&self.store, &new)
            .await
            .map_err(|error| error.to_wire())?;

        self.wants_the_hosts_file().await;
        self.now_has_a_certificate(&written).await;
        self.now_serves_what_it_declares().await?;

        self.detail(&written, &Holder::Project(project))
            .await
            .map(|site| SiteCreation { site })
    }

    /// `site.list` — every site, or one project's.
    ///
    /// # Errors
    ///
    /// `not_found` for a project matching nothing, and the wire error of a table that cannot be
    /// read.
    pub(crate) async fn list(&self, query: &SiteListQuery) -> Result<SiteList, Error> {
        let project = match &query.project {
            Some(reference) => Some(self.project(reference).await?),
            None => None,
        };

        let records = sites::records(&self.store, project.as_ref().map(|found| found.id))
            .await
            .map_err(|error| error.to_wire())?;

        // One lookup for the whole list rather than one per site: the front end's answer port is
        // a property of the home, and every shared site's URL is built from it.
        let web_port = self.web_port().await?;

        let mut listed = Vec::with_capacity(records.len());

        for record in records {
            let holder = match &project {
                Some(project) => Holder::Project(project.clone()),
                None => self.holder_of(&record).await?,
            };

            listed.push(summary(&record, &holder, web_port, &self.mdns));
        }

        Ok(SiteList { sites: listed })
    }

    /// `site.show` — one site, with its domains, its pool and its services.
    ///
    /// # Errors
    ///
    /// `not_found` for a reference matching nothing, and `invalid_argument` for a path whose
    /// project holds several sites.
    pub(crate) async fn show(&self, query: &SiteQuery) -> Result<SiteDetail, Error> {
        let (site, holder) = self.expect(&query.site).await?;

        self.detail(&site, &holder).await
    }

    /// `site.update` — change what a site is.
    ///
    /// `domains` and `services` **replace** rather than merge: with a merge there is no way to
    /// remove one.
    ///
    /// # Errors
    ///
    /// Everything [`Sites::create`] refuses, plus `not_found` for a site matching nothing.
    /// Set this site's domains to exactly this list — roadmap task **T46**.
    ///
    /// [`Self::update`] with one field filled in, named so that the two `domain.*` verbs read as
    /// what they are rather than as an update that happens to leave six fields empty — and so that
    /// there is one place to look when the shape of an update changes.
    ///
    /// # Errors
    ///
    /// [`Self::update`]'s: an unknown site, a public TLD, `.local` without the acknowledgement, a
    /// domain another site already holds.
    pub(crate) async fn replace_domains(
        &self,
        site: &SiteRef,
        domains: Vec<String>,
        accept_risky_tld: bool,
    ) -> Result<SiteDetail, Error> {
        self.update(&SiteUpdate {
            site: site.clone(),
            domains: Some(domains),
            accept_risky_tld,
            doc_root: None,
            kind: None,
            services: None,
            routes: None,
            https: None,
            https_redirect: None,
            state: None,
        })
        .await
    }

    pub(crate) async fn update(&self, update: &SiteUpdate) -> Result<SiteDetail, Error> {
        let (site, holder) = self.expect(&update.site).await?;
        let project = self.editable(&site, &holder)?;

        let kind = match &update.kind {
            Some(kind) => Some(self.settled(kind, project).await?),
            None => None,
        };

        let domains = match &update.domains {
            Some(domains) => Some(self.checked(domains, update.accept_risky_tld)?),
            None => None,
        };

        let services = match &update.services {
            Some(services) => Some(self.existing(services).await?),
            None => None,
        };

        let doc_root = match &update.doc_root {
            Some(doc_root) => {
                Some(sites::relative_doc_root(&project.root, doc_root).map_err(|e| e.to_wire())?)
            }
            None => None,
        };

        let routes = self.routes(project, update.routes.as_deref()).await?;

        let changed = sites::update(
            &self.store,
            site.id,
            &sites::Change {
                doc_root,
                kind,
                https_enabled: update.https,
                https_redirect: update.https_redirect,
                state: update.state,
                domains,
                services,
                routes,
            },
        )
        .await
        .map_err(|error| error.to_wire())?;

        self.wants_the_hosts_file().await;
        self.now_has_a_certificate(&changed).await;
        self.now_serves_what_it_declares().await?;

        self.detail(&changed, &holder).await
    }

    /// `site.start` — serve this site.
    ///
    /// **A flag and a walk** — D9. The state is set, the front end's configuration is re-rendered
    /// and the running server is told; nothing starts a process. A home whose Caddy is not running
    /// gets a rendered site file and no traffic, which is T39a's line held rather than a limitation:
    /// a `site.start` that started the front end would owe an answer to what it means when the pool
    /// starts and the front end does not.
    ///
    /// **Idempotent, and not because anything here checks.** A second call on an enabled site
    /// re-renders the same bytes; `document::install` compares before it stages, finds nothing
    /// changed, skips the validator and reloads nothing.
    ///
    /// # Errors
    ///
    /// `not_found` for a site matching nothing, `invalid_argument` for a path whose project holds
    /// several sites, and whatever the walk reports for a configuration the front end refuses.
    pub(crate) async fn start(&self, query: &SiteQuery) -> Result<SiteDetail, Error> {
        self.serving(query, SiteState::Enabled).await
    }

    /// `site.stop` — keep the declaration, stop serving it.
    ///
    /// The site's rendered file goes on the same walk, because the front end's `sites/` directory
    /// holds exactly what was rendered into it. Without that, a stopped site would go on being
    /// served until something else changed.
    ///
    /// # Errors
    ///
    /// [`Sites::start`]'s.
    pub(crate) async fn stop(&self, query: &SiteQuery) -> Result<SiteDetail, Error> {
        self.serving(query, SiteState::Disabled).await
    }

    /// Both of the above: set the state, walk, and answer with the site as it now is.
    ///
    /// The same answer as `site.show`, because what a caller wants back is the site as it now
    /// stands — not a confirmation that something happened to it.
    async fn serving(&self, query: &SiteQuery, state: SiteState) -> Result<SiteDetail, Error> {
        let (site, holder) = self.expect(&query.site).await?;

        let changed = sites::update(
            &self.store,
            site.id,
            &sites::Change {
                state: Some(state),
                ..sites::Change::default()
            },
        )
        .await
        .map_err(|error| error.to_wire())?;

        self.now_serves_what_it_declares().await?;

        self.detail(&changed, &holder).await
    }

    /// `site.delete` — take the row, and leave the files.
    ///
    /// **A shared site takes its share with it** — roadmap task **T199a**. The firewall rule and the
    /// mDNS name a share put outside the front end are reconciled from the rows, so once the row is
    /// gone the next reconciliation withdraws them; this is that reconciliation, in `unshare`'s
    /// order. Without it they stayed until the daemon next started, opening a port for a site
    /// nothing declared. Under the sharing lock for `unshare`'s reason: the watcher reads the same
    /// rows on a timer.
    ///
    /// # Errors
    ///
    /// `not_found` for a site matching nothing, and the wire error of a row that cannot be removed.
    pub(crate) async fn delete(&self, query: &SiteQuery) -> Result<SiteRemoval, Error> {
        let _sharing = self.sharing.lock().await;

        let (removed, holder) = self.expect(&query.site).await?;
        self.editable(&removed, &holder)?;

        sites::delete(&self.store, removed.id)
            .await
            .map_err(|error| error.to_wire())?;

        self.wants_the_hosts_file().await;

        match removed.sharing.is_some() {
            // The rule first, then the listener it was protecting, then the name — `unshare`'s order.
            // No certificate is reissued: the site it named is gone.
            true => {
                tracing::info!(
                    site = removed.id,
                    "a deleted site was shared; its share is withdrawn with it"
                );
                self.wants_the_firewall().await?;
                self.now_serves_what_it_declares().await?;
                self.advertises_what_it_declares().await?;
                self.announce(&removed, None, mixengine_proto::SharingChange::Requested {});
            }
            false => self.now_serves_what_it_declares().await?,
        }

        Ok(SiteRemoval {
            domains_released: removed.domains.clone(),
            doc_root_kept: doc_root_full(holder.root(), &removed.doc_root)
                .display()
                .to_string(),
            removed: summary(&removed, &holder, self.web_port().await?, &self.mdns),
        })
    }
}

impl Sites {
    /// The port this home's front end answers HTTP on, for the URL a shared site is reached at.
    ///
    /// 80 where this home has no front end, which is also a home where nothing can be shared — so
    /// the number is never used, and refusing to answer here would turn a listing into an error for
    /// a home that simply has not installed a web server yet.
    async fn web_port(&self) -> Result<u16, Error> {
        sites::web_port(&self.store, &crate::services::catalogue())
            .await
            .map_err(|error| error.to_wire())
    }
}

/// One record, as a listing shows it.
fn summary(
    site: &sites::SiteRecord,
    holder: &Holder,
    web_port: u16,
    mdns: &crate::mdns::Mdns,
) -> SiteSummary {
    SiteSummary {
        domain: site.domains.first().cloned().unwrap_or_default(),
        owner: holder.wire(),
        kind: site.kind.clone(),
        // Stored with `/`, sent in this system's spelling — roadmap task **T191**.
        doc_root: mixengine_core::paths::native_relative(&site.doc_root),
        https: site.https_enabled,
        https_redirect: site.https_redirect,
        state: site.state,
        // **In match order and not in the order somebody typed** — roadmap task **T135**. The
        // rendering resolves an overlap by specificity, so a listing showing declaration order
        // would be showing something the front end does not do.
        routes: {
            let mut routes = site.routes.clone();
            routes.sort_by(sites::by_specificity);
            routes.into_iter().map(natively).collect()
        },
        sharing: site.sharing.as_ref().map(|sharing| {
            let url = sites::shared_url(sharing.address, web_port);

            // The same three answers `site.share` gives, from the same two sources — roadmap task
            // T75. A listing that reported a name differently from the command that created it
            // would be a second definition of what this site is called.
            let name = site
                .domains
                .first()
                .and_then(|primary| sites::shared_name(primary));

            SiteSharing {
                interface: sharing.interface.clone(),
                address: sharing.address.to_string(),
                ca_url: format!("{url}/__mixengine/ca.crt"),
                advertised: name.as_deref().is_some_and(|name| mdns.advertising(name)),
                name,
                url,
                since: sharing.since,
                until: sharing.until,
            }
        }),
    }
}

/// Root plus doc root, as the filesystem spells it. `""` is the root itself.
fn doc_root_full(root: &Path, doc_root: &str) -> PathBuf {
    mixengine_core::paths::join_stored(root, doc_root)
}

#[cfg(test)]
mod tests;
