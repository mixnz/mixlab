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
    let Ok(metadata) = std::fs::metadata(&path) else {
        return Ok(None);
    };

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

    Ok(Some(previous))
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
