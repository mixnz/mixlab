//! One SQLite file, a small pool of connections, and the schema.
//!
//! **This is where the two implementations genuinely differ.** The Worker keeps one account per
//! Durable Object, where execution is serialized, so the registration race, the monotonic `seq` and
//! the per-record compare-and-swap are correct without a transaction being written. Here every
//! account is a row in one file and several requests are in flight at once, so each of those three
//! is a `BEGIN IMMEDIATE` written by hand — and each one carries a comment naming the guarantee it
//! is standing in for.
//!
//! Neither difference is visible through `/v1`, which is the claim `../conformance/` exists to
//! check.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

/// How many connections are kept. SQLite in WAL mode takes many readers and one writer, so this
/// buys real concurrency for the reads — which is what makes the transactions below load-bearing
/// rather than decorative.
const POOL: usize = 4;

pub struct Db {
    path: String,
    idle: Mutex<Vec<Connection>>,
}

impl Db {
    pub fn open(path: &str) -> rusqlite::Result<Arc<Self>> {
        let db = Arc::new(Self {
            path: path.to_owned(),
            idle: Mutex::new(Vec::with_capacity(POOL)),
        });
        let connection = db.connect()?;
        connection.execute_batch(SCHEMA)?;
        add_public_id(&connection)?;
        db.give(connection);
        Ok(db)
    }

    fn connect(&self) -> rusqlite::Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.execute_batch(
            // WAL so a reader never blocks the writer, and a busy timeout so a writer that arrives
            // mid-transaction waits rather than failing the request it was serving.
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        Ok(connection)
    }

    fn take(&self) -> rusqlite::Result<Connection> {
        if let Some(connection) = self.idle.lock().expect("the pool lock is poisoned").pop() {
            return Ok(connection);
        }
        self.connect()
    }

    fn give(&self, connection: Connection) {
        let mut idle = self.idle.lock().expect("the pool lock is poisoned");
        if idle.len() < POOL {
            idle.push(connection);
        }
    }

    /// Runs one piece of database work off the async runtime. `rusqlite` is synchronous, and a
    /// blocking call on a runtime thread stalls every other request sharing it.
    pub async fn call<T, F>(self: &Arc<Self>, work: F) -> rusqlite::Result<T>
    where
        F: FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let db = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let mut connection = db.take()?;
            let outcome = work(&mut connection);
            db.give(connection);
            outcome
        })
        .await
        .expect("a database task panicked")
    }
}

/// The tables. One file holds every account, which is the shape D8 names as the fallback if
/// Durable Objects ever stop being free — so the two halves of this repository already prove that
/// the protocol survives it.
/// A database made before `account.public_id` existed gets the column, and every row an id. The
/// published image follows `master`, so a file from before T178c may already be serving somebody;
/// `CREATE TABLE IF NOT EXISTS` leaves its table as it was.
fn add_public_id(connection: &Connection) -> rusqlite::Result<()> {
    let has_column = connection
        .prepare("SELECT 1 FROM pragma_table_info('account') WHERE name = 'public_id'")?
        .exists([])?;
    if !has_column {
        // SQLite cannot add a UNIQUE column; the ids are random and 16 bytes, which is uniqueness
        // enough for the rows this path ever sees.
        connection.execute("ALTER TABLE account ADD COLUMN public_id TEXT", [])?;
    }
    let missing: Vec<i64> = connection
        .prepare("SELECT id FROM account WHERE public_id IS NULL")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in missing {
        connection.execute(
            "UPDATE account SET public_id = ?1 WHERE id = ?2",
            rusqlite::params![crate::crypto::random_public_id(), id],
        )?;
    }
    Ok(())
}

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS account (
  id                   INTEGER PRIMARY KEY,
  -- The account's only name, and the same value the Worker uses for its object (D4a). The address
  -- is kept beside it so that whoever runs this for their own team can administer it.
  account_key          TEXT    NOT NULL UNIQUE,
  -- The account's id on the wire (`accountId`): random at registration and never reused, unlike
  -- `id`, which SQLite may hand out again once the last row is deleted (T178c, C4).
  public_id            TEXT    NOT NULL UNIQUE,
  email                TEXT    NOT NULL,
  verifier             TEXT    NOT NULL,
  salt_account         TEXT    NOT NULL,
  argon_m              INTEGER NOT NULL,
  argon_t              INTEGER NOT NULL,
  argon_p              INTEGER NOT NULL,
  wrapped_mk_password  TEXT    NOT NULL,
  wrapped_mk_recovery  TEXT    NOT NULL,
  verified             INTEGER NOT NULL DEFAULT 0,
  created_at           INTEGER NOT NULL,
  next_seq             INTEGER NOT NULL DEFAULT 0,
  stored_bytes         INTEGER NOT NULL DEFAULT 0,
  reaped_below_seq     INTEGER NOT NULL DEFAULT 0,
  -- Whether a client is copying this account elsewhere (D4b). There is no expiry: a freeze
  -- ends when a client asks for active again, which every signed-in machine can do. freeze_at
  -- is when it began, so a person can be told how long this has been true.
  freeze_state         TEXT    NOT NULL DEFAULT 'active',
  freeze_at            INTEGER
);

