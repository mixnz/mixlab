//! The three site tables: what is served out of a project's directory, and at what name.
//!
//! Roadmap task **T39a**. [`crate::projects`]' shape one table down, with the difference that makes
//! this module worth having: a site is *three* rows in three tables, and they are only ever right
//! together. `0001_initial.sql` says so at `site_domains_one_primary_per_site` — "at least one is
//! not expressible here … and it stays an invariant the site module upholds inside the transaction
//! that creates a site". This is that module and that transaction.
//!
//! # One place reads these tables
//!
//! `projects.rs` does not gain a `sites_of` and `services.rs` does not gain a `sites_declaring`,
//! though a site belongs to a project and declares services. Both questions are reads of `sites`,
//! and a second door onto a table is a second answer to a question that has one — the rule that put
//! the project walk in one place in the first place. They are [`records`] and [`declaring`] here.
//!
//! # Domains are an ordered list, and the head is the primary
//!
//! `is_primary` never leaves this module. Above it a site has a `Vec<String>` whose first element is
//! the primary, so "no primary" and "two primaries" are not states anything can spell. A replacement
//! deletes every row before it inserts any, which is what lets a domain move from one site to
//! another in two calls rather than being blocked by the unique index halfway through (spec D6).

use std::path::{Component, Path, PathBuf};

use mixengine_platform::paths::in_full;
use mixengine_proto::{ExtensionId, RouteTarget, ServiceId, SiteKind, SiteRoute, SiteState};
use sqlx::Sqlite;

use crate::{Error, Result, Store};

/// Who a site belongs to, which is also what gives its `doc_root` a root — roadmap task **T81b**.
///
/// **One of two, and never neither**: `0001_initial.sql` holds that with a CHECK, and this
/// type is what makes it unrepresentable above the row. A project's site is rooted at
/// `projects.root_path`; an extension's at `extensions.install_dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteOwner {
    /// A registered project, by rowid.
    Project(i64),

    /// An installed `web-app` extension.
    Extension(ExtensionId),
}

impl SiteOwner {
    /// The extension that owns this site, when one does — roadmap task **T82a**.
    #[must_use]
    pub fn extension(&self) -> Option<&ExtensionId> {
        match self {
            Self::Extension(id) => Some(id),
            Self::Project(_) => None,
        }
    }

    /// The two columns, exactly one of them set.
    fn columns(&self) -> (Option<i64>, Option<String>) {
        match self {
            Self::Project(id) => (Some(*id), None),
            Self::Extension(id) => (None, Some(id.as_str().to_owned())),
        }
    }

    /// The two columns read back, or the refusal for a row the CHECK should have made impossible.
    fn read(site: i64, project: Option<i64>, extension: Option<String>) -> Result<Self> {
        match (project, extension) {
            (Some(project), None) => Ok(Self::Project(project)),
            (None, Some(extension)) => ExtensionId::parse(extension.clone())
                .map(Self::Extension)
                .map_err(|_| {
                    Error::Project(crate::ProjectError::UnreadableSiteRow {
                        site,
                        column: "extension_id",
                        value: extension,
                    })
                }),
            (project, extension) => Err(Error::Project(crate::ProjectError::UnreadableSiteRow {
                site,
                column: "project_id",
                value: format!("{project:?} beside extension_id {extension:?}"),
            })),
        }
    }
}

/// One site, whole: the row, its ordered domains and its links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteRecord {
    /// The rowid, which stays inside this crate: the wire handle is a domain (spec D5).
    pub id: i64,

    /// Who it belongs to, and therefore what `doc_root` is relative to.
    pub owner: SiteOwner,

    /// Relative to the owner's root. `""` is the root itself.
    pub doc_root: String,

    /// What it serves.
    pub kind: SiteKind,

    /// Whether HTTPS is declared.
    pub https_enabled: bool,

    /// Whether the plaintext address redirects to the HTTPS one — roadmap task **T98**.
    ///
    /// **Never `true` while [`https_enabled`](Self::https_enabled) is `false`** —
    /// `sites.https_redirect`'s own `CHECK` refuses the row, so this field being `true` is
    /// itself a guarantee the site declares HTTPS, not a second fact to check it against.
    pub https_redirect: bool,

    /// Whether the web server should serve it.
    pub state: SiteState,

    /// Every domain, the primary first.
    pub domains: Vec<String>,

    /// The services it declares, in id order.
    pub services: Vec<ServiceId>,

    /// The routes it declares, in the order they were written down — roadmap task **T135**.
    ///
    /// **Declaration order and not match order.** Which route wins an overlap is decided by
    /// [`by_specificity`] where a rendering or an answer needs it; the row keeps what somebody
    /// typed, because that is the only thing a `position` column can honestly hold.
    pub routes: Vec<SiteRoute>,

    /// Where the local network reaches it, when it does — roadmap task **T74**.
    pub sharing: Option<Sharing>,
}

/// A site's LAN sharing, as the row holds it — roadmap task **T74**.
///
/// **One value or none, never three columns a reader has to agree about.** The schema enforces that
/// with a trigger (`0001_initial.sql`); this type is what makes it unrepresentable in the code
/// above it, so nothing has to check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sharing {
    /// The interface, by the name the OS gives it.
    pub interface: String,

    /// The IPv4 address bound, certified and printed.
    pub address: std::net::Ipv4Addr,

    /// When sharing began.
    pub since: mixengine_proto::Timestamp,

    /// When this share ends by itself, or [`None`] for one that does not — roadmap task **T76**.
    ///
    /// **Measured from [`since`](Self::since) and stored as the instant it lands on** — the T76
    /// design, D6. A length without the start it was measured from is not a deadline, and `--for`
    /// is a property of the share rather than of the command that set it: sharing an already-shared
    /// site again neither restarts the clock nor removes the alarm.
    ///
    /// The one of the four that is optional on its own. `0001_initial.sql` holds the
    /// other half — a site that is not shared carries no deadline either.
    pub until: Option<mixengine_proto::Timestamp>,
}

