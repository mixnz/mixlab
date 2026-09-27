//! The copy of a home's state an uninstall leaves in the folders it keeps — roadmap task **T182h**,
//! spec D5 and D6.
//!
//! Everything MixEngine knows is in `mixengine.db`, inside the home. An uninstall that removes the
//! home but keeps the folders `[paths]` moved writes this copy into each of them, so the next home
//! pointed at those folders can bring back its projects, sites and services.

use std::path::{Path, PathBuf};

use mixengine_proto::Timestamp;
use sqlx::Connection as _;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};

use crate::{Error, Result, Store};

/// The copy's file name, in each kept folder.
pub const FILE_NAME: &str = ".mixengine-state.db";

/// What a copy is renamed to once restored, so it is not offered twice.
pub const RESTORED_NAME: &str = ".mixengine-state.restored.db";

/// Rows that belong to the home being removed rather than to the things kept: its jobs, events and
/// numbers, what was waiting for an elevation prompt, its authority and certificates — the next
/// home makes its own — and extensions, whose files lived in the home. In deletion order: an
/// extension's ports, services and sites before the extension itself.
const LEFT_OUT: &[&str] = &[
    "DELETE FROM jobs",
    "DELETE FROM events",
    "DELETE FROM metrics_minutes",
    "DELETE FROM pending_privileged_ops",
    "DELETE FROM certificates",
    "DELETE FROM ca",
    "DELETE FROM extension_ports",
    "DELETE FROM sites WHERE extension_id IS NOT NULL",
    "DELETE FROM services WHERE extension_id IS NOT NULL",
    "DELETE FROM extensions",
];

/// What a copy holds, for the question *restore from your earlier install?*
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Previous {
    /// The copy.
    pub path: PathBuf,
    /// When it was written.
    pub taken_at: Timestamp,
    /// How many projects it would bring back.
    pub projects: u64,
    /// How many sites.
    pub sites: u64,
    /// How many services.
    pub services: u64,
    /// How many runtimes it names.
    pub runtimes: u64,
    /// How many packages.
    pub packages: u64,
    /// Whether a newer build wrote it, which this one refuses to restore.
    pub newer: bool,
}

/// Write the copy of `store` into `dir`, and answer where it went.
///
/// `VACUUM INTO` a `.partial` beside it, the old home's rows removed from that, then renamed into
/// place — so a file at [`FILE_NAME`] is always a copy that finished. One already there is
/// replaced: it is from an earlier uninstall of an earlier home.
///
/// # Errors
///
/// [`Error::Backup`] when the copy cannot be written, cleaned or renamed.
pub async fn write(store: &Store, dir: &Path) -> Result<PathBuf> {
    let target = dir.join(FILE_NAME);
    let partial = dir.join(format!("{FILE_NAME}.partial"));
    let failed = |source: sqlx::Error| Error::Backup {
        path: target.clone(),
        source,
    };

    if partial.exists() {
        std::fs::remove_file(&partial).map_err(|source| failed(sqlx::Error::Io(source)))?;
    }

    sqlx::query("VACUUM INTO ?")
        .bind(partial.to_string_lossy().into_owned())
        .execute(store.pool())
        .await
        .map_err(failed)?;

    // Its own connection, with foreign keys off: the rows removed are a set chosen here, and a
    // cascade reaching past them would remove what the copy exists to keep.
    let mut copy = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&partial)
            .foreign_keys(false),
    )
    .await
    .map_err(failed)?;
    for statement in LEFT_OUT {
        sqlx::query(*statement)
            .execute(&mut copy)
            .await
            .map_err(failed)?;
    }
    copy.close().await.map_err(failed)?;

    std::fs::rename(&partial, &target).map_err(|source| failed(sqlx::Error::Io(source)))?;

    Ok(target)
}

/// The copy in `dir`, when there is one.
///
/// # Errors
///
/// [`Error::Database`] when a copy is there and cannot be read.
pub async fn read(dir: &Path) -> Result<Option<Previous>> {
    let path = dir.join(FILE_NAME);
    if !path.is_file() {
        return Ok(None);
    }

    summary(&path).await.map(Some)
}

