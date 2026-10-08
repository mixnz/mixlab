//! What a front end has to serve, assembled out of the three site tables — roadmap task **T43**.
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
use crate::{Error, Result, Store};

/// The certificate a site is served with — roadmap task **T51**.
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
    /// **Rendered into the generated file's header, and read back by nothing** — the T51 design, D5.
    /// T50 reissues to the same path, so without this the file a reissue produces is byte-identical
    /// to the one already installed, `document::install` finds no difference, and the running server
    /// is never told to read the new certificate.
    ///
    /// **Being told is not the whole of it on Caddy**, and that half is the recipe's: the adapter
    /// strips comments, so the configuration a reissue produces is identical to the one the server
    /// is running and the reload it is sent is skipped. `recipes::caddy` passes `--force` for it.
    pub fingerprint: String,
}

/// What a shared site's rendering needs — roadmap tasks **T74** and **T75**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shared {
    /// The IPv4 address bound, certified and printed. IPv4 only — the T74 design, D4.
    pub address: std::net::Ipv4Addr,

    /// The mDNS name this site answers to, `<slug>-mixengine.local` — roadmap task **T75**.
    ///
    /// **The block has to name it, not only the network.** Advertising the name says where it
    /// resolves; the block's address list says which site replies to it. T74 paid for learning
    /// that those are two questions, with an address; this is the same lesson with a name.
    ///
    /// [`None`] for a primary domain nothing can be slugged out of — a site reached by address
    /// alone rather than a site that cannot be shared.
    pub name: Option<String>,
}

/// A prefix rewritten on the way out — roadmap task **T135**, that design's D2.
///
/// **A regex on both front ends**, because that is the one primitive they share: Caddy's `uri
/// path_regexp` and nginx's `rewrite … break` take the same two strings, so one computation answers
/// for both and a divergence between them has nowhere to hide.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Rewrite {
    /// Anchored, with every `.` in the matched prefix escaped — the design's D5.
    pub regex: String,

    /// `$1` carries whatever followed the prefix.
    pub replacement: String,
}

/// An address and the rewrite its path implies, split apart — roadmap task **T135**.
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

/// One route of a site, with everything a template would otherwise have to look up resolved —
/// roadmap task **T135**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedRoute {
    /// The prefix, exactly as the row holds it.
    pub path: String,

    /// What answers under it.
    pub target: ServedRouteTarget,
}

