//! Where a saved connection's passwords live: the operating system's own credential store —
//! Windows Credential Manager, the macOS Keychain, the Secret Service on Linux.
//!
//! Everything else about a saved connection (host, port, user, the sidebar width) stays in
//! `connections.json`, which is plain text by design: it is a list of what you connect to, and it
//! is useful to be able to read and copy it. What must not be in there is the credentials, which
//! is what this module keeps out of it.
//!
//! On macOS all of them share one entry — the vault — rather than keeping one each. The Keychain
//! asks before it hands an item to an application it does not recognise, and it decides what it
//! recognises from the application's code signature; MixLab is not signed, so every update is a
//! stranger to it. The question is asked once per *item*, which with an entry each meant one
//! dialog per saved connection: ten of them at once on the first look at the Database tab. All of
//! them in a single item is one dialog, and the read is cached for the run, so it is one dialog
//! per update rather than one per launch.
//!
//! Windows and Linux keep an entry each, because neither has the problem and Windows would be hurt
//! by the fix: Credential Manager refuses a secret over 2560 bytes, which a dozen connections in
//! one JSON object would pass. It never asks the question at all, and the Secret Service asks to
//! unlock the collection rather than for each item, so on both of them one entry per connection
//! costs nothing and stays well inside what the store will hold.
//!
//! **On macOS one launch is one dialog, never two.** Anything else this application keeps in the
//! credential store — sync's `sync-master-key` — goes into the vault too, rather than into an item
//! beside it: two items read at start are two password prompts on every update.
//!
//! On macOS, an entry written before the vault is moved into it the first time that connection is
//! read, and the old entry removed. There is no way to spare the user the dialogs on that one run:
//! those passwords are sitting in ten separately guarded items, and reading them is exactly what
//! the guard is asking about.

use crate::error::AppError;
use crate::platform::in_background;
use keyring::Entry;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use zeroize::Zeroize;

/// Stands in for a secret wherever a `Debug` line would otherwise print one.
///
/// Prints as `"***"`, and through `Option` as `Some("***")` or `None` — so a redacted line still
/// says whether there was a password at all, which is nearly always the actual question.
///
/// Holds nothing on purpose: a type that carried the secret in order to hide it would only be one
/// stray `.0` away from printing it.
pub struct Redacted;

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("\"***\"")
    }
}

/// The name this application's entries appear under in the OS credential store.
const SERVICE: &str = "MixLab";

/// The name they appeared under while this application was the standalone client.
///
/// Read once, by the import on the first launch after the rename (`crate::import`), and never
/// written to or deleted from: a standalone client may still be installed and still be in use, and
/// those entries are its own. See the T104 design, D4.
pub const LEGACY_SERVICE: &str = "MixDB";

/// The account the vault is stored under. Inside it, a key is a connection id — a uuid — or
/// `sync-master-key` (`crate::sync::saved`), so nothing can collide. Every other account name in
/// the service is a leftover from before the vault.
const VAULT: &str = "vault";

/// The secrets of one saved connection, keyed by the field they belong to (`password`, `uri`,
/// `sshPassword`, `sshPassphrase`). The frontend decides what goes in; this side only carries it.
pub type Secrets = HashMap<String, String>;

/// Every saved connection's secrets, by connection id — databases, REST requests and terminal
/// hosts alike, which is safe because all three take their ids from the same uuid generator.
type Vault = HashMap<String, Secrets>;

/// The credential store, narrowed to the three things this module asks of it.
///
/// It is a trait so that the vault — the migration, the caching, the counting of how many times
/// the store is actually touched — can be tested against a `HashMap`. The real store is the
/// machine's own, and a test that used it would write to the developer's keychain and, on macOS,
/// raise the very dialogs this module exists to avoid.
trait Store {
    /// The value under `account`, or `None` when there is nothing there.
    fn read(&self, account: &str) -> Result<Option<String>, AppError>;
    fn write(&self, account: &str, value: &str) -> Result<(), AppError>;
    /// Removes `account`. Removing what is not there is not a failure.
    fn forget(&self, account: &str) -> Result<(), AppError>;
}

