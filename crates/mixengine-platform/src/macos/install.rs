//! macOS: `/Library/PrivilegedHelperTools`, and emphatically not `/usr/local`.

use std::path::PathBuf;

use crate::Result;

/// Where macOS puts a privileged helper: the directory `SMJobBless` installs into, `root:wheel`,
/// and claimed by no package manager.
///
/// **`/usr/local` was the draft and is wrong on this system** — the T85 design, D3. Homebrew on an
/// Intel Mac takes ownership of `/usr/local` and everything under it for the installing user, which
/// makes it the one directory here where a "root-owned" helper would be nothing of the kind. On
/// Apple Silicon Homebrew uses `/opt/homebrew` and leaves `/usr/local` absent, so the same constant
/// would also mean two different things on two Macs.
///
/// The directory is flat by convention, so the file carries the reverse-DNS name rather than a bare
/// one that could collide with somebody else's helper.
const HELPER: &str = "/Library/PrivilegedHelperTools/dev.mixengine.elevate";

pub(crate) fn helper_path() -> Result<PathBuf> {
    Ok(PathBuf::from(HELPER))
}

/// The one directory the `.pkg` installs the command line into.
///
/// `packaging/common.sh`'s `MIX_INSTALL_MACOS`. `/usr/local/bin` and not `/usr/bin`: the `.pkg` is
/// not a system package manager's, and `/usr/local` is where a Mac expects one that is not.
const BIN: &str = "/usr/local/bin";

/// Where the `.pkg` and the portable tarball put MixEngine's programs — roadmap task **T107**.
///
/// **The window is not here**: a `.pkg` puts `MixLab.app` in `/Applications`, which is
/// [`window_dirs`]'s whole reason for existing.
pub(crate) fn program_dirs() -> Vec<PathBuf> {
    vec![PathBuf::from(BIN)]
}

/// Where `packaging/macos/build.sh` places the bundle.
const APPLICATIONS: &str = "/Applications";

/// `/Applications`, for an install whose programs are in [`program_dirs`] — roadmap task **T107**.
///
/// **The one system where an installer splits the two.** The `.pkg` puts the four command-line
/// binaries in [`BIN`] and `MixLab.app` in [`APPLICATIONS`], so a daemon that found nothing beside
/// itself has one more place to look — and exactly one.
///
/// **Only for an install**, which is what the guard says. A `cargo run` out of `target/debug` is not
/// an installed MixEngine, and a `/Applications/MixLab.app` some other install put there is not its
/// window: answering it would make `mix database open` a property of the machine rather than of the
/// install, and would turn the command-line suite red on any developer's Mac that has MixLab on it.
pub(crate) fn window_dirs(directory: Option<&std::path::Path>) -> Vec<PathBuf> {
    let installed = directory.is_some_and(|directory| {
        program_dirs()
            .iter()
            .any(|candidate| candidate.as_path() == directory)
    });

    if installed {
        vec![PathBuf::from(APPLICATIONS)]
    } else {
        Vec::new()
    }
}

/// Where a macOS application bundle keeps the files that are not its executable.
/// The command-line programs the `.pkg` places in [`BIN`] — roadmap task **T182a**, spec D2. The
/// names `packaging/common.sh` lists; `mixengine-core/tests/packaging.rs` holds the two in step.
const PACKAGE_BINARIES: [&str; 4] = [
    "mix",
    "mixengined",
    "mixengine-shim",
    "mixengine-trampoline",
];

/// The window's bundle in [`APPLICATIONS`], as `packaging/macos/build.sh` places it.
const WINDOW_BUNDLE: &str = "MixLab.app";

/// The identifier `apps/desktop/src-tauri/tauri.conf.json` gives the window. A bundle at
/// [`WINDOW_BUNDLE`]'s path is removed only when it carries this one.
const WINDOW_BUNDLE_IDENTIFIER: &str = "io.github.mixnz.mixlab";

/// The receipt `pkgbuild --identifier` writes.
const RECEIPT: &str = "dev.mixengine.cli";

/// What the `.pkg` places as root outside the home — roadmap task **T182a**.
pub(crate) fn package_paths() -> Option<crate::install::PackagePaths> {
    Some(crate::install::PackagePaths {
        binaries: PACKAGE_BINARIES
            .iter()
            .map(|name| PathBuf::from(BIN).join(name))
            .collect(),
        bundle: PathBuf::from(APPLICATIONS).join(WINDOW_BUNDLE),
        bundle_identifier: WINDOW_BUNDLE_IDENTIFIER.to_owned(),
        receipt: RECEIPT.to_owned(),
    })
}

