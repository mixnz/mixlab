//! Where an installed MixEngine keeps the one file it runs as root.
//!
//! **One answer per operating system, read by both sides of the privilege boundary.** The daemon
//! reads it to decide which file to hand the elevation prompt; `mixengine-elevate` reads the same
//! function to decide where to put itself. Two answers to that question would be a helper installed
//! somewhere nothing ever looks — so there is one, and it is here.
//!
//! Nothing in this module creates anything or checks anything: it says *where*, and
//! `mixengine-elevate` is the only thing that ever writes there. Who owns what is already
//! [`crate::elevated::owner_of`]'s question, and the answer to it is what
//! `mixengine_core::elevation::helper` refuses on. See the T85 design, D1 and D3.

use std::path::PathBuf;

use crate::Result;

/// The privileged helper's installed path on this machine, whether or not it is there yet.
///
/// `%ProgramFiles%\MixEngine\mixengine-elevate.exe` on Windows,
/// `/Library/PrivilegedHelperTools/dev.mixengine.elevate` on macOS,
/// `/usr/local/libexec/mixengine/mixengine-elevate` on Linux — each argued in its own module.
///
/// **Not the directory beside the program**, which is what `mixengine_core::elevation::helper`
/// falls back to and what every build out of `cargo` uses. This is the copy an installed MixEngine
/// runs, and the whole reason it exists is that nothing running as the user may rewrite it.
///
/// # Errors
///
/// [`Error::Os`](crate::Error::Os) on Windows when the shell will not name Program Files. The two
/// Unixes cannot fail: their answers are compiled-in constants, and the [`Result`] is there so all
/// three have one signature.
pub fn helper_path() -> Result<PathBuf> {
    crate::sys::install::helper_path()
}

/// Every copy of `mixengine-elevate` an install format on this system leaves behind that MixEngine
/// may install *from*, most trustworthy first — roadmap task **T88d**.
///
/// **A source is not [`helper_path`].** That one is the file this machine runs as root; these are
/// copies that ship with the product, and the only thing one is ever used for is being handed to an
/// elevation prompt on a machine with no installed helper — a development tree, a machine before
/// its first prompt, and, since T88d, a machine whose helper `mix uninstall` has removed.
/// `mixengine_core::elevation::helper` prefers the installed copy and never reads this list when
/// there is one.
///
/// **Never empty**: every system answers at least the file beside the program, which is what the
/// fallback was before this list existed.
///
/// **Two arguments and not one**, on [`application_file_name`]'s precedent and for its reason: the
/// window bundle's name is not derivable from anything this crate holds — `mixengine_core::window`
/// declares it, and that crate is the caller. Windows and Linux ignore it.
#[must_use]
pub fn helper_sources(program: &std::path::Path, bundle: &str) -> Vec<PathBuf> {
    crate::sys::install::helper_sources(program, bundle)
}

/// The package that owns `path`, when this system's package database says one does — roadmap
/// task **T88e**.
///
/// **Linux asks, and the other two answer [`None`].** A file a `.deb` or an `.rpm` placed is the
/// package manager's to remove, and `mix uninstall` keeps it. macOS has no `.pkg` uninstaller and
/// nothing that reads a receipt back, so a helper there is removed whoever placed it; Windows'
/// installer never places one. The T88e design, D1 and D2.
///
/// **A name, never a command**, on `Placement::Managed`'s rule: which command removes a package is
/// not something this crate may guess.
#[must_use]
pub fn packaged_by(path: &std::path::Path) -> Option<String> {
    crate::sys::install::packaged_by(path)
}

/// The helper as it is named beside a program: all three systems use this, and two use only this.
///
/// `.` when the program has no parent, which is the answer `mixengine_core::elevation::helper` gave
/// before the question moved here.
pub(crate) fn beside(program: &std::path::Path) -> PathBuf {
    program
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX))
}

