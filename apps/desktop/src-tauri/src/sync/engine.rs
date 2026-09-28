//! Pull and push, over a [`Remote`] and a [`Store`].
//!
//! **Everything here is sealed.** A caller seals before pushing and opens after fetching; this file
//! moves ciphertext and settles conflicts from the two fields D4's rule reads, so it never needs a
//! key and never sees a plaintext. **Callers fetch and commit before they push**, which is what
//! lets a push trust that a record it has never seen is one the server does not have.

use std::collections::{BTreeMap, HashSet};

use super::chunk::chunk;
use super::merge::{resolve, Keep};
use super::store::Store;
use super::transport::{refusal, AskHeads, PageOutcome, Remote};
use super::wire::{BatchResult, Capabilities, ErrorBody, Operation, Page, RecordBody, WireRecord};
use crate::error::AppError;

/// How many times one push goes round a conflict before giving up. Two machines writing one
/// record in the same second settle in one round; the rest is for a third arriving meanwhile.
const MAX_ROUNDS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Write { nonce: String, ciphertext: String },
    Delete,
}

/// One change this machine wants the server to have. `collection` and `id` are already opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub collection: String,
    pub id: String,
    pub updated_at: i64,
    pub change: Change,
}

/// What a push did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pushed {
    /// Written as this machine had them.
    pub accepted: usize,
    /// Conflicts the other side won. **The caller writes these down and then lands them**
    /// (`lend::land`), exactly as it would a pulled page: this machine's version of each lost, and
    /// until the winner is written here nothing about the record is recorded.
    pub superseded: Vec<WireRecord>,
    /// Opaque ids the server wrote as this machine had them: the only changes to agree on.
    pub landed: Vec<String>,
    /// The first refusal, of one entry or of a whole request. Its change keeps its stamp and is sent
    /// again at the next push (T178c, C2).
    pub error: Option<AppError>,
}

/// One page read and not yet recorded. The caller has the module write `records`, lands them
/// (`lend::land`), and only then [`commit`]s — so a page nobody wrote is read again next time,
/// rather than recorded as seen and never delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub records: Vec<WireRecord>,
    pub next_since: i64,
    pub more: bool,
    /// This page belongs to a resync: a read that began at 0 after the server forgot this
    /// machine's cursor. The page with `more: false` ends it, and forgets whatever the resync
    /// never met (T178b, M2).
    pub resync: bool,
}

/// The next page of `collection` after this machine's cursor. **Moves nothing**: see [`Fetched`].
/// A `410` starts a resync (M1). Every later page of it says `resync=1`, so it is not expired
/// halfway (M4). So does a record another `version` of the app skipped: this one may read it, and
/// only a resync delivers it again (T178d).
pub async fn fetch<R: Remote>(
    remote: &R,
    store: &Store,
    collection: &str,
    version: &str,
) -> Result<Fetched, AppError> {
    if store.owed_elsewhere(collection, version).await? {
        store.begin_resync(collection).await?;
        store.renew_owed(collection, version).await?;
    }
    let since = store.since(collection).await?;
    let resync = store.resyncing(collection).await?;
    match remote.page(collection, since, resync && since > 0).await? {
        PageOutcome::Page(page) => return Ok(fetched(page, resync)),
        PageOutcome::CursorExpired => store.begin_resync(collection).await?,
    }
    match remote.page(collection, 0, false).await? {
        PageOutcome::Page(page) => Ok(fetched(page, true)),
        // Expired again from the beginning: a server bug, and not one to loop on.
        PageOutcome::CursorExpired => Err(err!("error.syncServerRefused", code = "cursor-expired")),
    }
}

fn fetched(page: Page, resync: bool) -> Fetched {
    Fetched {
        records: page.records,
        next_since: page.next_since,
        more: page.more,
        resync,
    }
}

