//! macOS: nothing to install.

use std::path::Path;

use crate::{Error, RedistributableOutcome, Redistributables, Result};

/// This system's answer, which is that the question does not apply here.
#[derive(Debug, Default)]
pub(crate) struct Installer;

impl Redistributables for Installer {
    fn install_visual_cpp(&self, _installer: &Path) -> Result<RedistributableOutcome> {
        Err(Error::UnsupportedPlatform {
            capability: "Redistributables",
            reason:
                "the Microsoft Visual C++ Redistributable is a Windows runtime, not a macOS one"
                    .to_owned(),
        })
    }
}
