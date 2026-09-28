//! The record table: a compare-and-swap, a cursor, and a byte count. Nothing here opens a record.
//!
//! **Every write below is one `BEGIN IMMEDIATE` transaction**, and that is the difference from the
//! Worker rather than a habit. There, the object is the only thing running, so read-then-write is
//! safe and `next_seq` can be a column read and incremented in the open. Here several requests are
//! in flight against one file, so the read and the write have to be one thing or the read tells
//! you nothing by the time you act on it.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::accounts::{Session, authenticate, parse};
use crate::config::Capabilities;
use crate::crypto::now;
use crate::freeze::guard;
use crate::http::{Failure, invalid_request, invalid_token};
use crate::validate::{decoded_length, is_base64, is_opaque_id};

/// What the quota counts for one record beyond its payload: the row's own metadata, in round
/// numbers. It exists so that an account of ten thousand empty records is not free — the ceiling
/// the quota defends is the account's footprint, not the sum of its ciphertexts.
const ROW_OVERHEAD_BYTES: i64 = 256;

/// A record answer, successful or not. On a 409 or a 412 the current record travels **beside** the
/// error rather than instead of it: the client is the one that resolves a conflict (D4), so it
/// needs the other side of it in the same answer it was refused by.
pub struct Outcome {
    pub status: StatusCode,
    pub record: Option<Value>,
    pub error: Option<Value>,
}

impl Outcome {
    fn ok(status: StatusCode, record: Value) -> Self {
        Self {
            status,
            record: Some(record),
            error: None,
        }
    }

    fn refuse(status: StatusCode, code: &str, message: &str, record: Option<Value>) -> Self {
        Self {
            status,
            record,
            error: Some(json!({ "code": code, "message": message })),
        }
    }

    fn bad(message: &str) -> Self {
        Self::refuse(StatusCode::BAD_REQUEST, "invalid-request", message, None)
    }

    /// The single-record shape: the record's members and the error side by side.
    fn into_response(self) -> Response {
        let mut body = match self.record.clone() {
            Some(Value::Object(members)) => members,
            _ => Map::new(),
        };
        if let Some(error) = self.error.clone() {
            body.insert("error".to_owned(), error);
        }
        let etag = match (&self.record, &self.error) {
            (Some(record), None) => record
                .get("version")
                .and_then(Value::as_i64)
                .map(|version| format!("\"{version}\"")),
            _ => None,
        };
        match etag {
            Some(etag) => (
                self.status,
                [(header::ETAG, etag)],
                axum::Json(Value::Object(body)),
            )
                .into_response(),
            None => (self.status, axum::Json(Value::Object(body))).into_response(),
        }
    }

    /// The batch shape: one entry per operation, status and record kept apart (D4a).
    fn into_entry(self) -> Value {
        let mut entry = Map::new();
        entry.insert("status".to_owned(), json!(self.status.as_u16()));
        if let Some(record) = self.record {
            entry.insert("record".to_owned(), record);
        }
        if let Some(error) = self.error {
            entry.insert("error".to_owned(), error);
        }
        Value::Object(entry)
    }
}

struct Stored {
    version: i64,
    bytes: i64,
    deleted: bool,
}

/// A record as D3 draws it. A struct rather than eight arguments, so that the two opaque halves of
/// the address cannot be handed over the wrong way round by a call site in a hurry.
struct Row<'a> {
    collection: &'a str,
    id: &'a str,
    version: i64,
    seq: i64,
    updated_at: i64,
    deleted: bool,
    device: String,
    nonce: Option<String>,
    ciphertext: Option<String>,
}

fn wire(row: Row<'_>) -> Value {
    let mut record = Map::new();
    record.insert("collection".to_owned(), json!(row.collection));
    record.insert("id".to_owned(), json!(row.id));
    record.insert("version".to_owned(), json!(row.version));
    record.insert("seq".to_owned(), json!(row.seq));
    record.insert("updatedAt".to_owned(), json!(row.updated_at));
    record.insert("deleted".to_owned(), json!(row.deleted));
    record.insert("device".to_owned(), json!(row.device));
    // A tombstone carries no ciphertext (D3), so the two members are absent rather than null.
    if !row.deleted {
        record.insert("nonce".to_owned(), json!(row.nonce.unwrap_or_default()));
        record.insert(
            "ciphertext".to_owned(),
            json!(row.ciphertext.unwrap_or_default()),
        );
    }
    Value::Object(record)
}

