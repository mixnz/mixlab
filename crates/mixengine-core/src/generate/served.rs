//! What a front end has to serve, assembled out of the three site tables.
//!
//! [`Served`] is a site as a *template* needs it and not as the database holds it: the doc root
//! joined onto its project's root, the domains already ordered with the primary at the head, and a
//! php-fpm site's pool already resolved to the address that pool listens on. Everything a recipe
//! would otherwise have to look up itself is looked up once, here, for the same reason
//! [`Endpoints`](super::recipe::Endpoints) exists — a template that computed a path would be a
//! second place for it to be computed differently.
//!
//! # One place reads these tables
//!
//! [`crate::sites::records`] is that place, and this module asks it rather than writing a query of
//! its own. `sites.rs`' module note is explicit about why: a second door onto a table is a second
//! answer to a question that has one. What is added here is the join onto `projects`, which is the
//! one thing a doc root cannot be made absolute without.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mixengine_proto::{ServiceId, SiteKind, SiteState};
use serde::Serialize;

use super::recipe::{Upstream, Upstreams};
use crate::sites::SiteOwner;
use crate::{Error, Result, ServiceError, Store};

/// The certificate a site is served with.
///
/// **Paths and a fingerprint, and no bytes.** A template writes the two paths into a directive the
/// server reads for itself; nothing here has to hold a certificate, and there is nowhere a private
/// key could travel — the same shape [`SiteCert`](mixengine_proto::SiteCert) takes on the wire, for
/// the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteCertificate {
    /// Absolute path to the certificate.
    pub certificate: PathBuf,

    /// Absolute path to the private key.
    pub key: PathBuf,

    /// SHA-256 of the certificate's DER, lowercase hex.
    ///
    /// **Rendered into the generated file's header, and read back by nothing.**
    /// [`crate::certs::leaf`] reissues to the same path, so without this the file a reissue
    /// produces is byte-identical to the one already installed, `document::install` finds no
    /// difference, and the running server is never told to read the new certificate.
    ///
    /// **Being told is not the whole of it on Caddy**, and that half is the recipe's: the adapter
    /// strips comments, so the configuration a reissue produces is identical to the one the server
    /// is running and the reload it is sent is skipped. `recipes::caddy` passes `--force` for it.
    pub fingerprint: String,
}

/// What a shared site's rendering needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shared {
    /// The IPv4 address bound, certified and printed. IPv4 only.
    pub address: std::net::Ipv4Addr,

    /// The mDNS name this site answers to, `<slug>-mixengine.local`.
    ///
    /// **The block has to name it, not only the network.** Advertising the name says where it
    /// resolves; the block's address list says which site replies to it. LAN sharing paid for
    /// learning that those are two questions, with an address; this is the same lesson with a name.
    ///
    /// [`None`] for a primary domain nothing can be slugged out of — a site reached by address
    /// alone rather than a site that cannot be shared.
    pub name: Option<String>,
}

/// A prefix rewritten on the way out.
///
/// **A regex on both front ends**, because that is the one primitive they share: Caddy's `uri
/// path_regexp` and nginx's `rewrite … break` take the same two strings, so one computation answers
/// for both and a divergence between them has nowhere to hide.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Rewrite {
    /// Anchored, with every `.` in the matched prefix escaped.
    pub regex: String,

    /// `$1` carries whatever followed the prefix.
    pub replacement: String,
}

/// An address and the rewrite its path implies, split apart.
///
/// **Caddy refuses a path in a proxy upstream outright** — *"for now, URLs for proxy upstreams only
/// support scheme, host, and port components"*, measured against 2.11.4 — and the whole rendering is
/// judged by one `caddy validate` where it is staged, so a single site carrying one would cost
/// *every* site on the machine its new configuration. The split is what makes that URL mean the same
/// thing on both front ends instead of breaking one of them.
///
/// The rule it implements, in one sentence: the matched prefix is replaced by the upstream's path
/// with any trailing slash removed, and an upstream with no path replaces nothing.
fn split_upstream(path: &str, upstream: &str) -> (String, Option<Rewrite>) {
    let escaped = path.replace('.', r"\.");

    let Some((scheme, rest)) = upstream.split_once("://") else {
        return (upstream.to_owned(), None);
    };

    let Some((host, target)) = rest.split_once('/') else {
        return (upstream.to_owned(), None);
    };

    let address = format!("{scheme}://{host}");
    let target = target.trim_end_matches('/');

    // **Two shapes, and the empty one is why.** `^/abc(/.*)?$` with a replacement of `$1` would
    // send the exact prefix upstream with no path at all; `^/abc/?(.*)$` with `/$1` answers `/`
    // there and `/foo` below it.
    let rewrite = if target.is_empty() {
        Rewrite {
            regex: format!("^{escaped}/?(.*)$"),
            replacement: "/$1".to_owned(),
        }
    } else {
        Rewrite {
            regex: format!("^{escaped}(/.*)?$"),
            replacement: format!("/{target}$1"),
        }
    };

    (address, Some(rewrite))
}