/// What to tell a person who ran into `mixengine_core::Error::ElevateMissing` on this machine.
///
/// **Per OS and not one sentence**, because "reinstall" is not always the answer and "it ships
/// beside the program" is not always true. A `.pkg`, a `.deb` or an `.rpm` runs as root during
/// install and writes [`helper_path`] directly — no bootstrap copy is left beside the program, so
/// once the installed one is gone (`mix uninstall`, say) reinstalling is the only way back. The NSIS
/// installer and the portable archives cannot write their OS's protected directory at install time
/// at all, so they keep a copy beside the program on purpose, and that copy survives an uninstall —
/// each module argues its own case.
#[must_use]
pub fn missing_helper_advice() -> &'static str {
    crate::sys::install::missing_helper_advice()
}

/// Make a freshly written file one this machine will execute — roadmap task **T88**.
///
/// **A `.zip` does not carry the executable bit**, and the Windows portable archive is a `.zip`
/// while the two Unix payloads are tarballs. So a `mix` unpacked out of one on a Unix machine can
/// arrive without the bit, and a `mix` that cannot be executed is not an update. Rather than let
/// `mixengine-core` learn which archive shapes carry a mode — and grow a `#[cfg(unix)]` doing it,
/// which `CLAUDE.md` forbids outside this crate — the updater sets it unconditionally through here.
///
/// A no-op on Windows, which has no such bit: executability there is the file's extension and the
/// swap keeps the name.
///
/// **Not [`own_as_root`]**, which is about the one file that belongs to root. This is about the
/// three that belong to whoever installed MixEngine, and it changes nothing about ownership.
///
/// # Errors
///
/// [`Error::Io`](crate::Error::Io) when the permission cannot be set.
pub fn make_executable(path: &std::path::Path) -> Result<()> {
    crate::sys::install::make_executable(path)
}

/// The package receipt that names `path` — the T88f design, D1. [`None`] off macOS.
///
/// The daemon asks through [`crate::Host::installers`], which its tests can replace; this is the
/// same question for a caller holding no host.
#[cfg(feature = "host")]
#[must_use]
pub fn receipt_of(path: &std::path::Path) -> Option<String> {
    crate::host().installers().receipt_of(path)
}

/// Open `package` in the system's installer — the T88f design, D5.
///
/// # Errors
///
/// As [`crate::Installers::open`].
#[cfg(feature = "host")]
pub fn open_installer(package: &std::path::Path) -> Result<()> {
    crate::host().installers().open(package)
}

/// `(device, inode)` of `path`, or [`None`] where the system has no such pair or the file is gone.
///
/// What the T88f install check is cached by: Installer.app writes each file under a new inode and
/// keeps the modification time it had in the package (the T88f readings, M3), so a time would miss
/// a reinstall of the same build.
#[must_use]
pub fn file_identity(path: &std::path::Path) -> Option<(u64, u64)> {
    crate::sys::install::file_identity(path)
}

/// Every directory an installer of this operating system puts MixEngine's programs in, in the order
/// they are consulted — roadmap task **T107**.
///
/// `%LOCALAPPDATA%\Programs\MixEngine` on Windows, `/usr/local/bin` on macOS, `/usr/bin` and then
/// `/usr/local/bin` on Linux — each argued in its own module, and each held to
/// `packaging/common.sh` by `crates/mixengine-core/tests/packaging.rs`.
///
/// Empty is a possible answer and not a fault: a Windows profile whose shell will not name Local
/// AppData has no install location this crate can state, and [`program_path`] still has `PATH`.
#[must_use]
pub fn program_dirs() -> Vec<PathBuf> {
    crate::sys::install::program_dirs()
}

