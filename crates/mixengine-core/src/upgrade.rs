//! What an update within a line changes in this home's rows — roadmap tasks **T193b** and
//! **T193c**, the design's D5 and D6.
//!
//! **Every write an update makes is here, and each has its reverse.** The daemon orders the steps
//! and talks to processes; what it may never do is leave a site pointing at a pool that is not
//! there, so the one move that touches several tables is one transaction, and [`move_back`] undoes
//! it exactly.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mixengine_proto::{PackageVersion, PinSource, RuntimeKind, ServiceId, VersionConstraint};

use crate::{Paths, Result, Store};

/// The extension choices `to` keeps of `from`'s, and the names its build does not ship.
///
/// A choice is a *deviation* from the build's default (`runtimes::extensions::State::decide`), so
/// one that agrees with the new build's default is not stored, on that function's reasoning.
#[must_use]
pub fn carry_choices(
    choices: &BTreeMap<String, bool>,
    offered: &crate::index::Extensions,
) -> (BTreeMap<String, bool>, Vec<String>) {
    let mut kept = BTreeMap::new();
    let mut dropped = Vec::new();

    for (name, enabled) in choices {
        if !offered.shared.iter().any(|shared| shared == name) {
            dropped.push(name.clone());
            continue;
        }

        let by_default = offered.enabled.iter().any(|on| on == name);
        if *enabled != by_default {
            kept.insert(name.clone(), *enabled);
        }
    }

    (kept, dropped)
}