/// Everything creating a site has to write down.
///
/// Domains arrive already normalised by [`crate::domains::normalised`] — this module writes what it
/// is given, because a policy applied here as well would be a second place to change it.
#[derive(Debug, Clone)]
pub struct NewSite {
    /// Who it belongs to.
    pub owner: SiteOwner,
    /// Relative to the root, already made so by [`relative_doc_root`].
    pub doc_root: String,
    /// What it serves.
    pub kind: SiteKind,
    /// Whether HTTPS is declared.
    pub https_enabled: bool,
    /// Whether the plaintext address redirects to the HTTPS one — roadmap task **T98**. Refused by
    /// [`create`] when `true` beside `https_enabled: false`, on the same invariant the schema holds.
    pub https_redirect: bool,
    /// Ordered; the head becomes the primary. Must not be empty.
    pub domains: Vec<String>,
    /// The services it declares.
    pub services: Vec<ServiceId>,
    /// The routes it declares — roadmap task **T135**. May be empty, which is every site made
    /// before this task existed.
    pub routes: Vec<SiteRoute>,
}

/// What an update is changing, where [`None`] means "leave it".
#[derive(Debug, Clone, Default)]
pub struct Change {
    /// A new doc root, already relative.
    pub doc_root: Option<String>,
    /// A new kind.
    pub kind: Option<SiteKind>,
    /// Whether HTTPS is declared.
    pub https_enabled: Option<bool>,
    /// Whether the plaintext address redirects to the HTTPS one — roadmap task **T98**.
    ///
    /// **Not quite "leave it" on its own.** [`update`] turns this off without being asked when
    /// `https_enabled` is turning off and this is [`None`] and the site's redirect is currently on —
    /// a stale `true` beside a plaintext-only site is a row the schema's `CHECK` refuses to hold,
    /// for a reason the caller's own request never mentioned.
    pub https_redirect: Option<bool>,
    /// Whether the web server should serve it.
    pub state: Option<SiteState>,
    /// The domains, **replacing** the list. The head becomes the primary.
    pub domains: Option<Vec<String>>,
    /// The links, **replacing** them.
    pub services: Option<Vec<ServiceId>>,
    /// The routes, **replacing** them — roadmap task **T135**.
    pub routes: Option<Vec<SiteRoute>>,
}

/// A doc root as the row holds it: relative to the project's root, `""` for the root itself.
///
/// **Both sides are normalised** before they are compared, which is the rule T39/D5 paid for: a
/// project registered at `/tmp/blog` and a doc root typed as `/private/tmp/blog/public` are one
/// directory on macOS, and a test using a temporary directory meets that on the first run.
///
/// # Errors
///
/// [`ProjectError::DocRootOutsideProject`] for a path that resolves outside the root.
///
/// [`ProjectError::DocRootOutsideProject`]: crate::ProjectError::DocRootOutsideProject
pub fn relative_doc_root(root: &Path, doc_root: &str) -> Result<String> {
    let full = in_full(root);
    let candidate = Path::new(doc_root);

    // `in_full` resolves aliases over the prefix that exists; it does not fold `..`, and on Windows
    // it cannot — `GetLongPathNameW` expands short names and leaves the rest as it was handed in. A
    // `..` left in would strip against the root and answer with a path pointing outside it, so the
    // dot segments come off here, after the aliases have gone and against a prefix that is real.
    let joined = without_dot_segments(&match candidate.is_absolute() {
        true => in_full(candidate),
        false => in_full(&full.join(candidate)),
    });

    let relative = joined.strip_prefix(&full).map_err(|_| {
        Error::Project(crate::ProjectError::DocRootOutsideProject {
            doc_root: doc_root.to_owned(),
            root: full.display().to_string(),
        })
    })?;

    // Stored with forward slashes whatever this OS writes, because the value is rendered into a
    // web server's configuration and read by a person on a machine that may not be this one.
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

/// `.` and `..` folded away, so a path can be compared with the root it is supposed to be under.
fn without_dot_segments(path: &Path) -> PathBuf {
    let mut folded = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            // `pop` answers false at a root, where `..` is the root itself and there is nothing to
            // fold — keeping it there is what makes the comparison above refuse rather than accept.
            Component::ParentDir if !folded.pop() => folded.push(".."),
            Component::ParentDir => {}
            other => folded.push(other.as_os_str()),
        }
    }

    folded
}

