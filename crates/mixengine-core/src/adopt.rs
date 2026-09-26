//! Recording what is on disk without a row — roadmap task **T182f**.
//!
//! Everything MixEngine knows lives in `mixengine.db`, inside the home. An uninstall that removes the
//! home and keeps a directory `[paths]` moved elsewhere leaves runtimes and packages nobody knows
//! about, and an install refuses a directory that exists. [`marker`] is the file every install now
//! leaves in its directory so the next home can record it; [`walk`] finds such directories and
//! records them.
//!
//! Design: `docs/specs/2026-09-27-t182f-a-reinstall-finds-what-the-last-one-kept-design.md`.

pub mod marker;
pub mod walk;

use mixengine_proto::{PackageVersion, RuntimeKind};

/// Which install a directory is supposed to hold, read off its place under `runtimes/` or
/// `packages/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// `runtimes/<kind>/<version>`.
    Runtime {
        /// Which language.
        kind: RuntimeKind,
        /// Which version.
        version: PackageVersion,
    },
    /// `packages/<package>/<version>`.
    Package {
        /// Which package, by the name a recipe is found under.
        package: String,
        /// Which version.
        version: PackageVersion,
    },
}

impl Subject {
    /// The version the directory is named after.
    #[must_use]
    pub fn version(&self) -> &PackageVersion {
        match self {
            Self::Runtime { version, .. } | Self::Package { version, .. } => version,
        }
    }

    /// The kind or package name, as the index spells it.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Runtime { kind, .. } => kind.as_str(),
            Self::Package { package, .. } => package,
        }
    }
}
