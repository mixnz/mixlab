//! No `.pkg` receipts and no `.pkg` installer here. Nothing calls [`crate::Installers::open`] on
//! this system: the daemon only hands a package over for a copy whose receipt it found, and there
//! is none to find.

use std::path::Path;

use crate::{Error, Result};

#[derive(Debug, Default)]
pub(crate) struct Installers;

impl crate::Installers for Installers {
    fn receipt_of(&self, _path: &Path) -> Option<String> {
        None
    }

    fn open(&self, _package: &Path) -> Result<()> {
        Err(Error::UnsupportedPlatform {
            capability: "Installers",
            reason: "this system has no .pkg installer".to_owned(),
        })
    }
}
