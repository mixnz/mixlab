//! Being told when the entries of a directory change — roadmap task **T185b**, ADR 0057.
//!
//! `npm install -g yarn` writes a program into the Node install's own bindir and tells nobody. The
//! daemon turns that into a command, and this is how it hears about it: the kernel's own
//! notification on each system — `ReadDirectoryChangesW`, FSEvents, inotify — through `notify`,
//! which is the per-OS work this crate exists to be the only place for.
//!
//! **An idle machine pays nothing.** Nothing here wakes on a timer: a callback runs when a watched
//! directory gains, loses or renames an entry, and at no other time. That is what replaced the
//! two-second `stat` loop T131 shipped with.
//!
//! **Not recursive.** What becomes a command is a file directly inside a bindir; a package's own
//! files, which `npm` writes by the thousand under `node_modules`, are not. A directory that does not
//! exist yet — a fresh Python's `Scripts` — is watched through its nearest existing parent, so its
//! creation is the event, and the caller arms a new watch once it hears it.

use std::path::{Path, PathBuf};

use notify::event::{EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

/// The watch, for as long as it is held. Dropping it stops every notification it asked for.
#[derive(Debug)]
pub struct Watch {
    _watcher: Option<RecommendedWatcher>,
}

/// A directory this watch could not ask about, and why. The rest are still watched.
#[derive(Debug)]
pub struct Unwatched {
    /// The directory asked for.
    pub path: PathBuf,

    /// What the system said.
    pub reason: String,
}

/// Watch the entries of each of `directories`, calling `changed` whenever one of them gains, loses or
/// renames one.
///
/// `changed` runs on a thread of the watcher's own and is told nothing about what changed: every
/// caller here answers any change by looking again, and a burst of events — `npm` writes several
/// files for one tool — is theirs to coalesce.
pub fn watch<F>(directories: &[PathBuf], changed: F) -> (Watch, Vec<Unwatched>)
where
    F: Fn() + Send + 'static,
{
    let handler = move |event: notify::Result<notify::Event>| {
        // An error from the watcher's own queue — an overflow, typically — means events were lost,
        // and the answer to a lost event is the same as the answer to any: look again.
        if event
            .as_ref()
            .map_or(true, |event| changes_entries(&event.kind))
        {
            changed();
        }
    };

    let mut watcher = match notify::recommended_watcher(handler) {
        Ok(watcher) => watcher,
        Err(error) => {
            let unwatched = directories
                .iter()
                .map(|path| Unwatched {
                    path: path.clone(),
                    reason: error.to_string(),
                })
                .collect();

            return (Watch { _watcher: None }, unwatched);
        }
    };

    let mut unwatched = Vec::new();

    for directory in directories {
        let Some(target) = nearest_existing(directory) else {
            unwatched.push(Unwatched {
                path: directory.clone(),
                reason: "neither it nor any directory above it exists".to_owned(),
            });
            continue;
        };

        if let Err(error) = watcher.watch(&target, RecursiveMode::NonRecursive) {
            unwatched.push(Unwatched {
                path: directory.clone(),
                reason: error.to_string(),
            });
        }
    }

    (
        Watch {
            _watcher: Some(watcher),
        },
        unwatched,
    )
}

/// Whether an event is one that can change which files a directory holds.
///
/// A read, an attribute change and a write into a file that already existed cannot: the set of
/// names is the same afterwards. Anything the backend could not classify is taken as a change,
/// since the cost of a needless look is one scan and the cost of a missed one is a command that
/// does not exist.
fn changes_entries(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(_) => false,
        EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_)) => false,
        EventKind::Create(_)
        | EventKind::Remove(_)
        | EventKind::Modify(_)
        | EventKind::Any
        | EventKind::Other => true,
    }
}

/// `directory`, or the closest directory above it that exists.
fn nearest_existing(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|candidate| candidate.is_dir())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::mpsc;
    use std::time::Duration;

    /// Long enough for the slowest of the three backends to deliver one event, on a loaded runner.
    const DELIVERY: Duration = Duration::from_secs(10);

    fn watched(directories: &[PathBuf]) -> (Watch, mpsc::Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        let (watch, unwatched) = watch(directories, move || {
            let _ = sender.send(());
        });

        assert!(unwatched.is_empty(), "could not watch: {unwatched:?}");

        (watch, receiver)
    }

    /// What `npm install -g yarn` does to a bindir: a file appears in it.
    #[test]
    fn a_file_created_in_a_watched_directory_is_heard() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let (_watch, heard) = watched(&[home.path().to_path_buf()]);

        std::fs::write(home.path().join("yarn"), b"").expect("write a file");

        heard
            .recv_timeout(DELIVERY)
            .expect("no notification arrived for a file created in the watched directory");
    }

    /// A fresh Python has no `Scripts` until the first `pip install` makes one, and that creation is
    /// what has to be heard: the caller then watches `Scripts` itself.
    #[test]
    fn a_directory_that_does_not_exist_is_watched_through_its_parent() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let scripts = home.path().join("Scripts");
        let (_watch, heard) = watched(std::slice::from_ref(&scripts));

        std::fs::create_dir(&scripts).expect("create the directory");

        heard
            .recv_timeout(DELIVERY)
            .expect("no notification arrived when the missing directory was created");
    }

    /// The watch belongs to its value: once dropped, nothing more is delivered.
    #[test]
    fn a_dropped_watch_hears_nothing() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let (sender, heard) = mpsc::channel();

        // A sender kept here as well, so that the watch dropping its own copy does not disconnect
        // the channel: a disconnected receiver answers at once, and this would pass for that reason.
        let kept = sender.clone();
        let (watch, _) = watch(&[home.path().to_path_buf()], move || {
            let _ = sender.send(());
        });

        drop(watch);
        std::fs::write(home.path().join("yarn"), b"").expect("write a file");

        assert_eq!(
            heard.recv_timeout(Duration::from_millis(500)),
            Err(mpsc::RecvTimeoutError::Timeout),
            "a watch that had been dropped still delivered a notification"
        );
        drop(kept);
    }
}
