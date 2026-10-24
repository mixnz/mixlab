//! Completing an install from its own payload —
//! `docs/decisions/0054-an-install-completes-itself-from-its-own-payload.md`.
//!
//! An update never adds a binary (`apply::swap`, rule 1). A release that gained one — Windows'
//! `mixengine-trampoline` — therefore reaches an install that updated itself without it. This puts
//! it there from the payload that install was just updated from, which is still unpacked in the
//! cache: only names on [`COMPLETABLE`], only from a payload whose `mixengined` is byte for byte
//! the one running, and never over a file that exists.

use std::path::{Path, PathBuf};

use crate::shims;

/// What an install may add to itself. **Never the window**: a headless install stays headless, which
/// is the reason `swap` adds nothing in the first place.
pub const COMPLETABLE: &[&str] = &[shims::TRAMPOLINE];

/// The staged program whose bytes must be the running daemon's.
const DAEMON: &str = "mixengined";

/// What [`complete`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Completed {
    /// Names copied in by this call.
    pub added: Vec<String>,
    /// Names that were missing and are still missing, with why.
    pub failed: Vec<(String, String)>,
    /// Whether another start could do better with the same payload: only a copy that failed on the
    /// install's side. Another build, the wrong shape or a name the payload does not carry will
    /// fail the same way every time, so the caller need not keep the payload for them.
    pub worth_retrying: bool,
}

/// The [`COMPLETABLE`] names with no file beside the program in `directory`.
#[must_use]
pub fn missing(directory: &Path) -> Vec<&'static str> {
    COMPLETABLE
        .iter()
        .copied()
        .filter(|name| !directory.join(executable(name)).exists())
        .collect()
}

/// Copy the missing [`COMPLETABLE`] names into `directory` from `staged`, the unpacked payload, when
/// its `mixengined` is `running`'s bytes. Never an error: every failure is in [`Completed::failed`].
#[must_use]
pub fn complete(directory: &Path, staged: &Path, running: &Path) -> Completed {
    let missing = missing(directory);
    let mut completed = Completed::default();

    if missing.is_empty() {
        return completed;
    }

    let refuse = |completed: &mut Completed, why: &str| {
        for name in &missing {
            completed.failed.push(((*name).to_owned(), why.to_owned()));
        }
    };

    let Some(payload) = staged_daemon_directory(staged) else {
        refuse(&mut completed, "the payload is not laid out as a release");
        return completed;
    };

    if !same_bytes(&payload.join(executable(DAEMON)), running) {
        refuse(&mut completed, "the staged payload is not this build");
        return completed;
    }

    for name in missing {
        let source = payload.join(executable(name));

        if !source.is_file() {
            completed
                .failed
                .push((name.to_owned(), "the payload does not carry it".to_owned()));
            continue;
        }

        match copy_in(&source, directory, name) {
            Ok(()) => completed.added.push(name.to_owned()),
            Err(why) => {
                completed.failed.push((name.to_owned(), why));
                completed.worth_retrying = true;
            }
        }
    }

    completed
}

/// `staged` itself when the daemon is at its top, or the one directory under it that holds it —
/// the Windows zip's `mixengine/`.
fn staged_daemon_directory(staged: &Path) -> Option<PathBuf> {
    if staged.join(executable(DAEMON)).is_file() {
        return Some(staged.to_path_buf());
    }

    let directories: Vec<PathBuf> = std::fs::read_dir(staged)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();

    match directories.as_slice() {
        [only] if only.join(executable(DAEMON)).is_file() => Some(only.clone()),
        _ => None,
    }
}

fn same_bytes(left: &Path, right: &Path) -> bool {
    matches!(
        (crate::install::sha256_of(left), crate::install::sha256_of(right)),
        (Ok(a), Ok(b)) if a == b
    )
}

/// Copied beside and renamed into place, so no reader ever meets half a program.
fn copy_in(source: &Path, directory: &Path, name: &str) -> Result<(), String> {
    let target = directory.join(executable(name));
    let staging = directory.join(format!("{name}.new{}", std::env::consts::EXE_SUFFIX));

    let result = std::fs::copy(source, &staging)
        .map(|_| ())
        .map_err(|error| format!("cannot copy it in: {error}"))
        .and_then(|()| {
            std::fs::rename(&staging, &target)
                .map_err(|error| format!("cannot move it into place: {error}"))
        })
        .and_then(|()| {
            mixengine_platform::install::make_executable(&target)
                .map_err(|error| format!("cannot make it executable: {error}"))
        });

    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }

    result
}

