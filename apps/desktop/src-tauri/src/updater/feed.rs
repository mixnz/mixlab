//! The update feed, read and verified by MixLab — spec D2.
//!
//! The same document `mix self-update` reads, with this crate's own code. The signature is checked
//! before a byte is parsed, a document older than the one held is refused, and nothing is
//! `deny_unknown_fields`, because builds older than a feed must go on reading it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The document version this build reads.
pub const SCHEMA: u32 = 1;

/// What the signature beside a document is called: minisign's own convention.
pub const SIGNATURE_SUFFIX: &str = ".minisig";

/// The cached copy, in `<app data>/updates/`.
pub const CACHED_DOCUMENT: &str = "latest.json";
const CACHED_SIGNATURE: &str = "latest.json.minisig";

/// The format of every timestamp in the feed: `packaging/feed.sh` writes nothing else.
const TIMESTAMP: &str = "%Y-%m-%dT%H:%M:%SZ";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Feed {
    pub schema: u32,
    pub generated_at: String,
    pub version: String,
    pub published_at: String,
    pub notes: String,
    #[serde(default)]
    pub notes_url: Option<String>,
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub installers: Vec<Installer>,
}

/// One payload archive, for the in-place swap (Windows).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Artifact {
    pub os: String,
    pub arch: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub provides: BTreeMap<String, String>,
}

/// One installer, handed to the operating system (macOS, Linux).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Installer {
    pub os: String,
    pub arch: String,
    pub kind: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    /// `window` or `headless`. `None` is the window's: all a feed from before T182b listed.
    #[serde(default)]
    pub flavour: Option<String>,
}

impl Feed {
    /// The payload for this machine, spelled as `std::env::consts` spells it.
    pub fn artifact(&self, os: &str, arch: &str) -> Option<&Artifact> {
        self.artifacts.iter().find(|a| a.os == os && a.arch == arch)
    }

    /// The window's installer of `kind` for this machine. Never the headless one: MixLab only
    /// updates an install that carries the window.
    pub fn installer(&self, os: &str, arch: &str, kind: &str) -> Option<&Installer> {
        self.installers.iter().find(|i| {
            i.os == os
                && i.arch == arch
                && i.kind == kind
                && i.flavour
                    .as_deref()
                    .is_none_or(|flavour| flavour == "window")
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    /// Not signed by the key this build pins, or not a signature at all.
    Signature(String),
    /// Signed, but not a document this build can read.
    Unreadable(String),
    /// Signed and readable, with a schema this build does not know.
    Schema(u32),
    /// Older than the document already held: a stale edge or a replay.
    RolledBack { cached: String, offered: String },
    /// The network or the disk, before anything could be checked.
    Transport(String),
}

impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Signature(why) => write!(f, "the feed's signature does not verify: {why}"),
            Self::Unreadable(why) => write!(f, "the feed cannot be read: {why}"),
            Self::Schema(schema) => {
                write!(
                    f,
                    "the feed is schema {schema}, and this build reads {SCHEMA}"
                )
            }
            Self::RolledBack { cached, offered } => write!(
                f,
                "the feed offered ({offered}) is older than the one already held ({cached})"
            ),
            Self::Transport(why) => write!(f, "the feed could not be fetched: {why}"),
        }
    }
}

/// Verify `document` against `signature`, then parse it. Never parses unverified bytes.
pub fn verify(document: &[u8], signature: &str, public_key: &str) -> Result<Feed, FeedError> {
    let key = minisign_verify::PublicKey::from_base64(public_key)
        .map_err(|e| FeedError::Signature(e.to_string()))?;
    let signature = minisign_verify::Signature::decode(signature)
        .map_err(|e| FeedError::Signature(e.to_string()))?;
    key.verify(document, &signature, false)
        .map_err(|e| FeedError::Signature(e.to_string()))?;

    let feed: Feed =
        serde_json::from_slice(document).map_err(|e| FeedError::Unreadable(e.to_string()))?;
    if feed.schema != SCHEMA {
        return Err(FeedError::Schema(feed.schema));
    }
    timestamp(&feed.generated_at)?;
    Ok(feed)
}

/// Whether `offered` may replace `cached`: never when it was generated earlier.
pub fn accept(cached: Option<&Feed>, offered: &Feed) -> Result<(), FeedError> {
    let Some(cached) = cached else {
        return Ok(());
    };
    if timestamp(&offered.generated_at)? < timestamp(&cached.generated_at)? {
        return Err(FeedError::RolledBack {
            cached: cached.generated_at.clone(),
            offered: offered.generated_at.clone(),
        });
    }
    Ok(())
}

fn timestamp(text: &str) -> Result<chrono::NaiveDateTime, FeedError> {
    chrono::NaiveDateTime::parse_from_str(text, TIMESTAMP)
        .map_err(|e| FeedError::Unreadable(format!("timestamp {text:?}: {e}")))
}

/// The last verified document and its signature, re-verified on every read: a file in the user's
/// data directory is a file any local process can rewrite.
pub struct Cache {
    directory: PathBuf,
}

impl Cache {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn load(&self, public_key: &str) -> Option<Feed> {
        let document = std::fs::read(self.directory.join(CACHED_DOCUMENT)).ok()?;
        let signature = std::fs::read_to_string(self.directory.join(CACHED_SIGNATURE)).ok()?;
        verify(&document, &signature, public_key).ok()
    }