/// The credential store of the machine this is running on, under one service name.
///
/// The name is a field rather than the constant it used to be because two of them are addressed
/// from here: this application's own and — read-only — the one the standalone client used. MixEngine's is read
/// through `mixengine_platform` instead (`read_mixengine_entry`, T186).
struct OsStore {
    service: &'static str,
}

impl OsStore {
    const fn new(service: &'static str) -> Self {
        Self { service }
    }

    fn entry(&self, account: &str) -> Result<Entry, AppError> {
        Entry::new(self.service, account)
            .map_err(|e| err!("error.credentialStoreUnreachable", message = e))
    }
}

impl Store for OsStore {
    fn read(&self, account: &str) -> Result<Option<String>, AppError> {
        match self.entry(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(err!("error.cannotReadPassword", message = e)),
        }
    }

    fn write(&self, account: &str, value: &str) -> Result<(), AppError> {
        self.entry(account)?
            .set_password(value)
            .map_err(|e| err!("error.cannotSavePassword", message = e))
    }

    fn forget(&self, account: &str) -> Result<(), AppError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(err!("error.cannotRemovePassword", message = e)),
        }
    }
}

/// A credential store, and the vault held over it where the platform calls for one.
///
/// The vault is read once and held for the rest of the run, which is what turns ten connections
/// opening at once into one visit to the credential store. Behind a `Mutex` because those ten
/// arrive on ten threads of the blocking pool: the first to reach it does the reading and the
/// other nine wait for it, rather than each raising a dialog of its own. Where `vaulted` is false
/// the `Mutex` is never touched: nothing is cached, and each connection is read from and written
/// to an entry of its own.
struct Keeper<S: Store> {
    store: S,
    /// Whether every connection shares one entry. True on macOS and nowhere else — see the module
    /// documentation for both halves of why.
    vaulted: bool,
    vault: Mutex<Option<Vault>>,
}

impl<S: Store> Keeper<S> {
    fn new(store: S) -> Self {
        Self::with_vault(store, cfg!(target_os = "macos"))
    }

    fn with_vault(store: S, vaulted: bool) -> Self {
        Self {
            store,
            vaulted,
            vault: Mutex::new(None),
        }
    }

