//! Two machines, one server, and the whole round trip between them — each test a story that loses
//! data or never ends, from
//! `docs/specs/2026-09-22-t178-a-pull-that-keeps-what-it-has-not-agreed-design.md`.
//!
//! **[`Machine::sync`] is `loop.ts`'s `syncCollection` over `session.rs`'s calls**, in their order:
//! this machine's changes noticed, every page fetched, opened, written by the module, landed and
//! committed, then those changes pushed and any lost conflict written down and landed. The module
//! is a list of items applied the way `applySyncChanges` applies them, including the item it cannot
//! read and leaves alone. **[`Server`] answers the way both servers do**, `server/native/src/records.rs` and
//! `server/worker/src/records.ts`: one `seq` per account, a page per collection whose last page's
//! `nextSince` is the account's latest seq, one `reaped_below_seq` per account that a `resync=1`
//! read is not refused by, a tombstone that keeps the time its deletion was made and is a version
//! like any other.
//!
//! A change to either side's order belongs here too, or these tests describe a program nobody runs.
//!
//! Here rather than in a module's `mod tests` because no one module owns the story: each test
//! crosses `lend`, `engine` and `store` on two machines at once. Unlike `sync_live.rs`, it needs
//! no server and runs with the rest of `cargo test`.

// A test file: a panic here is a failed test. `clippy.toml` allows that inside a `#[test]`, not in
// the helpers around one.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

use serde_json::{json, Value};

use tauri_app_lib::sync::crypto;
use tauri_app_lib::sync::engine;
use tauri_app_lib::sync::lend::{self, Incoming, Item, Keys};
use tauri_app_lib::sync::store::Store;
use tauri_app_lib::sync::transport::{PageOutcome, Remote};
use tauri_app_lib::sync::wire::{
    BatchResult, Capabilities, ErrorDetail, Operation, Page, RecordBody, WireRecord,
};
use tauri_app_lib::sync::AppError;

/// More pages than any collection here has. A sync still fetching after this never ends.
const MAX_PAGES: usize = 50;

#[derive(Default)]
struct State {
    records: BTreeMap<(String, String), WireRecord>,
    seq: i64,
    reaped_below: i64,
    /// How many times a page was refused with `410 cursor-expired`.
    expired: usize,
    /// How many pages were read from the start of a collection: a first pull or a resync.
    reads_from_start: usize,
}

struct Server {
    page_records: usize,
    state: Mutex<State>,
}

impl Server {
    fn new(page_records: usize) -> Self {
        Self {
            page_records,
            state: Mutex::new(State::default()),
        }
    }

    /// The reaper with its horizon at now: every tombstone goes, and every cursor below the
    /// highest of them is incomplete news (`server/native/src/reaper.rs`).
    fn sweep(&self) {
        let mut state = self.state.lock().unwrap();
        let highest = state
            .records
            .values()
            .filter(|record| record.deleted)
            .map(|record| record.seq)
            .max()
            .unwrap_or(0);
        state.reaped_below = state.reaped_below.max(highest);
        state.records.retain(|_, record| !record.deleted);
    }

    fn record(&self, collection: &str, id: &str) -> Option<WireRecord> {
        let state = self.state.lock().unwrap();
        state
            .records
            .get(&(collection.to_owned(), id.to_owned()))
            .cloned()
    }

    fn expired(&self) -> usize {
        self.state.lock().unwrap().expired
    }

    fn reads_from_start(&self) -> usize {
        self.state.lock().unwrap().reads_from_start
    }

    fn page(&self, collection: &str, since: i64, resync: bool) -> PageOutcome {
        let mut state = self.state.lock().unwrap();
        if since == 0 {
            state.reads_from_start += 1;
        }
        if since > 0 && since < state.reaped_below && !resync {
            state.expired += 1;
            return PageOutcome::CursorExpired;
        }
        let mut records: Vec<_> = state
            .records
            .values()
            .filter(|record| record.collection == collection && record.seq > since)
            .cloned()
            .collect();
        records.sort_by_key(|record| record.seq);
        let more = records.len() > self.page_records;
        records.truncate(self.page_records);
        let last = records.last().map_or(since, |record| record.seq);
        let next_since = if more { last } else { last.max(state.seq) };
        PageOutcome::Page(Page {
            records,
            next_since,
            more,
        })
    }

