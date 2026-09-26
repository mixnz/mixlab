//! Finding and recording directories without a row — spec D2 and D3.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mixengine_proto::{PackageSummary, PackageVersion, RuntimeKind, RuntimeSummary, Timestamp};

use super::Subject;
use super::marker::{self, Marker};
use crate::index::{Artifact, Index, Package, Target};
use crate::install::SmokeTest;
use crate::{Error, Paths, Result, Store};

/// A directory where an install would be, with no row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// What its place says it holds.
    pub subject: Subject,
    /// The directory.
    pub path: PathBuf,
}

/// What a directory is checked against.
#[derive(Debug)]
pub enum Evidence<'a> {
    /// Its own marker, and nothing else.
    Marker,
    /// The package index, for a directory without one.
    Index {
        /// The verified index.
        index: &'a Index,
        /// Which machine's artifact to compare with.
        target: Target,
        /// The kind's or recipe's smoke test, when it has one.
        smoke: Option<SmokeTest>,
    },
}

/// A directory that is now recorded.
#[derive(Debug, Clone)]
pub enum Claimed {
    /// A runtime row.
    Runtime(RuntimeSummary),
    /// A package row.
    Package(PackageSummary),
}

/// A directory that stays unrecorded, and why.
#[derive(Debug, Clone)]
pub struct Left {
    /// The directory.
    pub found: Found,
    /// Why, as a phrase.
    pub reason: String,
}

/// What one walk did.
#[derive(Debug, Clone, Default)]
pub struct Walked {
    /// What it recorded.
    pub claimed: Vec<Claimed>,
    /// What it could not.
    pub left: Vec<Left>,
}

/// Why one directory was not recorded. Private so the walk can branch on the kind of refusal while
/// the public error carries only the sentence.
enum Refusal {
    /// No marker this build can read.
    NoMarker,
    /// A marker, but for something else.
    OtherInstall,
    /// The index has nothing for it on this machine.
    NotPublished,
    /// It does not hold what that build provides.
    Incomplete(String),
    /// It does not run here.
    WillNotRun(String),
}

impl Refusal {
    fn reason(&self, subject: &Subject) -> String {
        match self {
            Self::NoMarker => {
                "it has no marker, and the package index is needed to check it".to_owned()
            }
            Self::OtherInstall => "its marker names another install".to_owned(),
            Self::NotPublished => format!(
                "the package index publishes no {} {} for this machine",
                subject.name(),
                subject.version()
            ),
            Self::Incomplete(detail) => {
                format!("it does not hold what that build provides ({detail})")
            }
            Self::WillNotRun(detail) => format!("it does not run here ({detail})"),
        }
    }
}

/// Every install directory under `runtimes/` and `packages/` that has no row.
///
/// A name that starts with a dot (a staging directory, a tombstone) or is not a version is skipped,
/// and so is a kind this build does not know: none of them is something an install left.
///
/// # Errors
///
/// [`Error::Database`] when the rows cannot be read. A directory that cannot be listed is skipped.
pub async fn unrecorded(store: &Store, paths: &Paths) -> Result<Vec<Found>> {
    let runtimes: BTreeSet<(RuntimeKind, PackageVersion)> = crate::runtimes::records(store, None)
        .await?
        .into_iter()
        .map(|row| (row.kind, row.version))
        .collect();
    let packages: BTreeSet<(String, PackageVersion)> = crate::packages::records(store, None)
        .await?
        .into_iter()
        .map(|row| (row.package, row.version))
        .collect();

    let mut found = Vec::new();

    for (name, version, path) in two_levels(paths.runtimes()) {
        let (Some(kind), Ok(version)) = (RuntimeKind::parse(&name), PackageVersion::parse(version))
        else {
            continue;
        };
        if !runtimes.contains(&(kind, version.clone())) {
            found.push(Found {
                subject: Subject::Runtime { kind, version },
                path,
            });
        }
    }

    for (package, version, path) in two_levels(paths.packages()) {
        let Ok(version) = PackageVersion::parse(version) else {
            continue;
        };
        if !packages.contains(&(package.clone(), version.clone())) {
            found.push(Found {
                subject: Subject::Package { package, version },
                path,
            });
        }
    }

    Ok(found)
}

