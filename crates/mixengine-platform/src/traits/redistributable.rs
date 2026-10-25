//! Running Microsoft's Visual C++ Redistributable installer, once somebody agreed to it.
//!
//! **The one place MixEngine starts a program that asks Windows for administrator rights and is not
//! `mixengine-elevate`** — ADR 0037. What makes that acceptable is here: the file is believed only
//! after Windows accepts its signature, its signer is Microsoft, and it names itself the
//! redistributable; and it is held against change until it has run.
//!
//! **The half that is a decision is compiled everywhere**, on [`crate::AppControl`]'s pattern.

use std::path::Path;

use crate::Result;

/// The organisation Microsoft signs the installer as, read off a real installer's signature.
pub const VISUAL_CPP_PUBLISHER: &str = "Microsoft Corporation";

/// How one run of the installer ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedistributableOutcome {
    /// It installed the runtime.
    Installed,

    /// A runtime as new or newer was already there — `1638`.
    AlreadyNewer,

    /// It installed the runtime, and Windows would like a restart — `3010`.
    RestartRequired,

    /// The person declined Windows' approval dialog — `1223` from the launch, or `1602`.
    Declined,

    /// It ended with any other code.
    Failed {
        /// The installer's exit code.
        code: u32,
    },
}

impl RedistributableOutcome {
    /// What an exit code means.
    #[must_use]
    pub const fn from_exit_code(code: u32) -> Self {
        match code {
            0 => Self::Installed,
            1638 => Self::AlreadyNewer,
            3010 => Self::RestartRequired,
            1223 | 1602 => Self::Declined,
            code => Self::Failed { code },
        }
    }
}

/// Whether a version resource's `ProductName` is the redistributable's.
///
/// Two words rather than one exact string, because the year range in the name moves with every
/// toolset (`2015-2022`, `v14`) and a check that broke on each would be a check somebody disables.
#[must_use]
pub fn names_the_redistributable(product: &str) -> bool {
    product.contains("Visual C++") && product.contains("Redistributable")
}

/// Installing a Visual C++ runtime this machine lacks.
pub trait Redistributables: std::fmt::Debug + Send + Sync {
    /// Believe `installer` is Microsoft's Visual C++ Redistributable, run it quietly, and wait.
    ///
    /// **Blocks until the installer ends**, which includes however long a person takes to answer
    /// Windows' approval dialog — call it off the async runtime.
    ///
    /// # Errors
    ///
    /// [`crate::Error::NotTrusted`] when any check refuses the file, [`crate::Error::Io`] when it
    /// cannot be opened, [`crate::Error::Os`] when it cannot be started, and
    /// [`crate::Error::UnsupportedPlatform`] on macOS and Linux.
    fn install_visual_cpp(&self, installer: &Path) -> Result<RedistributableOutcome>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exit_code_says_how_it_went() {
        assert_eq!(
            RedistributableOutcome::from_exit_code(0),
            RedistributableOutcome::Installed
        );
        assert_eq!(
            RedistributableOutcome::from_exit_code(1638),
            RedistributableOutcome::AlreadyNewer
        );
        assert_eq!(
            RedistributableOutcome::from_exit_code(3010),
            RedistributableOutcome::RestartRequired
        );
        assert_eq!(
            RedistributableOutcome::from_exit_code(1223),
            RedistributableOutcome::Declined
        );
        assert_eq!(
            RedistributableOutcome::from_exit_code(1602),
            RedistributableOutcome::Declined
        );
        assert_eq!(
            RedistributableOutcome::from_exit_code(1603),
            RedistributableOutcome::Failed { code: 1603 }
        );
    }

    /// The first name is what aka.ms served on 2026-09-16; the second is the shape the 14.50 build
    /// registers itself under.
    #[test]
    fn only_a_file_naming_the_redistributable_is_it() {
        assert!(names_the_redistributable(
            "Microsoft Visual C++ 2015-2022 Redistributable (x64) - 14.44.35211"
        ));
        assert!(names_the_redistributable(
            "Microsoft Visual C++ v14 Redistributable (x64) - 14.50.35719"
        ));
        assert!(!names_the_redistributable(
            "Microsoft Visual Studio Installer"
        ));
        assert!(!names_the_redistributable(""));
    }
}