/// What the copy at `path` holds.
async fn summary(path: &Path) -> Result<Previous> {
    let path = path.to_path_buf();
    let metadata = std::fs::metadata(&path).map_err(|source| Error::Io {
        action: "read",
        path: path.clone(),
        source,
    })?;

    let copy = Store::open_read_only(&path).await?;
    let count = |statement: &'static str| {
        let copy = &copy;
        async move {
            sqlx::query_scalar::<_, i64>(statement)
                .fetch_one(copy.pool())
                .await
                .map(|n| u64::try_from(n).unwrap_or(0))
                .map_err(|source| copy.failure("read", source))
        }
    };

    let previous = Previous {
        taken_at: Timestamp::from_system_time(
            metadata
                .modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        ),
        projects: count("SELECT COUNT(*) FROM projects").await?,
        sites: count("SELECT COUNT(*) FROM sites").await?,
        services: count("SELECT COUNT(*) FROM services").await?,
        runtimes: count("SELECT COUNT(*) FROM runtime_installs").await?,
        packages: count("SELECT COUNT(*) FROM packages").await?,
        newer: sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(copy.pool())
            .await
            .map_err(|source| copy.failure("read", source))?
            .unwrap_or(0)
            > crate::store::schema_version(),
        path,
    };
    copy.close().await;

    Ok(previous)
}

/// What a restore brought back, and what it could not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restored {
    /// Projects restored.
    pub projects: u64,
    /// Sites restored.
    pub sites: u64,
    /// Runtime rows restored (not counting ones this home already had).
    pub runtimes: u64,
    /// Package rows restored.
    pub packages: u64,
    /// The ids of the services restored, each left stopped.
    pub services: Vec<String>,
    /// What was left out, one phrase each: a runtime whose folder is gone, and what needed it.
    pub skipped: Vec<String>,
    /// The restored sites that serve HTTPS, whose certificates the new home issues.
    pub https_sites: Vec<i64>,
}

/// Bring the copy at `copy` into `store`, in one transaction.
///
/// Runtimes and packages whose folder still exists are matched with the rows this home already has
/// by (kind, version) and (name, version) — the start may have recorded them already (T182f) — and
/// the copy's integer ids are mapped onto this home's. Services come by their text id and are left
/// stopped; one this home already has is kept. Projects, sites and their domains, routes and links
/// are copied with their own ids, which is why this home may hold none yet. A share is not restored:
/// its firewall rule went with the old home. The copy is renamed [`RESTORED_NAME`] afterwards.
///
/// # Errors
///
/// [`Error::RestoreRefused`] for a copy a newer build wrote and for a home that already has a
/// project, a site or a package's service; [`Error::Database`] when either database cannot be read
/// or written, in which case nothing is kept.
pub async fn restore(store: &Store, copy: &Path) -> Result<Restored> {
    let refuse = |reason: &str| Error::RestoreRefused {
        path: copy.to_path_buf(),
        reason: reason.to_owned(),
    };

    let previous = summary(copy).await?;
    if previous.newer {
        return Err(refuse(
            "a newer MixEngine wrote it; install that version to restore it",
        ));
    }

    if has_things_of_its_own(store).await? {
        return Err(refuse(
            "this home already has projects, sites or services of its own, and a restore only goes \
             into one that has none",
        ));
    }

    let fail = |source| store.failure("restore", source);
    let mut connection = store.pool().acquire().await.map_err(fail)?;
    sqlx::query("ATTACH DATABASE ? AS copy")
        .bind(copy.to_string_lossy().into_owned())
        .execute(&mut *connection)
        .await
        .map_err(fail)?;

    let merged = merge(&mut connection).await;

    // Detached whatever the merge answered: the connection goes back to the pool.
    let detached = sqlx::query("DETACH DATABASE copy")
        .execute(&mut *connection)
        .await
        .map_err(fail);
    let restored = merged.map_err(fail)?;
    detached?;

    std::fs::rename(copy, copy.with_file_name(RESTORED_NAME)).map_err(|source| Error::Io {
        action: "rename",
        path: copy.to_path_buf(),
        source,
    })?;

    Ok(restored)
}