/// Move the cursor past a page the caller has written and landed. A resync page marks what it
/// met; the last one forgets `unmet` and ends the resync (M2).
pub async fn commit(
    store: &Store,
    collection: &str,
    fetched: &Fetched,
    unmet: &[String],
) -> Result<(), AppError> {
    if fetched.resync {
        let met: Vec<String> = fetched
            .records
            .iter()
            .map(|record| record.id.clone())
            .collect();
        store.mark_met(collection, &met).await?;
        if !fetched.more {
            store.end_resync(collection, unmet).await?;
        }
    }
    store.set_since(collection, fetched.next_since).await
}

/// Which of `collections` (opaque) a full run must pull (T189, D4). A cursor at 0, a resync and a
/// record owed under another version are stale without asking: `fetch` does more than read from
/// the cursor for each, and the server sees none of them. So is a change still stamped: its push
/// may have landed unheard, and only the pull of that echo agrees on it. The rest are asked in as
/// few requests as the server takes, and **a collection the answer does not name has its cursor
/// moved to `nextSince`**: since the cursor, only this machine wrote to it, and its push
/// remembered those writes (D3). Never backwards.
pub async fn stale<A: AskHeads>(
    remote: &A,
    store: &Store,
    limits: &Capabilities,
    collections: &[String],
    version: &str,
) -> Result<Vec<String>, AppError> {
    let mut stale = Vec::new();
    let mut asked = Vec::new();
    for collection in collections {
        let since = store.since(collection).await?;
        if since == 0
            || store.resyncing(collection).await?
            || store.owed_elsewhere(collection, version).await?
            || store.unlanded(collection).await?
        {
            stale.push(collection.clone());
        } else {
            asked.push((collection.clone(), since));
        }
    }
    let per_request = usize::try_from(limits.max_batch_operations)
        .unwrap_or(usize::MAX)
        .max(1);
    for part in asked.chunks(per_request) {
        let cursors: BTreeMap<String, i64> = part.iter().cloned().collect();
        let heads = remote.heads(&cursors).await?;
        let named: HashSet<&str> = heads.stale.iter().map(String::as_str).collect();
        for (collection, since) in part {
            if named.contains(collection.as_str()) {
                stale.push(collection.clone());
            } else {
                store
                    .set_since(collection, heads.next_since.max(*since))
                    .await?;
            }
        }
    }
    Ok(stale)
}

/// Keep the first failure; later ones say less than it does.
fn refused(pushed: &mut Pushed, error: AppError) {
    if pushed.error.is_none() {
        pushed.error = Some(error);
    }
}

/// This machine's changes, in batches, settling each conflict by D4's rule. **Every entry of every
/// batch is read** (T178c, C2): one refused record says nothing about the others, and an entry the
/// server wrote is remembered whatever came before it. Only a request that fails as a whole stops
/// the push, and what it had not sent stays stamped for the next one.
pub async fn push<R: Remote>(
    remote: &R,
    store: &Store,
    limits: &Capabilities,
    device: &str,
    changes: Vec<Outgoing>,
) -> Result<Pushed, AppError> {
    let mut pushed = Pushed::default();
    let mut pending = changes;

    for _ in 0..MAX_ROUNDS {
        if pending.is_empty() {
            return Ok(pushed);
        }

        let mut sent = Vec::new();
        let mut operations = Vec::new();
        for change in &pending {
            match operation_for(store, change).await? {
                Some(operation) => {
                    sent.push(change);
                    operations.push(operation);
                }
                // A deletion of something the server never told this machine about: there is
                // nothing there to delete.
                None => pushed.accepted += 1,
            }
        }

        let mut retry = Vec::new();
        let mut offset = 0;
        for batch in chunk(operations, limits)? {
            let changes = &sent[offset..offset + batch.len()];
            offset += batch.len();
            let results = match remote.batch(&batch).await {
                Ok(results) if results.len() == batch.len() => results,
                Ok(_) => {
                    refused(&mut pushed, err!("error.syncServerAnswerUnreadable"));
                    return Ok(pushed);
                }
                Err(error) => {
                    refused(&mut pushed, error);
                    return Ok(pushed);
                }
            };
            for (result, change) in results.into_iter().zip(changes) {
                let BatchResult {
                    status,
                    record,
                    error,
                } = result;
                match (status, record) {
                    (200 | 201, Some(record)) => {
                        store.remember(&record).await?;
                        pushed.accepted += 1;
                        pushed.landed.push(change.id.clone());
                    }
                    // This machine's own write, which the server kept before this machine could
                    // remember it (T189, D5): the same device at the same stamp. The local item is
                    // this device's latest word on the record, so it is written over, not handed
                    // back as an edit that replaced it.
                    (409 | 412, Some(current))
                        if current.device == device && current.updated_at == change.updated_at =>
                    {
                        store.remember(&current).await?;
                        retry.push((*change).clone());
                    }
                    (409 | 412, Some(current)) => match resolve(
                        change.updated_at,
                        device,
                        current.updated_at,
                        &current.device,
                    ) {
                        // The retry replaces the server's version, so it says `If-Match` with it.
                        Keep::Local => {
                            store.remember(&current).await?;
                            retry.push((*change).clone());
                        }
                        // Not remembered: the version it reveals is recorded with the winner, once
                        // the caller has written it down (`lend::land`). Remembered here, a failed
                        // write would leave the next push carrying it — no `409`, and the older
                        // edit replaces the newer one (D4).
                        Keep::Remote => pushed.superseded.push(current),
                    },
                    // Deleting what the server no longer has is the outcome that was wanted.
                    (404, _) if change.change == Change::Delete => pushed.accepted += 1,
                    _ => refused(
                        &mut pushed,
                        error
                            .map(|error| refusal(&ErrorBody { error }))
                            .unwrap_or_else(|| err!("error.syncServerAnswerUnreadable")),
                    ),
                }
            }
        }
        pending = retry;
    }

    if !pending.is_empty() {
        refused(&mut pushed, err!("error.syncConflictUnresolved"));
    }
    Ok(pushed)
}