fn executable(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = std::env::consts::EXE_SUFFIX;

    fn file(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(path, bytes).expect("a file");
    }

    /// An install with a daemon and no trampoline, and a payload with both under `mixengine/`.
    struct Fixture {
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let fixture = Self {
                root: tempfile::tempdir().expect("a directory"),
            };
            file(&fixture.running(), b"the daemon");
            file(
                &fixture.payload().join(format!("mixengined{EXE}")),
                b"the daemon",
            );
            file(
                &fixture.payload().join(format!("mixengine-trampoline{EXE}")),
                b"the trampoline",
            );
            fixture
        }
        fn directory(&self) -> PathBuf {
            self.root.path().join("install")
        }
        fn running(&self) -> PathBuf {
            self.directory().join(format!("mixengined{EXE}"))
        }
        fn staged(&self) -> PathBuf {
            self.root.path().join("staged")
        }
        fn payload(&self) -> PathBuf {
            self.staged().join("mixengine")
        }
        fn trampoline(&self) -> PathBuf {
            self.directory().join(format!("mixengine-trampoline{EXE}"))
        }
        fn complete(&self) -> Completed {
            complete(&self.directory(), &self.staged(), &self.running())
        }
    }

    #[test]
    fn a_missing_trampoline_is_copied_in_and_a_second_pass_adds_nothing() {
        let fixture = Fixture::new();

        let first = fixture.complete();
        assert_eq!(
            first.added,
            vec!["mixengine-trampoline".to_owned()],
            "{first:?}"
        );
        assert_eq!(
            std::fs::read(fixture.trampoline()).expect("added"),
            b"the trampoline"
        );

        assert_eq!(fixture.complete(), Completed::default());
        assert!(
            !fixture
                .directory()
                .join(format!("mixengine-trampoline.new{EXE}"))
                .exists()
        );
    }

    #[test]
    fn a_flat_payload_is_found_as_well() {
        let fixture = Fixture::new();
        std::fs::rename(fixture.payload(), fixture.root.path().join("flat")).expect("moved");
        std::fs::remove_dir_all(fixture.staged()).ok();
        std::fs::rename(fixture.root.path().join("flat"), fixture.staged()).expect("moved");

        assert_eq!(
            fixture.complete().added,
            vec!["mixengine-trampoline".to_owned()]
        );
    }

    /// Review focus 1: the payload is another build.
    #[test]
    fn a_payload_that_is_not_this_build_adds_nothing_and_says_why() {
        let fixture = Fixture::new();
        file(
            &fixture.payload().join(format!("mixengined{EXE}")),
            b"another daemon",
        );

        let completed = fixture.complete();
        assert!(completed.added.is_empty());
        assert_eq!(completed.failed.len(), 1);
        assert!(
            completed.failed[0].1.contains("not this build"),
            "{completed:?}"
        );
        assert!(!fixture.trampoline().exists());
    }

    /// Review focus 2: the window is not on the list, whatever the payload carries.
    #[test]
    fn the_window_is_never_added() {
        let fixture = Fixture::new();
        file(
            &fixture.payload().join(format!("mixlab{EXE}")),
            b"the window",
        );

        fixture.complete();
        assert!(!fixture.directory().join(format!("mixlab{EXE}")).exists());
        assert!(!COMPLETABLE.contains(&"mixlab"));
    }

    #[test]
    fn no_staging_directory_adds_nothing() {
        let fixture = Fixture::new();
        std::fs::remove_dir_all(fixture.staged()).expect("removed");

        let completed = fixture.complete();
        assert!(completed.added.is_empty());
        assert!(
            completed.failed[0].1.contains("not laid out"),
            "{completed:?}"
        );
    }

    #[test]
    fn a_name_the_payload_does_not_carry_is_reported() {
        let fixture = Fixture::new();
        std::fs::remove_file(fixture.payload().join(format!("mixengine-trampoline{EXE}")))
            .expect("removed");

        let completed = fixture.complete();
        assert!(
            completed.failed[0].1.contains("does not carry"),
            "{completed:?}"
        );
    }

    #[test]
    fn a_payload_entry_that_is_a_directory_is_reported_and_nothing_is_written() {
        let fixture = Fixture::new();
        let entry = fixture.payload().join(format!("mixengine-trampoline{EXE}"));
        std::fs::remove_file(&entry).expect("removed");
        std::fs::create_dir(&entry).expect("a directory in its place");

        let completed = fixture.complete();
        assert_eq!(completed.failed.len(), 1, "{completed:?}");
        assert!(!fixture.trampoline().exists());
        assert!(
            !fixture
                .directory()
                .join(format!("mixengine-trampoline.new{EXE}"))
                .exists()
        );
    }

    #[test]
    fn a_file_that_exists_is_not_overwritten() {
        let fixture = Fixture::new();
        file(&fixture.trampoline(), b"placed by an installer");

        assert_eq!(fixture.complete(), Completed::default());
        assert_eq!(
            std::fs::read(fixture.trampoline()).expect("kept"),
            b"placed by an installer"
        );
    }

    /// A payload that will never do — another build, the wrong shape, a name it does not carry —
    /// is not worth keeping for another start: the next one would fail the same way.
    #[test]
    fn a_payload_that_will_never_do_is_not_worth_retrying() {
        let fixture = Fixture::new();
        file(
            &fixture.payload().join(format!("mixengined{EXE}")),
            b"another daemon",
        );

        let completed = fixture.complete();
        assert!(!completed.failed.is_empty());
        assert!(!completed.worth_retrying, "{completed:?}");
    }

    /// A copy that failed on the install's side may succeed next time, so the payload is kept.
    #[test]
    fn a_copy_that_failed_is_worth_retrying() {
        let fixture = Fixture::new();
        // The install directory is gone from under the running daemon's path: nothing can be
        // written into it, and the payload itself is fine.
        let running = fixture
            .root
            .path()
            .join(format!("elsewhere/mixengined{EXE}"));
        file(&running, b"the daemon");
        std::fs::remove_dir_all(fixture.directory()).expect("removed");

        let completed = complete(&fixture.directory(), &fixture.staged(), &running);
        assert_eq!(completed.failed.len(), 1, "{completed:?}");
        assert!(completed.worth_retrying, "{completed:?}");
    }
}
