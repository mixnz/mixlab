//! Who is running from a directory — roadmap task **T182**, D4.
//!
//! **Before an uninstall removes a directory, the processes whose executable lies inside it are the
//! ones that would stop it half-way**: a `php artisan serve` started through a shim, a database a
//! person started by hand from `runtimes/`. Asked of `sysinfo`, like the metrics sampler, which has
//! already done the per-system work.
//!
//! What a process table cannot show — a working directory, an open file, a watched folder — is
//! asked of the handle table by [`held_under`] (T182e) on Windows, where it refuses a rename, and
//! each thing found is probed for whether it could be moved anyway.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One process in the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occupant {
    /// Its pid, for a person to find it by.
    pub pid: u32,

    /// Its name, as the process table spells it.
    pub name: String,

    /// The executable, which is what put it on this list.
    pub executable: PathBuf,
}

/// Every process whose executable lies inside one of `directories`, except `spare` and anything
/// descended from it, in pid order.
///
/// **`spare` is the daemon asking**: it and everything it started are stopped by its own shutdown,
/// in dependency order, before anything is removed — so they are not in anybody's way.
#[must_use]
pub fn processes_under(directories: &[PathBuf], spare: Option<u32>) -> Vec<Occupant> {
    if directories.is_empty() {
        return Vec::new();
    }

    let roots: Vec<PathBuf> = directories.iter().map(|path| comparable(path)).collect();

    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
    );

    let parents: BTreeMap<u32, u32> = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| Some((pid.as_u32(), process.parent()?.as_u32())))
        .collect();

    let mut found: Vec<Occupant> = system
        .processes()
        .iter()
        .filter(|(_, process)| process.thread_kind().is_none())
        .filter_map(|(pid, process)| {
            let pid = pid.as_u32();
            let executable = process.exe()?;
            let at = comparable(executable);

            if !roots.iter().any(|root| at.starts_with(root)) {
                return None;
            }

            if spare.is_some_and(|spare| descends_from(pid, spare, &parents)) {
                return None;
            }

            Some(Occupant {
                pid,
                name: process.name().to_string_lossy().into_owned(),
                executable: executable.to_path_buf(),
            })
        })
        .collect();

    found.sort_by_key(|occupant| occupant.pid);
    found
}

/// Something another process holds inside a directory an uninstall removes — T182e, D1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldItem {
    /// The file or directory, spelled under the caller's own spelling of the directory it lies in.
    pub path: PathBuf,

    /// Whether it can be renamed and deleted while held: every holder shares delete.
    pub movable: bool,

    /// Who holds it, in pid order.
    pub holders: Vec<Holder>,
}

/// A process holding a [`HeldItem`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// Its pid.
    pub pid: u32,

    /// Its name, as the process table spells it.
    pub name: String,
}

/// How long [`held_under`] may read the handle table for — the T182e design, D2.
pub const HELD_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

/// Every file or directory strictly inside one of `directories` that a process other than `spare`
/// and its descendants holds open, probed for whether it could be moved — T182e, D1 to D3.
///
/// **Windows only finds anything**: it is the one system where something held open refuses a
/// directory's rename. One of `directories` itself is never an item: renaming an open directory is
/// allowed. An item that has gone by the time it is probed is left out.
#[must_use]
pub fn held_under(directories: &[PathBuf], spare: Option<u32>) -> Vec<HeldItem> {
    #[cfg(windows)]
    {
        held_on_windows(directories, spare)
    }

    #[cfg(not(windows))]
    {
        let _ = (directories, spare);
        Vec::new()
    }
}

