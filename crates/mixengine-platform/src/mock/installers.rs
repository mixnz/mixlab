//! The mock's installer: a receipt a test chose, and a record of every package it was asked to open.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::Result;

#[derive(Debug, Default)]
pub(crate) struct Installers {
    receipt: Option<String>,
    opened: Mutex<Vec<PathBuf>>,
}

impl Installers {
    pub(crate) fn with_receipt(receipt: &str) -> Self {
        Self {
            receipt: Some(receipt.to_owned()),
            opened: Mutex::default(),
        }
    }

    pub(crate) fn opened(&self) -> Vec<PathBuf> {
        self.opened
            .lock()
            .map(|opened| opened.clone())
            .unwrap_or_default()
    }
}

impl crate::Installers for Installers {
    fn receipt_of(&self, _path: &Path) -> Option<String> {
        self.receipt.clone()
    }

    fn open(&self, package: &Path) -> Result<()> {
        if let Ok(mut opened) = self.opened.lock() {
            opened.push(package.to_path_buf());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Host as _;

    /// What the daemon's `.pkg` update tests stand on: a host whose binary belongs to a package,
    /// and a record of what it handed to the installer.
    #[test]
    fn a_host_with_a_receipt_names_it_and_records_what_it_opened() {
        let home = tempfile::tempdir().expect("a home");
        let host = crate::mock::Host::with_receipt(home.path(), "dev.mixengine.cli");

        let receipt = host
            .installers()
            .receipt_of(std::path::Path::new("/usr/local/bin/mixengined"));
        host.installers()
            .open(std::path::Path::new("/tmp/x.pkg"))
            .expect("the mock opens anything");

        assert_eq!(receipt.as_deref(), Some("dev.mixengine.cli"));
        assert_eq!(host.opened(), vec![std::path::PathBuf::from("/tmp/x.pkg")]);
    }

    #[test]
    fn the_default_host_has_no_receipt() {
        let home = tempfile::tempdir().expect("a home");
        let host = crate::mock::Host::with_home(home.path());

        assert_eq!(
            host.installers().receipt_of(std::path::Path::new("/x")),
            None
        );
    }
}
