//! Service data an earlier home left under `data/` — roadmap task **T182g**, spec D4.
//!
//! A data directory MixEngine bootstrapped carries [`READY_MARKER`], holding the version of the
//! server that made it. That file is what makes adopting one safe: a new row pointing at the
//! directory skips the first run (`first_run::inspect` calls it `Ready`), and the credential repair
//! accepts it. What this module adds is the finding, and which installed version may open it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mixengine_proto::{PackageSummary, PackageVersion};
use sqlx::Row as _;

use crate::generate::first_run::READY_MARKER;
use crate::generate::{Catalogue, Instancing};
use crate::{Paths, Result, Store};

/// A data directory with no service row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundInstance {
    /// The package whose recipe the directory belongs to.
    pub package: String,
    /// The instance name its place gives it: the directory's name, or the package's for a server
    /// that exists once.
    pub instance: String,
    /// The directory.
    pub path: PathBuf,
    /// The version that bootstrapped it, from its [`READY_MARKER`]; [`None`] when its first run
    /// never finished.
    pub made_by: Option<PackageVersion>,
}

/// Which installed version may open a found directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opens {
    /// This one: the newest installed version of the series that made it.
    With(PackageVersion),
    /// None: its first run never finished, so there is no database in it to keep.
    NotReady,
    /// None installed: this, as `<package> <series>`, is what would.
    Needs(String),
}

/// Every service data directory under `data/` with no row.
///
/// `data/<package>/<instance>` for a server with named instances, `data/<package>` for one that
/// exists once. `data/extensions` (an extension's own data), a name that starts with a dot, a
/// package no recipe is found under, an empty directory, and an instance's temporary directory
/// beside it are none of them an instance.
///
/// # Errors
///
/// [`crate::Error::Database`] when the rows cannot be read.
pub async fn found(
    store: &Store,
    paths: &Paths,
    catalogue: &Catalogue,
) -> Result<Vec<FoundInstance>> {
    let rows = sqlx::query(
        "SELECT packages.name AS package, services.instance_name AS instance
           FROM services JOIN packages ON packages.id = services.package_id",
    )
    .fetch_all(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    let recorded: BTreeSet<(String, String)> = rows
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("package"),
                row.get::<String, _>("instance"),
            )
        })
        .collect();

    let mut found = Vec::new();

    for (package, dir) in visible_dirs(paths.data()) {
        if package == "extensions" {
            continue;
        }
        let Some(recipe) = catalogue.recipe(&package) else {
            continue;
        };

        let candidates = match recipe.instancing() {
            Instancing::Single => vec![(package.clone(), dir)],
            Instancing::Named => visible_dirs(&dir),
        };

        for (instance, path) in candidates {
            if recorded.contains(&(package.clone(), instance.clone()))
                || is_empty(&path)
                || is_scratch(&path)
            {
                continue;
            }
            found.push(FoundInstance {
                made_by: made_by(&path),
                package: package.clone(),
                instance,
                path,
            });
        }
    }

    Ok(found)
}

/// Which installed version may open `found`, of `installed` (every package row).
///
/// MariaDB and MySQL need the same major.minor: a newer series needs `mariadb-upgrade`, which this
/// build does not run, and an older server opening newer data corrupts it. PostgreSQL needs the same
/// major, which is what its data directory is tied to. Anything else opens with any version.
#[must_use]
pub fn opens(found: &FoundInstance, installed: &[PackageSummary]) -> Opens {
    let Some(made_by) = &found.made_by else {
        return Opens::NotReady;
    };

    let series = series_of(&found.package, made_by);

    installed
        .iter()
        .filter(|row| row.package == found.package)
        .filter(|row| {
            series.as_deref().is_none_or(|wanted| {
                series_of(&found.package, &row.version).as_deref() == Some(wanted)
            })
        })
        .max_by(|left, right| left.version.cmp_precedence(&right.version))
        .map_or_else(
            || {
                Opens::Needs(match &series {
                    Some(series) => format!("{} {series}", found.package),
                    None => found.package.clone(),
                })
            },
            |row| Opens::With(row.version.clone()),
        )
}

/// The part of a version data is tied to, for a package whose data is tied to one.
pub(crate) fn series_of(package: &str, version: &PackageVersion) -> Option<String> {
    let parts: Vec<&str> = version.as_str().split('.').collect();
    let take = match package {
        "mariadb" | "mysql" => 2,
        "postgres" => 1,
        _ => return None,
    };
    Some(
        parts
            .iter()
            .take(take)
            .copied()
            .collect::<Vec<_>>()
            .join("."),
    )
}

