//! `daemon.disk_usage` and `daemon.cleanup`.
//!
//! **A strict read and a job, on `daemon.uninstall_plan`/`daemon.uninstall`'s split.** A call that
//! measured and deleted in one breath would leave no moment in which somebody could be shown what
//! is about to go.
//!
//! **The read is cached, and that is not a convenience.** The dashboard this exists for holds the
//! event stream open and re-reads whenever something changes; a walk of `runtimes/` is tens of
//! thousands of files, so an uncached reading would be a disk walk per event. The last one is kept
//! for [`FRESH_FOR`] and [`DiskUsageQuery::refresh`] forces a new one.
//!
//! **Everything that touches the disk happens on a blocking thread**, per
//! `docs/standards/rust.md`: a `read_dir` of a cold `runtimes/` is seconds, and the daemon is
//! supervising processes while it happens.

mod measure;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mixengine_core::Paths;
use mixengine_proto::{
    CategoryUsage, Cleaned, Cleanup, CleanupQuery, CleanupReport, DiskCategory, DiskUsage,
    DiskUsageQuery, Error, ErrorCode, Reclaim, Timestamp, rpc,
};

use measure::{Measured, Reclaimable};

/// How long a reading is answered with before the disk is walked again.
///
/// A minute, because what it bounds is a screen: somebody watching a download wants the figure to
/// move within a minute of it finishing, and nothing on this screen changes faster than that without
/// somebody having started it. A caller who wants *now* says so.
const FRESH_FOR: Duration = Duration::from_secs(60);

/// Both halves: the measurement and the cleanup.
#[derive(Debug)]
pub(crate) struct Disk {
    /// The home's layout: which directory each category is, wherever `[paths]` put it.
    paths: Paths,

    /// The last reading, and when it was taken.
    ///
    /// **A `Mutex` held across the walk, which makes it single-flight**: a second caller arriving
    /// during a walk waits for that walk rather than starting one of its own, and then finds a fresh
    /// reading. Two callers each walking `runtimes/` is the cost this whole field exists to avoid.
    last: tokio::sync::Mutex<Option<(Instant, DiskUsage)>>,
}

impl Disk {
    /// The one of these the API holds.
    pub(crate) fn new(paths: &Paths) -> Arc<Self> {
        Arc::new(Self {
            paths: paths.clone(),
            last: tokio::sync::Mutex::new(None),
        })
    }

    /// `daemon.disk_usage` — where this home's disk has gone.
    ///
    /// **A read, and every branch of it.** Nothing here writes a row, writes a file, enqueues an
    /// operation or can raise a prompt.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Internal`] if the blocking thread the walk runs on panicked. A *machine* that
    /// could not be read is never an error: it is a note on the row it is about, because a total
    /// that failed rather than saying "at least this" is a screen nobody can use.
    pub(crate) async fn usage(&self, query: &DiskUsageQuery) -> Result<DiskUsage, Error> {
        let mut last = self.last.lock().await;

        if !query.refresh
            && let Some((taken, usage)) = last.as_ref()
            && taken.elapsed() < FRESH_FOR
        {
            return Ok(usage.clone());
        }

        let paths = self.paths.clone();
        let usage = tokio::task::spawn_blocking(move || read(&paths))
            .await
            .map_err(|error| {
                Error::new(
                    ErrorCode::Internal,
                    format!("this home's disk could not be measured: {error}"),
                )
            })?;

        *last = Some((Instant::now(), usage.clone()));

        Ok(usage)
    }