/// Where `name` is on this machine — roadmap task **T107**.
///
/// `name` is a bare name out of `packaging/common.sh`'s `MIX_BINARIES` — `mixengined`, `mix` — and
/// this platform's executable suffix is appended here, so no caller spells `.exe`.
///
/// **Three steps, in decreasing order of certainty.**
///
/// 1. *The running executable's own directory.* The portable archive, the AppImage and every
///    `cargo run`; the one step that cannot be wrong, because a program started out of a directory
///    belongs to the install in that directory.
/// 2. *[`program_dirs`].* The measured case: a per-user NSIS install edits the **user's** `PATH`,
///    and a process — or a window the file manager started — carries the `PATH` it inherited when
///    it opened. Asking `PATH` alone answers "MixEngine is not installed" on a machine that has it.
/// 3. *`PATH`*, for a distribution package or an arrangement somebody made themselves.
///
/// **Empty `PATH` entries are dropped**, and that is not tidiness: an empty entry means the current
/// directory on every system that has a `PATH`, and what this returns is executed.
///
/// The list itself is [`program_search_dirs`].
#[must_use]
pub fn program_path(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);

    first_file(&program_search_dirs(), &file)
}

/// Every directory [`program_path`] consults, in the order it consults them — roadmap task
/// **T111**.
///
/// The running executable's own directory, then [`program_dirs`], then `PATH` without its empty
/// entries — the three steps [`program_path`] argues for, as a list rather than as a search.
///
/// **Public so that a window which found nothing can say where it looked**, with the directories
/// the lookup really walked rather than a second description of them that could drift from it.
#[must_use]
pub fn program_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(directory) = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(std::path::Path::parent)
    {
        dirs.push(directory.to_path_buf());
    }
    dirs.extend(program_dirs());
    if let Some(listed) = std::env::var_os("PATH") {
        dirs.extend(split_path(&listed));
    }

    dirs
}

/// A `PATH` as directories, without the empty entries that mean "here".
fn split_path(listed: &std::ffi::OsStr) -> Vec<PathBuf> {
    std::env::split_paths(listed)
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect()
}

/// The first of `dirs` holding a file called `file`.
fn first_file(dirs: &[PathBuf], file: &str) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(file))
        .find(|candidate| candidate.is_file())
}

/// What a desktop application named `executable` is called on disk here — roadmap task **T106**.
///
/// `mixlab.exe` on Windows, `mixlab` on Linux, `MixLab.app` on macOS, where a windowed application
/// is a directory rather than a file.
///
/// **Two arguments and not one**, because the bundle's name is not derivable from the executable's:
/// `cargo` names the binary after `[package].name` and Tauri names the bundle after `productName`,
/// and `packaging/common.sh` declares both — `MIX_WINDOW` and `MIX_WINDOW_APP`. The caller holding
/// them is `mixengine_core::updates::apply`, which may not ask this question with a `cfg!` of its
/// own (`CLAUDE.md`) and whose two constants `crates/mixengine-core/tests/packaging.rs` keeps in step
/// with that file. Named in prose and not linked: that crate depends on this one, so a link would
/// point the wrong way down the graph and rustdoc would refuse it.
#[must_use]
pub fn application_file_name(executable: &str, bundle: &str) -> String {
    crate::sys::install::application_file_name(executable, bundle)
}

/// The thing an installer placed, given the executable inside it — roadmap task **T106**.
///
/// `…/MixLab.app` for `…/MixLab.app/Contents/MacOS/mixlab`; the executable itself on Windows and
/// Linux, and on a macOS build with no bundle around it.
///
/// **What this is for** is a window asking whether the file an update just replaced is the file it is
/// running from: the daemon answers the directory it swapped in, and the name to join onto it is this
/// path's last component rather than a second copy of the rule above.
#[must_use]
pub fn application_root(executable: &std::path::Path) -> std::path::PathBuf {
    crate::sys::install::application_root(executable)
}

