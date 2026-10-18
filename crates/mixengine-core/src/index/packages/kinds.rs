//! One kind's listing: read, cached, fetched alone or with the others, and stored.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest as _, Sha256};

use super::super::format::Package;
use super::super::schema2::{self, Entry, KindFile, Root};
use super::super::{Got, KindProblem};
use crate::{Error, InstallError, Result};

use super::{AT_ONCE, FetchedKind, HeldKind, LABEL, PackageIndex, State, beside, stamp};

impl PackageIndex {
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
        let wrong = |problem| {
            Error::Install(InstallError::IndexKind {
                kind: kind.to_owned(),
                url: from.to_owned(),
                problem,
            })
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

        let file: KindFile = serde_json::from_slice(bytes).map_err(|source| {
            Error::Install(InstallError::IndexUnreadable {
                document: LABEL,
                url: from.to_owned(),
                source,
            })
        })?;

        if file.schema != schema2::SCHEMA {
            return Err(Error::Install(InstallError::IndexSchema {
                document: LABEL,
                url: from.to_owned(),
                found: file.schema,
                expected: schema2::SCHEMA,
            }));
        }
        if file.kind != kind {
            return Err(wrong(KindProblem::Named(file.kind)));
        }

        schema2::decode(file, base_url).map_err(|why| wrong(KindProblem::Undecodable(why)))
    }

    /// The cached file for `kind`, when it is the one `root` names.
    pub(super) fn cached_kind(
        &self,
        state: &mut State,
        root: &Root,
        kind: &str,
    ) -> Option<Arc<[Package]>> {
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
        let wrong = |problem| {
            Error::Install(InstallError::IndexKind {
                kind: kind.clone(),
                url: url.clone(),
                problem,
            })
        };

        let got =
            super::super::get(&http, &url, per_file, entry.size, Some(entry.size), LABEL).await?;
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
    pub(super) fn fetching(
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
    pub(super) async fn fetch_kinds(
        &self,
        root: &Root,
        kinds: &[String],
    ) -> Result<Vec<FetchedKind>> {
        let mut fetched = Vec::with_capacity(kinds.len());

        for batch in kinds.chunks(AT_ONCE) {
            let mut running = tokio::task::JoinSet::new();
            for kind in batch {
                running.spawn(self.fetching(root, kind));
            }

            // Returning early drops the set, which aborts what is still running.
            while let Some(joined) = running.join_next().await {
                let one = joined.map_err(|source| {
                    Error::Install(InstallError::IndexTransport {
                        document: LABEL,
                        url: self.root_url.clone(),
                        source: Box::new(source),
                    })
                })?;
                fetched.push(one?);
            }
        }

        Ok(fetched)
    }

    /// Several kind files, [`AT_ONCE`] at a time, each with its own answer.
    ///
    /// Unlike [`Self::fetch_kinds`], a failure ends nothing: a kind that could not be read is
    /// missing from the view and does not fail the others. A kind whose task did not finish is
    /// absent from the map, and the caller fetches it again on its own.
    pub(super) async fn fetch_each(
        &self,
        root: &Root,
        kinds: &[String],
    ) -> BTreeMap<String, Result<FetchedKind>> {
        let mut fetched = BTreeMap::new();

        for batch in kinds.chunks(AT_ONCE) {
            let mut running = tokio::task::JoinSet::new();
            // A task that did not finish hands back its id and not its kind, so the kind is kept
            // beside the id to be named in the warning.
            let mut named = std::collections::HashMap::new();
            for kind in batch {
                let fetching = self.fetching(root, kind);
                let owned = kind.clone();
                let task = running.spawn(async move { (owned, fetching.await) });
                named.insert(task.id(), kind.as_str());
            }

            while let Some(joined) = running.join_next().await {
                match joined {
                    Ok((kind, one)) => {
                        fetched.insert(kind, one);
                    }
                    // Left out of the map, so the caller fetches it again on its own — said here,
                    // because nothing else would say that a task panicked or was cancelled.
                    Err(error) => tracing::warn!(
                        kind = named.get(&error.id()).copied().unwrap_or("unknown"),
                        %error,
                        "fetching a kind of the package index did not finish; trying it again alone"
                    ),
                }
            }
        }

        fetched
    }

    /// Write `bytes` beside `name` and rename over it, so a reader sees the old file or the new one.
    pub(super) fn store(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let path = self.path(name);
        let part = self.path(&format!("{name}.part"));

        // A write that fails half way takes its `.part` with it: nothing reads one, and nothing
        // else would ever remove it.
        let failed = |action, path, source| {
            let _ = std::fs::remove_file(&part);
            Error::Io {
                action,
                path,
                source,
            }
        };

        std::fs::write(&part, bytes).map_err(|source| failed("write", part.clone(), source))?;
        std::fs::rename(&part, &path).map_err(|source| failed("replace", path, source))
    }
}
