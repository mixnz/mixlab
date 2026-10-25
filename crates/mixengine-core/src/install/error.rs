//! The ways reading the signed index and installing what it names can fail.
//!
//! One of the enums [`crate::Error`] wraps transparently, so its messages and sources are
//! the ones a caller of this crate always saw.

use std::path::PathBuf;

/// The ways reading the signed index and installing what it names can fail.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// The package index could not be fetched.
    ///
    /// **`document` is what this family gained with the extension registry, and it is why there is
    /// no second family.** Two signed documents are published — the package index and the extension
    /// registry — and they fail in exactly these five ways. Duplicating the variants would
    /// duplicate every `match` that reads them; naming the document in the message is the whole of
    /// the difference a reader needs.
    ///
    /// **Not always fatal**, and the only place in this enum where that is true of the error itself
    /// rather than of what the caller does with it: [`index::Client::catalogue`] constructs this,
    /// looks for a cached index, and returns the cache instead if there is one. It reaches a user
    /// only when there is no cache at all.
    ///
    /// [`index::Client::catalogue`]: crate::index::Client::catalogue
    #[error("cannot reach the {document} at {url}")]
    IndexTransport {
        /// Which signed document this is about — see [`index::Document::LABEL`].
        ///
        /// [`index::Document::LABEL`]: crate::index::Document::LABEL
        document: &'static str,
        /// What was being fetched — the document or its signature, which are separate requests and
        /// separate ways to fail.
        url: String,
        /// What the HTTP client said. Boxed to keep this enum small: a `reqwest::Error` is several
        /// times the size of every other variant here.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The package index is not signed by the key this build trusts.
    ///
    /// The one failure that cannot happen by accident. A truncated download does not produce a valid
    /// signature over different bytes; a mirror serving somebody else's index does.
    #[error("the {document} at {url} is not signed by this build's key")]
    IndexSignature {
        /// Which signed document this is about — see [`index::Document::LABEL`].
        ///
        /// [`index::Document::LABEL`]: crate::index::Document::LABEL
        document: &'static str,
        /// Where the document came from — a URL, or the cache file that was found to be tampered
        /// with.
        url: String,
        /// What the verifier said. Boxed for [`Error::IndexTransport`]'s reason.
        ///
        /// [`Error::IndexTransport`]: crate::InstallError::IndexTransport
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The index verified, and then did not parse.
    ///
    /// Which means *we* published something malformed: the signature already established the
    /// document is ours. Distinct from [`Error::IndexSignature`] because the two send whoever reads
    /// the message to entirely different places.
    ///
    /// [`Error::IndexSignature`]: crate::InstallError::IndexSignature
    #[error("the {document} at {url} is signed but unreadable")]
    IndexUnreadable {
        /// Which signed document this is about — see [`index::Document::LABEL`].
        ///
        /// [`index::Document::LABEL`]: crate::index::Document::LABEL
        document: &'static str,
        /// Where the document came from.
        url: String,
        /// How it failed to parse.
        #[source]
        source: serde_json::Error,
    },

    /// The index is a document version this build does not know how to read.
    ///
    /// A MixEngine older than the index it is pointed at. The fix is an application update, and
    /// saying so is better than the field-by-field confusion a best-effort parse would produce.
    #[error("the {document} at {url} is schema {found}; this build reads schema {expected}")]
    IndexSchema {
        /// Which signed document this is about — see [`index::Document::LABEL`].
        ///
        /// [`index::Document::LABEL`]: crate::index::Document::LABEL
        document: &'static str,
        /// Where the document came from.
        url: String,
        /// What it says it is.
        found: u32,
        /// What this build can read.
        expected: u32,
    },

    /// A timestamp handed to a parser is not the one shape this product writes.
    ///
    /// Only reachable through [`FromStr`](std::str::FromStr), which is the publishing pipeline's
    /// door onto [`index::Timestamp`]; a timestamp arriving *inside* a document is refused by
    /// `Deserialize` instead, with the message serde needs. Two doors onto one parser, because the
    /// two callers need different sentences — this one is a person who mistyped a shell argument.
    ///
    /// [`index::Timestamp`]: crate::index::Timestamp
    #[error("{text:?} is not a UTC RFC 3339 second, e.g. 2026-08-14T06:55:12Z")]
    Timestamp {
        /// What was handed over.
        text: String,
    },

    /// A kind's file of the package index is not the one the signed root names.
    ///
    /// The root verified. This file is believed only because the root states its hash, and it is
    /// not that file: an index caught while it was being published, a mirror serving an old copy,
    /// or somebody in between. It costs that kind and no other.
    #[error("the package index's file for {kind} at {url} {problem}")]
    IndexKind {
        /// Which kind.
        kind: String,
        /// Where the file came from — a URL, or the cache file.
        url: String,
        /// What is wrong with it.
        problem: crate::index::KindProblem,
    },

    /// The server offered an index older than the one already cached.
    ///
    /// Every index we ever published is validly signed, so the signature cannot tell an old one from
    /// the current one — which makes replaying a copy from before a security release a real move
    /// rather than a theoretical one. `generated_at` is what separates them, and the cached document
    /// is kept.
    #[error(
        "the {document} at {url} went backwards: it says {offered}, the cached copy says {cached}"
    )]
    IndexRolledBack {
        /// Which signed document this is about — see [`index::Document::LABEL`].
        ///
        /// [`index::Document::LABEL`]: crate::index::Document::LABEL
        document: &'static str,
        /// Where the older document came from.
        url: String,
        /// When the cached document was generated.
        cached: String,
        /// When the offered one was.
        offered: String,
    },

    /// The public key compiled into this binary is not a public key.
    ///
    /// A broken build and nothing a user can act on. Reported rather than unwrapped because nothing
    /// in this crate panics.
    #[error("this build's package index key is not a valid minisign key")]
    IndexKey {
        /// What the parser said.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// An artifact could not be fetched.
    ///
    /// [`Error::IndexTransport`]'s sibling one layer down, and unlike it this one is **always
    /// fatal**: there is no cached copy of a runtime to fall back to, so the only thing left to say
    /// is that the download did not happen.
    ///
    /// [`Error::IndexTransport`]: crate::InstallError::IndexTransport
    #[error("cannot download {url}")]
    ArtifactTransport {
        /// What was being fetched.
        url: String,
        /// What the HTTP client said. Boxed to keep this enum small, as at [`Error::IndexTransport`].
        ///
        /// [`Error::IndexTransport`]: crate::InstallError::IndexTransport
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The transfer ended before the artifact did, after every attempt to resume it.
    ///
    /// What is on disk is **kept**: it is a prefix of the file that was wanted, and asking again is
    /// meant to continue from it rather than start over.
    #[error("{url} stopped after {received} of {expected} bytes")]
    ArtifactIncomplete {
        /// What was being fetched.
        url: String,
        /// How large the signed index says it is.
        expected: u64,
        /// How much arrived.
        received: u64,
    },

    /// The server offered more bytes than the index says the artifact has.
    ///
    /// Refused while it is happening rather than after, because the alternative to a bound here is
    /// filling a disk to discover that the checksum does not match.
    #[error("{url} is larger than the {expected} bytes the index declares")]
    ArtifactTooLarge {
        /// What was being fetched.
        url: String,
        /// How large the signed index says it is.
        expected: u64,
    },

    /// A download does not hash to what the signed index promised.
    ///
    /// The download is deleted, which
    /// [security-model.md](../../../docs/architecture/security-model.md) requires and which is
    /// also what stops a `.part` that can never verify from being resumed forever. Whether this is
    /// a corrupted transfer or a mirror serving something else, the next step is the same one.
    #[error("{url} does not match the checksum the index publishes for it")]
    ArtifactChecksum {
        /// What was being fetched.
        url: String,
        /// The hash the index publishes.
        expected: String,
        /// The hash of what arrived.
        found: String,
    },

    /// The index names an archive in a shape this build cannot unpack.
    ///
    /// A MixEngine older than the pipeline that packed it, which the update is the fix for. Refused
    /// before the download rather than after, so it costs a round trip and not an artifact.
    #[error("{url} is not an archive this build can unpack")]
    ArtifactFormat {
        /// What was being fetched.
        url: String,
    },

    /// An archive could not be read.
    ///
    /// Reached after the checksum has already agreed, so the bytes *are* the ones we published —
    /// which makes this a packaging failure of ours rather than a transfer failure, exactly as
    /// [`Error::IndexUnreadable`] is to [`Error::IndexSignature`].
    ///
    /// [`Error::IndexSignature`]: crate::InstallError::IndexSignature
    /// [`Error::IndexUnreadable`]: crate::InstallError::IndexUnreadable
    #[error("cannot unpack {}", archive.display())]
    ArchiveUnreadable {
        /// The downloaded file.
        archive: PathBuf,
        /// What the container or the decompressor said. Boxed for [`Error::IndexTransport`]'s reason.
        ///
        /// [`Error::IndexTransport`]: crate::InstallError::IndexTransport
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// An archive contains an entry that names a path outside where it is being unpacked.
    ///
    /// The oldest attack there is against an installer, and one that a correct signature and a
    /// matching checksum do nothing about: both say the archive is the one we published, and neither
    /// says what is inside it. Nothing partial is left behind — the staging directory this was being
    /// written into is removed.
    #[error("{} contains {entry}, which is not inside it", archive.display())]
    UnsafeArchiveEntry {
        /// The downloaded file.
        archive: PathBuf,
        /// The entry that named somewhere else.
        entry: String,
    },

    /// An archive does not contain something its index entry says it provides.
    ///
    /// A packaging bug found at install time instead of at the moment somebody needed the binary.
    #[error("{url} does not contain the {executable} it lists")]
    MissingFromArtifact {
        /// What was being installed.
        url: String,
        /// The name the index publishes it under.
        executable: String,
        /// Where inside the archive it was supposed to be.
        path: String,
    },

    /// The artifact was unpacked and then would not run on this machine.
    ///
    /// **The one failure a checksum cannot see.** A hash proves the bytes are ours; it says nothing
    /// about a missing VC++ redistributable, a glibc older than the build's floor, or an image the
    /// loader refuses. Found while the install is still in its staging directory, so nothing that
    /// does not work is ever renamed into place.
    #[error("{} does not run on this machine: {detail}", program.display())]
    SmokeTestFailed {
        /// Which binary was run.
        program: PathBuf,
        /// What happened when it was — an exit status with the first line of its complaint, the
        /// operating system's refusal to start it, or a timeout.
        detail: String,
    },

    /// A JDK's own `keytool` would not answer, or would not write — ADR 0039.
    ///
    /// **Fatal to nothing.** An install and a daemon start carry on without it; what it costs is a
    /// JDK that cannot verify this home's own HTTPS sites, which `mix doctor` reports and repairs.
    #[error("{} would not update this JDK's certificate store: {detail}", program.display())]
    KeytoolRefused {
        /// Which `keytool` was run.
        program: PathBuf,
        /// Its last line, the operating system's refusal to start it, or a timeout.
        detail: String,
    },

    /// An install stopped because it was asked to.
    ///
    /// Not a failure of anything, and the daemon turns it into a cancelled job rather than a failed
    /// one. The partial download is kept: somebody who cancels at sixty percent and asks again has
    /// not asked for those bytes to be thrown away.
    #[error("the install was cancelled")]
    InstallCancelled,
}
