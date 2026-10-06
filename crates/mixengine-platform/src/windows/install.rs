//! Windows: `%ProgramFiles%\MixEngine`, asked of the shell rather than of the environment.

use std::path::PathBuf;

use crate::Result;

/// MixEngine's own directory under Program Files.
///
/// The application itself installs per-user, into `%LOCALAPPDATA%\Programs\MixEngine`, so that an
/// update needs no UAC. This is the one directory it has anywhere else, and it holds exactly one
/// file.
const DIRECTORY: &str = "MixEngine";

/// The helper. `.exe`, because this is the one of the three systems where a program has a suffix.
const HELPER: &str = "mixengine-elevate.exe";

pub(crate) fn helper_path() -> Result<PathBuf> {
    Ok(super::known_folder::program_files()?
        .join(DIRECTORY)
        .join(HELPER))
}

/// The per-user directory every Windows artifact installs into, under `%LOCALAPPDATA%`.
///
/// `packaging/common.sh`'s `MIX_INSTALL_WINDOWS`, and `crates/mixengine-core/tests/packaging.rs`
/// holds the two together.
const PROGRAMS: &str = r"Programs\MixEngine";

/// Where the NSIS installer and the portable archive put MixEngine's programs — roadmap task
/// **T107**.
///
/// One directory, and per user: `RequestExecutionLevel user` means an update needs no UAC, which is
/// the reason the whole install lives under this profile rather than under Program Files.
///
/// Empty when the shell will not name the folder, which is a machine with no install location this
/// crate can state rather than an error: [`crate::install::program_path`] has `PATH` left to try.
pub(crate) fn program_dirs() -> Vec<PathBuf> {
    super::known_folder::local_app_data()
        .map(|base| vec![base.join(PROGRAMS)])
        .unwrap_or_default()
}

/// Nowhere beyond the directory the programs are in — roadmap task **T107**.
///
/// This system's installer places the window beside the other four, so the step in front of this
/// one has already looked. Present rather than absent so the three systems keep one signature.
pub(crate) fn window_dirs(_directory: Option<&std::path::Path>) -> Vec<PathBuf> {
    Vec::new()
}

/// Beside the program, and nowhere else — roadmap task **T88d**.
///
/// `RequestExecutionLevel user` means neither the NSIS installer nor the portable zip can write
/// `%ProgramFiles%` at install time, so both keep their copy where the rest of the install is —
/// which is also where `mix uninstall` leaves it, since it does not touch the program directory.
///
/// `bundle` is macOS' question: this system's window is a file beside the other four.
pub(crate) fn helper_sources(program: &std::path::Path, bundle: &str) -> Vec<PathBuf> {
    let _ = bundle;

    vec![crate::install::beside(program)]
}

/// Nobody — roadmap task **T88e**. The installer is per-user and never writes to Program Files;
/// the helper there is always one `HelperInstall {}` placed.
pub(crate) fn packaged_by(_path: &std::path::Path) -> Option<String> {
    None
}

/// What to tell a person who is missing the helper on this system.
///
/// **The NSIS installer and the portable zip both keep it beside `mixengined`**, in
/// `%LOCALAPPDATA%\Programs\MixEngine` — `RequestExecutionLevel user` means neither can write
/// `%ProgramFiles%` directly, so a bootstrap copy has to sit where the rest of the install already
/// is. That copy survives `mix uninstall`, which leaves the program directory alone, so a plain
/// `mix elevation grant` re-installs it — as it now does on macOS and Linux too, since **T88d** gave
/// those two systems a source of their own.
pub(crate) fn missing_helper_advice() -> &'static str {
    "a release keeps mixengine-elevate beside mixengined in %LOCALAPPDATA%\\Programs\\MixEngine — \
     reinstall MixEngine, or re-extract the zip release, to put it back"
}

/// Nothing to do — roadmap task **T88**.
///
/// There is no execute bit on this system: a file is a program because of its contents and its
/// extension, and the swap that just wrote it kept the name it had. Named rather than silently
/// omitted, so a reader comparing the two halves of this module finds a sentence rather than a gap.
pub(crate) fn make_executable(path: &std::path::Path) -> Result<()> {
    let _ = path;

    Ok(())
}

#[cfg(feature = "elevated")]
pub(crate) fn own_as_root(path: &std::path::Path) -> Result<()> {
    // **Nothing to do on this system, and both halves of that are measured.**
    //
    // There is no execute bit: a file is a program because of its contents and its extension, and
    // who may run it is the DACL the directory hands down — `(OI)(CI)RX` for `Users`, written by
    // `create_root_owned_directory` on the directory this file was just created in.
    //
    // And a file created by a process holding an administrative token belongs to
    // `BUILTIN\Administrators` by that token's default owner, not to the account behind it: read
    // back off a real install on 2026-09-04, where `(Get-Acl …).Owner` answered
    // `BUILTIN\Administrators`. `CopyFile` does not carry an owner across the way macOS's
    // `fcopyfile` does, which is the case the Unix half of this exists for.
    //
    // Named rather than silently omitted, the way `others_can_write` is one module over.
    let _ = path;

    Ok(())
}

