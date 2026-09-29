//! Rename what is installed out of the way, copy the new one in, and put everything back if any
//! step fails — spec D4 step 6.
//!
//! The same rename-then-place `mix self-update` does, written here again (ADR 0056 rule 3). It
//! works on Windows while the files are running: a running executable can be renamed, not
//! overwritten. An update adds nothing: a name the install does not have is kept, not created.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What a replaced file is renamed to, until the next start removes it.
pub const OLD_SUFFIX: &str = ".old";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swapped {
    pub replaced: Vec<String>,
    pub kept: Vec<String>,
}

/// What one payload name is called on disk here. The window is `mixlab.exe` like the rest: the
/// only machine that swaps in place is Windows.
pub fn installed_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

pub fn swap(
    staged: &Path,
    provides: &BTreeMap<String, String>,
    directory: &Path,
) -> Result<Swapped, String> {
    swap_named(staged, provides, directory, installed_name)
}

/// [`swap`] with the on-disk name of each entry given, so a tree can be tested on any system.
pub fn swap_named(
    staged: &Path,
    provides: &BTreeMap<String, String>,
    directory: &Path,
    name_on_disk: impl Fn(&str) -> String,
) -> Result<Swapped, String> {
    let mut swapped = Swapped::default();
    // What has been renamed, in order, so a failure part way can put it back in reverse.
    let mut renamed: Vec<(PathBuf, PathBuf)> = Vec::new();

    for (name, relative) in provides {
        let target = directory.join(name_on_disk(name));
        if !target.exists() {
            swapped.kept.push(name.clone());
            continue;
        }
        let old = with_old_suffix(&target);
        // A `.old` from an earlier update nobody cleaned: it is the one thing in the way.
        remove_any(&old);

        if let Err(error) = replace(&staged.join(relative), &target, &old) {
            unwind(&renamed);
            return Err(error);
        }
        renamed.push((target, old));
        swapped.replaced.push(name.clone());
    }
    Ok(swapped)
}

/// Remove the `.old` copy of every name an update is about to swap, before anything is stopped.
/// Answers the first one that cannot be removed.
///
/// **One that stays is a program still running from it**, and on Windows the likeliest is this
/// window ([`window_old`]): an update swapped the files underneath it and it never started again.
/// Swapping now would fail on that file half way, after the daemon had been stopped — so the caller
/// decides before anything is touched.
pub fn clear_old(provides: &BTreeMap<String, String>, directory: &Path) -> Result<(), PathBuf> {
    for name in provides.keys() {
        let old = with_old_suffix(&directory.join(installed_name(name)));
        remove_any(&old);
        if old.exists() {
            return Err(old);
        }
    }
    Ok(())
}

/// What the window's own file is renamed to by a swap: the file a window still running after one
/// runs from.
pub fn window_old(directory: &Path) -> PathBuf {
    with_old_suffix(&directory.join(installed_name("mixlab")))
}

/// Every name an update of this product swaps: the window and the MixEngine binaries beside it.
///
/// What [`discard_old`] may remove, and nothing else: the directory holding the window can hold
/// other people's files, and a `.old` there is not ours to delete unless we made it.
pub const PRODUCT_NAMES: &[&str] = &[
    "mixlab",
    "mix",
    "mixengined",
    "mixengine-shim",
    "mixengine-trampoline",
    "mixengine-elevate",
];

/// Remove the `.old` copies an update of this product left beside the binaries. Answers how many
/// went; one that cannot be removed yet (a running window's, on Windows) is left for the next start.
pub fn discard_old(directory: &Path) -> usize {
    PRODUCT_NAMES
        .iter()
        .map(|name| with_old_suffix(&directory.join(installed_name(name))))
        .filter(|old| old.exists())
        .filter(|old| {
            if old.is_dir() {
                std::fs::remove_dir_all(old).is_ok()
            } else {
                std::fs::remove_file(old).is_ok()
            }
        })
        .count()
}

/// One entry: rename the installed one out of the way, then copy the new one in. A copy that
/// fails removes what it wrote and renames the old one back before answering.
fn replace(source: &Path, target: &Path, old: &Path) -> Result<(), String> {
    std::fs::rename(target, old)
        .map_err(|e| format!("could not move {} out of the way: {e}", target.display()))?;
    let copied = if source.is_dir() {
        copy_tree(source, target)
    } else {
        std::fs::copy(source, target)
            .map(|_| ())
            .map_err(|e| format!("could not copy {} into place: {e}", target.display()))
    };
    if let Err(error) = copied {
        remove_any(target);
        let _ = std::fs::rename(old, target);
        return Err(error);
    }
    Ok(())
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|e| format!("could not create {}: {e}", target.display()))?;
    for entry in std::fs::read_dir(source)
        .map_err(|e| format!("could not read {}: {e}", source.display()))?
    {
        let entry = entry.map_err(|e| format!("could not read {}: {e}", source.display()))?;
        let (from, to) = (entry.path(), target.join(entry.file_name()));
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("could not copy {}: {e}", to.display()))?;
        }
    }
    Ok(())
}

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
        remove_any(target);
        if let Err(error) = std::fs::rename(old, target) {
            log::warn!(
                "could not put {} back after a failed update: {error}",
                target.display()
            );
        }
    }
}