/// `<root>/<a>/<b>` for every pair of directories whose names do not start with a dot.
fn two_levels(root: &Path) -> Vec<(String, String, PathBuf)> {
    visible_dirs(root)
        .into_iter()
        .flat_map(|(first, dir)| {
            visible_dirs(&dir)
                .into_iter()
                .map(move |(second, path)| (first.clone(), second, path))
        })
        .collect()
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

/// Record one directory, or say why not.
///
/// # Errors
///
/// [`Error::UnrecordedInstall`] naming the reason; the errors of `remember`.
pub async fn claim(store: &Store, found: &Found, evidence: &Evidence<'_>) -> Result<Claimed> {
    match attempt(store, found, evidence).await? {
        Ok(claimed) => Ok(claimed),
        Err(refusal) => Err(Error::UnrecordedInstall {
            path: found.path.clone(),
            reason: refusal.reason(&found.subject),
        }),
    }
}

/// [`claim`], with the refusal kept apart from the errors that stop a walk.
async fn attempt(
    store: &Store,
    found: &Found,
    evidence: &Evidence<'_>,
) -> Result<std::result::Result<Claimed, Refusal>> {
    let marker = match evidence {
        Evidence::Marker => match marker::read(&found.path) {
            // What the install said is not proof its files are still there, nor that its paths stay
            // inside the directory: the same check the index path makes, against the marker's map.
            Some(marker) if marker.names(&found.subject) => {
                if let Err(error) =
                    crate::install::provided(marker.provides(), marker.url(), &found.path)
                {
                    return Ok(Err(Refusal::Incomplete(error.to_string())));
                }
                marker
            }
            Some(_) => return Ok(Err(Refusal::OtherInstall)),
            None => return Ok(Err(Refusal::NoMarker)),
        },

        Evidence::Index {
            index,
            target,
            smoke,
        } => {
            let Some((package, artifact)) = published(found, index, *target) else {
                return Ok(Err(Refusal::NotPublished));
            };

            if let Err(error) = crate::install::present(artifact, &found.path) {
                return Ok(Err(Refusal::Incomplete(error.to_string())));
            }
            if let Some(smoke) = smoke
                && let Err(error) = crate::install::check_runs(artifact, &found.path, smoke).await
            {
                return Ok(Err(Refusal::WillNotRun(error.to_string())));
            }

            let marker = from_index(found, package, artifact);

            // Written now so the next start does not ask the index again. A marker that cannot be
            // written costs only that.
            if let Err(error) = std::fs::write(found.path.join(marker::FILE_NAME), marker.encode())
            {
                tracing::warn!(
                    path = %found.path.display(),
                    %error,
                    "a recorded directory could not be given its marker"
                );
            }

            marker
        }
    };

    let now = Timestamp::from_system_time(std::time::SystemTime::now());

    Ok(Ok(match marker {
        Marker::Runtime(runtime) => Claimed::Runtime(
            crate::runtimes::remember(store, &runtime.installation(found.path.clone()), now)
                .await?,
        ),
        Marker::Package(package) => Claimed::Package(
            crate::packages::remember(store, &package.installation(found.path.clone()), now)
                .await?,
        ),
    }))
}

/// The index's entry and artifact for this directory's install on `target`.
fn published<'a>(
    found: &Found,
    index: &'a Index,
    target: Target,
) -> Option<(&'a Package, &'a Artifact)> {
    let package = index.packages.iter().find(|package| {
        package.kind == found.subject.name() && package.version == found.subject.version().as_str()
    })?;

    Some((package, package.select(target)?.artifact))
}

/// The marker an install of this from the index would have written.
fn from_index(found: &Found, package: &Package, artifact: &Artifact) -> Marker {
    match &found.subject {
        Subject::Runtime { kind, version } => Marker::runtime(&crate::runtimes::Installation {
            kind: *kind,
            version: version.clone(),
            channel: package.channel.into(),
            path: found.path.clone(),
            bytes: artifact.size,
            url: artifact.url.clone(),
            sha256: artifact.sha256.clone(),
            provides: artifact.provides.clone(),
            extension_dir: artifact.extension_dir.clone(),
            extensions: artifact.extensions.clone(),
        }),
        Subject::Package { package, version } => Marker::package(&crate::packages::Installation {
            package: package.clone(),
            version: version.clone(),
            path: found.path.clone(),
            bytes: artifact.size,
            url: artifact.url.clone(),
            sha256: artifact.sha256.clone(),
            provides: artifact.provides.clone(),
        }),
    }
}

