//! What this machine remembers about sync, so a pull resumes and a push can say `If-Match`.
//!
//! Everything kept **per server**: the cursor each collection has read up to, the version of each
//! record this machine last saw and what it agreed that record says, the changes it has noticed
//! and not yet landed, and a resync under way. Per server because the same opaque ids mean nothing
//! on another one — a different server is a different account (D8). Nothing here is plaintext:
//! every collection and id is the opaque HMAC of D3.

use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use super::wire::WireRecord;
use crate::error::AppError;

/// What a deletion is stamped under. Never a hash: those are 64 hex characters.
pub const DELETED: &str = "deleted";

const SCHEMA: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS cursor (
       server     TEXT    NOT NULL,
       collection TEXT    NOT NULL,
       since      INTEGER NOT NULL,
       PRIMARY KEY (server, collection)
     )",
    "CREATE TABLE IF NOT EXISTS seen (
       server     TEXT    NOT NULL,
       collection TEXT    NOT NULL,
       id         TEXT    NOT NULL,
       version    INTEGER NOT NULL,
       updated_at INTEGER NOT NULL,
       deleted    INTEGER NOT NULL,
       -- What this machine and the server last agreed a record says: the local id it is known by
       -- here, and a hash of its canonical plaintext. Absent until a change has landed (D4).
       local_id   TEXT,
       hash       TEXT,
       PRIMARY KEY (server, collection, id)
     )",
    // When this machine first noticed a change it has not yet landed, and what the change said. A
    // retry reuses the time; a further edit — a different hash — replaces it (D4).
    "CREATE TABLE IF NOT EXISTS stamp (
       server     TEXT    NOT NULL,
       collection TEXT    NOT NULL,
       id         TEXT    NOT NULL,
       hash       TEXT    NOT NULL,
       at         INTEGER NOT NULL,
       PRIMARY KEY (server, collection, id)
     )",
    // A resync under way: `410` started it, and the page with `more: false` ends it. Kept on disk,
    // so a resync interrupted by quitting resumes as one (T178b, M1).
    "CREATE TABLE IF NOT EXISTS resync (
       server     TEXT NOT NULL,
       collection TEXT NOT NULL,
       PRIMARY KEY (server, collection)
     )",
    // Every record a resync has met so far; what it never meets was reaped (M2).
    "CREATE TABLE IF NOT EXISTS resync_met (
       server     TEXT NOT NULL,
       collection TEXT NOT NULL,
       id         TEXT NOT NULL,
       PRIMARY KEY (server, collection, id)
     )",
    // Records this machine's module skipped: the server holds them, this machine does not. Each
    // is asked for again once a new version of the app, the one that may read it, syncs (T178d).
    "CREATE TABLE IF NOT EXISTS owed (
       server     TEXT NOT NULL,
       collection TEXT NOT NULL,
       id         TEXT NOT NULL,
       version    TEXT NOT NULL,
       PRIMARY KEY (server, collection, id)
     )",
];

/// What this machine last saw of one record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub version: i64,
    pub updated_at: i64,
    pub deleted: bool,
}

/// What this machine and the server last agreed a record says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agreed {
    pub local_id: String,
    pub hash: String,
}

/// A change this machine noticed and has not landed: what it said, and when it was first noticed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamped {
    pub hash: String,
    pub at: i64,
}

pub struct Store {
    pool: SqlitePool,
    server: String,
}