    fn apply(&self, device: &str, operation: &Operation) -> BatchResult {
        let mut state = self.state.lock().unwrap();
        let (collection, id) = match operation {
            Operation::Put { collection, id, .. } | Operation::Delete { collection, id, .. } => {
                (collection.clone(), id.clone())
            }
        };
        let key = (collection.clone(), id.clone());
        let existing = state.records.get(&key).cloned();

        let deleted_at = match operation {
            Operation::Delete { updated_at, .. } => *updated_at,
            Operation::Put { .. } => 0,
        };
        let (expected, body) = match operation {
            Operation::Put {
                if_none_match: true,
                record,
                ..
            } => {
                // A tombstone counts as existing (D4a).
                if let Some(current) = existing {
                    return refused(412, "already-exists", Some(current));
                }
                (None, Some(record))
            }
            Operation::Put {
                if_match, record, ..
            } => (*if_match, Some(record)),
            Operation::Delete { if_match, .. } => (Some(*if_match), None),
        };
        if let Some(expected) = expected {
            let Some(current) = existing.clone() else {
                return refused(404, "unknown-record", None);
            };
            if current.version != expected {
                return refused(409, "version-conflict", Some(current));
            }
            // A delete retried over its own tombstone moves nothing.
            if body.is_none() && current.deleted {
                return BatchResult {
                    status: 200,
                    record: Some(current),
                    error: None,
                };
            }
        }

        state.seq += 1;
        let version = existing.as_ref().map_or(0, |current| current.version) + 1;
        let record = match body {
            Some(RecordBody {
                updated_at,
                nonce,
                ciphertext,
            }) => WireRecord {
                collection,
                id,
                version,
                seq: state.seq,
                updated_at: *updated_at,
                deleted: false,
                device: device.to_owned(),
                nonce: Some(nonce.clone()),
                ciphertext: Some(ciphertext.clone()),
            },
            // The tombstone keeps the time its deletion was made.
            None => WireRecord {
                collection,
                id,
                version,
                seq: state.seq,
                updated_at: deleted_at,
                deleted: true,
                device: device.to_owned(),
                nonce: None,
                ciphertext: None,
            },
        };
        state.records.insert(key, record.clone());
        BatchResult {
            status: if version == 1 { 201 } else { 200 },
            record: Some(record),
            error: None,
        }
    }
}

fn refused(status: u16, code: &str, record: Option<WireRecord>) -> BatchResult {
    BatchResult {
        status,
        record,
        error: Some(ErrorDetail {
            code: code.into(),
            retry_after: None,
        }),
    }
}

/// One machine's session on the server: the device the server stamps on its writes.
struct Link<'a> {
    server: &'a Server,
    device: &'a str,
}

impl Remote for Link<'_> {
    async fn page(
        &self,
        collection: &str,
        since: i64,
        resync: bool,
    ) -> Result<PageOutcome, AppError> {
        Ok(self.server.page(collection, since, resync))
    }

    async fn batch(&self, operations: &[Operation]) -> Result<Vec<BatchResult>, AppError> {
        Ok(operations
            .iter()
            .map(|operation| self.server.apply(self.device, operation))
            .collect())
    }
}

fn limits() -> Capabilities {
    Capabilities {
        protocol_versions: vec!["v1".into()],
        max_record_bytes: 1_000_000,
        max_batch_operations: 100,
        max_batch_bytes: 1_000_000,
        max_page_records: 2,
        account_quota_bytes: 20_971_520,
        tombstone_retention_days: 90,
        closing_on: None,
        features: vec![],
    }
}

struct Machine {
    device: String,
    keys: Keys,
    store: Store,
    /// What each collection's module holds, by collection name.
    lists: BTreeMap<String, Vec<Item>>,
    /// Ids this machine's module cannot read: its `fromSync` answers `null` for them.
    unreadable: HashSet<String>,
    /// The MixLab this machine runs. A record skipped under one is asked for again under the next.
    version: String,
}