async fn operation_for(store: &Store, change: &Outgoing) -> Result<Option<Operation>, AppError> {
    let seen = store.seen(&change.collection, &change.id).await?;
    Ok(match &change.change {
        Change::Write { nonce, ciphertext } => Some(Operation::Put {
            collection: change.collection.clone(),
            id: change.id.clone(),
            if_match: seen.map(|seen| seen.version),
            if_none_match: seen.is_none(),
            record: RecordBody {
                updated_at: change.updated_at,
                nonce: nonce.clone(),
                ciphertext: ciphertext.clone(),
            },
        }),
        Change::Delete => seen.map(|seen| Operation::Delete {
            collection: change.collection.clone(),
            id: change.id.clone(),
            if_match: seen.version,
            // The time it was first noticed: D4 weighs a deletion like any edit (T178c, C1).
            updated_at: change.updated_at,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::transport::AskHeads;
    use crate::sync::wire::{ErrorDetail, Heads};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    const VERSION: &str = "1.0.0";

    /// Just enough of `/v1` in a `Mutex` to exercise the engine: versions, `seq`, the writing
    /// device, `409`, `412`, paging two at a time, and a cursor the server can forget.
    struct Fake {
        device: String,
        state: Mutex<State>,
    }

    #[derive(Default)]
    struct State {
        records: BTreeMap<(String, String), WireRecord>,
        seq: i64,
        /// A cursor below this is one the server has forgotten.
        forgotten_below: i64,
        /// The `resync` flag of every page asked for, in order.
        resync_flags: Vec<bool>,
        /// How many `heads` requests were made.
        heads_asked: usize,
    }

    impl Fake {
        fn new(device: &str) -> Self {
            Self {
                device: device.into(),
                state: Mutex::new(State::default()),
            }
        }

        /// A record written by some other machine, as if it had arrived first.
        fn holds(&self, id: &str, version: i64, updated_at: i64, device: &str) {
            let mut state = self.state.lock().unwrap();
            state.seq += 1;
            let seq = state.seq;
            state.records.insert(
                ("c".into(), id.into()),
                WireRecord {
                    collection: "c".into(),
                    id: id.into(),
                    version,
                    seq,
                    updated_at,
                    deleted: false,
                    device: device.into(),
                    nonce: Some("n".into()),
                    ciphertext: Some("theirs".into()),
                },
            );
        }

        fn current(&self, id: &str) -> WireRecord {
            self.state.lock().unwrap().records[&("c".to_string(), id.to_string())].clone()
        }
    }

    fn conflict(status: u16, record: WireRecord) -> BatchResult {
        BatchResult {
            status,
            record: Some(record),
            error: Some(ErrorDetail {
                code: "version-conflict".into(),
                retry_after: None,
            }),
        }
    }

    impl Remote for Fake {
        async fn page(
            &self,
            collection: &str,
            since: i64,
            resync: bool,
        ) -> Result<PageOutcome, AppError> {
            let mut state = self.state.lock().unwrap();
            state.resync_flags.push(resync);
            if since != 0 && since < state.forgotten_below && !resync {
                return Ok(PageOutcome::CursorExpired);
            }
            let mut records: Vec<_> = state
                .records
                .values()
                .filter(|record| record.collection == collection && record.seq > since)
                .cloned()
                .collect();
            records.sort_by_key(|record| record.seq);
            let more = records.len() > 2;
            records.truncate(2);
            let next_since = records.last().map_or(since, |record| record.seq);
            Ok(PageOutcome::Page(Page {
                records,
                next_since,
                more,
            }))
        }

        async fn batch(&self, operations: &[Operation]) -> Result<Vec<BatchResult>, AppError> {
            let mut state = self.state.lock().unwrap();
            let mut results = Vec::new();
            for operation in operations {
                let (collection, id) = match operation {
                    Operation::Put { collection, id, .. }
                    | Operation::Delete { collection, id, .. } => (collection.clone(), id.clone()),
                };
                let key = (collection.clone(), id.clone());
                let existing = state.records.get(&key).cloned();
                let expected = match operation {
                    Operation::Put {
                        if_none_match: true,
                        ..
                    } => {
                        if let Some(current) = existing.clone() {
                            results.push(conflict(412, current));
                            continue;
                        }
                        0
                    }
                    Operation::Put { if_match, .. } => if_match.unwrap_or(0),
                    Operation::Delete { if_match, .. } => *if_match,
                };
                if let Some(current) = existing.clone() {
                    if current.version != expected {
                        results.push(conflict(409, current));
                        continue;
                    }
                }
                state.seq += 1;
                let seq = state.seq;
                let version = existing.as_ref().map_or(0, |current| current.version) + 1;
                let record = match operation {
                    Operation::Put { record, .. } => WireRecord {
                        collection,
                        id,
                        version,
                        seq,
                        updated_at: record.updated_at,
                        deleted: false,
                        device: self.device.clone(),
                        nonce: Some(record.nonce.clone()),
                        ciphertext: Some(record.ciphertext.clone()),
                    },
                    Operation::Delete { updated_at, .. } => WireRecord {
                        collection,
                        id,
                        version,
                        seq,
                        updated_at: *updated_at,
                        deleted: true,
                        device: self.device.clone(),
                        nonce: None,
                        ciphertext: None,
                    },
                };
                state.records.insert(key, record.clone());
                results.push(BatchResult {
                    status: if version == 1 { 201 } else { 200 },
                    record: Some(record),
                    error: None,
                });
            }
            Ok(results)
        }
    }

    fn limits() -> Capabilities {
        Capabilities {
            protocol_versions: vec!["v1".into()],
            max_record_bytes: 1_000_000,
            max_batch_operations: 2,
            max_batch_bytes: 1_000_000,
            max_page_records: 2,
            account_quota_bytes: 20_971_520,
            tombstone_retention_days: 90,
            closing_on: None,
            features: vec![],
        }
    }

    fn write(id: &str, updated_at: i64, ciphertext: &str) -> Outgoing {
        Outgoing {
            collection: "c".into(),
            id: id.into(),
            updated_at,
            change: Change::Write {
                nonce: "n".into(),
                ciphertext: ciphertext.into(),
            },
        }
    }

    /// Every page, each recorded before the next is read — the shell's order, minus the module.
    async fn pull_all(server: &Fake, store: &Store) -> Vec<WireRecord> {
        let mut all = Vec::new();
        loop {
            let fetched = fetch(server, store, "c", VERSION).await.unwrap();
            for record in &fetched.records {
                store.remember(record).await.unwrap();
            }
            commit(store, "c", &fetched, &[]).await.unwrap();
            all.extend(fetched.records.iter().cloned());
            if !fetched.more {
                return all;
            }
        }
    }

    impl AskHeads for Fake {
        async fn heads(&self, cursors: &BTreeMap<String, i64>) -> Result<Heads, AppError> {
            let mut state = self.state.lock().unwrap();
            state.heads_asked += 1;
            let stale = cursors
                .iter()
                .filter(|(collection, since)| {
                    let since = **since;
                    (since > 0 && since < state.forgotten_below)
                        || state.records.values().any(|record| {
                            &record.collection == *collection
                                && record.seq > since
                                && (since == 0 || record.device != self.device)
                        })
                })
                .map(|(collection, _)| collection.clone())
                .collect();
            Ok(Heads {
                stale,
                next_since: state.seq,
            })
        }
    }

    fn opaque(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    /// T189, D3: what only this machine wrote is not news, and the cursor steps over its echo.
    #[tokio::test]
    async fn a_collection_only_this_machine_wrote_is_not_stale_and_its_cursor_moves() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        server.holds("a", 1, 100, "theirs");
        pull_all(&server, &store).await;
        push(
            &server,
            &store,
            &limits(),
            "mine",
            vec![write("b", 200, "x")],
        )
        .await
        .unwrap();

        let stale = stale(&server, &store, &limits(), &opaque(&["c"]), VERSION)
            .await
            .unwrap();
        assert!(stale.is_empty());
        assert_eq!(store.since("c").await.unwrap(), 2);
    }

    #[tokio::test]
    async fn another_machines_write_is_stale_and_the_cursor_stays() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        server.holds("a", 1, 100, "theirs");
        pull_all(&server, &store).await;
        server.holds("b", 1, 100, "theirs");

        let stale = stale(&server, &store, &limits(), &opaque(&["c"]), VERSION)
            .await
            .unwrap();
        assert_eq!(stale, opaque(&["c"]));
        assert_eq!(store.since("c").await.unwrap(), 1);
    }

    /// T189, D4 step 2: the states in which `fetch` does more than read from the cursor.
    #[tokio::test]
    async fn a_zero_cursor_a_resync_and_an_owed_record_are_stale_without_asking() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        store.set_since("resyncing", 5).await.unwrap();
        store.begin_resync("resyncing").await.unwrap();
        store.set_since("owed", 5).await.unwrap();
        store
            .owe("owed", &["x".to_string()], "0.0.1-older")
            .await
            .unwrap();

        let mut stale = stale(
            &server,
            &store,
            &limits(),
            &opaque(&["zero", "resyncing", "owed"]),
            VERSION,
        )
        .await
        .unwrap();
        stale.sort();
        assert_eq!(stale, opaque(&["owed", "resyncing", "zero"]));
        assert_eq!(server.state.lock().unwrap().heads_asked, 0);
    }

    #[tokio::test]
    async fn heads_are_asked_in_chunks_the_server_takes() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        for collection in ["c1", "c2", "c3"] {
            store.set_since(collection, 1).await.unwrap();
        }
        // `limits()` allows two operations per batch.
        stale(
            &server,
            &store,
            &limits(),
            &opaque(&["c1", "c2", "c3"]),
            VERSION,
        )
        .await
        .unwrap();
        assert_eq!(server.state.lock().unwrap().heads_asked, 2);
    }

    /// A server that answers a smaller `nextSince` than this machine's cursor (restored from a
    /// backup, say) must not wind the cursor back over rows already landed.
    #[tokio::test]
    async fn the_cursor_never_moves_backwards() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        store.set_since("c", 7).await.unwrap();
        // The fake's `seq` is 0, so it answers `nextSince: 0`.
        stale(&server, &store, &limits(), &opaque(&["c"]), VERSION)
            .await
            .unwrap();
        assert_eq!(store.since("c").await.unwrap(), 7);
    }

    /// A change still stamped never landed: the server may hold it while this machine never
    /// remembered it — a creation the app died before recording, then deleted here. Only the pull
    /// of its echo agrees on it, so the deletion can follow; the cursor must not step over it.
    #[tokio::test]
    async fn a_change_that_never_landed_keeps_its_collection_stale() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        store.set_since("c", 1).await.unwrap();
        store.stamp("c", "a", "h", 100).await.unwrap();

        let stale = stale(&server, &store, &limits(), &opaque(&["c"]), VERSION)
            .await
            .unwrap();
        assert_eq!(stale, opaque(&["c"]));
        assert_eq!(server.state.lock().unwrap().heads_asked, 0);
        assert_eq!(store.since("c").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn a_first_push_creates_and_remembers_what_it_created() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        let pushed = push(
            &server,
            &store,
            &limits(),
            "mine",
            (0..5).map(|n| write(&n.to_string(), 100, "x")).collect(),
        )
        .await
        .unwrap();
        assert_eq!(pushed.accepted, 5);
        assert!(pushed.superseded.is_empty());
        assert_eq!(pushed.landed.len(), 5);
        assert_eq!(pushed.error, None);
        assert_eq!(
            store.seen("c", "3").await.unwrap().map(|seen| seen.version),
            Some(1)
        );
    }

    #[tokio::test]
    async fn a_second_push_says_if_match_with_what_it_remembered() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        push(
            &server,
            &store,
            &limits(),
            "mine",
            vec![write("a", 100, "one")],
        )
        .await
        .unwrap();
        push(
            &server,
            &store,
            &limits(),
            "mine",
            vec![write("a", 200, "two")],
        )
        .await
        .unwrap();
        assert_eq!(server.current("a").version, 2);
        assert_eq!(server.current("a").ciphertext.as_deref(), Some("two"));
    }

    #[tokio::test]
    async fn the_later_write_wins_a_conflict_and_goes_round_again() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        server.holds("a", 1, 100, "theirs");
        let pushed = push(
            &server,
            &store,
            &limits(),
            "mine",
            vec![write("a", 200, "mine")],
        )
        .await
        .unwrap();
        assert_eq!(pushed.accepted, 1);
        assert_eq!(server.current("a").ciphertext.as_deref(), Some("mine"));
        assert_eq!(server.current("a").device, "mine");
    }

    #[tokio::test]
    async fn the_earlier_write_is_superseded_and_handed_back() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        server.holds("a", 1, 300, "theirs");
        let pushed = push(
            &server,
            &store,
            &limits(),
            "mine",
            vec![write("a", 200, "mine")],
        )
        .await
        .unwrap();
        assert_eq!(pushed.accepted, 0);
        assert_eq!(pushed.superseded.len(), 1);
        assert_eq!(pushed.superseded[0].ciphertext.as_deref(), Some("theirs"));
        assert_eq!(server.current("a").ciphertext.as_deref(), Some("theirs"));
    }

    /// The loser learns nothing until it has written the winner down. Learnt early, a failed write
    /// would leave the next push carrying the winner's version: no `409`, and the older edit
    /// replaces the newer one without a conflict ever being seen (D4).
    #[tokio::test]
    async fn a_lost_conflict_is_met_again_until_its_winner_is_written() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        server.holds("a", 1, 300, "theirs");
        let edit = || vec![write("a", 200, "mine")];

        let first = push(&server, &store, &limits(), "mine", edit())
            .await
            .unwrap();
        assert_eq!(first.superseded.len(), 1);
        assert_eq!(store.seen("c", "a").await.unwrap(), None);

        // The winner was never written here, so the same edit is pushed again.
        let second = push(&server, &store, &limits(), "mine", edit())
            .await
            .unwrap();
        assert_eq!(second.superseded.len(), 1);
        assert_eq!(server.current("a").version, 1);
        assert_eq!(server.current("a").ciphertext.as_deref(), Some("theirs"));
    }

    /// T189, D5: the server kept this machine's write and the app died before remembering it. The
    /// same device at the same `updatedAt` is this machine's own word: written again, not handed
    /// back as a newer edit that replaced it.
    #[tokio::test]
    async fn this_machines_own_unremembered_write_is_written_again() {
        let (server, store) = (Fake::new("mine"), Store::in_memory("s").await.unwrap());
        server.holds("a", 1, 100, "mine");

        let pushed = push(
            &server,
            &store,
            &limits(),
            "mine",
            vec![write("a", 100, "again")],
        )
        .await
        .unwrap();

        assert!(pushed.superseded.is_empty());
        assert_eq!(pushed.landed, vec!["a".to_string()]);
        assert_eq!(server.current("a").version, 2);
        assert_eq!(server.current("a").ciphertext.as_deref(), Some("again"));
    }

    #[tokio::test]
    async fn a_tie_goes_to_the_greater_device() {
        let (server, store) = (Fake::new("zeta"), Store::in_memory("s").await.unwrap());
        server.holds("a", 1, 100, "alpha");
        let pushed = push(
            &server,
            &store,
            &limits(),
            "zeta",
            vec![write("a", 100, "zeta")],
        )
        .await
        .unwrap();
        assert_eq!(pushed.accepted, 1);
        assert_eq!(server.current("a").device, "zeta");
    }

    #[tokio::test]
    async fn every_page_is_read_and_the_cursor_moves_after_each() {
        let (server, store) = (Fake::new("theirs"), Store::in_memory("s").await.unwrap());
        for n in 0..5 {
            server.holds(&n.to_string(), 1, 100, "theirs");
        }
        assert_eq!(pull_all(&server, &store).await.len(), 5);
        assert_eq!(store.since("c").await.unwrap(), 5);
    }

    /// A fetch moves nothing: a page that was never committed is the page the next fetch returns.
    #[tokio::test]
    async fn a_page_not_committed_is_read_again() {
        let (server, store) = (Fake::new("theirs"), Store::in_memory("s").await.unwrap());
        server.holds("a", 1, 100, "theirs");
        let first = fetch(&server, &store, "c", VERSION).await.unwrap();
        let again = fetch(&server, &store, "c", VERSION).await.unwrap();
        assert_eq!(first, again);
        assert_eq!(store.since("c").await.unwrap(), 0);
        assert_eq!(store.seen("c", "a").await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_forgotten_cursor_starts_a_resync_that_keeps_what_was_agreed() {
        let (server, store) = (Fake::new("theirs"), Store::in_memory("s").await.unwrap());
        server.holds("a", 1, 100, "theirs");
        store.set_since("c", 1).await.unwrap();
        store.remember(&server.current("a")).await.unwrap();
        store.agree("c", "a", "local", "h").await.unwrap();
        server.state.lock().unwrap().forgotten_below = 50;

        let fetched = fetch(&server, &store, "c", VERSION).await.unwrap();
        assert!(fetched.resync);
        assert_eq!(fetched.records.len(), 1);
        assert!(store.resyncing("c").await.unwrap());
        assert!(store.agreed("c", "a").await.unwrap().is_some());
    }

    /// Past one page, a resync says so on every page after the first, and reaches the end (M4).
    #[tokio::test]
    async fn a_resync_says_so_on_every_page_after_the_first() {
        let (server, store) = (Fake::new("theirs"), Store::in_memory("s").await.unwrap());
        for n in 0..5 {
            server.holds(&n.to_string(), 1, 100, "theirs");
        }
        store.set_since("c", 1).await.unwrap();
        server.state.lock().unwrap().forgotten_below = 50;

        assert_eq!(pull_all(&server, &store).await.len(), 5);
        assert!(
            !store.resyncing("c").await.unwrap(),
            "the last page ended it"
        );
        let flags = server.state.lock().unwrap().resync_flags.clone();
        // The stale cursor, the restart at 0, then two resync pages.
        assert_eq!(flags, vec![false, false, true, true]);
    }

    /// A conflict that never settles — another machine that always wins by a tie it should lose —
    /// is given up on rather than chased for ever.
    #[tokio::test]
    async fn a_conflict_that_never_settles_is_given_up_on() {
        struct Stubborn;
        impl Remote for Stubborn {
            async fn page(&self, _: &str, _: i64, _: bool) -> Result<PageOutcome, AppError> {
                unreachable!("push does not page")
            }
            async fn batch(&self, operations: &[Operation]) -> Result<Vec<BatchResult>, AppError> {
                Ok(operations
                    .iter()
                    .map(|_| {
                        conflict(
                            409,
                            WireRecord {
                                collection: "c".into(),
                                id: "a".into(),
                                version: 9,
                                seq: 9,
                                updated_at: 100,
                                deleted: false,
                                device: "alpha".into(),
                                nonce: Some("n".into()),
                                ciphertext: Some("x".into()),
                            },
                        )
                    })
                    .collect())
            }
        }
        let store = Store::in_memory("s").await.unwrap();
        let pushed = push(
            &Stubborn,
            &store,
            &limits(),
            "zeta",
            vec![write("a", 100, "z")],
        )
        .await
        .unwrap();
        assert_eq!(pushed.error.unwrap().code, "error.syncConflictUnresolved");
    }

    /// A fake that refuses one record with `413` and writes the rest.
    struct Picky {
        inner: Fake,
        refuse: String,
    }

    impl Remote for Picky {
        async fn page(
            &self,
            collection: &str,
            since: i64,
            resync: bool,
        ) -> Result<PageOutcome, AppError> {
            self.inner.page(collection, since, resync).await
        }

        async fn batch(&self, operations: &[Operation]) -> Result<Vec<BatchResult>, AppError> {
            let mut results = Vec::new();
            for operation in operations {
                let Operation::Put { id, .. } = operation else {
                    unreachable!("puts only")
                };
                if *id == self.refuse {
                    results.push(BatchResult {
                        status: 413,
                        record: None,
                        error: Some(ErrorDetail {
                            code: "record-too-large".into(),
                            retry_after: None,
                        }),
                    });
                } else {
                    results.extend(self.inner.batch(std::slice::from_ref(operation)).await?);
                }
            }
            Ok(results)
        }
    }

    /// C2: one refused entry does not hide the entries after it, nor stop the batches after its
    /// own — `limits()` sends two at a time, so "2" is refused in the second of three.
    #[tokio::test]
    async fn a_refused_entry_does_not_hide_the_rest() {
        let server = Picky {
            inner: Fake::new("mine"),
            refuse: "2".into(),
        };
        let store = Store::in_memory("s").await.unwrap();
        let pushed = push(
            &server,
            &store,
            &limits(),
            "mine",
            (0..5).map(|n| write(&n.to_string(), 100, "x")).collect(),
        )
        .await
        .unwrap();
        assert_eq!(
            pushed.error.map(|error| error.code),
            Some("error.syncRecordTooLarge")
        );
        let mut landed = pushed.landed.clone();
        landed.sort();
        assert_eq!(landed, vec!["0", "1", "3", "4"]);
        for id in ["0", "1", "3", "4"] {
            assert!(
                store.seen("c", id).await.unwrap().is_some(),
                "{id} was written and not remembered"
            );
        }
        assert_eq!(store.seen("c", "2").await.unwrap(), None);
    }

    /// A record another version of the app skipped is asked for again: the collection resyncs
    /// from the start, once. The same version asks for nothing (T178d).
    #[tokio::test]
    async fn a_new_version_resyncs_a_collection_that_owes_a_record() {
        let (server, store) = (Fake::new("theirs"), Store::in_memory("s").await.unwrap());
        server.holds("a", 1, 100, "theirs");
        server.holds("b", 1, 100, "theirs");
        assert_eq!(pull_all(&server, &store).await.len(), 2);
        store.owe("c", &["b".into()], VERSION).await.unwrap();

        let same = fetch(&server, &store, "c", VERSION).await.unwrap();
        assert!(!same.resync);
        assert!(same.records.is_empty(), "the same version asked again");

        let upgraded = fetch(&server, &store, "c", "1.1.0").await.unwrap();
        assert!(upgraded.resync);
        assert_eq!(upgraded.records.len(), 2);
        assert!(!store.owed_elsewhere("c", "1.1.0").await.unwrap());
    }
}
