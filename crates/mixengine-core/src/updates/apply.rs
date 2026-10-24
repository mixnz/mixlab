//! Putting a release on this machine.
//!
//! # Two halves, and only the second one is new
//!
//! **Staging is an install**, and [`crate::install::Installer`] is the code that does installs here.
//! The feed's artifact *is* an [`Artifact`], which is what makes that literally true rather than
//! nearly: the resumable `.part` file, the SHA-256 the signed document carries, the archive entry
//! that would escape its root, the `provides` the payload promised, and the staged `mixengined` run
//! before anything is replaced — all of it is already written, already tested, and already the code
//! path every runtime this product installs goes through. So [`stage`] is a call and not an
//! implementation.
//!
//! **The swap is the part an install never has to do**, because an install writes somewhere nothing
//! is running from. This one replaces the binaries of the process performing it, which is why it is
//! rename-then-write and why it undoes itself.
//!
//! # The helper beside the program is swapped too
//!
//! `mixengine-elevate` beside the program is the copy the daemon installs a privileged helper from,
//! and it sits in a directory this account already writes, so skipping it protected nothing and
//! left the daemon an old copy to install. The *privileged* copy is somewhere only an administrator
//! can write, never under the install directory, so no swap can reach it: the daemon replaces that
//! one through a prompt, with the signature checked inside the elevated context.
//! `docs/features/updates.md`'s rule, that an auto-updated binary running as root is a local
//! privilege-escalation vector, holds because of where that copy lives, not because of a name here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[cfg(feature = "network")]
use crate::index::Artifact;
#[cfg(feature = "network")]
use crate::install::{Installer, NotAnArchive, SmokeTest, Watcher};
use crate::{Error, Result};

/// The executable the smoke test runs, by the name the payload publishes it under.
///
/// Public because it is a name `packaging/` has to keep: a payload whose `provides` does not carry
/// this key is one [`stage`] refuses with [`InstallError::MissingFromArtifact`], which is what
/// `crates/mixengine-core/tests/packaging.rs` checks the release list against.
///
/// [`InstallError::MissingFromArtifact`]: crate::InstallError::MissingFromArtifact
pub const SMOKE_EXECUTABLE: &str = "mixengined";

/// The window's key in a payload's `provides`, and what macOS wraps it in.
///
/// **The one payload entry that is not resolved by appending an executable suffix.** On macOS a
/// windowed application is a directory called [`WINDOW_BUNDLE`], which no suffix produces, so
/// [`WINDOW`] alone is looked up through
/// [`mixengine_platform::install::application_file_name`] — `installed_name` below — and
/// [`WINDOW_BUNDLE`] is the second argument that call takes.
///
/// Published under these two names because that is what this module has always published and what
/// `crates/mixengine-core/tests/packaging.rs` pins to `packaging/common.sh`; **defined in
/// [`crate::window`]**, which holds all four of the window's names — a display name and a URL
/// scheme are not an updater's business, and four names in two places is the drift this indirection
/// removes.
pub use crate::window::{BUNDLE as WINDOW_BUNDLE, EXECUTABLE as WINDOW};

/// What is renamed onto a binary before its replacement is written.
///
/// Removed by the next daemon start that succeeds, which gives the property that matters for free:
/// these survive exactly as long as they are the only way back, and a daemon that comes up has
/// proved they are not needed.
pub const OLD_SUFFIX: &str = ".old";

/// What one swap did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Swapped {
    /// The binaries that were replaced, by name.
    pub replaced: Vec<String>,

    /// The binaries the payload carried that this update deliberately did not replace.
    ///
    /// Anything the payload carries that this install does not have — which is how a release
    /// that gains a binary behaves against an install predating it.
    pub kept: Vec<String>,
}

