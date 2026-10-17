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
    /// Every account written, in order — a write on macOS is what moves the item's
    /// modification date, and possibly what narrows its trust to this build.
    writes: Mutex<Vec<String>>,
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

    fn writes_of(&self, account: &str) -> usize {
        self.writes
            .lock()
            .unwrap()
            .iter()
            .filter(|written| *written == account)
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
        self.writes.lock().unwrap().push(account.to_string());
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

/// A save that changes nothing writes nothing. On macOS a write moves the vault's modification
/// date, and saving a pin, a sidebar width or a sync page that brought the same password must
/// not be one.
#[test]
fn saving_what_is_already_there_writes_nothing() {
    let (store, keeper) = vaulted();
    keeper.save("a", &secrets("hunter2")).unwrap();

    keeper.save("a", &secrets("hunter2")).unwrap();
    // Deleting what was never there changes nothing either.
    keeper.delete("never-saved").unwrap();

    assert_eq!(store.writes_of(VAULT), 1);
    assert_eq!(keeper.load("a").unwrap(), secrets("hunter2"));

    keeper.save("a", &secrets("changed")).unwrap();
    assert_eq!(store.writes_of(VAULT), 2, "a real change is still written");
}

/// The same for sync's own entry: a refresh that hands back the token already kept writes
/// nothing, and a new token is written.
#[test]
fn writing_the_same_own_value_writes_nothing() {
    let (store, keeper) = vaulted();
    keeper.write_own("sync-master-key", "one").unwrap();

    keeper.write_own("sync-master-key", "one").unwrap();
    keeper.forget_own("never-kept").unwrap();
    assert_eq!(store.writes_of(VAULT), 1);

    keeper.write_own("sync-master-key", "two").unwrap();
    assert_eq!(store.writes_of(VAULT), 2);
}

/// Skipping the write does not skip the cleanup: a connection deleted before anything read it
/// still takes its pre-vault entry with it.
#[test]
fn deleting_an_unread_leftover_still_removes_it() {
    let (store, keeper) = vaulted();
    store.seed("old", &serde_json::to_string(&secrets("legacy")).unwrap());

    keeper.delete("old").unwrap();

    assert!(!store.has("old"));
    assert_eq!(store.writes_of(VAULT), 0);
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