/// Hand the helper and its directory to Windows' own removal queue — roadmap task **T87**.
///
/// **A measured constraint and not a preference.** A file whose image is mapped cannot be unlinked
/// on this system, and `mixengine-elevate.exe` is the running program when it applies
/// `helper-remove`. Renaming it is allowed; deleting it is not. `MoveFileExW` with a null
/// destination and `MOVEFILE_DELAY_UNTIL_REBOOT` writes the path into
/// `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\PendingFileRenameOperations`, which the
/// session manager applies **in the order it was written** at the next boot — so the directory can
/// follow the file it holds, provided it is queued after it.
///
/// `%ProgramFiles%\MixEngine` is MixEngine's own directory and holds exactly one file, so it is
/// queued too. `%ProgramFiles%` itself is never named.
///
/// **Rejected: the NTFS self-delete.** Renaming the primary data stream and then setting the delete
/// disposition does work, and it is the technique malware uses to remove its own dropper. Putting it
/// inside the one binary in this product that runs as root — whose stated design constraint is being
/// auditable in a sitting — buys one file's worth of tidiness for a paragraph no reviewer should
/// have to accept. See the T87 design, D8.
#[cfg(feature = "elevated")]
pub(crate) fn remove_helper() -> Result<crate::install::HelperRemoval> {
    let helper = helper_path()?;
    let mut removal = crate::install::HelperRemoval::default();

    // Nothing to schedule is the answer, not a fault: an uninstall run twice must not fail the
    // second time — and scheduling a path that does not exist would leave a queue entry naming
    // something that was never ours.
    if !helper.exists() {
        return Ok(removal);
    }

    schedule(&helper)?;
    removal.at_next_restart.push(helper.clone());

    // After the file and never before it: the queue is applied in the order it was written, and a
    // directory scheduled first would still hold the file when its turn came.
    if let Some(directory) = helper.parent() {
        schedule(directory)?;
        removal.at_next_restart.push(directory.to_path_buf());
    }

    Ok(removal)
}

/// One `MoveFileExW(path, NULL, MOVEFILE_DELAY_UNTIL_REBOOT)`.
///
/// Split out because it is called twice and because the `unsafe` belongs in a frame with nothing
/// else in it — `known_folder.rs`' arrangement, one module over.
#[cfg(feature = "elevated")]
fn schedule(path: &std::path::Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    #[expect(
        unsafe_code,
        reason = "one call with a NUL-terminated buffer owned by this frame and a null destination, \
                  which is how the documentation spells `delete this at the next restart`"
    )]
    let scheduled =
        unsafe { MoveFileExW(wide.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };

    if scheduled == 0 {
        return Err(crate::Error::Io {
            action: "schedule the removal of",
            path: path.to_path_buf(),
            source: std::io::Error::last_os_error(),
        });
    }

    Ok(())
}

/// What a desktop application is called on disk here — roadmap task **T106**.
///
/// `std::env::consts::EXE_SUFFIX` and nothing else: Windows has no application bundle, so the window
/// is one more file beside the other four and the NSIS installer places it as such.
pub(crate) fn application_file_name(executable: &str, _bundle: &str) -> String {
    format!("{executable}{}", std::env::consts::EXE_SUFFIX)
}

/// The executable itself: there is nothing wrapped around it to find.
pub(crate) fn application_root(executable: &std::path::Path) -> std::path::PathBuf {
    executable.to_path_buf()
}

/// The program is what was placed: there is nothing wrapped around it to look inside.
pub(crate) fn application_executable(
    placed: &std::path::Path,
    _executable: &str,
) -> std::path::PathBuf {
    placed.to_path_buf()
}

/// No inode on this system, and no `.pkg` to have replaced a file — roadmap task **T88f**.
pub(crate) fn file_identity(_path: &std::path::Path) -> Option<(u64, u64)> {
    None
}

#[cfg(test)]
mod application_tests {
    use std::path::{Path, PathBuf};

    /// Windows has no bundle: the application is the `.exe`, which is what the NSIS installer places
    /// beside the other four binaries.
    #[test]
    fn an_application_is_an_exe() {
        assert_eq!(
            super::application_file_name("mixlab", "MixLab.app"),
            "mixlab.exe"
        );
    }

    #[test]
    fn an_executable_is_its_own_root() {
        assert_eq!(
            super::application_root(Path::new(r"C:\Users\me\MixEngine\mixlab.exe")),
            PathBuf::from(r"C:\Users\me\MixEngine\mixlab.exe")
        );
    }

    /// T88d. Unchanged behaviour, stated as a fact so the list cannot quietly gain an entry: this
    /// system's installer and its portable zip both keep the source where the programs are.
    #[test]
    fn windows_offers_the_copy_beside_the_program_and_nothing_else() {
        assert_eq!(
            super::helper_sources(
                Path::new(r"C:\Users\x\AppData\Local\Programs\MixEngine\mixengined.exe"),
                "MixLab.app"
            ),
            vec![PathBuf::from(
                r"C:\Users\x\AppData\Local\Programs\MixEngine\mixengine-elevate.exe"
            )]
        );
    }
}

/// Only the macOS `.pkg` places the program this way — roadmap task **T182a**.
pub(crate) fn package_paths() -> Option<crate::install::PackagePaths> {
    None
}

/// No package receipts on this system.
#[cfg(feature = "elevated")]
pub(crate) fn forget_receipt(_receipt: &str) -> std::io::Result<bool> {
    Err(std::io::Error::other(
        "this system keeps no package receipts",
    ))
}
