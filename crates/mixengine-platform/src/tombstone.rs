//! A directory is renamed before it is deleted — roadmap task **T182**, D6.
//!
//! **Deleting a directory tree is not one operation**: a `remove_dir_all` that meets a held file half
//! way leaves half a database behind. A rename is one operation, and Windows refuses it while
//! anything inside the directory is held open or is a process's working directory — so every
//! directory is renamed to a *tombstone* first, all of them or none, and only then deleted.
//!
//! A tombstone is ours by its name, `<name>.removing-<pid>`, so one that could not be deleted is
//! found and removed by the next uninstall. **Not the restart queue**: `MoveFileEx` with
//! `MOVEFILE_DELAY_UNTIL_REBOOT` writes under `HKLM` and needs an administrator the daemon is not.
//!
//! **A refused rename is tried again for a while** (T182b): what refuses one is usually gone a
//! moment later — the daemon's last database connection closing, a scanner reading a file just
//! written — and only what is still there after [`PATIENCE`] puts everything back.
//!
//! `std::fs`, plus the Restart Manager on Windows to name who holds a file, and so not behind a
//! feature: `mix` reads [`tombstones_beside`] too, to tell a finished uninstall from one that left
//! something behind.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What sits between a directory's own name and the pid in its tombstone's.
pub const MARK: &str = ".removing-";

/// A tombstone that is still there after the delete.
#[derive(Debug)]
pub struct Leftover {
    /// The tombstone.
    pub path: PathBuf,

    /// Why it stayed.
    pub error: std::io::Error,
}

/// The directory whose rename was refused. Every directory is back where it was.
#[derive(Debug)]
pub struct Refused {
    /// The directory that could not be renamed.
    pub path: PathBuf,

    /// What the system said.
    pub error: std::io::Error,

    /// What inside it another program holds open, when something could be found — T182b. The
    /// system names only the directory, and the file is what tells a person which program to close.
    pub held: Option<Held>,

    /// How long the rename was tried before this gave up.
    pub tried_for: Duration,
}

/// Something inside a refused directory that another program holds open.
#[derive(Debug)]
pub struct Held {
    /// The file or directory.
    pub path: PathBuf,

    /// The programs holding it, as `name (pid)`, when the system could say. Windows' Restart
    /// Manager answers for files only, so a held directory — a window or a shell standing in it —
    /// comes back with none.
    pub by: Vec<String>,
}

/// How long a refused rename is tried again before everything is put back.
///
/// **A refusal is usually over in a moment**: the last database connection closing on its worker
/// thread, an antivirus scanner reading a file that was just written, a service's process still
/// being torn down. Every one of those used to cost the whole uninstall (T182b, measured on the
/// first real Windows uninstalls); a window or a terminal standing in the home does not go away on
/// its own, and is what is left to report once this has passed.
pub const PATIENCE: Duration = Duration::from_secs(10);

/// How long the first try waits, before anything is looked for — the T182e design, D5. Long enough
/// for the daemon's last database connection to close; a holder that is still there after it is
/// looked for, and what can be moved is moved.
pub const QUICK: Duration = Duration::from_secs(2);

/// How long between two tries of a refused rename.
const RETRY_EVERY: Duration = Duration::from_millis(100);