    /// `daemon.cleanup` — take back what is safe to lose.
    ///
    /// **What may be removed is a closed list of names**, built by the same two functions
    /// [`usage`](Self::usage) counts with — so what somebody was shown and what goes cannot
    /// disagree. Nothing here walks the home looking for things to delete.
    ///
    /// **Nothing here checks whether another job is running, and nothing here reports progress.**
    /// The refusal belongs to the caller, which is the only place that can raise it *before* this
    /// job exists; the progress lines belong to the job. Both live in `Api::cleanup_now`, which
    /// leaves this method callable — and testable — without a database.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Internal`] if the blocking thread the sweep runs on panicked. A file that would
    /// not go is never an error: it is [`Cleanup::Partial`] on the row it is about.
    pub(crate) async fn cleanup(&self, query: &CleanupQuery) -> Result<CleanupReport, Error> {
        let logs = self
            .sweep(
                DiskCategory::Logs,
                self.paths.logs().to_path_buf(),
                query.keep_logs,
                "you asked for the logs to be left",
            )
            .await?;

        let cache = self
            .sweep(
                DiskCategory::Cache,
                self.paths.cache().to_path_buf(),
                query.keep_cache,
                "you asked for the cache to be left",
            )
            .await?;

        // The held reading is of a home these files were still in. Dropped here rather than left to
        // expire, so that a client which cleaned up and re-read is not shown the sizes of files that
        // have gone and told nothing happened.
        self.forget().await;

        Ok(CleanupReport {
            items: vec![logs, cache],
        })
    }

    /// Drop the held reading, so the next cheap read measures.
    async fn forget(&self) {
        *self.last.lock().await = None;
    }

    /// One category's sweep, or the row that says it was left alone.
    async fn sweep(
        &self,
        id: DiskCategory,
        location: PathBuf,
        keep: bool,
        because: &str,
    ) -> Result<Cleaned, Error> {
        let outcome = if keep {
            Cleanup::Kept {
                because: because.to_owned(),
            }
        } else {
            let directory = location.clone();
            let of = id;

            tokio::task::spawn_blocking(move || {
                let reclaimable = match of {
                    DiskCategory::Logs => measure::reclaimable_logs(&directory),
                    _ => measure::reclaimable_cache(&directory),
                };

                measure::sweep(&reclaimable)
            })
            .await
            .map_err(|error| {
                Error::new(
                    ErrorCode::Internal,
                    format!("this home could not be cleaned up: {error}"),
                )
            })?
        };

        Ok(Cleaned {
            id,
            location: location.display().to_string(),
            outcome,
        })
    }
}

/// One walk of this home, and the answer built from it. Blocking.
fn read(paths: &Paths) -> DiskUsage {
    let logs = measure::reclaimable_logs(paths.logs());
    let cache = measure::reclaimable_cache(paths.cache());

    let categories = vec![
        row(
            DiskCategory::Runtimes,
            paths.runtimes(),
            Reclaim::ByMethod {
                method: rpc::method::RUNTIME_UNINSTALL.to_owned(),
                because: "one runtime at a time, and never one a running pool is using".to_owned(),
            },
        ),
        row(
            DiskCategory::Data,
            paths.data(),
            Reclaim::Never {
                because: "these are your databases, and nothing in MixEngine deletes them"
                    .to_owned(),
            },
        ),
        row(DiskCategory::Logs, paths.logs(), by_cleanup(&logs)),
        row(
            DiskCategory::Certs,
            paths.certs(),
            Reclaim::AtACost {
                because: "every site would lose HTTPS until `cert.issue` ran again, and no method \
                          here deletes a certificate: `mix uninstall` is what takes them"
                    .to_owned(),
            },
        ),
        row(DiskCategory::Cache, paths.cache(), by_cleanup(&cache)),
    ];

    DiskUsage {
        root: paths.root().display().to_string(),
        measured_at: Timestamp::from_system_time(std::time::SystemTime::now()),
        categories,
        other_bytes: remainder(paths),
    }
}

/// One row: what the directory weighs, and what would take it back.
fn row(id: DiskCategory, location: &Path, reclaim: Reclaim) -> CategoryUsage {
    let Measured {
        bytes,
        files,
        unreadable,
    } = measure::walk(location);

    CategoryUsage {
        id,
        location: location.display().to_string(),
        bytes,
        files,
        reclaim,
        unreadable,
    }
}

/// The plan half of a reclaimable row.
fn by_cleanup(reclaimable: &Reclaimable) -> Reclaim {
    Reclaim::ByCleanup {
        bytes: reclaimable.bytes,
        files: reclaimable.files,
    }
}