/// Download, verify, unpack and smoke-test a payload, leaving it under `into`.
///
/// **Every step is [`Installer::install`]'s**, which is what makes this function short. The one
/// worth naming on its own is the last: running the staged `mixengined` before anything is replaced
/// is the difference between *"the update was refused and nothing changed"* and *"MixEngine no
/// longer starts"*. `docs/features/updates.md` records that Windows Code Integrity judges each
/// file separately, again after every update, with refusal rather than a warning at the end of it —
/// and a payload for the wrong architecture or past this machine's glibc floor fails here too.
///
/// A staging directory left by an attempt that was killed is removed first: [`Installer::install`]
/// refuses a destination that exists, and what is in one is the wrong half of an archive nobody
/// wants.
///
/// # Errors
///
/// [`InstallError::ArtifactTransport`] and its neighbours from the download,
/// [`InstallError::ArtifactChecksum`] when what arrived is not what the signed feed promised,
/// [`InstallError::UnsafeArchiveEntry`] from unpacking, [`InstallError::MissingFromArtifact`] when
/// the payload does not hold what it declared, [`InstallError::SmokeTestFailed`] when the staged
/// daemon will not run here, and [`Error::Io`] for the staging directory itself. Every one of them
/// leaves the installed binaries untouched.
///
/// [`InstallError::ArtifactChecksum`]: crate::InstallError::ArtifactChecksum
/// [`InstallError::ArtifactTransport`]: crate::InstallError::ArtifactTransport
/// [`InstallError::MissingFromArtifact`]: crate::InstallError::MissingFromArtifact
/// [`InstallError::SmokeTestFailed`]: crate::InstallError::SmokeTestFailed
/// [`InstallError::UnsafeArchiveEntry`]: crate::InstallError::UnsafeArchiveEntry
#[cfg(feature = "network")]
pub async fn stage<W: Watcher>(
    installer: &Installer,
    artifact: &Artifact,
    into: &Path,
    watcher: &W,
) -> Result<PathBuf> {
    if into.exists() {
        tokio::fs::remove_dir_all(into)
            .await
            .map_err(|source| Error::Io {
                action: "remove the staging directory left by a previous update",
                path: into.to_path_buf(),
                source,
            })?;
    }

    if let Some(parent) = into.parent() {
        crate::paths::create_dir(parent)?;
    }

    let smoke = SmokeTest {
        executable: SMOKE_EXECUTABLE.to_owned(),
        args: vec!["--version".to_owned()],
        unset: &[],
    };

    let installed = installer
        .install(artifact, into, Some(&smoke), NotAnArchive::Refuse, watcher)
        .await?;

    Ok(installed.path)
}

/// Replace the installed binaries with the staged ones, or put everything back.
///
/// `provides` is the payload's own map of executable name to path inside the archive — the payload's
/// contents and not a list compiled into this binary, which is what lets an installed 0.2.0 take a
/// 0.3.0 payload that carries a binary 0.2.0 never had.
///
/// Two rules, in this order, per name:
///
/// 1. A name this install does not have is skipped and reported as kept. Nothing is *added* by an
///    update: a binary appearing for the first time is an install's business, not an update's.
/// 2. Otherwise `rename(target, target.old)` and then copy the staged file to `target`.
///
/// **Rename rather than overwrite**, which is what makes this work at all on Windows: the running
/// `mix.exe` is one of the files being replaced, an open image cannot be deleted or written, and it
/// *can* be renamed — after which the freed name accepts the new file. On Unix an overwrite would
/// also be safe, and doing it the same way on both keeps one code path and one set of tests.
///
/// **Copy and not rename from the staging directory**: the cache is inside `MIXENGINE_HOME` and the
/// install directory need not be on the same volume.
///
/// # Errors
///
/// [`Error::Io`] for the first rename or copy that fails — and every rename made before it is undone
/// first, so a partial swap is never left behind. What this cannot undo is the *stop* that preceded
/// it, which is why the caller starts the services again on this path.
pub fn swap(
    staged: &Path,
    provides: &BTreeMap<String, String>,
    directory: &Path,
) -> Result<Swapped> {
    let mut swapped = Swapped::default();
    // What has been renamed, so a failure part way through can put it back. In order, and undone in
    // reverse, which costs nothing and is what a reader expects of an unwind.
    let mut renamed: Vec<(PathBuf, PathBuf)> = Vec::new();

    for (name, relative) in provides {
        let target = directory.join(installed_name(name));

        if !target.exists() {
            swapped.kept.push(name.clone());
            continue;
        }

        let old = with_old_suffix(&target);
        let source = staged.join(relative);

        // A `.old` from an update whose daemon never came up. Removed rather than refused: it is
        // the one thing in the way, and the copy about to be made is the way back from here.
        remove_any(&old);

        if let Err(error) = replace(&source, &target, &old) {
            unwind(&renamed);
            return Err(error);
        }

        renamed.push((target, old));
        swapped.replaced.push(name.clone());
    }

    Ok(swapped)
}

