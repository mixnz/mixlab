//! Proving the password again: checking a verifier, and deleting the account it opens (D4b).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

use super::{
    LOGIN_WINDOW_SECONDS, authenticate, internal, parse, sized_field, too_many_attempts, window,
};
use crate::AppState;
use crate::crypto::{VERIFIER_BYTES, peppered, same_secret};
use crate::http::{Failure, invalid_request, invalid_token};

/// **Deleting an account leaves this server as it was before the account existed** (D4b): no
/// tombstone, no row saying the address was once here, and the address free to register again.
/// Keeping any of it would be keeping the one fact D1 promises a server does not accumulate.
///
/// Every other table names `account_id` with `ON DELETE CASCADE` and `PRAGMA foreign_keys` is on,
/// so removing the one row removes the records, the devices, the tokens and the counters with it.
/// The verifier `presented` against this account's, counted in the login window, so that no route
/// that asks for `A` is a cheaper way to guess it than signing in. `Some` is the refusal to send.
fn prove_verifier(
    connection: &Connection,
    account_id: i64,
    presented: &str,
    allowance: u64,
) -> rusqlite::Result<Option<Failure>> {
    if let Some(seconds) = window(
        connection,
        "attempt",
        &account_id,
        "login",
        allowance,
        LOGIN_WINDOW_SECONDS,
    )? {
        return Ok(Some(too_many_attempts(seconds)));
    }
    let stored: Option<String> = connection
        .query_row(
            "SELECT verifier FROM account WHERE id = ?1",
            params![account_id],
            |row| row.get(0),
        )
        .optional()?;
    if !stored.is_some_and(|stored| same_secret(&stored, presented)) {
        return Ok(Some(Failure::new(
            StatusCode::UNAUTHORIZED,
            "invalid-credentials",
            "That password does not match this account.",
        )));
    }
    Ok(None)
}

/// `POST /v1/account/check`: whether `A` is this account's. A move asks before it registers
/// anywhere with it, where a typo is still a typo.
pub async fn check_verifier(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let fields = match parse(&body) {
        Ok(fields) => fields,
        Err(failure) => return failure.into_response(),
    };
    let Some(a) = sized_field(&fields, "a", VERIFIER_BYTES) else {
        return invalid_request("The current verifier is required.").into_response();
    };
    let presented = peppered(&state.config.pepper, a);
    let allowance = state.config.limits.logins_per_window;

    let outcome = state
        .db
        .call(move |connection| {
            let Some(session) = authenticate(connection, &headers) else {
                return Ok(Some(invalid_token()));
            };
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let refusal = prove_verifier(&transaction, session.account_id, &presented, allowance)?;
            transaction.commit()?;
            Ok(refusal)
        })
        .await;

    match outcome {
        Err(error) => internal(error).into_response(),
        Ok(Some(failure)) => failure.into_response(),
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
    }
}

pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let fields = match parse(&body) {
        Ok(fields) => fields,
        Err(failure) => return failure.into_response(),
    };
    let Some(a) = sized_field(&fields, "a", VERIFIER_BYTES) else {
        return invalid_request("The current verifier is required.").into_response();
    };

    let presented = peppered(&state.config.pepper, a);
    let allowance = state.config.limits.logins_per_window;

    let outcome = state
        .db
        .call(move |connection| {
            let Some(session) = authenticate(connection, &headers) else {
                return Ok(Err(invalid_token()));
            };
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let account_id = session.account_id;

            // **A session is not enough.** A borrowed unlocked machine already holds one, so this
            // asks for the verifier: the person deleting the account is then the person who knows
            // the password.
            if let Some(failure) = prove_verifier(&transaction, account_id, &presented, allowance)?
            {
                transaction.commit()?;
                return Ok(Err(failure));
            }

            let records_deleted: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM record WHERE account_id = ?1",
                params![account_id],
                |row| row.get(0),
            )?;

            // **A frozen account may be deleted**, because the last step of a move is deleting the
            // source and the source is frozen at that point. Refusing here would mean thawing
            // first, which is a window for a second machine to write something the copy never saw.
            transaction.execute("DELETE FROM account WHERE id = ?1", params![account_id])?;
            transaction.commit()?;
            Ok(Ok(records_deleted))
        })
        .await;

    match outcome {
        Err(error) => internal(error).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        Ok(Ok(records_deleted)) => {
            Json(json!({ "recordsDeleted": records_deleted })).into_response()
        }
    }
}