/// One route of a site, with everything a template would otherwise have to look up resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedRoute {
    /// The prefix, exactly as the row holds it.
    pub path: String,

    /// What answers under it.
    pub target: ServedRouteTarget,
}

/// What answers under a route, with its addresses resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServedRouteTarget {
    /// Forwarded, with the prefix rewritten when the upstream asked for it.
    Proxy {
        /// Scheme, host and port, and never a path — `split_upstream` in this module is why.
        address: String,

        /// What the prefix becomes on the way out, or [`None`] to pass the path through.
        rewrite: Option<Rewrite>,
    },

    /// PHP through a pool, over the site's own document root.
    PhpFpm {
        /// Where the pool is, in this system's shape.
        upstream: Upstream,

        /// Where the activator waits for it, on the site's own rule.
        activator: Option<Upstream>,
    },

    /// Files from a directory, with the prefix stripped.
    Static {
        /// Absolute: the owner's root joined to the route's own.
        root: PathBuf,
    },
}

/// One site, as the thing that renders it needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    /// Ordered; the head is the primary.
    pub domains: Vec<String>,

    /// Absolute: the owner's root — a project's directory, or an extension's install directory
    /// — joined to the row's relative doc root.
    pub doc_root: PathBuf,

    /// The row's own spelling of it, before the join above.
    ///
    /// **Carried rather than recovered.** The welcome page names where a site's files are looked
    /// for, and it may not name an absolute path: a shared site's page is reachable from the local
    /// network, and where this machine keeps its projects is a fact about the machine. Recovering
    /// it with `strip_prefix` afterwards would be undoing the join two lines below, and would
    /// answer differently for the one doc root that is not under its own root.
    ///
    /// Empty for a site served from the owner's root itself, which is what the column holds.
    pub doc_root_relative: String,

    /// What the welcome page lists. Empty for an extension's site and for a project whose blueprint
    /// names no steps.
    pub steps: Vec<crate::generate::welcome::WelcomeStep>,

    /// What it serves, and what that kind needs to know.
    pub kind: ServedKind,

    /// What answers before the kind does, **in match order** — longest prefix first.
    ///
    /// **Sorted here and not in a template**, because the two front ends resolve an overlap by
    /// different native rules — Caddy takes its handlers in order, nginx has location precedence of
    /// its own — and a rendering that left the decision to either would be two behaviours wearing
    /// one name. Empty for every site made before routes existed, which renders exactly what it
    /// rendered then.
    pub routes: Vec<ServedRoute>,

    /// Whether HTTPS is declared.
    ///
    /// **Not the same question as [`certificate`](Self::certificate).** A site can declare HTTPS and
    /// have nothing on disk to serve it with — a home whose authority failed to generate — and the
    /// two are different states: this one is a problem `mix doctor` reports, a site that declared
    /// none is a site working as asked.
    pub https: bool,

    /// Whether the plaintext address redirects to the HTTPS one.
    ///
    /// **Read beside [`certificate`](Self::certificate), not instead of it.** A site can carry this
    /// as `true` and still have no usable certificate — the same gap `https` above already has a
    /// name for — and a recipe that redirected anyway would send every request into a TLS listener
    /// nothing is bound to. The recipes gate the redirect on both being true at once, the same way
    /// they already gate the `tls` block itself.
    pub https_redirect: bool,

    /// The certificate it is served with, when it has a usable one.
    ///
    /// [`None`] renders no TLS for this site at all. It keeps working over HTTP, the other sites are
    /// untouched, and `mix doctor`'s `SiteCertificateMissing` reports it.
    pub certificate: Option<SiteCertificate>,

    /// Where the local network reaches this site, when it does.
    ///
    /// **Per site, which is the whole of "opt-in per site".** The front end's own `bind_addr` is
    /// untouched by sharing; what changes is this one site's listeners. A site that is not shared
    /// renders exactly what it rendered before LAN sharing existed, and each recipe asserts that
    /// rather than assuming it.
    ///
    /// **One value or none**, like the [`Sharing`](crate::sites::Sharing) row it is built from: an
    /// address without the name it is advertised under is a pair of fields a reader has to agree
    /// about, and the mDNS advertisement needs both in the same places.
    pub shared: Option<Shared>,
}

