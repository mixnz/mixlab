//! The fetch half of an index: [`Client`], which reads a signed document over HTTP, checks its
//! signature and caches it, and the GET and signature check the package index shares with it.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use minisign_verify::{PublicKey, Signature};

use super::{Catalogue, Document, FRESH_FOR, Freshness, Timestamp};
use crate::{Error, InstallError, Result};

/// How long a fetch may take before it counts as "no network".
///
/// Every path that touches the network has one, per `docs/standards/rust.md`. This one is
/// generous because the alternative to waiting is falling back to a cache that may be missing, and
/// stingy enough that `mix runtime list` on a captive-portal wifi answers rather than hangs.
pub(crate) const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// The signature file sits beside the document, named the way minisign names it.
pub(crate) const SIGNATURE_SUFFIX: &str = ".minisig";

/// The transport [`Client::with`] builds for itself, exposed for a caller building more than one
/// [`Client`] that wants them to share it — see [`Client::with_transport`].
///
/// # Errors
///
/// Whatever `reqwest` raises for a transport it cannot build, which every platform this product
/// targets has never been observed to raise.
pub fn default_transport() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("mixengine/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Check `signature` over `document` against `key`.
///
/// **The one signature check there is**, for a single document and for the package index's root
/// alike. `false` refuses minisign's legacy algorithm: everything this project publishes is the
/// modern pre-hashed form, so accepting the other one would widen what we trust in exchange for
/// nothing. The call checks the signature over the trusted comment as well as the one over the
/// document.
pub(crate) fn verify(
    key: &PublicKey,
    document: &[u8],
    signature: &str,
) -> std::result::Result<(), minisign_verify::Error> {
    let signature = Signature::decode(signature)?;
    key.verify(document, &signature, false)
}

/// What one GET came back with.
pub(crate) enum Got {
    /// `404` or `410`: the server has no such file. Told apart from every other failure because
    /// it is the one answer that means "this source does not publish that".
    Absent,

    /// The server declared a length that is not the one expected, and the body was not read.
    Declared(u64),

    /// The body, and never more than one byte past the limit it was read under.
    Body(Vec<u8>),
}

/// GET `url` inside `timeout`, reading no more than one byte past `limit`.
///
/// `exact` is the length the body must be, when the caller knows it: a `Content-Length` that says
/// otherwise ends the request before the body. A body that runs past `limit` is cut there, and
/// what was read is then judged like any other — a cut root fails its signature and a cut kind
/// file fails its length — so a server cannot make this process hold more than it asked for.
pub(crate) async fn get(
    http: &reqwest::Client,
    url: &str,
    timeout: Duration,
    limit: u64,
    exact: Option<u64>,
    document: &'static str,
) -> Result<Got> {
    let transport = |source: reqwest::Error| {
        Error::Install(InstallError::IndexTransport {
            document,
            url: url.to_owned(),
            source: Box::new(source),
        })
    };

    let response = http
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(transport)?;

    if matches!(
        response.status(),
        reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::GONE
    ) {
        return Ok(Got::Absent);
    }

    let mut response = response.error_for_status().map_err(transport)?;

    if let (Some(exact), Some(declared)) = (exact, response.content_length())
        && declared != exact
    {
        return Ok(Got::Declared(declared));
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        let room =
            usize::try_from((limit + 1).saturating_sub(body.len() as u64)).unwrap_or(usize::MAX);
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);

        if body.len() as u64 > limit {
            break;
        }
    }

    Ok(Got::Body(body))
}

/// Reads a signed document, caching it and refusing to be walked backwards.
#[derive(Debug)]
pub struct Client<D> {
    url: String,
    key: PublicKey,
    cache_file: PathBuf,
    http: reqwest::Client,
    document: std::marker::PhantomData<fn() -> D>,
}

impl<D: Document> Client<D> {
    /// Point a client at a signed document at `url`, caching under `cache_dir`.
    ///
    /// This is what `MIXENGINE_INDEX_URL` and a team mirror use, and what `MockRegistry` uses in
    /// tests — the key has to be injectable because a test cannot hold the production private key,
    /// and a verification path that is switched off for tests is a verification path nothing checks.
    ///
    /// # Errors
    ///
    /// A transport that cannot be built, or a public key that is not one.
    pub fn with(url: &str, public_key: &str, cache_dir: &Path) -> Result<Self> {
        let http = default_transport().map_err(|source| {
            Error::Install(InstallError::IndexTransport {
                document: D::LABEL,
                url: url.to_owned(),
                source: Box::new(source),
            })
        })?;

        Self::with_transport(url, public_key, cache_dir, http)
    }