/// The program inside the thing an installer placed — roadmap task **T107**.
///
/// `…/MixLab.app/Contents/MacOS/mixlab` for `…/MixLab.app`; the path itself on Windows and Linux,
/// and on a macOS build with no bundle around it. The exact inverse of [`application_root`], and
/// the third of the trio [`application_file_name`] began: *what is it called*, *what was placed*,
/// *what runs*.
///
/// **What this is for** is a lookup that has just joined [`application_file_name`] onto a directory
/// and now has to say whether the result is really there — on macOS the join is a directory, and
/// `is_file()` on a directory is false however installed the application is.
#[must_use]
pub fn application_executable(placed: &std::path::Path, executable: &str) -> std::path::PathBuf {
    crate::sys::install::application_executable(placed, executable)
}

/// Make a freshly copied file root's, and one the elevation prompt can start.
///
/// The other half of [`helper_path`], and the reason this module has a write at all: putting a
/// binary where root keeps one is two OS-specific facts, not one. Where it goes is above; what a
/// freshly created file there ends up being is here.
///
/// **The owner is set and not assumed, and that is a measurement rather than a precaution.**
/// `std::fs::copy` on macOS is `fclonefileat`/`fcopyfile` with `COPYFILE_ALL`, which carries the
/// *source's* uid across — so a helper copied by root out of a user-owned build directory arrives
/// owned by that user, inside a directory root owns, and the whole point of the directory is gone.
/// CI's macOS leg is what said so: the file installed as uid 501. Linux copies permission bits and
/// not ownership, so it never showed there, and Windows has no such call.
///
/// `elevated` only: nothing running as the user has any business making a file in that directory.
///
/// # Errors
///
/// [`Error::Io`](crate::Error::Io) when the owner or the permission cannot be set.
#[cfg(feature = "elevated")]
pub fn own_as_root(path: &std::path::Path) -> Result<()> {
    crate::sys::install::own_as_root(path)
}

/// What a helper removal actually managed, per path.
///
/// **Two lists and not a boolean**, because on one of the three systems the answer is neither yes
/// nor no: Windows cannot unlink a file whose image is mapped, and the helper is the running program
/// when it is asked to remove itself — so what happens there is that the operating system accepts
/// the removal and performs it at the next restart. A caller that folded both into "removed" would
/// report a file as gone while it was still on disk. See the T87 design, D8.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HelperRemoval {
    /// Paths that are gone now.
    pub removed: Vec<PathBuf>,

    /// Paths the operating system has accepted and will remove at the next restart.
    pub at_next_restart: Vec<PathBuf>,
}

impl HelperRemoval {
    /// Was there nothing to do?
    ///
    /// A helper that was never installed answers `true`, which is what makes running an uninstall
    /// twice not a failure the second time.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.at_next_restart.is_empty()
    }
}

/// Take the privileged helper off this machine — roadmap task **T87**.
///
/// The other end of [`own_as_root`], and the reversal
/// [ADR 0015](../../../docs/decisions/0015-the-helper-installs-itself.md) owed uninstall: the
/// helper installs itself, so the helper is what removes itself.
///
/// Removes the file, and then the directory holding it **only where that directory is MixEngine's
/// own and only when it is empty**. A directory somebody else has put a file in is not this
/// function's to delete, and `rmdir` refusing is the check rather than a walk deciding what belongs
/// to whom. `/Library/PrivilegedHelperTools` on macOS is therefore never touched at all: it is
/// shared with every other product that installs a helper there.
///
/// **A helper that is not installed is not an error.** `mix uninstall` run twice must not fail the
/// second time, and *there was nothing there* is the answer rather than a fault.
///
/// `elevated` only: nothing running as the user can write that directory, so nothing running as the
/// user has any business trying.
///
/// # Errors
///
/// [`Error::Io`](crate::Error::Io) when a path that is there cannot be removed or, on Windows,
/// cannot be scheduled, and [`Error::Os`](crate::Error::Os) on Windows when the shell will not name
/// Program Files.
#[cfg(feature = "elevated")]
pub fn remove_helper() -> Result<HelperRemoval> {
    crate::sys::install::remove_helper()
}

