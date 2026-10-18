//! The package index as callers ask for it: these kinds, from wherever is cheapest — roadmap task
//! **T196**.
//!
//! # What is published
//!
//! One small signed root, `index-v2.json`, naming every kind and the sha256 of that kind's file;
//! and one unsigned file per kind beside it. A kind file is believed only because a signed root
//! states its hash, which is what lets one signature cover all of them — see [`schema2`].
//!
//! # The contract
//!
//! `mixengine-packages` states it ("What a client must do") and the T196 design's D4–D10 are how it
//! is kept here:
//!
//! 1. Ask for the signature. If it is byte for byte the one already held, nothing was published.
//! 2. Otherwise fetch the root, verify it *before* parsing it, and refuse to go backwards.
//! 3. Bring every kind already cached up to the new root, checking each file's length and hash
//!    before parsing it — and store nothing unless every one of them checks out.
//! 4. If any of that fails, keep everything and answer [`Freshness::Stale`].
//! 5. A kind nobody has asked for yet is fetched when somebody does.
//!
//! # One invariant
//!
//! **Every kind file in the cache hashes to what the cached root says.** A file that does not is
//! *absent* — not an error — which is what lets a commit interrupted half way heal itself at the
//! cost of one fetch.
//!
//! # What is kept in memory
//!
//! The verified root and every kind parsed, for as long as the file each was read from keeps the
//! length and modification time it had. So a call costs one `stat` per kind rather than a read, a
//! hash and a parse — and a cache somebody emptied or rewrote is noticed, because its files changed.
//! Memory is not the disk: what is held there was verified when it was read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use minisign_verify::PublicKey;

use super::format::{Missing, Package, Timestamp};
use super::schema2::{self, Root};
use super::{
    Catalogue, Client, FETCH_TIMEOUT, FRESH_FOR, Freshness, Index, SIGNATURE_SUFFIX, schema1,
};
use crate::{Error, InstallError, Result};

mod kinds;
mod root;

/// What this document is called in a log line and in an error.
const LABEL: &str = <schema1::Document as super::Document>::LABEL;

/// The most a root is read. 1,914 bytes on 2026-09-30, and about a hundred more per kind added.
const ROOT_LIMIT: u64 = 1024 * 1024;

/// The most a signature is read. 308 bytes on 2026-09-30.
const SIGNATURE_LIMIT: u64 = 4 * 1024;

/// How many kind files are fetched at once.
///
/// A choice rather than the result of comparing several: one after another, a first listing of six
/// runtimes is eight requests, and each asset of a GitHub release is a redirect and then the file.
/// Measured on 2026-09-30 against the published index, from one machine, in a debug build: that
/// first listing took 4.6 s, the same call again 1.3 ms, and a refresh with nothing published
/// 279 ms.
const AT_ONCE: usize = 4;

/// A file's length and modification time: what says whether memory still describes it.
type Stamp = (u64, SystemTime);

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// How long ago a file was written.
///
/// A file stamped in the future is a clock that moved, not a fresh file: it is read as "as old as
/// possible" and the network decides — [`Client`]'s rule.
fn age(stamp: Stamp) -> Duration {
    SystemTime::now()
        .duration_since(stamp.1)
        .unwrap_or(Duration::MAX)
}

/// `url` with its last path segment replaced by `name` — a mirror is one setting, and everything
/// the index is made of sits beside whatever that setting names.
fn beside(url: &str, name: &str) -> String {
    match url.rsplit_once('/') {
        Some((base, _)) => format!("{base}/{name}"),
        None => format!("{url}/{name}"),
    }
}

/// The verified root, and the signature it verified against.
#[derive(Debug, Clone)]
struct HeldRoot {
    root: Arc<Root>,

    /// Byte for byte what the server sent: the next refresh compares against it.
    signature: String,

    /// Of the cached root file, which is also where the index's age is read from.
    stamp: Stamp,
}

/// One kind, parsed from a file that hashed to what a root said.
#[derive(Debug, Clone)]
struct HeldKind {
    sha256: String,
    stamp: Stamp,
    packages: Arc<[Package]>,
}

/// What this process has verified and still holds.
#[derive(Debug, Default)]
struct State {
    root: Option<HeldRoot>,

    /// Each kind as decoded. Decoding copies an artifact's shape into it, so this is the expanded
    /// catalogue of the kinds this home has asked for and not of every kind there is.
    ///
    /// Measured on 2026-09-30 with a counting allocator: 471 KB for the six runtime kinds (48
    /// packages) and 1.0 MB for all eighteen (119 packages, 635 artifacts). It grows with the
    /// index, which never loses a version; sharing shapes between artifacts is what would shrink
    /// it, and that means an `Artifact` that no longer owns its fields.
    kinds: BTreeMap<String, HeldKind>,