fn read_one(
    connection: &Connection,
    account_id: i64,
    collection: &str,
    id: &str,
) -> rusqlite::Result<Option<(Stored, Value)>> {
    connection
        .query_row(
            "SELECT version, seq, updated_at, deleted, nonce, ciphertext, bytes, device FROM record
             WHERE account_id = ?1 AND collection = ?2 AND id = ?3",
            params![account_id, collection, id],
            |row| {
                let deleted: i64 = row.get(3)?;
                Ok((
                    Stored {
                        version: row.get(0)?,
                        bytes: row.get(6)?,
                        deleted: deleted == 1,
                    },
                    wire(Row {
                        collection,
                        id,
                        version: row.get(0)?,
                        seq: row.get(1)?,
                        updated_at: row.get(2)?,
                        deleted: deleted == 1,
                        device: row.get(7)?,
                        nonce: row.get(4)?,
                        ciphertext: row.get(5)?,
                    }),
                ))
            },
        )
        .optional()
}

fn next_seq(connection: &Connection, account_id: i64) -> rusqlite::Result<i64> {
    // **The monotonic counter.** The Worker reads and writes this in the open because nothing else
    // is running in its object; here it is a single statement inside the write transaction, which
    // is the same guarantee bought differently.
    connection.execute(
        "UPDATE account SET next_seq = next_seq + 1 WHERE id = ?1",
        params![account_id],
    )?;
    connection.query_row(
        "SELECT next_seq FROM account WHERE id = ?1",
        params![account_id],
        |row| row.get(0),
    )
}

pub struct Precondition {
    pub if_match: Option<i64>,
    pub if_none_match: bool,
}

