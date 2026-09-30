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
        self.flush(&next)?;
        *vault = next;
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
mod tests {
    use super::{Keeper, Secrets, Store, VAULT};
    use crate::error::AppError;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// A credential store that is a `HashMap`, and that remembers every account it was asked for,
    /// so a test can say how many dialogs macOS would have raised.
    ///
    /// Shared through an `Arc` because a `Keeper` owns its store: the test needs the store to
    /// outlive one keeper in order to open a second one over it and stand in for a fresh run.
    #[derive(Default)]
    struct MemoryStore {
        items: Mutex<HashMap<String, String>>,
        reads: Mutex<Vec<String>>,
        /// Makes every write fail, standing in for a credential store that is locked, full, or
        /// whose dialog the user answered with *Deny*.
        refuse_writes: Mutex<bool>,
    }

    impl MemoryStore {
        fn seed(&self, account: &str, value: &str) {
            self.items
                .lock()
                .unwrap()
                .insert(account.to_string(), value.to_string());
        }

        fn has(&self, account: &str) -> bool {
            self.items.lock().unwrap().contains_key(account)
        }

        fn refuse_writes(&self, refuse: bool) {
            *self.refuse_writes.lock().unwrap() = refuse;
        }

        fn reads_of(&self, account: &str) -> usize {
            self.reads
                .lock()
                .unwrap()
                .iter()
                .filter(|asked| *asked == account)
                .count()
        }
    }

    impl Store for Arc<MemoryStore> {
        fn read(&self, account: &str) -> Result<Option<String>, AppError> {
            self.reads.lock().unwrap().push(account.to_string());
            Ok(self.items.lock().unwrap().get(account).cloned())
        }

        fn write(&self, account: &str, value: &str) -> Result<(), AppError> {
            if *self.refuse_writes.lock().unwrap() {
                return Err(crate::err!("error.cannotSavePassword"));
            }
            self.seed(account, value);
            Ok(())
        }

        fn forget(&self, account: &str) -> Result<(), AppError> {
            self.items.lock().unwrap().remove(account);
            Ok(())
        }
    }

    /// One password, which is what nearly every one of these is about.
    fn secrets(password: &str) -> Secrets {
        let mut secrets = Secrets::new();
        secrets.insert("password".to_string(), password.to_string());
        secrets
    }

    /// A store and a keeper over it, the store handed back so a test can look inside.
    ///
    /// Both modes are asked for by name rather than taken from `cfg!`, so the whole of this module
    /// is tested wherever the suite runs — the vault included, on a machine that would never use
    /// it.
    fn fixture(vaulted: bool) -> (Arc<MemoryStore>, Keeper<Arc<MemoryStore>>) {
        let store = Arc::new(MemoryStore::default());
        (store.clone(), Keeper::with_vault(store, vaulted))
    }

    /// A keeper that keeps everything in one entry, as macOS does.
    fn vaulted() -> (Arc<MemoryStore>, Keeper<Arc<MemoryStore>>) {
        fixture(true)
    }

    /// A keeper that gives each connection an entry, as Windows and Linux do.
    fn per_entry() -> (Arc<MemoryStore>, Keeper<Arc<MemoryStore>>) {
        fixture(false)
    }

    #[test]
    fn secrets_survive_a_round_trip_through_the_vault() {
        let (_store, keeper) = vaulted();

        // A connection with nothing stored reads as empty rather than as a failure.
        assert!(keeper.load("a").unwrap().is_empty());

        let mut written = secrets("hunter2");
        written.insert("sshPassphrase".to_string(), "let me in".to_string());
        keeper.save("a", &written).unwrap();
        assert_eq!(keeper.load("a").unwrap(), written);
    }

    #[test]
    fn one_connection_leaving_the_vault_does_not_take_another_with_it() {
        let (_store, keeper) = vaulted();
        keeper.save("a", &secrets("one")).unwrap();
        keeper.save("b", &secrets("two")).unwrap();

        keeper.delete("a").unwrap();

        assert!(keeper.load("a").unwrap().is_empty());
        assert_eq!(keeper.load("b").unwrap(), secrets("two"));
    }

