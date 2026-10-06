//! Linux: `/usr/local/libexec/mixengine`.

use std::path::PathBuf;

use crate::Result;

/// Root-owned on every Linux, and the same path the `.deb` and the `.rpm` write to.
///
/// **One lookup path per system, whatever put the file there** — the T85 design, D3. A distribution
/// package writing under `/usr/local` is against Debian policy and is deliberate: these packages are
/// published by us and installed by hand, and a daemon that had to look in two places depending on
/// how MixEngine arrived is a daemon with two answers to the question of which file it runs as root.
///
/// `libexec` rather than `bin` because nobody runs this by hand: it is started by the elevation
/// prompt with one argument, and by nothing else.
const HELPER: &str = "/usr/local/libexec/mixengine/mixengine-elevate";

pub(crate) fn helper_path() -> Result<PathBuf> {
    Ok(PathBuf::from(HELPER))
}

/// What the `.deb` and the `.rpm` write — `packaging/common.sh`'s `MIX_INSTALL_LINUX`.
const BIN: &str = "/usr/bin";

/// Where a copy placed by hand conventionally goes.
const LOCAL_BIN: &str = "/usr/local/bin";

/// Where the `.deb`, the `.rpm` and a hand-placed copy put MixEngine's programs — roadmap task
/// **T107**.
///
/// [`BIN`] first, because that is what both native packages write and what `packaging/common.sh`
/// declares; [`LOCAL_BIN`] after it, for a copy somebody placed themselves. The AppImage and the
/// tarball are neither — their programs are found beside the one that is running, which is the step
/// in front of this one.
pub(crate) fn program_dirs() -> Vec<PathBuf> {
    vec![PathBuf::from(BIN), PathBuf::from(LOCAL_BIN)]
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
/// **The `.deb` and the `.rpm` ship their source into [`BIN`], which is where `mixengined` is**, so
/// this system needs no candidate the beside rule does not already produce. That directory is
/// root-owned on every Linux, so nothing running as the user can rewrite the file an elevation
/// prompt would run — which is the property `/usr/local/bin` does not have on a Mac, and is why
/// macOS' sibling has an entry this one does not. The AppImage and the tarball put theirs beside
/// the program they were unpacked to, which is the same rule reaching a different directory.
///
/// `bundle` is macOS' question: this system's window is a file beside the other four.
pub(crate) fn helper_sources(program: &std::path::Path, bundle: &str) -> Vec<PathBuf> {
    let _ = bundle;

    vec![crate::install::beside(program)]
}

/// `dpkg-query`, by the path every Debian-family system has it at.
const DPKG_QUERY: &str = "/usr/bin/dpkg-query";

/// `rpm`, by the path every RPM-family system has it at.
const RPM: &str = "/usr/bin/rpm";

/// How long a package database is given to answer. Both answer in milliseconds; this is for a
/// database that is locked or broken, which is not a reason for an uninstall plan to hang.
const ASKING: std::time::Duration = std::time::Duration::from_secs(5);

/// The package that owns `path`: `dpkg-query -S` first, then `rpm -qf` — roadmap task **T88e**.
///
/// **A database that is missing, fails or does not answer in time is "no package"**, which leaves
/// the row as it was before this task: a helper `mix uninstall` removes. Both programs answer an
/// ordinary account, so the daemon asks and the helper never does.
pub(crate) fn packaged_by(path: &std::path::Path) -> Option<String> {
    owner(path).map(|(_, name)| name)
}

/// [`packaged_by`], and which database answered — roadmap task **T182b**, D5: an update of a
/// packaged copy is the next package of *that* kind, `.deb` or `.rpm`.
pub(crate) fn owner(path: &std::path::Path) -> Option<(crate::packages::PackageDatabase, String)> {
    use crate::packages::{PackageDatabase, owning_package};

    let path = path.to_str()?;

    let dpkg = answer(DPKG_QUERY, &["-S", path])
        .and_then(|stdout| owning_package(PackageDatabase::Dpkg, &stdout))
        .map(|name| (PackageDatabase::Dpkg, name));

    dpkg.or_else(|| {
        answer(RPM, &["-qf", "--queryformat", "%{NAME}\n", path])
            .and_then(|stdout| owning_package(PackageDatabase::Rpm, &stdout))
            .map(|name| (PackageDatabase::Rpm, name))
    })
}

/// `program`'s standard output, when it exits 0 within [`ASKING`].
///
/// **Waited on by polling, not by reading**: the answer is one short line, far below a pipe's
/// buffer, so the child cannot block on a full pipe while this waits for it to exit.
/// `LC_ALL=C` keeps the wording [`owning_package`](crate::packages::owning_package) refuses in
/// English.
fn answer(program: &str, args: &[&str]) -> Option<String> {
    use std::io::Read as _;
    use std::process::{Command, Stdio};

    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = std::time::Instant::now() + ASKING;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut stdout = String::new();
                child.stdout.take()?.read_to_string(&mut stdout).ok()?;
                return Some(stdout);
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// What to tell a person who is missing the helper on this system.
///
/// **The `.deb` and the `.rpm` ship a source into [`BIN`]** — roadmap task T88d — so the answer is
/// no longer "reinstall the package": the file the next elevation prompt installs from is already
/// on the machine, and `mix uninstall` does not touch the directory it is in. The AppImage and the
/// tarball keep theirs beside the program they were unpacked to, which is the same sentence with a
/// different directory in it.
pub(crate) fn missing_helper_advice() -> &'static str {
    "the .deb and the .rpm keep a copy of mixengine-elevate beside mixengined in /usr/bin, and the \
     AppImage and the tarball keep theirs beside the program — granting the next elevation prompt \
     installs it from there"
}

#[cfg(feature = "elevated")]
pub(crate) use crate::unix::install::own_as_root;

/// `/usr/local/libexec/mixengine` is MixEngine's own directory and goes with the file it holds —
/// which is why this passes `true` where macOS passes `false`.
#[cfg(feature = "elevated")]
pub(crate) fn remove_helper() -> Result<crate::install::HelperRemoval> {
    crate::unix::install::remove(&helper_path()?, true)
}

/// The executable bit an archive may not have carried — roadmap task **T88**.
pub(crate) use crate::unix::install::{file_identity, make_executable};

/// What a desktop application is called on disk here — roadmap task **T106**.
///
/// The bare name. Linux has no application bundle and no executable suffix; a `.desktop` file points
/// at this file and the packaging scripts place it beside the other four.
pub(crate) fn application_file_name(executable: &str, _bundle: &str) -> String {
    executable.to_owned()
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

#[cfg(test)]
mod application_tests {
    use std::path::{Path, PathBuf};

    /// Linux has no bundle and no suffix: the application is the file `MIX_BINARIES` names.
    #[test]
    fn an_application_is_the_bare_name() {
        assert_eq!(
            super::application_file_name("mixlab", "MixLab.app"),
            "mixlab"
        );
    }

    #[test]
    fn an_executable_is_its_own_root() {
        assert_eq!(
            super::application_root(Path::new("/opt/mixengine/mixlab")),
            PathBuf::from("/opt/mixengine/mixlab")
        );
    }

    /// T88d. [`BIN`](super::BIN) is `MIX_INSTALL_LINUX`, so the copy the `.deb` and the `.rpm` ship
    /// beside `mixengined` **is** the beside candidate — this system needs no second one.
    #[test]
    fn linux_offers_the_copy_beside_the_program_and_nothing_else() {
        assert_eq!(
            super::helper_sources(Path::new("/usr/bin/mixengined"), "MixLab.app"),
            vec![PathBuf::from("/usr/bin/mixengine-elevate")]
        );
    }

    #[test]
    fn the_advice_names_the_directory_beside_the_daemon() {
        let said = super::missing_helper_advice();

        assert!(said.contains("/usr/bin"), "{said}");
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
