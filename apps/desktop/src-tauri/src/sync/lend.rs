//! What a module lends, turned into what the engine moves — and what comes back, turned into
//! something a module can apply.
//!
//! **`updatedAt` is decided here** (D4). No module stores when an item changed, so the store keeps
//! a hash of each record's canonical plaintext as last agreed with the server, and an item whose
//! hash differs is a change made now. **A hash is recorded only after the change has landed** —
//! pushed and accepted, or pulled and written by the module — never when it is merely sent or
//! received. Recorded early, a write that then failed would read as agreed, and the stale local
//! copy would be pushed back over something newer as if it were an edit.

use std::collections::HashSet;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::crypto::{self, RecordAddress, Sealed};
use super::engine::{Change, Outgoing, Pushed};
use super::merge::{resolve, Keep};
use super::store::{Store, DELETED};
use super::wire::WireRecord;
use crate::error::AppError;

/// The two keys D2 derives from `MK` that lending needs: one names, one seals.
pub struct Keys {
    pub id: [u8; 32],
    pub data: [u8; 32],
}

/// One item as a module lends it: `{ id, data }`, the same shape `core/syncCollection.ts` has.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub data: Value,
}

/// What a module applies after a pull.
#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Incoming {
    pub upserts: Vec<Item>,
    pub removed: Vec<String>,
}

/// What a pull brought, opened and weighed: what the module applies, what to agree on once it has,
/// and — on the page that ends a resync — the records to forget.
#[derive(Debug, Default)]
pub struct Opened {
    pub changes: Incoming,
    pub agreements: Vec<Agreement>,
    pub unmet: Vec<String>,
}

/// An agreement to record once its change has landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agreement {
    pub id: String,
    pub local_id: String,
    pub hash: String,
}

/// The plaintext D3 seals: the real collection name, the real id, and the item.
#[derive(Deserialize)]
struct Envelope {
    collection: String,
    id: String,
    data: Value,
}

/// A value with every object's keys in order, at every depth, serialized.
///
/// **Needed because `preserve_order` is on** in this dependency graph: without it, two readers that
/// built the same item with keys in a different order would hash differently and sync a change
/// nobody made.
pub fn canonical(value: &Value) -> Vec<u8> {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut out = Map::new();
                for key in keys {
                    out.insert(key.clone(), sorted(&map[key]));
                }
                Value::Object(out)
            }
            Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_vec(&sorted(value)).unwrap_or_default()
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut out, byte| {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
            out
        })
}

fn plaintext(collection: &str, item: &Item) -> Vec<u8> {
    canonical(&json!({ "collection": collection, "id": item.id, "data": item.data }))
}

/// One change this machine holds and the server does not: an item whose hash differs from what was
/// agreed, or an agreed item a reader no longer returns.
enum Pending<'a> {
    Write {
        id: String,
        item: &'a Item,
        plain: Vec<u8>,
        digest: String,
    },
    Delete {
        id: String,
    },
}

async fn pending<'a>(
    store: &Store,
    keys: &Keys,
    collection: &str,
    items: &'a [Item],
) -> Result<Vec<Pending<'a>>, AppError> {
    let opaque_collection = crypto::opaque_id(&keys.id, collection);
    let mut out = Vec::new();
    let mut present = HashSet::new();
    for item in items {
        let id = crypto::opaque_id(&keys.id, &item.id);
        present.insert(id.clone());
        let plain = plaintext(collection, item);
        let digest = hash(&plain);
        if store
            .agreed(&opaque_collection, &id)
            .await?
            .is_some_and(|agreed| agreed.hash == digest)
        {
            continue;
        }
        out.push(Pending::Write {
            id,
            item,
            plain,
            digest,
        });
    }
    for (id, _) in store.agreed_live(&opaque_collection).await? {
        if !present.contains(&id) {
            out.push(Pending::Delete { id });
        }
    }
    Ok(out)
}