/// What answers under a route, with its addresses resolved — roadmap task **T135**.
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

        /// Where the activator waits for it — roadmap task **T70**, the site's own rule.
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
    /// (roadmap task **T81b**) — joined to the row's relative doc root.
    pub doc_root: PathBuf,

    /// The row's own spelling of it, before the join above — roadmap task **T124**.
    ///
    /// **Carried rather than recovered.** The welcome page names where a site's files are looked
    /// for, and it may not name an absolute path: a shared site's page is reachable from the local
    /// network (T74, T75), and where this machine keeps its projects is a fact about the machine.
    /// Recovering it with `strip_prefix` afterwards would be undoing the join two lines below, and
    /// would answer differently for the one doc root that is not under its own root.
    ///
    /// Empty for a site served from the owner's root itself, which is what the column holds.
    pub doc_root_relative: String,

    /// What the welcome page lists — roadmap task **T205**, D6. Empty for an extension's site and
    /// for a project whose blueprint names no steps.
    pub steps: Vec<crate::generate::welcome::WelcomeStep>,

    /// What it serves, and what that kind needs to know.
    pub kind: ServedKind,

    /// What answers before the kind does, **in match order** — longest prefix first — roadmap task
    /// **T135**.
    ///
    /// **Sorted here and not in a template**, because the two front ends resolve an overlap by
    /// different native rules — Caddy takes its handlers in order, nginx has location precedence of
    /// its own — and a rendering that left the decision to either would be two behaviours wearing
    /// one name. Empty for every site made before T135, which renders exactly what it rendered then.
    pub routes: Vec<ServedRoute>,

    /// Whether HTTPS is declared.
    ///
    /// **Not the same question as [`certificate`](Self::certificate).** A site can declare HTTPS and
    /// have nothing on disk to serve it with — a home whose authority failed to generate — and the
    /// two are different states: this one is a problem `mix doctor` reports, a site that declared
    /// none is a site working as asked.
    pub https: bool,

    /// Whether the plaintext address redirects to the HTTPS one — roadmap task **T98**.
    ///
    /// **Read beside [`certificate`](Self::certificate), not instead of it.** A site can carry this
    /// as `true` and still have no usable certificate — the same gap `https` above already has a
    /// name for — and a recipe that redirected anyway would send every request into a TLS listener
    /// nothing is bound to. The recipes gate the redirect on both being true at once, the same way
    /// they already gate the `tls` block itself.
    pub https_redirect: bool,

    /// The certificate it is served with, when it has a usable one — roadmap task **T51**.
    ///
    /// [`None`] renders no TLS for this site at all. It keeps working over HTTP, the other sites are
    /// untouched, and `mix doctor`'s `SiteCertificateMissing` reports it.
    pub certificate: Option<SiteCertificate>,

    /// Where the local network reaches this site, when it does — roadmap tasks **T74** and **T75**.
    ///
    /// **Per site, which is the whole of "opt-in per site".** The front end's own `bind_addr` is
    /// untouched by sharing; what changes is this one site's listeners. A site that is not shared
    /// renders exactly what it rendered before T74 existed, and each recipe asserts that rather
    /// than assuming it.
    ///
    /// **One value or none**, like the [`Sharing`](crate::sites::Sharing) row it is built from: an
    /// address without the name it is advertised under is a pair of fields a reader has to agree
    /// about, and T75 needs both in the same places.
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

        /// Where the activator waits for it, when something can start this pool by connecting to it
        /// — roadmap task **T70**.
        ///
        /// The site names it *after* [`upstream`](Self::PhpFpm::upstream), so a request arriving
        /// while the pool is idle-stopped is retried against it instead of answered with a 502.
        /// [`None`] renders exactly what this site rendered before T70: a home whose pool nothing
        /// can wake is the home it was yesterday.
        activator: Option<Upstream>,
    },

    /// Files, and nothing running.
    Static,

    /// Everything forwarded to an address the user already has listening.
    ReverseProxy {
        /// Scheme, host and port — **and never a path**, roadmap task **T135**.
        ///
        /// The row may hold one, and until T135 it was written into the configuration unchanged:
        /// nginx read it as a prefix replacement and Caddy refused the file outright, which cost
        /// every site on the machine its rendering. `split_upstream` in this module takes it off,
        /// and [`rewrite`](Self::ReverseProxy::rewrite) carries what it meant.
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
/// [`Error::TemplateBroken`], naming `template`.
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
        .map_err(|source| Error::TemplateBroken {
            service: service.as_str().to_owned(),
            file: template,
            source: Box::new(source),
        })
}

