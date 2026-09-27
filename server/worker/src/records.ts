// The record table: a compare-and-swap, a cursor, and a byte count. Nothing here opens a record.
//
// **The compare-and-swap needs no transaction**, because everything in this file runs inside one
// Durable Object, where execution is serialized (D8). Read-then-write is safe for the same reason
// `next_seq` can be a column that is read, incremented and written without a lock. `server/native/`
// writes the transaction this does not need, and says where.

import type { Capabilities } from "./config";
import type { AccountRow, RecordRow } from "./schema";
import { asObject, isBase64, isOpaqueId, isTimestamp } from "./validate";

/**
 * What the quota counts for one record, beyond its payload: the row's own metadata, in round
 * numbers. It exists so that an account of ten thousand empty records is not free — the ceiling
 * the quota defends is the account's footprint, not the sum of its ciphertexts.
 */
const ROW_OVERHEAD_BYTES = 256;

export interface WireRecord {
  collection: string;
  id: string;
  version: number;
  seq: number;
  updatedAt: number;
  deleted: boolean;
  /** The device whose session wrote it (D3). */
  device: string;
  nonce?: string;
  ciphertext?: string;
}

export interface Outcome {
  status: number;
  record?: WireRecord;
  error?: { code: string; message: string; [member: string]: unknown };
}

export interface Precondition {
  ifMatch?: number;
  ifNoneMatch: boolean;
}

/** Bytes a standard-base64 string stands for, without decoding it. */
export function decodedLength(value: string): number {
  const padding = value.endsWith("==") ? 2 : value.endsWith("=") ? 1 : 0;
  return (value.length / 4) * 3 - padding;
}

const wire = (row: RecordRow): WireRecord => ({
  collection: row.collection,
  id: row.id,
  version: row.version,
  seq: row.seq,
  updatedAt: row.updated_at,
  deleted: row.deleted === 1,
  device: row.device,
  // A tombstone carries no ciphertext (D3), so the two members are absent rather than null.
  ...(row.deleted === 1 ? {} : { nonce: row.nonce ?? "", ciphertext: row.ciphertext ?? "" }),
});

const invalid = (message: string): Outcome => ({
  status: 400,
  error: { code: "invalid-request", message },
});

function find(sql: SqlStorage, collection: string, id: string): RecordRow | null {
  return (
    sql
      .exec<RecordRow>(`SELECT * FROM record WHERE collection = ? AND id = ?`, collection, id)
      .toArray()[0] ?? null
  );
}

function account(sql: SqlStorage): AccountRow {
  return sql.exec<AccountRow>(`SELECT * FROM account WHERE id = 1`).toArray()[0]!;
}

function nextSeq(sql: SqlStorage): number {
  const seq = account(sql).next_seq + 1;
  sql.exec(`UPDATE account SET next_seq = ? WHERE id = 1`, seq);
  return seq;
}

function addStoredBytes(sql: SqlStorage, delta: number): void {
  sql.exec(`UPDATE account SET stored_bytes = MAX(0, stored_bytes + ?) WHERE id = 1`, delta);
}

