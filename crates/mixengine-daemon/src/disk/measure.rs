//! The walk, and the closed list of what a cleanup may remove — roadmap task **T96**.
//!
//! **Nothing here follows a symbolic link.** `read_dir` plus `symlink_metadata`, descending only
//! into an entry whose own metadata says it is a directory. Without that rule a link somebody put in
//! `data/` pointing at `/` is a walk of the whole disk reported as MixEngine's, and a cycle is a
//! hang.
//!
//! **Sizes are apparent** — `metadata.len()` — and not blocks allocated. That is what Explorer's
//! *Size* column and `ls -l` say, and it is the only figure `std` gives on all three systems.
//!
//! **What may be removed is a list of names, never the result of a walk.** [`reclaimable_logs`]
//! matches `daemon.log.<digits>` and `current.log.<digits>` and nothing else, at no other depth; the
//! live files, `logs/crashes/` and a service directory whose service has been deleted are all out of
//! reach because they do not match, not because something remembered to skip them.
//!
//! **Everything here blocks**, and every caller is inside `spawn_blocking`.

use std::path::{Path, PathBuf};

/// The daemon's own log, whose rotated copies are `daemon.log.1` … — the same string
/// `mixengine_core::paths::DAEMON_LOG_FILE_NAME` is, for the same file.
const DAEMON_LOG: &str = "daemon.log";

/// Where the per-service log directories live, inside `logs/`.
const SERVICES: &str = "services";

/// One service's live log, whose rotated copies are `current.log.1` … — the same string
/// `mixengine_supervisor::logs::CURRENT_LOG_FILE_NAME` is.
const CURRENT_LOG: &str = "current.log";

/// The documents the extension registry, the update feed and a schema 1 package index cache at
/// the top of `cache/`.
///
/// The package index is one of two layouts, never both: `index.json`, or since roadmap task
/// **T196** a signed root beside one file per kind, which the index client itself recognises —
/// [`mixengine_core::index::packages::is_cache_file`], one rule for everything that removes them.
const CACHED_DOCUMENTS: [&str; 3] = ["index.json", "extensions.json", "latest.json"];

/// The three directories under `cache/` whose contents are ours to empty.
///
/// Emptied and never removed: `Paths::bootstrap` created `cache/` and the daemon is running out of
/// the home that holds it.
const CACHE_DIRECTORIES: [&str; 3] = ["downloads", "updates", "diagnostics"];

/// What one walk found.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Measured {
    /// Apparent bytes.
    pub(super) bytes: u64,

    /// Regular files and symbolic links, each counted once.
    pub(super) files: u64,

    /// Set when something under the directory could not be read, so [`bytes`](Self::bytes) is a
    /// floor. A note and never an error — the T96 design, D8.
    pub(super) unreadable: Option<String>,
}

/// What a cleanup may take from one directory: the paths, and what they weigh.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Reclaimable {
    /// Every path that may be removed, in no particular order. A path here is a file, a symbolic
    /// link, or a directory whose whole tree goes.
    pub(super) targets: Vec<PathBuf>,

    /// Apparent bytes across all of them.
    pub(super) bytes: u64,

    /// Files across all of them, counting a tree's contents rather than the tree.
    pub(super) files: u64,
}

