//! Reading the signed package index: what exists, and where to get it.
//!
//! # One verification path, two sources
//!
//! A signed document arrives either from the network or from the cache on disk, and **both go
//! through the same function**: [`Client`]'s for a single document, and for the package index the
//! two doors [`PackageIndex`] has — one for its signed root, one for a kind file, which is believed
//! because that root states its hash. Re-verifying a file this process wrote a minute ago looks
//! redundant and is not: the cache is an ordinary file in the user's home that any local process can
//! rewrite, and a client that trusts it because it trusted the network once has moved the trust
//! boundary from "we signed this" to "nothing on this machine touched it".
//!
//! It is also cheap. The package index's root was 1,914 bytes on 2026-09-30 — one BLAKE2b hash and
//! one Ed25519 check — and its largest kind file 30,670, one SHA-256. A file this process has
//! already verified and that has not changed since is not read again at all.
//!
//! # Why the network lives here rather than in the daemon
//!
//! Whether to go to the network at all *is* the cache policy — freshness, staleness and the
//! rollback check are the same decision as "fetch or do not fetch". Splitting them would put the
//! policy in one crate and its trigger in another, and the interesting failures all live in the
//! seam. So this module owns `reqwest` the way [`crate::store`] owns `sqlx`.
//!
//! # TLS
//!
//! `reqwest`'s default rustls path uses [`rustls-platform-verifier`], which delegates to the
//! operating system's own verifier. That is what makes MixEngine work on a machine behind a
//! corporate proxy that installs its own root — where a bundled root store would fail with nothing
//! the user could do about it — and it is what lets `MIXENGINE_MIRROR_URL` point at a team mirror
//! with an internal certificate, which
//! [runtime-packaging.md](../../../docs/operations/runtime-packaging.md) promises.
//!
//! Being permissive there is affordable precisely because TLS is **not** what decides whether an
//! index is ours: the Ed25519 signature is, end to end, and it is checked after the bytes arrive
//! regardless of how they arrived. A proxy that can read the traffic still cannot forge the
//! document.
//!
//! One consequence worth stating before somebody finds it and calls it a hole: from Phase 5 onwards
//! MixEngine installs its own CA into that same OS trust store, so its own CA becomes trusted by its
//! own downloader. The private key for it sits on the user's machine — anybody holding it already
//! owns the machine — and it cannot touch the signature above, which is the layer that actually
//! decides authenticity.
//!
//! [`rustls-platform-verifier`]: https://github.com/rustls/rustls-platform-verifier

use std::time::Duration;

#[cfg(feature = "network")]
mod client;
pub mod format;
#[cfg(feature = "network")]
pub mod packages;
pub mod schema1;
pub mod schema2;

#[cfg(feature = "network")]
pub use client::{Client, default_transport};
#[cfg(feature = "network")]
pub(crate) use client::{FETCH_TIMEOUT, Got, SIGNATURE_SUFFIX, get, verify};
pub use format::{
    Arch, Artifact, Channel, Extensions, Index, Missing, Os, Package, Requires, Selection, TARGETS,
    Target, Timestamp,
};
#[cfg(feature = "network")]
pub use packages::PackageIndex;

/// The key every published index is signed with, compiled in.
///
/// Rotating it needs an application release, which is the point: a key the index itself could
/// announce would be a key an attacker serving the index could announce. The same value is committed
/// as `minisign.pub` in the packaging repository, so a reader can check that what signs the index is
/// what this binary trusts.
pub const PUBLIC_KEY: &str = "RWSUOSSPLuuv4OGGJTNtxoUeKFOWBAQ8UwqucFPqcJ8hAdoRZCNgzPEW";

/// Where the index is published.
///
/// A GitHub release asset whose tag is moved rather than added to, so the URL never changes while
/// the document behind it does. It names schema 1's `index.json`; [`PackageIndex`] reads the
/// schema 2 set published beside it, and this document only where that set is not.
pub const DEFAULT_URL: &str =
    "https://github.com/mixnz/mixengine-packages/releases/download/index/index.json";

