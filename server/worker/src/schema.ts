// The tables inside one account's Durable Object.
//
// One account, one object, one SQLite file. There is no account table and no second store, because
// **nothing in this design ever queries across accounts** (D8): no administrative screen, no
// statistic, no search, and every operation begins by naming one account.

export const SCHEMA = [
  // A single row. The `CHECK` is not decoration: it is what makes "this object holds one account"
  // a property of the database rather than of the code that writes to it.
  `CREATE TABLE IF NOT EXISTS account (
     id                   INTEGER PRIMARY KEY CHECK (id = 1),
     account_key          TEXT    NOT NULL,
     -- The account's id on the wire, accountId: random at registration, never reused. An
     -- object made before it existed gets the column by migration, which is why it may be null
     -- here (T178c, C4).
     public_id            TEXT,
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
     -- when it began, so a person can be told how long this has been true.
     freeze_state         TEXT    NOT NULL DEFAULT 'active',
     freeze_at            INTEGER
   )`,

  // The record table of D3. The primary key is the pair a record is addressed by, and both halves
  // are opaque: 32 bytes of keyed hash each, so this table can be read end to end without learning
  // what kind of thing any row is.
  //
  // `bytes` is what the quota counts, kept on the row so that the sum is a column rather than a
  // walk. `written_at` is when the row arrived and is what reaping reads — never `updated_at`,
  // which is the client's clock and is not the server's to trust (D1).
  `CREATE TABLE IF NOT EXISTS record (
     collection  TEXT    NOT NULL,
     id          TEXT    NOT NULL,
     version     INTEGER NOT NULL,
     seq         INTEGER NOT NULL,
     updated_at  INTEGER NOT NULL,
     deleted     INTEGER NOT NULL DEFAULT 0,
     -- The device whose session wrote this row (D3), stamped here and never taken from a
     -- body. D4's tie-break reads it.
     device      TEXT    NOT NULL DEFAULT '',
     nonce       TEXT,
     ciphertext  TEXT,
     bytes       INTEGER NOT NULL DEFAULT 0,
     written_at  INTEGER NOT NULL,
     PRIMARY KEY (collection, id)
   )`,
  // What `since` reads, and the order every answer is in.
  `CREATE INDEX IF NOT EXISTS record_by_seq ON record (seq)`,
  // What `heads` reads: one collection's rows above a cursor, without walking the others (T189).
  `CREATE INDEX IF NOT EXISTS record_by_collection_seq ON record (collection, seq)`,

  `CREATE TABLE IF NOT EXISTS device (
     id            TEXT    PRIMARY KEY,
     name          TEXT    NOT NULL,
     created_at    INTEGER NOT NULL,
     last_seen_at  INTEGER NOT NULL
   )`,

  // Tokens are stored as their SHA-256 and never in the clear: a database read is not a list of
  // credentials. `rotated` is kept rather than deleted, because a rotated refresh token coming
  // back is the one signal this design gets for free that a token has been copied (D4a).
  `CREATE TABLE IF NOT EXISTS token (
     hash        TEXT    PRIMARY KEY,
     kind        TEXT    NOT NULL,
     device_id   TEXT    NOT NULL,
     expires_at  INTEGER NOT NULL,
     rotated     INTEGER NOT NULL DEFAULT 0
   )`,
  `CREATE INDEX IF NOT EXISTS token_by_device ON token (device_id)`,

  `CREATE TABLE IF NOT EXISTS mail_token (
     hash        TEXT    PRIMARY KEY,
     kind        TEXT    NOT NULL,
     expires_at  INTEGER NOT NULL,
     used        INTEGER NOT NULL DEFAULT 0
   )`,

  // Guessing at one account, counted where that account lives. Opening *many* accounts is a
  // different abuse and is counted somewhere else — see `ratelimit.ts` for why it cannot be here.
  `CREATE TABLE IF NOT EXISTS attempt (
     action      TEXT    PRIMARY KEY,
     count       INTEGER NOT NULL,
     started_at  INTEGER NOT NULL
   )`,

  // Only ever written in test-outbox mode, which is refused unless a server was started with it
  // deliberately. It exists because no HTTP suite can read an inbox (D4a).
  `CREATE TABLE IF NOT EXISTS outbox (
     id       INTEGER PRIMARY KEY AUTOINCREMENT,
     kind     TEXT    NOT NULL,
     token    TEXT    NOT NULL,
     sent_at  INTEGER NOT NULL
   )`,
] as const;

// Each row type carries an index signature because `SqlStorage.exec<T>` constrains `T` to a record
// of storable values; without it the type is rejected rather than the query.
export interface AccountRow extends Record<string, SqlStorageValue> {
  account_key: string;
  public_id: string | null;
  email: string;
  verifier: string;
  salt_account: string;
  argon_m: number;
  argon_t: number;
  argon_p: number;
  wrapped_mk_password: string;
  wrapped_mk_recovery: string;
  verified: number;
  created_at: number;
  next_seq: number;
  stored_bytes: number;
  reaped_below_seq: number;
  freeze_state: string;
  freeze_at: number | null;
}

export interface RecordRow extends Record<string, SqlStorageValue> {
  collection: string;
  id: string;
  version: number;
  seq: number;
  updated_at: number;
  deleted: number;
  device: string;
  nonce: string | null;
  ciphertext: string | null;
  bytes: number;
  written_at: number;
}

export interface DeviceRow extends Record<string, SqlStorageValue> {
  id: string;
  name: string;
  created_at: number;
  last_seen_at: number;
}

export interface TokenRow extends Record<string, SqlStorageValue> {
  hash: string;
  kind: string;
  device_id: string;
  expires_at: number;
  rotated: number;
}

export interface MailTokenRow extends Record<string, SqlStorageValue> {
  hash: string;
  kind: string;
  expires_at: number;
  used: number;
}
