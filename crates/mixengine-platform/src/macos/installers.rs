//! `pkgutil` and Installer.app.

use std::path::Path;
use std::process::Command;

use crate::{Error, Result};

/// Where macOS keeps the two programs. Absolute, so a `PATH` somebody set cannot put another in
/// front of them.
const PKGUTIL: &str = "/usr/sbin/pkgutil";
const OPEN: &str = "/usr/bin/open";

#[derive(Debug, Default)]
pub(crate) struct Installers;

impl crate::Installers for Installers {
    fn receipt_of(&self, path: &Path) -> Option<String> {
        let output = match Command::new(PKGUTIL).arg("--file-info").arg(path).output() {
            Ok(output) => output,
            Err(error) => {
                tracing::warn!(
                    %error,
                    path = %path.display(),
                    "pkgutil could not be run, so this copy is not taken for the .pkg's"
                );
                return None;
            }
        };

        crate::pkgid_of(&String::from_utf8_lossy(&output.stdout)).map(str::to_owned)
    }

    fn open(&self, package: &Path) -> Result<()> {
        // Returns when `open` has, not when the installation has: the person drives Installer.app,
        // and over SSH it still comes up on the desktop session's screen (measured on a Mac).
        let output = Command::new(OPEN)
            .arg("-a")
            .arg("Installer")
            .arg(package)
            .output()
            .map_err(|source| Error::Io {
                action: "open Installer.app for",
                path: package.to_path_buf(),
                source,
            })?;

        if output.status.success() {
            return Ok(());
        }

        Err(Error::Command {
            command: "open",
            path: Some(package.to_path_buf()),
            status: output.status.to_string(),
            output: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Installers as _;

    /// The real `pkgutil`, on a path no package installed: exit 0 and no `pkgid:` line (measured on
    /// a Mac), which is no receipt rather than an error.
    #[test]
    fn a_path_no_package_installed_has_no_receipt() {
        let directory = tempfile::tempdir().expect("a directory");

        assert_eq!(
            super::Installers.receipt_of(&directory.path().join("mixengined")),
            None
        );
    }
}
