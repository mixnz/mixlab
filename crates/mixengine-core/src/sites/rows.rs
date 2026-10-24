//! The columns a site's kind and routes are written as and read back from, and the writers that
//! replace its domains, links and routes.

use mixengine_proto::{RouteTarget, ServiceId, SiteKind, SiteRoute, SiteState};
use sqlx::Sqlite;

use crate::{Error, ProjectError, Result, Store};

use super::checks::holder;
use super::{SiteRecord, records};

/// The `kind` word and the `php_service_id` a kind writes into its row.
pub(super) fn columns(kind: &SiteKind) -> (&'static str, Option<String>) {
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
pub(super) fn payload(kind: &SiteKind) -> String {
    let value = match kind {
        SiteKind::PhpFpm { .. } | SiteKind::Static => serde_json::json!({}),
        SiteKind::ReverseProxy { upstream } => serde_json::json!({ "upstream": upstream }),
        SiteKind::NodeApp { port } => serde_json::json!({ "port": port }),
    };

    value.to_string()
}

/// The `target` word and the `php_service_id` a route's target writes into its row. [`columns`]'
/// shape one table down.
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
pub(super) fn read_route_target(
    site: i64,
    target: &str,
    pool: Option<String>,
    config: &str,
) -> Result<RouteTarget> {
    let unreadable = |column: &'static str, value: &str| {
        Error::Project(ProjectError::UnreadableSiteRow {
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

/// Which of two routes a request under both of them belongs to.
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
pub(super) fn read_kind(
    site: i64,
    kind: &str,
    pool: Option<String>,
    config: &str,
) -> Result<SiteKind> {
    let unreadable = |column: &'static str, value: &str| {
        Error::Project(ProjectError::UnreadableSiteRow {
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
pub(super) fn read_state(site: i64, state: &str) -> Result<SiteState> {
    match state {
        "enabled" => Ok(SiteState::Enabled),
        "disabled" => Ok(SiteState::Disabled),
        other => Err(Error::Project(ProjectError::UnreadableSiteRow {
            site,
            column: "state",
            value: other.to_owned(),
        })),
    }
}

/// One site by rowid, for the writers above that already know it exists.
pub(super) async fn record(store: &Store, id: i64) -> Result<SiteRecord> {
    records(store, None)
        .await?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| Error::NotFound {
            kind: "site",
            id: id.to_string(),
        })
}

/// Replace a site's domains: every delete before any insert.
pub(super) async fn write_domains(
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
                Some(holder) => Error::Project(ProjectError::DomainTaken {
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
pub(super) async fn write_links(
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

/// Replace a site's routes, same shape as [`write_links`].
///
/// `position` is the index the caller wrote them in. The unique index refuses a duplicate path; the
/// daemon has already refused one with a sentence naming it, so what arrives here is a database
/// failure rather than an argument the caller can act on.
pub(super) async fn write_routes(
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