/// Remove the `.old` files a completed update left beside the binaries it replaced.
///
/// Called by the **next daemon start that succeeds**, which is what makes it safe: a daemon that is
/// answering has proved the binaries beside these are the ones this machine runs. Failures are
/// reported as a count and never as an error — on Windows a `mix.exe.old` is still held open by the
/// `mix` that ran the update, and a `mixlab.exe.old` by the window still running the image it started
/// from; both go at the start after that one.
///
/// **A `.old` may be a directory.** macOS's window is an application bundle, and `remove_file` does
/// not remove one: left as it was, a machine that updated twice would keep every bundle it had ever
/// run.
#[must_use]
pub fn discard_old(directory: &Path, names: &[String]) -> usize {
    names
        .iter()
        .filter(|name| {
            let old = with_old_suffix(&directory.join(installed_name(name)));

            if !old.exists() {
                return false;
            }
            if old.is_dir() {
                std::fs::remove_dir_all(&old).is_ok()
            } else {
                std::fs::remove_file(&old).is_ok()
            }
        })
        .count()
}

/// One entry's swap: rename what is installed out of the way, then write the new one.
///
/// **A file or a tree.** The window is a directory on macOS ([`WINDOW_BUNDLE`]), and a bundle half
/// written is an application the operating system refuses to start, which is worse than the update
/// that was refused. So a tree that cannot be finished is removed and the rename undone, exactly as
/// the file path already did.
fn replace(source: &Path, target: &Path, old: &Path) -> Result<()> {
    std::fs::rename(target, old).map_err(|source| Error::Io {
        action: "rename the installed binary out of the way",
        path: target.to_path_buf(),
        source,
    })?;

    let copied = if source.is_dir() {
        copy_tree(source, target)
    } else {
        std::fs::copy(source, target)
            .map(|_| ())
            .map_err(|error| Error::Io {
                action: "copy the staged binary into place",
                path: target.to_path_buf(),
                source: error,
            })
    };

    if let Err(error) = copied {
        // The rename this function made, undone by this function: the caller's unwind covers the
        // ones made before it, and leaving a half-done name for it to guess at would be worse.
        remove_any(target);
        let _ = std::fs::rename(old, target);
        return Err(error);
    }

    // A directory needs no bit set: what has to be executable is the file inside it, and the archive
    // carried its mode across. On Windows this is a no-op either way.
    if target.is_dir() {
        return Ok(());
    }

    mixengine_platform::install::make_executable(target).map_err(|error| Error::Io {
        action: "make the replacement executable",
        path: target.to_path_buf(),
        source: std::io::Error::other(error.to_string()),
    })
}

