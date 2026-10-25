//! A credential store that is one file in a home — ADR 0052.
//!
//! **The development build's, and only its.** A daemon out of `target/debug` is unsigned, so the
//! macOS Keychain treats every rebuild as a program it has never seen and asks for the login
//! password before handing it anything an earlier build wrote. The file this module keeps is how a
//! build that is not a release never asks: its credentials live in its own home, beside everything
//! else ADR 0024 already keeps there. A release never constructs one — `mixengined` refuses the
//! flag that would.
//!
//! **Plain text, owner-only.** Written through [`crate::write_private`], so it is exactly as
//! protected as the home's CA private key and no more.
//!
//! **An unreadable file is an error, never an empty store.** First-run code that read "no password
//! here" would generate a new one over the one a running server already has.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use crate::{Error, Keyring, Result};

/// The only shape this build reads or writes.
const VERSION: u32 = 1;

/// The whole file.
#[derive(Serialize, Deserialize)]
struct Document {
    version: u32,

    /// `service` → `key` → `secret`. Ordered, so the file diffs sensibly when a person looks at it.
    entries: BTreeMap<String, BTreeMap<String, String>>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: VERSION,
            entries: BTreeMap::new(),
        }
    }
}

/// One home's credentials, in `path`.
#[derive(Debug)]
pub(crate) struct File {
    path: PathBuf,

    /// Held across every read-modify-write. One daemon owns a home (its lock), so this is the only
    /// writer there is to coordinate with.
    turn: Mutex<()>,
}

impl File {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            turn: Mutex::new(()),
        }
    }

    fn load(&self, action: &'static str, service: &str, key: &str) -> Result<Document> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Document::default());
            }
            Err(error) => return Err(self.failure(action, service, key, &error.to_string())),
        };

        // `serde_json`'s message names a line and a column and never quotes the input, which is
        // what keeps a torn value out of the error — `no_error_carries_a_stored_value` holds it.
        let document: Document = serde_json::from_slice(&bytes)
            .map_err(|error| self.failure(action, service, key, &error.to_string()))?;

        if document.version != VERSION {
            return Err(self.failure(
                action,
                service,
                key,
                &format!(
                    "it is version {}, and this build reads version {VERSION}",
                    document.version
                ),
            ));
        }

        Ok(document)
    }

    fn save(
        &self,
        document: &Document,
        action: &'static str,
        service: &str,
        key: &str,
    ) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(document)
            .map_err(|error| self.failure(action, service, key, &error.to_string()))?;

        crate::write_private(&self.path, &bytes)
    }

    /// A failure naming the file and the entry, and never a value.
    fn failure(&self, action: &'static str, service: &str, key: &str, why: &str) -> Error {
        Error::Secret {
            action,
            service: service.to_owned(),
            key: key.to_owned(),
            source: format!("{}: {why}", self.path.display()).into(),
        }
    }
}

impl Keyring for File {
    fn secret(&self, service: &str, key: &str) -> Result<Option<String>> {
        let _turn = self.turn.lock().unwrap_or_else(PoisonError::into_inner);
        let document = self.load("read", service, key)?;

        Ok(document
            .entries
            .get(service)
            .and_then(|entries| entries.get(key))
            .cloned())
    }

    fn set_secret(&self, service: &str, key: &str, secret: &str) -> Result<()> {
        let _turn = self.turn.lock().unwrap_or_else(PoisonError::into_inner);
        let mut document = self.load("store", service, key)?;

        document
            .entries
            .entry(service.to_owned())
            .or_default()
            .insert(key.to_owned(), secret.to_owned());

        self.save(&document, "store", service, key)
    }

    fn forget_secret(&self, service: &str, key: &str) -> Result<()> {
        let _turn = self.turn.lock().unwrap_or_else(PoisonError::into_inner);
        let mut document = self.load("forget", service, key)?;

        let Some(entries) = document.entries.get_mut(service) else {
            return Ok(());
        };
        if entries.remove(key).is_none() {
            return Ok(());
        }
        if entries.is_empty() {
            document.entries.remove(service);
        }

        self.save(&document, "forget", service, key)
    }

    fn keys(&self, service: &str) -> Result<Vec<String>> {
        let _turn = self.turn.lock().unwrap_or_else(PoisonError::into_inner);
        let document = self.load("list", service, "*")?;

        Ok(document
            .entries
            .get(service)
            .map(|entries| entries.keys().cloned().collect())
            .unwrap_or_default())
    }
}