/// What the macOS `.pkg` places as root outside the home — roadmap task **T182a**, spec D2.
///
/// **Constants, never a request's**: the privileged helper removes exactly these, so a daemon that
/// has been talked into anything cannot aim the removal elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePaths {
    /// `/usr/local/bin/{mix,mixengined,mixengine-shim,mixengine-trampoline}`.
    pub binaries: Vec<PathBuf>,
    /// `/Applications/MixLab.app`.
    pub bundle: PathBuf,
    /// The identifier the bundle must carry to be removed.
    pub bundle_identifier: String,
    /// The package receipt, `dev.mixengine.cli`.
    pub receipt: String,
}

/// What [`remove_package`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PackageRemoval {
    /// Paths that are gone now.
    pub removed: Vec<PathBuf>,
    /// A bundle at the bundle's path that is not MixLab's, left alone: the identifier it carried,
    /// or `no Info.plist`.
    pub kept_bundle: Option<String>,
    /// Whether the package database forgot the receipt in this run.
    pub receipt_forgotten: bool,
}

/// [`PackagePaths`] on macOS; `None` elsewhere, where nothing places the program this way.
#[must_use]
pub fn package_paths() -> Option<PackagePaths> {
    crate::sys::install::package_paths()
}

/// The `CFBundleIdentifier` in `<bundle>/Contents/Info.plist`, read from the XML Tauri writes.
/// `None` when there is no plist or no such key.
#[must_use]
pub fn bundle_identifier(bundle: &std::path::Path) -> Option<String> {
    let plist = std::fs::read_to_string(bundle.join("Contents/Info.plist")).ok()?;
    let after = plist.split("<key>CFBundleIdentifier</key>").nth(1)?;
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")? + start;
    Some(after[start..end].trim().to_owned())
}

/// Ask the package database to forget `receipt`: `Ok(true)` forgotten, `Ok(false)` there was none.
///
/// # Errors
///
/// When `pkgutil` could not be run or refused, and on a system with no receipts.
#[cfg(feature = "elevated")]
pub fn forget_receipt(receipt: &str) -> std::io::Result<bool> {
    crate::sys::install::forget_receipt(receipt)
}

/// Take the program the `.pkg` placed off this machine — roadmap task **T182a**.
///
/// The binaries go whatever they are (absent is fine); the bundle goes **only** when its
/// `Info.plist` names [`PackagePaths::bundle_identifier`], so somebody else's application at that
/// path is left alone and named; then `forget` is asked about the receipt. Every step reads *absent*
/// on a second run, so a removal that stopped part-way is finished by running it again.
///
/// # Errors
///
/// [`Error::Io`](crate::Error::Io) for a path that is there and cannot be removed, and for a
/// receipt the database could not be asked about.
#[cfg(feature = "elevated")]
pub fn remove_package(
    paths: &PackagePaths,
    forget: &mut dyn FnMut(&str) -> std::io::Result<bool>,
) -> Result<PackageRemoval> {
    let mut removal = PackageRemoval::default();

    for binary in &paths.binaries {
        match std::fs::remove_file(binary) {
            Ok(()) => removal.removed.push(binary.clone()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(crate::Error::Io {
                    action: "remove",
                    path: binary.clone(),
                    source,
                });
            }
        }
    }

    if paths.bundle.symlink_metadata().is_ok() {
        match bundle_identifier(&paths.bundle) {
            Some(identifier) if identifier == paths.bundle_identifier => {
                std::fs::remove_dir_all(&paths.bundle).map_err(|source| crate::Error::Io {
                    action: "remove",
                    path: paths.bundle.clone(),
                    source,
                })?;
                removal.removed.push(paths.bundle.clone());
            }
            Some(other) => removal.kept_bundle = Some(other),
            None => removal.kept_bundle = Some("no Info.plist".to_owned()),
        }
    }

    removal.receipt_forgotten = forget(&paths.receipt).map_err(|source| crate::Error::Io {
        action: "forget the package receipt",
        path: PathBuf::from(&paths.receipt),
        source,
    })?;

    Ok(removal)
}