    /// Kinds whose file was refused, with the signature of the root it was refused under and the
    /// reason — so a file that is wrong is not asked for again on every call (the design's D5).
    refused: BTreeMap<String, (String, String)>,

    /// A cached schema 1 `index.json`, for a home from before T196 and for a source with no set.
    one: Option<(Stamp, Index)>,

    /// Whether falling back to schema 1 has been said. Once per daemon run.
    fell_back: bool,

    /// When the network last failed to answer — see [`RETRY_AFTER`].
    unreachable_at: Option<std::time::Instant>,
}

/// How long after the network failed a call is answered from the cache without asking again.
///
/// A choice, not a measurement: long enough to cover the calls one screen makes together, which
/// would otherwise queue behind the failing one and each wait out a timeout of their own; short
/// enough that a network that came back is used again within a minute. A refresh a person asked
/// for is never held back by it.
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// Remember when the network failed, if that is what `refusal` is.
///
/// **Only a failure that came out of the HTTP client counts** — no connection, a timeout, a server
/// answering `5xx`. A root missing beside its signature is a publish half way through, and a
/// signature that does not verify or a root from the past is a server that answered; none of those
/// is a reason to stop asking.
fn note_unreachable(state: &mut State, refusal: &Error) {
    if let Error::Install(InstallError::IndexTransport { source, .. }) = refusal
        && source.downcast_ref::<reqwest::Error>().is_some()
    {
        state.unreachable_at = Some(std::time::Instant::now());
    }
}

/// Whether `name` is a file [`PackageIndex`] writes in its cache directory: the root, its
/// signature, a kind's file, or one of those half written.
///
/// **One rule for everything that removes them** — falling back to schema 1 here, and
/// `daemon.cleanup` in the daemon — so a file somebody else put beside them stays theirs.
#[must_use]
pub fn is_cache_file(name: &str) -> bool {
    let name = name.strip_suffix(".part").unwrap_or(name);

    name == schema2::ROOT
        || name
            .strip_prefix(schema2::ROOT)
            .is_some_and(|rest| rest == SIGNATURE_SUFFIX)
        || name
            .strip_prefix("index-v2-")
            .and_then(|rest| rest.strip_suffix(".json"))
            .is_some_and(schema2::is_kind_name)
}

/// Which kinds a caller wants.
#[derive(Debug, Clone, Copy)]
enum Want<'a> {
    These(&'a [&'a str]),
    All,
}

/// How a refresh ended, when it did not fail.
enum Refreshed {
    /// The signature is the one already held: nothing was published.
    Unchanged(HeldRoot),

    /// A newer root, committed together with every cached kind it changed.
    New(HeldRoot),

    /// The source publishes no schema 2 set (the design's D9).
    NoSchema2,
}

/// One kind file, fetched and checked: the kind, its bytes, and what they decode to.
type FetchedKind = (String, Vec<u8>, Vec<Package>);

/// Reads the package index, a kind at a time.
#[derive(Debug)]
pub struct PackageIndex {
    /// Where the root is. Its signature and every kind file sit beside it.
    root_url: String,
    key: PublicKey,
    cache_dir: PathBuf,
    http: reqwest::Client,

    /// How long each file may take — [`FETCH_TIMEOUT`], per file rather than for the whole refresh.
    per_file: Duration,

    /// `index.json`, as every MixEngine before T196 read it.
    schema1: Client<schema1::Document>,
    state: tokio::sync::Mutex<State>,
}

impl PackageIndex {
    /// Point a client at the published package index, caching under `cache_dir`.
    ///
    /// # Errors
    ///
    /// As [`PackageIndex::with`].
    pub fn new(cache_dir: &Path) -> Result<Self> {
        Self::with(super::DEFAULT_URL, super::PUBLIC_KEY, cache_dir)
    }

    /// The same, against a named URL and key.
    ///
    /// This is what `MIXENGINE_INDEX_URL` and a team mirror use, and what `MockRegistry` uses in
    /// tests — the key has to be injectable because a test cannot hold the production private key,
    /// and a verification path that is switched off for tests is a verification path nothing checks.
    ///
    /// `url` names a document; the schema 2 set is looked for beside it, whichever of `index.json`
    /// and `index-v2.json` it names.
    ///
    /// # Errors
    ///
    /// A transport that cannot be built, or a public key that is not one.
    pub fn with(url: &str, public_key: &str, cache_dir: &Path) -> Result<Self> {
        let http = super::default_transport().map_err(|source| {
            Error::Install(InstallError::IndexTransport {
                document: LABEL,
                url: url.to_owned(),
                source: Box::new(source),
            })
        })?;

        Self::with_transport(url, public_key, cache_dir, http)
    }