/// The version a finished first run wrote into the directory.
fn made_by(dir: &Path) -> Option<PackageVersion> {
    let text = std::fs::read_to_string(dir.join(READY_MARKER)).ok()?;
    PackageVersion::parse(text.trim()).ok()
}

/// Whether `dir` is another instance's temporary directory rather than an instance of its own.
///
/// The MySQL family keeps its temporary files in `<data>.tmp`, beside the data directory
/// (`recipes::scratch_dir`), and the server writes to it on every start, so it is never empty. A
/// `<name>.tmp` beside a `<name>` is that directory, unless its own first run finished — an
/// instance a person named that way.
fn is_scratch(dir: &Path) -> bool {
    let Some(owner) = dir
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };

    dir.with_file_name(owner).is_dir() && !dir.join(READY_MARKER).exists()
}

fn is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).map_or(true, |mut entries| entries.next().is_none())
}

/// The directories directly inside `dir` whose names are UTF-8 and do not start with a dot.
fn visible_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            (!name.starts_with('.')).then(|| (name, entry.path()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::PathOverrides;
    use crate::generate::Catalogue;
    use crate::generate::first_run::READY_MARKER;
    use crate::{Paths, Store};

    async fn home() -> (tempfile::TempDir, Store, Paths) {
        let temp = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(&temp.path().join("mixengine.db"))
            .await
            .expect("a database");
        let paths = Paths::new(temp.path().to_path_buf(), &PathOverrides::default());
        (temp, store, paths)
    }

    fn version(text: &str) -> PackageVersion {
        PackageVersion::parse(text).expect("a version")
    }

    /// A data directory the way a finished first run leaves it.
    fn data(paths: &Paths, relative: &str, made_by: Option<&str>) -> PathBuf {
        let dir = paths.data().join(relative);
        std::fs::create_dir_all(&dir).expect("the directory");
        std::fs::write(dir.join("ibdata1"), b"somebody's data").expect("a file");
        if let Some(made_by) = made_by {
            std::fs::write(dir.join(READY_MARKER), format!("{made_by}\n")).expect("the marker");
        }
        dir
    }

    async fn installed(store: &Store, package: &str, text: &str) -> PackageSummary {
        crate::packages::remember(
            store,
            &crate::packages::Installation {
                package: package.to_owned(),
                version: version(text),
                path: PathBuf::from(format!("/packages/{package}/{text}")),
                bytes: 1,
                url: "https://example.invalid/p".to_owned(),
                sha256: "ab".to_owned(),
                provides: std::collections::BTreeMap::new(),
            },
            mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
        )
        .await
        .expect("a package row")
    }

    fn a_found(package: &str, made_by: Option<&str>) -> FoundInstance {
        FoundInstance {
            package: package.to_owned(),
            instance: "main".to_owned(),
            path: PathBuf::from("/data"),
            made_by: made_by.map(version),
        }
    }

    #[tokio::test]
    async fn a_mariadb_directory_without_a_row_is_found_with_the_version_that_made_it() {
        let (_temp, store, paths) = home().await;
        let dir = data(&paths, "mariadb/main", Some("11.4.3"));

        let found = found(&store, &paths, &Catalogue::builtin())
            .await
            .expect("a walk");

        assert_eq!(
            found,
            [FoundInstance {
                package: "mariadb".to_owned(),
                instance: "main".to_owned(),
                path: dir,
                made_by: Some(version("11.4.3")),
            }]
        );
    }

    #[tokio::test]
    async fn an_instance_with_a_row_is_not_found() {
        let (_temp, store, paths) = home().await;
        data(&paths, "mariadb/main", Some("11.4.3"));
        installed(&store, "mariadb", "11.4.3").await;
        sqlx::query(
            "INSERT INTO services (id, package_id, instance_name, state, port)
             SELECT 'mariadb@main', id, 'main', 'stopped', 3306 FROM packages WHERE name = 'mariadb'",
        )
        .execute(store.pool())
        .await
        .expect("a service row");

        let found = found(&store, &paths, &Catalogue::builtin())
            .await
            .expect("a walk");

        assert!(found.is_empty(), "{found:?}");
    }

    #[tokio::test]
    async fn extensions_and_empty_and_unknown_directories_are_not_instances() {
        let (_temp, store, paths) = home().await;
        data(&paths, "extensions/mailpit", None);
        std::fs::create_dir_all(paths.data().join("mariadb/empty")).expect("an empty directory");
        data(&paths, "cobol/main", Some("1.0.0"));
        data(&paths, "mariadb/.tombstone", Some("11.4.3"));

        let found = found(&store, &paths, &Catalogue::builtin())
            .await
            .expect("a walk");

        assert!(found.is_empty(), "{found:?}");
    }

    #[tokio::test]
    async fn an_instances_scratch_directory_is_not_an_instance() {
        let (_temp, store, paths) = home().await;
        data(&paths, "mysql/5.7", Some("5.7.44"));
        let scratch = paths.data().join("mysql/5.7.tmp");
        std::fs::create_dir_all(&scratch).expect("the scratch directory");
        std::fs::write(scratch.join("ibFA1E.tmp"), b"").expect("a temporary file");

        let found = found(&store, &paths, &Catalogue::builtin())
            .await
            .expect("a walk");

        assert_eq!(
            found
                .iter()
                .map(|found| found.instance.as_str())
                .collect::<Vec<_>>(),
            ["5.7"]
        );
    }

    #[tokio::test]
    async fn a_finished_instance_named_like_a_scratch_directory_is_still_found() {
        let (_temp, store, paths) = home().await;
        data(&paths, "mariadb/main", Some("11.4.3"));
        data(&paths, "mariadb/main.tmp", Some("11.4.3"));

        let mut found = found(&store, &paths, &Catalogue::builtin())
            .await
            .expect("a walk");
        found.sort_by(|left, right| left.instance.cmp(&right.instance));

        assert_eq!(
            found
                .iter()
                .map(|found| found.instance.as_str())
                .collect::<Vec<_>>(),
            ["main", "main.tmp"]
        );
    }

    #[tokio::test]
    async fn a_single_instance_package_is_found_under_its_own_name() {
        let (_temp, store, paths) = home().await;
        data(&paths, "caddy", None);

        let found = found(&store, &paths, &Catalogue::builtin())
            .await
            .expect("a walk");

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            (found[0].package.as_str(), found[0].instance.as_str()),
            ("caddy", "caddy")
        );
    }

    #[tokio::test]
    async fn opens_picks_the_newest_installed_version_of_the_same_series() {
        let (_temp, store, _paths) = home().await;
        let rows = vec![
            installed(&store, "mariadb", "11.4.2").await,
            installed(&store, "mariadb", "11.4.5").await,
            installed(&store, "mariadb", "11.8.1").await,
        ];

        assert_eq!(
            opens(&a_found("mariadb", Some("11.4.3")), &rows),
            Opens::With(version("11.4.5"))
        );
    }

    #[tokio::test]
    async fn data_from_a_newer_series_than_anything_installed_needs_that_series() {
        let (_temp, store, _paths) = home().await;
        let rows = vec![installed(&store, "mariadb", "11.4.5").await];

        assert_eq!(
            opens(&a_found("mariadb", Some("11.8.1")), &rows),
            Opens::Needs("mariadb 11.8".to_owned())
        );
    }

    #[tokio::test]
    async fn postgres_needs_the_same_major() {
        let (_temp, store, _paths) = home().await;
        let rows = vec![
            installed(&store, "postgres", "16.9").await,
            installed(&store, "postgres", "17.2").await,
        ];

        assert_eq!(
            opens(&a_found("postgres", Some("16.4")), &rows),
            Opens::With(version("16.9"))
        );
    }

    #[test]
    fn a_directory_that_never_finished_is_not_ready() {
        assert_eq!(opens(&a_found("mariadb", None), &[]), Opens::NotReady);
    }

    #[test]
    fn nothing_installed_names_the_install() {
        assert_eq!(
            opens(&a_found("mariadb", Some("11.4.3")), &[]),
            Opens::Needs("mariadb 11.4".to_owned())
        );
    }

    #[tokio::test]
    async fn a_package_with_no_series_opens_with_any_installed_version() {
        let (_temp, store, _paths) = home().await;
        let rows = vec![installed(&store, "redis", "7.4.1").await];

        assert_eq!(
            opens(&a_found("redis", Some("7.2.0")), &rows),
            Opens::With(version("7.4.1"))
        );
    }
}