CREATE TABLE IF NOT EXISTS device (
  id            TEXT    PRIMARY KEY,
  account_id    INTEGER NOT NULL REFERENCES account (id) ON DELETE CASCADE,
  name          TEXT    NOT NULL,
  created_at    INTEGER NOT NULL,
  last_seen_at  INTEGER NOT NULL
);

-- Tokens are stored as their SHA-256 and never in the clear: a database read is not a list of
-- credentials. `rotated` is kept rather than deleted, because a rotated refresh token coming back
-- is the one signal this design gets for free that a token has been copied.
CREATE TABLE IF NOT EXISTS token (
  hash        TEXT    PRIMARY KEY,
  account_id  INTEGER NOT NULL REFERENCES account (id) ON DELETE CASCADE,
  kind        TEXT    NOT NULL,
  device_id   TEXT    NOT NULL,
  expires_at  INTEGER NOT NULL,
  rotated     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS token_by_device ON token (device_id);

CREATE TABLE IF NOT EXISTS mail_token (
  hash        TEXT    PRIMARY KEY,
  account_id  INTEGER NOT NULL REFERENCES account (id) ON DELETE CASCADE,
  kind        TEXT    NOT NULL,
  expires_at  INTEGER NOT NULL,
  used        INTEGER NOT NULL DEFAULT 0
);

-- Only ever written in test-outbox mode, which is refused unless the server was started with it
-- deliberately. It exists because no HTTP suite can read an inbox.
CREATE TABLE IF NOT EXISTS outbox (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id  INTEGER NOT NULL REFERENCES account (id) ON DELETE CASCADE,
  kind        TEXT    NOT NULL,
  token       TEXT    NOT NULL,
  sent_at     INTEGER NOT NULL
);

-- The record table of D3. Both halves of the address are opaque — 32 bytes of keyed hash each — so
-- this table can be read end to end without learning what kind of thing any row is.
CREATE TABLE IF NOT EXISTS record (
  account_id  INTEGER NOT NULL REFERENCES account (id) ON DELETE CASCADE,
  collection  TEXT    NOT NULL,
  id          TEXT    NOT NULL,
  version     INTEGER NOT NULL,
  seq         INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  deleted     INTEGER NOT NULL DEFAULT 0,
  -- The device whose session wrote this row (D3), stamped here and never taken from a body.
  -- D4's tie-break reads it.
  device      TEXT    NOT NULL DEFAULT '',
  nonce       TEXT,
  ciphertext  TEXT,
  bytes       INTEGER NOT NULL DEFAULT 0,
  written_at  INTEGER NOT NULL,
  PRIMARY KEY (account_id, collection, id)
);
CREATE INDEX IF NOT EXISTS record_by_seq ON record (account_id, seq);
-- What `heads` reads: one collection's rows above a cursor, without walking the others (T189).
CREATE INDEX IF NOT EXISTS record_by_collection_seq ON record (account_id, collection, seq);

-- Guessing at one account, counted against that account.
CREATE TABLE IF NOT EXISTS attempt (
  account_id  INTEGER NOT NULL REFERENCES account (id) ON DELETE CASCADE,
  action      TEXT    NOT NULL,
  count       INTEGER NOT NULL,
  started_at  INTEGER NOT NULL,
  PRIMARY KEY (account_id, action)
);

-- Opening many accounts is a different abuse, and a counter kept per account cannot see it.
CREATE TABLE IF NOT EXISTS source_window (
  source      TEXT    NOT NULL,
  action      TEXT    NOT NULL,
  count       INTEGER NOT NULL,
  started_at  INTEGER NOT NULL,
  PRIMARY KEY (source, action)
);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// A file from before `public_id` opens with the column added and its account named.
    #[test]
    fn an_old_database_gets_an_account_id() {
        let path = std::env::temp_dir().join(format!(
            "mixlab-sync-old-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = path.to_str().unwrap().to_owned();
        let old = SCHEMA.replace("  public_id            TEXT    NOT NULL UNIQUE,\n", "");
        assert_ne!(
            old, SCHEMA,
            "the old schema is the current one without public_id"
        );
        {
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(&old).unwrap();
            connection
                .execute(
                    "INSERT INTO account (account_key, email, verifier, salt_account, argon_m,
                                          argon_t, argon_p, wrapped_mk_password,
                                          wrapped_mk_recovery, created_at)
                     VALUES ('k', 'a@example.invalid', 'v', 's', 1, 1, 1, 'w', 'r', 0)",
                    [],
                )
                .unwrap();
        }

        drop(Db::open(&path).unwrap());
        let connection = Connection::open(&path).unwrap();
        let id: String = connection
            .query_row("SELECT public_id FROM account", [], |row| row.get(0))
            .unwrap();
        assert_eq!(id.len(), 32);
        drop(connection);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{path}{suffix}"));
        }
    }
}