    /// The reason the vault exists: ten saved connections opening at once are one visit to the
    /// credential store, which on macOS is one dialog instead of ten.
    #[test]
    fn ten_connections_are_one_visit_to_the_store() {
        let (store, keeper) = vaulted();
        let ids: Vec<String> = (0..10).map(|i| format!("id-{i}")).collect();
        for id in &ids {
            keeper.save(id, &secrets(id)).unwrap();
        }
        drop(keeper);

        // A fresh run: the same store, nothing cached.
        let keeper = Keeper::with_vault(store.clone(), true);
        let before = store.reads_of(VAULT);
        for id in &ids {
            assert_eq!(keeper.load(id).unwrap(), secrets(id));
        }

        assert_eq!(store.reads_of(VAULT) - before, 1);
        // No connection's own id was ever asked for: they all came out of the vault.
        assert_eq!(store.reads_of("id-3"), 0);
    }

    /// What the first run after the update does: an entry written when there was one per
    /// connection is folded into the vault and the old entry removed, so it is asked for once and
    /// never again.
    #[test]
    fn an_entry_from_before_the_vault_moves_across_on_first_read() {
        let (store, keeper) = vaulted();
        store.seed("old", &serde_json::to_string(&secrets("legacy")).unwrap());

        assert_eq!(keeper.load("old").unwrap(), secrets("legacy"));
        assert!(
            !store.has("old"),
            "the old entry is removed once it has been moved"
        );

        assert_eq!(keeper.load("old").unwrap(), secrets("legacy"));
        assert_eq!(
            store.reads_of("old"),
            1,
            "the second read comes from the vault"
        );
    }

    #[test]
    fn saving_nothing_leaves_nothing_behind() {
        let (store, keeper) = vaulted();
        keeper.save("a", &secrets("hunter2")).unwrap();
        assert!(store.has(VAULT));

        keeper.save("a", &Secrets::new()).unwrap();

        assert!(keeper.load("a").unwrap().is_empty());
        assert!(
            !store.has(VAULT),
            "an empty vault is deleted, not stored as an empty object"
        );
    }

    /// A leftover from before the vault does not outlive the password it held, even when the
    /// connection was saved again before anything ever read it.
    #[test]
    fn saving_clears_the_entry_this_connection_used_to_have() {
        let (store, keeper) = vaulted();
        store.seed("a", &serde_json::to_string(&secrets("legacy")).unwrap());

        keeper.save("a", &secrets("current")).unwrap();

        assert!(!store.has("a"));
        assert_eq!(keeper.load("a").unwrap(), secrets("current"));
    }

    /// The move into the vault is a copy before it is a move: a store that will not take the write
    /// leaves the old entry exactly where it was, and the connection still has its password on the
    /// next run.
    #[test]
    fn a_failed_move_into_the_vault_destroys_nothing() {
        let (store, keeper) = vaulted();
        store.seed("old", &serde_json::to_string(&secrets("legacy")).unwrap());
        store.refuse_writes(true);

        assert!(keeper.load("old").is_err());
        assert!(
            store.has("old"),
            "the old entry is still there to be read again"
        );

        // The store recovers, and so does the connection — from the entry that was never deleted.
        store.refuse_writes(false);
        assert_eq!(keeper.load("old").unwrap(), secrets("legacy"));
        assert!(!store.has("old"));
    }

    /// A save the store refuses is a save that did not happen anywhere. The run must not go on
    /// reporting the new password back as if it had been written.
    #[test]
    fn a_failed_save_leaves_the_old_password_in_place() {
        let (store, keeper) = vaulted();
        keeper.save("a", &secrets("first")).unwrap();

        store.refuse_writes(true);
        assert!(keeper.save("a", &secrets("second")).is_err());

        assert_eq!(keeper.load("a").unwrap(), secrets("first"));
        store.refuse_writes(false);
        assert_eq!(keeper.load("a").unwrap(), secrets("first"));
    }