pub fn precondition_from(headers: &HeaderMap) -> Precondition {
    Precondition {
        if_match: headers
            .get(header::IF_MATCH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim_matches('"').parse().ok()),
        if_none_match: headers
            .get(header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            == Some("*"),
    }
}

/// `writer` is the session making the write: the account it lands in, and the device the record
/// is stamped with (D3). One argument rather than two, because they never arrive separately.
pub fn apply_put(
    connection: &Connection,
    limits: &Capabilities,
    writer: &Session,
    collection: &str,
    id: &str,
    body: &Value,
    precondition: &Precondition,
) -> rusqlite::Result<Outcome> {
    let (account_id, device) = (writer.account_id, writer.device_id.as_str());
    if !is_opaque_id(collection) || !is_opaque_id(id) {
        return Ok(Outcome::bad(
            "Collection and record IDs must be 64 lowercase hex characters.",
        ));
    }

    let (Some(updated_at), Some(nonce), Some(ciphertext)) = (
        body.get("updatedAt")
            .and_then(Value::as_i64)
            .filter(|at| *at >= 0),
        body.get("nonce")
            .and_then(Value::as_str)
            .filter(|value| is_base64(value, Some(24))),
        body.get("ciphertext")
            .and_then(Value::as_str)
            .filter(|value| is_base64(value, None)),
    ) else {
        return Ok(Outcome::bad(
            "A record needs updatedAt, a 24-byte nonce and a ciphertext.",
        ));
    };

    let payload = decoded_length(ciphertext) as i64;
    if payload > limits.max_record_bytes as i64 {
        return Ok(Outcome {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            record: None,
            error: Some(json!({
                "code": "record-too-large",
                "message": "That record is larger than this server accepts.",
                "limit": limits.max_record_bytes,
            })),
        });
    }
    let bytes = payload + decoded_length(nonce) as i64 + ROW_OVERHEAD_BYTES;

    if !precondition.if_none_match && precondition.if_match.is_none() {
        return Ok(Outcome::refuse(
            StatusCode::PRECONDITION_REQUIRED,
            "precondition-required",
            "Send If-Match with the version you hold, or If-None-Match: * to create.",
            None,
        ));
    }

    let existing = read_one(connection, account_id, collection, id)?;

    if precondition.if_none_match {
        // A tombstone counts as existing. Treating a deleted row as absent would let a creation
        // slip past a deletion and leave two machines disagreeing about which one won (D4a).
        if let Some((_, record)) = existing {
            return Ok(Outcome::refuse(
                StatusCode::PRECONDITION_FAILED,
                "already-exists",
                "That record already exists.",
                Some(record),
            ));
        }
    } else {
        match &existing {
            None => {
                return Ok(Outcome::refuse(
                    StatusCode::NOT_FOUND,
                    "unknown-record",
                    "No such record.",
                    None,
                ));
            }
            Some((stored, record)) if Some(stored.version) != precondition.if_match => {
                return Ok(Outcome::refuse(
                    StatusCode::CONFLICT,
                    "version-conflict",
                    "Another device changed this record first.",
                    Some(record.clone()),
                ));
            }
            Some(_) => {}
        }
    }

    let was_stored = existing
        .as_ref()
        .map(|(stored, _)| stored.bytes)
        .unwrap_or(0);
    let used: i64 = connection.query_row(
        "SELECT stored_bytes FROM account WHERE id = ?1",
        params![account_id],
        |row| row.get(0),
    )?;
    if used - was_stored + bytes > limits.account_quota_bytes as i64 {
        return Ok(Outcome {
            status: StatusCode::INSUFFICIENT_STORAGE,
            record: None,
            error: Some(json!({
                "code": "quota-exceeded",
                "message": "This account is full.",
                "limit": limits.account_quota_bytes,
                "used": used,
            })),
        });
    }

    let created = existing.is_none();
    let version = existing
        .as_ref()
        .map(|(stored, _)| stored.version)
        .unwrap_or(0)
        + 1;
    let seq = next_seq(connection, account_id)?;

    connection.execute(
        "INSERT INTO record (account_id, collection, id, version, seq, updated_at, deleted,
                             device, nonce, ciphertext, bytes, written_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT (account_id, collection, id) DO UPDATE SET
           version = excluded.version, seq = excluded.seq, updated_at = excluded.updated_at,
           deleted = 0, device = excluded.device, nonce = excluded.nonce,
           ciphertext = excluded.ciphertext, bytes = excluded.bytes, written_at = excluded.written_at",
        params![
            account_id,
            collection,
            id,
            version,
            seq,
            updated_at,
            device,
            nonce,
            ciphertext,
            bytes,
            now()
        ],
    )?;
    connection.execute(
        "UPDATE account SET stored_bytes = MAX(0, stored_bytes + ?1) WHERE id = ?2",
        params![bytes - was_stored, account_id],
    )?;

    Ok(Outcome::ok(
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        wire(Row {
            collection,
            id,
            version,
            seq,
            updated_at,
            deleted: false,
            device: device.to_owned(),
            nonce: Some(nonce.to_owned()),
            ciphertext: Some(ciphertext.to_owned()),
        }),
    ))
}

pub fn apply_delete(
    connection: &Connection,
    writer: &Session,
    collection: &str,
    id: &str,
    if_match: Option<i64>,
    updated_at: Option<i64>,
) -> rusqlite::Result<Outcome> {
    let (account_id, device) = (writer.account_id, writer.device_id.as_str());
    if !is_opaque_id(collection) || !is_opaque_id(id) {
        return Ok(Outcome::bad(
            "Collection and record IDs must be 64 lowercase hex characters.",
        ));
    }
    // D4 weighs a deletion like any other edit, so it needs the time it was made — kept as the
    // tombstone's own, never the replaced version's (T178c, C1).
    let Some(updated_at) = updated_at.filter(|at| *at >= 0) else {
        return Ok(Outcome::bad("A deletion needs updatedAt."));
    };
    let Some(if_match) = if_match else {
        return Ok(Outcome::refuse(
            StatusCode::PRECONDITION_REQUIRED,
            "precondition-required",
            "Send If-Match with the version you hold.",
            None,
        ));
    };

    let Some((stored, record)) = read_one(connection, account_id, collection, id)? else {
        return Ok(Outcome::refuse(
            StatusCode::NOT_FOUND,
            "unknown-record",
            "No such record.",
            None,
        ));
    };
    if stored.version != if_match {
        return Ok(Outcome::refuse(
            StatusCode::CONFLICT,
            "version-conflict",
            "Another device changed this record first.",
            Some(record),
        ));
    }
    // Already a tombstone: hand back the one that is there, unmoved. Without this, a delete retried
    // after a dropped connection bumps `seq` and every other machine pulls a change that is not one.
    if stored.deleted {
        return Ok(Outcome::ok(StatusCode::OK, record));
    }

    let seq = next_seq(connection, account_id)?;
    connection.execute(
        "UPDATE record SET version = ?1, seq = ?2, deleted = 1, device = ?3, nonce = NULL,
                           ciphertext = NULL, bytes = 0, written_at = ?4, updated_at = ?5
         WHERE account_id = ?6 AND collection = ?7 AND id = ?8",
        params![
            stored.version + 1,
            seq,
            device,
            now(),
            updated_at,
            account_id,
            collection,
            id
        ],
    )?;
    connection.execute(
        "UPDATE account SET stored_bytes = MAX(0, stored_bytes - ?1) WHERE id = ?2",
        params![stored.bytes, account_id],
    )?;

    Ok(Outcome::ok(
        StatusCode::OK,
        wire(Row {
            collection,
            id,
            version: stored.version + 1,
            seq,
            updated_at,
            deleted: true,
            device: device.to_owned(),
            nonce: None,
            ciphertext: None,
        }),
    ))
}

// --- the routes --------------------------------------------------------------------------------

fn server_error(error: impl std::fmt::Display) -> Response {
    tracing::error!("database error: {error}");
    Failure::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "server-error",
        "Something went wrong on the server.",
    )
    .into_response()
}