/// How long a fetched index is used without asking again.
///
/// Six hours is short enough that a security release is picked up the same day and long enough that
/// a person installing four runtimes in a row does it over one round trip.
pub const FRESH_FOR: Duration = Duration::from_secs(6 * 60 * 60);

/// How a kind file failed to be the one its root names.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KindProblem {
    /// The server has no such file.
    Absent,

    /// It is not the length the root states.
    Length {
        /// What the root says.
        expected: u64,
        /// What arrived, or what the server said would.
        found: u64,
    },

    /// It does not hash to what the root states.
    Hash,

    /// It says it is another kind's file.
    Named(String),

    /// It is the root's, by hash, and what it says cannot be turned into packages.
    Undecodable(String),
}

impl std::fmt::Display for KindProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Absent => formatter.write_str("is not published"),
            Self::Length { expected, found } => {
                write!(
                    formatter,
                    "is {found} bytes and the signed root says {expected}"
                )
            }
            Self::Hash => formatter.write_str("does not hash to what the signed root says"),
            Self::Named(other) => write!(formatter, "says it is {other}'s"),
            Self::Undecodable(why) => write!(formatter, "cannot be read: {why}"),
        }
    }
}

/// How the index in hand was obtained, and whether the caller should say so.
///
/// Returned rather than logged alone because the three cases want different words in three different
/// places — a CLI prints them, the GUI shows a badge, and `mix doctor` reports them — and a caller
/// that only wants the data can ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Freshness {
    /// Fetched from the network just now.
    Fetched,

    /// Read from the cache, still inside [`FRESH_FOR`].
    Cached {
        /// How long ago it was fetched.
        age: Duration,
    },

    /// Read from the cache, past [`FRESH_FOR`], because the network could not be reached.
    ///
    /// **An answer and not an error.** The alternative is a tool that can list nothing while a
    /// user's wifi is down, and a version list a few days old is still enough to install PHP 8.3.
    /// The signature was checked exactly as it would have been on a fresh copy: old is not
    /// untrusted.
    Stale {
        /// How long ago it was fetched.
        age: Duration,
    },
}

impl Freshness {
    /// Whether the caller should tell somebody.
    #[must_use]
    pub fn is_stale(self) -> bool {
        matches!(self, Self::Stale { .. })
    }
}

/// A verified document, and how it got here.
#[derive(Debug, Clone)]
pub struct Catalogue<D> {
    /// What was signed.
    pub index: D,
    /// Where it came from.
    pub freshness: Freshness,
}

/// A signed document this client knows how to read.
///
/// **More than one document is published**: the package index says what can be installed,
/// `extensions.json` says which extensions exist, and `latest.json` which release of MixEngine is
/// newest. The package index is read by [`PackageIndex`], and through this trait only where a
/// source has nothing but `index.json`. They want identical treatment — verify before parse, cache,
/// refuse to be walked backwards — and they must not share a cache file or a rollback mark, or a
/// registry fetched at noon would look like an index rolled back to noon. So the client is generic
/// over the document and this trait is everything it needs to know about one.
///
/// The [`InstallError::IndexSignature`](crate::InstallError::IndexSignature) family is deliberately
/// *not* generic with it: every variant already carries the `url`, which is what says which
/// document failed, and renaming them to something document-neutral would touch every call site
/// and every test asserting one in order to rename something that is still accurate —
/// `extensions.json` is an index too.
pub trait Document: serde::de::DeserializeOwned + Clone + std::fmt::Debug {
    /// The document version this build can read.
    const SCHEMA: u32;

    /// What this document is called in a log line a person reads.
    const LABEL: &'static str;

    /// What it is cached as, under the cache directory. Two documents must not share one file.
    const CACHE_FILE: &'static str;

    /// The version the document says it is.
    fn schema(&self) -> u32;

    /// When the publishing pipeline generated it — what makes a rollback detectable, since every
    /// version we ever published is signed just as validly as the newest.
    fn generated_at(&self) -> Timestamp;
}