export function applyPut(
  sql: SqlStorage,
  limits: Capabilities,
  collection: string,
  id: string,
  body: unknown,
  precondition: Precondition,
  device: string,
): Outcome {
  if (!isOpaqueId(collection) || !isOpaqueId(id)) {
    return invalid("Collection and record IDs must be 64 lowercase hex characters.");
  }

  const fields = asObject(body);
  if (
    !fields ||
    !isTimestamp(fields["updatedAt"]) ||
    !isBase64(fields["nonce"], 24) ||
    !isBase64(fields["ciphertext"])
  ) {
    return invalid("A record needs updatedAt, a 24-byte nonce and a ciphertext.");
  }

  const ciphertext = fields["ciphertext"] as string;
  const nonce = fields["nonce"] as string;
  const bytes = decodedLength(ciphertext) + decodedLength(nonce) + ROW_OVERHEAD_BYTES;

  if (decodedLength(ciphertext) > limits.maxRecordBytes) {
    return {
      status: 413,
      error: {
        code: "record-too-large",
        message: "That record is larger than this server accepts.",
        limit: limits.maxRecordBytes,
      },
    };
  }

  if (!precondition.ifNoneMatch && precondition.ifMatch === undefined) {
    return {
      status: 428,
      error: {
        code: "precondition-required",
        message: "Send If-Match with the version you hold, or If-None-Match: * to create.",
      },
    };
  }

  const existing = find(sql, collection, id);

  if (precondition.ifNoneMatch) {
    // A tombstone counts as existing. Treating a deleted row as absent would let a creation slip
    // past a deletion and leave two machines disagreeing about which one won (D4a).
    if (existing) {
      return {
        status: 412,
        record: wire(existing),
        error: { code: "already-exists", message: "That record already exists." },
      };
    }
  } else if (!existing) {
    return { status: 404, error: { code: "unknown-record", message: "No such record." } };
  } else if (existing.version !== precondition.ifMatch) {
    return {
      status: 409,
      record: wire(existing),
      error: { code: "version-conflict", message: "Another device changed this record first." },
    };
  }

  const wasStored = existing?.bytes ?? 0;
  const stored = account(sql).stored_bytes;
  if (stored - wasStored + bytes > limits.accountQuotaBytes) {
    return {
      status: 507,
      error: {
        code: "quota-exceeded",
        message: "This account is full.",
        limit: limits.accountQuotaBytes,
        used: stored,
      },
    };
  }

  const version = (existing?.version ?? 0) + 1;
  const seq = nextSeq(sql);
  const at = Math.floor(Date.now() / 1000);

  sql.exec(
    `INSERT INTO record (collection, id, version, seq, updated_at, deleted, device, nonce, ciphertext,
                         bytes, written_at)
     VALUES (?, ?, ?, ?, ?, 0, ?, ?, ?, ?, ?)
     ON CONFLICT (collection, id) DO UPDATE SET
       version = excluded.version, seq = excluded.seq, updated_at = excluded.updated_at,
       deleted = 0, device = excluded.device, nonce = excluded.nonce,
       ciphertext = excluded.ciphertext, bytes = excluded.bytes, written_at = excluded.written_at`,
    collection,
    id,
    version,
    seq,
    fields["updatedAt"],
    device,
    nonce,
    ciphertext,
    bytes,
    at,
  );
  addStoredBytes(sql, bytes - wasStored);

  return { status: existing ? 200 : 201, record: wire(find(sql, collection, id)!) };
}

export function applyDelete(
  sql: SqlStorage,
  collection: string,
  id: string,
  ifMatch: number | undefined,
  updatedAt: unknown,
  device: string,
): Outcome {
  if (!isOpaqueId(collection) || !isOpaqueId(id)) {
    return invalid("Collection and record IDs must be 64 lowercase hex characters.");
  }
  // D4 weighs a deletion like any other edit, so it needs the time it was made — kept as the
  // tombstone's own, never the replaced version's (T178c, C1).
  if (!isTimestamp(updatedAt)) return invalid("A deletion needs updatedAt.");
  if (ifMatch === undefined) {
    return {
      status: 428,
      error: { code: "precondition-required", message: "Send If-Match with the version you hold." },
    };
  }

  const existing = find(sql, collection, id);
  if (!existing) return { status: 404, error: { code: "unknown-record", message: "No such record." } };
  if (existing.version !== ifMatch) {
    return {
      status: 409,
      record: wire(existing),
      error: { code: "version-conflict", message: "Another device changed this record first." },
    };
  }

  // Already a tombstone: hand back the one that is there, unmoved. Without this, a delete retried
  // after a dropped connection bumps `seq` and every other machine pulls a change that is not one.
  if (existing.deleted === 1) return { status: 200, record: wire(existing) };

  const seq = nextSeq(sql);
  sql.exec(
    `UPDATE record SET version = ?, seq = ?, deleted = 1, device = ?, nonce = NULL,
                       ciphertext = NULL, bytes = 0, written_at = ?, updated_at = ?
     WHERE collection = ? AND id = ?`,
    existing.version + 1,
    seq,
    device,
    Math.floor(Date.now() / 1000),
    updatedAt,
    collection,
    id,
  );
  addStoredBytes(sql, -existing.bytes);

  return { status: 200, record: wire(find(sql, collection, id)!) };
}