/// Everything the five categories are not.
///
/// A closed list of directories and files, and not a walk of the root: a stray directory somebody
/// dropped in the home is theirs, and counting it would make this number mean *"what is in the
/// home"* rather than *"what MixEngine put there"* — `daemon.bundle`'s rule in the other direction.
fn remainder(paths: &Paths) -> u64 {
    let directories = [
        paths.bin(),
        paths.etc(),
        paths.packages(),
        paths.extensions(),
        paths.blueprints(),
        paths.run(),
    ];

    let mut bytes = directories
        .iter()
        .map(|directory| measure::walk(directory).bytes)
        .fold(0, u64::saturating_add);

    // The database's own two companions travel with it: a write-ahead log is megabytes on a busy
    // home, and a total that left it out would be one that disagreed with the file manager.
    let database = paths.database_file();
    let files = [
        database.to_path_buf(),
        with_suffix(database, "-wal"),
        with_suffix(database, "-shm"),
        paths.config_file().to_path_buf(),
    ];

    for file in files {
        if let Ok(meta) = std::fs::symlink_metadata(&file)
            && meta.is_file()
        {
            bytes = bytes.saturating_add(meta.len());
        }
    }

    bytes
}

/// `mixengine.db` → `mixengine.db-wal`.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.to_path_buf().into_os_string();
    name.push(suffix);

    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five rows are always there, in one order, each saying what would take it back. A client
    /// that had to derive that would be deriving exactly the policy `CLAUDE.md` keeps out of
    /// clients.
    #[tokio::test]
    async fn the_five_rows_are_always_there_and_each_says_what_reclaims_it() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let paths = home_at(&home);

        let disk = Disk::new(&paths);
        let usage = disk
            .usage(&DiskUsageQuery { refresh: true })
            .await
            .expect("a reading");

        let ids: Vec<DiskCategory> = usage.categories.iter().map(|row| row.id).collect();
        assert_eq!(ids, DiskCategory::ALL.to_vec());

        let reclaims: Vec<&Reclaim> = usage.categories.iter().map(|row| &row.reclaim).collect();
        assert!(matches!(
            reclaims[0],
            Reclaim::ByMethod { method, .. } if method == rpc::method::RUNTIME_UNINSTALL
        ));
        assert!(matches!(reclaims[1], Reclaim::Never { .. }));
        assert!(matches!(reclaims[2], Reclaim::ByCleanup { .. }));
        assert!(matches!(reclaims[3], Reclaim::AtACost { .. }));
        assert!(matches!(reclaims[4], Reclaim::ByCleanup { .. }));

        assert_eq!(usage.root, paths.root().display().to_string());
    }

    /// `logs/`'s own weight is everything under it; what a cleanup would take is the rotated copies
    /// alone. The two numbers being different is the whole point of the plan half.
    #[tokio::test]
    async fn the_logs_row_weighs_everything_and_offers_only_the_rotated_copies() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let paths = home_at(&home);
        std::fs::create_dir_all(paths.crashes()).expect("a crashes directory");
        std::fs::write(paths.daemon_log_file(), vec![0u8; 500]).expect("a live log");
        std::fs::write(paths.logs().join("daemon.log.1"), vec![0u8; 100]).expect("a rotated log");
        std::fs::write(paths.crashes().join("a.json"), vec![0u8; 50]).expect("a crash report");

        let disk = Disk::new(&paths);
        let usage = disk
            .usage(&DiskUsageQuery { refresh: true })
            .await
            .expect("a reading");

        let logs = usage
            .categories
            .iter()
            .find(|row| row.id == DiskCategory::Logs)
            .expect("a logs row");

        assert_eq!(logs.bytes, 650);
        assert_eq!(
            logs.reclaim,
            Reclaim::ByCleanup {
                bytes: 100,
                files: 1
            }
        );
    }

    /// A second call without `refresh` answers from the reading the first one took, which is what
    /// keeps a dashboard from walking `runtimes/` once an event.
    #[tokio::test]
    async fn a_second_read_without_refresh_is_the_first_ones_answer() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let paths = home_at(&home);
        std::fs::create_dir_all(paths.cache()).expect("a cache directory");

        let disk = Disk::new(&paths);
        let first = disk
            .usage(&DiskUsageQuery { refresh: true })
            .await
            .expect("a reading");

        std::fs::write(paths.cache().join("index.json"), vec![0u8; 999]).expect("a cached index");

        let cached = disk
            .usage(&DiskUsageQuery { refresh: false })
            .await
            .expect("a reading");
        assert_eq!(cached, first, "a cheap read walked the disk again");

        let fresh = disk
            .usage(&DiskUsageQuery { refresh: true })
            .await
            .expect("a reading");
        assert_ne!(fresh, first, "a forced read answered from the cache");
    }

    /// `keep_logs` leaves them, and says so rather than reporting an empty sweep — a report that
    /// printed only what it removed would leave somebody unable to tell the two apart. And a
    /// finished cleanup forgets the reading, so the next cheap read is of the home as it now is.
    #[tokio::test]
    async fn keeping_a_category_reports_it_kept_and_a_cleanup_forgets_the_reading() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let paths = home_at(&home);
        std::fs::create_dir_all(paths.logs()).expect("a logs directory");
        std::fs::create_dir_all(paths.cache()).expect("a cache directory");
        std::fs::write(paths.logs().join("daemon.log.1"), vec![0u8; 100]).expect("a rotated log");
        std::fs::write(paths.cache().join("index.json"), vec![0u8; 30]).expect("a cached index");

        let disk = Disk::new(&paths);
        let before = disk
            .usage(&DiskUsageQuery { refresh: true })
            .await
            .expect("a reading");

        let report = disk
            .cleanup(&CleanupQuery {
                keep_logs: true,
                keep_cache: false,
            })
            .await
            .expect("a report");

        assert_eq!(report.items.len(), 2);
        assert_eq!(report.items[0].id, DiskCategory::Logs);
        assert!(matches!(report.items[0].outcome, Cleanup::Kept { .. }));
        assert_eq!(
            report.items[1].outcome,
            Cleanup::Reclaimed {
                files: 1,
                bytes: 30
            }
        );

        assert!(paths.logs().join("daemon.log.1").exists());
        assert!(!paths.cache().join("index.json").exists());
        assert_eq!(report.reclaimed_bytes(), 30);
        assert!(!report.left_behind());

        let after = disk
            .usage(&DiskUsageQuery { refresh: false })
            .await
            .expect("a reading");
        assert_ne!(after, before, "the cleanup left a stale reading behind");
    }

    /// The live log, the crash reports and everything in `data/` are out of reach by construction: a
    /// cleanup matches names, it does not walk looking for things to delete.
    #[tokio::test]
    async fn a_cleanup_cannot_reach_the_live_log_the_crashes_or_the_data() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let paths = home_at(&home);
        std::fs::create_dir_all(paths.crashes()).expect("a crashes directory");
        std::fs::create_dir_all(paths.data()).expect("a data directory");
        std::fs::create_dir_all(paths.cache()).expect("a cache directory");
        std::fs::write(paths.daemon_log_file(), vec![0u8; 10]).expect("a live log");
        std::fs::write(paths.logs().join("daemon.log.1"), vec![0u8; 10]).expect("a rotated log");
        std::fs::write(paths.crashes().join("a.json"), vec![0u8; 10]).expect("a crash report");
        std::fs::write(paths.data().join("mysql.ibd"), vec![0u8; 10]).expect("a database file");

        let disk = Disk::new(&paths);
        disk.cleanup(&CleanupQuery::default())
            .await
            .expect("a report");

        assert!(paths.daemon_log_file().exists());
        assert!(paths.crashes().join("a.json").exists());
        assert!(paths.data().join("mysql.ibd").exists());
        assert!(!paths.logs().join("daemon.log.1").exists());
    }

    /// The layout of a home that only exists for one test, with no `[paths]` override.
    fn home_at(home: &tempfile::TempDir) -> Paths {
        Paths::new(
            home.path().to_path_buf(),
            &mixengine_core::config::PathOverrides::default(),
        )
    }
}