pub async fn write(
    State(state): State<Arc<AppState>>,
    Path((collection, id)): Path<(String, String)>,
    method: axum::http::Method,
    headers: HeaderMap,
    body: String,
) -> Response {
    if let Some(failure) = guard(&state, &headers).await {
        return failure.into_response();
    }
    let limits = state.config.capabilities.clone();
    let precondition = precondition_from(&headers);
    // Both methods carry a body: a PUT the record, a DELETE the time it was made (T178c, C1).
    let parsed = match parse(&body) {
        Ok(fields) => fields,
        Err(failure) => return failure.into_response(),
    };

    let outcome = state
        .db
        .call(move |connection| {
            let Some(session) = authenticate(connection, &headers) else {
                return Ok(None);
            };
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let outcome = if method == axum::http::Method::PUT {
                apply_put(
                    &transaction,
                    &limits,
                    &session,
                    &collection,
                    &id,
                    &parsed,
                    &precondition,
                )?
            } else {
                apply_delete(
                    &transaction,
                    &session,
                    &collection,
                    &id,
                    precondition.if_match,
                    parsed.get("updatedAt").and_then(Value::as_i64),
                )?
            };
            transaction.commit()?;
            Ok(Some(outcome))
        })
        .await;

    match outcome {
        Err(error) => server_error(error),
        Ok(None) => invalid_token().into_response(),
        Ok(Some(outcome)) => outcome.into_response(),
    }
}