/// Every enabled site, joined to the project that gives its doc root a root, with each php-fpm
/// site's pool resolved through `upstreams`.
///
/// **A site whose pool is not in the map is left out**, and that is a decision rather than an
/// oversight — the T43 design, D5. `sites.php_service_id` is `ON DELETE SET NULL` and
/// `service.delete --force` is allowed to cross a site's declaration, so a php-fpm site with no pool
/// is a row this build can produce. Failing the render over one would mean a single `--force` left a
/// daemon that could not render *anything*; serving it any other way would mean answering PHP with
/// something that is not PHP. So it is absent, its file is swept away, a line goes in `daemon.log`,
/// and reporting it to a person is `mix doctor`'s (T47).
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
    // **The blueprint each project came from, beside its root** — roadmap task **T205**, D6: the
    // welcome page lists what that blueprint says to run.
    let rows = sqlx::query!(
        r#"SELECT p.id AS "id!: i64", p.root_path AS "root_path!: String",
                  p.name AS "name!: String", b.manifest_toml AS "manifest?: String"
           FROM projects p LEFT JOIN blueprints b ON b.id = p.blueprint_id"#
    )
    .fetch_all(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    let mut roots: BTreeMap<i64, String> = BTreeMap::new();
    let mut welcome_steps: BTreeMap<i64, Vec<crate::generate::welcome::WelcomeStep>> =
        BTreeMap::new();

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
                    Some(crate::generate::welcome::WelcomeStep {
                        run: step.run?,
                        note: step.note,
                    })
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

        // **The owner's root, whichever owner** — roadmap task **T81b**, the design's D3. Both
        // foreign keys make the misses unreachable through the database's own rules, so reaching one
        // is a row somebody wrote by hand. Said rather than rendered against nothing.
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

                // **A route whose pool has gone loses the route, not the site** — the T135 design,
                // D8. A site with no pool has nothing left to serve and is dropped whole; a route is
                // one prefix of a site that still has a root, a kind and possibly four other
                // routes, and taking it down would turn one `--force` into an outage.
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
                SiteOwner::Project(project) => {
                    welcome_steps.get(project).cloned().unwrap_or_default()
                }
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

/// What this site is served with, or [`None`] — roadmap task **T51**.
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
mod tests {
    use super::*;
    use crate::Paths;
    use crate::config::PathOverrides;

    /// An absolute path on whichever system this is compiled for.
    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\src\blog")
        } else {
            PathBuf::from("/src/blog")
        }
    }

    /// A home with a database and one project in it.
    async fn home() -> (tempfile::TempDir, Store) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let paths = Paths::new(directory.path().to_path_buf(), &PathOverrides::default());
        let store = Store::open(paths.database_file())
            .await
            .expect("a database");

        sqlx::query(
            "INSERT INTO projects (id, name, root_path, created_at)
             VALUES (1, 'blog', ?, '2026-08-23T00:00:00Z')",
        )
        .bind(root().to_string_lossy().into_owned())
        .execute(store.pool())
        .await
        .expect("a project row");

        (directory, store)
    }

    /// One site and its primary domain.
    async fn site(store: &Store, id: i64, domain: &str, doc_root: &str, kind: &str, state: &str) {
        sqlx::query(
            "INSERT INTO sites (id, project_id, doc_root, kind, state) VALUES (?, 1, ?, ?, ?)",
        )
        .bind(id)
        .bind(doc_root)
        .bind(kind)
        .bind(state)
        .execute(store.pool())
        .await
        .expect("a site row");

        sqlx::query("INSERT INTO site_domains (site_id, domain, is_primary) VALUES (?, ?, 1)")
            .bind(id)
            .bind(domain)
            .execute(store.pool())
            .await
            .expect("a domain row");
    }

    /// **One rule** — the T135 design, D2: the matched prefix is replaced by the upstream's path,
    /// trailing slash removed; an upstream with no path replaces nothing.
    ///
    /// **Two regex shapes**, because the strip-to-root case cannot share the other's: `$1` for the
    /// exact prefix is empty there, and a request may not leave with no path at all. Both measured
    /// on Caddy 2.11.4 before either was written.
    #[test]
    fn an_upstream_path_becomes_the_prefix_the_request_leaves_with() {
        assert_eq!(
            split_upstream("/abc", "http://127.0.0.1:3003"),
            ("http://127.0.0.1:3003".to_owned(), None),
            "an upstream with no path replaces nothing"
        );

        assert_eq!(
            split_upstream("/abc", "http://127.0.0.1:3003/xyz"),
            (
                "http://127.0.0.1:3003".to_owned(),
                Some(Rewrite {
                    regex: "^/abc(/.*)?$".to_owned(),
                    replacement: "/xyz$1".to_owned(),
                })
            )
        );

        assert_eq!(
            split_upstream("/abc", "http://127.0.0.1:3003/xyz/"),
            (
                "http://127.0.0.1:3003".to_owned(),
                Some(Rewrite {
                    regex: "^/abc(/.*)?$".to_owned(),
                    replacement: "/xyz$1".to_owned(),
                })
            ),
            "a trailing slash on the upstream's path is not a third behaviour"
        );

        assert_eq!(
            split_upstream("/abc", "http://127.0.0.1:3003/"),
            (
                "http://127.0.0.1:3003".to_owned(),
                Some(Rewrite {
                    regex: "^/abc/?(.*)$".to_owned(),
                    replacement: "/$1".to_owned(),
                })
            ),
            "stripping to the root is the shape that cannot leave an empty path"
        );

        // **A `.` is a regex metacharacter and a path segment may hold one** — the design's D5.
        assert_eq!(
            split_upstream("/v1.0", "http://h:1/x")
                .1
                .expect("a rewrite")
                .regex,
            r"^/v1\.0(/.*)?$"
        );

        // **A whole site is the empty prefix, not `/`** — every path is under it, so what the
        // upstream's path replaces is the string before the first slash.
        assert_eq!(
            split_upstream("", "http://127.0.0.1:3003/xyz"),
            (
                "http://127.0.0.1:3003".to_owned(),
                Some(Rewrite {
                    regex: "^(/.*)?$".to_owned(),
                    replacement: "/xyz$1".to_owned(),
                })
            )
        );
    }

    /// The doc root a template gets is absolute and joined onto the project's root, whatever the row
    /// stores — the row is relative and forward-slashed on every system, and a Caddyfile needs a
    /// path this machine can open.
    #[tokio::test]
    async fn a_doc_root_comes_back_absolute() {
        let (home, store) = home().await;
        site(&store, 1, "blog.test", "web/public", "static", "enabled").await;

        let served = served(
            &store,
            &BTreeMap::new(),
            &home.path().join("certs"),
            &BTreeMap::new(),
        )
        .await
        .expect("the sites read");

        assert_eq!(served.len(), 1);
        assert_eq!(served[0].doc_root, root().join("web").join("public"));
        assert_eq!(served[0].primary(), "blog.test");
    }

    /// A doc root of `""` is the project's root itself, and joining it must not leave a trailing
    /// separator in a web server's configuration.
    #[tokio::test]
    async fn an_empty_doc_root_is_the_project_root_itself() {
        let (home, store) = home().await;
        site(&store, 1, "blog.test", "", "static", "enabled").await;

        let served = served(
            &store,
            &BTreeMap::new(),
            &home.path().join("certs"),
            &BTreeMap::new(),
        )
        .await
        .expect("the sites");

        assert_eq!(served[0].doc_root, root());
    }

    /// "Declared and deliberately not rendered" is the whole of what `Disabled` means: the site is
    /// simply not in the set, so no document is rendered and the sweep removes the file it had.
    #[tokio::test]
    async fn a_disabled_site_is_not_in_the_set() {
        let (home, store) = home().await;
        site(&store, 1, "on.test", "", "static", "enabled").await;
        site(&store, 2, "off.test", "", "static", "disabled").await;

        let served = served(
            &store,
            &BTreeMap::new(),
            &home.path().join("certs"),
            &BTreeMap::new(),
        )
        .await
        .expect("the sites");

        assert_eq!(served.len(), 1);
        assert_eq!(served[0].primary(), "on.test");
    }

    /// A php-fpm site carries the address its pool listens on, resolved through the map the
    /// generator built from every recipe's own answer.
    #[tokio::test]
    async fn a_php_site_carries_the_address_its_pool_listens_on() {
        let (home, store) = home().await;

        sqlx::query(
            r#"INSERT INTO runtime_installs
                   (id, kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256, provides_json)
               VALUES (1, 'php', '8.3.33', 'stable', '/runtimes/php/8.3.33',
                       '2026-08-23T00:00:00Z', 1, 'https://example.invalid/php', 'ab',
                       '{"php-fpm":"sbin/php-fpm"}')"#,
        )
        .execute(store.pool())
        .await
        .expect("a runtime install");

        sqlx::query(
            "INSERT INTO services (id, runtime_install_id, instance_name, state, port)
             VALUES ('php-fpm@8.3.33', 1, '8.3.33', 'stopped', 9000)",
        )
        .execute(store.pool())
        .await
        .expect("a pool row");

        sqlx::query(
            "INSERT INTO sites (id, project_id, doc_root, kind, php_service_id, state)
             VALUES (1, 1, 'public', 'php-fpm', 'php-fpm@8.3.33', 'enabled')",
        )
        .execute(store.pool())
        .await
        .expect("a php site");

        sqlx::query(
            "INSERT INTO site_domains (site_id, domain, is_primary) VALUES (1, 'p.test', 1)",
        )
        .execute(store.pool())
        .await
        .expect("a domain");

        let pool = ServiceId::parse("php-fpm@8.3.33").expect("an id");
        let address = "127.0.0.1:9000".parse().expect("an address");
        let waking = "127.0.0.1:9500".parse().expect("an address");
        let upstreams = BTreeMap::from([(
            pool,
            Upstreams {
                listen: Upstream::Tcp(address),
                activator: Some(Upstream::Tcp(waking)),
            },
        )]);

        let served = served(
            &store,
            &upstreams,
            &home.path().join("certs"),
            &BTreeMap::new(),
        )
        .await
        .expect("the sites");

        assert_eq!(
            served[0].kind,
            ServedKind::PhpFpm {
                upstream: Upstream::Tcp(address),
                activator: Some(Upstream::Tcp(waking)),
            }
        );
    }

    /// D5's honest half: `service.delete --force` is allowed to cross a site's declaration, so a
    /// pool that is gone is a state this build can reach. The site is **left out** rather than
    /// rendered with an empty address and rather than failing the render — a render that failed
    /// would leave a daemon that cannot render anything at all, which is far worse than the one site
    /// it was about. `mix doctor` (T47) is what reports it to a person.
    #[tokio::test]
    async fn a_site_whose_pool_is_gone_is_left_out_rather_than_failing_the_render() {
        let (home, store) = home().await;

        sqlx::query(
            "INSERT INTO sites (id, project_id, doc_root, kind, php_service_id, state)
             VALUES (1, 1, 'public', 'php-fpm', NULL, 'enabled')",
        )
        .execute(store.pool())
        .await
        .expect("a php site with no pool");

        sqlx::query(
            "INSERT INTO site_domains (site_id, domain, is_primary) VALUES (1, 'p.test', 1)",
        )
        .execute(store.pool())
        .await
        .expect("a domain");

        site(&store, 2, "ok.test", "", "static", "enabled").await;

        let served = served(
            &store,
            &BTreeMap::new(),
            &home.path().join("certs"),
            &BTreeMap::new(),
        )
        .await
        .expect("a missing pool does not fail the render");

        assert_eq!(served.len(), 1);
        assert_eq!(served[0].primary(), "ok.test");
    }

    /// A site with a certificate on disk carries the two paths a template has to write, and a
    /// fingerprint — roadmap task **T51**.
    #[tokio::test]
    async fn a_site_with_a_certificate_carries_its_paths_and_fingerprint() {
        let (home, store) = home().await;
        site(&store, 1, "blog.test", "", "static", "enabled").await;

        let certs = home.path().join("certs");
        std::fs::create_dir_all(&certs).expect("the certs directory");
        crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");
        crate::certs::leaf::ensure(
            &certs,
            &["blog.test".to_owned()],
            None,
            std::time::SystemTime::now(),
        )
        .expect("a leaf");

        let served = served(&store, &BTreeMap::new(), &certs, &BTreeMap::new())
            .await
            .expect("the sites are read");

        let certificate = served[0]
            .certificate
            .as_ref()
            .unwrap_or_else(|| panic!("no certificate on a site that has one: {served:?}"));

        assert_eq!(
            certificate.certificate,
            crate::certs::leaf::certificate_path(&certs, "blog.test")
        );
        assert_eq!(
            certificate.key,
            crate::certs::leaf::key_path(&certs, "blog.test")
        );
        assert_eq!(certificate.fingerprint.len(), 64);
    }

    /// **`https_redirect` mirrors the record, plainly** — roadmap task **T98**. No transformation
    /// happens between the column and this field, unlike `certificate`, which is why this is its own
    /// test rather than folded into the certificate ones above: a regression here would not be one a
    /// missing or present `certificate` would catch.
    #[tokio::test]
    async fn https_redirect_mirrors_the_record() {
        let (home, store) = home().await;
        site(&store, 1, "blog.test", "", "static", "enabled").await;
        sqlx::query("UPDATE sites SET https_redirect = 1 WHERE id = 1")
            .execute(store.pool())
            .await
            .expect("the row is updated");

        let served = served(
            &store,
            &BTreeMap::new(),
            &home.path().join("certs"),
            &BTreeMap::new(),
        )
        .await
        .expect("the sites are read");

        assert!(served[0].https_redirect, "{served:?}");
    }

    /// **And carries no certificate along with it when there is none to carry** — T98 read beside
    /// T51's D4. A site can declare the redirect and still have nothing on disk to serve HTTPS
    /// with; the two recipes gate the redirect on both being true together for exactly this reason.
    #[tokio::test]
    async fn a_redirecting_site_with_no_certificate_still_carries_none() {
        let (home, store) = home().await;
        site(&store, 1, "blog.test", "", "static", "enabled").await;
        sqlx::query("UPDATE sites SET https_redirect = 1 WHERE id = 1")
            .execute(store.pool())
            .await
            .expect("the row is updated");

        let served = served(
            &store,
            &BTreeMap::new(),
            &home.path().join("certs"),
            &BTreeMap::new(),
        )
        .await
        .expect("the sites are read");

        assert!(served[0].https_redirect, "{served:?}");
        assert!(served[0].certificate.is_none(), "{served:?}");
    }

    /// **A site with no certificate is `None` and not an error** — the T51 design, D4. This is the
    /// state a home with no authority is in, and rendering has to keep working there.
    #[tokio::test]
    async fn a_site_with_no_certificate_carries_none() {
        let (home, store) = home().await;
        site(&store, 1, "blog.test", "", "static", "enabled").await;

        let served = served(
            &store,
            &BTreeMap::new(),
            &home.path().join("certs"),
            &BTreeMap::new(),
        )
        .await
        .expect("the sites are read");

        assert!(served[0].certificate.is_none(), "{served:?}");
    }

    /// **Half a pair is `None` too.** `leaf::read` answers `Unusable` for a certificate with no key,
    /// and a template that wrote `tls` against it would fail `caddy validate` — which is the whole
    /// reason this reads through `leaf::read` rather than asking whether a file exists.
    #[tokio::test]
    async fn a_site_with_half_a_pair_carries_none() {
        let (home, store) = home().await;
        site(&store, 1, "blog.test", "", "static", "enabled").await;

        let certs = home.path().join("certs");
        std::fs::create_dir_all(certs.join("sites")).expect("the sites directory");
        std::fs::write(
            crate::certs::leaf::certificate_path(&certs, "blog.test"),
            "not a certificate",
        )
        .expect("written");

        let served = served(&store, &BTreeMap::new(), &certs, &BTreeMap::new())
            .await
            .expect("the sites are read");

        assert!(served[0].certificate.is_none(), "{served:?}");
    }

    /// A site that does not declare HTTPS carries none whatever is on disk — the certificate is a
    /// property of what the site asked for, not of what a previous declaration left behind.
    #[tokio::test]
    async fn a_site_that_declares_no_https_carries_none() {
        let (home, store) = home().await;
        site(&store, 1, "blog.test", "", "static", "enabled").await;
        sqlx::query("UPDATE sites SET https_enabled = 0 WHERE id = 1")
            .execute(store.pool())
            .await
            .expect("the row is updated");

        let certs = home.path().join("certs");
        std::fs::create_dir_all(&certs).expect("the certs directory");
        crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");
        crate::certs::leaf::ensure(
            &certs,
            &["blog.test".to_owned()],
            None,
            std::time::SystemTime::now(),
        )
        .expect("a leaf");

        let served = served(&store, &BTreeMap::new(), &certs, &BTreeMap::new())
            .await
            .expect("the sites are read");

        assert!(served[0].certificate.is_none(), "{served:?}");
    }

    /// **T81b, D3.** An extension's site is rooted at its install directory, joined component by
    /// component like a project's.
    #[tokio::test]
    async fn an_extension_site_is_rooted_at_its_install_dir() {
        let (home, store) = home().await;
        let install_dir = if cfg!(windows) {
            PathBuf::from(r"C:\mixengine\extensions\phpmyadmin")
        } else {
            PathBuf::from("/mixengine/extensions/phpmyadmin")
        };

        sqlx::query(
            "INSERT INTO extensions (id, name, version, kind, manifest_json, install_dir, data_dir,
                                     source, signed, installed_at)
             VALUES ('phpmyadmin', 'phpMyAdmin', '5.2.1', 'web-app', '{}', ?, '/data/x', 'path',
                     0, '2026-09-03T00:00:00Z')",
        )
        .bind(install_dir.to_string_lossy().into_owned())
        .execute(store.pool())
        .await
        .expect("an extension row");
        sqlx::query(
            "INSERT INTO sites (id, extension_id, doc_root, kind, state)
             VALUES (7, 'phpmyadmin', 'app/public', 'static', 'enabled')",
        )
        .execute(store.pool())
        .await
        .expect("a site row");
        sqlx::query(
            "INSERT INTO site_domains (site_id, domain, is_primary)
             VALUES (7, 'phpmyadmin.mixengine.test', 1)",
        )
        .execute(store.pool())
        .await
        .expect("a domain row");

        let manifest = crate::extensions::manifest::read(
            Path::new("extension.toml"),
            mixengine_testkit::extension::PHPMYADMIN,
        )
        .expect("the fixture parses");
        let installed = BTreeMap::from([(
            "phpmyadmin".to_owned(),
            crate::extensions::store::Installed {
                id: manifest.extension.id.clone(),
                manifest,
                install_dir: install_dir.clone(),
                data_dir: PathBuf::from("/data/x"),
                source: crate::extensions::store::Source::Path,
                signed: false,
                installed_at: mixengine_proto::Timestamp(0),
                ports: BTreeMap::new(),
            },
        )]);

        let served = served(
            &store,
            &BTreeMap::new(),
            &home.path().join("certs"),
            &installed,
        )
        .await
        .expect("the sites");

        assert_eq!(served.len(), 1);
        assert_eq!(served[0].doc_root, install_dir.join("app").join("public"));
        assert_eq!(served[0].primary(), "phpmyadmin.mixengine.test");
    }
}
