//! One Keychain item per home for `mixengined`'s credentials — roadmap task **T186**, ADR 0055.
//!
//! **macOS asks once per item**, for any program it does not recognise by signature, and an
//! unsigned release is such a program after every update. One item per credential was one question
//! per password; one item per home is one question.
//!
//! The address callers use does not change: `<home-id>/<rest>` is split at its first `/` into the
//! item's account and the entry inside it. A key with no home in front, or of another service,
//! passes through untouched.
//!
//! **Held for the run.** An item is read once and kept, behind one lock, so services starting in
//! parallel wait for the first read rather than each raising a dialog. The daemon is the only
//! writer of a home (its lock), so once this process has written a vault its copy is the truth and a
//! miss is final. A copy this process only read may be stale (the window reading what the daemon
//! keeps), so a miss there reads the item once more.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use serde::{Deserialize, Serialize};

use crate::{Error, Keyring, Result};

/// The only shape this build reads or writes.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Default)]
struct Document {
    version: u32,
    entries: BTreeMap<String, String>,
}

/// One home's vault as this process last saw it.
#[derive(Debug, Default)]
pub(crate) struct Held {
    entries: BTreeMap<String, String>,
    /// This process wrote it, so nobody else has anything newer.
    wrote: bool,
}

/// A credential store with `service`'s per-home keys folded into one item per home.
#[derive(Debug)]
pub(crate) struct Vaulted<K> {
    inner: K,
    service: &'static str,
    held: Arc<Mutex<HashMap<String, Held>>>,
}

impl<K: Keyring> Vaulted<K> {
    pub(crate) fn new(
        inner: K,
        service: &'static str,
        held: Arc<Mutex<HashMap<String, Held>>>,
    ) -> Self {
        Self {
            inner,
            service,
            held,
        }
    }

    /// The OS store's, sharing one cache across every `Host` this process builds: the window
    /// builds one per call, and a cache per `Host` would be a question per call.
    pub(crate) fn over_the_os(inner: K) -> Self {
        static HELD: OnceLock<Arc<Mutex<HashMap<String, Held>>>> = OnceLock::new();

        Self::new(
            inner,
            crate::KEYRING_SERVICE,
            Arc::clone(HELD.get_or_init(Arc::default)),
        )
    }

    /// `(home, rest)` when `key` belongs in a vault.
    fn split<'a>(&self, service: &str, key: &'a str) -> Option<(&'a str, &'a str)> {
        (service == self.service)
            .then(|| key.split_once('/'))
            .flatten()
            .filter(|(home, rest)| is_a_home(home) && !rest.is_empty())
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Held>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read `home`'s item from the store into `held`.
    fn load(
        &self,
        held: &mut HashMap<String, Held>,
        home: &str,
        action: &'static str,
    ) -> Result<()> {
        let entries = match self.inner.secret(self.service, home)? {
            None => BTreeMap::new(),
            Some(text) => {
                let document: Document = serde_json::from_str(&text)
                    .ok()
                    .filter(|document: &Document| document.version == VERSION)
                    .ok_or_else(|| Error::Secret {
                        action,
                        service: self.service.to_owned(),
                        key: home.to_owned(),
                        source: "this home's vault is not in a shape this build reads".into(),
                    })?;
                document.entries
            }
        };

        held.insert(
            home.to_owned(),
            Held {
                entries,
                wrote: false,
            },
        );
        Ok(())
    }

    /// Write `home`'s held entries back as one item, or remove the item when there are none.
    fn save(&self, held: &mut HashMap<String, Held>, home: &str) -> Result<()> {
        let vault = held.entry(home.to_owned()).or_default();
        vault.wrote = true;

        if vault.entries.is_empty() {
            return self.inner.forget_secret(self.service, home);
        }

        let text = serde_json::to_string(&Document {
            version: VERSION,
            entries: vault.entries.clone(),
        })
        .map_err(|error| Error::Secret {
            action: "store",
            service: self.service.to_owned(),
            key: home.to_owned(),
            source: error.to_string().into(),
        })?;

        self.inner.set_secret(self.service, home, &text)
    }
}