/// `pkgutil --forget`, named absolutely so the caller's `PATH` cannot decide what runs. Exit 0 is
/// forgotten; a receipt the database does not hold is nothing to forget.
#[cfg(feature = "elevated")]
pub(crate) fn forget_receipt(receipt: &str) -> std::io::Result<bool> {
    let output = std::process::Command::new("/usr/sbin/pkgutil")
        .args(["--forget", receipt])
        .output()?;
    if output.status.success() {
        return Ok(true);
    }
    let said = String::from_utf8_lossy(&output.stderr);
    if said.contains("No receipt") {
        return Ok(false);
    }
    Err(std::io::Error::other(said.trim().to_owned()))
}

const RESOURCES: &str = "Contents/Resources";

/// What the source inside the bundle is called — the name every other system's copy has.
const HELPER_FILE: &str = "mixengine-elevate";

/// The copy inside the window's bundle, then the copy beside the program — roadmap task **T88d**.
///
/// **The one system whose installer leaves nothing beside `mixengined`.** The `.pkg` splits the
/// install — the command-line binaries into [`BIN`], the bundle into [`APPLICATIONS`] — so the
/// source it ships travels *inside* the bundle, where it is one of the application's own files. A
/// source has to survive `mix uninstall` and **not** survive removing the application, and one of
/// the application's own files is the only thing here that does both: a copy in [`HELPER`]'s own
/// directory would outlive a bundle somebody dragged to the Trash, and `/usr/local/bin` is refused
/// for the reason [`HELPER`] is not there either.
///
/// The bundle first because the installer wrote its contents as root.
///
/// [`window_dirs`] is what decides whether there is a bundle to name, and its guard is the one that
/// matters: a `/Applications/MixLab.app` beside a `cargo run` belongs to a different install.
pub(crate) fn helper_sources(program: &std::path::Path, bundle: &str) -> Vec<PathBuf> {
    let mut sources: Vec<PathBuf> = window_dirs(program.parent())
        .into_iter()
        .map(|root| root.join(bundle).join(RESOURCES).join(HELPER_FILE))
        .collect();

    sources.push(crate::install::beside(program));

    sources
}

/// Nobody, as far as `mix uninstall` is concerned — roadmap task **T88e**, D2.
///
/// The `.pkg` does place the helper, and `pkgutil --file-info` would say so. But macOS has no
/// `.pkg` uninstaller, so a root-owned file kept here would be kept for ever, and nothing on this
/// system reads a receipt back to find the file missing.
pub(crate) fn packaged_by(_path: &std::path::Path) -> Option<String> {
    None
}

/// What to tell a person who is missing the helper on this system.
///
/// **The `.pkg` ships a source inside the bundle** — roadmap task T88d — so the answer is no longer
/// "reinstall". A machine that still has `MixLab.app` installs the helper from it at the next
/// elevation prompt; one that has lost the bundle as well has lost the application, and then
/// reinstalling is the answer again.
pub(crate) fn missing_helper_advice() -> &'static str {
    "the .pkg keeps a copy of mixengine-elevate inside MixLab.app, in Contents/Resources — \
     granting the next elevation prompt installs it from there; reinstall the .pkg if the bundle \
     is gone too"
}

#[cfg(feature = "elevated")]
pub(crate) use crate::unix::install::own_as_root;

/// `/Library/PrivilegedHelperTools` is shared with every other product that installs a helper there,
/// so the file goes and the directory stays — which is why this passes `false` where Linux passes
/// `true`. The same fact that made the directory the right place to install into makes it the wrong
/// one to remove.
#[cfg(feature = "elevated")]
pub(crate) fn remove_helper() -> Result<crate::install::HelperRemoval> {
    crate::unix::install::remove(&helper_path()?, false)
}

/// The executable bit an archive may not have carried — roadmap task **T88**.
pub(crate) use crate::unix::install::{file_identity, make_executable};

/// What a desktop application is called on disk here — roadmap task **T106**.
///
/// The bundle, always. macOS wraps a windowed application in a directory,
/// `packaging/macos/build.sh` places that directory in `/Applications` and `packaging/feed.sh` names
/// it in the payload's `provides`, so it is what `mixengine_core::updates::apply::swap` has to look
/// for. The executable inside it keeps `executable` as its name and is never the thing an installer
/// placed.
pub(crate) fn application_file_name(_executable: &str, bundle: &str) -> String {
    bundle.to_owned()
}

/// The bundle around `executable`, or `executable` itself when there is none.
///
/// **The nearest `.app` walking up and not the outermost**: an application shipped inside another
/// application's `Resources` is still its own application, and the one this process is running. A
/// build with no bundle at all — `cargo tauri dev`, and every `cargo run` — answers the executable,
/// which is the honest answer rather than a failure.
pub(crate) fn application_root(executable: &std::path::Path) -> std::path::PathBuf {
    executable
        .ancestors()
        .find(|ancestor| {
            ancestor
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        })
        .map_or_else(|| executable.to_path_buf(), std::path::Path::to_path_buf)
}