impl Machine {
    async fn new(device: &str, master: &[u8; 32]) -> Self {
        Self {
            device: device.into(),
            keys: Keys {
                id: crypto::id_key(master),
                data: crypto::data_key(master),
            },
            store: Store::in_memory("https://sync.test").await.unwrap(),
            lists: BTreeMap::new(),
            unreadable: HashSet::new(),
            version: "1.0.0".into(),
        }
    }

    fn opaque(&self, name: &str) -> String {
        crypto::opaque_id(&self.keys.id, name)
    }

    fn link<'a>(&'a self, server: &'a Server) -> Link<'a> {
        Link {
            server,
            device: &self.device,
        }
    }

    fn list(&mut self, collection: &str) -> &mut Vec<Item> {
        self.lists.entry(collection.to_owned()).or_default()
    }

    /// An edit made here, as a module saves it.
    fn edit(&mut self, collection: &str, id: &str, data: Value) {
        let list = self.list(collection);
        match list.iter_mut().find(|item| item.id == id) {
            Some(item) => item.data = data,
            None => list.push(Item {
                id: id.into(),
                data,
            }),
        }
    }

    fn remove(&mut self, collection: &str, id: &str) {
        self.list(collection).retain(|item| item.id != id);
    }

    fn holds(&self, collection: &str, id: &str) -> Option<&Value> {
        self.lists
            .get(collection)
            .and_then(|list| list.iter().find(|item| item.id == id))
            .map(|item| &item.data)
    }

    /// A new release of this machine's MixLab, whose reader understands `readable`.
    fn upgrade(&mut self, version: &str, readable: &[&str]) {
        self.version = version.into();
        for id in readable {
            self.unreadable.remove(*id);
        }
    }

    /// `applySyncChanges`: a removed id dropped, an upsert replacing its item where it stands or
    /// appended — and an item the module cannot read left as it was, and named as skipped.
    fn write(&mut self, collection: &str, changes: Incoming) -> Vec<String> {
        let unreadable = self.unreadable.clone();
        let list = self.list(collection);
        let mut skipped = Vec::new();
        list.retain(|item| !changes.removed.contains(&item.id));
        for synced in changes.upserts {
            if unreadable.contains(&synced.id) {
                skipped.push(synced.id);
                continue;
            }
            match list.iter_mut().find(|item| item.id == synced.id) {
                Some(item) => *item = synced,
                None => list.push(synced),
            }
        }
        skipped
    }

    /// The local check that runs while the machine is offline: the change is noticed and stamped,
    /// and the request that would carry it never leaves.
    async fn notice_offline(&mut self, collection: &str, now: i64) {
        let items = self.list(collection).clone();
        lend::outgoing(&self.store, &self.keys, collection, &items, now)
            .await
            .unwrap();
    }

    /// `syncCollection`: this machine's changes noticed, every page pulled and written, then those
    /// changes pushed.
    async fn sync(&mut self, server: &Server, collection: &str, now: i64) {
        let items = self.list(collection).clone();
        lend::notice(&self.store, &self.keys, collection, &items, now)
            .await
            .unwrap();
        for _ in 0..MAX_PAGES {
            if !self.pull_page(server, collection).await {
                self.push(server, collection, now).await;
                return;
            }
        }
        panic!("{collection}: still pulling after {MAX_PAGES} pages — the sync never ends");
    }

    /// One page: fetched, opened, written by the module, landed and committed. Answers `more`.
    async fn pull_page(&mut self, server: &Server, collection: &str) -> bool {
        let opaque = self.opaque(collection);
        let fetched = engine::fetch(&self.link(server), &self.store, &opaque, &self.version)
            .await
            .unwrap();
        let opened = lend::incoming(
            &self.store,
            &self.keys,
            collection,
            &self.device,
            &fetched.records,
            fetched.resync && !fetched.more,
        )
        .await
        .unwrap();
        let skipped = if !opened.changes.upserts.is_empty() || !opened.changes.removed.is_empty() {
            self.write(collection, opened.changes)
        } else {
            Vec::new()
        };
        lend::land(
            &self.store,
            &self.keys,
            collection,
            &fetched.records,
            opened.agreements,
            &skipped,
            &self.version,
        )
        .await
        .unwrap();
        engine::commit(&self.store, &opaque, &fetched, &opened.unmet)
            .await
            .unwrap();
        fetched.more
    }