/// Stamp every change this machine holds, sending nothing: what a full run does before its first
/// page, so that the pull can weigh them (D4). `outgoing` later reads the same stamps.
pub async fn notice(
    store: &Store,
    keys: &Keys,
    collection: &str,
    items: &[Item],
    now: i64,
) -> Result<(), AppError> {
    let opaque_collection = crypto::opaque_id(&keys.id, collection);
    for change in pending(store, keys, collection, items).await? {
        match change {
            Pending::Write { id, digest, .. } => {
                store.stamp(&opaque_collection, &id, &digest, now).await?
            }
            Pending::Delete { id } => store.stamp(&opaque_collection, &id, DELETED, now).await?,
        };
    }
    Ok(())
}

/// This machine's items as the changes the server does not have, and what to record once they have
/// landed. An item agreed before and no longer returned is a deletion. **Each change carries the
/// time it was first noticed**, which `now` is only for a change seen for the first time.
pub async fn outgoing(
    store: &Store,
    keys: &Keys,
    collection: &str,
    items: &[Item],
    now: i64,
) -> Result<(Vec<Outgoing>, Vec<Agreement>), AppError> {
    let opaque_collection = crypto::opaque_id(&keys.id, collection);
    let mut changes = Vec::new();
    let mut agreements = Vec::new();
    for change in pending(store, keys, collection, items).await? {
        match change {
            Pending::Write {
                id,
                item,
                plain,
                digest,
            } => {
                let sealed = crypto::seal_record(
                    &keys.data,
                    &RecordAddress {
                        collection: &opaque_collection,
                        id: &id,
                        deleted: false,
                    },
                    &plain,
                )?;
                let updated_at = store.stamp(&opaque_collection, &id, &digest, now).await?;
                changes.push(Outgoing {
                    collection: opaque_collection.clone(),
                    id: id.clone(),
                    updated_at,
                    change: Change::Write {
                        nonce: STANDARD.encode(sealed.nonce),
                        ciphertext: STANDARD.encode(sealed.ciphertext),
                    },
                });
                agreements.push(Agreement {
                    id,
                    local_id: item.id.clone(),
                    hash: digest,
                });
            }
            Pending::Delete { id } => {
                let updated_at = store.stamp(&opaque_collection, &id, DELETED, now).await?;
                changes.push(Outgoing {
                    collection: opaque_collection.clone(),
                    id,
                    updated_at,
                    change: Change::Delete,
                });
            }
        }
    }
    Ok((changes, agreements))
}

/// After a push: agree on exactly what the server wrote. A conflict lost, an entry refused and a
/// change never sent are not agreed, and keep their stamps.
pub async fn settle_pushed(
    store: &Store,
    keys: &Keys,
    collection: &str,
    agreements: Vec<Agreement>,
    pushed: &Pushed,
) -> Result<(), AppError> {
    let landed: HashSet<&str> = pushed.landed.iter().map(String::as_str).collect();
    let kept = agreements
        .into_iter()
        .filter(|agreement| landed.contains(agreement.id.as_str()))
        .collect();
    settle(store, keys, collection, kept).await
}

/// After a module has written what a pull brought: agree on it.
pub async fn settle(
    store: &Store,
    keys: &Keys,
    collection: &str,
    agreements: Vec<Agreement>,
) -> Result<(), AppError> {
    let opaque_collection = crypto::opaque_id(&keys.id, collection);
    for agreement in agreements {
        store
            .agree(
                &opaque_collection,
                &agreement.id,
                &agreement.local_id,
                &agreement.hash,
            )
            .await?;
    }
    Ok(())
}