    /// The regression this guards: sync's entry kept as an item of its own beside the vault, which
    /// on macOS was a second password dialog on every launch of a new build. One launch reads the
    /// vault and nothing else, whatever it holds.
    #[test]
    fn sync_and_the_connections_are_one_visit_to_the_store() {
        let (store, keeper) = vaulted();
        keeper.save("a", &secrets("hunter2")).unwrap();
        keeper.write_own("sync-master-key", "{\"mk\":1}").unwrap();
        drop(keeper);

        let keeper = Keeper::with_vault(store.clone(), true);
        let before = store.reads.lock().unwrap().len();
        assert_eq!(
            keeper.read_own("sync-master-key").unwrap().as_deref(),
            Some("{\"mk\":1}")
        );
        assert_eq!(keeper.load("a").unwrap(), secrets("hunter2"));

        let asked = store.reads.lock().unwrap()[before..].to_vec();
        assert_eq!(asked, [VAULT], "one item, so one dialog");
        assert!(!store.has("sync-master-key"), "nothing beside the vault");
    }

    /// Signing out takes the entry out of the vault and leaves every connection where it was.
    #[test]
    fn forgetting_sync_keeps_the_connections() {
        let (store, keeper) = vaulted();
        keeper.save("a", &secrets("hunter2")).unwrap();
        keeper.write_own("sync-master-key", "saved").unwrap();

        keeper.forget_own("sync-master-key").unwrap();

        assert_eq!(keeper.read_own("sync-master-key").unwrap(), None);
        assert_eq!(keeper.load("a").unwrap(), secrets("hunter2"));

        keeper.delete("a").unwrap();
        assert!(!store.has(VAULT), "an empty vault is still deleted");
    }

    /// Without the vault, sync's entry is an item of its own, as a connection's is.
    #[test]
    fn without_the_vault_sync_keeps_its_own_entry() {
        let (store, keeper) = per_entry();
        keeper.write_own("sync-master-key", "saved").unwrap();

        assert!(store.has("sync-master-key") && !store.has(VAULT));
        assert_eq!(
            keeper.read_own("sync-master-key").unwrap().as_deref(),
            Some("saved")
        );

        keeper.forget_own("sync-master-key").unwrap();
        assert!(!store.has("sync-master-key"));
    }

    /// What Windows and Linux still do: an entry each, no vault, nothing cached.
    #[test]
    fn without_the_vault_every_connection_keeps_its_own_entry() {
        let (store, keeper) = per_entry();

        assert!(keeper.load("a").unwrap().is_empty());

        keeper.save("a", &secrets("hunter2")).unwrap();
        keeper.save("b", &secrets("two")).unwrap();
        assert_eq!(keeper.load("a").unwrap(), secrets("hunter2"));
        assert!(store.has("a") && store.has("b"));
        assert!(!store.has(VAULT), "no vault is written where none is used");

        keeper.delete("a").unwrap();
        assert!(!store.has("a"));
        assert_eq!(keeper.load("b").unwrap(), secrets("two"));
    }

    /// Nothing is cached without the vault, so a password changed in the store is seen at once —
    /// and, more to the point, the store is read every time rather than once per run.
    #[test]
    fn without_the_vault_every_read_reaches_the_store() {
        let (store, keeper) = per_entry();
        keeper.save("a", &secrets("hunter2")).unwrap();

        keeper.load("a").unwrap();
        keeper.load("a").unwrap();

        assert_eq!(store.reads_of("a"), 2);
        assert_eq!(store.reads_of(VAULT), 0);
    }

    /// The standalone client on Windows and Linux: an entry per account. The reader takes what the store files
    /// name and leaves everything else, the entries themselves included.
    #[test]
    fn the_legacy_reader_takes_per_account_entries_and_writes_nothing() {
        let store = Arc::new(MemoryStore::default());
        store.seed("a", &serde_json::to_string(&secrets("one")).unwrap());
        store.seed("b", &serde_json::to_string(&secrets("two")).unwrap());

        let (found, failed) = super::read_all(&store, false, &["a".into(), "c".into()]);

        assert_eq!(found, vec![("a".to_string(), secrets("one"))]);
        assert!(
            failed.is_empty(),
            "an account with no entry is not a failure"
        );
        assert!(
            store.has("a") && store.has("b"),
            "the old entries stay where they are"
        );
    }