#[cfg(test)]
mod tests {

    /// Installer.app writes each file under a new inode and keeps its modification time (the T88f
    /// readings, M3), so the identity is what tells the file at a path was replaced.
    #[test]
    fn a_file_renamed_over_another_has_a_new_identity() {
        let directory = tempfile::tempdir().expect("a directory");
        let path = directory.path().join("mixengined");
        std::fs::write(&path, b"old").expect("the old file");
        let before = file_identity(&path);

        let fresh = directory.path().join("mixengined.new");
        std::fs::write(&fresh, b"new").expect("the new file");
        std::fs::rename(&fresh, &path).expect("renamed over the old one");

        if cfg!(unix) {
            assert!(before.is_some(), "a Unix file has a device and an inode");
            assert_ne!(
                before,
                file_identity(&path),
                "a replaced file is a different file"
            );
        } else {
            assert_eq!(before, None, "no inode to read on this system");
        }
    }

    #[test]
    fn a_missing_file_has_no_identity() {
        assert_eq!(
            file_identity(std::path::Path::new("/nonexistent/mixengined")),
            None
        );
    }
    use super::*;

    /// An empty removal is neither a failure nor a change, and the two vectors are how a caller
    /// tells "it was not there" from "it is scheduled".
    #[test]
    fn a_removal_that_did_nothing_carries_nothing() {
        let nothing = HelperRemoval::default();

        assert!(nothing.removed.is_empty());
        assert!(nothing.at_next_restart.is_empty());
        assert!(nothing.is_empty());
    }

    /// The one property all three systems share, asserted from the outside: a helper that is not
    /// there is a removal that did nothing, and never an error. That is what makes `mix uninstall`
    /// idempotent — running it twice must not fail the second time.
    ///
    /// **Where one *is* installed the premise does not hold**, and this asserts the premise instead
    /// of the behaviour: running it there would be uninstalling the developer's own helper.
    #[cfg(feature = "elevated")]
    #[test]
    fn removing_a_helper_that_is_not_installed_is_not_a_failure() {
        let path = helper_path().expect("this OS names a directory for a privileged helper");

        if path.exists() {
            return;
        }

        let removal = remove_helper().expect("nothing to remove is not an error");

        assert!(removal.is_empty(), "{removal:?}");
    }

