//! The rate limits: a fixed window per account or per source, and the refusals they answer with.

use axum::http::{HeaderMap, StatusCode, header};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

use super::SOURCE_WINDOW_SECONDS;
use crate::crypto::now;
use crate::http::Failure;

/// A fixed window. `Some(seconds)` means refuse. The allowance is configuration and is not
/// reported by `/v1/capabilities`: publishing the number that stops abuse helps only the abuser.
pub(crate) fn window(
    connection: &Connection,
    table: &str,
    key: &dyn rusqlite::ToSql,
    action: &str,
    allowance: u64,
    window: i64,
) -> rusqlite::Result<Option<i64>> {
    let at = now();
    let column = if table == "attempt" {
        "account_id"
    } else {
        "source"
    };
    let existing: Option<(i64, i64)> = connection
        .query_row(
            &format!("SELECT count, started_at FROM {table} WHERE {column} = ?1 AND action = ?2"),
            params![key, action],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    match existing {
        Some((count, started_at)) if at - started_at < window => {
            if count as u64 >= allowance {
                return Ok(Some(started_at + window - at));
            }
            connection.execute(
                &format!(
                    "UPDATE {table} SET count = count + 1 WHERE {column} = ?1 AND action = ?2"
                ),
                params![key, action],
            )?;
        }
        _ => {
            connection.execute(
                &format!(
                    "INSERT INTO {table} ({column}, action, count, started_at) VALUES (?1, ?2, 1, ?3)
                     ON CONFLICT ({column}, action) DO UPDATE SET count = 1, started_at = excluded.started_at"
                ),
                params![key, action, at],
            )?;
        }
    }
    Ok(None)
}

pub(crate) fn source_for(headers: &HeaderMap) -> String {
    // **Set by this server, never by the request.** `crate::resolve_source` runs before every
    // route, strips whatever arrived under this name, and puts the hash of the address it worked
    // out there instead (D4a). Reading `X-Forwarded-For` here would mean believing a header any
    // caller can write.
    headers
        .get(crate::SOURCE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| "local".to_owned())
}

/// Whether one more letter may be sent to this address this hour, counting it if so.
///
/// **Kept against the address rather than against the account**, and in `source_window` rather
/// than `attempt`, because `attempt` hangs off the account row — and registering over an
/// unverified account deletes that row, which is one of the two ways to ask for a letter. A
/// counter the counted party can clear by asking again is not a counter.
pub(crate) fn may_send_letter(
    connection: &Connection,
    account_key: &str,
    allowance: u64,
) -> rusqlite::Result<Option<i64>> {
    window(
        connection,
        "source_window",
        &format!("letter:{account_key}"),
        "letter",
        allowance,
        SOURCE_WINDOW_SECONDS,
    )
}

/// Per source: somebody's whole network is being noisy.
pub(crate) fn retry_after(seconds: i64) -> Failure {
    throttled(
        "too-many-requests",
        "Too many requests from this network.",
        seconds,
    )
}

/// Per account: somebody is working at one account. A different sentence, and a different thing
/// for a person to be told, so a different code.
pub(crate) fn too_many_attempts(seconds: i64) -> Failure {
    throttled(
        "too-many-attempts",
        "Too many attempts on this account.",
        seconds,
    )
}

fn throttled(code: &'static str, message: &'static str, seconds: i64) -> Failure {
    Failure::new(StatusCode::TOO_MANY_REQUESTS, code, message)
        // In the body as well as the header: a localised "try again in four minutes" needs the
        // number, and a client should not have to remember that one code hides half of itself.
        .with("retryAfter", json!(seconds))
        .header(
            header::RETRY_AFTER,
            seconds
                .to_string()
                .parse()
                .expect("a number is a valid header"),
        )
}