    /// `pushCollection`.
    async fn push(&mut self, server: &Server, collection: &str, now: i64) {
        let items = self.list(collection).clone();
        let (changes, agreed) = lend::outgoing(&self.store, &self.keys, collection, &items, now)
            .await
            .unwrap();
        if changes.is_empty() {
            return;
        }
        let pushed = engine::push(
            &self.link(server),
            &self.store,
            &limits(),
            &self.device,
            changes,
        )
        .await
        .unwrap();
        lend::settle_pushed(&self.store, &self.keys, collection, agreed, &pushed)
            .await
            .unwrap();
        if pushed.superseded.is_empty() {
            return;
        }
        let opened = lend::incoming(
            &self.store,
            &self.keys,
            collection,
            &self.device,
            &pushed.superseded,
            false,
        )
        .await
        .unwrap();
        let skipped = self.write(collection, opened.changes);
        lend::land(
            &self.store,
            &self.keys,
            collection,
            &pushed.superseded,
            opened.agreements,
            &skipped,
            &self.version,
        )
        .await
        .unwrap();
    }
}

async fn two_machines() -> (Machine, Machine) {
    let master = crypto::new_master_key();
    (
        Machine::new("device-a", &master).await,
        Machine::new("device-b", &master).await,
    )
}

// Every story below after the first two syncs a machine a second time after it pushes, before it
// changes anything again. **A push does not move the cursor**, so the next pull hands a machine
// back its own writes and the module writes them over whatever changed since — the first two
// tests. Caught up, the other stories are about the finding they name and not about that one.

/// L1 and L3: a machine's next pull brings back what it pushed itself, and the
/// module writes that older copy over an edit made since. No second machine and no network
/// failure — a focus or a request that runs a full sync before the 30-second local check is enough.
#[tokio::test]
async fn an_edit_is_not_overwritten_by_this_machines_own_earlier_push() {
    let server = Server::new(2);
    let (mut a, _) = two_machines().await;
    a.edit("c", "req", json!("draft"));
    a.sync(&server, "c", 10).await;

    a.edit("c", "req", json!("final"));
    a.sync(&server, "c", 20).await;

    assert_eq!(
        a.holds("c", "req"),
        Some(&json!("final")),
        "the pull brought back a's own push of `draft` and wrote it over `final`"
    );
}

/// The same, for a deletion: the item comes back as if nobody had removed it.
#[tokio::test]
async fn a_deletion_is_not_undone_by_this_machines_own_earlier_push() {
    let server = Server::new(2);
    let (mut a, _) = two_machines().await;
    a.edit("c", "req", json!("r"));
    a.sync(&server, "c", 10).await;

    a.remove("c", "req");
    a.sync(&server, "c", 20).await;

    assert_eq!(
        a.holds("c", "req"),
        None,
        "the pull brought back a's own push of `req` after it was removed here"
    );
}

/// L2: the conflict rule keeps the later edit, whichever machine made it — and a pull runs before
/// every push, so the rule has to hold there too, not only on a `409`.
#[tokio::test]
async fn a_newer_edit_made_offline_survives_a_pull_of_an_older_one() {
    let server = Server::new(2);
    let (mut a, mut b) = two_machines().await;
    b.edit("c", "conn", json!("first"));
    b.sync(&server, "c", 10).await;
    b.sync(&server, "c", 10).await;
    a.sync(&server, "c", 10).await;

    a.edit("c", "conn", json!("edited on a"));
    a.notice_offline("c", 100).await;
    b.edit("c", "conn", json!("edited on b"));
    b.sync(&server, "c", 50).await;

    a.sync(&server, "c", 200).await;
    b.sync(&server, "c", 300).await;

    assert_eq!(
        a.holds("c", "conn"),
        Some(&json!("edited on a")),
        "the pull replaced a's edit (stamped 100) with b's older one (stamped 50)"
    );
    assert_eq!(b.holds("c", "conn"), Some(&json!("edited on a")));
}