impl<K: Keyring> Keyring for Vaulted<K> {
    fn secret(&self, service: &str, key: &str) -> Result<Option<String>> {
        let Some((home, rest)) = self.split(service, key) else {
            return self.inner.secret(service, key);
        };

        let mut held = self.lock();
        let fresh = !held.contains_key(home);
        if fresh {
            self.load(&mut held, home, "read")?;
        }

        let vault = &held[home];
        if let Some(found) = vault.entries.get(rest) {
            return Ok(Some(found.clone()));
        }
        if !fresh && !vault.wrote {
            self.load(&mut held, home, "read")?;
            if let Some(found) = held[home].entries.get(rest) {
                return Ok(Some(found.clone()));
            }
        }

        // **A daemon from before T186 kept this password as an item of its own**, under the account
        // `<home>/<rest>`, and a vault that does not hold it is not the last word: the item is read
        // where that daemon left it. Listed first, which asks the store nothing it would prompt for,
        // so a key nobody ever kept still costs no visit (T186's promise, the test above).
        if self.inner.keys(service)?.iter().any(|flat| flat == key) {
            return self.inner.secret(service, key);
        }
        Ok(None)
    }

    fn set_secret(&self, service: &str, key: &str, secret: &str) -> Result<()> {
        let Some((home, rest)) = self.split(service, key) else {
            return self.inner.set_secret(service, key, secret);
        };

        let mut held = self.lock();
        if !held.contains_key(home) {
            self.load(&mut held, home, "store")?;
        }
        held.get_mut(home)
            .expect("loaded above")
            .entries
            .insert(rest.to_owned(), secret.to_owned());

        self.save(&mut held, home)
    }

    fn forget_secret(&self, service: &str, key: &str) -> Result<()> {
        let Some((home, rest)) = self.split(service, key) else {
            return self.inner.forget_secret(service, key);
        };

        let mut held = self.lock();
        if !held.contains_key(home) {
            self.load(&mut held, home, "forget")?;
        }
        if held
            .get_mut(home)
            .expect("loaded above")
            .entries
            .remove(rest)
            .is_some()
        {
            self.save(&mut held, home)?;
        }

        // And the item a daemon from before T186 would have kept under this very name, which `keys`
        // lists as it is: an uninstall that forgot the vault's entry alone left it behind. The
        // inner store's forget is idempotent, so a name it never held costs nothing.
        self.inner.forget_secret(service, key)
    }

    fn keys(&self, service: &str) -> Result<Vec<String>> {
        let accounts = self.inner.keys(service)?;
        if service != self.service {
            return Ok(accounts);
        }

        let mut held = self.lock();
        let mut keys = Vec::new();

        for account in accounts {
            if !is_a_home(&account) {
                keys.push(account);
                continue;
            }
            if !held.contains_key(&account) {
                self.load(&mut held, &account, "list")?;
            }
            keys.extend(
                held[&account]
                    .entries
                    .keys()
                    .map(|rest| format!("{account}/{rest}")),
            );
        }

        Ok(keys)
    }

    /// Never here: [`keys`](Self::keys) opens every home's item to name what is inside it, and an
    /// item this process has not read yet is a Keychain question.
    fn keys_without_asking(&self, _service: &str) -> Option<Result<Vec<String>>> {
        None
    }
}

