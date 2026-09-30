//! The package index as callers ask for it: these kinds, from wherever is cheapest — roadmap task
//! **T196**.

use std::path::Path;

use super::{Catalogue, Client, Index, schema1};
use crate::Result;

/// Reads the package index, a kind at a time.
#[derive(Debug)]
pub struct PackageIndex {
    schema1: Client<schema1::Document>,
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
    /// # Errors
    ///
    /// A transport that cannot be built, or a public key that is not one.
    pub fn with(url: &str, public_key: &str, cache_dir: &Path) -> Result<Self> {
        Ok(Self {
            schema1: Client::with(url, public_key, cache_dir)?,
        })
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
        Ok(Self {
            schema1: Client::with_transport(url, public_key, cache_dir, http)?,
        })
    }

    /// These kinds, from the cache while it is fresh and from the network otherwise.
    ///
    /// # Errors
    ///
    /// Only when no index can be obtained at all — as [`Client::refresh`].
    pub async fn kinds(&self, _kinds: &[&str]) -> Result<Catalogue<Index>> {
        self.schema1.catalogue().await.map(whole)
    }

    /// The same, asking the network whatever the age of what is cached.
    ///
    /// # Errors
    ///
    /// As [`PackageIndex::kinds`].
    pub async fn refresh(&self, _kinds: &[&str]) -> Result<Catalogue<Index>> {
        self.schema1.refresh().await.map(whole)
    }

    /// Every kind the index names.
    ///
    /// # Errors
    ///
    /// As [`PackageIndex::kinds`].
    pub async fn all(&self) -> Result<Catalogue<Index>> {
        self.schema1.catalogue().await.map(whole)
    }
}

/// A whole schema 1 document, as the view every caller holds.
fn whole(read: Catalogue<schema1::Document>) -> Catalogue<Index> {
    Catalogue {
        index: read.index.into(),
        freshness: read.freshness,
    }
}