#[cfg(windows)]
fn held_on_windows(directories: &[PathBuf], spare: Option<u32>) -> Vec<HeldItem> {
    if directories.is_empty() {
        return Vec::new();
    }

    let roots: Vec<PathBuf> = directories.iter().map(|path| comparable(path)).collect();

    // Grouped by path before any process table is read: on a machine nobody holds anything on,
    // the scan stops here. **Each path is rebuilt on the caller's own spelling of the directory it
    // lies in** — the tombstone module matches lifted items to armed directories component by
    // component, and a case the disk spells differently from `config.toml` would otherwise make a
    // movable item look like it belongs to nothing.
    let mut held: BTreeMap<PathBuf, Vec<u32>> = BTreeMap::new();
    for open in crate::sys::handles::open_on_disk(HELD_BUDGET) {
        // Already a final path; lower case is all `comparable` would add, and canonicalising every
        // handle on the machine would cost a system call each.
        let at = PathBuf::from(open.path.to_string_lossy().to_lowercase());
        let Some(index) = roots
            .iter()
            .position(|root| at.starts_with(root) && at != *root)
        else {
            continue;
        };
        let rest: PathBuf = open
            .path
            .components()
            .skip(roots[index].components().count())
            .collect();
        held.entry(directories[index].join(rest))
            .or_default()
            .push(open.pid);
    }
    if held.is_empty() {
        return Vec::new();
    }

    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );
    let parents: BTreeMap<u32, u32> = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| Some((pid.as_u32(), process.parent()?.as_u32())))
        .collect();

    held.into_iter()
        .filter_map(|(path, mut pids)| {
            pids.sort_unstable();
            pids.dedup();
            pids.retain(|pid| !spare.is_some_and(|spare| descends_from(*pid, spare, &parents)));
            if pids.is_empty() {
                return None;
            }

            let movable = movable(&path)?;
            let holders = pids
                .into_iter()
                .map(|pid| Holder {
                    pid,
                    name: system.process(sysinfo::Pid::from_u32(pid)).map_or_else(
                        || format!("pid {pid}"),
                        |process| process.name().to_string_lossy().into_owned(),
                    ),
                })
                .collect();

            Some(HeldItem {
                path,
                movable,
                holders,
            })
        })
        .collect()
}

/// Could `path` be renamed and deleted now? `None` when it is not there any more — T182e, D1.
///
/// The access a rename and a delete need, asked for while sharing everything: refused with a
/// sharing violation exactly when some existing handle does not share delete. Opened and closed;
/// nothing changes.
#[cfg(windows)]
fn movable(path: &Path) -> Option<bool> {
    use std::os::windows::fs::OpenOptionsExt as _;

    const DELETE: u32 = 0x0001_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const SHARING_VIOLATION: i32 = 32;

    match std::fs::OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(0x7)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
    {
        Ok(_) => Some(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) if error.raw_os_error() == Some(SHARING_VIOLATION) => Some(false),
        // Anything else — access denied on something this user may not delete — cannot be moved
        // by this uninstall either.
        Err(_) => Some(false),
    }
}

/// Is `pid` `ancestor`, or started by it at any depth?
///
/// **Bounded**, because a parent table is a snapshot of a moving system, and a pid the system handed
/// round can make it a loop.
fn descends_from(pid: u32, ancestor: u32, parents: &BTreeMap<u32, u32>) -> bool {
    let mut current = pid;

    for _ in 0..=parents.len() {
        if current == ancestor {
            return true;
        }

        match parents.get(&current) {
            Some(&parent) if parent != current => current = parent,
            _ => return false,
        }
    }

    false
}