    /// The same, against a `reqwest::Client` the caller already built.
    ///
    /// **For a daemon that reads more than one signed document.** `Fetcher`'s own comment already
    /// argues this for the index client and the installer that shares its cache directory: built
    /// once, handed to everything that needs it, rather than once per namespace. A `reqwest::Client`
    /// is the same shape of thing — cheap to clone, meant to be reused across every host a process
    /// talks to — so a daemon reading the package index, the extension registry and the update feed
    /// builds one transport and hands a clone to each, instead of three independent TLS
    /// configurations and connection pools that will only ever hold one host's connections apiece.
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
            url: url.to_owned(),
            key,
            cache_file: cache_dir.join(D::CACHE_FILE),
            http,
            document: std::marker::PhantomData,
        })
    }

    /// The index, from the cache if it is fresh and from the network otherwise.
    ///
    /// # Errors
    ///
    /// As [`Client::refresh`].
    pub async fn catalogue(&self) -> Result<Catalogue<D>> {
        if let Some((index, age)) = self.cached()
            && age < FRESH_FOR
        {
            return Ok(Catalogue {
                index,
                freshness: Freshness::Cached { age },
            });
        }

        self.refresh().await
    }

    /// The index from the network, whatever the age of what is cached.
    ///
    /// **[`catalogue`](Client::catalogue) without the cache shortcut**, and public because a caller
    /// whose *own* clock is the policy has no use for a second one. The update feed's 24 h check is
    /// that caller, and so is `mix self-update --check`, which
    /// `docs/features/updates.md` says forces an immediate check.
    ///
    /// Failing still falls back to the cached document, which is what makes this safe to put on a
    /// clock: a machine that goes offline keeps the last document it verified rather than losing it
    /// every time the timer fires.
    ///
    /// # Errors
    ///
    /// Only ever the reason the *last* usable index could not be obtained, because anything that
    /// goes wrong while there is a cached index falls back to it: [`InstallError::IndexTransport`]
    /// when the server cannot be reached, [`InstallError::IndexSignature`] when what it served is
    /// not ours, [`InstallError::IndexUnreadable`] or [`InstallError::IndexSchema`] when it is ours
    /// and unusable, [`InstallError::IndexRolledBack`] when it is older than what is already held,
    /// and [`Error::Io`] when the cache cannot be written.
    pub async fn refresh(&self) -> Result<Catalogue<D>> {
        self.refresh_not_before(None).await
    }

    /// [`refresh`](Client::refresh), refusing as well anything generated before `floor`.
    ///
    /// For a document published in more than one encoding. The package index is, and the mark one
    /// encoding left must hold for the other, or a home could be walked backwards by being moved
    /// between them.
    ///
    /// # Errors
    ///
    /// As [`Client::refresh`].
    pub(crate) async fn refresh_not_before(
        &self,
        floor: Option<Timestamp>,
    ) -> Result<Catalogue<D>> {
        let cached = self.cached();

        let refreshed = match self.fetch().await {
            Ok((document, signature, index)) => self
                .accept(cached.as_ref().map(|(index, _)| index), floor, &index)
                .and_then(|()| self.store(&document, &signature))
                .map(|()| index),
            Err(unreachable) => Err(unreachable),
        };

        // **One fallback for every way of failing to get a new index**, and that is the design
        // rather than a shortcut. An unreachable server, a signature that does not verify, a
        // document offered from before the one we hold, a cache directory that has gone read-only:
        // they differ in what a person should do about them, and not at all in what this call can
        // do next. The last index we verified is still the last index we verified, so it is
        // returned and the reason is said out loud.
        match (refreshed, cached) {
            (Ok(index), _) => Ok(Catalogue {
                index,
                freshness: Freshness::Fetched,
            }),
            (Err(refusal), Some((index, age))) => {
                tracing::warn!(
                    url = %self.url,
                    age_hours = age.as_secs() / 3600,
                    error = %refusal,
                    document = D::LABEL,
                    "keeping the cached document; the published one was not usable"
                );
                Ok(Catalogue {
                    index,
                    freshness: Freshness::Stale { age },
                })
            }
            (Err(refusal), None) => Err(refusal),
        }
    }

    /// Whether a newly fetched index may replace what is cached.
    ///
    /// **A correctly signed index can still be the wrong one.** Every version we ever published was
    /// signed by us, so an older one serves perfectly against the same key — a stale CDN edge, or
    /// somebody replaying a copy from before a security release was added. The signature cannot tell
    /// them apart; `generated_at` can, and refusing to move backwards is one comparison.
    ///
    /// Done now rather than later on purpose: adding it to a fleet that already has caches means
    /// deciding what to do about the ones already holding a newer document than the server has.
    fn accept(&self, cached: Option<&D>, floor: Option<Timestamp>, offered: &D) -> Result<()> {
        let held = [cached.map(Document::generated_at), floor]
            .into_iter()
            .flatten()
            .max();
        let Some(held) = held else {
            return Ok(());
        };
        if offered.generated_at() < held {
            return Err(Error::Install(InstallError::IndexRolledBack {
                document: D::LABEL,
                url: self.url.clone(),
                cached: held.to_string(),
                offered: offered.generated_at().to_string(),
            }));
        }
        Ok(())
    }

    /// Fetch the document and its signature, and verify them.
    async fn fetch(&self) -> Result<(Vec<u8>, String, D)> {
        let transport = |source: reqwest::Error| {
            Error::Install(InstallError::IndexTransport {
                document: D::LABEL,
                url: self.url.clone(),
                source: Box::new(source),
            })
        };

        let document = self
            .http
            .get(&self.url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(transport)?
            .bytes()
            .await
            .map_err(transport)?;

        let signature_url = format!("{}{SIGNATURE_SUFFIX}", self.url);
        let signature = self
            .http
            .get(&signature_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|source| {
                Error::Install(InstallError::IndexTransport {
                    document: D::LABEL,
                    url: signature_url,
                    source: Box::new(source),
                })
            })?
            .text()
            .await
            .map_err(transport)?;

        let index = self.verified(&document, &signature)?;
        Ok((document.to_vec(), signature, index))
    }

    /// Check a signature, then read what it covered.
    ///
    /// In that order, and the order is the point: nothing here parses a document it has not first
    /// established we wrote. A JSON parser is a far larger attack surface than an Ed25519 check, and
    /// running it on unverified bytes would hand that surface to whoever answers the URL.
    ///
    /// # Errors
    ///
    /// [`InstallError::IndexSignature`] when the signature does not verify against the compiled-in
    /// key, [`InstallError::IndexUnreadable`] when what it covered is not JSON we understand, and
    /// [`InstallError::IndexSchema`] when it is a document version this build cannot read.
    fn verified(&self, document: &[u8], signature: &str) -> Result<D> {
        verify(&self.key, document, signature).map_err(|source| {
            Error::Install(InstallError::IndexSignature {
                document: D::LABEL,
                url: self.url.clone(),
                source: Box::new(source),
            })
        })?;

        let index: D = serde_json::from_slice(document).map_err(|source| {
            Error::Install(InstallError::IndexUnreadable {
                document: D::LABEL,
                url: self.url.clone(),
                source,
            })
        })?;

        if index.schema() != D::SCHEMA {
            return Err(Error::Install(InstallError::IndexSchema {
                document: D::LABEL,
                url: self.url.clone(),
                found: index.schema(),
                expected: D::SCHEMA,
            }));
        }
        Ok(index)
    }

    /// The cached index and its age, or [`None`] if there is not a usable one.
    ///
    /// Every failure here is [`None`] rather than an error: a cache that is missing, unreadable,
    /// truncated, tampered with or written by a newer schema all mean the same thing to the caller,
    /// which is "go to the network". The one that is worth a word in the log is a *signature*
    /// failure, because that is the only one that cannot happen by accident.
    pub(crate) fn cached(&self) -> Option<(D, Duration)> {
        let document = std::fs::read(&self.cache_file).ok()?;
        let signature = std::fs::read_to_string(self.signature_file()).ok()?;

        let index = match self.verified(&document, &signature) {
            Ok(index) => index,
            Err(refusal @ Error::Install(InstallError::IndexSignature { .. })) => {
                tracing::warn!(
                    path = %self.cache_file.display(),
                    error = %refusal,
                    document = D::LABEL,
                    "the cached document is not signed by this build's key; ignoring it"
                );
                return None;
            }
            Err(_) => return None,
        };

        let modified = std::fs::metadata(&self.cache_file).ok()?.modified().ok()?;
        // A cache stamped in the future is a clock that moved, not a fresh file. Treating it as
        // age zero would pin a machine to a stale index until its clock caught up, so it is read as
        // "as old as possible" and the network decides.
        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::MAX);

        Some((index, age))
    }

    /// Write both halves, document last.
    ///
    /// The order matters for the same reason the age is read off the document: a reader takes the
    /// document's mtime as the fetch time and needs the signature already beside it. Writing the
    /// document first would leave a window where a concurrent reader sees a new document against an
    /// old signature and concludes the cache is tampered with.
    fn store(&self, document: &[u8], signature: &str) -> Result<()> {
        let write = |path: PathBuf, bytes: &[u8]| {
            std::fs::write(&path, bytes).map_err(|source| Error::Io {
                action: "write",
                path,
                source,
            })
        };
        write(self.signature_file(), signature.as_bytes())?;
        write(self.cache_file.clone(), document)
    }

    /// Remove the cached document and its signature, where there is one.
    ///
    /// For a document published in more than one encoding, when the other one has just been
    /// stored: one layout on disk at a time.
    pub(crate) fn forget(&self) {
        let _ = std::fs::remove_file(&self.cache_file);
        let _ = std::fs::remove_file(self.signature_file());
    }

    fn signature_file(&self) -> PathBuf {
        let mut name = self.cache_file.clone().into_os_string();
        name.push(SIGNATURE_SUFFIX);
        PathBuf::from(name)
    }
}
