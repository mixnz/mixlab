//! The ways updating MixEngine and its helper can fail — what `updates` reports.
//!
//! One of the enums [`crate::Error`] wraps transparently, so its messages and sources are
//! the ones a caller of this crate always saw.

use std::path::PathBuf;

/// The ways updating MixEngine and its helper can fail
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// A `settings` row could not be written as JSON.
    ///
    /// Unreachable for the same reason as [`JobError::JobOutcomeUnwritable`]: every value written
    /// to that table is one of ours. Reported rather than unwrapped because nothing in this crate
    /// panics, and named by key so the one line in a log says which setting.
    ///
    /// [`JobError::JobOutcomeUnwritable`]: crate::JobError::JobOutcomeUnwritable
    #[error("the setting {key} could not be stored")]
    SettingUnwritable {
        /// The key that was being written.
        key: String,
        /// How it failed to serialise.
        #[source]
        source: serde_json::Error,
    },

    /// This copy of MixEngine is not one that may replace itself.
    ///
    /// A `.deb`, an `.rpm`, a `.pkg` or an AppImage put it where it is, and whatever did that is
    /// what updates it. **Refused before a byte is downloaded**, and never by attempting an
    /// elevation: an updater that could ask for root would be the local privilege-escalation path
    /// `docs/features/updates.md` is written to avoid.
    #[error("MixEngine cannot replace its own binaries in {}: {because}", directory.display())]
    UpdateNotWritable {
        /// Where this copy is installed.
        directory: PathBuf,
        /// Why not, from [`updates::Placement`].
        ///
        /// [`updates::Placement`]: crate::updates::Placement
        because: String,
    },

    /// The version a client asked to install is not the one the feed offers.
    ///
    /// **Which is what stops an update nobody read the notes for.** `update.apply` takes the version
    /// the client showed the user, and a check that landed between the prompt and the answer would
    /// otherwise install whatever the feed says now.
    #[error("{asked} is no longer the release being offered")]
    UpdateNotOffered {
        /// What the client asked for.
        asked: String,
        /// What is offered now, or [`None`] when nothing is.
        offered: Option<String>,
    },

    /// The published release has no build for this machine.
    ///
    /// Named rather than reported as "no update", because the two send a reader to entirely
    /// different places: one is a machine that is up to date and the other is a release that skipped
    /// an architecture.
    #[error("the published release has no build for {os}/{arch}")]
    UpdateUnavailable {
        /// This machine's operating system, as the feed spells it.
        os: String,
        /// This machine's architecture, as the feed spells it.
        arch: String,
    },

    /// This copy was installed by the macOS `.pkg`, and the next `.pkg` updates it. What
    /// `update.apply` answers for it, naming the way that does work.
    #[error("the .pkg installed MixEngine in {}, so the next .pkg updates it", directory.display())]
    UpdateUsesInstaller {
        /// Where this copy is installed.
        directory: PathBuf,
    },

    /// The published release has no installer for this machine. A feed from before installers
    /// were published lists none at all.
    #[error("the published release has no installer for {os}/{arch}")]
    InstallerUnavailable {
        /// This machine's operating system, as the feed spells it.
        os: String,
        /// This machine's architecture, as the feed spells it.
        arch: String,
    },

    /// `update.finish` was asked for before the new binaries were on disk.
    #[error("MixEngine {version} is not installed yet")]
    UpdateNotInstalled {
        /// The version that was handed to the installer.
        version: String,
    },

    /// A candidate privileged helper is not signed by the key this build trusts.
    ///
    /// Checked here as well as inside the elevated process, and the two are not one check twice:
    /// the elevated one is the security boundary, and this one is what stops a mirror serving
    /// rubbish from costing an elevation prompt to discover. `docs/features/updates.md` asks
    /// that a tampered artifact be refused *with the reason shown*, and a reason nobody sees until
    /// after they have clicked Allow is not shown.
    #[error("{what} did not verify against MixEngine's update key")]
    HelperSignature {
        /// Which of the three things being checked failed — the key, the signature, or the bytes.
        what: String,
        /// What the verifier said.
        #[source]
        source: Box<minisign_verify::Error>,
    },

    /// The signature is MixEngine's and says nothing this build can read about what it covers.
    ///
    /// The trusted comment is the one place a fact about a signed artifact travels without being
    /// taken on trust, so a comment in a grammar this build does not know is a refusal rather than
    /// a value to guess at.
    #[error("the helper's signature carries {comment:?}, which is not what this build can read")]
    HelperStampUnreadable {
        /// The trusted comment, as it arrived.
        comment: String,
    },

    /// A correctly signed helper for a machine that is not this one.
    ///
    /// Named rather than folded into [`Error::HelperSignature`], because the two send a reader
    /// somewhere different: one is an artifact somebody tampered with, and the other is a release
    /// whose row for this pair points at the wrong file. And installing it would be worse than
    /// either — a privileged helper this machine cannot load is a machine with no elevation left.
    ///
    /// [`Error::HelperSignature`]: crate::UpdateError::HelperSignature
    #[error("that privileged helper is MixEngine's {os}/{arch} build and this machine is not that")]
    HelperNotForThisMachine {
        /// What the signed stamp said.
        os: String,
        /// And its architecture.
        arch: String,
    },

    /// The published release has no privileged helper for this machine.
    ///
    /// A release from before helpers were published lists none at all, which reads the same way and
    /// is the honest answer: there is nothing to fetch.
    #[error("the published release has no privileged helper for {os}/{arch}")]
    HelperUnavailable {
        /// This machine's operating system, as the feed spells it.
        os: String,
        /// This machine's architecture, as the feed spells it.
        arch: String,
    },
}