impl Served {
    /// The domain this site is named after, in a listing and in a filename.
    ///
    /// The head of the list, which `core::sites` guarantees is the primary — and guarantees is
    /// there: a site with no domain is not a row this build can write.
    #[must_use]
    pub fn primary(&self) -> &str {
        self.domains.first().map_or("", String::as_str)
    }
}

/// What a site serves, with everything a template would otherwise have to look up resolved.
///
/// [`SiteKind`]'s shape with one difference, and it is the difference
/// this type exists for: a php-fpm site carries the *address* its pool listens on rather than the
/// pool's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServedKind {
    /// PHP through a pool, at the address that pool listens on.
    PhpFpm {
        /// Where the pool is, in this system's shape.
        upstream: Upstream,

        /// Where the activator waits for it, when a connection can start this pool.
        ///
        /// The site names it *after* [`upstream`](Self::PhpFpm::upstream), so a request arriving
        /// while the pool is idle-stopped is retried against it instead of answered with a 502.
        /// [`None`] renders exactly what this site rendered before on-demand activation: a home
        /// whose pool nothing can wake is the home it was yesterday.
        activator: Option<Upstream>,
    },

    /// Files, and nothing running.
    Static,

    /// Everything forwarded to an address the user already has listening.
    ReverseProxy {
        /// Scheme, host and port — **and never a path**.
        ///
        /// The row may hold one, and once it was written into the configuration unchanged: nginx
        /// read it as a prefix replacement and Caddy refused the file outright, which cost every
        /// site on the machine its rendering. `split_upstream` in this module takes it off, and
        /// [`rewrite`](Self::ReverseProxy::rewrite) carries what it meant.
        upstream: String,

        /// What `/` becomes on the way out, when the row's upstream carried a path.
        rewrite: Option<Rewrite>,
    },

    /// A node process the user runs, on a loopback port.
    ///
    /// **Rendered exactly as a reverse proxy to `127.0.0.1:<port>`, and that is all it is.** Nothing
    /// in this build starts `npm run dev`; what distinguishes this from
    /// [`ReverseProxy`](Self::ReverseProxy) is the scope of the address rather than a mechanism, and
    /// writing that down is more honest than a kind that pretends to more.
    NodeApp {
        /// The loopback port it listens on.
        port: u16,
    },
}

/// Render one site template, with the environment every generated file in this build is rendered
/// under.
///
/// Beside [`recipe::render`](super::recipe::render) rather than inside it, because that one renders
/// a recipe's *declared* files — one per [`TemplateFile`](super::recipe::TemplateFile) — and a site
/// is one template rendered once per row. The two share the settings and nothing else, and those
/// settings are what is worth sharing: strict undefined, so a misspelled variable is a failure with
/// a name in it rather than a config line with nothing after it, and a kept trailing newline,
/// because every one of these formats is one where somebody cares.
///
/// # Errors
///
/// [`ServiceError::TemplateBroken`], naming `template`.
pub(super) fn render(
    source: &str,
    template: &'static str,
    service: &ServiceId,
    value: &impl Serialize,
) -> Result<String> {
    let mut environment = minijinja::Environment::new();
    environment.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    environment.set_keep_trailing_newline(true);

    environment
        .render_str(source, minijinja::Value::from_serialize(value))
        .map_err(|source| {
            Error::Service(ServiceError::TemplateBroken {
                service: service.as_str().to_owned(),
                file: template,
                source: Box::new(source),
            })
        })
}