    /// The same, against a `reqwest::Client` the caller already built — see
    /// [`Client::with_transport`].
    ///
    /// # Errors
    ///
    /// The wire error of a public key that is not one.
    pub fn with_transport(
        url: &str,
        public_key: &str,
        cache_dir: &Path,
        http: reqwest::Client,
    ) -> Result<Self> {
        let key = PublicKey::from_base64(public_key).map_err(|source| {
            Error::Install(InstallError::IndexKey {
                source: Box::new(source),
            })
        })?;

        Ok(Self {
            root_url: beside(url, schema2::ROOT),
            key,
            cache_dir: cache_dir.to_path_buf(),
            schema1: Client::with_transport(
                &beside(url, <schema1::Document as super::Document>::CACHE_FILE),
                public_key,
                cache_dir,
                http.clone(),
            )?,
            http,
            per_file: FETCH_TIMEOUT,
            state: tokio::sync::Mutex::new(State::default()),
        })
    }

    /// The same client, giving each file `per_file` rather than the thirty seconds it has.
    ///
    /// What a test shrinks: the limit is thirty seconds and no test can wait one out.
    #[must_use]
    pub fn with_timeout(mut self, per_file: Duration) -> Self {
        self.per_file = per_file;
        self
    }

    /// These kinds, from memory or the cache while the root is fresh, from the network otherwise.
    ///
    /// A kind the index does not name is an empty one. A kind it names and that could not be read
    /// is in the view's [`missing`](Index::missing), and does not fail the others.
    ///
    /// # Errors
    ///
    /// Only the reason the *last* usable index could not be obtained: anything that goes wrong
    /// while there is a cached one falls back to it, with [`Freshness::Stale`].
    pub async fn kinds(&self, kinds: &[&str]) -> Result<Catalogue<Index>> {
        self.view(Want::These(kinds), false).await
    }

    /// The same, asking the network whatever the age of the root — and asking again for a kind
    /// whose file was refused.
    ///
    /// # Errors
    ///
    /// As [`PackageIndex::kinds`].
    pub async fn refresh(&self, kinds: &[&str]) -> Result<Catalogue<Index>> {
        self.view(Want::These(kinds), true).await
    }

    /// Every kind the index names.
    ///
    /// Nothing in the daemon asks for this: a listing names the kinds this build can install, and
    /// a kind published ahead of a release is never fetched. It is what reads the published index
    /// whole, to hold it to `index.json`.
    ///
    /// # Errors
    ///
    /// As [`PackageIndex::kinds`].
    pub async fn all(&self) -> Result<Catalogue<Index>> {
        self.view(Want::All, false).await
    }

    fn path(&self, name: &str) -> PathBuf {
        self.cache_dir.join(name)
    }

    async fn view(&self, want: Want<'_>, force: bool) -> Result<Catalogue<Index>> {
        // One caller at a time: a refresh reads and replaces several files, and two of them
        // interleaved could leave a root beside another root's kinds.
        let mut state = self.state.lock().await;
        let state = &mut *state;

        if force {
            state.refused.clear();
        }

        let held = self.cached_root(state);
        let one = self.cached_schema1(state);

        if !force {
            match (&held, &one) {
                (Some(held), _) if age(held.stamp) < FRESH_FOR => {
                    let freshness = Freshness::Cached {
                        age: age(held.stamp),
                    };
                    return self
                        .collect(state, held.clone(), freshness, false, want)
                        .await;
                }
                (None, Some((stamp, index))) if age(*stamp) < FRESH_FOR => {
                    return Ok(Catalogue {
                        index: index.clone(),
                        freshness: Freshness::Cached { age: age(*stamp) },
                    });
                }
                _ => {}
            }
        }

        // **One floor across both encodings** (the design's D10): a home cannot be walked
        // backwards by being upgraded, nor by a source that stops serving one of them.
        let floor = [
            held.as_ref().map(|held| held.root.generated_at),
            one.as_ref().map(|(_, index)| index.generated_at()),
        ]
        .into_iter()
        .flatten()
        .max();

        // **A network that failed a moment ago is not asked again at once** — the calls queued
        // behind a slow failure are answered from the cache rather than each waiting out a timeout
        // of their own. Only with something cached to answer from, and never for a caller who
        // asked for a refresh.
        let cached = held.is_some() || one.is_some();
        if !force
            && cached
            && state
                .unreachable_at
                .is_some_and(|at| at.elapsed() < RETRY_AFTER)
        {
            return self.keep(state, held, one, None, want).await;
        }

        match self.refresh_root(state, held.as_ref(), floor).await {
            Ok(Refreshed::Unchanged(held) | Refreshed::New(held)) => {
                state.unreachable_at = None;
                self.collect(state, held, Freshness::Fetched, true, want)
                    .await
            }
            Ok(Refreshed::NoSchema2) => self.through_schema1(state, held, floor, want).await,
            Err(refusal) => {
                note_unreachable(state, &refusal);
                self.keep(state, held, one, Some(refusal), want).await
            }
        }
    }