/// Copy a directory into `target`, file by file, creating what it needs.
///
/// **`std::fs::copy` per file rather than a rename of the tree**: the staging directory is inside
/// `MIXENGINE_HOME` and the install directory need not be on the same volume, which is the same
/// reason the file path copies. `copy` carries the mode across on Unix, so a bundle's executable
/// arrives executable — the tarball preserved the bit and this preserves it again.
///
/// # Errors
///
/// [`Error::Io`] for the first directory or file that cannot be written. The caller removes what was
/// written before it.
fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    let io = |action: &'static str, path: &Path| {
        let path = path.to_path_buf();
        move |error: std::io::Error| Error::Io {
            action,
            path,
            source: error,
        }
    };

    std::fs::create_dir_all(target).map_err(io("create the replacement directory", target))?;

    for entry in std::fs::read_dir(source).map_err(io("read the staged directory", source))? {
        let entry = entry.map_err(io("read the staged directory", source))?;
        let from = entry.path();
        let to = target.join(entry.file_name());

        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(io("copy the staged file into place", &to))?;
        }
    }

    Ok(())
}

/// Remove a path whichever shape it is, and say nothing when there was nothing there.
///
/// `remove_file` does not remove a directory and `remove_dir_all` does not remove a file, and the
/// window is one shape on macOS and the other everywhere else. Both failures are ignorable by
/// construction here: every caller is either clearing something out of the way or abandoning
/// something half-written, and there is a rename behind each of them that is the real report.
fn remove_any(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

/// Put back everything a failed swap had already moved.
fn unwind(renamed: &[(PathBuf, PathBuf)]) {
    for (target, old) in renamed.iter().rev() {
        // The new file is in the way of its own predecessor, and it is the thing being abandoned.
        remove_any(target);

        if let Err(error) = std::fs::rename(old, target) {
            // Nothing left to try, and a warning is the only honest thing: the caller is about to
            // report the failure that started this, and the path is what somebody needs.
            tracing::warn!(
                path = %target.display(),
                kept = %old.display(),
                %error,
                "an update that was rolled back could not put a binary back under its own name"
            );
        }
    }
}

/// What a binary is called on this system.
fn binary_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// What one payload name is called on this machine's disk.
///
/// Four of the five are [`binary_name`]. The fifth is [`WINDOW`], and asking the platform is the
/// whole point: on macOS it is a bundle directory, and this crate may not hold that fact itself
/// (`CLAUDE.md`). One special case, stated once, rather than every caller remembering it.
fn installed_name(name: &str) -> String {
    if name == WINDOW {
        mixengine_platform::install::application_file_name(WINDOW, WINDOW_BUNDLE)
    } else {
        binary_name(name)
    }
}

/// `mix.exe` → `mix.exe.old`.
///
/// Appended rather than substituted, so `mix.exe.old` is not something Windows will start by
/// accident and so the name says which file it came from.
fn with_old_suffix(path: &Path) -> PathBuf {
    let mut name = path.to_path_buf().into_os_string();
    name.push(OLD_SUFFIX);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A staged payload and an install directory, each holding the named binaries.
    fn layout(
        staged_names: &[&str],
        installed_names: &[&str],
    ) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("a temporary directory");
        let staged = root.path().join("staged");
        let installed = root.path().join("installed");
        std::fs::create_dir_all(staged.join("mixengine")).expect("a staging directory");
        std::fs::create_dir_all(&installed).expect("an install directory");

        for name in staged_names {
            std::fs::write(staged.join("mixengine").join(binary_name(name)), b"new")
                .expect("a staged file");
        }
        for name in installed_names {
            std::fs::write(installed.join(binary_name(name)), b"old").expect("an installed file");
        }

        (root, staged, installed)
    }

    /// A staged payload and an install directory in which one entry is a *directory* rather than a
    /// file — the shape a macOS application bundle takes. `file` is the name that entry has on disk.
    ///
    /// **The on-disk name and not a payload key**, because the two are the same for four of the five
    /// names and are not for the fifth: the window is `MixLab.app` on macOS and `binary_name` never
    /// produces that. Taking the name already resolved is what lets the tree tests below use an
    /// ordinary binary — [`replace`] branches on `source.is_dir()` and never on the operating system,
    /// so they exercise the whole tree path on all three — and lets the window's own test ask
    /// `mixengine_platform::install::application_file_name` for its name, which is the question this
    /// helper must not answer for it.
    fn tree_layout(file: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("a temporary directory");
        let staged = root.path().join("staged");
        let installed = root.path().join("installed");

        let inside = staged.join("mixengine").join(file);
        std::fs::create_dir_all(inside.join("Contents/MacOS")).expect("a staged bundle");
        std::fs::write(inside.join("Contents/MacOS/window"), b"new").expect("its executable");
        std::fs::write(inside.join("Contents/Info.plist"), b"new plist").expect("its plist");

        let here = installed.join(file);
        std::fs::create_dir_all(here.join("Contents/MacOS")).expect("an installed bundle");
        std::fs::write(here.join("Contents/MacOS/window"), b"old").expect("its executable");

        (root, staged, installed)
    }

    /// One `provides` row: the payload's key, and the path that key sits at inside the archive.
    ///
    /// Beside [`provides`], which derives the path from the key with [`binary_name`] — true of the
    /// four ordinary binaries and false of the window, whose archive entry is its bundle.
    fn provides_at(name: &str, file: &str) -> BTreeMap<String, String> {
        [(name.to_owned(), format!("mixengine/{file}"))]
            .into_iter()
            .collect()
    }

    /// The map the feed carries, as `packaging/feed.sh` computes it from the archive.
    fn provides(names: &[&str]) -> BTreeMap<String, String> {
        names
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    format!("mixengine/{}", binary_name(name)),
                )
            })
            .collect()
    }

    #[test]
    fn the_swap_replaces_what_is_installed_and_keeps_the_old_file_beside_it() {
        let (_root, staged, installed) = layout(&["mix", "mixengined"], &["mix", "mixengined"]);

        let swapped = swap(&staged, &provides(&["mix", "mixengined"]), &installed).expect("a swap");

        assert_eq!(
            swapped.replaced,
            vec!["mix".to_owned(), "mixengined".to_owned()]
        );
        assert_eq!(
            std::fs::read(installed.join(binary_name("mix"))).expect("the new file"),
            b"new"
        );
        assert_eq!(
            std::fs::read(with_old_suffix(&installed.join(binary_name("mix"))))
                .expect("the old file"),
            b"old"
        );
    }

    /// The helper beside the program is swapped like every other binary: it is the copy the daemon
    /// installs from, and an update that left it behind would leave the daemon nothing current to
    /// install. The *privileged* copy is never under the install directory, so no swap can reach
    /// it.
    #[test]
    fn the_helper_beside_the_program_is_swapped_like_every_other_binary() {
        let helper = "mixengine-elevate";
        let (_root, staged, installed) = layout(&["mix", helper], &["mix", helper]);

        let swapped = swap(&staged, &provides(&["mix", helper]), &installed).expect("a swap");

        assert!(swapped.kept.is_empty(), "{swapped:?}");
        assert!(swapped.replaced.contains(&helper.to_owned()), "{swapped:?}");
        assert_ne!(
            std::fs::read(installed.join(binary_name(helper))).expect("the helper"),
            b"old",
            "the helper beside the program is the staged one now"
        );
    }

    /// A payload that gained a binary against an install that does not have it yet — which is how
    /// `mixengine-trampoline` first reached an install that updated itself.
    #[test]
    fn a_binary_this_install_does_not_have_is_left_alone() {
        let (_root, staged, installed) = layout(&["mix", "mixengine-shim"], &["mix"]);

        let swapped =
            swap(&staged, &provides(&["mix", "mixengine-shim"]), &installed).expect("a swap");

        assert_eq!(swapped.replaced, vec!["mix".to_owned()]);
        assert_eq!(swapped.kept, vec!["mixengine-shim".to_owned()]);
        assert!(!installed.join(binary_name("mixengine-shim")).exists());
    }

    /// The rollback. A staged directory missing its second file fails half way, and everything the
    /// first half moved comes back.
    #[test]
    fn a_swap_that_fails_half_way_puts_everything_back() {
        let (_root, staged, installed) = layout(&["mix"], &["mix", "mixengined"]);

        swap(&staged, &provides(&["mix", "mixengined"]), &installed)
            .expect_err("the payload has no mixengined to copy");

        assert_eq!(
            std::fs::read(installed.join(binary_name("mix"))).expect("the old file, back"),
            b"old"
        );
        assert!(
            !with_old_suffix(&installed.join(binary_name("mix"))).exists(),
            "the rename was undone rather than left for somebody to find"
        );
        assert_eq!(
            std::fs::read(installed.join(binary_name("mixengined"))).expect("untouched"),
            b"old"
        );
    }

    /// A `.old` from an update whose daemon never came up must not stop the next attempt.
    #[test]
    fn a_leftover_old_file_does_not_refuse_the_next_swap() {
        let (_root, staged, installed) = layout(&["mix"], &["mix"]);
        std::fs::write(
            with_old_suffix(&installed.join(binary_name("mix"))),
            b"older",
        )
        .expect("a leftover");

        swap(&staged, &provides(&["mix"]), &installed).expect("a swap");

        assert_eq!(
            std::fs::read(with_old_suffix(&installed.join(binary_name("mix"))))
                .expect("the old file"),
            b"old",
            "the file replaced by this swap, not the one left by the last"
        );
    }

    /// macOS's window is a directory, and an update that could only replace files would leave a new
    /// daemon beside a window from the release before it.
    ///
    /// Staged under an ordinary binary's name so this runs the same way on all three systems: what
    /// is being tested is [`replace`]'s branch on `source.is_dir()`, and that branch has no `cfg`.
    /// Which *name* the window is looked up under is the test after next.
    #[test]
    fn the_swap_replaces_a_directory_as_a_tree() {
        let file = binary_name("mixengined");
        let (_root, staged, installed) = tree_layout(&file);

        let swapped = swap(&staged, &provides_at("mixengined", &file), &installed).expect("a swap");

        assert_eq!(swapped.replaced, vec!["mixengined".to_owned()]);
        let here = installed.join(&file);
        assert_eq!(
            std::fs::read(here.join("Contents/MacOS/window")).expect("the new executable"),
            b"new"
        );
        assert!(
            here.join("Contents/Info.plist").is_file(),
            "a file the old bundle did not have is part of the new one"
        );
        assert_eq!(
            std::fs::read(with_old_suffix(&here).join("Contents/MacOS/window"))
                .expect("the old executable"),
            b"old",
            "the bundle that was replaced is kept beside the one that replaced it"
        );
    }

    /// **The window is looked up under the name this system installs it as** — [`installed_name`],
    /// and not [`binary_name`].
    ///
    /// The fixture asks `mixengine_platform::install::application_file_name` for that name, which is
    /// the only way to write this test once for three systems — and it is the question that matters:
    /// a `swap` that appended an executable suffix here would look for `mixlab` in a directory
    /// holding `MixLab.app`, find nothing, and report the window as *kept* on every macOS update
    /// for ever, with no error and no log line.
    #[test]
    fn the_window_is_looked_up_by_the_name_this_system_installs_it_under() {
        let file = mixengine_platform::install::application_file_name(WINDOW, WINDOW_BUNDLE);
        let (_root, staged, installed) = tree_layout(&file);

        let swapped = swap(&staged, &provides_at(WINDOW, &file), &installed).expect("a swap");

        assert_eq!(swapped.replaced, vec![WINDOW.to_owned()]);
        assert_eq!(
            std::fs::read(installed.join(&file).join("Contents/MacOS/window"))
                .expect("the new executable"),
            b"new"
        );
    }

    /// And it comes back. A tree half copied is a bundle macOS would refuse to start, which is a
    /// worse outcome than the update that was refused.
    #[test]
    fn a_swap_that_fails_on_a_tree_puts_everything_back() {
        let file = binary_name("mixengined");
        let (_root, staged, installed) = tree_layout(&file);
        // A second name the payload promises and does not carry. `provides` is a BTreeMap, so "mix"
        // is walked before "mixengined": the failure comes first and the tree is never touched,
        // which is the unwind's *other* half. The tree's own rename is undone by the case below it.
        std::fs::write(installed.join(binary_name("mix")), b"old").expect("an installed file");

        swap(&staged, &provides(&["mix", "mixengined"]), &installed)
            .expect_err("the payload has no mix to copy");

        let here = installed.join(&file);
        assert_eq!(
            std::fs::read(here.join("Contents/MacOS/window"))
                .expect("the old executable, untouched"),
            b"old"
        );
        assert!(
            !with_old_suffix(&here).exists(),
            "nothing was renamed, so nothing is left for somebody to find"
        );
    }

    /// The unwind's own half: a tree that *was* replaced, and a name after it that fails.
    #[test]
    fn a_replaced_tree_is_put_back_when_a_later_name_fails() {
        let file = binary_name("mixengined");
        let (_root, staged, installed) = tree_layout(&file);
        // "mixengined" sorts before "zzz", so the tree is swapped and then the walk fails.
        std::fs::write(installed.join(binary_name("zzz")), b"old").expect("an installed file");

        swap(&staged, &provides(&["mixengined", "zzz"]), &installed)
            .expect_err("the payload has no zzz to copy");

        let here = installed.join(&file);
        assert_eq!(
            std::fs::read(here.join("Contents/MacOS/window")).expect("the old executable, back"),
            b"old"
        );
        assert!(
            !here.join("Contents/Info.plist").exists(),
            "the tree that was abandoned was removed rather than merged into the one it replaced"
        );
        assert!(!with_old_suffix(&here).exists(), "the rename was undone");
    }

    /// The `.old` of a bundle is a directory, and `remove_file` does not remove one — so a machine
    /// that updated twice would keep every bundle it had ever run.
    #[test]
    fn the_old_tree_of_a_finished_update_is_discarded() {
        let file = binary_name("mixengined");
        let (_root, staged, installed) = tree_layout(&file);
        let swapped = swap(&staged, &provides_at("mixengined", &file), &installed).expect("a swap");

        assert_eq!(discard_old(&installed, &swapped.replaced), 1);
        assert!(!with_old_suffix(&installed.join(&file)).exists());
    }

    /// A leftover `.old` tree from an update whose daemon never came up must not refuse the next.
    #[test]
    fn a_leftover_old_tree_does_not_refuse_the_next_swap() {
        let file = binary_name("mixengined");
        let (_root, staged, installed) = tree_layout(&file);
        let old = with_old_suffix(&installed.join(&file));
        std::fs::create_dir_all(&old).expect("a leftover");
        std::fs::write(old.join("stale"), b"older").expect("something in it");

        swap(&staged, &provides_at("mixengined", &file), &installed).expect("a swap");

        assert!(
            !old.join("stale").exists(),
            "the tree replaced by this swap, not the one left by the last"
        );
        assert!(
            old.join("Contents/MacOS/window").is_file(),
            "and what is there is the bundle this swap moved aside"
        );
    }

    #[test]
    fn the_old_files_of_a_finished_update_are_discarded() {
        let (_root, staged, installed) = layout(&["mix"], &["mix"]);
        let swapped = swap(&staged, &provides(&["mix"]), &installed).expect("a swap");

        assert_eq!(discard_old(&installed, &swapped.replaced), 1);
        assert!(!with_old_suffix(&installed.join(binary_name("mix"))).exists());
        assert_eq!(
            discard_old(&installed, &swapped.replaced),
            0,
            "a second start has nothing left to discard, and says so rather than failing"
        );
    }
}