    /// The vault, read from the store on the first call and held after that.
    ///
    /// A failed read leaves the cache empty rather than filling it with an empty vault, so the
    /// next call tries again: a credential store that was locked or busy for one call is worth
    /// asking twice, and the alternative is a run that quietly believes there are no passwords.
    fn open(&self) -> Result<MutexGuard<'_, Option<Vault>>, AppError> {
        // A panic elsewhere while the vault was open says nothing about the vault itself: it is a
        // plain map, and whatever panicked had either written it or not.
        let mut guard = self.vault.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            let vault = match self.store.read(VAULT)? {
                Some(json) => serde_json::from_str(&json)
                    .map_err(|e| err!("error.cannotReadPassword", message = e))?,
                None => Vault::new(),
            };
            guard.replace(vault);
        }
        Ok(guard)
    }

    /// Puts the vault back in the store. An empty one is deleted rather than stored as `{}` — an
    /// app with nothing to hide should leave nothing behind.
    ///
    /// Both callers change a *copy* of the vault, flush that, and only then put it in the cache.
    /// A failed write is then a change that did not happen anywhere: the store holds what it held,
    /// the cache agrees with it, and nothing that was about to be deleted has been. The
    /// alternative — writing to the cache first — leaves a run believing a password was saved that
    /// was not, and telling the user so until the app is next started.
    fn flush(&self, vault: &Vault) -> Result<(), AppError> {
        if vault.is_empty() {
            return self.store.forget(VAULT);
        }
        let json = serde_json::to_string(vault)
            .map_err(|e| err!("error.cannotSavePassword", message = e))?;
        self.store.write(VAULT, &json)
    }

    /// One connection's own entry, which is where every platform kept them before the vault and
    /// where Windows and Linux keep them still.
    fn read_entry(&self, id: &str) -> Result<Secrets, AppError> {
        match self.store.read(id)? {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| err!("error.cannotReadPassword", message = e)),
            None => Ok(Secrets::new()),
        }
    }

    /// The connection's secrets, or an empty set when it has none.
    ///
    /// Under the vault, a connection whose secrets are still in an entry of their own is moved
    /// across here and its old entry removed. That removal is allowed to fail without failing the
    /// read: on macOS it is a second guarded operation on the same item, so a user who answered
    /// the first dialog with a plain *Allow* is asked again, and a *Deny* there must not take the
    /// connection list down with it. What is left behind then is a stale duplicate — the vault is
    /// what MixLab reads and writes from that point on — and it goes when the connection is next
    /// saved.
    fn load(&self, id: &str) -> Result<Secrets, AppError> {
        if !self.vaulted {
            return self.read_entry(id);
        }
        let mut guard = self.open()?;
        let vault = guard.get_or_insert_with(Vault::new);
        if let Some(secrets) = vault.get(id) {
            return Ok(secrets.clone());
        }
        let Some(json) = self.store.read(id)? else {
            return Ok(Secrets::new());
        };
        let secrets: Secrets = serde_json::from_str(&json)
            .map_err(|e| err!("error.cannotReadPassword", message = e))?;
        let mut moved = vault.clone();
        moved.insert(id.to_string(), secrets.clone());
        self.flush(&moved)?;
        *vault = moved;
        let _ = self.store.forget(id);
        Ok(secrets)
    }

    /// Writes the connection's secrets, replacing whatever was there. An empty set removes the
    /// connection rather than storing an empty object — a connection with nothing to hide should
    /// leave nothing behind.
    ///
    /// Under the vault this clears the pre-vault entry for this id as well. Nearly always there is
    /// none and this costs a lookup that finds nothing; when there is one, this is what stops an
    /// old copy of a password outliving the password itself.
    ///
    /// A save that would leave the vault as it is writes nothing. The pre-vault entry is still
    /// cleared: that lookup is not a write, and a leftover is exactly what it is there to remove.
    fn save(&self, id: &str, secrets: &Secrets) -> Result<(), AppError> {
        if !self.vaulted {
            if secrets.is_empty() {
                return self.store.forget(id);
            }
            let json = serde_json::to_string(secrets)
                .map_err(|e| err!("error.cannotSavePassword", message = e))?;
            return self.store.write(id, &json);
        }
        let mut guard = self.open()?;
        let vault = guard.get_or_insert_with(Vault::new);
        let mut next = vault.clone();
        if secrets.is_empty() {
            next.remove(id);
        } else {
            next.insert(id.to_string(), secrets.clone());
        }
        // The same vault is not written again: on macOS a write moves the item's modification
        // date and may narrow its trust to this build, for a change nobody made.
        if next != *vault {
            self.flush(&next)?;
            *vault = next;
        }
        self.store.forget(id)
    }

    /// Forgets everything stored for the connection, for when the connection itself is deleted.
    fn delete(&self, id: &str) -> Result<(), AppError> {
        self.save(id, &Secrets::new())
    }

    /// One entry of this application's own that is not a connection's — sync's `sync-master-key`.
    ///
    /// Under the vault it is held **inside** it, under its account name, as `{"value": …}`: a
    /// second item is a second dialog on every launch of a build macOS does not recognise, and a
    /// person must never be asked twice for one start. An item kept beside the vault by an earlier
    /// build is not read: sync had not shipped, so there is nothing of anybody's to carry across.
    fn read_own(&self, account: &str) -> Result<Option<String>, AppError> {
        if !self.vaulted {
            return self.store.read(account);
        }
        let mut guard = self.open()?;
        let vault = guard.get_or_insert_with(Vault::new);
        Ok(vault
            .get(account)
            .and_then(|kept| kept.get(OWN_FIELD))
            .cloned())
    }

    fn write_own(&self, account: &str, value: &str) -> Result<(), AppError> {
        if !self.vaulted {
            return self.store.write(account, value);
        }
        let mut own = Secrets::new();
        own.insert(OWN_FIELD.to_string(), value.to_string());
        self.replace_own(account, Some(own))
    }

    fn forget_own(&self, account: &str) -> Result<(), AppError> {
        if !self.vaulted {
            return self.store.forget(account);
        }
        self.replace_own(account, None)
    }

    /// Puts `own` in the vault under `account`, or takes it out, and scrubs whichever copy of the
    /// vault is no longer wanted — a sign-out has to leave nothing of the master key in memory.
    fn replace_own(&self, account: &str, own: Option<Secrets>) -> Result<(), AppError> {
        let mut guard = self.open()?;
        let vault = guard.get_or_insert_with(Vault::new);
        let mut next = vault.clone();
        let replaced = match own {
            Some(own) => next.insert(account.to_string(), own),
            None => next.remove(account),
        };
        if let Some(replaced) = replaced {
            scrub(replaced);
        }
        if next == *vault {
            scrub_vault(next);
            return Ok(());
        }
        match self.flush(&next) {
            Ok(()) => {
                scrub_vault(std::mem::replace(vault, next));
                Ok(())
            }
            Err(e) => {
                scrub_vault(next);
                Err(e)
            }
        }
    }
}