    /// The one property all three systems share: an absolute path with a parent.
    ///
    /// A relative answer would be resolved against whatever directory the daemon happened to be
    /// started from, which is not a property of this machine at all — and the caller that acts on
    /// it is running as root.
    #[test]
    fn the_helper_has_an_absolute_home_on_this_system() {
        let path = helper_path().expect("this OS names a directory for a privileged helper");

        assert!(path.is_absolute(), "{} is not absolute", path.display());
        assert!(path.parent().is_some());
        assert!(
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.contains("elevate")),
            "{} is not named after the helper",
            path.display()
        );
    }

    /// The search is the list, in order, and the first file wins. A directory that does not exist
    /// is skipped rather than ending the walk: `program_dirs` names an install location whether or
    /// not this machine has one.
    #[test]
    fn the_first_directory_holding_the_file_answers() {
        let first = tempfile::tempdir().expect("a directory");
        let second = tempfile::tempdir().expect("a directory");
        let name = format!("mixengine-test{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(second.path().join(&name), b"x").expect("a file");

        let dirs = vec![
            PathBuf::from("/mixengine-nothing-is-here"),
            first.path().to_path_buf(),
            second.path().to_path_buf(),
        ];

        assert_eq!(
            first_file(&dirs, &name),
            Some(second.path().join(&name)),
            "the second directory is the only one holding it"
        );
        assert_eq!(first_file(&dirs, "mixengine-test-absent"), None);
    }

    /// An empty `PATH` entry is the current directory on both systems that have a `PATH`, and what
    /// this module returns is executed. Dropping it is the whole of the check.
    #[test]
    fn an_empty_path_entry_is_not_a_directory_to_search() {
        let listed = std::env::join_paths([
            std::ffi::OsString::from(""),
            std::ffi::OsString::from("/usr/bin"),
        ])
        .expect("a PATH");

        assert_eq!(split_path(&listed), vec![PathBuf::from("/usr/bin")]);
    }

    /// Every install location this system names is absolute, and there is at least one.
    ///
    /// The value is per OS and pinned to `packaging/common.sh` in `mixengine-core`'s packaging
    /// test; what is asserted here is the property all three share.
    #[test]
    fn this_system_names_an_install_location() {
        let dirs = program_dirs();

        assert!(!dirs.is_empty(), "every supported system has one");
        for dir in &dirs {
            assert!(dir.is_absolute(), "{} is not absolute", dir.display());
        }
    }

    /// There is always somewhere to look for a program, and the running test binary's own
    /// directory is the first of them — the step that cannot be wrong.
    #[test]
    fn the_running_executables_directory_is_looked_at_first() {
        let running = std::env::current_exe().expect("this test has a path");
        let name = running
            .file_stem()
            .expect("and a name")
            .to_string_lossy()
            .into_owned();

        assert_eq!(program_path(&name).as_deref(), Some(running.as_path()));
    }

    /// The list a window shows when it found nothing is the list the lookup walked — the same
    /// function, not a second description of it — and it begins where [`program_path`] begins.
    ///
    /// Roadmap task **T111**: MixLab prints this when `mixengined` is nowhere, so a person can see
    /// which directory to look in rather than being told only that something is missing.
    #[test]
    fn the_search_list_begins_beside_the_running_executable_and_has_no_empty_entry() {
        let dirs = program_search_dirs();
        let running = std::env::current_exe().expect("this test has a path");

        assert_eq!(dirs.first().map(PathBuf::as_path), running.parent());
        assert!(dirs.iter().all(|dir| !dir.as_os_str().is_empty()));
        for dir in program_dirs() {
            assert!(
                dirs.contains(&dir),
                "{} is an install location and is not searched",
                dir.display()
            );
        }
    }

    /// The inverse of [`application_root`], on every system: what an installer placed, back to the
    /// program inside it, and back again.
    ///
    /// **Tested as a round trip rather than by path**, so the macOS rule — a bundle is a directory
    /// and the program is three components inside it — is asserted on all three systems rather than
    /// on the one that would notice it broken.
    #[test]
    fn what_an_installer_placed_and_the_program_inside_it_are_inverses() {
        let placed =
            PathBuf::from("/opt/mixengine").join(application_file_name("mixlab", "MixLab.app"));

        let program = application_executable(&placed, "mixlab");

        assert!(
            program.ends_with(format!("mixlab{}", std::env::consts::EXE_SUFFIX)),
            "{} does not end in the executable",
            program.display()
        );
        assert_eq!(application_root(&program), placed);
    }
}

/// What the macOS `.pkg` places and how it is taken back off — roadmap task **T182a**, spec D2.
#[cfg(test)]
mod package_tests {
    use super::*;