/// Count every regular file under `directory`, without following a symbolic link.
///
/// A directory that is not there weighs nothing and says nothing: a home that has never crashed has
/// no `logs/crashes/`, and a `[paths]` override onto a disk that is not mounted is a `data/` nobody
/// can read. An entry that vanishes between `read_dir` listing it and this reading its metadata is
/// skipped silently — a rotation and an install finishing are both ordinary.
pub(super) fn walk(directory: &Path) -> Measured {
    let mut measured = Measured::default();
    let mut pending = vec![directory.to_path_buf()];
    let mut unreadable = 0_u64;
    let mut first: Option<String> = None;

    while let Some(current) = pending.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            // Not there at all is the only silent case, and only for the directory this was asked
            // about: everything deeper was listed a moment ago by its own parent.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && current == directory => {
                continue;
            }
            Err(error) => {
                unreadable = unreadable.saturating_add(1);
                first.get_or_insert_with(|| error.to_string());
                continue;
            }
        };

        for entry in entries {
            let Ok(entry) = entry else {
                unreadable = unreadable.saturating_add(1);
                continue;
            };

            let path = entry.path();

            // `symlink_metadata` and not `metadata`: the second follows the link, which is both the
            // wrong size and, for a link to a directory, the wrong tree.
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                // Gone between the listing and the reading. Ordinary, and not worth a note.
                continue;
            };

            if meta.is_dir() {
                pending.push(path);
            } else {
                measured.bytes = measured.bytes.saturating_add(meta.len());
                measured.files = measured.files.saturating_add(1);
            }
        }
    }

    if unreadable > 0 {
        measured.unreadable = Some(format!(
            "the total is at least this: {unreadable} {} could not be read ({})",
            if unreadable == 1 { "entry" } else { "entries" },
            first.unwrap_or_else(|| "no reason given".to_owned())
        ));
    }

    measured
}

/// The rotated log copies under `logs`, and what they weigh.
///
/// `<logs>/daemon.log.<digits>` and `<logs>/services/<any>/current.log.<digits>`, and nothing else,
/// at no other depth, and only where `symlink_metadata` says the entry is a regular file.
pub(super) fn reclaimable_logs(logs: &Path) -> Reclaimable {
    let mut reclaimable = Reclaimable::default();

    take_rotated(logs, DAEMON_LOG, &mut reclaimable);

    if let Ok(entries) = std::fs::read_dir(logs.join(SERVICES)) {
        for entry in entries.flatten() {
            let directory = entry.path();

            // A symbolic link where a service's log directory should be is not descended into, for
            // [`walk`]'s reason: what it points at is not this home's to remove.
            if std::fs::symlink_metadata(&directory).is_ok_and(|meta| meta.is_dir()) {
                take_rotated(&directory, CURRENT_LOG, &mut reclaimable);
            }
        }
    }

    reclaimable
}

/// Every `<live>.<digits>` directly inside `directory`, added to `into`.
fn take_rotated(directory: &Path, live: &str, into: &mut Reclaimable) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
            // A name that is not UTF-8 cannot be one of ours: both live names are ASCII, and a
            // rotated copy is that name plus digits.
            continue;
        };

        if !is_rotated(name, live) {
            continue;
        }

        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };

        if !meta.is_file() {
            continue;
        }

        into.bytes = into.bytes.saturating_add(meta.len());
        into.files = into.files.saturating_add(1);
        into.targets.push(path);
    }
}

/// Is `name` a rotated copy of `live` — `daemon.log.3`, `current.log.12`?
///
/// The live file itself is not, which is what keeps a cleanup away from a handle this process holds
/// open. Neither is `daemon.log.tmp`, `daemon.log.1.bak` or `daemon.logs.1`.
fn is_rotated(name: &str, live: &str) -> bool {
    let Some(suffix) = name.strip_prefix(live) else {
        return false;
    };

    let Some(digits) = suffix.strip_prefix('.') else {
        return false;
    };

    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// What a cleanup may take from `cache`, and what it weighs.
///
/// The three cached documents, and the contents of the three directories this build writes — the
/// directories themselves stay. Anything else somebody has put in `cache/` is theirs.
pub(super) fn reclaimable_cache(cache: &Path) -> Reclaimable {
    let mut reclaimable = Reclaimable::default();

    let index_files = std::fs::read_dir(cache)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(mixengine_core::index::packages::is_cache_file)
        })
        .map(|entry| entry.path());

    for path in CACHED_DOCUMENTS
        .iter()
        .map(|document| cache.join(document))
        .chain(index_files)
    {
        if let Ok(meta) = std::fs::symlink_metadata(&path)
            && meta.is_file()
        {
            reclaimable.bytes = reclaimable.bytes.saturating_add(meta.len());
            reclaimable.files = reclaimable.files.saturating_add(1);
            reclaimable.targets.push(path);
        }
    }

    for directory in CACHE_DIRECTORIES {
        let Ok(entries) = std::fs::read_dir(cache.join(directory)) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };

            // A whole staged update is one target: `cache/updates/0.0.3/` is a tree, and removing it
            // one file at a time would leave a half-unpacked payload behind on the first failure.
            let measured = if meta.is_dir() {
                walk(&path)
            } else {
                Measured {
                    bytes: meta.len(),
                    files: 1,
                    unreadable: None,
                }
            };

            reclaimable.bytes = reclaimable.bytes.saturating_add(measured.bytes);
            reclaimable.files = reclaimable.files.saturating_add(measured.files);
            reclaimable.targets.push(path);
        }
    }

    reclaimable
}