/// Rename every directory in `paths` to its tombstone, then delete the tombstones.
///
/// A path that is not there is skipped, and one that already is a tombstone is deleted as it is. A
/// rename the system refuses is tried again for up to `patience` ([`QUICK`] or [`PATIENCE`]) before
/// anything is put back.
///
/// **`lifted` are files or directories inside one of `paths` that go out on their own first**,
/// deepest first, each to a tombstone beside the directory holding it — T182b and T182e, D5. A
/// directory another program *watches*, or a file it holds sharing delete, can be renamed, but the
/// directory above it cannot: Windows refuses the parent's rename for as long as it is held. A
/// home's `bin/` is on `PATH`, and editors watch every directory on `PATH` to offer its commands —
/// measured with VS Code, whose watch kept every uninstall of a home until it was closed. Lifted
/// out, the holder goes with it and the parent is free to move. A lifted item is put back with
/// everything else when a later rename is refused.
///
/// # Errors
///
/// [`Refused`] when a rename is still refused after that. Every directory renamed before it is
/// renamed back first, so nothing has been deleted.
pub fn remove_all_or_nothing(
    paths: &[PathBuf],
    lifted: &[PathBuf],
    pid: u32,
    patience: Duration,
) -> Result<Vec<Leftover>, Refused> {
    // Deepest first, so a held file is out of a held directory before that directory moves (T182e,
    // D5). Each goes beside the armed directory holding it; one inside nothing armed stays where it
    // is and goes with whatever removes it.
    let mut ordered: Vec<&PathBuf> = lifted.iter().collect();
    ordered.sort_by_key(|inner| std::cmp::Reverse(inner.components().count()));
    ordered.dedup();

    let lifts = ordered
        .into_iter()
        .enumerate()
        .filter_map(|(order, inner)| {
            let outer = paths
                .iter()
                .find(|outer| inner.starts_with(outer) && inner != *outer)?;
            Some((
                inner.clone(),
                lifted_tombstone_for(inner, outer, pid, order),
            ))
        });
    let moves = lifts.chain(
        paths
            .iter()
            .map(|path| (path.clone(), tombstone_for(path, pid))),
    );

    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();

    for (path, tombstone) in moves {
        let path = &path;

        if std::fs::symlink_metadata(path).is_err() {
            continue;
        }

        if is_tombstone(path) {
            moved.push((path.clone(), path.clone()));
            continue;
        }

        if let Err((error, tried_for)) = rename_patiently(path, &tombstone, patience) {
            // **Looked for before anything is put back**, while whatever refused the rename is
            // most likely still holding on: looked for afterwards, a holder that had just let go
            // was reported as nobody at all.
            let held = first_held(path);

            for (original, renamed) in moved.iter().rev() {
                if original != renamed {
                    let _ = std::fs::rename(renamed, original);
                }
            }

            return Err(Refused {
                held,
                path: path.clone(),
                error,
                tried_for,
            });
        }

        moved.push((path.clone(), tombstone));
    }

    let mut left = Vec::new();

    for (_, renamed) in &moved {
        // A lifted item may be a file (T182e), which `remove_dir_all` refuses.
        let removed = match std::fs::symlink_metadata(renamed) {
            Ok(metadata) if !metadata.is_dir() => std::fs::remove_file(renamed),
            _ => std::fs::remove_dir_all(renamed),
        };
        match removed {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => left.push(Leftover {
                path: renamed.clone(),
                error,
            }),
        }
    }

    Ok(left)
}

/// Every tombstone an earlier run left beside `path`, in name order.
#[must_use]
pub fn tombstones_beside(path: &Path) -> Vec<PathBuf> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Vec::new();
    };

    let prefix = format!("{}{MARK}", name.to_string_lossy());

    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };

    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .map(|entry| entry.path())
        .collect();

    found.sort();
    found
}

/// Where the daemon `pid` leaves the reason its removal did not finish, for `mix` to read once that
/// process is gone — roadmap task **T182b**.
///
/// **Outside the home**, because the home is what could not be removed, and **not the daemon's
/// standard error**, which a daemon started in the background writes to nowhere. Found by the pid,
/// which `mix` already holds to wait for the process.
#[must_use]
pub fn note_for(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("mixengined-{pid}.uninstall"))
}

/// Rename `from` to `to`, trying again for up to `patience` while the system refuses it — T182b.
///
/// Only a refusal is tried again: access denied, which is what a held file or a process standing
/// in the directory makes a rename say, and a sharing violation. Anything else — a tombstone's name
/// already taken, a path that went away — will say the same thing in ten seconds.
fn rename_patiently(
    from: &Path,
    to: &Path,
    patience: Duration,
) -> Result<(), (std::io::Error, Duration)> {
    let began = Instant::now();

    loop {
        let Err(error) = std::fs::rename(from, to) else {
            return Ok(());
        };

        let refused = error.kind() == std::io::ErrorKind::PermissionDenied
            || error.raw_os_error() == Some(SHARING_VIOLATION);

        if !refused || began.elapsed() + RETRY_EVERY > patience {
            return Err((error, began.elapsed()));
        }

        std::thread::sleep(RETRY_EVERY);
    }
}

/// `ERROR_SHARING_VIOLATION`: somebody else has it open.
const SHARING_VIOLATION: i32 = 32;

