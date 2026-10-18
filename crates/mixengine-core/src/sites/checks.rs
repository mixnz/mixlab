//! What a site may not be written with: a pool that belongs to another extension, a service nothing
//! answers to.

use mixengine_proto::{ExtensionId, ServiceId, SiteKind};
use sqlx::Sqlite;

use crate::{Error, ExtensionError, Result, Store};

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
///
/// [`update`]: super::update
pub(super) async fn pool_is_free_for<'c>(
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
        Some(extension) => Err(Error::Extension(ExtensionError::ExtensionPoolNotShared {
            pool: pool.as_str().to_owned(),
            extension,
        })),
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
pub(super) async fn declared_services_exist<'c>(
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
pub(super) async fn holder(
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