/// A path two spellings of which compare equal: canonical where it exists, and on Windows — where
/// the file system ignores case — folded to lower case.
fn comparable(path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    if cfg!(windows) {
        PathBuf::from(canonical.to_string_lossy().to_lowercase())
    } else {
        canonical
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long-running program copied into a directory of its own, so the test controls where its
    /// executable lies. `ping` and `sleep` are on every machine this runs on.
    fn start_a_copy(directory: &Path) -> std::process::Child {
        let (source, args): (PathBuf, &[&str]) = if cfg!(windows) {
            (
                PathBuf::from(std::env::var("SystemRoot").expect("SystemRoot"))
                    .join(r"System32\PING.EXE"),
                &["-n", "30", "127.0.0.1"],
            )
        } else {
            (PathBuf::from("/bin/sleep"), &["30"])
        };

        let copy = directory.join(source.file_name().expect("a file name"));
        std::fs::copy(&source, &copy).expect("copy the program");

        std::process::Command::new(&copy)
            .args(args)
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("start the copy")
    }

    #[test]
    fn a_program_running_from_the_directory_is_found() {
        let inside = tempfile::tempdir().expect("tempdir");
        let mut child = start_a_copy(inside.path());

        let found = processes_under(&[inside.path().to_path_buf()], None);

        let _ = child.kill();
        let _ = child.wait();
        assert!(
            found.iter().any(|occupant| occupant.pid == child.id()),
            "{found:?}"
        );
    }

    #[test]
    fn a_program_running_from_elsewhere_is_not() {
        let inside = tempfile::tempdir().expect("tempdir");
        let elsewhere = tempfile::tempdir().expect("tempdir");
        let mut child = start_a_copy(elsewhere.path());

        let found = processes_under(&[inside.path().to_path_buf()], None);

        let _ = child.kill();
        let _ = child.wait();
        assert!(found.is_empty(), "{found:?}");
    }

    /// The daemon spares itself and everything it started: its own shutdown stops those.
    #[test]
    fn a_descendant_of_the_spared_pid_is_not() {
        let inside = tempfile::tempdir().expect("tempdir");
        let mut child = start_a_copy(inside.path());

        let found = processes_under(&[inside.path().to_path_buf()], Some(std::process::id()));

        let _ = child.kill();
        let _ = child.wait();
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn the_ancestry_walk_stops_on_a_loop() {
        let parents = BTreeMap::from([(1, 2), (2, 1)]);

        assert!(!descends_from(1, 99, &parents));
        assert!(descends_from(1, 2, &parents));
    }

    fn a_home() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home");
        for inner in ["data", "bin"] {
            std::fs::create_dir_all(home.join(inner)).expect("a directory");
            std::fs::write(home.join(inner).join("file"), b"x").expect("a file");
        }
        (root, home)
    }

    #[cfg(windows)]
    fn item_held_by<'a>(found: &'a [HeldItem], pid: u32, name: &str) -> Option<&'a HeldItem> {
        found.iter().find(|item| {
            item.holders.iter().any(|holder| holder.pid == pid)
                && item
                    .path
                    .file_name()
                    .is_some_and(|file| file.eq_ignore_ascii_case(name))
        })
    }

    #[cfg(windows)]
    fn open_with(path: &Path, access: u32, share: u32) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .access_mode(access)
            .share_mode(share)
            .custom_flags(0x0200_0000)
            .open(path)
            .expect("held open")
    }

    #[cfg(windows)]
    fn ping_standing_in(directory: &Path) -> std::process::Child {
        let ping = PathBuf::from(std::env::var("SystemRoot").expect("SystemRoot"))
            .join(r"System32\PING.EXE");
        std::process::Command::new(ping)
            .args(["-n", "30", "127.0.0.1"])
            .current_dir(directory)
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("a process standing in the directory")
    }

    /// T182e, D1. A watched directory: held, and movable — its watch shares delete and follows a
    /// rename.
    #[cfg(windows)]
    #[test]
    fn a_watched_directory_is_held_and_movable() {
        let (_root, home) = a_home();
        let _watch = open_with(&home.join("data"), 0x0010_0081, 0x7);

        let found = held_under(std::slice::from_ref(&home), None);

        let item = item_held_by(&found, std::process::id(), "data").expect("found");
        assert!(item.movable, "{found:?}");
    }

    /// A file held sharing delete, as `std` opens one: movable.
    #[cfg(windows)]
    #[test]
    fn a_file_held_sharing_delete_is_movable() {
        let (_root, home) = a_home();
        let _open = std::fs::File::open(home.join("data").join("file")).expect("held");

        let found = held_under(std::slice::from_ref(&home), None);

        let item = item_held_by(&found, std::process::id(), "file").expect("found");
        assert!(item.movable, "{found:?}");
    }

    /// A file held without sharing delete, as SQLite holds one: stuck.
    #[cfg(windows)]
    #[test]
    fn a_file_held_without_sharing_delete_is_stuck() {
        let (_root, home) = a_home();
        let _open = open_with(
            &home.join("data").join("file"),
            0x8000_0000 | 0x4000_0000,
            0x3,
        );

        let found = held_under(std::slice::from_ref(&home), None);

        let item = item_held_by(&found, std::process::id(), "file").expect("found");
        assert!(!item.movable, "{found:?}");
    }

    /// A terminal standing in a folder: its working directory does not share delete, so stuck.
    #[cfg(windows)]
    #[test]
    fn a_directory_a_process_stands_in_is_stuck() {
        let (_root, home) = a_home();
        let mut child = ping_standing_in(&home.join("data"));

        let found = held_under(std::slice::from_ref(&home), None);

        let _ = child.kill();
        let _ = child.wait();
        let item = item_held_by(&found, child.id(), "data").expect("found");
        assert!(!item.movable, "{found:?}");
    }

    /// Renaming a directory that is itself open is allowed, so it is not a held item.
    #[cfg(windows)]
    #[test]
    fn the_armed_directory_itself_is_not_a_held_item() {
        let (_root, home) = a_home();
        let _watch = open_with(&home, 0x0010_0081, 0x7);

        let found = held_under(std::slice::from_ref(&home), None);

        assert!(
            item_held_by(&found, std::process::id(), "home").is_none(),
            "{found:?}"
        );
    }

    /// The daemon spares itself and everything it started.
    #[cfg(windows)]
    #[test]
    fn the_spared_process_and_its_children_hold_nothing() {
        let (_root, home) = a_home();
        let _open = std::fs::File::open(home.join("data").join("file")).expect("held");
        let mut child = ping_standing_in(&home.join("data"));

        let found = held_under(std::slice::from_ref(&home), Some(std::process::id()));

        let _ = child.kill();
        let _ = child.wait();
        assert!(found.is_empty(), "{found:?}");
    }

    /// The home as a person might type it still matches.
    #[cfg(windows)]
    #[test]
    fn a_root_spelled_in_upper_case_still_matches() {
        let (_root, home) = a_home();
        let _open = std::fs::File::open(home.join("data").join("file")).expect("held");
        let shouting = PathBuf::from(home.to_string_lossy().to_uppercase());

        let found = held_under(std::slice::from_ref(&shouting), None);

        assert!(
            item_held_by(&found, std::process::id(), "file").is_some(),
            "{found:?}"
        );
    }

    /// A held item comes back on the caller's spelling of the directory, so `tombstone` can match
    /// it to that directory component by component (and a person reads `C:\…`, not `\\?\C:\…`).
    #[cfg(windows)]
    #[test]
    fn a_held_item_is_spelled_under_the_callers_directory() {
        let (_root, home) = a_home();
        let _open = std::fs::File::open(home.join("data").join("file")).expect("held");
        let shouting = PathBuf::from(home.to_string_lossy().to_uppercase());

        let found = held_under(std::slice::from_ref(&shouting), None);

        let item = item_held_by(&found, std::process::id(), "file").expect("found");
        assert!(
            item.path.starts_with(&shouting),
            "{:?} is not under {shouting:?}",
            item.path
        );
    }

    /// `home-dev` beside `home` is not inside it.
    #[cfg(windows)]
    #[test]
    fn a_sibling_with_the_same_prefix_is_not_inside() {
        let (root, home) = a_home();
        let sibling = root.path().join("home-dev");
        std::fs::create_dir_all(&sibling).expect("a sibling");
        std::fs::write(sibling.join("elsewhere"), b"x").expect("a file");
        let _open = std::fs::File::open(sibling.join("elsewhere")).expect("held");

        let found = held_under(std::slice::from_ref(&home), None);

        assert!(
            item_held_by(&found, std::process::id(), "elsewhere").is_none(),
            "{found:?}"
        );
    }

    #[test]
    fn nothing_is_held_in_a_directory_nobody_opened() {
        let (_root, home) = a_home();

        let found = held_under(std::slice::from_ref(&home), None);

        assert!(
            found.iter().all(|item| item
                .holders
                .iter()
                .all(|holder| holder.pid != std::process::id())),
            "{found:?}"
        );
    }
}