/// What under `directory` another program holds open, looked for after a refusal.
///
/// **Windows only**, because it is the one system whose rename a held file refuses. Opening a file
/// or a directory with no sharing at all fails exactly when somebody else has it open. **A file is
/// named in preference to a directory**: an editor watching a folder for changes holds it without
/// stopping the rename, where a held file always stops it — so a directory is named only when no
/// file was found, and `directory` itself is one of the candidates, for a shell standing in it.
///
/// The walk is bounded, since it runs once, on a failure, over a home that can hold a great many
/// files.
#[cfg(windows)]
fn first_held(directory: &Path) -> Option<Held> {
    const LOOKED_AT_MOST: usize = 50_000;

    let mut waiting = vec![directory.to_path_buf()];
    let mut held_directory = held_alone(directory).then(|| directory.to_path_buf());
    let mut looked = 0;

    while let Some(current) = waiting.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };

        for entry in entries.filter_map(Result::ok) {
            looked += 1;
            if looked > LOOKED_AT_MOST {
                break;
            }

            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };

            if kind.is_dir() {
                if held_directory.is_none() && held_alone(&path) {
                    held_directory = Some(path.clone());
                }
                waiting.push(path);
                continue;
            }

            if held_alone(&path) {
                let by = holders(&path);
                return Some(Held { path, by });
            }
        }
    }

    held_directory.map(|path| Held {
        path,
        by: Vec::new(),
    })
}

/// Does opening `path` with no sharing fail because somebody else has it open?
///
/// `FILE_FLAG_BACKUP_SEMANTICS` is what lets a directory be opened at all, and is harmless on a
/// file; read access is what a watcher's or a working directory's handle conflicts with.
#[cfg(windows)]
fn held_alone(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt as _;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .is_err_and(|error| error.raw_os_error() == Some(SHARING_VIOLATION))
}

/// The programs holding `file` open, as `name (pid)` — Windows' Restart Manager, which is what the
/// system's own "file in use" dialog asks. Empty when it cannot say.
#[cfg(windows)]
fn holders(file: &Path) -> Vec<String> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::RestartManager::{
        CCH_RM_SESSION_KEY, RM_PROCESS_INFO, RmEndSession, RmGetList, RmRegisterResources,
        RmStartSession,
    };

    let wide: Vec<u16> = file.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut session = 0u32;
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];

    #[expect(
        unsafe_code,
        reason = "both out-parameters are locals, and the key buffer is the length the API documents"
    )]
    let started = unsafe { RmStartSession(&raw mut session, 0, key.as_mut_ptr()) };
    if started != ERROR_SUCCESS {
        return Vec::new();
    }

    let names = (|| {
        let files = [wide.as_ptr()];

        #[expect(
            unsafe_code,
            reason = "`files` holds one pointer to `wide`, a live NUL-terminated local, and the \
                      counts passed are the lengths of the arrays given"
        )]
        let registered = unsafe {
            RmRegisterResources(
                session,
                1,
                files.as_ptr(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
            )
        };
        if registered != ERROR_SUCCESS {
            return Vec::new();
        }

        // Asked twice at most: once for how many, once with room for them. A program that opens
        // the file in between makes the second answer "more" again, and this then says nothing
        // rather than loop.
        let mut found: Vec<RM_PROCESS_INFO> = Vec::new();
        for _ in 0..2 {
            let mut needed = 0u32;
            let mut room = u32::try_from(found.len()).unwrap_or(u32::MAX);
            let mut reasons = 0u32;

            #[expect(
                unsafe_code,
                reason = "`found` has exactly `room` elements, and every other argument is a local"
            )]
            let listed = unsafe {
                RmGetList(
                    session,
                    &raw mut needed,
                    &raw mut room,
                    found.as_mut_ptr(),
                    &raw mut reasons,
                )
            };

            if listed == ERROR_SUCCESS {
                found.truncate(room as usize);
                return found
                    .iter()
                    .map(|process| {
                        let name = &process.strAppName;
                        let end = name
                            .iter()
                            .position(|&unit| unit == 0)
                            .unwrap_or(name.len());
                        format!(
                            "{} ({})",
                            String::from_utf16_lossy(&name[..end]),
                            process.Process.dwProcessId
                        )
                    })
                    .collect();
            }

            if listed != ERROR_MORE_DATA {
                return Vec::new();
            }

            found = vec![RM_PROCESS_INFO::default(); needed as usize];
        }

        Vec::new()
    })();

    #[expect(
        unsafe_code,
        reason = "the session was started above and is not used again"
    )]
    unsafe {
        RmEndSession(session);
    }

    names
}

/// Nothing to look for: an open file does not refuse a rename here.
#[cfg(not(windows))]
fn first_held(_directory: &Path) -> Option<Held> {
    None
}

/// Where `path` is set aside while it is being deleted.
fn tombstone_for(path: &Path, pid: u32) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    path.with_file_name(format!("{name}{MARK}{pid}"))
}