/// `mix.exe` → `mix.exe.old`: appended, so Windows never starts it by accident.
fn with_old_suffix(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(OLD_SUFFIX);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn provides(names: &[&str]) -> BTreeMap<String, String> {
        names
            .iter()
            .map(|n| ((*n).to_owned(), format!("mixengine/{}", installed_name(n))))
            .collect()
    }

    fn write(path: &std::path::Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(path: &std::path::Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn every_present_name_is_replaced_and_an_absent_one_is_kept() {
        let (staged, installed) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        for name in ["mix", "mixengined", "mixlab"] {
            write(
                &staged.path().join("mixengine").join(installed_name(name)),
                "new",
            );
        }
        write(&installed.path().join(installed_name("mix")), "old");
        write(&installed.path().join(installed_name("mixlab")), "old");

        let swapped = swap(
            staged.path(),
            &provides(&["mix", "mixengined", "mixlab"]),
            installed.path(),
        )
        .unwrap();

        assert_eq!(
            swapped.replaced,
            vec!["mix".to_owned(), "mixlab".to_owned()]
        );
        assert_eq!(swapped.kept, vec!["mixengined".to_owned()]);
        assert!(
            !installed.path().join(installed_name("mixengined")).exists(),
            "an update adds nothing"
        );
        assert_eq!(read(&installed.path().join(installed_name("mix"))), "new");
    }

    #[test]
    fn a_failure_part_way_puts_back_everything_already_moved() {
        let (staged, installed) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(
            &staged.path().join("mixengine").join(installed_name("mix")),
            "new",
        );
        // `mixengined` is missing from the staging directory, so its copy fails.
        write(&installed.path().join(installed_name("mix")), "old");
        write(&installed.path().join(installed_name("mixengined")), "old");

        assert!(swap(
            staged.path(),
            &provides(&["mix", "mixengined"]),
            installed.path()
        )
        .is_err());
        for name in ["mix", "mixengined"] {
            let path = installed.path().join(installed_name(name));
            assert_eq!(read(&path), "old", "{name} is back");
        }
    }

    #[test]
    fn a_directory_entry_is_swapped_as_a_tree() {
        let (staged, installed) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(&staged.path().join("mixengine/tree/inner/file"), "new");
        write(&installed.path().join("tree/inner/file"), "old");
        let map = BTreeMap::from([("tree".to_owned(), "mixengine/tree".to_owned())]);
        swap_named(staged.path(), &map, installed.path(), |n| n.to_owned()).unwrap();
        assert_eq!(read(&installed.path().join("tree/inner/file")), "new");
    }

    #[test]
    fn old_copies_are_discarded() {
        let installed = tempfile::tempdir().unwrap();
        write(
            &installed
                .path()
                .join(format!("{}{OLD_SUFFIX}", installed_name("mix"))),
            "old",
        );
        assert_eq!(discard_old(installed.path()), 1);
        assert_eq!(discard_old(installed.path()), 0);
    }

    #[test]
    fn a_leftover_old_copy_is_cleared_before_a_swap() {
        let installed = tempfile::tempdir().unwrap();
        let old = installed
            .path()
            .join(format!("{}{OLD_SUFFIX}", installed_name("mix")));
        write(&old, "old");

        assert_eq!(
            clear_old(&provides(&["mix", "mixlab"]), installed.path()),
            Ok(())
        );
        assert!(!old.exists());
    }

    /// The window an update swapped underneath and never restarted: its `.old` is in use, and the
    /// answer names it, as the window's, rather than stopping MixEngine to fail on it.
    #[cfg(windows)]
    #[test]
    fn an_old_copy_in_use_is_named() {
        use std::os::windows::fs::OpenOptionsExt as _;

        let installed = tempfile::tempdir().unwrap();
        let old = installed
            .path()
            .join(format!("{}{OLD_SUFFIX}", installed_name("mixlab")));
        write(&old, "running");
        // No sharing at all: what a running image's file refuses a delete with.
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&old)
            .unwrap();

        assert_eq!(
            clear_old(&provides(&["mix", "mixlab"]), installed.path()),
            Err(window_old(installed.path()))
        );
        assert_eq!(old, window_old(installed.path()));
    }

    /// The directory holding the window can hold other people's files — `/Applications`, a shared
    /// `bin`. Only what an update of this product renamed is ever removed.
    #[test]
    fn an_old_copy_of_something_else_is_left_alone() {
        let installed = tempfile::tempdir().unwrap();
        let theirs = installed.path().join(format!("Other.app{OLD_SUFFIX}"));
        write(&theirs.join("Contents/Info.plist"), "theirs");
        write(
            &installed.path().join(format!("notes.txt{OLD_SUFFIX}")),
            "theirs",
        );

        assert_eq!(discard_old(installed.path()), 0);
        assert!(theirs.exists());
    }
}
