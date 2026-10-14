//! The ways recording, resolving and restoring runtimes and packages can fail.
//!
//! One of the enums [`crate::Error`] wraps transparently, so its messages and sources are
//! the ones a caller of this crate always saw — roadmap task **T207j**.

use std::path::PathBuf;

/// The ways recording, resolving and restoring runtimes and packages can fail.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    /// Something is already installed where this one was going.
    ///
    /// An install never mutates a version that is already there: a runtime directory is immutable
    /// once it exists, which is what lets a project pin one and be sure of what it pinned.
    #[error("{} is already installed", path.display())]
    AlreadyInstalled {
        /// Where the install was going.
        path: PathBuf,
    },

    /// A copy of an earlier home's state that this home will not take — roadmap task **T182h**.
    #[error("this home cannot be restored from {}: {reason}", path.display())]
    RestoreRefused {
        /// The copy.
        path: PathBuf,
        /// Why, as a phrase.
        reason: String,
    },

    /// A directory where an install would go, which could not be recorded as one — roadmap task
    /// **T182f**. Distinct from [`AlreadyInstalled`](Self::AlreadyInstalled) because what a person
    /// does about it is different: record it by hand once it can be checked, or remove it.
    #[error("{} is on disk but not recorded: {reason}", path.display())]
    UnrecordedInstall {
        /// The directory.
        path: PathBuf,
        /// Why it could not be recorded, as a phrase.
        reason: String,
    },

    /// A runtime of this kind and version is already written down.
    ///
    /// Distinct from [`Error::AlreadyInstalled`], which is about a *directory* that is already
    /// there: this one is the row, and the two are separate because the ordering
    /// [`runtimes`] follows deliberately allows a directory with no row — an install that landed
    /// and whose row could not be written is a repair, and the repair is asking again.
    ///
    /// [`Error::AlreadyInstalled`]: crate::RuntimeError::AlreadyInstalled
    /// [`runtimes`]: crate::runtimes
    #[error("{kind} {version} is already installed")]
    AlreadyRecorded {
        /// Which language.
        kind: mixengine_proto::RuntimeKind,
        /// Which version.
        version: mixengine_proto::PackageVersion,
    },

    /// An extension that is compiled into this build was asked to be turned off.
    ///
    /// Not a rewritten file that quietly does nothing: `opcache` is static on the Unix cells and a
    /// DLL on Windows, so the same request is answerable on one machine and not on the other, and
    /// what it would take here is a different build rather than a different setting.
    #[error("{name} is compiled into {kind} {version} and cannot be turned off")]
    ExtensionCompiledIn {
        /// Which language.
        kind: mixengine_proto::RuntimeKind,
        /// Which version.
        version: mixengine_proto::PackageVersion,
        /// The extension that was asked about.
        name: String,
    },

    /// A package of this name and version is already written down.
    ///
    /// [`Error::AlreadyRecorded`]'s sibling one table across, separate because the two name
    /// different things: that one carries a [`RuntimeKind`](mixengine_proto::RuntimeKind), and the
    /// set of packages is open where the set of runtimes is closed.
    ///
    /// [`Error::AlreadyRecorded`]: crate::RuntimeError::AlreadyRecorded
    #[error("{package} {version} is already installed")]
    PackageAlreadyRecorded {
        /// Which package.
        package: String,
        /// Which version.
        version: mixengine_proto::PackageVersion,
    },

    /// A `packages` row, or a `services.id` joined to one, holds a value this build cannot read
    /// back.
    ///
    /// [`Error::UnreadableRuntimeRow`]'s sibling, and one variant for every column for the same
    /// reason: what a reader can do about them is identical and what it needs to say is which one.
    ///
    /// [`Error::UnreadableRuntimeRow`]: crate::RuntimeError::UnreadableRuntimeRow
    #[error("a packages row holds a {column} this build cannot read: {value}")]
    UnreadablePackageRow {
        /// Which column.
        column: &'static str,
        /// What is in it.
        value: String,
    },

    /// A `runtime_installs` row holds a value this build cannot read back.
    ///
    /// One variant for four columns, unlike the `jobs` table's pair, because what a reader can do
    /// about them is identical and what it needs to say is which column: `kind` has a `CHECK` behind
    /// it and the other three have nothing, so a row written by a build that knew a fifth channel —
    /// or edited by hand — arrives here naming the field rather than the table.
    #[error("a runtime_installs row holds a {column} this build cannot read: {value}")]
    UnreadableRuntimeRow {
        /// Which column.
        column: &'static str,
        /// What is in it.
        value: String,
    },

    /// Nothing installed satisfies the version this directory asks for.
    ///
    /// **Never resolved against the index**, which is what makes this an error rather than a
    /// download: a `cd` into a directory must not start one. What the daemon turns it into is
    /// `dependency_missing` with [`resolve::install_command`] as the hint.
    ///
    /// [`resolve::install_command`]: crate::resolve::install_command
    #[error("no installed {kind} matches {constraint}, asked for by {origin}")]
    RuntimeUnresolved {
        /// Which language.
        kind: mixengine_proto::RuntimeKind,
        /// What was asked for.
        constraint: mixengine_proto::VersionConstraint,
        /// Where it was asked from, as a phrase completing "asked for by …".
        origin: String,
    },

    /// Nothing asked for a version, and the kind has no default to fall back on.
    ///
    /// Either nothing of this kind is installed at all, or the version that was the default has
    /// been uninstalled — [`runtimes::forget`] promotes nothing in its place, deliberately, so this
    /// is the state a home is left in and the message has to be able to say so.
    ///
    /// [`runtimes::forget`]: crate::runtimes::forget
    #[error("no {kind} version is installed as the default")]
    NoDefaultRuntime {
        /// Which language.
        kind: mixengine_proto::RuntimeKind,
    },

    /// The version asked for is recorded and its folder is gone — roadmap task **T206d**, D2.
    ///
    /// **Never answered with another version**: a default that moved because a folder vanished would
    /// change what a project runs behind its back. What the daemon turns it into is
    /// `dependency_missing` with [`resolve::restore_command`] as the hint.
    ///
    /// [`resolve::restore_command`]: crate::resolve::restore_command
    #[error("{kind} {version} is recorded but its folder is gone ({})", path.display())]
    RuntimeMissing {
        /// Which language.
        kind: mixengine_proto::RuntimeKind,
        /// Which version.
        version: mixengine_proto::PackageVersion,
        /// Where the folder was.
        path: std::path::PathBuf,
    },

    /// The version resolved, and it publishes no executable under that name.
    ///
    /// Two different disappointments with one message, deliberately, because the person reading it
    /// cannot tell them apart and does not need to: a `pecl` that this build of PHP genuinely does
    /// not ship, and a runtime installed before `provides_json` existed, whose map is empty. Both
    /// are answered by naming what the runtime *does* publish — an empty list being the second case,
    /// stated rather than explained.
    #[error(
        "{kind} {version} publishes no executable called {executable} (it has: {})",
        if known.is_empty() { "nothing recorded".to_owned() } else { known.join(", ") }
    )]
    RuntimeProvidesNothing {
        /// Which language.
        kind: mixengine_proto::RuntimeKind,
        /// Which version.
        version: mixengine_proto::PackageVersion,
        /// The name that was looked up.
        executable: String,
        /// What it does publish, in the order a listing shows them.
        known: Vec<String>,
    },

    /// The shim binary is not beside the program that went looking for it.
    ///
    /// A broken installation and nothing a user did: a release ships `mixengined` and
    /// `mixengine-shim` in one directory, so this means half of one was copied somewhere, or a
    /// development tree was built without `-p mixengine-shim`. Its own variant rather than an
    /// [`Error::Io`] because there is no operation to name — nothing was attempted.
    ///
    /// [`Error::Io`]: crate::Error::Io
    #[error("the shim binary is missing from {}", path.display())]
    ShimMissing {
        /// Where it was expected to be.
        path: PathBuf,
    },
}