/// Where `inner`, inside `outer`, is set aside on its own: beside `outer`, named after it and
/// numbered by `order`, so two lifted `logs` cannot collide and [`tombstones_beside`] finds every one
/// with `outer`'s own — T182e, D5.
fn lifted_tombstone_for(inner: &Path, outer: &Path, pid: u32, order: usize) -> PathBuf {
    let inner_name = inner
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut tombstone = tombstone_for(outer, pid).into_os_string();
    tombstone.push(format!("-{order}-{inner_name}"));
    PathBuf::from(tombstone)
}

/// Is `path` already somebody's tombstone?
fn is_tombstone(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().contains(MARK))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory_with_a_file(parent: &Path, name: &str) -> PathBuf {
        let directory = parent.join(name);
        std::fs::create_dir_all(&directory).expect("create");
        std::fs::write(directory.join("file"), b"x").expect("write");
        directory
    }

    #[test]
    fn every_directory_goes_when_nothing_is_in_the_way() {
        let root = tempfile::tempdir().expect("tempdir");
        let a = directory_with_a_file(root.path(), "a");
        let b = directory_with_a_file(root.path(), "b");

        let left = remove_all_or_nothing(&[a.clone(), b.clone()], &[], 7, PATIENCE)
            .expect("nothing refused");

        assert!(left.is_empty(), "{left:?}");
        assert!(!a.exists() && !b.exists());
        assert!(tombstones_beside(&a).is_empty());
    }

    /// The second directory cannot be renamed — its tombstone's name is taken by a non-empty
    /// directory, which refuses a rename on every system — so the first is put back and nothing is
    /// deleted.
    #[test]
    fn one_refusal_puts_every_directory_back() {
        let root = tempfile::tempdir().expect("tempdir");
        let a = directory_with_a_file(root.path(), "a");
        let b = directory_with_a_file(root.path(), "b");
        directory_with_a_file(root.path(), "b.removing-7");

        let refused = remove_all_or_nothing(&[a.clone(), b.clone()], &[], 7, Duration::ZERO)
            .expect_err("refused");

        assert_eq!(refused.path, b);
        assert!(a.join("file").exists(), "a was not put back");
        assert!(b.join("file").exists(), "b was touched");
        assert!(!root.path().join("a.removing-7").exists());
    }

    #[test]
    fn a_missing_directory_is_not_a_refusal() {
        let root = tempfile::tempdir().expect("tempdir");

        let left =
            remove_all_or_nothing(&[root.path().join("gone")], &[], 7, PATIENCE).expect("fine");

        assert!(left.is_empty());
    }

    /// A tombstone from an earlier run is deleted as it is, not renamed again.
    #[test]
    fn an_old_tombstone_is_found_and_removed() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("MixEngine");
        let old = directory_with_a_file(root.path(), "MixEngine.removing-3");

        assert_eq!(tombstones_beside(&home), vec![old.clone()]);

        let left =
            remove_all_or_nothing(std::slice::from_ref(&old), &[], 7, PATIENCE).expect("fine");

        assert!(left.is_empty());
        assert!(!old.exists());
    }

    /// Windows: a file held open without share-delete stops the rename, which is the whole point.
    #[cfg(windows)]
    #[test]
    fn a_held_file_puts_every_directory_back() {
        use std::os::windows::fs::OpenOptionsExt as _;

        let root = tempfile::tempdir().expect("tempdir");
        let a = directory_with_a_file(root.path(), "a");
        let b = directory_with_a_file(root.path(), "b");

        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(b.join("file"))
            .expect("hold");

        let refused = remove_all_or_nothing(&[a.clone(), b.clone()], &[], 7, Duration::ZERO)
            .expect_err("refused");
        drop(held);

        assert_eq!(refused.path, b);
        assert!(a.join("file").exists() && b.join("file").exists());
    }

    /// Windows: the same, for a file held the way `std` opens one — with share-delete — which is how
    /// most programs on this system hold a log open.
    #[cfg(windows)]
    #[test]
    fn a_file_held_with_share_delete_still_stops_the_rename() {
        let root = tempfile::tempdir().expect("tempdir");
        let a = directory_with_a_file(root.path(), "a");

        let held = std::fs::File::open(a.join("file")).expect("hold");
        let outcome = remove_all_or_nothing(std::slice::from_ref(&a), &[], 7, Duration::ZERO);
        drop(held);

        assert!(outcome.is_err(), "{outcome:?}");
        assert!(a.join("file").exists());
    }

    /// T182b. The file another program holds is found and named, not only the directory above it.
    #[cfg(windows)]
    #[test]
    fn the_held_file_is_found_under_the_directory() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let inner = root.path().join("logs").join("services");
        std::fs::create_dir_all(&inner).expect("a nested directory");
        std::fs::write(root.path().join("free.txt"), b"nobody has this").expect("a free file");
        let held = inner.join("current.log");
        std::fs::write(&held, b"somebody has this").expect("a held file");

        let _open = std::fs::File::open(&held).expect("held open");

        let found = first_held(root.path()).expect("the held file is found");
        assert_eq!(found.path, held);
    }

    /// T182b. A holder that lets go while the rename is being retried costs nothing: the directory
    /// goes. This is the last database connection closing a moment after the daemon asked it to,
    /// which refused every real Windows uninstall of a home with a front end.
    #[cfg(windows)]
    #[test]
    fn a_holder_that_lets_go_in_time_does_not_stop_the_removal() {
        let root = tempfile::tempdir().expect("tempdir");
        let a = directory_with_a_file(root.path(), "a");

        let held = std::fs::File::open(a.join("file")).expect("hold");
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });

        let left = remove_all_or_nothing(std::slice::from_ref(&a), &[], 7, Duration::from_secs(5))
            .expect("the rename went through once the file was let go");
        releaser.join().expect("the releasing thread");

        assert!(left.is_empty(), "{left:?}");
        assert!(!a.exists());
    }

    /// T182b. A directory another program has open inside a home stops the home's rename, and the
    /// same directory lifted out first does not: the handle goes with it, and the home moves. Put
    /// back afterwards, when a later rename is refused, like every other directory.
    #[cfg(windows)]
    #[test]
    fn a_held_directory_lifted_out_first_does_not_stop_its_parent() {
        use std::os::windows::fs::OpenOptionsExt as _;

        let root = tempfile::tempdir().expect("tempdir");
        let home = directory_with_a_file(root.path(), "home");
        let bin = directory_with_a_file(&home, "bin");

        // How an editor holds a directory it watches: open for listing, sharing everything.
        let watching = std::fs::OpenOptions::new()
            .access_mode(0x0010_0081)
            .share_mode(0x7)
            .custom_flags(0x0200_0000)
            .open(&bin)
            .expect("the directory held open");

        let refused = remove_all_or_nothing(std::slice::from_ref(&home), &[], 7, Duration::ZERO);
        assert!(
            refused.is_err(),
            "the parent of a held directory was renamed: {refused:?}"
        );
        assert!(bin.join("file").exists(), "a refusal touched the home");

        let left = remove_all_or_nothing(
            std::slice::from_ref(&home),
            std::slice::from_ref(&bin),
            7,
            Duration::ZERO,
        )
        .expect("lifted out first, the held directory does not stop its parent");
        drop(watching);

        assert!(left.is_empty(), "{left:?}");
        assert!(!home.exists());
        assert!(
            tombstones_beside(&home).is_empty(),
            "{:?}",
            tombstones_beside(&home)
        );
    }

    /// T182b. A lifted directory is put back into its parent when a later rename is refused, so the
    /// refusal leaves the home exactly as it was.
    #[test]
    fn a_lifted_directory_is_put_back_on_a_refusal() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = directory_with_a_file(root.path(), "home");
        let bin = directory_with_a_file(&home, "bin");
        let other = directory_with_a_file(root.path(), "other");
        // The second directory's tombstone name is taken, which refuses its rename on every system.
        directory_with_a_file(root.path(), "other.removing-7");

        let refused = remove_all_or_nothing(
            &[home.clone(), other.clone()],
            std::slice::from_ref(&bin),
            7,
            Duration::ZERO,
        )
        .expect_err("refused");

        assert_eq!(refused.path, other);
        assert!(bin.join("file").exists(), "bin was not put back");
        assert!(home.join("file").exists(), "home was not put back");
        assert!(
            tombstones_beside(&home).is_empty(),
            "{:?}",
            tombstones_beside(&home)
        );
    }

    /// T182b. A directory held open — a shell standing in it, a window showing it — is named when
    /// no file inside is, rather than nothing being named at all.
    #[cfg(windows)]
    #[test]
    fn a_held_directory_is_named_when_no_file_is() {
        use std::os::windows::fs::OpenOptionsExt as _;

        let root = tempfile::tempdir().expect("a temporary directory");
        let inner = root.path().join("etc").join("caddy");
        std::fs::create_dir_all(&inner).expect("a nested directory");
        std::fs::write(root.path().join("free.txt"), b"nobody has this").expect("a free file");

        // What a working directory's handle is: the directory itself, open for traversal.
        let _standing = std::fs::OpenOptions::new()
            .access_mode(0x0010_0020)
            .share_mode(0x7)
            .custom_flags(0x0200_0000)
            .open(&inner)
            .expect("the directory held open");

        let found = first_held(root.path()).expect("the held directory is found");
        assert_eq!(found.path, inner);
        assert!(found.by.is_empty(), "{:?}", found.by);
    }

    /// T182b. The program holding a file is named, which is what tells a person what to close.
    #[cfg(windows)]
    #[test]
    fn the_program_holding_a_file_is_named() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let held = root.path().join("current.log");
        std::fs::write(&held, b"this process has this").expect("a held file");

        let _open = std::fs::File::open(&held).expect("held open");

        let found = first_held(root.path()).expect("the held file is found");
        let this = format!("({})", std::process::id());
        assert!(
            found.by.iter().any(|holder| holder.ends_with(&this)),
            "this process holds it, and was not named: {:?}",
            found.by
        );
    }

    #[test]
    fn nothing_is_held_in_a_directory_nobody_has_open() {
        let root = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(root.path().join("free.txt"), b"nobody has this").expect("a free file");

        assert!(first_held(root.path()).is_none());
    }

    /// T182e, D5. A held file inside a held directory leaves first, then the directory, then the
    /// home; on a later refusal all of it comes back.
    #[test]
    fn nested_lifts_leave_deepest_first_and_come_back() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = directory_with_a_file(root.path(), "home");
        let logs = directory_with_a_file(&home, "logs");
        let log = logs.join("file");
        let other = directory_with_a_file(root.path(), "other");
        directory_with_a_file(root.path(), "other.removing-7");

        let refused = remove_all_or_nothing(
            &[home.clone(), other.clone()],
            // Given shallowest first on purpose: the order is the function's to fix.
            &[logs.clone(), log.clone()],
            7,
            Duration::ZERO,
        )
        .expect_err("refused");

        assert_eq!(refused.path, other);
        assert!(log.exists(), "the file was not put back");
        assert!(home.join("file").exists(), "home was not put back");
        assert!(
            tombstones_beside(&home).is_empty(),
            "{:?}",
            tombstones_beside(&home)
        );
    }

    /// The same, with the file held open sharing delete — which on Windows refuses the rename of
    /// the directory holding it, so the order is what lets both go: the file first, then its
    /// directory, then the home.
    #[test]
    fn nested_lifts_all_go_when_nothing_refuses() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = directory_with_a_file(root.path(), "home");
        let logs = directory_with_a_file(&home, "logs");
        let _held = std::fs::File::open(logs.join("file")).expect("held sharing delete");

        let left = remove_all_or_nothing(
            std::slice::from_ref(&home),
            // Shallowest first on purpose.
            &[logs.clone(), logs.join("file")],
            7,
            Duration::ZERO,
        )
        .expect("nothing refused");

        assert!(left.is_empty(), "{left:?}");
        assert!(!home.exists());
        assert!(
            tombstones_beside(&home).is_empty(),
            "{:?}",
            tombstones_beside(&home)
        );
    }

    /// T182e, D5. A lifted file is deleted with the tombstones.
    #[test]
    fn a_lifted_file_is_deleted() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = directory_with_a_file(root.path(), "home");

        let left = remove_all_or_nothing(
            std::slice::from_ref(&home),
            &[home.join("file")],
            7,
            Duration::ZERO,
        )
        .expect("nothing refused");

        assert!(left.is_empty(), "{left:?}");
        assert!(
            tombstones_beside(&home).is_empty(),
            "{:?}",
            tombstones_beside(&home)
        );
    }

    /// Two lifted items with one name do not collide.
    #[test]
    fn two_lifted_items_with_one_name_do_not_collide() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = directory_with_a_file(root.path(), "home");
        let first = directory_with_a_file(&home.join("a"), "logs");
        let second = directory_with_a_file(&home.join("b"), "logs");

        let left = remove_all_or_nothing(
            std::slice::from_ref(&home),
            &[first, second],
            7,
            Duration::ZERO,
        )
        .expect("nothing refused");

        assert!(left.is_empty(), "{left:?}");
        assert!(!home.exists());
    }
}
