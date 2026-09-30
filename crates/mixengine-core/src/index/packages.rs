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
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use minisign_verify::PublicKey;
use sha2::{Digest as _, Sha256};

use super::format::{Missing, Package, Timestamp};
use super::schema2::{self, Entry, KindFile, Root};
use super::{
    Catalogue, Client, FETCH_TIMEOUT, FRESH_FOR, Freshness, Got, Index, KindProblem,
    SIGNATURE_SUFFIX, schema1,
};
use crate::{Error, Result};

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
        let http = super::default_transport().map_err(|source| Error::IndexTransport {
            document: LABEL,
            url: url.to_owned(),
            source: Box::new(source),
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
        let key = PublicKey::from_base64(public_key).map_err(|source| Error::IndexKey {
            source: Box::new(source),
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

        match self.refresh_root(state, held.as_ref(), floor).await {
            Ok(Refreshed::Unchanged(held) | Refreshed::New(held)) => {
                self.collect(state, held, Freshness::Fetched, true, want)
                    .await
            }
            Ok(Refreshed::NoSchema2) => self.through_schema1(state, held, floor, want).await,
            Err(refusal) => self.keep(state, held, one, refusal, want).await,
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
        refusal: Error,
        want: Want<'_>,
    ) -> Result<Catalogue<Index>> {
        match (held, one) {
            (Some(held), _) => {
                let age = age(held.stamp);
                tracing::warn!(
                    url = %self.root_url,
                    age_hours = age.as_secs() / 3600,
                    error = %refusal,
                    document = LABEL,
                    "keeping the cached document; the published one was not usable"
                );
                self.collect(state, held, Freshness::Stale { age }, true, want)
                    .await
            }
            (None, Some((stamp, index))) => {
                let age = age(stamp);
                tracing::warn!(
                    url = %self.root_url,
                    age_hours = age.as_secs() / 3600,
                    error = %refusal,
                    document = LABEL,
                    "keeping the cached index.json; the published index was not usable"
                );
                Ok(Catalogue {
                    index,
                    freshness: Freshness::Stale { age },
                })
            }
            (None, None) => Err(refusal),
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
                    self.forget_set(state);
                }
                Ok(Catalogue {
                    index: read.index.into(),
                    freshness: read.freshness,
                })
            }
            Err(refusal) => self.keep(state, held, None, refusal, want).await,
        }
    }

    /// Remove the schema 2 set from the cache and from memory.
    fn forget_set(&self, state: &mut State) {
        if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("index-v2"))
                {
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

    /// The cached root, re-verified unless the file is the one memory already describes.
    ///
    /// Every failure is [`None`] rather than an error, on [`Client`]'s reasoning: a cache that is
    /// missing, truncated, tampered with or written by a newer schema all mean "go to the network".
    /// The one worth a word in the log is a *signature* failure, because that is the only one that
    /// cannot happen by accident.
    fn cached_root(&self, state: &mut State) -> Option<HeldRoot> {
        let path = self.path(schema2::ROOT);
        let Some(now) = stamp(&path) else {
            state.root = None;
            state.kinds.clear();
            return None;
        };

        if let Some(held) = &state.root
            && held.stamp == now
        {
            return Some(held.clone());
        }

        state.root = None;
        state.kinds.clear();

        let document = std::fs::read(&path).ok()?;
        let signature =
            std::fs::read_to_string(self.path(&format!("{}{SIGNATURE_SUFFIX}", schema2::ROOT)))
                .ok()?;

        let root = match self.read_root(&document, &signature, &path.display().to_string()) {
            Ok(root) => root,
            Err(refusal @ Error::IndexSignature { .. }) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %refusal,
                    document = LABEL,
                    "the cached document is not signed by this build's key; ignoring it"
                );
                return None;
            }
            Err(_) => return None,
        };

        let held = HeldRoot {
            root: Arc::new(root),
            signature,
            stamp: now,
        };
        state.root = Some(held.clone());
        Some(held)
    }

    /// Check a signature, then read the root it covered, then check what a root must be.
    ///
    /// In that order, and the order is the point — see [`Client`]'s own note: nothing here parses
    /// a document it has not first established we wrote.
    fn read_root(&self, document: &[u8], signature: &str, from: &str) -> Result<Root> {
        super::verify(&self.key, document, signature).map_err(|source| Error::IndexSignature {
            document: LABEL,
            url: from.to_owned(),
            source: Box::new(source),
        })?;

        let root: Root =
            serde_json::from_slice(document).map_err(|source| Error::IndexUnreadable {
                document: LABEL,
                url: from.to_owned(),
                source,
            })?;

        if root.schema != schema2::SCHEMA {
            return Err(Error::IndexSchema {
                document: LABEL,
                url: from.to_owned(),
                found: root.schema,
                expected: schema2::SCHEMA,
            });
        }

        // Every artifact's URL starts with this, so it is an address or the root is unusable.
        if !(root.base_url.starts_with("https://") || root.base_url.starts_with("http://")) {
            return Err(Error::IndexUnreadable {
                document: LABEL,
                url: from.to_owned(),
                source: <serde_json::Error as serde::de::Error>::custom(format!(
                    "base_url {:?} is not an http or https URL",
                    root.base_url
                )),
            });
        }

        Ok(root)
    }

    /// Bytes that are `entry`'s, decoded.
    ///
    /// **The one door a kind file comes through**, from the network and from the cache alike:
    /// length, then hash, and only then the parser — so a file read back from disk is believed for
    /// the same reason one just downloaded is, and for no other.
    fn read_kind(
        kind: &str,
        entry: &Entry,
        base_url: &str,
        bytes: &[u8],
        from: &str,
    ) -> Result<Vec<Package>> {
        let wrong = |problem| Error::IndexKind {
            kind: kind.to_owned(),
            url: from.to_owned(),
            problem,
        };

        if bytes.len() as u64 != entry.size {
            return Err(wrong(KindProblem::Length {
                expected: entry.size,
                found: bytes.len() as u64,
            }));
        }
        if format!("{:x}", Sha256::digest(bytes)) != entry.sha256 {
            return Err(wrong(KindProblem::Hash));
        }

        let file: KindFile =
            serde_json::from_slice(bytes).map_err(|source| Error::IndexUnreadable {
                document: LABEL,
                url: from.to_owned(),
                source,
            })?;

        if file.schema != schema2::SCHEMA {
            return Err(Error::IndexSchema {
                document: LABEL,
                url: from.to_owned(),
                found: file.schema,
                expected: schema2::SCHEMA,
            });
        }
        if file.kind != kind {
            return Err(wrong(KindProblem::Named(file.kind)));
        }

        schema2::decode(file, base_url).map_err(|why| wrong(KindProblem::Undecodable(why)))
    }

    /// The cached file for `kind`, when it is the one `root` names.
    fn cached_kind(&self, state: &mut State, root: &Root, kind: &str) -> Option<Arc<[Package]>> {
        let entry = root.kinds.get(kind)?;
        let path = self.path(&schema2::kind_file(kind));
        let Some(now) = stamp(&path) else {
            state.kinds.remove(kind);
            return None;
        };

        if let Some(held) = state.kinds.get(kind)
            && held.stamp == now
            && held.sha256 == entry.sha256
        {
            return Some(Arc::clone(&held.packages));
        }

        state.kinds.remove(kind);
        let bytes = std::fs::read(&path).ok()?;
        let packages: Arc<[Package]> = Self::read_kind(
            kind,
            entry,
            &root.base_url,
            &bytes,
            &path.display().to_string(),
        )
        .ok()?
        .into();

        state.kinds.insert(
            kind.to_owned(),
            HeldKind {
                sha256: entry.sha256.clone(),
                stamp: now,
                packages: Arc::clone(&packages),
            },
        );
        Some(packages)
    }

    /// One kind file from the network, checked against `entry`.
    ///
    /// Owns everything it is given, so that several can run at once on a [`tokio::task::JoinSet`].
    async fn fetch_kind(
        http: reqwest::Client,
        url: String,
        per_file: Duration,
        kind: String,
        entry: Entry,
        base_url: String,
    ) -> Result<FetchedKind> {
        let wrong = |problem| Error::IndexKind {
            kind: kind.clone(),
            url: url.clone(),
            problem,
        };

        let got = super::get(&http, &url, per_file, entry.size, Some(entry.size), LABEL).await?;
        let bytes = match got {
            Got::Absent => return Err(wrong(KindProblem::Absent)),
            Got::Declared(found) => {
                return Err(wrong(KindProblem::Length {
                    expected: entry.size,
                    found,
                }));
            }
            Got::Body(bytes) => bytes,
        };

        let packages = Self::read_kind(&kind, &entry, &base_url, &bytes, &url)?;
        Ok((kind, bytes, packages))
    }

    /// The future that fetches `kind` as `root` names it.
    fn fetching(
        &self,
        root: &Root,
        kind: &str,
    ) -> impl Future<Output = Result<FetchedKind>> + Send + 'static {
        Self::fetch_kind(
            self.http.clone(),
            beside(&self.root_url, &schema2::kind_file(kind)),
            self.per_file,
            kind.to_owned(),
            root.kinds[kind].clone(),
            root.base_url.clone(),
        )
    }

    /// Several kind files, [`AT_ONCE`] at a time. The first failure ends the rest: a machine with
    /// no network waits for one timeout, not one per file.
    async fn fetch_kinds(&self, root: &Root, kinds: &[String]) -> Result<Vec<FetchedKind>> {
        let mut fetched = Vec::with_capacity(kinds.len());

        for batch in kinds.chunks(AT_ONCE) {
            let mut running = tokio::task::JoinSet::new();
            for kind in batch {
                running.spawn(self.fetching(root, kind));
            }

            // Returning early drops the set, which aborts what is still running.
            while let Some(joined) = running.join_next().await {
                let one = joined.map_err(|source| Error::IndexTransport {
                    document: LABEL,
                    url: self.root_url.clone(),
                    source: Box::new(source),
                })?;
                fetched.push(one?);
            }
        }

        Ok(fetched)
    }

    /// Write `bytes` beside `name` and rename over it, so a reader sees the old file or the new one.
    fn store(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let path = self.path(name);
        let part = self.path(&format!("{name}.part"));

        std::fs::write(&part, bytes).map_err(|source| Error::Io {
            action: "write",
            path: part.clone(),
            source,
        })?;
        std::fs::rename(&part, &path).map_err(|source| Error::Io {
            action: "replace",
            path,
            source,
        })
    }

    /// Steps 1 to 4 of the contract. `held` is the cached root, when there is one that verifies.
    async fn refresh_root(
        &self,
        state: &mut State,
        held: Option<&HeldRoot>,
        floor: Option<Timestamp>,
    ) -> Result<Refreshed> {
        let signature_name = format!("{}{SIGNATURE_SUFFIX}", schema2::ROOT);
        let signature_url = format!("{}{SIGNATURE_SUFFIX}", self.root_url);

        let signature = match super::get(
            &self.http,
            &signature_url,
            self.per_file,
            SIGNATURE_LIMIT,
            None,
            LABEL,
        )
        .await?
        {
            Got::Absent => return Ok(Refreshed::NoSchema2),
            Got::Declared(_) => unreachable!("no length was expected of a signature"),
            Got::Body(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        };

        // 1. **Nothing was published.** Every publish writes a new `generated_at`, so every publish
        // has a new signature. A server replaying an old one only holds a client on the root it
        // already has, which is what withholding the index does anyway.
        if let Some(held) = held
            && held.signature == signature
        {
            let path = self.path(schema2::ROOT);
            let touched = std::fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(SystemTime::now()));
            if let Err(error) = touched {
                tracing::debug!(%error, "the cached root's age could not be started again");
            }

            // The file this process just moved is still the file memory describes.
            let mut held = held.clone();
            held.stamp = stamp(&path).unwrap_or(held.stamp);
            state.root = Some(held.clone());
            return Ok(Refreshed::Unchanged(held));
        }

        // 2. The root: verified before it is parsed, and never backwards.
        let got = super::get(
            &self.http,
            &self.root_url,
            self.per_file,
            ROOT_LIMIT,
            None,
            LABEL,
        )
        .await?;
        let Got::Body(document) = got else {
            return Err(Error::IndexTransport {
                document: LABEL,
                url: self.root_url.clone(),
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "the signature is published and the root is not",
                )),
            });
        };

        let root = self.read_root(&document, &signature, &self.root_url)?;
        if let Some(floor) = floor
            && root.generated_at < floor
        {
            return Err(Error::IndexRolledBack {
                document: LABEL,
                url: self.root_url.clone(),
                cached: floor.to_string(),
                offered: root.generated_at.to_string(),
            });
        }

        // 3. Every kind already cached, brought up to the new root — or nothing is stored.
        let mut behind = Vec::new();
        let mut dropped = Vec::new();
        if let Some(held) = held {
            for (kind, old) in &held.root.kinds {
                if !schema2::is_kind_name(kind)
                    || self.cached_kind(state, &held.root, kind).is_none()
                {
                    continue;
                }

                match root.kinds.get(kind) {
                    Some(new) if new.sha256 == old.sha256 => {}
                    Some(_) => behind.push(kind.clone()),
                    None => dropped.push(kind.clone()),
                }
            }
        }
        let fetched = self.fetch_kinds(&root, &behind).await?;

        // 4. Commit: kind files, then the signature, then the root — whose modification time is
        // the age of the whole set, so it is written last.
        let committed = (|| -> Result<()> {
            for (kind, bytes, _) in &fetched {
                self.store(&schema2::kind_file(kind), bytes)?;
            }
            self.store(&signature_name, signature.as_bytes())?;
            self.store(schema2::ROOT, &document)
        })();

        // Whatever happened, memory no longer describes these files.
        state.root = None;
        state.kinds.clear();
        committed?;

        for kind in &dropped {
            tracing::warn!(
                kind,
                "the package index no longer names a kind it promised to keep"
            );
            let _ = std::fs::remove_file(self.path(&schema2::kind_file(kind)));
        }

        // One layout on disk at a time, and its mark was honoured above.
        self.schema1.forget();
        state.one = None;
        state.refused.clear();

        let held = HeldRoot {
            root: Arc::new(root),
            signature,
            stamp: stamp(&self.path(schema2::ROOT)).unwrap_or((0, SystemTime::UNIX_EPOCH)),
        };
        state.root = Some(held.clone());

        for (kind, _, packages) in fetched {
            if let Some(now) = stamp(&self.path(&schema2::kind_file(&kind))) {
                state.kinds.insert(
                    kind.clone(),
                    HeldKind {
                        sha256: held.root.kinds[&kind].sha256.clone(),
                        stamp: now,
                        packages: packages.into(),
                    },
                );
            }
        }

        Ok(Refreshed::New(held))
    }

    /// Fetch a kind nobody has asked for yet, against the cached root, and keep it.
    async fn first_use(
        &self,
        state: &mut State,
        held: &HeldRoot,
        kind: &str,
    ) -> Result<Arc<[Package]>> {
        let (_, bytes, packages) = self.fetching(&held.root, kind).await?;
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

                let tried = self.first_use(state, &held, &kind).await;

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
                if !matches!(refusal, Error::IndexTransport { .. }) {
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