pub async fn batch(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let fields = match parse(&body) {
        Ok(fields) => fields,
        Err(failure) => return failure.into_response(),
    };
    if let Some(failure) = guard(&state, &headers).await {
        return failure.into_response();
    }
    let limits = state.config.capabilities.clone();
    let Some(operations) = fields.get("operations").and_then(Value::as_array).cloned() else {
        return invalid_request("A batch needs a list of operations.").into_response();
    };
    if operations.is_empty() || operations.len() as u64 > limits.max_batch_operations {
        return invalid_request(format!(
            "A batch needs between 1 and {} operations.",
            limits.max_batch_operations
        ))
        .into_response();
    }

    let outcome = state
        .db
        .call(move |connection| {
            let Some(session) = authenticate(connection, &headers) else {
                return Ok(None);
            };
            let used: i64 = connection.query_row(
                "SELECT stored_bytes FROM account WHERE id = ?1",
                params![session.account_id],
                |row| row.get(0),
            )?;
            // The envelope refuses only when the account is already full: past that point every
            // entry would fail alike, and answering once is kinder than a hundred times.
            if used > limits.account_quota_bytes as i64 {
                return Ok(Some(Err(used)));
            }

            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            // Independent compare-and-swaps and **not a transaction between entries** (D4): they
            // share one here only so that the sequence numbers they take are contiguous, and a 409
            // in entry seven is news for the client rather than a failed request.
            let mut results = Vec::with_capacity(operations.len());
            for operation in &operations {
                let collection = operation
                    .get("collection")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let id = operation.get("id").and_then(Value::as_str).unwrap_or("");
                let if_match = operation.get("ifMatch").and_then(Value::as_i64);

                let outcome = match operation.get("op").and_then(Value::as_str) {
                    Some("delete") => apply_delete(
                        &transaction,
                        &session,
                        collection,
                        id,
                        if_match,
                        operation.get("updatedAt").and_then(Value::as_i64),
                    )?,
                    Some("put") => apply_put(
                        &transaction,
                        &limits,
                        &session,
                        collection,
                        id,
                        operation.get("record").unwrap_or(&Value::Null),
                        &Precondition {
                            if_match,
                            if_none_match: operation.get("ifNoneMatch") == Some(&json!(true)),
                        },
                    )?,
                    _ => Outcome::bad("`op` is put or delete."),
                };
                results.push(outcome.into_entry());
            }
            transaction.commit()?;
            Ok(Some(Ok(results)))
        })
        .await;

    match outcome {
        Err(error) => server_error(error),
        Ok(None) => invalid_token().into_response(),
        Ok(Some(Err(used))) => Failure::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "quota-exceeded",
            "This account is full.",
        )
        .with(
            "limit",
            json!(state.config.capabilities.account_quota_bytes),
        )
        .with("used", json!(used))
        .into_response(),
        Ok(Some(Ok(results))) => axum::Json(json!({ "results": results })).into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct Cursor {
    pub since: Option<String>,
    pub collection: Option<String>,
    pub resync: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(cursor): Query<Cursor>,
    headers: HeaderMap,
) -> Response {
    let limits = state.config.capabilities.clone();
    let Some(since) = cursor
        .since
        .as_deref()
        .unwrap_or("0")
        .parse::<i64>()
        .ok()
        .filter(|since| *since >= 0)
    else {
        return invalid_request("`since` is a sequence number.").into_response();
    };
    if let Some(collection) = cursor.collection.as_deref()
        && !is_opaque_id(collection)
    {
        return invalid_request("A collection ID must be 64 lowercase hex characters.")
            .into_response();
    }
    // `resync=1`: this cursor came from a read that began at 0, which has missed nothing, so it is
    // not expired (T178b, M4). Any other value is refused rather than guessed at: a flag that turns
    // expiry off should not also mean `true`, `0` or nothing.
    let resync = match cursor.resync.as_deref() {
        None => false,
        Some("1") => true,
        Some(_) => return invalid_request("`resync` is 1 or absent.").into_response(),
    };

    let outcome = state
        .db
        .call(move |connection| {
            let Some(session) = authenticate(connection, &headers) else {
                return Ok(None);
            };

            // One snapshot for every read below: the pool's WAL connections let a writer commit
            // between two statements, and `nextSince` must never name a seq the rows were read
            // before (T178b, M3).
            let snapshot = connection.transaction()?;

            // D3: a machine that has been away longer than a tombstone lives is told to resync
            // from empty rather than told incomplete news quietly. `since = 0` is exempt — a
            // machine with no history has missed nothing — and so is a cursor from a read that
            // began at 0, for the same reason (M4).
            let (reaped_below, latest): (i64, i64) = snapshot.query_row(
                "SELECT reaped_below_seq, next_seq FROM account WHERE id = ?1",
                params![session.account_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if since > 0 && since < reaped_below && !resync {
                return Ok(Some(Err(())));
            }

            // One more than the page, which is how `more` is known without a second count.
            let limit = limits.max_page_records as i64 + 1;
            let mut rows = Vec::new();
            {
                let mut statement = snapshot.prepare(
                    "SELECT collection, id, version, seq, updated_at, deleted, nonce, ciphertext,
                            device
                     FROM record
                     WHERE account_id = ?1 AND seq > ?2 AND (?3 IS NULL OR collection = ?3)
                     ORDER BY seq ASC LIMIT ?4",
                )?;
                let mut query = statement.query(params![
                    session.account_id,
                    since,
                    cursor.collection,
                    limit
                ])?;
                while let Some(row) = query.next()? {
                    let deleted: i64 = row.get(5)?;
                    let collection: String = row.get(0)?;
                    let id: String = row.get(1)?;
                    rows.push(wire(Row {
                        collection: &collection,
                        id: &id,
                        version: row.get(2)?,
                        seq: row.get(3)?,
                        updated_at: row.get(4)?,
                        deleted: deleted == 1,
                        device: row.get(8)?,
                        nonce: row.get(6)?,
                        ciphertext: row.get(7)?,
                    }));
                }
            }

            let more = rows.len() as u64 > limits.max_page_records;
            rows.truncate(limits.max_page_records as usize);
            let last = rows
                .last()
                .and_then(|record| record.get("seq"))
                .and_then(Value::as_i64)
                .unwrap_or(since);
            // The last page ends at the account's latest seq: nothing of this collection lies
            // between its last row and there, and a cursor stopped at the row would sit below
            // every later write in the account, where one reap expires it for good (M3).
            let next_since = if more { last } else { last.max(latest) };
            snapshot.commit()?;

            Ok(Some(Ok(json!({
                "records": rows,
                "nextSince": next_since,
                "more": more,
            }))))
        })
        .await;

    match outcome {
        Err(error) => server_error(error),
        Ok(None) => invalid_token().into_response(),
        Ok(Some(Err(()))) => Failure::new(
            StatusCode::GONE,
            "cursor-expired",
            "That cursor is too old. Sync again from the beginning.",
        )
        .into_response(),
        Ok(Some(Ok(page))) => axum::Json(page).into_response(),
    }
}

/// `POST /v1/records/heads` (T189, D2): which of the cursors a pull would bring news of. A row
/// this device wrote is not news to it, since its push remembered the row; a row of unknown device
/// is. One snapshot, so `nextSince` never names a seq the tests below did not see (M3).
pub async fn heads(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let fields = match parse(&body) {
        Ok(fields) => fields,
        Err(failure) => return failure.into_response(),
    };
    let limits = state.config.capabilities.clone();
    let cursors = fields.get("cursors").and_then(Value::as_object);
    let count = cursors.map_or(0, Map::len);
    if count == 0 || count as u64 > limits.max_batch_operations {
        return invalid_request(format!(
            "`cursors` names between 1 and {} collections.",
            limits.max_batch_operations
        ))
        .into_response();
    }
    let mut asked = Vec::with_capacity(count);
    for (collection, since) in cursors.into_iter().flatten() {
        if !is_opaque_id(collection) {
            return invalid_request("A collection ID must be 64 lowercase hex characters.")
                .into_response();
        }
        // `as_i64` is `None` for 1.5 and for "5": a cursor is an integer, never coerced.
        let Some(since) = since.as_i64().filter(|since| *since >= 0) else {
            return invalid_request("A cursor is a sequence number.").into_response();
        };
        asked.push((collection.clone(), since));
    }

    let outcome = state
        .db
        .call(move |connection| {
            let Some(session) = authenticate(connection, &headers) else {
                return Ok(None);
            };
            let snapshot = connection.transaction()?;
            let (reaped_below, latest): (i64, i64) = snapshot.query_row(
                "SELECT reaped_below_seq, next_seq FROM account WHERE id = ?1",
                params![session.account_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let mut stale = Vec::new();
            {
                let mut any = snapshot.prepare(
                    "SELECT 1 FROM record WHERE account_id = ?1 AND collection = ?2 LIMIT 1",
                )?;
                let mut another = snapshot.prepare(
                    "SELECT 1 FROM record
                     WHERE account_id = ?1 AND collection = ?2 AND seq > ?3 AND device != ?4
                     LIMIT 1",
                )?;
                for (collection, since) in &asked {
                    // A pull would answer 410 (D3): stale, so the client meets it and resyncs.
                    let is_stale = if *since > 0 && *since < reaped_below {
                        true
                    } else if *since == 0 {
                        any.exists(params![session.account_id, collection])?
                    } else {
                        another.exists(params![
                            session.account_id,
                            collection,
                            since,
                            session.device_id
                        ])?
                    };
                    if is_stale {
                        stale.push(collection.clone());
                    }
                }
            }
            snapshot.commit()?;
            Ok(Some(json!({ "stale": stale, "nextSince": latest })))
        })
        .await;

    match outcome {
        Err(error) => server_error(error),
        Ok(None) => invalid_token().into_response(),
        Ok(Some(answer)) => axum::Json(answer).into_response(),
    }
}