/// Every enabled site, joined to the project that gives its doc root a root, with each php-fpm
/// site's pool resolved through `upstreams`.
///
/// **A site whose pool is not in the map is left out**, and that is a decision rather than an
/// oversight. `sites.php_service_id` is `ON DELETE SET NULL` and `service.delete --force` is
/// allowed to cross a site's declaration, so a php-fpm site with no pool is a row this build can
/// produce. Failing the render over one would mean a single `--force` left a daemon that could not
/// render *anything*; serving it any other way would mean answering PHP with something that is not
/// PHP. So it is absent, its file is swept away, a line goes in `daemon.log`, and reporting it to a
/// person is `mix doctor`'s.
///
/// # Errors
///
/// [`Error::Database`] when the tables cannot be read, and whatever
/// [`crate::sites::records`] reports for a row this build cannot read.
pub(super) async fn served(
    store: &Store,
    upstreams: &BTreeMap<ServiceId, Upstreams>,
    certs: &Path,
    extensions: &BTreeMap<String, crate::extensions::store::Installed>,
) -> Result<Vec<Served>> {
    // **The blueprint each project came from, beside its root**: the welcome page lists what that
    // blueprint says to run.
    let rows = sqlx::query!(
        r#"SELECT p.id AS "id!: i64", p.root_path AS "root_path!: String",
                  p.name AS "name!: String", b.manifest_toml AS "manifest?: String"
           FROM projects p LEFT JOIN blueprints b ON b.id = p.blueprint_id"#
    )
    .fetch_all(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    let mut roots: BTreeMap<i64, String> = BTreeMap::new();
    // Each step beside the site it names, when it names one.
    let mut welcome_steps: BTreeMap<
        i64,
        Vec<(Option<String>, crate::generate::welcome::WelcomeStep)>,
    > = BTreeMap::new();

    for row in rows {
        // A row whose manifest does not read is a page with no steps, never a render that fails:
        // the page is a courtesy, and the site it sits in front of matters more.
        if let Some(manifest) = row
            .manifest
            .as_deref()
            .and_then(|text| crate::blueprints::manifest::read(text).ok())
        {
            let steps = crate::blueprints::steps::expanded(&manifest.next_steps, &row.name)
                .into_iter()
                .filter(|step| step.kind != mixengine_proto::NextStepKind::Open)
                .filter_map(|step| {
                    Some((
                        step.site,
                        crate::generate::welcome::WelcomeStep {
                            run: step.run?,
                            note: step.note,
                        },
                    ))
                })
                .collect();
            welcome_steps.insert(row.id, steps);
        }

        roots.insert(row.id, row.root_path);
    }

    let mut served = Vec::new();

    for record in crate::sites::records(store, None).await? {
        if record.state != SiteState::Enabled {
            continue;
        }

        // **The owner's root, whichever owner.** Both foreign keys make the misses unreachable
        // through the database's own rules, so reaching one is a row somebody wrote by hand. Said
        // rather than rendered against nothing.
        let root: PathBuf = match &record.owner {
            SiteOwner::Project(project) => match roots.get(project) {
                Some(root) => PathBuf::from(root),
                None => {
                    tracing::warn!(
                        site = record.id,
                        project,
                        "a site belongs to a project that is not there; it is not being served"
                    );
                    continue;
                }
            },
            SiteOwner::Extension(id) => match extensions.get(id.as_str()) {
                Some(installed) => installed.install_dir.clone(),
                None => {
                    tracing::warn!(
                        site = record.id,
                        extension = id.as_str(),
                        "a site belongs to an extension that is not installed; it is not being \
                         served"
                    );
                    continue;
                }
            },
        };

        let kind = match &record.kind {
            SiteKind::PhpFpm { pool: Some(pool) } => match upstreams.get(pool) {
                Some(pool) => ServedKind::PhpFpm {
                    upstream: pool.listen.clone(),
                    activator: pool.activator.clone(),
                },
                None => {
                    tracing::warn!(
                        site = record.id,
                        pool = pool.as_str(),
                        "this site's pool is not a service this home declares, so the site is not \
                         being served; `mix doctor` is what reconciles that"
                    );
                    continue;
                }
            },
            SiteKind::PhpFpm { pool: None } => {
                tracing::warn!(
                    site = record.id,
                    "this site names no pool, so it is not being served; the pool it named was \
                     deleted, and `mix doctor` is what reconciles that"
                );
                continue;
            }
            SiteKind::Static => ServedKind::Static,
            SiteKind::ReverseProxy { upstream } => {
                // **The empty prefix and not `/`.** A site-wide proxy matches every path, so what
                // its upstream's path replaces is the string before the first slash — which is
                // nothing. `"/"` here would build `^/(/.*)?$`, an expression that matches `/` and
                // no path below it.
                let (address, rewrite) = split_upstream("", upstream);

                ServedKind::ReverseProxy {
                    upstream: address,
                    rewrite,
                }
            }
            SiteKind::NodeApp { port } => ServedKind::NodeApp { port: *port },
        };

        let mut declared = record.routes.clone();
        declared.sort_by(crate::sites::by_specificity);

        let mut routes = Vec::with_capacity(declared.len());

        for route in declared {
            let target = match route.target {
                mixengine_proto::RouteTarget::Proxy { ref upstream } => {
                    let (address, rewrite) = split_upstream(&route.path, upstream);

                    ServedRouteTarget::Proxy { address, rewrite }
                }

                // **A route whose pool has gone loses the route, not the site.** A site with no
                // pool has nothing left to serve and is dropped whole; a route is one prefix of a
                // site that still has a root, a kind and possibly four other routes, and taking it
                // down would turn one `--force` into an outage.
                mixengine_proto::RouteTarget::PhpFpm { pool: None } => {
                    tracing::warn!(
                        site = record.id,
                        route = route.path,
                        "this route names no pool, so it is not being served; the pool it named \
                         was deleted, and the rest of the site is unaffected"
                    );
                    continue;
                }
                mixengine_proto::RouteTarget::PhpFpm {
                    pool: Some(ref pool),
                } => match upstreams.get(pool) {
                    Some(resolved) => ServedRouteTarget::PhpFpm {
                        upstream: resolved.listen.clone(),
                        activator: resolved.activator.clone(),
                    },
                    None => {
                        tracing::warn!(
                            site = record.id,
                            route = route.path,
                            pool = pool.as_str(),
                            "this route's pool is not a service this home declares, so the \
                                 route is not being served; the rest of the site is"
                        );
                        continue;
                    }
                },

                mixengine_proto::RouteTarget::Static { root: ref under_it } => {
                    ServedRouteTarget::Static {
                        root: under(&root, under_it),
                    }
                }
            };

            routes.push(ServedRoute {
                path: route.path,
                target,
            });
        }

        served.push(Served {
            // Before `record.domains` moves out of the record below.
            certificate: certificate(certs, &record),
            shared: record.sharing.as_ref().map(|sharing| Shared {
                address: sharing.address,
                name: record
                    .domains
                    .first()
                    .and_then(|primary| crate::sites::shared_name(primary)),
            }),
            doc_root: under(&root, &record.doc_root),
            doc_root_relative: record.doc_root.clone(),
            steps: match &record.owner {
                SiteOwner::Project(project) => welcome_steps
                    .get(project)
                    .map(|steps| {
                        steps
                            .iter()
                            // **A step that names a site belongs to that site alone.** One naming
                            // none is the one site's.
                            .filter(|(site, _)| {
                                site.as_ref()
                                    .is_none_or(|site| record.domains.contains(site))
                            })
                            .map(|(_, step)| step.clone())
                            .collect()
                    })
                    .unwrap_or_default(),
                SiteOwner::Extension(_) => Vec::new(),
            },
            domains: record.domains,
            kind,
            routes,
            https: record.https_enabled,
            https_redirect: record.https_redirect,
        });
    }

    Ok(served)
}

/// What this site is served with, or [`None`].
///
/// **The one call in this module that touches a disk.** `generate` is otherwise a function of the
/// database alone, and that is worth saying out loud rather than discovering later: `tls` names two
/// files, so something has to know whether they are there, and the choice was to know it here rather
/// than in a template or in a second pass over the rendering. It is one call, in one place, and its
/// result is data the rest of the render treats like any other field.
///
/// **Through `leaf::read` and not `Path::exists`**, because the check that decides whether to write
/// a `tls` line should be the check that decides whether the pair is usable: a truncated certificate
/// passes an existence test and fails `caddy validate` — and a validation failure costs *every* site
/// its new configuration rather than this one, which is what the whole one-file-per-site layout
/// exists to prevent.
fn certificate(certs: &Path, record: &crate::sites::SiteRecord) -> Option<SiteCertificate> {
    if !record.https_enabled {
        return None;
    }

    let primary = record.domains.first()?;

    let mixengine_proto::CertState::Present { cert } =
        crate::certs::leaf::read(certs, primary, std::time::SystemTime::now())
    else {
        return None;
    };

    Some(SiteCertificate {
        certificate: crate::certs::leaf::certificate_path(certs, primary),
        key: crate::certs::leaf::key_path(certs, primary),
        fingerprint: cert.fingerprint,
    })
}

/// A project's root joined to a doc root as the row stores it.
///
/// **Component by component**, because the row is forward-slashed on every system — `sites.rs` says
/// so, and says why: the value is rendered into a web server's configuration and read by a person on
/// a machine that may not be this one. `Path::join` on Windows would leave `C:\src\blog\web/public`,
/// which works and reads as a mistake. An empty doc root is the root itself, and joining `""` would
/// leave a trailing separator.
fn under(root: &Path, doc_root: &str) -> PathBuf {
    let mut path = root.to_path_buf();

    for segment in doc_root.split('/').filter(|segment| !segment.is_empty()) {
        path.push(segment);
    }

    path
}

#[cfg(test)]
mod tests;