export interface Page {
  records: WireRecord[];
  nextSince: number;
  more: boolean;
}

export function listSince(
  sql: SqlStorage,
  limits: Capabilities,
  since: number,
  collection: string | null,
  resync: boolean,
): Outcome | Page {
  if (!Number.isSafeInteger(since) || since < 0) return invalid("`since` is a sequence number.");
  if (collection !== null && !isOpaqueId(collection)) {
    return invalid("A collection ID must be 64 lowercase hex characters.");
  }

  // D3: a machine that has been away longer than a tombstone lives is told to resync from empty
  // rather than told incomplete news quietly. `resync` is a cursor from a read that began at 0,
  // which has missed nothing (T178b, M4).
  const reapedBelow = account(sql).reaped_below_seq;
  if (since > 0 && since < reapedBelow && !resync) {
    return {
      status: 410,
      error: {
        code: "cursor-expired",
        message: "That cursor is too old. Sync again from the beginning.",
      },
    };
  }

  // One more than the page, which is how `more` is known without a second count.
  const limit = limits.maxPageRecords + 1;
  const rows =
    collection === null
      ? sql
          .exec<RecordRow>(`SELECT * FROM record WHERE seq > ? ORDER BY seq ASC LIMIT ?`, since, limit)
          .toArray()
      : sql
          .exec<RecordRow>(
            `SELECT * FROM record WHERE seq > ? AND collection = ? ORDER BY seq ASC LIMIT ?`,
            since,
            collection,
            limit,
          )
          .toArray();

  const more = rows.length > limits.maxPageRecords;
  const page = more ? rows.slice(0, limits.maxPageRecords) : rows;

  const last = page.at(-1)?.seq ?? since;
  return {
    records: page.map(wire),
    // The last page ends at the account's latest seq (M3): a cursor stopped at the collection's own
    // last row would sit below every later write in the account. The Durable Object runs one
    // request at a time, so nothing is written between these reads.
    nextSince: more ? last : Math.max(last, account(sql).next_seq),
    more,
  };
}

export interface Heads {
  stale: string[];
  nextSince: number;
}

/**
 * `POST /v1/records/heads` (T189, D2): which of the cursors a pull would bring news of. A row this
 * device wrote is not news to it, since its push remembered the row; a row of unknown device is.
 * The Durable Object runs one request at a time, so `nextSince` and every test below read one
 * state.
 */
export function staleSince(
  sql: SqlStorage,
  limits: Capabilities,
  body: unknown,
  device: string,
): Outcome | Heads {
  const cursors = asObject(asObject(body)?.["cursors"]);
  const entries = cursors ? Object.entries(cursors) : [];
  if (entries.length === 0 || entries.length > limits.maxBatchOperations) {
    return invalid(`\`cursors\` names between 1 and ${limits.maxBatchOperations} collections.`);
  }
  for (const [collection, since] of entries) {
    if (!isOpaqueId(collection)) return invalid("A collection ID must be 64 lowercase hex characters.");
    if (typeof since !== "number" || !Number.isSafeInteger(since) || since < 0) {
      return invalid("A cursor is a sequence number.");
    }
  }

  const { reaped_below_seq: reapedBelow, next_seq: nextSince } = account(sql);
  const stale = entries
    .filter(([collection, value]) => {
      const since = value as number;
      // A pull would answer 410 (D3): stale, so the client meets it and resyncs.
      if (since > 0 && since < reapedBelow) return true;
      const rows =
        since === 0
          ? sql.exec(`SELECT 1 FROM record WHERE collection = ? LIMIT 1`, collection)
          : sql.exec(
              `SELECT 1 FROM record WHERE collection = ? AND seq > ? AND device != ? LIMIT 1`,
              collection,
              since,
              device,
            );
      return rows.toArray().length > 0;
    })
    .map(([collection]) => collection);
  return { stale, nextSince };
}