/// Remove every target, and count what actually went.
///
/// **Measured, not claimed** — the size is taken immediately before each removal, so the figure is
/// of files that are genuinely not there any more. A target that is already gone is not a failure:
/// two cleanups in a row, or a rotation that shifted a copy away between the reading and this, are
/// both ordinary.
///
/// Blocking, and every caller is inside `spawn_blocking`.
pub(super) fn sweep(reclaimable: &Reclaimable) -> mixengine_proto::Cleanup {
    if reclaimable.targets.is_empty() {
        return mixengine_proto::Cleanup::Empty {};
    }

    let mut bytes = 0_u64;
    let mut files = 0_u64;
    let mut left_behind = 0_u64;
    let mut first: Option<String> = None;

    for target in &reclaimable.targets {
        let Ok(meta) = std::fs::symlink_metadata(target) else {
            // Already gone. Nothing to count and nothing to report.
            continue;
        };

        // A symbolic link is unlinked, never followed: what it points at is not this home's.
        let (weight, count, removed) = if meta.is_dir() {
            let measured = walk(target);
            (
                measured.bytes,
                measured.files,
                std::fs::remove_dir_all(target),
            )
        } else {
            (meta.len(), 1, std::fs::remove_file(target))
        };

        match removed {
            Ok(()) => {
                bytes = bytes.saturating_add(weight);
                files = files.saturating_add(count);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                left_behind = left_behind.saturating_add(count.max(1));
                first.get_or_insert_with(|| format!("{}: {error}", target.display()));
            }
        }
    }

    match first {
        None => mixengine_proto::Cleanup::Reclaimed { files, bytes },
        Some(because) => mixengine_proto::Cleanup::Partial {
            files,
            bytes,
            left_behind,
            because,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree is counted whole, and a directory that is not there weighs nothing rather than
    /// failing: a home that has never crashed has no `logs/crashes/`.
    #[test]
    fn a_tree_is_counted_and_a_missing_directory_is_zero() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let root = home.path().join("runtimes");
        std::fs::create_dir_all(root.join("php/8.3/bin")).expect("a tree");
        std::fs::write(root.join("php/8.3/bin/php"), vec![0u8; 100]).expect("a file");
        std::fs::write(root.join("php/README"), vec![0u8; 20]).expect("a file");

        let measured = walk(&root);

        assert_eq!(measured.bytes, 120);
        assert_eq!(measured.files, 2);
        assert!(measured.unreadable.is_none());

        let absent = walk(&home.path().join("nowhere"));
        assert_eq!(absent.bytes, 0);
        assert_eq!(absent.files, 0);
        assert!(absent.unreadable.is_none());
    }

    /// A symlink is counted as itself and never followed. Without that rule a link somebody put in
    /// `data/` pointing at `/` is a walk of the whole disk reported as MixEngine's — and a cycle is
    /// a hang.
    #[cfg(unix)]
    #[test]
    fn a_symlink_is_counted_as_itself_and_never_followed() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let outside = home.path().join("outside");
        std::fs::create_dir_all(&outside).expect("a directory");
        std::fs::write(outside.join("big"), vec![0u8; 10_000]).expect("a file");

        let inside = home.path().join("data");
        std::fs::create_dir_all(&inside).expect("a directory");
        std::fs::write(inside.join("small"), vec![0u8; 10]).expect("a file");
        std::os::unix::fs::symlink(&outside, inside.join("escape")).expect("a symlink");

        let measured = walk(&inside);

        assert_eq!(measured.files, 2, "the link counts as one file");
        assert!(
            measured.bytes < 1_000,
            "the link was followed: {} bytes",
            measured.bytes
        );
    }

    /// Only rotated copies go. The live files have an open handle in this process, and
    /// `logs/crashes/` is evidence nobody has read yet — `crash.rs` says so in as many words.
    #[test]
    fn only_rotated_log_files_are_reclaimable() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let logs = home.path().join("logs");
        std::fs::create_dir_all(logs.join("services/api")).expect("a tree");
        std::fs::create_dir_all(logs.join("crashes")).expect("a tree");

        for (path, bytes) in [
            ("daemon.log", 10),
            ("daemon.log.1", 100),
            ("daemon.log.2", 200),
            ("daemon.log.tmp", 7),
            ("services/api/current.log", 10),
            ("services/api/current.log.1", 300),
            ("services/api/notes.txt", 9),
            ("crashes/2026-09-06.json", 400),
        ] {
            std::fs::write(logs.join(path), vec![0u8; bytes]).expect("a file");
        }

        let reclaimable = reclaimable_logs(&logs);

        let mut names: Vec<String> = reclaimable
            .targets
            .iter()
            .map(|path| {
                path.file_name()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();

        assert_eq!(names, ["current.log.1", "daemon.log.1", "daemon.log.2"]);
        assert_eq!(reclaimable.bytes, 600);
        assert_eq!(reclaimable.files, 3);
    }

    /// **T196.** The package index is cached as a root, its signature and a file per kind, and a
    /// cleanup takes all of them — and nothing that merely looks like one.
    #[test]
    fn the_cache_list_takes_the_package_index_a_kind_at_a_time() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let cache = home.path().join("cache");
        std::fs::create_dir_all(&cache).expect("a tree");

        for (path, bytes) in [
            ("index-v2.json", 10),
            ("index-v2.json.minisig", 20),
            ("index-v2-php.json", 30),
            ("index-v2-php-fpm.json", 40),
            ("index-v2-notes.txt", 50),
            ("index-v2-Not_A_Kind.json", 60),
            ("index-v3.json", 70),
            ("index-v2-php.json.part", 3),
            ("index-v2.json.minisig.part", 5),
            ("index-v2-notes.txt.part", 80),
        ] {
            std::fs::write(cache.join(path), vec![0u8; bytes]).expect("a file");
        }

        let reclaimable = reclaimable_cache(&cache);

        assert_eq!(
            reclaimable.bytes, 108,
            "only the index's own files, and what a write of one left half done"
        );
        assert_eq!(reclaimable.files, 6);
        assert!(
            reclaimable
                .targets
                .contains(&cache.join("index-v2-php-fpm.json"))
        );
        assert!(
            !reclaimable
                .targets
                .contains(&cache.join("index-v2-notes.txt"))
        );
    }

    /// The cache is a closed list of six things, and the three directories are emptied rather than
    /// removed: `Paths::bootstrap` created them and the daemon is running.
    #[test]
    fn the_cache_list_is_the_six_things_this_build_puts_there() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let cache = home.path().join("cache");
        std::fs::create_dir_all(cache.join("downloads")).expect("a tree");
        std::fs::create_dir_all(cache.join("updates/0.0.3")).expect("a tree");
        std::fs::create_dir_all(cache.join("diagnostics")).expect("a tree");

        for (path, bytes) in [
            ("index.json", 10),
            ("extensions.json", 20),
            ("latest.json", 30),
            ("downloads/abc.part", 40),
            ("updates/0.0.3/mixengined", 50),
            ("diagnostics/bundle.zip", 60),
            ("something-else.txt", 70),
        ] {
            std::fs::write(cache.join(path), vec![0u8; bytes]).expect("a file");
        }

        let reclaimable = reclaimable_cache(&cache);

        assert_eq!(reclaimable.bytes, 210, "the stray file is not ours to take");
        assert_eq!(reclaimable.files, 6);

        assert!(reclaimable.targets.contains(&cache.join("updates/0.0.3")));
        assert!(!reclaimable.targets.contains(&cache.join("downloads")));
        assert!(
            !reclaimable
                .targets
                .contains(&cache.join("something-else.txt"))
        );
    }

    /// A cache that has never been written to answers nothing, not an error.
    #[test]
    fn an_empty_cache_is_nothing_to_take() {
        let home = tempfile::tempdir().expect("a temporary directory");

        let reclaimable = reclaimable_cache(&home.path().join("cache"));

        assert!(reclaimable.targets.is_empty());
        assert_eq!(reclaimable.bytes, 0);
    }

    /// The sweep removes exactly its targets and counts what actually went — measured, not claimed.
    #[test]
    fn a_sweep_removes_its_targets_and_counts_what_went() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let cache = home.path().join("cache");
        std::fs::create_dir_all(cache.join("updates/0.0.3")).expect("a tree");
        std::fs::write(cache.join("index.json"), vec![0u8; 10]).expect("a file");
        std::fs::write(cache.join("updates/0.0.3/payload"), vec![0u8; 90]).expect("a file");
        std::fs::write(cache.join("keep-me"), vec![0u8; 5]).expect("a file");

        let reclaimable = reclaimable_cache(&cache);
        let outcome = sweep(&reclaimable);

        assert_eq!(
            outcome,
            mixengine_proto::Cleanup::Reclaimed {
                files: 2,
                bytes: 100
            }
        );
        assert!(!cache.join("index.json").exists());
        assert!(!cache.join("updates/0.0.3").exists());
        assert!(cache.join("updates").is_dir(), "the directory itself stays");
        assert!(cache.join("keep-me").exists());
    }

    /// Nothing to take is `Empty`, which is an answer and not a failure.
    #[test]
    fn a_sweep_with_no_targets_is_empty() {
        assert_eq!(
            sweep(&Reclaimable::default()),
            mixengine_proto::Cleanup::Empty {}
        );
    }

    /// A target that is already gone is not a failure: two cleanups in a row, or a rotation that
    /// shifted a copy away between the reading and the removal, are both ordinary.
    #[test]
    fn a_target_that_is_already_gone_is_not_a_failure() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let missing = home.path().join("never-existed");

        let outcome = sweep(&Reclaimable {
            targets: vec![missing],
            bytes: 10,
            files: 1,
        });

        assert_eq!(
            outcome,
            mixengine_proto::Cleanup::Reclaimed { files: 0, bytes: 0 }
        );
    }

    /// `daemon.log.1` is a rotated copy; `daemon.log`, `daemon.log.tmp` and `daemon.logs` are not.
    #[test]
    fn a_rotated_name_is_the_live_one_and_digits() {
        assert!(is_rotated("daemon.log.1", "daemon.log"));
        assert!(is_rotated("daemon.log.12", "daemon.log"));
        assert!(!is_rotated("daemon.log", "daemon.log"));
        assert!(!is_rotated("daemon.log.", "daemon.log"));
        assert!(!is_rotated("daemon.log.tmp", "daemon.log"));
        assert!(!is_rotated("daemon.log.1.bak", "daemon.log"));
        assert!(!is_rotated("daemon.logs.1", "daemon.log"));
        assert!(is_rotated("current.log.3", "current.log"));
    }
}
