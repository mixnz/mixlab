//! The index's root: read from the cache, read from a response, and refreshed from the network.

use std::sync::Arc;
use std::time::SystemTime;

use super::super::format::Timestamp;
use super::super::schema2::{self, Root};
use super::super::{Got, SIGNATURE_SUFFIX};
use crate::{Error, InstallError, Result};

use super::{
    HeldKind, HeldRoot, LABEL, PackageIndex, ROOT_LIMIT, Refreshed, SIGNATURE_LIMIT, State, stamp,
};

impl PackageIndex {
    /// The cached root, re-verified unless the file is the one memory already describes.
    ///
    /// Every failure is [`None`] rather than an error, on [`Client`]'s reasoning: a cache that is
    /// missing, truncated, tampered with or written by a newer schema all mean "go to the network".
    /// The one worth a word in the log is a *signature* failure, because that is the only one that
    /// cannot happen by accident.
    ///
    /// [`Client`]: super::Client
    pub(super) fn cached_root(&self, state: &mut State) -> Option<HeldRoot> {
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
            Err(refusal @ Error::Install(InstallError::IndexSignature { .. })) => {
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
    ///
    /// [`Client`]: super::Client
    fn read_root(&self, document: &[u8], signature: &str, from: &str) -> Result<Root> {
        super::super::verify(&self.key, document, signature).map_err(|source| {
            Error::Install(InstallError::IndexSignature {
                document: LABEL,
                url: from.to_owned(),
                source: Box::new(source),
            })
        })?;

        let root: Root = serde_json::from_slice(document).map_err(|source| {
            Error::Install(InstallError::IndexUnreadable {
                document: LABEL,
                url: from.to_owned(),
                source,
            })
        })?;

        if root.schema != schema2::SCHEMA {
            return Err(Error::Install(InstallError::IndexSchema {
                document: LABEL,
                url: from.to_owned(),
                found: root.schema,
                expected: schema2::SCHEMA,
            }));
        }

        // Every artifact's URL starts with this, so it is an address or the root is unusable.
        if !(root.base_url.starts_with("https://") || root.base_url.starts_with("http://")) {
            return Err(Error::Install(InstallError::IndexUnreadable {
                document: LABEL,
                url: from.to_owned(),
                source: <serde_json::Error as serde::de::Error>::custom(format!(
                    "base_url {:?} is not an http or https URL",
                    root.base_url
                )),
            }));
        }

        Ok(root)
    }

    /// Steps 1 to 4 of the contract. `held` is the cached root, when there is one that verifies.
    pub(super) async fn refresh_root(
        &self,
        state: &mut State,
        held: Option<&HeldRoot>,
        floor: Option<Timestamp>,
    ) -> Result<Refreshed> {
        let signature_name = format!("{}{SIGNATURE_SUFFIX}", schema2::ROOT);
        let signature_url = format!("{}{SIGNATURE_SUFFIX}", self.root_url);

        let signature = match super::super::get(
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
        let got = super::super::get(
            &self.http,
            &self.root_url,
            self.per_file,
            ROOT_LIMIT,
            None,
            LABEL,
        )
        .await?;
        let Got::Body(document) = got else {
            return Err(Error::Install(InstallError::IndexTransport {
                document: LABEL,
                url: self.root_url.clone(),
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "the signature is published and the root is not",
                )),
            }));
        };

        let root = self.read_root(&document, &signature, &self.root_url)?;
        if let Some(floor) = floor
            && root.generated_at < floor
        {
            return Err(Error::Install(InstallError::IndexRolledBack {
                document: LABEL,
                url: self.root_url.clone(),
                cached: floor.to_string(),
                offered: root.generated_at.to_string(),
            }));
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
}