/// Write a site, its domains and its links, or write none of them.
///
/// # Errors
///
/// [`ProjectError::DomainTaken`] naming the site already holding one of the domains,
/// [`ProjectError::HttpsRedirectNeedsHttps`] for `https_redirect: true` beside `https_enabled: false`, and
/// [`Error::Database`] when the write cannot be made.
///
/// [`ProjectError::DomainTaken`]: crate::ProjectError::DomainTaken
/// [`ProjectError::HttpsRedirectNeedsHttps`]: crate::ProjectError::HttpsRedirectNeedsHttps
pub async fn create(store: &Store, new: &NewSite) -> Result<SiteRecord> {
    // Checked before anything opens a transaction: this is a fact about the two fields `new` already
    // carries, not about a row that could change underneath a concurrent writer — roadmap task
    // **T98**. `sites.https_redirect`'s `CHECK` would refuse the `INSERT` below just as
    // surely, in the words it was written in rather than the ones a caller asked in.
    if new.https_redirect && !new.https_enabled {
        return Err(Error::Project(crate::ProjectError::HttpsRedirectNeedsHttps));
    }

    // `BEGIN IMMEDIATE` for `crate::services::transition`'s reason: a deferred `BEGIN` would leave
    // the first `INSERT` to upgrade a read snapshot into a write and fail unrecoverably against a
    // concurrent writer.
    let mut tx = store
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|source| store.failure("write", source))?;

    // **A pool an extension owns serves that extension and nothing else** — roadmap task **T82a**,
    // that design's D5. Inside the transaction, so an extension installed between the read and the
    // insert cannot open a window.
    pool_is_free_for(store, &mut *tx, new.owner.extension(), &new.kind).await?;

    // Before the write and inside the transaction, on the line above's reasoning — and before
    // `write_links` rather than inside it, so the pool and the links are answered for in one place
    // and in the order the caller asked them.
    declared_services_exist(store, &mut *tx, &new.kind, &new.services).await?;

    let (kind, pool) = columns(&new.kind);
    let config = payload(&new.kind);

    // `last_insert_rowid` rather than `RETURNING id`, on `crate::projects::create`'s precedent: the
    // column is an `INTEGER PRIMARY KEY`, which `RETURNING` types as nullable and this never is.
    let (project, extension) = new.owner.columns();

    let inserted = sqlx::query!(
        "INSERT INTO sites (project_id, extension_id, doc_root, kind, php_service_id, https_enabled,
                            https_redirect, config_json, state)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'enabled')",
        project,
        extension,
        new.doc_root,
        kind,
        pool,
        new.https_enabled,
        new.https_redirect,
        config,
    )
    .execute(&mut *tx)
    .await
    .map_err(|source| store.failure("write", source))?;

    let id = inserted.last_insert_rowid();

    write_domains(store, &mut tx, id, &new.domains).await?;
    write_links(store, &mut tx, id, &new.services).await?;
    write_routes(store, &mut tx, id, &new.routes).await?;

    tx.commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    tracing::info!(site = id, domain = %new.domains[0], "a site was created");

    Ok(SiteRecord {
        id,
        owner: new.owner.clone(),
        doc_root: new.doc_root.clone(),
        kind: new.kind.clone(),
        https_enabled: new.https_enabled,
        https_redirect: new.https_redirect,
        state: SiteState::Enabled,
        domains: new.domains.clone(),
        services: new.services.clone(),
        routes: new.routes.clone(),
        // A site is created unshared, always. Sharing is a thing a person turns on afterwards for
        // a site they are looking at, and a create that could take one would be a create that could
        // open a firewall rule as a side effect of an import.
        sharing: None,
    })
}