/// After a module has written what a pull or a lost conflict brought: record each record's version,
/// then agree on what it says — **except what the module says it skipped**, whose version is still
/// the server's but whose content this machine does not hold. **Only after the write** — see the
/// module comment; the engine leaves a lost conflict unrecorded for exactly this call.
///
/// A skipped record is owed to this machine under `version`, the app that skipped it, so the next
/// release asks for it again.
pub async fn land(
    store: &Store,
    keys: &Keys,
    collection: &str,
    records: &[WireRecord],
    agreements: Vec<Agreement>,
    skipped: &[String],
    version: &str,
) -> Result<(), AppError> {
    for record in records {
        store.remember(record).await?;
    }
    let skipped: HashSet<&str> = skipped.iter().map(String::as_str).collect();
    let (owed, kept): (Vec<Agreement>, Vec<Agreement>) = agreements
        .into_iter()
        .partition(|agreement| skipped.contains(agreement.local_id.as_str()));
    let owed: Vec<String> = owed.into_iter().map(|agreement| agreement.id).collect();
    store
        .owe(&crypto::opaque_id(&keys.id, collection), &owed, version)
        .await?;
    settle(store, keys, collection, kept).await
}

/// Pulled records, opened and weighed against what this machine has not landed (D4).
///
/// **A record whose content is already agreed is not news** and never reaches the module: this
/// machine's own push coming back, above all. A record another machine changed meets this machine's
/// stamp, if it has one, and D4 decides which survives; the loser here is left alone, and its
/// version remembered on landing, so the next push replaces it without a `409`. A tombstone removes
/// the local id this machine agreed on, unless a later change here beats it. On the page that ends
/// a resync, every agreed live item the resync never met is removed — unless a change stamped here
/// beats a deletion that old — and named in `unmet` for the store to forget.
/// **An opened record whose own id does not hash to its address is refused**: the AAD binds the
/// address, and this binds the id inside it, so a record cannot claim to be another item.
pub async fn incoming(
    store: &Store,
    keys: &Keys,
    collection: &str,
    device: &str,
    records: &[WireRecord],
    ending_resync: bool,
) -> Result<Opened, AppError> {
    let opaque_collection = crypto::opaque_id(&keys.id, collection);
    let mut opened = Opened::default();

    for record in records
        .iter()
        .filter(|record| record.collection == opaque_collection)
    {
        let agreed = store.agreed(&opaque_collection, &record.id).await?;
        let stamped = store.stamped(&opaque_collection, &record.id).await?;
        let here_wins = stamped.as_ref().is_some_and(|stamp| {
            resolve(stamp.at, device, record.updated_at, &record.device) == Keep::Local
        });

        if record.deleted {
            if let Some(agreed) = agreed {
                if !here_wins {
                    opened.changes.removed.push(agreed.local_id);
                }
            }
            continue;
        }
        let unreadable = || err!("error.syncCannotOpenRecord");
        let nonce: [u8; 24] = STANDARD
            .decode(record.nonce.as_deref().ok_or_else(unreadable)?)
            .map_err(|_| unreadable())?
            .try_into()
            .map_err(|_| unreadable())?;
        let ciphertext = STANDARD
            .decode(record.ciphertext.as_deref().ok_or_else(unreadable)?)
            .map_err(|_| unreadable())?;
        let plain = crypto::open_record(
            &keys.data,
            &RecordAddress {
                collection: &record.collection,
                id: &record.id,
                deleted: false,
            },
            &Sealed { nonce, ciphertext },
        )?;
        let envelope: Envelope = serde_json::from_slice(&plain).map_err(|_| unreadable())?;
        if envelope.collection != collection
            || crypto::opaque_id(&keys.id, &envelope.id) != record.id
        {
            return Err(unreadable());
        }
        let value: Value = serde_json::from_slice(&plain).map_err(|_| unreadable())?;
        let digest = hash(&canonical(&value));

        if agreed.is_some_and(|agreed| agreed.hash == digest) {
            continue;
        }
        let agreement = Agreement {
            id: record.id.clone(),
            local_id: envelope.id.clone(),
            hash: digest.clone(),
        };
        // What this machine is still pushing says exactly this: agreed, with nothing to write.
        if stamped.is_some_and(|stamp| stamp.hash == digest) {
            opened.agreements.push(agreement);
            continue;
        }
        if here_wins {
            continue;
        }
        opened.agreements.push(agreement);
        opened.changes.upserts.push(Item {
            id: envelope.id,
            data: envelope.data,
        });
    }

    if ending_resync {
        let met = store.met(&opaque_collection).await?;
        let carried: HashSet<&str> = records
            .iter()
            .filter(|record| record.collection == opaque_collection)
            .map(|record| record.id.as_str())
            .collect();
        for (id, local_id) in store.agreed_live(&opaque_collection).await? {
            if met.contains(&id) || carried.contains(id.as_str()) {
                continue;
            }
            // Reaped: deleted at least a retention ago. A change stamped here is newer than that.
            if store.stamped(&opaque_collection, &id).await?.is_none() {
                opened.changes.removed.push(local_id);
            }
            opened.unmet.push(id);
        }
    }
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Keys {
        let master = crypto::new_master_key();
        Keys {
            id: crypto::id_key(&master),
            data: crypto::data_key(&master),
        }
    }

    fn item(id: &str, data: Value) -> Item {
        Item {
            id: id.into(),
            data,
        }
    }

    /// What the server would hand back for a change `outgoing` produced.
    fn landed(change: &Outgoing, version: i64) -> WireRecord {
        let (nonce, ciphertext, deleted) = match &change.change {
            Change::Write { nonce, ciphertext } => {
                (Some(nonce.clone()), Some(ciphertext.clone()), false)
            }
            Change::Delete => (None, None, true),
        };
        WireRecord {
            collection: change.collection.clone(),
            id: change.id.clone(),
            version,
            seq: version,
            updated_at: change.updated_at,
            deleted,
            device: "d".into(),
            nonce,
            ciphertext,
        }
    }

    #[test]
    fn key_order_does_not_change_the_bytes() {
        let a: Value = serde_json::from_str(r#"{"b":1,"a":{"d":2,"c":[{"f":3,"e":4}]}}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":{"c":[{"e":4,"f":3}],"d":2},"b":1}"#).unwrap();
        assert_eq!(canonical(&a), canonical(&b));
    }

    #[tokio::test]
    async fn a_first_lend_is_every_item() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let (changes, agreements) = outgoing(
            &store,
            &keys,
            "c",
            &[item("1", json!({"n": 1})), item("2", json!({"n": 2}))],
            100,
        )
        .await
        .unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(agreements.len(), 2);
        assert!(changes.iter().all(|change| change.updated_at == 100));
    }

    /// Nothing changed is nothing to send — even when a reader built the item in another order.
    #[tokio::test]
    async fn an_agreed_item_is_not_sent_again() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let first = [item("1", serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap())];
        let (changes, agreements) = outgoing(&store, &keys, "c", &first, 100).await.unwrap();
        store.remember(&landed(&changes[0], 1)).await.unwrap();
        settle(&store, &keys, "c", agreements).await.unwrap();

        let reordered = [item("1", serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap())];
        let (again, _) = outgoing(&store, &keys, "c", &reordered, 200).await.unwrap();
        assert!(again.is_empty());
    }

    #[tokio::test]
    async fn an_edit_is_sent_and_a_removal_is_a_deletion() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let items = [item("1", json!("one")), item("2", json!("two"))];
        let (changes, agreements) = outgoing(&store, &keys, "c", &items, 100).await.unwrap();
        for change in &changes {
            store.remember(&landed(change, 1)).await.unwrap();
        }
        settle(&store, &keys, "c", agreements).await.unwrap();

        let (next, _) = outgoing(&store, &keys, "c", &[item("1", json!("ONE"))], 200)
            .await
            .unwrap();
        assert_eq!(next.len(), 2);
        assert!(next
            .iter()
            .any(|change| matches!(change.change, Change::Write { .. })));
        assert!(next.iter().any(|change| change.change == Change::Delete));
    }

    #[tokio::test]
    async fn what_one_machine_lends_another_can_apply() {
        let keys = keys();
        let here = Store::in_memory("s").await.unwrap();
        let there = Store::in_memory("s").await.unwrap();
        let (changes, _) = outgoing(&here, &keys, "c", &[item("1", json!({"x": 1}))], 100)
            .await
            .unwrap();
        let records: Vec<_> = changes.iter().map(|change| landed(change, 1)).collect();

        let opened = incoming(&there, &keys, "c", HERE, &records, false)
            .await
            .unwrap();
        assert_eq!(opened.changes.upserts, vec![item("1", json!({"x": 1}))]);
        assert_eq!(opened.agreements.len(), 1);
    }

    #[tokio::test]
    async fn a_tombstone_removes_what_was_agreed_here() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let (changes, agreements) = outgoing(&store, &keys, "c", &[item("1", json!(1))], 100)
            .await
            .unwrap();
        store.remember(&landed(&changes[0], 1)).await.unwrap();
        settle(&store, &keys, "c", agreements).await.unwrap();

        let mut dead = landed(&changes[0], 2);
        dead.deleted = true;
        let opened = incoming(&store, &keys, "c", HERE, &[dead], false)
            .await
            .unwrap();
        assert_eq!(opened.changes.removed, vec!["1".to_string()]);
    }

    /// A record moved into another item's slot is refused rather than applied as that item.
    #[tokio::test]
    async fn a_record_cannot_claim_another_item() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let (changes, _) = outgoing(&store, &keys, "c", &[item("1", json!(1))], 100)
            .await
            .unwrap();
        let mut moved = landed(&changes[0], 1);
        moved.id = crypto::opaque_id(&keys.id, "2");
        assert!(incoming(&store, &keys, "c", HERE, &[moved], false)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_superseded_change_is_not_agreed() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let (changes, agreements) = outgoing(&store, &keys, "c", &[item("1", json!(1))], 100)
            .await
            .unwrap();
        store.remember(&landed(&changes[0], 1)).await.unwrap();
        let pushed = Pushed {
            superseded: vec![landed(&changes[0], 2)],
            ..Pushed::default()
        };
        settle_pushed(&store, &keys, "c", agreements, &pushed)
            .await
            .unwrap();
        let opaque = crypto::opaque_id(&keys.id, "c");
        assert_eq!(store.agreed(&opaque, &changes[0].id).await.unwrap(), None);
    }

    /// A retry is not a newer edit. Stamped afresh on each attempt, an edit made offline yesterday
    /// would beat one made elsewhere this morning simply by being retried last (D4).
    #[tokio::test]
    async fn a_change_keeps_the_time_it_was_first_noticed() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let draft = [item("1", json!("draft"))];
        let (first, _) = outgoing(&store, &keys, "c", &draft, 100).await.unwrap();
        let (retry, _) = outgoing(&store, &keys, "c", &draft, 200).await.unwrap();
        assert_eq!(first[0].updated_at, 100);
        assert_eq!(retry[0].updated_at, 100, "a retry is the same edit");

        let (edited, _) = outgoing(&store, &keys, "c", &[item("1", json!("final"))], 300)
            .await
            .unwrap();
        assert_eq!(edited[0].updated_at, 300, "a further edit is a newer one");
    }

    #[tokio::test]
    async fn a_deletion_keeps_its_time_too() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let (changes, agreements) = outgoing(&store, &keys, "c", &[item("1", json!(1))], 100)
            .await
            .unwrap();
        store.remember(&landed(&changes[0], 1)).await.unwrap();
        settle(&store, &keys, "c", agreements).await.unwrap();

        let (first, _) = outgoing(&store, &keys, "c", &[], 200).await.unwrap();
        let (retry, _) = outgoing(&store, &keys, "c", &[], 300).await.unwrap();
        assert_eq!(first[0].updated_at, 200);
        assert_eq!(retry[0].updated_at, 200);
    }

    /// Noticed before a pull, a change keeps that time when it is finally pushed (D4's first rule).
    #[tokio::test]
    async fn a_change_noticed_before_a_pull_keeps_that_time_when_pushed() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let draft = [item("1", json!("draft"))];
        notice(&store, &keys, "c", &draft, 100).await.unwrap();
        let (changes, _) = outgoing(&store, &keys, "c", &draft, 200).await.unwrap();
        assert_eq!(changes[0].updated_at, 100);
    }

    #[tokio::test]
    async fn a_removal_noticed_is_stamped_as_a_deletion() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let (changes, agreements) = outgoing(&store, &keys, "c", &[item("1", json!(1))], 100)
            .await
            .unwrap();
        store.remember(&landed(&changes[0], 1)).await.unwrap();
        settle(&store, &keys, "c", agreements).await.unwrap();

        notice(&store, &keys, "c", &[], 200).await.unwrap();
        let opaque = crypto::opaque_id(&keys.id, "c");
        assert_eq!(
            store.stamped(&opaque, &changes[0].id).await.unwrap(),
            Some(crate::sync::store::Stamped {
                hash: DELETED.into(),
                at: 200
            })
        );
    }

    /// A winner written down and landed is agreed: the loser has nothing left to push. This is the
    /// step that ends a conflict (D4).
    #[tokio::test]
    async fn a_landed_winner_leaves_nothing_to_push() {
        let keys = keys();
        let (store, there) = (
            Store::in_memory("s").await.unwrap(),
            Store::in_memory("s").await.unwrap(),
        );
        let (theirs, _) = outgoing(&there, &keys, "c", &[item("1", json!("theirs"))], 300)
            .await
            .unwrap();
        let winner = landed(&theirs[0], 2);
        let opened = incoming(
            &store,
            &keys,
            "c",
            HERE,
            std::slice::from_ref(&winner),
            false,
        )
        .await
        .unwrap();
        land(
            &store,
            &keys,
            "c",
            &[winner],
            opened.agreements,
            &[],
            VERSION,
        )
        .await
        .unwrap();

        let (next, _) = outgoing(&store, &keys, "c", &opened.changes.upserts, 400)
            .await
            .unwrap();
        assert!(next.is_empty());
        let opaque = crypto::opaque_id(&keys.id, "c");
        assert_eq!(
            store
                .seen(&opaque, &theirs[0].id)
                .await
                .unwrap()
                .map(|seen| seen.version),
            Some(2)
        );
    }

    /// An edit that another machine's version replaced is gone, and typing the same bytes again
    /// later is a new edit — not the old one come back with its old time.
    #[tokio::test]
    async fn an_edit_a_pull_replaced_is_forgotten() {
        let keys = keys();
        let (store, there) = (
            Store::in_memory("s").await.unwrap(),
            Store::in_memory("s").await.unwrap(),
        );
        outgoing(&store, &keys, "c", &[item("1", json!("mine"))], 100)
            .await
            .unwrap();

        let (theirs, _) = outgoing(&there, &keys, "c", &[item("1", json!("theirs"))], 150)
            .await
            .unwrap();
        let record = landed(&theirs[0], 1);
        let opened = incoming(
            &store,
            &keys,
            "c",
            HERE,
            std::slice::from_ref(&record),
            false,
        )
        .await
        .unwrap();
        store.remember(&record).await.unwrap();
        settle(&store, &keys, "c", opened.agreements).await.unwrap();

        let (again, _) = outgoing(&store, &keys, "c", &[item("1", json!("mine"))], 500)
            .await
            .unwrap();
        assert_eq!(again[0].updated_at, 500);
    }
    const HERE: &str = "here";
    const VERSION: &str = "1.0.0";

    /// Agree `data` for item "1" here as if pushed and landed at `version`, and return its change.
    async fn agreed_here(store: &Store, keys: &Keys, data: Value, version: i64) -> Outgoing {
        let (changes, agreements) = outgoing(store, keys, "c", &[item("1", data)], 10)
            .await
            .unwrap();
        store.remember(&landed(&changes[0], version)).await.unwrap();
        settle(store, keys, "c", agreements).await.unwrap();
        changes[0].clone()
    }

    /// What another machine pushed for item "1", stamped `at`, landed at `version`.
    async fn theirs(keys: &Keys, data: Value, at: i64, version: i64) -> WireRecord {
        let there = Store::in_memory("s").await.unwrap();
        let (changes, _) = outgoing(&there, keys, "c", &[item("1", data)], at)
            .await
            .unwrap();
        landed(&changes[0], version)
    }

    #[tokio::test]
    async fn a_pulled_copy_of_what_is_agreed_is_not_handed_to_the_module() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let mine = agreed_here(&store, &keys, json!("same"), 1).await;
        // Edited since, not yet pushed: the echo of the earlier push must not write over it.
        notice(&store, &keys, "c", &[item("1", json!("edited"))], 20)
            .await
            .unwrap();
        let opened = incoming(&store, &keys, "c", HERE, &[landed(&mine, 1)], false)
            .await
            .unwrap();
        assert!(opened.changes.upserts.is_empty());
        assert!(opened.agreements.is_empty());
    }

    #[tokio::test]
    async fn a_later_change_here_keeps_its_place_against_an_older_pull() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        notice(&store, &keys, "c", &[item("1", json!("mine"))], 300)
            .await
            .unwrap();
        let record = theirs(&keys, json!("theirs"), 150, 1).await;
        let opened = incoming(&store, &keys, "c", HERE, &[record], false)
            .await
            .unwrap();
        assert!(opened.changes.upserts.is_empty());
        assert!(opened.agreements.is_empty());
    }

    #[tokio::test]
    async fn an_earlier_change_here_gives_way_to_a_later_pull() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        notice(&store, &keys, "c", &[item("1", json!("mine"))], 100)
            .await
            .unwrap();
        let record = theirs(&keys, json!("theirs"), 150, 1).await;
        let opened = incoming(&store, &keys, "c", HERE, &[record], false)
            .await
            .unwrap();
        assert_eq!(opened.changes.upserts, vec![item("1", json!("theirs"))]);
        assert_eq!(opened.agreements.len(), 1);
    }

    #[tokio::test]
    async fn a_pull_that_says_what_this_machine_says_is_agreed_without_a_write() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        notice(&store, &keys, "c", &[item("1", json!("same"))], 100)
            .await
            .unwrap();
        let record = theirs(&keys, json!("same"), 150, 1).await;
        let opened = incoming(&store, &keys, "c", HERE, &[record], false)
            .await
            .unwrap();
        assert!(opened.changes.upserts.is_empty());
        assert_eq!(opened.agreements.len(), 1);
    }

    #[tokio::test]
    async fn a_later_edit_here_survives_an_older_tombstone() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let mine = agreed_here(&store, &keys, json!("v0"), 1).await;
        notice(&store, &keys, "c", &[item("1", json!("edited"))], 300)
            .await
            .unwrap();
        let mut dead = landed(&mine, 2);
        dead.deleted = true;
        dead.updated_at = 100;
        let opened = incoming(&store, &keys, "c", HERE, &[dead], false)
            .await
            .unwrap();
        assert!(opened.changes.removed.is_empty());
    }

    #[tokio::test]
    async fn a_tombstone_later_than_an_edit_here_removes_it() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let mine = agreed_here(&store, &keys, json!("v0"), 1).await;
        notice(&store, &keys, "c", &[item("1", json!("edited"))], 50)
            .await
            .unwrap();
        let mut dead = landed(&mine, 2);
        dead.deleted = true;
        dead.updated_at = 100;
        let opened = incoming(&store, &keys, "c", HERE, &[dead], false)
            .await
            .unwrap();
        assert_eq!(opened.changes.removed, vec!["1".to_string()]);
    }
    /// A record the module did not write is remembered — its version is the server's — and never
    /// agreed, so this machine neither deletes it as missing nor pushes an old copy as an edit.
    #[tokio::test]
    async fn a_record_the_module_skipped_is_remembered_but_not_agreed() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let record = theirs(&keys, json!({"shape": "new"}), 100, 1).await;
        let opened = incoming(
            &store,
            &keys,
            "c",
            HERE,
            std::slice::from_ref(&record),
            false,
        )
        .await
        .unwrap();
        land(
            &store,
            &keys,
            "c",
            std::slice::from_ref(&record),
            opened.agreements,
            &["1".into()],
            VERSION,
        )
        .await
        .unwrap();
        assert_eq!(
            store.agreed(&record.collection, &record.id).await.unwrap(),
            None
        );
        assert_eq!(
            store.owed(&record.collection).await.unwrap(),
            HashSet::from([record.id.clone()]),
            "skipped here, so owed here"
        );
        assert_eq!(
            store
                .seen(&record.collection, &record.id)
                .await
                .unwrap()
                .map(|seen| seen.version),
            Some(1)
        );
        let (next, _) = outgoing(&store, &keys, "c", &[], 200).await.unwrap();
        assert!(
            next.is_empty(),
            "nothing to delete: it was never agreed here"
        );
    }

    /// The page that ends a resync removes what the resync never met: its tombstone was reaped.
    #[tokio::test]
    async fn the_page_that_ends_a_resync_removes_what_it_did_not_meet() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let gone = agreed_here(&store, &keys, json!("g"), 1).await;
        let opened = incoming(&store, &keys, "c", HERE, &[], true).await.unwrap();
        assert_eq!(opened.changes.removed, vec!["1".to_string()]);
        assert_eq!(opened.unmet, vec![gone.id]);
    }

    /// A change stamped here is newer than a deletion old enough to be reaped: kept, not removed,
    /// and still named in `unmet` so its version is forgotten and it is pushed as a creation.
    #[tokio::test]
    async fn a_stamped_item_a_resync_did_not_meet_is_kept() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let gone = agreed_here(&store, &keys, json!("g"), 1).await;
        notice(&store, &keys, "c", &[item("1", json!("edited"))], 50)
            .await
            .unwrap();
        let opened = incoming(&store, &keys, "c", HERE, &[], true).await.unwrap();
        assert!(opened.changes.removed.is_empty());
        assert_eq!(opened.unmet, vec![gone.id]);
    }

    #[tokio::test]
    async fn a_page_that_does_not_end_a_resync_removes_nothing_it_did_not_carry() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        agreed_here(&store, &keys, json!("g"), 1).await;
        let opened = incoming(&store, &keys, "c", HERE, &[], false)
            .await
            .unwrap();
        assert!(opened.changes.removed.is_empty());
        assert!(opened.unmet.is_empty());
    }

    /// A change the server refused is not agreed, and keeps its stamp for the next push.
    #[tokio::test]
    async fn a_refused_change_is_not_agreed() {
        let (store, keys) = (Store::in_memory("s").await.unwrap(), keys());
        let (changes, agreements) = outgoing(
            &store,
            &keys,
            "c",
            &[item("1", json!(1)), item("2", json!(2))],
            100,
        )
        .await
        .unwrap();
        store.remember(&landed(&changes[0], 1)).await.unwrap();
        let pushed = Pushed {
            accepted: 1,
            landed: vec![changes[0].id.clone()],
            error: Some(err!("error.syncRecordTooLarge")),
            ..Pushed::default()
        };
        settle_pushed(&store, &keys, "c", agreements, &pushed)
            .await
            .unwrap();
        let opaque = crypto::opaque_id(&keys.id, "c");
        assert!(store
            .agreed(&opaque, &changes[0].id)
            .await
            .unwrap()
            .is_some());
        assert_eq!(store.agreed(&opaque, &changes[1].id).await.unwrap(), None);
        assert!(store
            .stamped(&opaque, &changes[1].id)
            .await
            .unwrap()
            .is_some());
    }
}