    /// Answer from what is cached, because what is published could not be had.
    ///
    /// **One fallback for every way of failing to get a new index**, on [`Client::refresh`]'s
    /// reasoning: an unreachable server, a signature that does not verify, a root from before the
    /// one held, a kind file that does not hash, a cache directory gone read-only — they differ in
    /// what a person should do and not at all in what this call can do next.
    async fn keep(
        &self,
        state: &mut State,
        held: Option<HeldRoot>,
        one: Option<(Stamp, Index)>,
        refusal: Option<Error>,
        want: Want<'_>,
    ) -> Result<Catalogue<Index>> {
        // [`None`] is a network that failed a moment ago and was not asked again: said once, when
        // it failed, and not on every call answered from the cache since.
        match (held, one) {
            (Some(held), _) => {
                let age = age(held.stamp);
                if let Some(refusal) = &refusal {
                    tracing::warn!(
                        url = %self.root_url,
                        age_hours = age.as_secs() / 3600,
                        error = %refusal,
                        document = LABEL,
                        "keeping the cached document; the published one was not usable"
                    );
                }
                self.collect(state, held, Freshness::Stale { age }, true, want)
                    .await
            }
            (None, Some((stamp, index))) => {
                let age = age(stamp);
                if let Some(refusal) = &refusal {
                    tracing::warn!(
                        url = %self.root_url,
                        age_hours = age.as_secs() / 3600,
                        error = %refusal,
                        document = LABEL,
                        "keeping the cached index.json; the published index was not usable"
                    );
                }
                Ok(Catalogue {
                    index,
                    freshness: Freshness::Stale { age },
                })
            }
            (None, None) => Err(refusal.unwrap_or_else(|| {
                Error::Install(InstallError::IndexTransport {
                    document: LABEL,
                    url: self.root_url.clone(),
                    source: Box::new(std::io::Error::other(
                        "the package index could not be reached a moment ago",
                    )),
                })
            })),
        }
    }

    /// A source with no schema 2 set: read `index.json`, exactly as before T196.
    async fn through_schema1(
        &self,
        state: &mut State,
        held: Option<HeldRoot>,
        floor: Option<Timestamp>,
        want: Want<'_>,
    ) -> Result<Catalogue<Index>> {
        if !state.fell_back {
            state.fell_back = true;
            tracing::info!(
                url = %self.root_url,
                "this source publishes no schema 2 package index; reading index.json, whole"
            );
        }

        match self.schema1.refresh_not_before(floor).await {
            Ok(read) => {
                // One layout on disk at a time: what was just stored is `index.json`.
                if read.freshness == Freshness::Fetched {
                    state.unreachable_at = None;
                    self.forget_set(state);
                }
                Ok(Catalogue {
                    index: read.index.into(),
                    freshness: read.freshness,
                })
            }
            Err(refusal) => {
                note_unreachable(state, &refusal);
                self.keep(state, held, None, Some(refusal), want).await
            }
        }
    }

    /// Remove the schema 2 set from the cache and from memory.
    fn forget_set(&self, state: &mut State) {
        if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
            for entry in entries.flatten() {
                if entry.file_name().to_str().is_some_and(is_cache_file) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }

        state.root = None;
        state.kinds.clear();
        state.refused.clear();
    }

    /// The cached `index.json`, when there is one that verifies.
    fn cached_schema1(&self, state: &mut State) -> Option<(Stamp, Index)> {
        let path = self.path(<schema1::Document as super::Document>::CACHE_FILE);
        let Some(now) = stamp(&path) else {
            state.one = None;
            return None;
        };

        if let Some((held, index)) = &state.one
            && *held == now
        {
            return Some((now, index.clone()));
        }

        let (document, _) = self.schema1.cached()?;
        let index = Index::from(document);
        state.one = Some((now, index.clone()));
        Some((now, index))
    }