/// The one field an own entry has inside the vault.
const OWN_FIELD: &str = "value";

/// Overwrites every secret in `secrets` before it is dropped.
fn scrub(mut secrets: Secrets) {
    secrets.values_mut().for_each(Zeroize::zeroize);
}

/// The same, for a whole copy of the vault.
fn scrub_vault(vault: Vault) {
    vault.into_values().for_each(scrub);
}

fn keeper() -> &'static Keeper<OsStore> {
    static KEEPER: OnceLock<Keeper<OsStore>> = OnceLock::new();
    KEEPER.get_or_init(|| Keeper::new(OsStore::new(SERVICE)))
}

/// Writes a saved connection's secrets, replacing whatever was there. An empty set deletes them.
pub fn save(id: &str, secrets: &Secrets) -> Result<(), AppError> {
    keeper().save(id, secrets)
}

/// A saved connection's secrets, or an empty set when it has none — which is also what a
/// connection whose entry the user deleted from the OS store looks like.
pub fn load(id: &str) -> Result<Secrets, AppError> {
    keeper().load(id)
}

/// Forgets everything stored for a saved connection.
pub fn delete(id: &str) -> Result<(), AppError> {
    keeper().delete(id)
}

/// One entry of this application's own under `account` — sync's `sync-master-key` (the design's
/// D2). On macOS it lives inside the vault, so a launch is one Keychain dialog and never two; see
/// `Keeper::read_own`. Elsewhere it is an entry of its own, as a connection's is.
pub fn read_own(account: &str) -> Result<Option<String>, AppError> {
    keeper().read_own(account)
}

pub fn write_own(account: &str, value: &str) -> Result<(), AppError> {
    keeper().write_own(account, value)
}

pub fn forget_own(account: &str) -> Result<(), AppError> {
    keeper().forget_own(account)
}

/// Writes a saved connection's secrets to the OS credential store, replacing what was there.
#[tauri::command]
pub async fn secrets_save(id: String, secrets: Secrets) -> Result<(), AppError> {
    in_background(move || save(&id, &secrets)).await
}

/// A saved connection's secrets, or nothing when it has none stored.
#[tauri::command]
pub async fn secrets_load(id: String) -> Result<Secrets, AppError> {
    in_background(move || load(&id)).await
}

/// Forgets a saved connection's secrets, for when the connection itself is deleted.
#[tauri::command]
pub async fn secrets_delete(id: String) -> Result<(), AppError> {
    in_background(move || delete(&id)).await
}