/// The shape `mixengine_core::home::HomeId` has: lowercase hex, not empty.
///
/// Repeated here rather than imported because this crate sits below `mixengine-core`. A service id
/// (`mariadb@main`) never passes it, which is what keeps a pre-T126 key out of a vault.
fn is_a_home(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{Held, Vaulted};
    use crate::{Keyring, Result};

    const SERVICE: &str = "mixengine";
    const HOME: &str = "0123456789ab";

    /// A store that counts how often it is touched: the whole point of the vault is that number.
    #[derive(Debug, Default)]
    struct Counting {
        items: Mutex<HashMap<(String, String), String>>,
        reads: AtomicUsize,
    }

    impl Keyring for Arc<Counting> {
        fn secret(&self, service: &str, key: &str) -> Result<Option<String>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .items
                .lock()
                .unwrap()
                .get(&(service.into(), key.into()))
                .cloned())
        }
        fn set_secret(&self, service: &str, key: &str, secret: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap()
                .insert((service.into(), key.into()), secret.into());
            Ok(())
        }
        fn forget_secret(&self, service: &str, key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap()
                .remove(&(service.into(), key.into()));
            Ok(())
        }
        fn keys(&self, service: &str) -> Result<Vec<String>> {
            Ok(self
                .items
                .lock()
                .unwrap()
                .keys()
                .filter(|(s, _)| s == service)
                .map(|(_, k)| k.clone())
                .collect())
        }
    }

    fn vaulted(store: &Arc<Counting>) -> Vaulted<Arc<Counting>> {
        Vaulted::new(
            Arc::clone(store),
            SERVICE,
            Arc::new(Mutex::new(HashMap::<String, Held>::new())),
        )
    }

    /// A diagnosis does not get to open every home's item: on macOS each one is a question.
    #[test]
    fn a_vault_is_never_listed_without_asking() {
        let store = Arc::new(Counting::default());
        let vault = vaulted(&store);
        vault
            .set_secret(SERVICE, "0123456789ab/mariadb@main/root", "a")
            .expect("a write");

        assert!(vault.keys_without_asking(SERVICE).is_none());
    }

    #[test]
    fn a_homes_credentials_are_one_item() {
        let store = Arc::new(Counting::default());
        let vault = vaulted(&store);

        vault
            .set_secret(SERVICE, &format!("{HOME}/mariadb@main/root"), "a")
            .unwrap();
        vault
            .set_secret(SERVICE, &format!("{HOME}/postgres@main/postgres"), "b")
            .unwrap();

        let items = store.items.lock().unwrap();
        assert_eq!(items.len(), 1, "one item for the home: {:?}", items.keys());
        assert!(items.contains_key(&(SERVICE.to_owned(), HOME.to_owned())));
    }

    #[test]
    fn many_reads_are_one_visit_to_the_store() {
        let store = Arc::new(Counting::default());
        vaulted(&store)
            .set_secret(SERVICE, &format!("{HOME}/mariadb@main/root"), "a")
            .unwrap();
        vaulted(&store)
            .set_secret(SERVICE, &format!("{HOME}/postgres@main/postgres"), "b")
            .unwrap();
        store.reads.store(0, Ordering::SeqCst);

        let reader = vaulted(&store);
        for _ in 0..5 {
            assert_eq!(
                reader
                    .secret(SERVICE, &format!("{HOME}/mariadb@main/root"))
                    .unwrap()
                    .as_deref(),
                Some("a")
            );
            assert_eq!(
                reader
                    .secret(SERVICE, &format!("{HOME}/postgres@main/postgres"))
                    .unwrap()
                    .as_deref(),
                Some("b")
            );
        }

        assert_eq!(store.reads.load(Ordering::SeqCst), 1);
    }

    /// Review focus 2: the window cached the vault, then the daemon added an account.
    #[test]
    fn a_reader_finds_what_the_writer_added_after_it_looked() {
        let store = Arc::new(Counting::default());
        let writer = vaulted(&store);
        let reader = vaulted(&store);

        writer
            .set_secret(SERVICE, &format!("{HOME}/mariadb@main/root"), "a")
            .unwrap();
        assert!(
            reader
                .secret(SERVICE, &format!("{HOME}/mariadb@main/root"))
                .unwrap()
                .is_some()
        );

        writer
            .set_secret(SERVICE, &format!("{HOME}/mariadb@main/blog"), "b")
            .unwrap();
        assert_eq!(
            reader
                .secret(SERVICE, &format!("{HOME}/mariadb@main/blog"))
                .unwrap()
                .as_deref(),
            Some("b")
        );
    }

    /// Review focus 2, the other half: a writer's copy is the truth, and a miss does not re-read.
    #[test]
    fn a_writer_does_not_go_back_to_the_store_on_a_miss() {
        let store = Arc::new(Counting::default());
        let writer = vaulted(&store);
        writer
            .set_secret(SERVICE, &format!("{HOME}/mariadb@main/root"), "a")
            .unwrap();
        let before = store.reads.load(Ordering::SeqCst);

        assert_eq!(
            writer
                .secret(SERVICE, &format!("{HOME}/nothing/here"))
                .unwrap(),
            None
        );
        assert_eq!(store.reads.load(Ordering::SeqCst), before);
    }

    /// Review focus 3.
    #[test]
    fn forgetting_the_last_key_removes_the_item() {
        let store = Arc::new(Counting::default());
        let vault = vaulted(&store);
        let key = format!("{HOME}/mariadb@main/root");

        vault.set_secret(SERVICE, &key, "a").unwrap();
        vault.forget_secret(SERVICE, &key).unwrap();

        assert!(store.items.lock().unwrap().is_empty());
        assert!(vault.keys(SERVICE).unwrap().is_empty());
    }

    /// A daemon from before T186 wrote each password as an item of its own, under the account
    /// `<home>/<rest>`. `keys` lists such an item as it is, so a reader and an uninstall that took
    /// the vault path alone found nothing there: the password unreadable, the item never forgotten
    /// ("3 of 3 are still in this user's credential store", found by the T182a walk).
    #[test]
    fn a_key_an_older_daemon_kept_flat_is_read_and_forgotten() {
        let store = Arc::new(Counting::default());
        let vault = vaulted(&store);
        let key = format!("{HOME}/mariadb@main/root");
        store.set_secret(SERVICE, &key, "flat").unwrap();

        assert_eq!(
            vault.secret(SERVICE, &key).unwrap().as_deref(),
            Some("flat")
        );
        assert_eq!(vault.keys(SERVICE).unwrap(), vec![key.clone()]);

        vault.forget_secret(SERVICE, &key).unwrap();

        assert!(
            store.items.lock().unwrap().is_empty(),
            "the flat item is gone"
        );
        assert!(vault.keys(SERVICE).unwrap().is_empty());
    }

    /// Both shapes at once: the vault's entry wins the read, and a forget takes both.
    #[test]
    fn a_vault_entry_shadows_a_flat_item_and_a_forget_takes_both() {
        let store = Arc::new(Counting::default());
        let vault = vaulted(&store);
        let key = format!("{HOME}/mariadb@main/root");
        store.set_secret(SERVICE, &key, "flat").unwrap();
        vault.set_secret(SERVICE, &key, "vaulted").unwrap();

        assert_eq!(
            vault.secret(SERVICE, &key).unwrap().as_deref(),
            Some("vaulted")
        );

        vault.forget_secret(SERVICE, &key).unwrap();

        assert!(
            store.items.lock().unwrap().is_empty(),
            "{:?}",
            store.items.lock().unwrap()
        );
    }

    #[test]
    fn keys_expand_a_vault_and_pass_the_rest_through() {
        let store = Arc::new(Counting::default());
        let vault = vaulted(&store);

        vault
            .set_secret(SERVICE, &format!("{HOME}/mariadb@main/root"), "a")
            .unwrap();
        vault
            .set_secret(SERVICE, "mariadb@main/root", "legacy")
            .unwrap();
        vault
            .set_secret("elsewhere", &format!("{HOME}/x"), "c")
            .unwrap();

        let mut keys = vault.keys(SERVICE).unwrap();
        keys.sort();
        assert_eq!(
            keys,
            [
                format!("{HOME}/mariadb@main/root"),
                "mariadb@main/root".to_owned()
            ]
        );

        // Neither the pre-T126 key nor another service is vaulted.
        let items = store.items.lock().unwrap();
        assert!(items.contains_key(&(SERVICE.to_owned(), "mariadb@main/root".to_owned())));
        assert!(items.contains_key(&("elsewhere".to_owned(), format!("{HOME}/x"))));
    }

    #[test]
    fn a_vault_that_does_not_parse_is_an_error_that_quotes_nothing() {
        let store = Arc::new(Counting::default());
        store.set_secret(SERVICE, HOME, "hunter2-not-json").unwrap();

        let error = vaulted(&store)
            .secret(SERVICE, &format!("{HOME}/mariadb@main/root"))
            .expect_err("a refusal");
        assert!(
            !format!("{error:?} {error}").contains("hunter2"),
            "{error:?}"
        );
    }
}