    pub fn store(&self, document: &[u8], signature: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        write_atomically(&self.directory.join(CACHED_DOCUMENT), document)?;
        write_atomically(&self.directory.join(CACHED_SIGNATURE), signature.as_bytes())
    }
}

/// Write beside the target, then rename over it, so a reader never sees half a file.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path)
}

/// What one check found: the feed to act on (fresh, or the cached one when fetching failed), and
/// why fetching failed, if it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub feed: Option<Feed>,
    pub failure: Option<String>,
}

/// Fetch, verify, refuse a rollback, cache. Falls back to the cached document on any failure.
pub async fn refresh(
    http: &reqwest::Client,
    url: &str,
    public_key: &str,
    cache: &Cache,
) -> Checked {
    let cached = cache.load(public_key);

    let fetched = async {
        let document = get(http, url).await?;
        let signature = String::from_utf8(get(http, &format!("{url}{SIGNATURE_SUFFIX}")).await?)
            .map_err(|e| FeedError::Signature(e.to_string()))?;
        let feed = verify(&document, &signature, public_key)?;
        accept(cached.as_ref(), &feed)?;
        cache
            .store(&document, &signature)
            .map_err(|e| FeedError::Transport(e.to_string()))?;
        Ok::<Feed, FeedError>(feed)
    }
    .await;

    match fetched {
        Ok(feed) => Checked {
            feed: Some(feed),
            failure: None,
        },
        Err(error) => Checked {
            feed: cached,
            failure: Some(error.to_string()),
        },
    }
}

async fn get(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, FeedError> {
    let response = http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| FeedError::Transport(e.to_string()))?;
    Ok(response
        .bytes()
        .await
        .map_err(|e| FeedError::Transport(e.to_string()))?
        .to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixengine_testkit::signing::Signer;

    const FIXTURE: &[u8] = include_bytes!("../../../../../packaging/testdata/latest.json");

    fn signed(document: &[u8]) -> (Signer, String) {
        let signer = Signer::new();
        let signature = signer.sign(document);
        (signer, signature)
    }

    #[test]
    fn the_fixture_verifies_and_parses() {
        let (signer, signature) = signed(FIXTURE);
        let feed = verify(FIXTURE, &signature, &signer.public_key()).expect("verifies");
        assert_eq!(feed.version, "9.9.9");
        assert_eq!(
            feed.artifact("windows", "x86_64").unwrap().provides.len(),
            6
        );
    }

    #[test]
    fn a_flipped_byte_is_refused_before_it_is_parsed() {
        let (signer, signature) = signed(FIXTURE);
        let mut tampered = FIXTURE.to_vec();
        tampered[20] ^= 1;
        assert!(matches!(
            verify(&tampered, &signature, &signer.public_key()),
            Err(FeedError::Signature(_))
        ));
    }

    #[test]
    fn another_schema_is_its_own_refusal() {
        let document = String::from_utf8(FIXTURE.to_vec())
            .unwrap()
            .replace("\"schema\": 1", "\"schema\": 2");
        let (signer, signature) = signed(document.as_bytes());
        assert_eq!(
            verify(document.as_bytes(), &signature, &signer.public_key()),
            Err(FeedError::Schema(2))
        );
    }

    #[test]
    fn an_older_document_does_not_replace_a_newer_one() {
        let (signer, signature) = signed(FIXTURE);
        let held = verify(FIXTURE, &signature, &signer.public_key()).unwrap();
        let mut older = held.clone();
        older.generated_at = "2026-09-30T09:00:00Z".to_owned();
        assert!(matches!(
            accept(Some(&held), &older),
            Err(FeedError::RolledBack { .. })
        ));
        assert_eq!(accept(Some(&older), &held), Ok(()));
        assert_eq!(accept(None, &older), Ok(()));
    }

    #[test]
    fn the_window_flavour_is_the_one_offered_and_a_row_without_one_is_the_windows() {
        let (signer, signature) = signed(FIXTURE);
        let feed = verify(FIXTURE, &signature, &signer.public_key()).unwrap();
        assert!(feed
            .installer("linux", "x86_64", "deb")
            .unwrap()
            .url
            .contains("mixlab_"));
        assert!(feed.installer("macos", "aarch64", "pkg").is_some());
        assert!(feed.installer("linux", "aarch64", "deb").is_none());
    }

    #[test]
    fn the_cache_reverifies_what_it_reads() {
        let directory = tempfile::tempdir().unwrap();
        let cache = Cache::new(directory.path().to_path_buf());
        let (signer, signature) = signed(FIXTURE);
        cache.store(FIXTURE, &signature).unwrap();
        assert!(cache.load(&signer.public_key()).is_some());

        std::fs::write(directory.path().join(CACHED_DOCUMENT), b"{}").unwrap();
        assert!(cache.load(&signer.public_key()).is_none());
    }

    #[test]
    fn the_compiled_in_key_is_the_committed_one() {
        let committed = include_str!("../../../../../packaging/updates.pub");
        let key = committed
            .lines()
            .nth(1)
            .expect("the key is on line two")
            .trim();
        assert_eq!(key, crate::updater::PUBLIC_KEY);
    }
}