/// The program inside a bundle: `Contents/MacOS/<executable>`, the layout every `.app` has.
///
/// A path that is not a bundle answers itself — `cargo tauri dev` and every `cargo run` produce a
/// bare executable, and a lookup that refused those would be one that only works on a machine with
/// an installer's output on it.
pub(crate) fn application_executable(
    placed: &std::path::Path,
    executable: &str,
) -> std::path::PathBuf {
    if placed
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    {
        return placed.join("Contents").join("MacOS").join(executable);
    }

    placed.to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// macOS wraps a desktop application in a directory, and that directory is what an installer
    /// places — so it is the name an updater has to look for beside the binaries.
    #[test]
    fn an_application_is_named_by_its_bundle() {
        assert_eq!(
            super::application_file_name("mixlab", "MixLab.app"),
            "MixLab.app"
        );
    }

    #[test]
    fn the_root_of_a_bundled_executable_is_the_bundle() {
        assert_eq!(
            super::application_root(Path::new("/Applications/MixLab.app/Contents/MacOS/mixlab")),
            PathBuf::from("/Applications/MixLab.app")
        );
    }

    /// `cargo tauri dev` builds the executable with no bundle around it, and a window started that
    /// way must answer something rather than panic.
    #[test]
    fn an_unbundled_executable_is_its_own_root() {
        assert_eq!(
            super::application_root(Path::new("/work/target/release/mixlab")),
            PathBuf::from("/work/target/release/mixlab")
        );
    }

    /// The split the `.pkg` makes, and the guard that keeps it from applying to a build directory.
    #[test]
    fn only_an_installed_daemon_looks_in_applications() {
        assert_eq!(
            super::window_dirs(Some(Path::new("/usr/local/bin"))),
            vec![PathBuf::from("/Applications")]
        );
        assert!(super::window_dirs(Some(Path::new("/work/target/debug"))).is_empty());
        assert!(super::window_dirs(None).is_empty());
    }

    /// The bundle's own layout, spelled out on the one system where it is not the identity.
    #[test]
    fn the_program_inside_a_bundle_is_three_components_down() {
        assert_eq!(
            super::application_executable(Path::new("/Applications/MixLab.app"), "mixlab"),
            PathBuf::from("/Applications/MixLab.app/Contents/MacOS/mixlab")
        );
        assert_eq!(
            super::application_executable(Path::new("/work/target/release/mixlab"), "mixlab"),
            PathBuf::from("/work/target/release/mixlab")
        );
    }

    /// T88d. The `.pkg` leaves nothing beside `mixengined`, so the source it ships is the copy
    /// inside the bundle — and it comes first, because the installer wrote it as root while
    /// `/usr/local/bin` on an Intel Mac may be Homebrew's.
    #[test]
    fn an_installed_mac_offers_the_bundle_before_the_copy_beside_the_program() {
        assert_eq!(
            super::helper_sources(Path::new("/usr/local/bin/mixengined"), "MixLab.app"),
            vec![
                PathBuf::from("/Applications/MixLab.app/Contents/Resources/mixengine-elevate"),
                PathBuf::from("/usr/local/bin/mixengine-elevate"),
            ]
        );
    }

    /// A `cargo run` is not an installed MixEngine, and `/Applications/MixLab.app` is somebody
    /// else's — the same guard [`window_dirs`](super::window_dirs) already applies.
    #[test]
    fn a_development_tree_offers_only_the_copy_beside_the_program() {
        assert_eq!(
            super::helper_sources(Path::new("/work/target/debug/mixengined"), "MixLab.app"),
            vec![PathBuf::from("/work/target/debug/mixengine-elevate")]
        );
    }

    /// The advice has to name the place this system's source belongs, or it is telling somebody to
    /// reinstall over a file that is already there.
    #[test]
    fn the_advice_names_the_bundle() {
        let said = super::missing_helper_advice();

        assert!(said.contains("MixLab.app"), "{said}");
        assert!(said.contains("Contents/Resources"), "{said}");
    }

    /// The nearest bundle and not the outermost: an application inside another application's
    /// `Resources` is still its own application.
    #[test]
    fn the_nearest_bundle_wins() {
        assert_eq!(
            super::application_root(Path::new(
                "/A.app/Contents/Resources/B.app/Contents/MacOS/b"
            )),
            PathBuf::from("/A.app/Contents/Resources/B.app")
        );
    }
}