    /// A plist written the way Tauri writes it: XML, one key per line.
    fn write_bundle(root: &std::path::Path, identifier: Option<&str>) -> PathBuf {
        let bundle = root.join("MixLab.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("the bundle");
        std::fs::write(bundle.join("Contents/MacOS/mixlab"), b"program").expect("the program");
        if let Some(identifier) = identifier {
            std::fs::write(
                bundle.join("Contents/Info.plist"),
                format!(
                    "<plist><dict>\n<key>CFBundleIdentifier</key>\n<string>{identifier}</string>\n</dict></plist>"
                ),
            )
            .expect("the plist");
        }
        bundle
    }

    const NAMES: [&str; 4] = [
        "mix",
        "mixengined",
        "mixengine-shim",
        "mixengine-trampoline",
    ];

    fn paths_under(root: &std::path::Path, identifier: Option<&str>) -> PackagePaths {
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        for name in NAMES {
            std::fs::write(bin.join(name), b"program").expect("a binary");
        }
        PackagePaths {
            binaries: NAMES.iter().map(|name| bin.join(name)).collect(),
            bundle: write_bundle(root, identifier),
            bundle_identifier: "io.github.mixnz.mixlab".to_owned(),
            receipt: "dev.mixengine.cli".to_owned(),
        }
    }

    #[test]
    fn the_bundle_identifier_is_read_out_of_info_plist() {
        let root = tempfile::tempdir().expect("a directory");
        let bundle = write_bundle(root.path(), Some("io.github.mixnz.mixlab"));
        assert_eq!(
            bundle_identifier(&bundle).as_deref(),
            Some("io.github.mixnz.mixlab")
        );
        assert_eq!(bundle_identifier(&root.path().join("nothing.app")), None);
    }

    #[cfg(feature = "elevated")]
    #[test]
    fn the_package_is_removed_and_the_receipt_forgotten() {
        let root = tempfile::tempdir().expect("a directory");
        let paths = paths_under(root.path(), Some("io.github.mixnz.mixlab"));
        let mut forgotten = Vec::new();
        let removal = remove_package(&paths, &mut |receipt| {
            forgotten.push(receipt.to_owned());
            Ok(true)
        })
        .expect("a removal");

        assert_eq!(removal.removed.len(), 5, "{removal:?}");
        assert!(removal.kept_bundle.is_none());
        assert!(removal.receipt_forgotten);
        assert_eq!(forgotten, ["dev.mixengine.cli"]);
        for path in paths.binaries.iter().chain([&paths.bundle]) {
            assert!(!path.exists(), "{}", path.display());
        }
    }

    /// A bundle at the path that is not MixLab is somebody else's, and is left alone and named.
    #[cfg(feature = "elevated")]
    #[test]
    fn a_bundle_of_another_identity_is_kept_and_named() {
        let root = tempfile::tempdir().expect("a directory");
        let paths = paths_under(root.path(), Some("com.example.other"));
        let removal = remove_package(&paths, &mut |_| Ok(false)).expect("a removal");

        assert_eq!(removal.kept_bundle.as_deref(), Some("com.example.other"));
        assert!(paths.bundle.exists());
        assert_eq!(removal.removed.len(), 4);
        assert!(!removal.receipt_forgotten);
    }

    /// Run twice, the second run finds nothing and says so: an uninstall can always be finished.
    #[cfg(feature = "elevated")]
    #[test]
    fn a_second_run_reads_absent() {
        let root = tempfile::tempdir().expect("a directory");
        let paths = paths_under(root.path(), Some("io.github.mixnz.mixlab"));
        remove_package(&paths, &mut |_| Ok(true)).expect("the first run");
        let again = remove_package(&paths, &mut |_| Ok(false)).expect("the second run");
        assert_eq!(again, PackageRemoval::default());
    }

    #[test]
    fn package_paths_are_the_pkgs_on_macos_and_none_elsewhere() {
        let paths = package_paths();
        if !cfg!(target_os = "macos") {
            assert_eq!(paths, None);
            return;
        }
        let paths = paths.expect("macOS names the .pkg's paths");
        assert_eq!(paths.bundle, PathBuf::from("/Applications/MixLab.app"));
        assert_eq!(paths.receipt, "dev.mixengine.cli");
        assert_eq!(paths.bundle_identifier, "io.github.mixnz.mixlab");
        assert_eq!(paths.binaries.len(), 4);
        assert!(
            paths
                .binaries
                .iter()
                .all(|path| path.starts_with("/usr/local/bin"))
        );
    }
}