/// Record every directory without a row that can be recorded.
///
/// By its marker first; then, with an index, by the index — except a directory whose marker names
/// another install, which the index is not asked to overrule. A kind that has no default afterwards
/// gets its newest version, which is the rule a first install follows.
///
/// # Errors
///
/// [`Error::Database`] when rows cannot be read or written. A directory that cannot be recorded is
/// a [`Left`], never an error.
pub async fn walk(
    store: &Store,
    paths: &Paths,
    index: Option<(&Index, Target)>,
    smoke_for: &(dyn Fn(&Subject) -> Option<SmokeTest> + Sync),
) -> Result<Walked> {
    let mut walked = Walked::default();

    // Asked before anything is recorded: `remember` makes the first version of a kind its default,
    // which for a walk is whichever directory happened to be listed first.
    let had_a_default: BTreeSet<RuntimeKind> = crate::runtimes::records(store, None)
        .await?
        .into_iter()
        .filter(|row| row.default)
        .map(|row| row.kind)
        .collect();

    for found in unrecorded(store, paths).await? {
        let outcome = match (attempt(store, &found, &Evidence::Marker).await?, index) {
            (Ok(claimed), _) => Ok(claimed),
            (Err(Refusal::NoMarker), Some((index, target))) => {
                let evidence = Evidence::Index {
                    index,
                    target,
                    smoke: smoke_for(&found.subject),
                };
                attempt(store, &found, &evidence).await?
            }
            (Err(refusal), _) => Err(refusal),
        };

        match outcome {
            Ok(claimed) => walked.claimed.push(claimed),
            Err(refusal) => walked.left.push(Left {
                reason: refusal.reason(&found.subject),
                found,
            }),
        }
    }

    defaults(store, &walked, &had_a_default).await?;

    Ok(walked)
}