    /// Keep a kind nobody had asked for yet, fetched against the cached root by [`Self::fetch_each`].
    fn first_use(
        &self,
        state: &mut State,
        held: &HeldRoot,
        kind: &str,
        fetched: Result<FetchedKind>,
    ) -> Result<Arc<[Package]>> {
        let (_, bytes, packages) = fetched?;
        let packages: Arc<[Package]> = packages.into();
        let name = schema2::kind_file(kind);

        match self.store(&name, &bytes) {
            Ok(()) => {
                if let Some(now) = stamp(&self.path(&name)) {
                    state.kinds.insert(
                        kind.to_owned(),
                        HeldKind {
                            sha256: held.root.kinds[kind].sha256.clone(),
                            stamp: now,
                            packages: Arc::clone(&packages),
                        },
                    );
                }
            }
            // A kind that verified is still an answer when the cache cannot hold it.
            Err(error) => {
                tracing::warn!(%error, kind, "a kind of the package index could not be cached");
            }
        }

        Ok(packages)
    }

    /// The view: each kind from memory, from the cache, or — step 5 of the contract — fetched on
    /// first use.
    ///
    /// `asked` is whether this call has already been to the network for the root. A kind that does
    /// not match a root nobody has just checked means the root is behind: refresh once, and try
    /// again.
    async fn collect(
        &self,
        state: &mut State,
        mut held: HeldRoot,
        mut freshness: Freshness,
        mut asked: bool,
        want: Want<'_>,
    ) -> Result<Catalogue<Index>> {
        'root: loop {
            let names: Vec<String> = match want {
                Want::These(kinds) => kinds.iter().map(|kind| (*kind).to_owned()).collect(),
                Want::All => held.root.kinds.keys().cloned().collect(),
            };
            let mut kinds: BTreeMap<String, Arc<[Package]>> = BTreeMap::new();
            let mut missing = Vec::new();

            // **Every kind not held yet is fetched at once**, before the walk below: one after
            // another, a first listing waited one round trip per kind, behind the lock every
            // listing takes — a cold Packages screen spent seconds on Loading.
            let unheld: Vec<String> = names
                .iter()
                .filter(|kind| {
                    held.root.kinds.contains_key(*kind)
                        && schema2::is_kind_name(kind)
                        && self.cached_kind(state, &held.root, kind).is_none()
                        && !state
                            .refused
                            .get(*kind)
                            .is_some_and(|(under, _)| *under == held.signature)
                })
                .cloned()
                .collect();
            let mut fetched = self.fetch_each(&held.root, &unheld).await;

            for kind in names {
                // A kind the root does not name does not exist. One whose name is not a name
                // never becomes a path or a URL.
                if !held.root.kinds.contains_key(&kind) || !schema2::is_kind_name(&kind) {
                    if held.root.kinds.contains_key(&kind) {
                        tracing::warn!(
                            kind,
                            "the package index names a kind that is not a name; skipping it"
                        );
                    }
                    kinds.insert(kind, Arc::from(Vec::new()));
                    continue;
                }

                if let Some(packages) = self.cached_kind(state, &held.root, &kind) {
                    kinds.insert(kind, packages);
                    continue;
                }

                if let Some((under, reason)) = state.refused.get(&kind)
                    && *under == held.signature
                {
                    missing.push(Missing {
                        kind,
                        reason: reason.clone(),
                    });
                    continue;
                }

                let one = match fetched.remove(&kind) {
                    Some(one) => one,
                    None => self.fetching(&held.root, &kind).await,
                };
                let tried = self.first_use(state, &held, &kind, one);

                let refusal = match tried {
                    Ok(packages) => {
                        kinds.insert(kind, packages);
                        continue;
                    }
                    Err(refusal) => refusal,
                };

                if !asked {
                    asked = true;
                    let floor = Some(held.root.generated_at);

                    // A newer root: everything gathered so far was gathered under the old one, so
                    // the view is built again from the top. `asked` makes that happen once.
                    if let Ok(Refreshed::New(newer)) =
                        self.refresh_root(state, Some(&held), floor).await
                    {
                        held = newer;
                        freshness = Freshness::Fetched;
                        continue 'root;
                    }
                }

                tracing::warn!(
                    kind,
                    error = %refusal,
                    "this kind of the package index could not be read"
                );
                let reason = refusal.to_string();

                // A file that is wrong stays wrong until the root changes. A server that could
                // not be reached is the network, and the next call tries again.
                if !matches!(refusal, Error::Install(InstallError::IndexTransport { .. })) {
                    state
                        .refused
                        .insert(kind.clone(), (held.signature.clone(), reason.clone()));
                }
                missing.push(Missing { kind, reason });
            }

            return Ok(Catalogue {
                index: Index::from_kinds(held.root.generated_at, kinds, missing),
                freshness,
            });
        }
    }
}