impl Store {
    pub async fn open(path: &Path, server: &str) -> Result<Self, AppError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        Self::with(options, server).await
    }

    /// A store that lives as long as the value does — for tests.
    pub async fn in_memory(server: &str) -> Result<Self, AppError> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:").map_err(store_error)?;
        Self::with(options, server).await
    }

    /// One connection: an in-memory database is per connection, and a file needs no more.
    async fn with(options: SqliteConnectOptions, server: &str) -> Result<Self, AppError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(store_error)?;
        // Each is a literal: sqlx 0.9 accepts only a `&'static str` without an audit wrapper.
        for statement in SCHEMA {
            sqlx::query(*statement)
                .execute(&pool)
                .await
                .map_err(store_error)?;
        }
        Ok(Self {
            pool,
            server: server.to_owned(),
        })
    }

    /// Where a pull of `collection` resumes. `0` is the beginning.
    pub async fn since(&self, collection: &str) -> Result<i64, AppError> {
        let row = sqlx::query("SELECT since FROM cursor WHERE server = ?1 AND collection = ?2")
            .bind(&self.server)
            .bind(collection)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        Ok(row.map(|row| row.get::<i64, _>(0)).unwrap_or(0))
    }

    /// Whether this account's `collection` has been pulled at least once: a cursor is recorded,
    /// however empty the collection was. A resync under way has none until its first page lands.
    pub async fn has_pulled(&self, collection: &str) -> Result<bool, AppError> {
        let row = sqlx::query("SELECT 1 FROM cursor WHERE server = ?1 AND collection = ?2")
            .bind(&self.server)
            .bind(collection)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        Ok(row.is_some())
    }

    pub async fn set_since(&self, collection: &str, since: i64) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO cursor (server, collection, since) VALUES (?1, ?2, ?3)
             ON CONFLICT (server, collection) DO UPDATE SET since = excluded.since",
        )
        .bind(&self.server)
        .bind(collection)
        .bind(since)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(())
    }

    pub async fn seen(&self, collection: &str, id: &str) -> Result<Option<Seen>, AppError> {
        let row = sqlx::query(
            "SELECT version, updated_at, deleted FROM seen
             WHERE server = ?1 AND collection = ?2 AND id = ?3",
        )
        .bind(&self.server)
        .bind(collection)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(row.map(|row| Seen {
            version: row.get(0),
            updated_at: row.get(1),
            deleted: row.get::<i64, _>(2) == 1,
        }))
    }

    /// Remember the version the server holds, so the next write can say `If-Match` with it.
    pub async fn remember(&self, record: &WireRecord) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO seen (server, collection, id, version, updated_at, deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (server, collection, id) DO UPDATE SET
               version = excluded.version, updated_at = excluded.updated_at,
               deleted = excluded.deleted",
        )
        .bind(&self.server)
        .bind(&record.collection)
        .bind(&record.id)
        .bind(record.version)
        .bind(record.updated_at)
        .bind(i64::from(record.deleted))
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        // A tombstone is the end of a deletion this machine was landing. An edit made here is not
        // ended by it: it may yet beat the tombstone (D4), and keeps its stamp until it lands. A
        // record owed here is owed no more: there is nothing left to deliver.
        if record.deleted {
            self.unstamp_deletion(&record.collection, &record.id)
                .await?;
            self.unowe(&record.collection, &record.id).await?;
        }
        Ok(())
    }

    /// What `410 cursor-expired` asks for — D3's *resync from empty rather than incomplete news
    /// quietly*: the cursor back to the start and a resync under way. **What was agreed stays**, so
    /// a surviving tombstone still removes what it names and an agreed record is not rewritten
    /// (T178b, M1).
    pub async fn begin_resync(&self, collection: &str) -> Result<(), AppError> {
        for statement in [
            "DELETE FROM cursor WHERE server = ?1 AND collection = ?2",
            "DELETE FROM resync_met WHERE server = ?1 AND collection = ?2",
            "INSERT OR IGNORE INTO resync (server, collection) VALUES (?1, ?2)",
        ] {
            sqlx::query(statement)
                .bind(&self.server)
                .bind(collection)
                .execute(&self.pool)
                .await
                .map_err(store_error)?;
        }
        Ok(())
    }

    pub async fn resyncing(&self, collection: &str) -> Result<bool, AppError> {
        let row = sqlx::query("SELECT 1 FROM resync WHERE server = ?1 AND collection = ?2")
            .bind(&self.server)
            .bind(collection)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        Ok(row.is_some())
    }

    /// Records a resync page carried, once the module has written it.
    pub async fn mark_met(&self, collection: &str, ids: &[String]) -> Result<(), AppError> {
        for id in ids {
            sqlx::query(
                "INSERT OR IGNORE INTO resync_met (server, collection, id) VALUES (?1, ?2, ?3)",
            )
            .bind(&self.server)
            .bind(collection)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(store_error)?;
        }
        Ok(())
    }

    pub async fn met(&self, collection: &str) -> Result<HashSet<String>, AppError> {
        let rows = sqlx::query("SELECT id FROM resync_met WHERE server = ?1 AND collection = ?2")
            .bind(&self.server)
            .bind(collection)
            .fetch_all(&self.pool)
            .await
            .map_err(store_error)?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    /// The page that ends a resync has been written: forget each record it never met — its
    /// tombstone was reaped — with any deletion of it this machine was landing, and end the
    /// resync. An edit's stamp stays: with no version remembered, it is pushed as a creation (M2).
    /// A record owed and never met is owed no more. `unmet` does not name it, because it was never
    /// agreed, so `resync_met` decides (T178d).
    pub async fn end_resync(&self, collection: &str, unmet: &[String]) -> Result<(), AppError> {
        for id in unmet {
            sqlx::query("DELETE FROM seen WHERE server = ?1 AND collection = ?2 AND id = ?3")
                .bind(&self.server)
                .bind(collection)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(store_error)?;
            self.unstamp_deletion(collection, id).await?;
        }
        for statement in [
            "DELETE FROM owed WHERE server = ?1 AND collection = ?2
               AND id NOT IN (SELECT id FROM resync_met WHERE server = ?1 AND collection = ?2)",
            "DELETE FROM resync_met WHERE server = ?1 AND collection = ?2",
            "DELETE FROM resync WHERE server = ?1 AND collection = ?2",
        ] {
            sqlx::query(statement)
                .bind(&self.server)
                .bind(collection)
                .execute(&self.pool)
                .await
                .map_err(store_error)?;
        }
        Ok(())
    }

    pub async fn agreed(&self, collection: &str, id: &str) -> Result<Option<Agreed>, AppError> {
        let row = sqlx::query(
            "SELECT local_id, hash FROM seen
             WHERE server = ?1 AND collection = ?2 AND id = ?3
               AND local_id IS NOT NULL AND hash IS NOT NULL",
        )
        .bind(&self.server)
        .bind(collection)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(row.map(|row| Agreed {
            local_id: row.get(0),
            hash: row.get(1),
        }))
    }

    /// Record agreement **once the change it describes has landed** — never before, or a write
    /// that failed afterwards would read as agreed and be pushed back over something newer.
    pub async fn agree(
        &self,
        collection: &str,
        id: &str,
        local_id: &str,
        hash: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE seen SET local_id = ?4, hash = ?5
             WHERE server = ?1 AND collection = ?2 AND id = ?3",
        )
        .bind(&self.server)
        .bind(collection)
        .bind(id)
        .bind(local_id)
        .bind(hash)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        // Whatever this machine was still trying to land is either what was agreed or was replaced
        // by it; either way it is no longer in flight.
        self.unstamp(collection, id).await?;
        // Agreed means written here: a record owed is delivered.
        self.unowe(collection, id).await?;
        Ok(())
    }

    /// Every record in `collection` this machine agreed on that is not a tombstone, as
    /// `(opaque id, local id)`: what a reader must still return, or it was deleted here.
    pub async fn agreed_live(&self, collection: &str) -> Result<Vec<(String, String)>, AppError> {
        let rows = sqlx::query(
            "SELECT id, local_id FROM seen
             WHERE server = ?1 AND collection = ?2 AND deleted = 0 AND local_id IS NOT NULL",
        )
        .bind(&self.server)
        .bind(collection)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(rows.iter().map(|row| (row.get(0), row.get(1))).collect())
    }

    /// Records the module skipped, owed to this machine. `version` is the app that skipped them;
    /// skipped again, a record takes the later one (T178d).
    pub async fn owe(
        &self,
        collection: &str,
        ids: &[String],
        version: &str,
    ) -> Result<(), AppError> {
        for id in ids {
            sqlx::query(
                "INSERT INTO owed (server, collection, id, version) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (server, collection, id) DO UPDATE SET version = excluded.version",
            )
            .bind(&self.server)
            .bind(collection)
            .bind(id)
            .bind(version)
            .execute(&self.pool)
            .await
            .map_err(store_error)?;
        }
        Ok(())
    }

    pub async fn owed(&self, collection: &str) -> Result<HashSet<String>, AppError> {
        let rows = sqlx::query("SELECT id FROM owed WHERE server = ?1 AND collection = ?2")
            .bind(&self.server)
            .bind(collection)
            .fetch_all(&self.pool)
            .await
            .map_err(store_error)?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    /// Whether `collection` owes a record that another version of the app skipped. This one's
    /// reader may understand it now.
    pub async fn owed_elsewhere(&self, collection: &str, version: &str) -> Result<bool, AppError> {
        let row = sqlx::query(
            "SELECT 1 FROM owed WHERE server = ?1 AND collection = ?2 AND version <> ?3 LIMIT 1",
        )
        .bind(&self.server)
        .bind(collection)
        .bind(version)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(row.is_some())
    }

    /// Every record `collection` owes, now asked for by `version`. One this version cannot read
    /// either waits for the next release, not the next launch.
    pub async fn renew_owed(&self, collection: &str, version: &str) -> Result<(), AppError> {
        sqlx::query("UPDATE owed SET version = ?3 WHERE server = ?1 AND collection = ?2")
            .bind(&self.server)
            .bind(collection)
            .bind(version)
            .execute(&self.pool)
            .await
            .map_err(store_error)?;
        Ok(())
    }

    /// When this machine first noticed the change it is still trying to land — the same time on
    /// every retry, so an edit is never newer for having waited (D4). A different `hash` is a
    /// further edit, and is stamped `now`.
    pub async fn stamp(
        &self,
        collection: &str,
        id: &str,
        hash: &str,
        now: i64,
    ) -> Result<i64, AppError> {
        let row = sqlx::query(
            "SELECT at FROM stamp
             WHERE server = ?1 AND collection = ?2 AND id = ?3 AND hash = ?4",
        )
        .bind(&self.server)
        .bind(collection)
        .bind(id)
        .bind(hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        if let Some(row) = row {
            return Ok(row.get(0));
        }
        sqlx::query(
            "INSERT INTO stamp (server, collection, id, hash, at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (server, collection, id) DO UPDATE SET
               hash = excluded.hash, at = excluded.at",
        )
        .bind(&self.server)
        .bind(collection)
        .bind(id)
        .bind(hash)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(now)
    }

    /// The change this machine is still trying to land for `id`, if any — what a pull weighs
    /// against the version it brings (D4).
    pub async fn stamped(&self, collection: &str, id: &str) -> Result<Option<Stamped>, AppError> {
        let row = sqlx::query(
            "SELECT hash, at FROM stamp WHERE server = ?1 AND collection = ?2 AND id = ?3",
        )
        .bind(&self.server)
        .bind(collection)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(row.map(|row| Stamped {
            hash: row.get(0),
            at: row.get(1),
        }))
    }

    /// Whether any change to `collection` is still stamped: made here and not yet landed. The
    /// server may hold it already, from a push this machine never heard the end of (T189).
    pub async fn unlanded(&self, collection: &str) -> Result<bool, AppError> {
        let row = sqlx::query("SELECT 1 FROM stamp WHERE server = ?1 AND collection = ?2 LIMIT 1")
            .bind(&self.server)
            .bind(collection)
            .fetch_optional(&self.pool)
            .await
            .map_err(store_error)?;
        Ok(row.is_some())
    }

    async fn unstamp_deletion(&self, collection: &str, id: &str) -> Result<(), AppError> {
        sqlx::query(
            "DELETE FROM stamp WHERE server = ?1 AND collection = ?2 AND id = ?3 AND hash = ?4",
        )
        .bind(&self.server)
        .bind(collection)
        .bind(id)
        .bind(DELETED)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(())
    }

    async fn unstamp(&self, collection: &str, id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM stamp WHERE server = ?1 AND collection = ?2 AND id = ?3")
            .bind(&self.server)
            .bind(collection)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(store_error)?;
        Ok(())
    }

    async fn unowe(&self, collection: &str, id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM owed WHERE server = ?1 AND collection = ?2 AND id = ?3")
            .bind(&self.server)
            .bind(collection)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(store_error)?;
        Ok(())
    }
}

fn store_error(error: sqlx::Error) -> AppError {
    err!("error.syncStoreFailed", message = error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(collection: &str, id: &str, version: i64) -> WireRecord {
        WireRecord {
            collection: collection.into(),
            id: id.into(),
            version,
            seq: version,
            updated_at: 100 + version,
            deleted: false,
            device: "d".into(),
            nonce: Some("n".into()),
            ciphertext: Some("x".into()),
        }
    }

    #[tokio::test]
    async fn a_new_collection_starts_at_the_beginning() {
        let store = Store::in_memory("https://a").await.unwrap();
        assert_eq!(store.since("c").await.unwrap(), 0);
        store.set_since("c", 42).await.unwrap();
        assert_eq!(store.since("c").await.unwrap(), 42);
    }

    #[tokio::test]
    async fn the_latest_version_seen_is_the_one_remembered() {
        let store = Store::in_memory("https://a").await.unwrap();
        assert_eq!(store.seen("c", "i").await.unwrap(), None);
        store.remember(&record("c", "i", 1)).await.unwrap();
        store.remember(&record("c", "i", 2)).await.unwrap();
        assert_eq!(
            store.seen("c", "i").await.unwrap(),
            Some(Seen {
                version: 2,
                updated_at: 102,
                deleted: false
            })
        );
    }

    /// `410` moves the cursor back and starts a resync, and **keeps what was agreed**: a surviving
    /// tombstone still has something to remove, and an agreed record is not rewritten (M1).
    #[tokio::test]
    async fn a_resync_moves_the_cursor_and_keeps_what_was_agreed() {
        let store = Store::in_memory("https://a").await.unwrap();
        store.set_since("c", 9).await.unwrap();
        store.set_since("other", 7).await.unwrap();
        store.remember(&record("c", "i", 1)).await.unwrap();
        store.agree("c", "i", "local", "h").await.unwrap();
        store.begin_resync("c").await.unwrap();
        assert_eq!(store.since("c").await.unwrap(), 0);
        assert!(store.resyncing("c").await.unwrap());
        assert!(store.agreed("c", "i").await.unwrap().is_some());
        assert_eq!(store.since("other").await.unwrap(), 7);
        assert!(!store.resyncing("other").await.unwrap());
    }

    /// The end of a resync forgets what it did not meet — and a deletion this machine was
    /// landing for one of them — but keeps an edit's stamp, which is pushed as a creation (M2).
    #[tokio::test]
    async fn the_end_of_a_resync_forgets_what_it_did_not_meet() {
        let store = Store::in_memory("https://a").await.unwrap();
        store.begin_resync("c").await.unwrap();
        for id in ["met", "gone", "edited"] {
            store.remember(&record("c", id, 1)).await.unwrap();
            store.agree("c", id, id, "h").await.unwrap();
        }
        store.stamp("c", "gone", DELETED, 5).await.unwrap();
        store.stamp("c", "edited", "h2", 5).await.unwrap();
        store.mark_met("c", &["met".into()]).await.unwrap();
        assert_eq!(
            store.met("c").await.unwrap(),
            HashSet::from(["met".to_string()])
        );

        store
            .end_resync("c", &["gone".into(), "edited".into()])
            .await
            .unwrap();
        assert!(store.seen("c", "met").await.unwrap().is_some());
        assert_eq!(store.seen("c", "gone").await.unwrap(), None);
        assert_eq!(store.seen("c", "edited").await.unwrap(), None);
        assert_eq!(store.stamped("c", "gone").await.unwrap(), None);
        assert!(store.stamped("c", "edited").await.unwrap().is_some());
        assert!(!store.resyncing("c").await.unwrap());
        assert!(store.met("c").await.unwrap().is_empty());
    }

    /// **A different server is a different account.** Its cursor and versions are not this one's.
    #[tokio::test]
    async fn two_servers_share_nothing() {
        let path = std::env::temp_dir().join(format!("sync-store-{}.db", uuid::Uuid::new_v4()));
        let a = Store::open(&path, "https://a").await.unwrap();
        a.set_since("c", 5).await.unwrap();
        a.remember(&record("c", "i", 3)).await.unwrap();
        drop(a);

        let b = Store::open(&path, "https://b").await.unwrap();
        assert_eq!(b.since("c").await.unwrap(), 0);
        assert_eq!(b.seen("c", "i").await.unwrap(), None);
        drop(b);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn agreement_is_kept_by_remember_and_read_back() {
        let store = Store::in_memory("s").await.unwrap();
        store.remember(&record("c", "i", 1)).await.unwrap();
        assert_eq!(store.agreed("c", "i").await.unwrap(), None);
        store.agree("c", "i", "local-1", "h1").await.unwrap();
        store.remember(&record("c", "i", 2)).await.unwrap();
        assert_eq!(
            store.agreed("c", "i").await.unwrap(),
            Some(Agreed {
                local_id: "local-1".into(),
                hash: "h1".into()
            })
        );
    }

    #[tokio::test]
    async fn a_stamp_lasts_until_its_change_is_agreed() {
        let store = Store::in_memory("s").await.unwrap();
        assert_eq!(store.stamp("c", "i", "h1", 100).await.unwrap(), 100);
        assert_eq!(store.stamp("c", "i", "h1", 200).await.unwrap(), 100);
        assert_eq!(store.stamp("c", "i", "h2", 300).await.unwrap(), 300);

        store.remember(&record("c", "i", 1)).await.unwrap();
        store.agree("c", "i", "local", "h2").await.unwrap();
        assert_eq!(store.stamp("c", "i", "h2", 400).await.unwrap(), 400);
    }

    #[tokio::test]
    async fn a_tombstone_is_not_live() {
        let store = Store::in_memory("s").await.unwrap();
        store.remember(&record("c", "a", 1)).await.unwrap();
        store.remember(&record("c", "b", 1)).await.unwrap();
        store.agree("c", "a", "la", "h").await.unwrap();
        store.agree("c", "b", "lb", "h").await.unwrap();
        let mut dead = record("c", "b", 2);
        dead.deleted = true;
        store.remember(&dead).await.unwrap();
        assert_eq!(
            store.agreed_live("c").await.unwrap(),
            vec![("a".into(), "la".into())]
        );
    }

    #[tokio::test]
    async fn a_stamp_reads_back_with_its_time() {
        let store = Store::in_memory("https://a").await.unwrap();
        assert_eq!(store.stamped("c", "i").await.unwrap(), None);
        store.stamp("c", "i", "h1", 100).await.unwrap();
        assert_eq!(
            store.stamped("c", "i").await.unwrap(),
            Some(Stamped {
                hash: "h1".into(),
                at: 100
            })
        );
    }

    /// A tombstone ends a deletion this machine was landing, and nothing else: an edit made here
    /// may yet beat it (D4), and needs the time it was first noticed to do so.
    #[tokio::test]
    async fn a_tombstone_ends_a_deletion_but_not_an_edit() {
        let store = Store::in_memory("https://a").await.unwrap();
        store.stamp("c", "edited", "h1", 100).await.unwrap();
        store.stamp("c", "removed", DELETED, 100).await.unwrap();
        for id in ["edited", "removed"] {
            let mut dead = record("c", id, 2);
            dead.deleted = true;
            store.remember(&dead).await.unwrap();
        }
        assert!(store.stamped("c", "edited").await.unwrap().is_some());
        assert_eq!(store.stamped("c", "removed").await.unwrap(), None);
    }

    /// A push trusts that a record it never saw is one the server does not have, which holds only
    /// once this account's collection has been pulled at least once — however empty it was.
    #[tokio::test]
    async fn a_collection_is_pulled_once_a_cursor_is_recorded() {
        let store = Store::in_memory("https://a").await.unwrap();
        assert!(!store.has_pulled("c").await.unwrap());
        store.set_since("c", 0).await.unwrap();
        assert!(store.has_pulled("c").await.unwrap());
        assert!(!store.has_pulled("other").await.unwrap());
        store.begin_resync("c").await.unwrap();
        assert!(
            !store.has_pulled("c").await.unwrap(),
            "a resync under way has not pulled yet"
        );
    }

    /// A skipped record is owed until it stops being owed: agreed once the module writes it, a
    /// tombstone remembered for it, or a resync that ends without meeting it (T178d).
    #[tokio::test]
    async fn a_skipped_record_is_owed_until_it_is_agreed_deleted_or_reaped() {
        let store = Store::in_memory("https://a").await.unwrap();
        let ids = ["written", "deleted", "reaped", "still"];
        for id in ids {
            store.remember(&record("c", id, 1)).await.unwrap();
        }
        let owed: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        store.owe("c", &owed, "1.0.0").await.unwrap();

        store.agree("c", "written", "local", "h").await.unwrap();
        let mut dead = record("c", "deleted", 2);
        dead.deleted = true;
        store.remember(&dead).await.unwrap();
        assert_eq!(
            store.owed("c").await.unwrap(),
            HashSet::from(["reaped".to_string(), "still".to_string()])
        );

        store.begin_resync("c").await.unwrap();
        store.mark_met("c", &["still".into()]).await.unwrap();
        store.end_resync("c", &[]).await.unwrap();
        assert_eq!(
            store.owed("c").await.unwrap(),
            HashSet::from(["still".to_string()]),
            "`unmet` never names an owed record: it was never agreed"
        );
    }

    /// Only a record skipped by another version is worth asking for again, and asking re-stamps
    /// it, so the same version does not ask twice.
    #[tokio::test]
    async fn a_record_owed_to_another_version_is_asked_for_once() {
        let store = Store::in_memory("https://a").await.unwrap();
        store.owe("c", &["i".into()], "1.0.0").await.unwrap();
        assert!(!store.owed_elsewhere("c", "1.0.0").await.unwrap());
        assert!(store.owed_elsewhere("c", "1.1.0").await.unwrap());
        assert!(!store.owed_elsewhere("other", "1.1.0").await.unwrap());

        store.renew_owed("c", "1.1.0").await.unwrap();
        assert!(!store.owed_elsewhere("c", "1.1.0").await.unwrap());
        assert_eq!(
            store.owed("c").await.unwrap(),
            HashSet::from(["i".to_string()])
        );
    }
}