/// Give every kind this walk recorded, and that had no default before it, its newest version.
async fn defaults(
    store: &Store,
    walked: &Walked,
    had_a_default: &BTreeSet<RuntimeKind>,
) -> Result<()> {
    let kinds: BTreeSet<RuntimeKind> = walked
        .claimed
        .iter()
        .filter_map(|claimed| match claimed {
            Claimed::Runtime(summary) if !had_a_default.contains(&summary.kind) => {
                Some(summary.kind)
            }
            _ => None,
        })
        .collect();

    for kind in kinds {
        let rows = crate::runtimes::records(store, Some(kind)).await?;
        if let Some(newest) = rows
            .into_iter()
            .max_by(|left, right| left.version.cmp_precedence(&right.version))
        {
            crate::runtimes::set_default(store, kind, &newest.version).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use mixengine_proto::{PackageVersion, RuntimeKind};

    use crate::adopt::Subject;
    use crate::adopt::marker::{self, Marker};
    use crate::config::PathOverrides;
    use crate::install::SmokeTest;
    use crate::{Paths, Store};

    async fn home() -> (tempfile::TempDir, Store, Paths) {
        let temp = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(&temp.path().join("mixengine.db"))
            .await
            .expect("a database");
        let paths = Paths::new(temp.path().to_path_buf(), &PathOverrides::default());
        (temp, store, paths)
    }

    fn a_node(paths: &Paths, version: &str) -> crate::runtimes::Installation {
        let version = PackageVersion::parse(version).expect("a version");
        crate::runtimes::Installation {
            kind: RuntimeKind::Node,
            path: crate::runtimes::directory(paths, RuntimeKind::Node, &version),
            version,
            channel: mixengine_proto::PackageChannel::Stable,
            bytes: 1,
            url: "https://example.invalid/node.zip".to_owned(),
            sha256: "ab".repeat(32),
            provides: [("node".to_owned(), "node".to_owned())]
                .into_iter()
                .collect(),
            extension_dir: None,
            extensions: crate::index::Extensions::default(),
        }
    }

    /// A directory the way an install leaves it, with the file `provides` names in it.
    fn on_disk(installation: &crate::runtimes::Installation, marked: bool) {
        std::fs::create_dir_all(&installation.path).expect("the directory");
        std::fs::write(installation.path.join("node"), b"").expect("the program");
        if marked {
            std::fs::write(
                installation.path.join(marker::FILE_NAME),
                Marker::runtime(installation).encode(),
            )
            .expect("the marker");
        }
    }

    fn no_smoke(_: &Subject) -> Option<SmokeTest> {
        None
    }

    /// **The machine this was reported from, with markers**: two Nodes on disk, no rows. Both are
    /// recorded, and the newer becomes the default.
    #[tokio::test]
    async fn marked_directories_are_recorded_and_the_newest_becomes_the_default() {
        let (_temp, store, paths) = home().await;
        on_disk(&a_node(&paths, "22.0.0"), true);
        on_disk(&a_node(&paths, "24.19.0"), true);

        let walked = walk(&store, &paths, None, &no_smoke).await.expect("a walk");

        assert_eq!(walked.claimed.len(), 2, "{walked:?}");
        assert!(walked.left.is_empty(), "{walked:?}");
        let rows = crate::runtimes::records(&store, Some(RuntimeKind::Node))
            .await
            .expect("rows");
        let default: Vec<_> = rows
            .iter()
            .filter(|row| row.default)
            .map(|row| row.version.as_str())
            .collect();
        assert_eq!(default, ["24.19.0"]);
    }

    /// A directory that already has its row is left exactly as it is: no second row, no marker.
    #[tokio::test]
    async fn a_directory_with_a_row_is_not_touched() {
        let (_temp, store, paths) = home().await;
        let installed = a_node(&paths, "24.19.0");
        on_disk(&installed, false);
        crate::runtimes::remember(
            &store,
            &installed,
            mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
        )
        .await
        .expect("a row");

        let walked = walk(&store, &paths, None, &no_smoke).await.expect("a walk");

        assert!(
            walked.claimed.is_empty() && walked.left.is_empty(),
            "{walked:?}"
        );
        assert!(
            !installed.path.join(marker::FILE_NAME).exists(),
            "no marker was written for it"
        );
    }

    /// Staging directories, tombstones, names that are not versions and kinds this build does not
    /// know are none of them something an install left.
    #[tokio::test]
    async fn half_finished_and_foreign_directories_are_skipped() {
        let (_temp, store, paths) = home().await;
        for name in [".24.19.0.staging", "not a version", ".tombstone-1"] {
            std::fs::create_dir_all(paths.runtimes().join("node").join(name)).expect("a directory");
        }
        std::fs::create_dir_all(paths.runtimes().join("cobol").join("1.0.0")).expect("a directory");

        let found = unrecorded(&store, &paths).await.expect("a walk");

        assert!(found.is_empty(), "{found:?}");
    }

    /// A marker for another version, copied into this directory, is not trusted, and the index is
    /// not asked to overrule it.
    #[tokio::test]
    async fn a_marker_naming_another_version_is_left_with_the_reason() {
        let (_temp, store, paths) = home().await;
        let other = a_node(&paths, "22.0.0");
        let here = a_node(&paths, "24.19.0");
        on_disk(&here, false);
        std::fs::write(
            here.path.join(marker::FILE_NAME),
            Marker::runtime(&other).encode(),
        )
        .expect("a marker");

        let walked = walk(&store, &paths, None, &no_smoke).await.expect("a walk");

        assert!(walked.claimed.is_empty(), "{walked:?}");
        assert_eq!(walked.left.len(), 1, "{walked:?}");
        assert!(
            walked.left[0].reason.contains("another install"),
            "{}",
            walked.left[0].reason
        );
        assert!(
            crate::runtimes::records(&store, None)
                .await
                .expect("rows")
                .is_empty()
        );
    }

    /// A marker is what an install said, not proof its files are still there: a directory whose
    /// program is gone, or whose marker points outside it, is left rather than recorded as a runtime
    /// that cannot run.
    #[tokio::test]
    async fn a_marker_whose_files_are_missing_or_outside_is_left() {
        let (_temp, store, paths) = home().await;

        let gone = a_node(&paths, "24.19.0");
        on_disk(&gone, true);
        std::fs::remove_file(gone.path.join("node")).expect("the program");

        let mut outside = a_node(&paths, "22.0.0");
        outside.provides = [("node".to_owned(), "../../elsewhere/node".to_owned())]
            .into_iter()
            .collect();
        on_disk(&outside, true);

        let walked = walk(&store, &paths, None, &no_smoke).await.expect("a walk");

        assert!(walked.claimed.is_empty(), "{walked:?}");
        assert_eq!(walked.left.len(), 2, "{walked:?}");
        for left in &walked.left {
            assert!(left.reason.contains("does not hold"), "{}", left.reason);
        }
    }

    /// Without a marker and without an index there is nothing to go on, and nothing is recorded:
    /// the offline start.
    #[tokio::test]
    async fn an_unmarked_directory_waits_for_the_index() {
        let (_temp, store, paths) = home().await;
        on_disk(&a_node(&paths, "24.19.0"), false);

        let walked = walk(&store, &paths, None, &no_smoke).await.expect("a walk");

        assert!(walked.claimed.is_empty(), "{walked:?}");
        assert_eq!(walked.left.len(), 1);
        assert!(
            walked.left[0].reason.contains("index"),
            "{}",
            walked.left[0].reason
        );
    }
}
