//! A `.deb` or an `.rpm`, and the software installer that opens one, on ADR 0050's pattern for the
//! macOS `.pkg`.
//!
//! **The receipt says which database owns the file**, `dpkg:<package>` or `rpm:<package>`, because
//! an update of a packaged copy is the next package of the same kind. **And nothing here elevates**:
//! [`Installers::open`](crate::Installers::open) hands the verified file to the desktop's software
//! installer, which asks for the password itself through polkit.

use std::path::Path;

use crate::packages::PackageDatabase;
use crate::{Error, Result};

#[derive(Debug, Default)]
pub(crate) struct Installers;

impl crate::Installers for Installers {
    fn receipt_of(&self, path: &Path) -> Option<String> {
        let (database, package) = crate::sys::install::owner(path)?;

        Some(receipt(database, &package))
    }

    fn open(&self, package: &Path) -> Result<()> {
        // No display is no software centre to open: the caller prints the command instead.
        let has_display = ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));

        if !has_display {
            return Err(Error::UnsupportedPlatform {
                capability: "Installers",
                reason: "there is no desktop session to open a software installer in".to_owned(),
            });
        }

        let status = std::process::Command::new("xdg-open")
            .arg(package)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|source| Error::Io {
                action: "open the package in this desktop's software installer",
                path: package.to_path_buf(),
                source,
            })?;

        match status.success() {
            true => Ok(()),
            false => Err(Error::UnsupportedPlatform {
                capability: "Installers",
                reason: format!("xdg-open could not open {}", package.display()),
            }),
        }
    }
}

/// `dpkg:<package>` or `rpm:<package>`.
fn receipt(database: PackageDatabase, package: &str) -> String {
    match database {
        PackageDatabase::Dpkg => format!("dpkg:{package}"),
        PackageDatabase::Rpm => format!("rpm:{package}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_receipt_names_the_database_that_answered() {
        assert_eq!(receipt(PackageDatabase::Dpkg, "mixlab"), "dpkg:mixlab");
        assert_eq!(
            receipt(PackageDatabase::Rpm, "mixengine-headless"),
            "rpm:mixengine-headless"
        );
    }
}