/// Write `from`'s choices onto `to` and render `to`'s `conf.d`; answers the names dropped.
///
/// # Errors
///
/// [`crate::Error::NotFound`] when either version has no row, and database or file errors.
pub async fn carry_extension_choices(
    store: &Store,
    paths: &Paths,
    kind: RuntimeKind,
    from: &PackageVersion,
    to: &PackageVersion,
) -> Result<Vec<String>> {
    let old = crate::runtimes::extensions::state(store, kind, from).await?;
    let new = crate::runtimes::extensions::state(store, kind, to).await?;
    let (kept, dropped) = carry_choices(&old.choices, &new.offered);

    let encoded = serde_json::to_string(&kept).unwrap_or_else(|_| "{}".to_owned());
    let (kind_column, version_column) = (kind.as_str(), to.as_str());
    sqlx::query!(
        "UPDATE runtime_installs SET extension_choices_json = ? WHERE kind = ? AND version = ?",
        encoded,
        kind_column,
        version_column
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    let state = crate::runtimes::extensions::state(store, kind, to).await?;
    crate::runtimes::extensions::render(paths, &state).await?;

    Ok(dropped)
}

/// Give the pool `to` the settings a person gave `from`: overrides, limits, idle policy, autostart.
///
/// # Errors
///
/// A database error.
pub async fn copy_pool_settings(store: &Store, from: &ServiceId, to: &ServiceId) -> Result<()> {
    let (from, to) = (from.as_str(), to.as_str());
    sqlx::query!(
        "UPDATE services SET
           config_overrides_json = (SELECT config_overrides_json FROM services WHERE id = ?),
           limits_json           = (SELECT limits_json FROM services WHERE id = ?),
           idle_minutes          = (SELECT idle_minutes FROM services WHERE id = ?),
           autostart             = (SELECT autostart FROM services WHERE id = ?)
         WHERE id = ?",
        from,
        from,
        from,
        from,
        to
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    Ok(())
}

/// Record that a person, not the daemon, left this stopped service stopped — so on-demand
/// activation does not wake the new pool of an old pool a person had stopped (D5 step 4).
///
/// # Errors
///
/// A database error.
pub async fn set_stopped_by_person(store: &Store, service: &ServiceId) -> Result<()> {
    let id = service.as_str();
    sqlx::query!(
        "UPDATE services SET stopped_by = 'person' WHERE id = ? AND state = 'stopped'",
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    Ok(())
}

/// A project pin in SQLite that names the old version and not the new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinRewrite {
    /// The project's rowid.
    pub project_id: i64,
    /// The project's name.
    pub project: String,
    /// The pin as it is now.
    pub from: VersionConstraint,
}

/// Every registered pin of `kind` that matches `from` and not `to`.
///
/// # Errors
///
/// A database error, or a pins column this build cannot read.
pub async fn pins_to_rewrite(
    store: &Store,
    kind: RuntimeKind,
    from: &PackageVersion,
    to: &PackageVersion,
) -> Result<Vec<PinRewrite>> {
    Ok(crate::projects::records(store)
        .await?
        .into_iter()
        .filter_map(|project| {
            let pin = project.pins.get(&kind)?;
            (pin.matches(from) && !pin.matches(to)).then(|| PinRewrite {
                project_id: project.id,
                project: project.name.clone(),
                from: pin.clone(),
            })
        })
        .collect())
}

/// A `mixengine.toml` pin that only `from` answers among what is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestPin {
    /// The project.
    pub project: String,
    /// The file.
    pub path: String,
    /// What it pins.
    pub constraint: VersionConstraint,
}

/// Every registered project's `mixengine.toml` pin that `from` satisfies, `to` does not, and no
/// other installed version does either — what keeps `from` installed (D5 step 9).
///
/// # Errors
///
/// A database error, or a manifest that cannot be read.
pub async fn manifest_pins_needing(
    store: &Store,
    kind: RuntimeKind,
    from: &PackageVersion,
    to: &PackageVersion,
) -> Result<Vec<ManifestPin>> {
    let others: Vec<PackageVersion> = crate::runtimes::records(store, Some(kind))
        .await?
        .into_iter()
        .map(|row| row.version)
        .filter(|version| version != from)
        .collect();

    let mut needing = Vec::new();
    for project in crate::projects::records(store).await? {
        for pin in crate::projects::effective_pins(store, &project).await? {
            let PinSource::Manifest { path } = &pin.source else {
                continue;
            };

            if pin.kind == kind
                && pin.constraint.matches(from)
                && !pin.constraint.matches(to)
                && !others.iter().any(|other| pin.constraint.matches(other))
            {
                needing.push(ManifestPin {
                    project: project.name.clone(),
                    path: path.clone(),
                    constraint: pin.constraint.clone(),
                });
            }
        }
    }

    Ok(needing)
}

/// The tools somebody installed into `from` (`npm install -g yarn`) that `to` does not have.
///
/// # Errors
///
/// A database error.
pub async fn tools_only_in(
    store: &Store,
    kind: RuntimeKind,
    from: &PackageVersion,
    to: &PackageVersion,
) -> Result<Vec<String>> {
    let old = installed_tools(store, kind, from).await?;
    let new = installed_tools(store, kind, to).await?;

    Ok(old.difference(&new).cloned().collect())
}

/// What `runtimes::globals::scan` finds in one installed version, or nothing for one not installed.
async fn installed_tools(
    store: &Store,
    kind: RuntimeKind,
    version: &PackageVersion,
) -> Result<BTreeSet<String>> {
    let (kind_column, version_column) = (kind.as_str(), version.as_str());
    let Some(row) = sqlx::query!(
        "SELECT install_path, provides_json FROM runtime_installs WHERE kind = ? AND version = ?",
        kind_column,
        version_column
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    else {
        return Ok(BTreeSet::new());
    };

    let provides: BTreeMap<String, String> =
        serde_json::from_str(&row.provides_json).unwrap_or_default();

    Ok(crate::runtimes::globals::scan(
        kind,
        Path::new(&row.install_path),
        &provides,
    ))
}

/// Everything one runtime update moves in one transaction — D5 step 6.
#[derive(Debug, Clone)]
pub struct RuntimeMove {
    /// Which language.
    pub kind: RuntimeKind,
    /// The version everything leaves.
    pub from: PackageVersion,
    /// The version everything arrives at.
    pub to: PackageVersion,
    /// For PHP: the shared pool of each version, `(from, to)`.
    pub pools: Option<(ServiceId, ServiceId)>,
    /// `web-app` pools whose `requires` `to` satisfies.
    pub extension_pools: Vec<ServiceId>,
    /// The SQLite pins to rewrite.
    pub pins: Vec<PinRewrite>,
}

/// What [`move_runtime`] did, which is what [`move_back`] undoes.
#[derive(Debug, Clone)]
pub struct Moved {
    /// What was asked.
    pub request: RuntimeMove,
    /// The sites whose pool changed.
    pub sites: Vec<i64>,
    /// Whether the default moved.
    pub default_moved: bool,
}

/// Point sites, extension pools, the default and pins at `to`, in one transaction.
///
/// # Errors
///
/// A database error; nothing is written then.
pub async fn move_runtime(store: &Store, request: &RuntimeMove) -> Result<Moved> {
    let projects = crate::projects::records(store).await?;
    let mut transaction = store
        .pool()
        .begin()
        .await
        .map_err(|source| store.failure("write", source))?;

    let mut sites = Vec::new();
    if let Some((from_pool, to_pool)) = &request.pools {
        let (from_pool, to_pool) = (from_pool.as_str(), to_pool.as_str());
        sites = sqlx::query_scalar!("SELECT id FROM sites WHERE php_service_id = ?", from_pool)
            .fetch_all(&mut *transaction)
            .await
            .map_err(|source| store.failure("read", source))?;
        sqlx::query!(
            "UPDATE sites SET php_service_id = ? WHERE php_service_id = ?",
            to_pool,
            from_pool
        )
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    let (kind, from, to) = (
        request.kind.as_str(),
        request.from.as_str(),
        request.to.as_str(),
    );

    for pool in &request.extension_pools {
        let pool = pool.as_str();
        sqlx::query!(
            "UPDATE services SET runtime_install_id =
               (SELECT id FROM runtime_installs WHERE kind = ? AND version = ?)
             WHERE id = ?",
            kind,
            to,
            pool
        )
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    let default_moved = sqlx::query_scalar!(
        "SELECT is_default FROM runtime_installs WHERE kind = ? AND version = ?",
        kind,
        from
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|source| store.failure("read", source))?
        == Some(1);

    if default_moved {
        // Cleared before it is set: the partial unique index allows one default per kind.
        sqlx::query!(
            "UPDATE runtime_installs SET is_default = 0 WHERE kind = ? AND version = ?",
            kind,
            from
        )
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;
        sqlx::query!(
            "UPDATE runtime_installs SET is_default = 1 WHERE kind = ? AND version = ?",
            kind,
            to
        )
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    for rewrite in &request.pins {
        let Some(project) = projects
            .iter()
            .find(|project| project.id == rewrite.project_id)
        else {
            continue;
        };
        let mut pins = project.pins.clone();
        pins.insert(request.kind, VersionConstraint::from(request.to.clone()));
        let encoded = crate::projects::encode(&pins);
        sqlx::query!(
            "UPDATE projects SET runtime_pins_json = ? WHERE id = ?",
            encoded,
            rewrite.project_id
        )
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    transaction
        .commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    Ok(Moved {
        request: request.clone(),
        sites,
        default_moved,
    })
}

/// Undo [`move_runtime`] — D5 step 7, when the front end refuses the new set.
///
/// # Errors
///
/// A database error; nothing is written then.
pub async fn move_back(store: &Store, moved: &Moved) -> Result<()> {
    let request = &moved.request;
    let projects = crate::projects::records(store).await?;
    let mut transaction = store
        .pool()
        .begin()
        .await
        .map_err(|source| store.failure("write", source))?;

    if let Some((from_pool, _)) = &request.pools {
        let from_pool = from_pool.as_str();
        for site in &moved.sites {
            sqlx::query!(
                "UPDATE sites SET php_service_id = ? WHERE id = ?",
                from_pool,
                site
            )
            .execute(&mut *transaction)
            .await
            .map_err(|source| store.failure("write", source))?;
        }
    }

    let (kind, from, to) = (
        request.kind.as_str(),
        request.from.as_str(),
        request.to.as_str(),
    );

    for pool in &request.extension_pools {
        let pool = pool.as_str();
        sqlx::query!(
            "UPDATE services SET runtime_install_id =
               (SELECT id FROM runtime_installs WHERE kind = ? AND version = ?)
             WHERE id = ?",
            kind,
            from,
            pool
        )
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    if moved.default_moved {
        sqlx::query!(
            "UPDATE runtime_installs SET is_default = 0 WHERE kind = ? AND version = ?",
            kind,
            to
        )
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;
        sqlx::query!(
            "UPDATE runtime_installs SET is_default = 1 WHERE kind = ? AND version = ?",
            kind,
            from
        )
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    for rewrite in &request.pins {
        let Some(project) = projects
            .iter()
            .find(|project| project.id == rewrite.project_id)
        else {
            continue;
        };
        let mut pins = project.pins.clone();
        pins.insert(request.kind, rewrite.from.clone());
        let encoded = crate::projects::encode(&pins);
        sqlx::query!(
            "UPDATE projects SET runtime_pins_json = ? WHERE id = ?",
            encoded,
            rewrite.project_id
        )
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    transaction
        .commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    Ok(())
}

/// Point one server instance at another installed version of its package — D6 step 3.
///
/// # Errors
///
/// [`crate::Error::NotFound`] when that version is not installed, and a database error.
pub async fn repoint_instance(
    store: &Store,
    service: &ServiceId,
    package: &str,
    version: &PackageVersion,
) -> Result<()> {
    let (id, version_column) = (service.as_str(), version.as_str());
    let written = sqlx::query!(
        "UPDATE services SET package_id =
           (SELECT id FROM packages WHERE name = ? AND version = ?)
         WHERE id = ?
           AND EXISTS (SELECT 1 FROM packages WHERE name = ? AND version = ?)",
        package,
        version_column,
        id,
        package,
        version_column
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    if written.rows_affected() == 0 {
        return Err(crate::Error::NotFound {
            kind: "package",
            id: format!("{package} {version}"),
        });
    }

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> PackageVersion {
        PackageVersion::parse(text).unwrap()
    }

    fn id(text: &str) -> ServiceId {
        ServiceId::parse(text).unwrap()
    }

    async fn store() -> (tempfile::TempDir, Store) {
        let home = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(&home.path().join(crate::paths::DATABASE_FILE_NAME))
            .await
            .expect("a store");
        (home, store)
    }

    async fn a_php(store: &Store, version: &str, default: bool) {
        sqlx::query(
            "INSERT INTO runtime_installs
               (kind, version, channel, install_path, installed_at, size_bytes, source_url, sha256,
                is_default)
             VALUES ('php', ?, 'stable', ?, '2026-09-29T00:00:00Z', 1, 'u', 's', ?)",
        )
        .bind(version)
        .bind(format!("/runtimes/php/{version}"))
        .bind(i64::from(default))
        .execute(store.pool())
        .await
        .unwrap();
    }

    async fn a_pool(store: &Store, pool: &str, version: &str) {
        sqlx::query(
            "INSERT INTO services (id, runtime_install_id, instance_name, state,
                                   config_overrides_json, idle_minutes, autostart)
             SELECT ?, id, ?, 'stopped', '{\"pm.max_children\":\"9\"}', 7, 1
             FROM runtime_installs WHERE kind = 'php' AND version = ?",
        )
        .bind(pool)
        .bind(pool.split_once('@').unwrap().1)
        .bind(version)
        .execute(store.pool())
        .await
        .unwrap();
    }

    async fn a_project(store: &Store, name: &str, pins: &str) -> i64 {
        sqlx::query(
            "INSERT INTO projects (name, root_path, runtime_pins_json, created_at)
             VALUES (?, ?, ?, '2026-09-29T00:00:00Z')",
        )
        .bind(name)
        .bind(format!("/nonexistent/{name}"))
        .bind(pins)
        .execute(store.pool())
        .await
        .unwrap()
        .last_insert_rowid()
    }

    async fn a_site(store: &Store, project: i64, pool: &str) -> i64 {
        sqlx::query(
            "INSERT INTO sites (project_id, doc_root, kind, php_service_id)
             VALUES (?, '.', 'php-fpm', ?)",
        )
        .bind(project)
        .bind(pool)
        .execute(store.pool())
        .await
        .unwrap()
        .last_insert_rowid()
    }

    async fn pool_of_site(store: &Store, site: i64) -> String {
        sqlx::query_scalar("SELECT php_service_id FROM sites WHERE id = ?")
            .bind(site)
            .fetch_one(store.pool())
            .await
            .unwrap()
    }

    async fn default_php(store: &Store) -> String {
        sqlx::query_scalar(
            "SELECT version FROM runtime_installs WHERE kind = 'php' AND is_default = 1",
        )
        .fetch_one(store.pool())
        .await
        .unwrap()
    }

    fn extensions(shared: &[&str], enabled: &[&str]) -> crate::index::Extensions {
        crate::index::Extensions {
            compiled_in: Vec::new(),
            shared: shared.iter().map(|name| (*name).to_owned()).collect(),
            enabled: enabled.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    #[test]
    fn a_choice_the_new_build_ships_travels_and_one_it_does_not_is_named() {
        let choices = BTreeMap::from([
            ("xdebug".to_owned(), true),
            ("imagick".to_owned(), true),
            ("opcache".to_owned(), false),
        ]);

        let (kept, dropped) =
            carry_choices(&choices, &extensions(&["xdebug", "opcache"], &["opcache"]));

        assert_eq!(
            kept,
            BTreeMap::from([("xdebug".to_owned(), true), ("opcache".to_owned(), false)])
        );
        assert_eq!(dropped, vec!["imagick".to_owned()]);
    }

    #[test]
    fn a_choice_that_agrees_with_the_new_build_s_default_is_not_stored() {
        let choices = BTreeMap::from([("opcache".to_owned(), true)]);
        let (kept, dropped) = carry_choices(&choices, &extensions(&["opcache"], &["opcache"]));
        assert!(kept.is_empty() && dropped.is_empty());
    }

    #[tokio::test]
    async fn a_pool_s_settings_travel_to_the_new_pool() {
        let (_home, store) = store().await;
        a_php(&store, "8.4.24", false).await;
        a_php(&store, "8.4.25", false).await;
        a_pool(&store, "php-fpm@8.4.24", "8.4.24").await;
        sqlx::query(
            "INSERT INTO services (id, runtime_install_id, instance_name, state)
             SELECT 'php-fpm@8.4.25', id, '8.4.25', 'stopped'
             FROM runtime_installs WHERE version = '8.4.25'",
        )
        .execute(store.pool())
        .await
        .unwrap();

        copy_pool_settings(&store, &id("php-fpm@8.4.24"), &id("php-fpm@8.4.25"))
            .await
            .unwrap();

        let (overrides, idle, autostart): (String, Option<i64>, i64) = sqlx::query_as(
            "SELECT config_overrides_json, idle_minutes, autostart FROM services WHERE id = ?",
        )
        .bind("php-fpm@8.4.25")
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(overrides, "{\"pm.max_children\":\"9\"}");
        assert_eq!(idle, Some(7));
        assert_eq!(autostart, 1);
    }

    #[tokio::test]
    async fn only_an_exact_pin_on_the_old_version_is_rewritten() {
        let (_home, store) = store().await;
        a_project(&store, "exact", r#"{"php":"8.4.24"}"#).await;
        a_project(&store, "line", r#"{"php":"8.4"}"#).await;
        a_project(&store, "caret", r#"{"php":"^8.4"}"#).await;

        let rewrites = pins_to_rewrite(&store, RuntimeKind::Php, &v("8.4.24"), &v("8.4.25"))
            .await
            .unwrap();

        assert_eq!(
            rewrites
                .iter()
                .map(|pin| pin.project.as_str())
                .collect::<Vec<_>>(),
            ["exact"]
        );
    }

    #[tokio::test]
    async fn a_move_carries_sites_default_and_pins_and_moving_back_restores_every_row() {
        let (_home, store) = store().await;
        a_php(&store, "8.4.24", true).await;
        a_php(&store, "8.4.25", false).await;
        a_pool(&store, "php-fpm@8.4.24", "8.4.24").await;
        // The install hook makes the new pool before anything moves; a site may only name a real one.
        a_pool(&store, "php-fpm@8.4.25", "8.4.25").await;
        let project = a_project(&store, "blog", r#"{"php":"8.4.24"}"#).await;
        let site = a_site(&store, project, "php-fpm@8.4.24").await;

        let pins = pins_to_rewrite(&store, RuntimeKind::Php, &v("8.4.24"), &v("8.4.25"))
            .await
            .unwrap();
        let moved = move_runtime(
            &store,
            &RuntimeMove {
                kind: RuntimeKind::Php,
                from: v("8.4.24"),
                to: v("8.4.25"),
                pools: Some((id("php-fpm@8.4.24"), id("php-fpm@8.4.25"))),
                extension_pools: Vec::new(),
                pins,
            },
        )
        .await
        .unwrap();

        assert_eq!(pool_of_site(&store, site).await, "php-fpm@8.4.25");
        assert_eq!(default_php(&store).await, "8.4.25");
        assert!(moved.default_moved);
        let blog = crate::projects::records(&store).await.unwrap().remove(0);
        assert_eq!(blog.pins[&RuntimeKind::Php].as_str(), "8.4.25");

        move_back(&store, &moved).await.unwrap();

        assert_eq!(pool_of_site(&store, site).await, "php-fpm@8.4.24");
        assert_eq!(default_php(&store).await, "8.4.24");
        let blog = crate::projects::records(&store).await.unwrap().remove(0);
        assert_eq!(blog.pins[&RuntimeKind::Php].as_str(), "8.4.24");
    }

    /// **Review focus 2.** Two patches of one line, each with its own pool: a move names one pool
    /// and leaves the other's sites alone.
    #[tokio::test]
    async fn a_move_touches_only_the_pool_it_names() {
        let (_home, store) = store().await;
        for version in ["8.4.23", "8.4.24", "8.4.25"] {
            a_php(&store, version, false).await;
        }
        a_pool(&store, "php-fpm@8.4.23", "8.4.23").await;
        a_pool(&store, "php-fpm@8.4.24", "8.4.24").await;
        a_pool(&store, "php-fpm@8.4.25", "8.4.25").await;
        let project = a_project(&store, "two", "{}").await;
        let on_23 = a_site(&store, project, "php-fpm@8.4.23").await;
        let on_24 = a_site(&store, project, "php-fpm@8.4.24").await;

        move_runtime(
            &store,
            &RuntimeMove {
                kind: RuntimeKind::Php,
                from: v("8.4.24"),
                to: v("8.4.25"),
                pools: Some((id("php-fpm@8.4.24"), id("php-fpm@8.4.25"))),
                extension_pools: Vec::new(),
                pins: Vec::new(),
            },
        )
        .await
        .unwrap();

        assert_eq!(pool_of_site(&store, on_23).await, "php-fpm@8.4.23");
        assert_eq!(pool_of_site(&store, on_24).await, "php-fpm@8.4.25");
    }
}