/// M1: a deletion whose tombstone is still on the server.
#[tokio::test]
async fn a_deletion_is_not_undone_by_a_resync_after_the_cursor_expired() {
    let server = Server::new(2);
    let (mut a, mut b) = two_machines().await;
    b.edit("c", "kept", json!("k"));
    b.edit("c", "reaped", json!("r"));
    b.sync(&server, "c", 10).await;
    b.sync(&server, "c", 10).await;
    a.sync(&server, "c", 10).await;

    // One tombstone reaped expires a's cursor; the deletion after it still has its tombstone.
    b.remove("c", "reaped");
    b.sync(&server, "c", 20).await;
    b.sync(&server, "c", 20).await;
    server.sweep();
    b.remove("c", "kept");
    b.sync(&server, "c", 30).await;

    a.sync(&server, "c", 40).await;
    assert_eq!(
        server.expired(),
        1,
        "the story needs a's cursor, and only a's, to expire"
    );

    let kept = server
        .record(&a.opaque("c"), &a.opaque("kept"))
        .expect("a tombstone for `kept`");
    assert!(
        kept.deleted,
        "a pushed `kept` back over its tombstone: the resync forgot what a had agreed on"
    );
    assert_eq!(a.holds("c", "kept"), None);
}

/// M2: a deletion whose tombstone the reaper has already taken. A resync meets every live record
/// and every surviving tombstone, so anything agreed here that it never meets was deleted.
#[tokio::test]
async fn a_deletion_whose_tombstone_was_reaped_is_not_undone_by_a_resync() {
    let server = Server::new(2);
    let (mut a, mut b) = two_machines().await;
    b.edit("c", "kept", json!("k"));
    b.edit("c", "reaped", json!("r"));
    b.sync(&server, "c", 10).await;
    b.sync(&server, "c", 10).await;
    a.sync(&server, "c", 10).await;

    b.remove("c", "reaped");
    b.sync(&server, "c", 20).await;
    server.sweep();

    a.sync(&server, "c", 40).await;
    assert_eq!(server.expired(), 1, "the story needs a's cursor to expire");

    assert_eq!(
        server.record(&a.opaque("c"), &a.opaque("reaped")),
        None,
        "a created `reaped` again: the resync did not tell it the item was gone"
    );
    assert_eq!(a.holds("c", "reaped"), None);
}

/// M3: a cursor handed out by a resync is current; one collection's quiet must not
/// be expired for ever by a tombstone reaped in another.
#[tokio::test]
async fn a_quiet_collection_resyncs_once_and_not_on_every_pull() {
    let server = Server::new(2);
    let (mut a, mut b) = two_machines().await;
    b.edit("quiet", "q", json!("q"));
    b.sync(&server, "quiet", 10).await;
    a.sync(&server, "quiet", 10).await;

    b.edit("busy", "gone", json!("g"));
    b.sync(&server, "busy", 20).await;
    b.sync(&server, "busy", 20).await;
    b.remove("busy", "gone");
    b.sync(&server, "busy", 30).await;
    server.sweep();

    a.sync(&server, "quiet", 40).await;
    a.sync(&server, "quiet", 50).await;
    a.sync(&server, "quiet", 60).await;
    assert_eq!(
        server.expired(),
        1,
        "each pull of `quiet` was refused: its cursor stops at its own last row, below the reaped seq"
    );
}

/// M4: past a single page, after a restart the second page's cursor is below the
/// reaped seq again, so the resync starts over — and over.
#[tokio::test]
async fn a_resync_longer_than_a_page_reaches_the_end() {
    let server = Server::new(2);
    let (mut a, mut b) = two_machines().await;
    b.edit("c", "1", json!(1));
    b.sync(&server, "c", 10).await;
    b.sync(&server, "c", 10).await;
    a.sync(&server, "c", 10).await;

    for n in 2..=5 {
        b.edit("c", &n.to_string(), json!(n));
    }
    b.edit("c", "gone", json!("g"));
    b.sync(&server, "c", 20).await;
    b.sync(&server, "c", 20).await;
    b.remove("c", "gone");
    b.sync(&server, "c", 30).await;
    server.sweep();

    a.sync(&server, "c", 40).await;
    for n in 1..=5 {
        assert_eq!(a.holds("c", &n.to_string()), Some(&json!(n)));
    }
}