/// Whether `store` has a project, a site or a package's service of its own — a home a restore
/// does not go into, since projects and sites are copied with their own ids.
///
/// # Errors
///
/// [`Error::Database`] when the rows cannot be read.
pub async fn has_things_of_its_own(store: &Store) -> Result<bool> {
    let own: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM projects) + (SELECT COUNT(*) FROM sites)
              + (SELECT COUNT(*) FROM services WHERE package_id IS NOT NULL)",
    )
    .fetch_one(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    Ok(own > 0)
}

/// The merge itself, on one connection with the copy attached, in one transaction.
async fn merge(
    connection: &mut sqlx::SqliteConnection,
) -> std::result::Result<Restored, sqlx::Error> {
    use sqlx::Row as _;

    let mut tx = connection.begin().await?;
    let mut restored = Restored::default();

    // --- runtimes and packages, matched or inserted, their ids mapped ----------------------------
    let mut runtime_ids = std::collections::BTreeMap::<i64, i64>::new();
    for row in
        sqlx::query("SELECT id, kind, version, install_path, is_default FROM copy.runtime_installs")
            .fetch_all(&mut *tx)
            .await?
    {
        let (old, kind, version, path): (i64, String, String, String) = (
            row.get("id"),
            row.get("kind"),
            row.get("version"),
            row.get("install_path"),
        );
        if !Path::new(&path).is_dir() {
            restored
                .skipped
                .push(format!("{kind} {version}: its folder {path} is gone"));
            continue;
        }
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM main.runtime_installs WHERE kind = ? AND version = ?",
        )
        .bind(&kind)
        .bind(&version)
        .fetch_optional(&mut *tx)
        .await?;
        let new = match existing {
            Some(id) => id,
            None => {
                restored.runtimes += 1;
                sqlx::query(
                    "INSERT INTO main.runtime_installs (kind, version, channel, install_path,
                         installed_at, size_bytes, source_url, sha256, is_default, provides_json,
                         extension_dir, extensions_json, extension_choices_json)
                     SELECT kind, version, channel, install_path, installed_at, size_bytes,
                         source_url, sha256, 0, provides_json, extension_dir, extensions_json,
                         extension_choices_json
                       FROM copy.runtime_installs WHERE id = ?",
                )
                .bind(old)
                .execute(&mut *tx)
                .await?
                .last_insert_rowid()
            }
        };
        runtime_ids.insert(old, new);

        let was_default: i64 = row.get("is_default");
        if was_default == 1 {
            // The copy's default, where this home has none for that kind yet.
            sqlx::query(
                "UPDATE main.runtime_installs SET is_default = 1 WHERE id = ?
                   AND NOT EXISTS (SELECT 1 FROM main.runtime_installs
                                    WHERE kind = ? AND is_default = 1)",
            )
            .bind(new)
            .bind(&kind)
            .execute(&mut *tx)
            .await?;
        }
    }

    let mut package_ids = std::collections::BTreeMap::<i64, i64>::new();
    for row in sqlx::query("SELECT id, name, version, install_path FROM copy.packages")
        .fetch_all(&mut *tx)
        .await?
    {
        let (old, name, version, path): (i64, String, String, String) = (
            row.get("id"),
            row.get("name"),
            row.get("version"),
            row.get("install_path"),
        );
        if !Path::new(&path).is_dir() {
            restored
                .skipped
                .push(format!("{name} {version}: its folder {path} is gone"));
            continue;
        }
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT id FROM main.packages WHERE name = ? AND version = ?")
                .bind(&name)
                .bind(&version)
                .fetch_optional(&mut *tx)
                .await?;
        let new = match existing {
            Some(id) => id,
            None => {
                restored.packages += 1;
                sqlx::query(
                    "INSERT INTO main.packages (name, version, install_path, installed_at,
                         source_url, sha256, size_bytes, provides_json)
                     SELECT name, version, install_path, installed_at, source_url, sha256,
                         size_bytes, provides_json
                       FROM copy.packages WHERE id = ?",
                )
                .bind(old)
                .execute(&mut *tx)
                .await?
                .last_insert_rowid()
            }
        };
        package_ids.insert(old, new);
    }

    // --- services, by their own id, stopped ------------------------------------------------------
    for row in sqlx::query(
        "SELECT id, package_id, runtime_install_id FROM copy.services WHERE extension_id IS NULL",
    )
    .fetch_all(&mut *tx)
    .await?
    {
        let id: String = row.get("id");
        let package: Option<i64> = row.get("package_id");
        let runtime: Option<i64> = row.get("runtime_install_id");

        let (package, runtime) = match (package, runtime) {
            (Some(old), _) => match package_ids.get(&old) {
                Some(new) => (Some(*new), None),
                None => {
                    restored
                        .skipped
                        .push(format!("{id}: the package it runs is not here"));
                    continue;
                }
            },
            (None, Some(old)) => match runtime_ids.get(&old) {
                Some(new) => (None, Some(*new)),
                None => {
                    restored
                        .skipped
                        .push(format!("{id}: the runtime it runs on is not here"));
                    continue;
                }
            },
            (None, None) => continue,
        };

        let exists: Option<String> =
            sqlx::query_scalar("SELECT id FROM main.services WHERE id = ?")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_some() {
            continue;
        }

        sqlx::query(
            "INSERT INTO main.services (id, package_id, runtime_install_id, extension_id,
                 instance_name, state, autostart, port, activation_port, bind_addr, data_dir,
                 config_overrides_json, limits_json, idle_minutes, stopped_by, last_started_at,
                 last_exit_code, pid, pid_start_time)
             SELECT id, ?, ?, NULL, instance_name, 'stopped', autostart, port, activation_port,
                 bind_addr, data_dir, config_overrides_json, limits_json, idle_minutes, 'never',
                 NULL, NULL, NULL, NULL
               FROM copy.services WHERE id = ?",
        )
        .bind(package)
        .bind(runtime)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
        restored.services.push(id);
    }

    // --- blueprints, projects, sites and what hangs off them -------------------------------------
    sqlx::query("INSERT OR IGNORE INTO main.blueprints SELECT * FROM copy.blueprints")
        .execute(&mut *tx)
        .await?;

    restored.projects = sqlx::query("INSERT INTO main.projects SELECT * FROM copy.projects")
        .execute(&mut *tx)
        .await?
        .rows_affected();

    restored.sites = sqlx::query(
        "INSERT INTO main.sites (id, project_id, extension_id, doc_root, kind, php_service_id,
             https_enabled, http_port, https_port, config_json, state, shared_interface,
             shared_address, shared_since, shared_until, https_redirect)
         SELECT id, project_id, NULL, doc_root, kind,
             CASE WHEN php_service_id IN (SELECT id FROM main.services) THEN php_service_id END,
             https_enabled, http_port, https_port, config_json, state, NULL, NULL, NULL, NULL,
             https_redirect
           FROM copy.sites WHERE extension_id IS NULL",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    sqlx::query(
        "INSERT INTO main.site_domains SELECT * FROM copy.site_domains
          WHERE site_id IN (SELECT id FROM main.sites)",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO main.site_routes (id, site_id, position, path, target, php_service_id,
             config_json)
         SELECT id, site_id, position, path, target,
             CASE WHEN php_service_id IN (SELECT id FROM main.services) THEN php_service_id END,
             config_json
           FROM copy.site_routes WHERE site_id IN (SELECT id FROM main.sites)",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO main.site_service_links SELECT site_id, service_id FROM copy.site_service_links
          WHERE site_id IN (SELECT id FROM main.sites)
            AND service_id IN (SELECT id FROM main.services)",
    )
    .execute(&mut *tx)
    .await?;

    restored.https_sites =
        sqlx::query_scalar("SELECT id FROM main.sites WHERE https_enabled = 1 ORDER BY id")
            .fetch_all(&mut *tx)
            .await?;

    tx.commit().await?;

    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A home holding one of everything a copy keeps, and two things it leaves out.
    async fn a_home_with_state(dir: &Path) -> Store {
        let store = Store::open(&dir.join("mixengine.db"))
            .await
            .expect("a database");
        for statement in [
            "INSERT INTO packages (id, name, version, install_path, installed_at, source_url, sha256)
             VALUES (1, 'mariadb', '11.4.3', '/packages/mariadb/11.4.3', '2026-09-01T00:00:00Z', 'u', 'ab')",
            "INSERT INTO services (id, package_id, instance_name, state, port)
             VALUES ('mariadb@main', 1, 'main', 'running', 3306)",
            "INSERT INTO projects (id, name, root_path, created_at)
             VALUES (1, 'shop', '/work/shop', '2026-09-01T00:00:00Z')",
            "INSERT INTO sites (id, project_id, doc_root, kind) VALUES (1, 1, 'public', 'static')",
            "INSERT INTO site_domains (site_id, domain, is_primary) VALUES (1, 'shop.test', 1)",
            "INSERT INTO jobs (id, kind, state, started_at) VALUES (1, 'runtime.install', 'running', 1)",
        ] {
            sqlx::query(statement)
                .execute(store.pool())
                .await
                .unwrap_or_else(|error| panic!("{statement}: {error}"));
        }
        store
    }

    #[tokio::test]
    async fn a_copy_keeps_the_home_s_things_and_leaves_its_jobs() {
        let home = tempfile::tempdir().expect("a home");
        let kept = tempfile::tempdir().expect("a kept folder");
        let store = a_home_with_state(home.path()).await;

        let written = write(&store, kept.path()).await.expect("a copy");

        assert_eq!(written, kept.path().join(FILE_NAME));
        let previous = read(kept.path()).await.expect("readable").expect("a copy");
        assert_eq!(
            (
                previous.projects,
                previous.sites,
                previous.services,
                previous.packages
            ),
            (1, 1, 1, 1)
        );
        assert!(!previous.newer);

        let copy = Store::open_read_only(&written).await.expect("the copy");
        let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs")
            .fetch_one(copy.pool())
            .await
            .expect("a count");
        assert_eq!(jobs, 0, "a copy leaves the old home's jobs");
        assert!(
            !kept.path().join(format!("{FILE_NAME}.partial")).exists(),
            "nothing half written is left"
        );
    }

    /// A copy whose package and runtime folders exist, written from a home that also held a php-fpm
    /// pool and a site using both services.
    async fn a_copy(kept: &Path) -> PathBuf {
        let home = tempfile::tempdir().expect("a home");
        let mariadb = kept.join("packages/mariadb/11.4.3");
        let php = kept.join("runtimes/php/8.3.33");
        std::fs::create_dir_all(&mariadb).expect("a package folder");
        std::fs::create_dir_all(&php).expect("a runtime folder");

        let store = Store::open(&home.path().join("mixengine.db"))
            .await
            .expect("a database");
        let statements = [
            format!(
                "INSERT INTO packages (id, name, version, install_path, installed_at, source_url, sha256)
                 VALUES (7, 'mariadb', '11.4.3', '{}', '2026-09-01T00:00:00Z', 'u', 'ab')",
                mariadb.display()
            ),
            format!(
                "INSERT INTO runtime_installs (id, kind, version, channel, install_path, installed_at,
                     size_bytes, source_url, sha256, is_default)
                 VALUES (9, 'php', '8.3.33', 'stable', '{}', '2026-09-01T00:00:00Z', 1, 'u', 'ab', 1)",
                php.display()
            ),
            "INSERT INTO services (id, package_id, instance_name, state, port, pid)
             VALUES ('mariadb@main', 7, 'main', 'running', 3306, 4242)"
                .to_owned(),
            "INSERT INTO services (id, runtime_install_id, instance_name, state)
             VALUES ('php-fpm@8.3', 9, '8.3', 'stopped')"
                .to_owned(),
            "INSERT INTO projects (id, name, root_path, created_at)
             VALUES (3, 'shop', '/work/shop', '2026-09-01T00:00:00Z')"
                .to_owned(),
            "INSERT INTO sites (id, project_id, doc_root, kind, php_service_id, https_enabled,
                 shared_interface, shared_address, shared_since)
             VALUES (5, 3, 'public', 'php-fpm', 'php-fpm@8.3', 1, 'eth0', '192.168.1.2', 1)"
                .to_owned(),
            "INSERT INTO site_domains (site_id, domain, is_primary) VALUES (5, 'shop.test', 1)"
                .to_owned(),
            "INSERT INTO site_service_links (site_id, service_id) VALUES (5, 'mariadb@main')"
                .to_owned(),
        ];
        for statement in &statements {
            sqlx::query(sqlx::AssertSqlSafe(statement.clone()))
                .execute(store.pool())
                .await
                .unwrap_or_else(|error| panic!("{statement}: {error}"));
        }

        let written = write(&store, kept).await.expect("a copy");
        store.close().await;
        written
    }

    async fn fresh() -> (tempfile::TempDir, Store) {
        let home = tempfile::tempdir().expect("a home");
        let store = Store::open(&home.path().join("mixengine.db"))
            .await
            .expect("a database");
        (home, store)
    }

    async fn count(store: &Store, statement: &'static str) -> i64 {
        sqlx::query_scalar(statement)
            .fetch_one(store.pool())
            .await
            .expect("a count")
    }

    #[tokio::test]
    async fn a_restore_brings_back_projects_sites_and_services_stopped_and_unshared() {
        let kept = tempfile::tempdir().expect("kept folders");
        let copy = a_copy(kept.path()).await;
        let (_home, store) = fresh().await;

        let restored = restore(&store, &copy).await.expect("a restore");

        assert_eq!(
            (
                restored.projects,
                restored.sites,
                restored.runtimes,
                restored.packages
            ),
            (1, 1, 1, 1),
            "{restored:?}"
        );
        assert_eq!(restored.services.len(), 2, "{restored:?}");
        assert_eq!(restored.https_sites, [5]);
        assert_eq!(count(&store, "SELECT COUNT(*) FROM site_domains").await, 1);
        assert_eq!(
            count(&store, "SELECT COUNT(*) FROM site_service_links").await,
            1
        );
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM services WHERE state = 'stopped' AND pid IS NULL"
            )
            .await,
            2,
            "a restored service is stopped, with no pid of the old home's"
        );
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM sites WHERE shared_address IS NULL"
            )
            .await,
            1,
            "a share is not restored: its firewall rule went with the old home"
        );
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM runtime_installs WHERE is_default = 1"
            )
            .await,
            1
        );
        assert!(!copy.exists() && copy.with_file_name(RESTORED_NAME).exists());
    }

    /// Review focus 1: a home that already has a project is refused, and nothing is written.
    #[tokio::test]
    async fn a_home_with_a_project_of_its_own_is_refused() {
        let kept = tempfile::tempdir().expect("kept folders");
        let copy = a_copy(kept.path()).await;
        let (_home, store) = fresh().await;
        sqlx::query(
            "INSERT INTO projects (name, root_path, created_at) VALUES ('mine', '/work/mine', 'now')",
        )
        .execute(store.pool())
        .await
        .expect("a project");

        let refused = restore(&store, &copy).await.expect_err("refused");

        assert!(matches!(refused, Error::RestoreRefused { .. }), "{refused}");
        assert_eq!(count(&store, "SELECT COUNT(*) FROM packages").await, 0);
        assert!(copy.exists(), "the copy is left to try again");
    }

    /// Review focus 2: a runtime whose folder is gone is skipped, and so is what needed it.
    #[tokio::test]
    async fn a_runtime_whose_folder_is_gone_is_skipped_with_what_needed_it() {
        let kept = tempfile::tempdir().expect("kept folders");
        let copy = a_copy(kept.path()).await;
        std::fs::remove_dir_all(kept.path().join("runtimes/php/8.3.33")).expect("gone");
        let (_home, store) = fresh().await;

        let restored = restore(&store, &copy).await.expect("a restore");

        assert_eq!(restored.runtimes, 0);
        assert_eq!(
            restored.services.len(),
            1,
            "the pool on that PHP is skipped: {restored:?}"
        );
        assert!(
            restored
                .skipped
                .iter()
                .any(|line| line.contains("php 8.3.33")),
            "{restored:?}"
        );
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM sites WHERE php_service_id IS NULL"
            )
            .await,
            1,
            "the site comes back, without a pool that is not there"
        );
    }

    /// Review focus 3: a row this home already has is matched, not duplicated.
    #[tokio::test]
    async fn rows_this_home_already_has_are_matched_not_duplicated() {
        let kept = tempfile::tempdir().expect("kept folders");
        let copy = a_copy(kept.path()).await;
        let (_home, store) = fresh().await;
        let php = kept.path().join("runtimes/php/8.3.33");
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO runtime_installs (id, kind, version, channel, install_path, installed_at,
                 size_bytes, source_url, sha256, is_default)
             VALUES (1, 'php', '8.3.33', 'stable', '{}', 'now', 1, 'u', 'ab', 1)",
            php.display()
        )))
        .execute(store.pool())
        .await
        .expect("a runtime T182f recorded");
        sqlx::query(
            "INSERT INTO services (id, runtime_install_id, instance_name, state)
             VALUES ('php-fpm@8.3', 1, '8.3', 'stopped')",
        )
        .execute(store.pool())
        .await
        .expect("the pool the start made for it");

        let restored = restore(&store, &copy).await.expect("a restore");

        assert_eq!(
            count(&store, "SELECT COUNT(*) FROM runtime_installs").await,
            1
        );
        assert_eq!(count(&store, "SELECT COUNT(*) FROM services").await, 2);
        assert_eq!(
            restored.services.len(),
            1,
            "only mariadb@main is new: {restored:?}"
        );
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM sites WHERE php_service_id = 'php-fpm@8.3'"
            )
            .await,
            1,
            "the site links to the pool this home already had"
        );
    }

    /// Review focus 4: a copy from a newer build is refused, and nothing is written.
    #[tokio::test]
    async fn a_copy_from_a_newer_build_is_refused() {
        let kept = tempfile::tempdir().expect("kept folders");
        let copy = a_copy(kept.path()).await;
        let writable = Store::open(&copy).await.expect("the copy");
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time)
             VALUES (9999, 'from the future', '2027-01-01', 1, x'00', 0)",
        )
        .execute(writable.pool())
        .await
        .expect("a newer migration");
        writable.close().await;
        let (_home, store) = fresh().await;

        let refused = restore(&store, &copy).await.expect_err("refused");

        assert!(matches!(refused, Error::RestoreRefused { .. }), "{refused}");
        assert_eq!(count(&store, "SELECT COUNT(*) FROM projects").await, 0);
    }

    /// Review focus 5: a restored copy is not offered again.
    #[tokio::test]
    async fn a_restored_copy_is_not_offered_again() {
        let kept = tempfile::tempdir().expect("kept folders");
        let copy = a_copy(kept.path()).await;
        let (_home, store) = fresh().await;

        restore(&store, &copy).await.expect("a restore");

        assert!(read(kept.path()).await.expect("readable").is_none());
    }

    #[tokio::test]
    async fn no_copy_is_none() {
        let kept = tempfile::tempdir().expect("a folder");

        assert!(read(kept.path()).await.expect("readable").is_none());
    }

    #[tokio::test]
    async fn a_copy_from_a_newer_build_says_so() {
        let home = tempfile::tempdir().expect("a home");
        let kept = tempfile::tempdir().expect("a kept folder");
        let store = a_home_with_state(home.path()).await;
        let written = write(&store, kept.path()).await.expect("a copy");

        let copy = Store::open(&written).await.expect("the copy");
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time)
             VALUES (9999, 'from the future', '2027-01-01', 1, x'00', 0)",
        )
        .execute(copy.pool())
        .await
        .expect("a newer migration");
        copy.close().await;

        assert!(
            read(kept.path())
                .await
                .expect("readable")
                .expect("a copy")
                .newer
        );
    }
}
