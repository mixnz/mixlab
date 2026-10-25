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
    /// again at the next push.
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
    /// never met.
    pub resync: bool,
}

/// The next page of `collection` after this machine's cursor. **Moves nothing**: see [`Fetched`].
/// A `410` starts a resync. Every later page of it says `resync=1`, so it is not expired halfway.
/// So does a record another `version` of the app skipped: this one may read it, and only a resync
/// delivers it again.
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
/// met; the last one forgets `unmet` and ends the resync.
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

/// Which of `collections` (opaque) a full run must pull. A cursor at 0, a resync and a record owed
/// under another version are stale without asking: `fetch` does more than read from the cursor for
/// each, and the server sees none of them. So is a change still stamped: its push may have landed
/// unheard, and only the pull of that echo agrees on it. The rest are asked in as few requests as
/// the server takes, and **a collection the answer does not name has its cursor moved to
/// `nextSince`**: since the cursor, only this machine wrote to it, and its push remembered those
/// writes. Never backwards.
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
/// batch is read**: one refused record says nothing about the others, and an entry the server wrote
/// is remembered whatever came before it. Only a request that fails as a whole stops the push, and
/// what it had not sent stays stamped for the next one.
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
                    // remember it: the same device at the same stamp. The local item is this
                    // device's latest word on the record, so it is written over, not handed back as
                    // an edit that replaced it.
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
            // The time it was first noticed: D4 weighs a deletion like any edit.
            updated_at: change.updated_at,
        }),
    })
}

#[cfg(test)]
mod tests;
