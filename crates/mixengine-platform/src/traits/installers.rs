//! Who installed this copy, and handing a package to the system's installer
//! ([ADR 0050](../../../../docs/decisions/0050-a-copy-the-pkg-installed-is-updated-by-the-pkg.md)).
//!
//! **Not `install::packaged_by`.** That one answers "who removes this file", and on macOS it
//! deliberately answers nobody. This one answers "who installed this copy", which on macOS is the
//! `.pkg` receipt.

use std::path::Path;

use crate::Result;

/// The package identifier on a `pkgutil --file-info` answer, or [`None`].
///
/// **The line is the answer, and the exit status is not.** `pkgutil` exits 0 for a file no package
/// installed and for a path that does not exist, and then prints only `volume:` and `path:`
/// (measured on a Mac). Pure and compiled everywhere, so its fixtures run on every system.
#[must_use]
pub fn pkgid_of(file_info: &str) -> Option<&str> {
    file_info
        .lines()
        .find_map(|line| line.strip_prefix("pkgid:"))
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// The system's own package installer.
pub trait Installers: std::fmt::Debug + Send + Sync {
    /// The receipt that names `path`, or [`None`]: on a system without receipts, for a file no
    /// package installed, and when the receipt tool could not be run, which is logged and never an
    /// error — a copy nobody can place is placed by the write probe, as it was before this.
    fn receipt_of(&self, path: &Path) -> Option<String>;

    /// Show `package` to the person in the system's installer, and return once it is open.
    ///
    /// **Not once it is installed.** The person drives the installer, and may cancel it.
    ///
    /// # Errors
    ///
    /// [`crate::Error::UnsupportedPlatform`] where there is no such installer,
    /// [`crate::Error::Io`] when it could not be started, and [`crate::Error::Command`] when it
    /// started and refused.
    fn open(&self, package: &Path) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `pkgutil --file-info /usr/local/bin/mixengined` printed on macOS 15.7.3.
    const OURS: &str = "volume: /\npath: /usr/local/bin/mixengined\n\npkgid: dev.mixengine.cli\n\
                        pkg-version: 0.0.7\ninstall-time: 1790183317\nuid: 0\ngid: 0\nmode: 100755\n";

    /// A file no package installed, and a path that does not exist, both print this, with exit 0.
    const NOBODYS: &str = "volume: /\npath: /usr/local/bin/code\n";

    #[test]
    fn the_pkgid_line_is_the_answer() {
        assert_eq!(pkgid_of(OURS), Some("dev.mixengine.cli"));
    }

    #[test]
    fn no_pkgid_line_means_no_package_whatever_the_exit_status() {
        assert_eq!(pkgid_of(NOBODYS), None, "{NOBODYS}");
        assert_eq!(pkgid_of(""), None);
    }

    #[test]
    fn an_empty_pkgid_is_not_a_package() {
        assert_eq!(pkgid_of("volume: /\npkgid: \n"), None);
    }
}