/// L4: `applySyncChanges` leaves an item it cannot read as it was; agreeing on it
/// anyway turns that care into a deletion on every machine.
#[tokio::test]
async fn a_record_the_module_cannot_read_is_not_deleted_by_the_machine_that_skipped_it() {
    let server = Server::new(2);
    let (mut a, mut b) = two_machines().await;
    a.unreadable.insert("host".into());
    b.edit("c", "host", json!({ "shape": "from a newer version" }));
    b.sync(&server, "c", 10).await;

    a.sync(&server, "c", 20).await;

    let host = server
        .record(&a.opaque("c"), &a.opaque("host"))
        .expect("the record b wrote");
    assert!(
        !host.deleted,
        "a deleted a record its module never wrote: the pull agreed on it anyway"
    );
}

/// A machine that skipped a record `host` from a newer MixLab, its cursor long past it.
async fn a_skips_what_b_wrote(server: &Server) -> (Machine, Machine) {
    let (mut a, mut b) = two_machines().await;
    a.unreadable.insert("host".into());
    b.edit("c", "host", json!({ "shape": "from a newer version" }));
    b.sync(server, "c", 10).await;
    a.sync(server, "c", 20).await;
    assert_eq!(a.holds("c", "host"), None);
    (a, b)
}

/// Once a release can read it, the record is delivered again, with nobody editing it and the cursor
/// long past it.
#[tokio::test]
async fn a_skipped_record_is_delivered_after_an_upgrade() {
    let server = Server::new(2);
    let (mut a, _b) = a_skips_what_b_wrote(&server).await;

    a.upgrade("1.1.0", &["host"]);
    a.sync(&server, "c", 30).await;

    assert_eq!(
        a.holds("c", "host"),
        Some(&json!({ "shape": "from a newer version" })),
        "the upgraded reader never met the record: nothing delivered it again"
    );
}

/// With no new release, nothing is asked for again: the reader still cannot read the record, and
/// a resync on every sync would download the collection for nothing.
#[tokio::test]
async fn the_same_version_does_not_ask_again() {
    let server = Server::new(2);
    let (mut a, _b) = a_skips_what_b_wrote(&server).await;
    let asked = server.reads_from_start();

    a.sync(&server, "c", 30).await;
    a.sync(&server, "c", 40).await;

    assert_eq!(
        server.reads_from_start(),
        asked,
        "a resynced with nothing new to read"
    );
}

/// A release that still cannot read the record leaves it owed, asks once, and the release after
/// it asks again.
#[tokio::test]
async fn a_record_still_unreadable_waits_for_the_next_release() {
    let server = Server::new(2);
    let (mut a, _b) = a_skips_what_b_wrote(&server).await;

    a.upgrade("1.1.0", &[]);
    a.sync(&server, "c", 30).await;
    assert_eq!(a.holds("c", "host"), None);
    let asked = server.reads_from_start();
    a.sync(&server, "c", 40).await;
    assert_eq!(
        server.reads_from_start(),
        asked,
        "1.1.0 asked a second time"
    );

    a.upgrade("1.2.0", &["host"]);
    a.sync(&server, "c", 50).await;
    assert_eq!(
        a.holds("c", "host"),
        Some(&json!({ "shape": "from a newer version" }))
    );
}

/// L3: an edit noticed in the same second as this machine's own earlier push. The conflict rule's
/// exact tie keeps the remote copy, and the remote copy is the old push coming back; content, not
/// time, says it is not news.
#[tokio::test]
async fn an_edit_in_the_same_second_as_its_own_push_survives() {
    let server = Server::new(2);
    let (mut a, _) = two_machines().await;
    a.edit("c", "req", json!("draft"));
    a.sync(&server, "c", 10).await;

    a.edit("c", "req", json!("final"));
    a.sync(&server, "c", 10).await;

    assert_eq!(a.holds("c", "req"), Some(&json!("final")));
}