/// Every site, or every site of one project, in primary-domain order.
///
/// # Errors
///
/// [`Error::Database`] when the tables cannot be read, and [`ProjectError::UnreadableSiteRow`] for a row
/// this build cannot make a [`SiteKind`] of.
///
/// [`ProjectError::UnreadableSiteRow`]: crate::ProjectError::UnreadableSiteRow
pub async fn records(store: &Store, project: Option<i64>) -> Result<Vec<SiteRecord>> {
    let rows = sqlx::query!(
        "SELECT id, project_id, extension_id, doc_root, kind, php_service_id, https_enabled,
                https_redirect, config_json, state, shared_interface, shared_address, shared_since,
                shared_until
         FROM sites
         WHERE ?1 IS NULL OR project_id = ?1
         ORDER BY id",
        project
    )
    .fetch_all(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    let mut sites = Vec::with_capacity(rows.len());

    for row in rows {
        let domains = sqlx::query_scalar!(
            "SELECT domain FROM site_domains WHERE site_id = ? ORDER BY is_primary DESC, domain",
            row.id
        )
        .fetch_all(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

        let linked = sqlx::query_scalar!(
            "SELECT service_id FROM site_service_links WHERE site_id = ? ORDER BY service_id",
            row.id
        )
        .fetch_all(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

        let mut services = Vec::with_capacity(linked.len());

        for id in linked {
            services.push(ServiceId::parse(&id).map_err(|_| {
                Error::Project(crate::ProjectError::UnreadableSiteRow {
                    site: row.id,
                    column: "service_id",
                    value: id.clone(),
                })
            })?);
        }

        let declared = sqlx::query!(
            "SELECT path, target, php_service_id, config_json FROM site_routes
             WHERE site_id = ? ORDER BY position",
            row.id
        )
        .fetch_all(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

        let mut routes = Vec::with_capacity(declared.len());

        for route in declared {
            routes.push(SiteRoute {
                path: route.path,
                target: read_route_target(
                    row.id,
                    &route.target,
                    route.php_service_id,
                    &route.config_json,
                )?,
            });
        }

        sites.push(SiteRecord {
            id: row.id,
            owner: SiteOwner::read(row.id, row.project_id, row.extension_id)?,
            doc_root: row.doc_root,
            kind: read_kind(row.id, &row.kind, row.php_service_id, &row.config_json)?,
            https_enabled: row.https_enabled != 0,
            https_redirect: row.https_redirect != 0,
            state: read_state(row.id, &row.state)?,
            domains,
            services,
            routes,
            sharing: read_sharing(
                row.id,
                row.shared_interface,
                row.shared_address,
                row.shared_since,
                row.shared_until,
            )?,
        });
    }

    sites.sort_by(|left, right| left.domains.first().cmp(&right.domains.first()));

    Ok(sites)
}

/// Every service the sites of one project declare, in id order — roadmap task **T125**.
///
/// **The links, plus the pool a php-fpm site names.** `site_service_links` holds what a site was
/// created *with* — for `blueprint.apply` that is `context.ensured`, the database and the cache it
/// made or found — while the pool lives in `sites.php_service_id` and therefore in
/// [`SiteKind::PhpFpm`]. A caller reading only one of the two would start a project's database and
/// leave the thing that runs its code down, or the other way round.
///
/// **The front end is not here**, and that is the boundary this function draws: it is not a service
/// any site *declares*, it is the one program every site on the machine is reached through. Which
/// one a home has is `services::front_end::held_by`'s question, and the caller that needs both asks
/// both.
///
/// # Errors
///
/// The errors [`records`] gives.
pub async fn services_of(store: &Store, project: i64) -> Result<Vec<ServiceId>> {
    let mut declared: Vec<ServiceId> = Vec::new();

    for site in records(store, Some(project)).await? {
        declared.extend(site.services);

        if let SiteKind::PhpFpm { pool: Some(pool) } = site.kind {
            declared.push(pool);
        }
    }

    // Two sites of one project share a database far more often than not, and a plan is a set.
    declared.sort();
    declared.dedup();

    Ok(declared)
}

/// The site answering to this domain, primary or alias, or [`None`].
///
/// # Errors
///
/// The errors [`records`] gives.
pub async fn by_domain(store: &Store, domain: &str) -> Result<Option<SiteRecord>> {
    let found = sqlx::query_scalar!("SELECT site_id FROM site_domains WHERE domain = ?", domain)
        .fetch_optional(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    let Some(id) = found else {
        return Ok(None);
    };

    Ok(records(store, None)
        .await?
        .into_iter()
        .find(|site| site.id == id))
}

/// The site an extension is served on, or [`None`] for one that has none — roadmap task **T81b**.
///
/// At most one, which `sites_one_per_extension` enforces; read back through [`records`] for
/// [`by_domain`]'s reason.
///
/// # Errors
///
/// The errors [`records`] gives.
pub async fn of_extension(store: &Store, id: &ExtensionId) -> Result<Option<SiteRecord>> {
    let id_column = id.as_str();

    let found = sqlx::query_scalar!("SELECT id FROM sites WHERE extension_id = ?", id_column)
        .fetch_optional(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    let Some(site) = found else {
        return Ok(None);
    };

    Ok(records(store, None)
        .await?
        .into_iter()
        .find(|record| record.id == site))
}

/// Every extension whose site is frozen on this pool — roadmap task **T81b**, the design's D9.
///
/// **Extensions only.** A project's site on the same pool is protected by its pin through
/// `projects::pins_broken_by`; this is the other half of `runtime.uninstall`'s refusal, and it
/// names the thing a person can act on.
///
/// # Errors
///
/// [`Error::Database`] when the table cannot be read, and [`ProjectError::UnreadableSiteRow`] for an
/// `extension_id` that is not one.
///
/// [`ProjectError::UnreadableSiteRow`]: crate::ProjectError::UnreadableSiteRow
pub async fn frozen_on(store: &Store, pool: &ServiceId) -> Result<Vec<ExtensionId>> {
    let pool_column = pool.as_str();

    let rows = sqlx::query!(
        r#"SELECT id, extension_id AS "extension!: String"
           FROM sites
           WHERE extension_id IS NOT NULL AND php_service_id = ?
           ORDER BY extension_id"#,
        pool_column
    )
    .fetch_all(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    rows.into_iter()
        .map(|row| {
            ExtensionId::parse(row.extension.clone()).map_err(|_| {
                Error::Project(crate::ProjectError::UnreadableSiteRow {
                    site: row.id,
                    column: "extension_id",
                    value: row.extension,
                })
            })
        })
        .collect()
}

/// Apply a change, and answer with the site as it now stands.
///
/// # Errors
///
/// [`Error::NotFound`] for a site that is not there, [`ProjectError::DomainTaken`] naming the site holding
/// a domain being claimed, [`ProjectError::HttpsRedirectNeedsHttps`] for a `https_redirect: Some(true)`
/// that would leave the site with `https_enabled: false`, and [`Error::Database`] when the write
/// cannot be made.
///
/// [`ProjectError::DomainTaken`]: crate::ProjectError::DomainTaken
/// [`ProjectError::HttpsRedirectNeedsHttps`]: crate::ProjectError::HttpsRedirectNeedsHttps
pub async fn update(store: &Store, id: i64, change: &Change) -> Result<SiteRecord> {
    let mut tx = store
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|source| store.failure("write", source))?;

    // Asked inside the transaction so an update against a site that has just gone is a `not_found`
    // rather than a set of `UPDATE`s that quietly touch no rows.
    //
    // **The owner is read with it** — roadmap task **T82a** — because the refusal below is about who
    // this site belongs to, and a second read outside the transaction would be a second answer.
    //
    // **`https_enabled` is read with it too** — roadmap task **T98** — because whether
    // `change.https_redirect` is refusable depends on what HTTPS is *left* as by this same call, and
    // a site this `update` is not touching at all still has to answer that question.
    let existing = sqlx::query!(
        r#"SELECT extension_id AS "extension: String", https_enabled FROM sites WHERE id = ?"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|source| store.failure("read", source))?
    .ok_or_else(|| Error::NotFound {
        kind: "site",
        id: id.to_string(),
    })?;

    let owner = existing
        .extension
        .map(|value| {
            ExtensionId::parse(value.clone()).map_err(|_| {
                Error::Project(crate::ProjectError::UnreadableSiteRow {
                    site: id,
                    column: "extension_id",
                    value,
                })
            })
        })
        .transpose()?;

    // **Resolved together, because the two columns are not independent** — the T98 design, D2.
    //
    // `https_redirect: Some(true)` is refused against whatever HTTPS this same call leaves the site
    // with, whether that comes from `change.https_enabled` turning it off or from the site already
    // being plaintext-only: either way it is the one combination `sites.https_redirect`'s
    // `CHECK` makes unrepresentable, and the caller is told so in words about the request rather
    // than about the row.
    let https_enabled = change.https_enabled.unwrap_or(existing.https_enabled != 0);

    if change.https_redirect == Some(true) && !https_enabled {
        return Err(Error::Project(crate::ProjectError::HttpsRedirectNeedsHttps));
    }

    // **The cascade nobody asked for in so many words.** `https_enabled` turning off while the
    // caller said nothing about the redirect carries it to `false` as well — leaving a stale `true`
    // on a plaintext-only site is not "leave it", it is a row the `CHECK` refuses to hold, which
    // would abort this update's every other field for a reason the request never mentioned.
    let https_redirect = match change.https_redirect {
        Some(value) => Some(value),
        None if change.https_enabled == Some(false) => Some(false),
        None => None,
    };

    // **A pool an extension owns serves that extension and nothing else** — the design's D5. Here
    // as well as in [`create`], because `blueprint.apply` and `site.update` both arrive through this
    // door and neither goes through a CLI.
    if let Some(kind) = &change.kind {
        pool_is_free_for(store, &mut *tx, owner.as_ref(), kind).await?;
    }

    // The same door, so the same check — T122. A kind or a link list left alone names nothing new
    // and is not asked about; what is already on the row was answered for when it was written.
    declared_services_exist(
        store,
        &mut *tx,
        change.kind.as_ref().unwrap_or(&SiteKind::Static),
        change.services.as_deref().unwrap_or_default(),
    )
    .await?;

    if let Some(doc_root) = &change.doc_root {
        sqlx::query!("UPDATE sites SET doc_root = ? WHERE id = ?", doc_root, id)
            .execute(&mut *tx)
            .await
            .map_err(|source| store.failure("write", source))?;
    }

    if let Some(kind) = &change.kind {
        let (word, pool) = columns(kind);
        let config = payload(kind);

        sqlx::query!(
            "UPDATE sites SET kind = ?, php_service_id = ?, config_json = ? WHERE id = ?",
            word,
            pool,
            config,
            id
        )
        .execute(&mut *tx)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    // **Written together when HTTPS is turning off, and that is not an optimisation** — roadmap
    // task **T98**. The schema's `CHECK` is immediate, not deferred: two statements in the obvious
    // order would set `https_enabled = 0` first, against a row whose `https_redirect` is still `1`
    // from before this call, and the database would refuse that write outright — the second
    // statement, the one that would have carried the redirect down with it, is never reached.
    // Turning HTTPS *on* has no such ordering hazard (`https_enabled = 1` alone always satisfies
    // the `CHECK`, whatever `https_redirect` still says), so only this one direction needs it.
    match (change.https_enabled, https_redirect) {
        (Some(false), Some(redirect)) => {
            sqlx::query!(
                "UPDATE sites SET https_enabled = 0, https_redirect = ? WHERE id = ?",
                redirect,
                id
            )
            .execute(&mut *tx)
            .await
            .map_err(|source| store.failure("write", source))?;
        }
        (https, redirect) => {
            if let Some(https) = https {
                sqlx::query!("UPDATE sites SET https_enabled = ? WHERE id = ?", https, id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|source| store.failure("write", source))?;
            }

            if let Some(redirect) = redirect {
                sqlx::query!(
                    "UPDATE sites SET https_redirect = ? WHERE id = ?",
                    redirect,
                    id
                )
                .execute(&mut *tx)
                .await
                .map_err(|source| store.failure("write", source))?;
            }
        }
    }

    if let Some(state) = change.state {
        let word = state.as_str();

        sqlx::query!("UPDATE sites SET state = ? WHERE id = ?", word, id)
            .execute(&mut *tx)
            .await
            .map_err(|source| store.failure("write", source))?;
    }

    if let Some(domains) = &change.domains {
        write_domains(store, &mut tx, id, domains).await?;
    }

    if let Some(services) = &change.services {
        write_links(store, &mut tx, id, services).await?;
    }

    if let Some(routes) = &change.routes {
        write_routes(store, &mut tx, id, routes).await?;
    }

    tx.commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    // Read back rather than assembled here: the row this answers with is the row on disk, and the
    // three tables have to agree about it.
    records(store, None)
        .await?
        .into_iter()
        .find(|site| site.id == id)
        .ok_or_else(|| Error::NotFound {
            kind: "site",
            id: id.to_string(),
        })
}

/// Delete a site, and let the cascade take its domains and its links.
///
/// # Errors
///
/// [`Error::NotFound`] for a site that is not there, and [`Error::Database`] when the row cannot be
/// removed.
pub async fn delete(store: &Store, id: i64) -> Result<()> {
    let removed = sqlx::query!("DELETE FROM sites WHERE id = ?", id)
        .execute(store.pool())
        .await
        .map_err(|source| store.failure("write", source))?;

    match removed.rows_affected() {
        0 => Err(Error::NotFound {
            kind: "site",
            id: id.to_string(),
        }),
        _ => {
            tracing::info!(site = id, "a site was deleted");
            Ok(())
        }
    }
}

/// The primary domains of every site that names this service — as its pool, or as a link.
///
/// **Both, in one answer**, because the refusal that reads it (`service.delete`, spec D4) does not
/// care which way a site named the service: either way, deleting it changes what the next
/// `site.start` would do.
///
/// # Errors
///
/// [`Error::Database`] when the tables cannot be read, and the errors [`records`] gives.
pub async fn declaring(store: &Store, service: &ServiceId) -> Result<Vec<String>> {
    let id = service.as_str();

    let sites = sqlx::query_scalar!(
        "SELECT DISTINCT s.id FROM sites s
         LEFT JOIN site_service_links l ON l.site_id = s.id
         WHERE s.php_service_id = ? OR l.service_id = ?",
        id,
        id
    )
    .fetch_all(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    let known = records(store, None).await?;

    Ok(known
        .into_iter()
        .filter(|site| sites.contains(&site.id))
        .filter_map(|site| site.domains.into_iter().next())
        .collect())
}

/// The `kind` word and the `php_service_id` a kind writes into its row.
fn columns(kind: &SiteKind) -> (&'static str, Option<String>) {
    match kind {
        SiteKind::PhpFpm { pool } => (
            "php-fpm",
            pool.as_ref().map(|pool| pool.as_str().to_owned()),
        ),
        SiteKind::Static => ("static", None),
        SiteKind::ReverseProxy { .. } => ("reverse-proxy", None),
        SiteKind::NodeApp { .. } => ("node-app", None),
    }
}

/// The rest of a kind's payload, as `config_json` holds it.
///
/// Built with `serde_json` so escaping an upstream with a path in it is not this module's problem.
fn payload(kind: &SiteKind) -> String {
    let value = match kind {
        SiteKind::PhpFpm { .. } | SiteKind::Static => serde_json::json!({}),
        SiteKind::ReverseProxy { upstream } => serde_json::json!({ "upstream": upstream }),
        SiteKind::NodeApp { port } => serde_json::json!({ "port": port }),
    };

    value.to_string()
}

/// The `target` word and the `php_service_id` a route's target writes into its row — roadmap task
/// **T135**. [`columns`]' shape one table down.
fn route_columns(target: &RouteTarget) -> (&'static str, Option<String>) {
    match target {
        RouteTarget::Proxy { .. } => ("proxy", None),
        RouteTarget::PhpFpm { pool } => (
            "php-fpm",
            pool.as_ref().map(|pool| pool.as_str().to_owned()),
        ),
        RouteTarget::Static { .. } => ("static", None),
    }
}

/// The rest of a target's payload, as `site_routes.config_json` holds it — [`payload`]'s shape and
/// its reason: `serde_json` escapes an upstream with a path in it so this module never has to.
fn route_payload(target: &RouteTarget) -> String {
    let value = match target {
        RouteTarget::Proxy { upstream } => serde_json::json!({ "upstream": upstream }),
        RouteTarget::PhpFpm { .. } => serde_json::json!({}),
        RouteTarget::Static { root } => serde_json::json!({ "root": root }),
    };

    value.to_string()
}

/// The inverse of [`route_columns`] and [`route_payload`].
fn read_route_target(
    site: i64,
    target: &str,
    pool: Option<String>,
    config: &str,
) -> Result<RouteTarget> {
    let unreadable = |column: &'static str, value: &str| {
        Error::Project(crate::ProjectError::UnreadableSiteRow {
            site,
            column,
            value: value.to_owned(),
        })
    };

    let text = |key: &str| -> Result<String> {
        let payload: serde_json::Value =
            serde_json::from_str(config).map_err(|_| unreadable("config_json", config))?;

        payload
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| unreadable("config_json", config))
    };

    match target {
        "proxy" => Ok(RouteTarget::Proxy {
            upstream: text("upstream")?,
        }),
        "php-fpm" => {
            let pool = pool
                .map(|id| ServiceId::parse(&id).map_err(|_| unreadable("php_service_id", &id)))
                .transpose()?;

            Ok(RouteTarget::PhpFpm { pool })
        }
        "static" => Ok(RouteTarget::Static {
            root: text("root")?,
        }),
        other => Err(unreadable("target", other)),
    }
}

/// Which of two routes a request under both of them belongs to — roadmap task **T135**.
///
/// **Longest first, and the whole order is total.** More segments wins, then more bytes, then the
/// path itself — so two routes never compare equal and a rendering is the same on every run. It is
/// the one place this question is answered: the two front ends resolve an overlap by different
/// native rules (Caddy takes handlers in order, nginx has location precedence), and sorting here
/// means neither is ever asked to.
#[must_use]
pub fn by_specificity(left: &SiteRoute, right: &SiteRoute) -> std::cmp::Ordering {
    let segments = |path: &str| path.split('/').filter(|part| !part.is_empty()).count();

    segments(&right.path)
        .cmp(&segments(&left.path))
        .then_with(|| right.path.len().cmp(&left.path.len()))
        .then_with(|| left.path.cmp(&right.path))
}

/// The inverse of [`columns`] and [`payload`].
fn read_kind(site: i64, kind: &str, pool: Option<String>, config: &str) -> Result<SiteKind> {
    let unreadable = |column: &'static str, value: &str| {
        Error::Project(crate::ProjectError::UnreadableSiteRow {
            site,
            column,
            value: value.to_owned(),
        })
    };

    match kind {
        "php-fpm" => {
            let pool = pool
                .map(|id| ServiceId::parse(&id).map_err(|_| unreadable("php_service_id", &id)))
                .transpose()?;

            Ok(SiteKind::PhpFpm { pool })
        }
        "static" => Ok(SiteKind::Static),
        "reverse-proxy" => {
            let payload: serde_json::Value =
                serde_json::from_str(config).map_err(|_| unreadable("config_json", config))?;

            let upstream = payload
                .get("upstream")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| unreadable("config_json", config))?;

            Ok(SiteKind::ReverseProxy {
                upstream: upstream.to_owned(),
            })
        }
        "node-app" => {
            let payload: serde_json::Value =
                serde_json::from_str(config).map_err(|_| unreadable("config_json", config))?;

            let port = payload
                .get("port")
                .and_then(serde_json::Value::as_u64)
                .and_then(|port| u16::try_from(port).ok())
                .ok_or_else(|| unreadable("config_json", config))?;

            Ok(SiteKind::NodeApp { port })
        }
        other => Err(unreadable("kind", other)),
    }
}

/// The state word, or the refusal for a row the CHECK should have made impossible.
fn read_state(site: i64, state: &str) -> Result<SiteState> {
    match state {
        "enabled" => Ok(SiteState::Enabled),
        "disabled" => Ok(SiteState::Disabled),
        other => Err(Error::Project(crate::ProjectError::UnreadableSiteRow {
            site,
            column: "state",
            value: other.to_owned(),
        })),
    }
}

/// The URL a phone opens, for a site shared at `address` and answered on `port`.
///
/// **HTTP, and http alone until T75** — the T74 design. The certificate does cover this address,
/// but a phone will not trust this home's authority until it has installed it, and the endpoint
/// that serves the authority is T75's. A URL a person is told to open must be one that opens.
///
/// The port is omitted when it is 80, because a URL a person reads off a terminal and types into a
/// phone is a URL worth keeping short.
#[must_use]
pub fn shared_url(address: std::net::Ipv4Addr, port: u16) -> String {
    match port {
        80 => format!("http://{address}"),
        other => format!("http://{address}:{other}"),
    }
}

/// The mDNS name a shared site answers to: `<slug>-mixengine.local` — roadmap task **T75**.
///
/// **One label before `.local`, and that is not a style choice.** mDNS conventions single-label host
/// names under `.local` (RFC 6762 section 3) and Windows' resolver enforces the convention: measured
/// on 2026-08-31, `blog-mixengine.local` resolved to the shared address while
/// `blog.mixengine.local` answered *DNS name does not exist* — same responder, same interface, same
/// minute. The T75 design, D1, which is where the roadmap's and the feature spec's own spelling of
/// this name was overturned.
///
/// **Not `<slug>.local`**, which would also resolve. The flat `.local` namespace is shared with
/// every printer and phone on the Wi-Fi; `-mixengine` is what makes this our name rather than a
/// claim on somebody else's.
///
/// [`None`] where the primary domain has no usable label, which is what stops this ever answering
/// `-mixengine.local`.
#[must_use]
pub fn shared_name(primary: &str) -> Option<String> {
    let label = primary.split('.').next().unwrap_or(primary);

    crate::domains::slug(label).map(|slug| format!("{slug}-mixengine.local"))
}

/// The shared site already answering to `name`, if one is — roadmap task **T75**.
///
/// **Only shared sites collide.** The name is on the network while a site is shared and nowhere
/// else, so `blog.test` and `blog.dev` are an ordinary pair right up until somebody shares the
/// second one. `except` is the site being asked about, which must not collide with itself when it
/// is re-shared or when its domains are updated.
///
/// The T75 design, D2: what comes back is the namesake's primary domain, because the refusal names
/// the site somebody has to unshare.
#[must_use]
pub fn name_taken<'a>(records: &'a [SiteRecord], except: i64, name: &str) -> Option<&'a str> {
    records
        .iter()
        .filter(|record| record.id != except && record.sharing.is_some())
        .find(|record| {
            record
                .domains
                .first()
                .and_then(|primary| shared_name(primary))
                .is_some_and(|taken| taken == name)
        })
        .and_then(|record| record.domains.first().map(String::as_str))
}

/// The port this home's front end answers HTTP on, or 80 where it declares none.
///
/// **The answer port and not the bound one** — T43, D8: on macOS a front end listens on 8080 behind
/// a packet-filter redirect, and what a browser asks for is still 80. A LAN URL is what somebody
/// types, so it carries the number they type.
///
/// # Errors
///
/// [`Error::Database`] when the tables cannot be read.
pub async fn web_port(store: &Store, catalogue: &crate::generate::Catalogue) -> Result<u16> {
    let Some(front_end) = crate::services::front_end::held_by(store, catalogue).await? else {
        return Ok(80);
    };

    let port = sqlx::query_scalar!("SELECT port FROM services WHERE id = ?", front_end)
        .fetch_optional(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?
        .flatten();

    Ok(port.and_then(|port| u16::try_from(port).ok()).unwrap_or(80))
}

/// The three sharing columns as one value, or the refusal for a row the trigger should have made
/// impossible.
///
/// **An unparsable address is a refusal and not a shrug.** The alternative — treating it as not
/// shared — would silently un-share a site whose listener is up and whose firewall rule is open,
/// which is the one direction this must never fail in.
fn read_sharing(
    site: i64,
    interface: Option<String>,
    address: Option<String>,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<Option<Sharing>> {
    let (Some(interface), Some(address), Some(since)) = (interface, address, since) else {
        return Ok(None);
    };

    let parsed = address.parse::<std::net::Ipv4Addr>().map_err(|_| {
        Error::Project(crate::ProjectError::UnreadableSiteRow {
            site,
            column: "shared_address",
            value: address.clone(),
        })
    })?;

    Ok(Some(Sharing {
        interface,
        address: parsed,
        since: mixengine_proto::Timestamp(since),
        until: until.map(mixengine_proto::Timestamp),
    }))
}

/// Write, or clear, a site's sharing — roadmap task **T74**.
///
/// All three columns move together, which is what the trigger in `0001_initial.sql` holds and
/// what [`Sharing`] makes unrepresentable otherwise. [`None`] is the unshare.
///
/// # Errors
///
/// [`Error::Database`] when the row cannot be written.
pub async fn set_sharing(store: &Store, id: i64, sharing: Option<&Sharing>) -> Result<SiteRecord> {
    let interface = sharing.map(|sharing| sharing.interface.clone());
    let address = sharing.map(|sharing| sharing.address.to_string());
    let since = sharing.map(|sharing| sharing.since.0);
    let until = sharing
        .and_then(|sharing| sharing.until)
        .map(|until| until.0);

    sqlx::query!(
        "UPDATE sites
         SET shared_interface = ?1, shared_address = ?2, shared_since = ?3, shared_until = ?4
         WHERE id = ?5",
        interface,
        address,
        since,
        until,
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    record(store, id).await
}

/// One site by rowid, for the writers above that already know it exists.
async fn record(store: &Store, id: i64) -> Result<SiteRecord> {
    records(store, None)
        .await?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| Error::NotFound {
            kind: "site",
            id: id.to_string(),
        })
}

/// Replace a site's domains: every delete before any insert (spec D6).
async fn write_domains(
    store: &Store,
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    site: i64,
    domains: &[String],
) -> Result<()> {
    sqlx::query!("DELETE FROM site_domains WHERE site_id = ?", site)
        .execute(&mut **tx)
        .await
        .map_err(|source| store.failure("write", source))?;

    for (index, domain) in domains.iter().enumerate() {
        let is_primary = i64::from(index == 0);

        let written = sqlx::query!(
            "INSERT INTO site_domains (site_id, domain, is_primary) VALUES (?, ?, ?)",
            site,
            domain,
            is_primary
        )
        .execute(&mut **tx)
        .await;

        if let Err(source) = written {
            // The unique index got there first. Ask who has it, so the answer can name the site
            // rather than say "taken" with nowhere to go and look.
            let holder = holder(store, tx, domain).await?;

            return Err(match holder {
                Some(holder) => Error::Project(crate::ProjectError::DomainTaken {
                    domain: domain.clone(),
                    holder,
                }),
                None => store.failure("write", source),
            });
        }
    }

    Ok(())
}

/// Replace a site's service links, same shape as [`write_domains`].
async fn write_links(
    store: &Store,
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    site: i64,
    services: &[ServiceId],
) -> Result<()> {
    sqlx::query!("DELETE FROM site_service_links WHERE site_id = ?", site)
        .execute(&mut **tx)
        .await
        .map_err(|source| store.failure("write", source))?;

    for service in services {
        let id = service.as_str();

        sqlx::query!(
            "INSERT INTO site_service_links (site_id, service_id) VALUES (?, ?)",
            site,
            id
        )
        .execute(&mut **tx)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    Ok(())
}

/// Replace a site's routes, same shape as [`write_links`] — roadmap task **T135**.
///
/// `position` is the index the caller wrote them in. The unique index refuses a duplicate path; the
/// daemon has already refused one with a sentence naming it, so what arrives here is a database
/// failure rather than an argument the caller can act on.
async fn write_routes(
    store: &Store,
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    site: i64,
    routes: &[SiteRoute],
) -> Result<()> {
    sqlx::query!("DELETE FROM site_routes WHERE site_id = ?", site)
        .execute(&mut **tx)
        .await
        .map_err(|source| store.failure("write", source))?;

    for (index, route) in routes.iter().enumerate() {
        let position = i64::try_from(index).unwrap_or(i64::MAX);
        let (target, pool) = route_columns(&route.target);
        let config = route_payload(&route.target);

        sqlx::query!(
            "INSERT INTO site_routes (site_id, position, path, target, php_service_id, config_json)
             VALUES (?, ?, ?, ?, ?, ?)",
            site,
            position,
            route.path,
            target,
            pool,
            config,
        )
        .execute(&mut **tx)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    Ok(())
}

/// Refuse a pool that belongs to an extension to a site that is not that extension's — roadmap task
/// **T82a**, that design's D5.
///
/// **The instance half is the whole lookup.** `extensions::pools::id` composes `php-fpm@<id>`, so
/// *does this pool belong to an extension* is *is its instance an installed extension* — one query,
/// and exact because the rule is this build's own.
///
/// **Here rather than in the daemon**, because `blueprint.apply` and `domain.add` reach [`update`]
/// without going through a CLI, and a refusal they could cross is no refusal — T81b's D6 at a
/// second field.
///
/// **And it cannot be crossed by ordering.** The pool row exists only while the extension is
/// installed; a site must name a service that exists; and when the extension goes,
/// `sites.php_service_id` is `ON DELETE SET NULL`, so no site keeps a dangling name for a reinstall
/// to adopt.
async fn pool_is_free_for<'c>(
    store: &Store,
    executor: impl sqlx::SqliteExecutor<'c>,
    owner: Option<&ExtensionId>,
    kind: &SiteKind,
) -> Result<()> {
    let SiteKind::PhpFpm { pool: Some(pool) } = kind else {
        return Ok(());
    };

    let Some(instance) = pool.instance() else {
        return Ok(());
    };

    if owner.is_some_and(|owner| owner.as_str() == instance) {
        return Ok(());
    }

    let found = sqlx::query_scalar!("SELECT id FROM extensions WHERE id = ?", instance)
        .fetch_optional(executor)
        .await
        .map_err(|source| store.failure("read", source))?;

    match found {
        Some(extension) => Err(Error::ExtensionPoolNotShared {
            pool: pool.as_str().to_owned(),
            extension,
        }),
        None => Ok(()),
    }
}

/// Refuse a site that names a service nothing answers to — roadmap task **T122**.
///
/// **The schema already refused it; what it could not do was say so.** `sites.php_service_id` and
/// `site_service_links.service_id` are both foreign keys, so an id with no row has never been
/// writable — but SQLite answers `FOREIGN KEY constraint failed` and names neither the column nor
/// the id, and that sentence is what a `blueprint.apply` handed a person after it had downloaded a
/// runtime, created a database and started a web server. [`pool_is_free_for`]'s own doc has
/// asserted *a site must name a service that exists* since T82a; this is that assertion given a
/// voice.
///
/// **Here rather than in the daemon**, for the reason the check above it carries: `blueprint.apply`
/// reaches these rows without going through a CLI, so a refusal only the CLI made would be no
/// refusal at all.
///
/// Inside the caller's transaction, so a service deleted between this read and the insert is still
/// caught — by the foreign key, which is where a race belongs. What this moves is the ordinary
/// case, not the racing one.
async fn declared_services_exist<'c>(
    store: &Store,
    executor: impl sqlx::SqliteExecutor<'c>,
    kind: &SiteKind,
    services: &[ServiceId],
) -> Result<()> {
    let pool = match kind {
        SiteKind::PhpFpm { pool: Some(pool) } => Some(pool),
        _ => None,
    };

    // One query rather than one per id: a site with four links on a cold page cache should not be
    // five round trips to answer "everything is there".
    let wanted: Vec<&ServiceId> = pool.into_iter().chain(services.iter()).collect();

    if wanted.is_empty() {
        return Ok(());
    }

    let mut found = std::collections::BTreeSet::new();
    let mut query = sqlx::QueryBuilder::<Sqlite>::new("SELECT id FROM services WHERE id IN (");
    let mut separated = query.separated(", ");
    for service in &wanted {
        separated.push_bind(service.as_str());
    }
    query.push(")");

    let rows: Vec<String> = query
        .build_query_scalar()
        .fetch_all(executor)
        .await
        .map_err(|source| store.failure("read", source))?;

    found.extend(rows);

    // In the order they were asked for, so the one named is the first a caller would look at — the
    // pool before the links, because a php-fpm site with no pool is the broken half a person is
    // most likely to be staring at.
    for service in wanted {
        if !found.contains(service.as_str()) {
            return Err(Error::NotFound {
                kind: "service",
                id: service.as_str().to_owned(),
            });
        }
    }

    Ok(())
}

/// The primary domain of whichever site owns `domain`.
async fn holder(
    store: &Store,
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    domain: &str,
) -> Result<Option<String>> {
    let owner = sqlx::query_scalar!("SELECT site_id FROM site_domains WHERE domain = ?", domain)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|source| store.failure("read", source))?;

    let Some(site) = owner else {
        return Ok(None);
    };

    sqlx::query_scalar!(
        "SELECT domain FROM site_domains WHERE site_id = ? ORDER BY is_primary DESC, domain LIMIT 1",
        site
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(|source| store.failure("read", source))
}

#[cfg(test)]
mod tests;