    /// The standalone client on macOS: one `vault` entry holding every account. Read once, and left alone.
    #[test]
    fn the_legacy_reader_takes_the_vault() {
        let store = Arc::new(MemoryStore::default());
        let mut vault = HashMap::new();
        vault.insert("a".to_string(), secrets("one"));
        vault.insert("b".to_string(), secrets("two"));
        store.seed(VAULT, &serde_json::to_string(&vault).unwrap());

        let (found, failed) = super::read_all(&store, true, &["b".into(), "zz".into()]);

        assert_eq!(found, vec![("b".to_string(), secrets("two"))]);
        assert!(failed.is_empty());
        assert_eq!(
            store.reads_of(VAULT),
            1,
            "the vault is read once, not once per account"
        );
        assert!(store.has(VAULT));
    }

    /// A connection the standalone client saved before the vault existed keeps an entry of its own beside it. The
    /// reader looks there second, and still moves nothing.
    #[test]
    fn the_legacy_reader_falls_back_to_a_pre_vault_entry() {
        let store = Arc::new(MemoryStore::default());
        let mut vault = HashMap::new();
        vault.insert("a".to_string(), secrets("one"));
        store.seed(VAULT, &serde_json::to_string(&vault).unwrap());
        store.seed("old", &serde_json::to_string(&secrets("legacy")).unwrap());

        let (found, _) = super::read_all(&store, true, &["a".into(), "old".into()]);

        assert_eq!(
            found,
            vec![
                ("a".to_string(), secrets("one")),
                ("old".to_string(), secrets("legacy")),
            ]
        );
        assert!(store.has("old"), "the pre-vault entry is read, never moved");
    }

    /// What could not be read is counted rather than swallowed: the marker the import writes says
    /// how many accounts it did not reach.
    #[test]
    fn the_legacy_reader_counts_what_it_could_not_read() {
        let store = Arc::new(MemoryStore::default());
        store.seed("a", "this is not json");

        let (found, failed) = super::read_all(&store, false, &["a".into()]);

        assert!(found.is_empty());
        assert_eq!(failed, vec!["a".to_string()]);
    }

    /// Ignored by default: it writes to the machine's real credential store, which a headless
    /// Linux CI box has no running Secret Service for. Run it by hand with
    /// `cargo test -- --ignored` on a desktop to check the store is actually reachable.
    #[test]
    #[ignore]
    fn secrets_survive_a_round_trip_through_the_os_store() {
        let id = format!("mixlab-test-{}", uuid::Uuid::new_v4());

        assert!(super::load(&id).unwrap().is_empty());

        let mut written = secrets("hunter2");
        written.insert("sshPassphrase".to_string(), "let me in".to_string());
        super::save(&id, &written).unwrap();
        assert_eq!(super::load(&id).unwrap(), written);

        super::save(&id, &Secrets::new()).unwrap();
        assert!(super::load(&id).unwrap().is_empty());

        super::delete(&id).unwrap();
    }

    /// Ignored for the same reason as the round-trip above: it reaches the machine's real
    /// credential store. Run by hand with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn a_mixengine_entry_round_trips_and_a_missing_one_is_none() {
        // Written the way the daemon writes it (T186): through the platform crate, at an address
        // with a home in front, which on macOS lands inside that home's vault item.
        let home = uuid::Uuid::new_v4().simple().to_string();
        let key = format!("{home}/mariadb@main/root");
        let host = mixengine_platform::host();
        let keyring = host.keyring();
        assert_eq!(super::read_mixengine_entry(&key).unwrap(), None);

        keyring
            .set_secret(mixengine_platform::KEYRING_SERVICE, &key, "hunter2")
            .unwrap();
        assert_eq!(
            super::read_mixengine_entry(&key).unwrap(),
            Some("hunter2".to_string())
        );

        keyring
            .forget_secret(mixengine_platform::KEYRING_SERVICE, &key)
            .unwrap();
        assert_eq!(super::read_mixengine_entry(&key).unwrap(), None);
    }
}