/// M2: a change stamped here is newer than a deletion old enough to be reaped, so a resync that
/// never meets its item keeps it, and pushes it as a creation.
#[tokio::test]
async fn an_edit_a_resync_did_not_meet_is_created_again() {
    let server = Server::new(2);
    let (mut a, mut b) = two_machines().await;
    b.edit("c", "kept", json!("k"));
    b.edit("c", "reaped", json!("r"));
    b.sync(&server, "c", 10).await;
    b.sync(&server, "c", 10).await;
    a.sync(&server, "c", 10).await;

    b.remove("c", "reaped");
    b.sync(&server, "c", 20).await;
    server.sweep();

    a.edit("c", "reaped", json!("edited on a"));
    a.notice_offline("c", 30).await;
    a.sync(&server, "c", 40).await;

    let record = server
        .record(&a.opaque("c"), &a.opaque("reaped"))
        .expect("created again");
    assert!(!record.deleted);
    assert_eq!(a.holds("c", "reaped"), Some(&json!("edited on a")));
}

/// M1: a resync interrupted after its first page resumes as a resync — flagged, and ending by
/// removing what it never met — rather than as an ordinary pull that has forgotten why.
#[tokio::test]
async fn a_resync_interrupted_after_one_page_still_ends() {
    let server = Server::new(2);
    let (mut a, mut b) = two_machines().await;
    for n in 1..=4 {
        b.edit("c", &n.to_string(), json!(n));
    }
    b.edit("c", "reaped", json!("r"));
    b.sync(&server, "c", 10).await;
    b.sync(&server, "c", 10).await;
    a.sync(&server, "c", 10).await;

    b.remove("c", "reaped");
    b.sync(&server, "c", 20).await;
    server.sweep();

    assert!(
        a.pull_page(&server, "c").await,
        "the resync has a page after this one"
    );
    // Quit here: nothing is held in memory across a restart, only the store.
    a.sync(&server, "c", 40).await;

    assert_eq!(a.holds("c", "reaped"), None);
    for n in 1..=4 {
        assert_eq!(a.holds("c", &n.to_string()), Some(&json!(n)));
    }
    assert_eq!(
        server.expired(),
        1,
        "resumed with resync=1, not expired again"
    );
}

/// A deletion made later than an edit wins, whichever reaches the server first. While the tombstone
/// kept the replaced version's time, the edit that arrived second came back to life.
async fn a_later_deletion_against_an_earlier_edit(
    deletion_first: bool,
) -> (Server, Machine, Machine) {
    let server = Server::new(2);
    let (mut a, mut b) = two_machines().await;
    b.edit("c", "x", json!("v0"));
    b.sync(&server, "c", 10).await;
    b.sync(&server, "c", 10).await;
    a.sync(&server, "c", 10).await;

    b.edit("c", "x", json!("edited on b"));
    b.notice_offline("c", 150).await;
    a.remove("c", "x");
    a.notice_offline("c", 200).await;
    if deletion_first {
        a.sync(&server, "c", 250).await;
        b.sync(&server, "c", 300).await;
    } else {
        b.sync(&server, "c", 250).await;
        a.sync(&server, "c", 300).await;
    }
    a.sync(&server, "c", 400).await;
    b.sync(&server, "c", 400).await;
    (server, a, b)
}

#[tokio::test]
async fn a_later_deletion_wins_when_it_arrives_first() {
    let (server, a, b) = a_later_deletion_against_an_earlier_edit(true).await;
    let x = server
        .record(&a.opaque("c"), &a.opaque("x"))
        .expect("a tombstone");
    assert!(
        x.deleted,
        "the edit arriving second brought the record back"
    );
    assert_eq!(a.holds("c", "x"), None);
    assert_eq!(b.holds("c", "x"), None);
}

#[tokio::test]
async fn a_later_deletion_wins_when_it_arrives_second() {
    let (server, a, b) = a_later_deletion_against_an_earlier_edit(false).await;
    let x = server
        .record(&a.opaque("c"), &a.opaque("x"))
        .expect("a tombstone");
    assert!(x.deleted);
    assert_eq!(a.holds("c", "x"), None);
    assert_eq!(b.holds("c", "x"), None);
}