/// Reads one of MixEngine's own keyring entries by the key half of its address.
///
/// **Through `mixengine_platform`, not through `OsStore`** — T186. On macOS the daemon keeps a
/// home's credentials in one Keychain item, and the platform crate is what knows how to find an
/// entry inside it. Bypasses `Keeper` entirely: that vault is this app's own. The service is
/// `mixengine_platform::KEYRING_SERVICE`, a compile-time constant on this side too and never taken
/// from a caller: see the module doc of `modules/db/handoff.rs` for why it must not travel on the
/// wire. `SavedConnection.keyringRef` on the frontend is only ever the key half of the address.
///
/// One `Host` for the run, so the platform's cache of the daemon's vault is too.
///
/// `Ok(None)` for an entry that is not there. MixEngine removes an entry when whatever owned it
/// is gone, and a reference outliving its credential is that account's normal end, not a failure —
/// the caller shows the same empty-password form a connection that was never saved would.
fn read_mixengine_entry(key: &str) -> Result<Option<String>, AppError> {
    static HOST: OnceLock<std::sync::Arc<dyn mixengine_platform::Host>> = OnceLock::new();

    HOST.get_or_init(mixengine_platform::host)
        .keyring()
        .secret(mixengine_platform::KEYRING_SERVICE, key)
        .map_err(|e| err!("error.cannotReadPassword", message = e))
}

/// Every account's secrets as the standalone client left them, and the accounts that could not be read.
///
/// Reads and nothing else — no write, no delete, no move into a vault. The standalone client may still be
/// installed and in use, and its entries are its own (T104, D5).
///
/// Two shapes, because the standalone client wrote two: one entry per account on Windows and Linux, and on macOS a
/// single `vault` account holding every account's secrets in one object — with a per-account entry
/// still possible beside it for a connection saved before the vault existed. Generic over the
/// store so both are tested against a `HashMap` rather than against the developer's keychain.
fn read_all<S: Store>(
    store: &S,
    vaulted: bool,
    accounts: &[String],
) -> (Vec<(String, Secrets)>, Vec<String>) {
    let vault: Vault = if vaulted {
        match store.read(VAULT) {
            Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_else(|e| {
                log::warn!("import: {LEGACY_SERVICE}'s vault could not be parsed: {e}");
                Vault::new()
            }),
            Ok(None) => Vault::new(),
            Err(e) => {
                log::warn!("import: {LEGACY_SERVICE}'s vault could not be read: {e:?}");
                Vault::new()
            }
        }
    } else {
        Vault::new()
    };

    let mut found = Vec::new();
    let mut failed = Vec::new();
    for account in accounts {
        if let Some(secrets) = vault.get(account) {
            if !secrets.is_empty() {
                found.push((account.clone(), secrets.clone()));
            }
            continue;
        }
        match store.read(account) {
            Ok(Some(json)) => match serde_json::from_str::<Secrets>(&json) {
                Ok(secrets) if !secrets.is_empty() => found.push((account.clone(), secrets)),
                Ok(_) => {}
                Err(e) => {
                    log::warn!(
                        "import: what {LEGACY_SERVICE} stored for an account is not readable: {e}"
                    );
                    failed.push(account.clone());
                }
            },
            // Nothing stored is the ordinary case, not a gap: a connection that never had a
            // password, and the `rest-env:` form of every id that is not a REST environment.
            Ok(None) => {}
            Err(e) => {
                log::warn!("import: an account could not be read from {LEGACY_SERVICE}: {e:?}");
                failed.push(account.clone());
            }
        }
    }
    (found, failed)
}

/// The same, against the machine's own credential store under the name this application used to
/// have. The vault is macOS's, exactly as it is for this application's own entries.
pub fn read_legacy(accounts: &[String]) -> (Vec<(String, Secrets)>, Vec<String>) {
    read_all(
        &OsStore::new(LEGACY_SERVICE),
        cfg!(target_os = "macos"),
        accounts,
    )
}

/// The password a `SavedConnection.keyringRef` points at, or `None` when MixEngine no longer has
/// that entry.
#[tauri::command]
pub async fn secrets_resolve_mixengine(key: String) -> Result<Option<String>, AppError> {
    in_background(move || read_mixengine_entry(&key)).await
}

#[cfg(test)]
mod tests;
