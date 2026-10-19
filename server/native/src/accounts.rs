//! Accounts, tokens and devices — and the transactions the Durable Object did not need.
//!
//! Every place below that says `BEGIN IMMEDIATE` is standing in for something the Worker gets from
//! its object being the only thing running. The comments name which one, because that is the
//! difference the second implementation exists to make visible.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::AppState;
use crate::crypto::{
    SALT_BYTES, VERIFIER_BYTES, WRAPPED_KEY_BYTES, account_key, invented_salt, normalise_code, now,
    peppered, random_code, random_public_id, random_token, same_secret, sha256_hex,
};
use crate::email::LetterKind;
use crate::http::{Failure, invalid_code, invalid_email, invalid_request, invalid_token};
use crate::validate::{is_base64, is_email};
use axum::extract::Query;

mod limits;
mod verifier;

pub(crate) use limits::{may_send_letter, retry_after, source_for, too_many_attempts, window};
pub use verifier::{check_verifier, delete_account};

pub const ACCESS_TOKEN_SECONDS: i64 = 15 * 60;
const REFRESH_TOKEN_SECONDS: i64 = 90 * 24 * 60 * 60;
const VERIFICATION_TOKEN_SECONDS: i64 = 24 * 60 * 60;
const LOGIN_WINDOW_SECONDS: i64 = 15 * 60;
const SOURCE_WINDOW_SECONDS: i64 = 60 * 60;

pub fn parse(body: &str) -> Result<Value, Failure> {
    serde_json::from_str::<Value>(body)
        .ok()
        .filter(Value::is_object)
        .ok_or_else(|| invalid_request("The body is not a JSON object."))
}

fn text<'a>(fields: &'a Value, name: &str) -> Option<&'a str> {
    fields.get(name).and_then(Value::as_str)
}

/// Every one of these has a fixed length (D4a): the account row is the one thing the per-account
/// quota does not count, so without a bound registration takes as many bytes as anybody sends.
fn sized_field<'a>(fields: &'a Value, name: &str, bytes: usize) -> Option<&'a str> {
    text(fields, name).filter(|value| is_base64(value, Some(bytes)))
}

fn internal(error: impl std::fmt::Display) -> Failure {
    tracing::error!("database error: {error}");
    Failure::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "server-error",
        "Something went wrong on the server.",
    )
}

/// The session behind a bearer token. **The same lookup that authenticates is the one that would
/// notice a revocation**, which is why deleting a device ends both its tokens at once and costs
/// nothing to do (D4a).
pub struct Session {
    pub account_id: i64,
    pub device_id: String,
}

pub fn authenticate(connection: &Connection, headers: &HeaderMap) -> Option<Session> {
    let presented = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;

    connection
        .query_row(
            "SELECT account_id, device_id FROM token
             WHERE hash = ?1 AND kind = 'access' AND expires_at > ?2",
            params![sha256_hex(presented), now()],
            |row| {
                Ok(Session {
                    account_id: row.get(0)?,
                    device_id: row.get(1)?,
                })
            },
        )
        .optional()
        .ok()
        .flatten()
        .inspect(|session| {
            let _ = connection.execute(
                "UPDATE device SET last_seen_at = ?1 WHERE id = ?2",
                params![now(), session.device_id],
            );
        })
}

#[derive(serde::Deserialize)]
pub struct Address {
    pub email: Option<String>,
}

/// What a client needs before it can compute `A` at all. **No authentication**, and an address with
/// no account gets an answer anyway — one nobody can tell from a real one (D4a).
pub async fn params(
    State(state): State<Arc<AppState>>,
    Query(address): Query<Address>,
    headers: HeaderMap,
) -> Response {
    let Some(email) = address.email.filter(|email| is_email(email)) else {
        return invalid_email().into_response();
    };

    let key = account_key(&email);
    let pepper = state.config.pepper.clone();
    let source = source_for(&headers);
    let allowance = state.config.limits.params_per_hour;

    let outcome = state
        .db
        .call(move |connection| {
            // Unauthenticated and askable about any address, so probing is bounded here rather
            // than left to whoever finds the route first.
            if let Some(seconds) = window(
                connection,
                "source_window",
                &source,
                "params",
                allowance,
                SOURCE_WINDOW_SECONDS,
            )? {
                return Ok(Err(seconds));
            }

            let found: Option<(String, u64, u64, u64)> = connection
                .query_row(
                    "SELECT salt_account, argon_m, argon_t, argon_p FROM account
                     WHERE account_key = ?1",
                    params![key],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?;

            Ok(Ok(match found {
                Some((salt, m, t, p)) => json!({
                    "saltAccount": salt,
                    "argon": { "m": m, "t": t, "p": p },
                }),
                None => json!({
                    "saltAccount": invented_salt(&pepper, &key),
                    "argon": { "m": 65536, "t": 3, "p": 4 },
                }),
            }))
        })
        .await;

    match outcome {
        Err(error) => internal(error).into_response(),
        Ok(Err(seconds)) => retry_after(seconds).into_response(),
        Ok(Ok(body)) => Json(body).into_response(),
    }
}

// --- registration ------------------------------------------------------------------------------

pub async fn register(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let fields = match parse(&body) {
        Ok(fields) => fields,
        Err(failure) => return failure.into_response(),
    };

    let (Some(email), Some(a), Some(salt), Some(wrapped_password), Some(wrapped_recovery)) = (
        text(&fields, "email").filter(|value| is_email(value)),
        sized_field(&fields, "a", VERIFIER_BYTES),
        sized_field(&fields, "saltAccount", SALT_BYTES),
        sized_field(&fields, "wrappedMkPassword", WRAPPED_KEY_BYTES),
        sized_field(&fields, "wrappedMkRecovery", WRAPPED_KEY_BYTES),
    ) else {
        return if text(&fields, "email")
            .filter(|value| is_email(value))
            .is_none()
        {
            invalid_email().into_response()
        } else {
            invalid_request("The request needs a verifier, a salt and two wrapped keys.")
                .into_response()
        };
    };

    let argon = ["m", "t", "p"].map(|name| {
        fields
            .get("argon")
            .and_then(|argon| argon.get(name))
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
    });
    let [Some(m), Some(t), Some(p)] = argon else {
        return invalid_request("The request needs the Argon2 parameters the client used.")
            .into_response();
    };

    let email = email.trim().to_lowercase();
    let key = account_key(&email);
    let verifier = peppered(&state.config.pepper, a);
    let (salt, wrapped_password, wrapped_recovery) = (
        salt.to_owned(),
        wrapped_password.to_owned(),
        wrapped_recovery.to_owned(),
    );
    let source = source_for(&headers);
    let allowance = state.config.limits.registrations_per_hour;
    let letters = state.config.limits.letters_per_account_per_hour;
    let letter_email = email.clone();

    let outcome = state
        .db
        .call(move |connection| {
            // **The registration race.** The Worker settles it by two attempts arriving at one
            // serialized object; here they arrive at one file, so the check and the insert are one
            // write transaction or the check means nothing.
            let transaction = connection.transaction_with_behavior(
                rusqlite::TransactionBehavior::Immediate,
            )?;

            if let Some(seconds) = window(
                &transaction,
                "source_window",
                &source,
                "register",
                allowance,
                SOURCE_WINDOW_SECONDS,
            )? {
                transaction.commit()?;
                return Ok(Err(retry_after(seconds)));
            }

            let existing: Option<(i64, i64)> = transaction
                .query_row(
                    "SELECT id, verified FROM account WHERE account_key = ?1",
                    params![key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            match existing {
                Some((_, 1)) => {
                    transaction.commit()?;
                    return Ok(Err(Failure::new(
                        StatusCode::CONFLICT,
                        "email-taken",
                        "That address already has an account.",
                    )));
                }
                // **An unverified account is replaced rather than defended.** A verification token
                // lives a day; once it expires the person cannot verify, cannot register again,
                // and cannot reset — a reset is only offered to an address that proved itself.
                // Replacing loses nothing, because D4 forbids writing any record before
                // verification, so there is never anything there to lose.
                Some((previous, _)) => {
                    transaction.execute("DELETE FROM account WHERE id = ?1", params![previous])?;
                }
                None => {}
            }

            // **Only once a letter is certain.** Spending this above, before the taken-address
            // check, would let anybody exhaust a verified address's allowance and stop its
            // owner asking for a reset — a refusal that lands on the wrong person.
            if let Some(seconds) = may_send_letter(&transaction, &key, letters)? {
                transaction.commit()?;
                return Ok(Err(too_many_attempts(seconds)));
            }

            transaction.execute(
                "INSERT INTO account (account_key, email, verifier, salt_account, argon_m,
                                      argon_t, argon_p, wrapped_mk_password, wrapped_mk_recovery,
                                      verified, created_at, public_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11)",
                params![
                    key,
                    email,
                    verifier,
                    salt,
                    m,
                    t,
                    p,
                    wrapped_password,
                    wrapped_recovery,
                    now(),
                    random_public_id()
                ],
            )?;
            let account_id = transaction.last_insert_rowid();

            let code = random_code();
            transaction.execute(
                "INSERT INTO mail_token (hash, account_id, kind, expires_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    sha256_hex(&code.replace('-', "")),
                    account_id,
                    LetterKind::Verification.as_str(),
                    now() + VERIFICATION_TOKEN_SECONDS
                ],
            )?;
            transaction.commit()?;
            Ok(Ok(code))
        })
        .await;

    match outcome {
        Err(error) => internal(error).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        Ok(Ok(code)) => {
            // **Registration is not complete until the letter is accepted.** Keeping an account
            // whose letter never went out would hand somebody an address they can never use and
            // never free: registering again answers 409, and `/v1` has no route that re-sends one.
            if deliver(&state, LetterKind::Verification, &letter_email, &code).await {
                (StatusCode::CREATED, Json(json!({}))).into_response()
            } else {
                let _ = state
                    .db
                    .call(move |connection| {
                        connection.execute(
                            "DELETE FROM account WHERE account_key = ?1",
                            params![account_key(&letter_email)],
                        )
                    })
                    .await;
                Failure::new(
                    StatusCode::BAD_GATEWAY,
                    "letter-not-sent",
                    "This server could not send the confirmation letter, so no account was made.",
                )
                .into_response()
            }
        }
    }
}

/// Either records the letter where the suite can read it, or sends it. `false` means the provider
/// refused it, and the caller decides what that costs.
pub async fn deliver(state: &Arc<AppState>, kind: LetterKind, email: &str, code: &str) -> bool {
    if state.config.test_outbox {
        let (email, code, kind) = (email.to_owned(), code.to_owned(), kind.as_str());
        let _ = state
            .db
            .call(move |connection| {
                connection.execute(
                    "INSERT INTO outbox (account_id, kind, token, sent_at)
                     SELECT id, ?2, ?3, ?4 FROM account WHERE account_key = ?1",
                    params![account_key(&email), kind, code, now()],
                )
            })
            .await;
        return true;
    }

    // **A code, not a link** (D4a). A link would need this server to know the address it is
    // reachable at, which is a setting that is wrong silently until the first person clicks one.
    match crate::email::send(&state.config, kind, email, code).await {
        Ok(()) => true,
        Err(reason) => {
            tracing::error!("could not send the {} letter: {reason}", kind.as_str());
            false
        }
    }
}

// --- verification, signing in, tokens ----------------------------------------------------------

pub async fn verify(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let fields = match parse(&body) {
        Ok(fields) => fields,
        Err(failure) => return failure.into_response(),
    };
    let Some(key) = text(&fields, "email").map(account_key) else {
        return bad_code().into_response();
    };
    let code = text(&fields, "token").and_then(normalise_code);
    let allowance = state.config.limits.verify_attempts_per_window;
    let source = source_for(&headers);
    let per_source = state.config.limits.auth_per_hour;

    let outcome = state
        .db
        .call(move |connection| {
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

            // Counted per source as well as per account: a per-account counter cannot see somebody
            // working through a list of addresses.
            if let Some(seconds) = window(
                &transaction,
                "source_window",
                &source,
                "auth",
                per_source,
                SOURCE_WINDOW_SECONDS,
            )? {
                transaction.commit()?;
                return Ok(Err(retry_after(seconds)));
            }

            let account: Option<(i64, i64)> = transaction
                .query_row(
                    "SELECT id, verified FROM account WHERE account_key = ?1",
                    params![key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            // **Eight characters are only safe because guessing is bounded** (D4a). Counted
            // before the code is looked at, so a wrong one costs an attempt whatever was wrong
            // about it.
            if let Some((account_id, _)) = account
                && let Some(seconds) = window(
                    &transaction,
                    "attempt",
                    &account_id,
                    "verify",
                    allowance,
                    LOGIN_WINDOW_SECONDS,
                )?
            {
                transaction.commit()?;
                return Ok(Err(too_many_attempts(seconds)));
            }

            // Already verified, a spent code and a wrong one look alike on purpose: the sentence a
            // person needs is the same in all three cases.
            let (Some((account_id, 0)), Some(code)) = (account, code) else {
                transaction.commit()?;
                return Ok(Ok(false));
            };
            if !spend(&transaction, account_id, &code, LetterKind::Verification)? {
                transaction.commit()?;
                return Ok(Ok(false));
            }
            transaction.execute(
                "UPDATE account SET verified = 1 WHERE id = ?1",
                params![account_id],
            )?;
            transaction.commit()?;
            Ok(Ok(true))
        })
        .await;

    match outcome {
        Err(error) => internal(error).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        Ok(Ok(false)) => bad_code().into_response(),
        Ok(Ok(true)) => Json(json!({})).into_response(),
    }
}

pub(crate) fn bad_code() -> Failure {
    invalid_code()
}

pub(crate) fn spend(
    connection: &Connection,
    account_id: i64,
    presented: &str,
    kind: LetterKind,
) -> rusqlite::Result<bool> {
    spend_kind(connection, account_id, presented, kind.as_str())
}

/// The same, for a token that is not a letter. **A reset ticket has no message** and must never
/// reach the letter composer, so it is a kind rather than a `LetterKind` (D6).
pub(crate) fn spend_kind(
    connection: &Connection,
    account_id: i64,
    presented: &str,
    kind: &str,
) -> rusqlite::Result<bool> {
    let hash = sha256_hex(presented);
    let found: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM mail_token
             WHERE hash = ?1 AND account_id = ?2 AND kind = ?3 AND used = 0 AND expires_at > ?4",
            params![hash, account_id, kind, now()],
            |row| row.get(0),
        )
        .optional()?;
    if found.is_none() {
        return Ok(false);
    }
    connection.execute(
        "UPDATE mail_token SET used = 1 WHERE hash = ?1",
        params![hash],
    )?;
    Ok(true)
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let fields = match parse(&body) {
        Ok(fields) => fields,
        Err(failure) => return failure.into_response(),
    };
    let (Some(email), Some(a), Some(device_name)) = (
        text(&fields, "email").filter(|value| is_email(value)),
        sized_field(&fields, "a", VERIFIER_BYTES),
        text(&fields, "deviceName").filter(|value| !value.trim().is_empty() && value.len() <= 128),
    ) else {
        return if text(&fields, "email")
            .filter(|value| is_email(value))
            .is_none()
        {
            invalid_email().into_response()
        } else if sized_field(&fields, "a", VERIFIER_BYTES).is_none() {
            invalid_request("A verifier is required.").into_response()
        } else {
            Failure::new(
                StatusCode::BAD_REQUEST,
                "invalid-device-name",
                "The device name must be 1 to 128 characters.",
            )
            .into_response()
        };
    };

    let key = account_key(email);
    let presented = peppered(&state.config.pepper, a);
    let device_name = device_name.trim().to_owned();
    let allowance = state.config.limits.logins_per_window;
    let source = source_for(&headers);
    let per_source = state.config.limits.auth_per_hour;

    let outcome =
        state
            .db
            .call(move |connection| {
                let transaction = connection
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                // Counted per source as well as per account, for the same reason as `verify`.
                if let Some(seconds) = window(
                    &transaction,
                    "source_window",
                    &source,
                    "auth",
                    per_source,
                    SOURCE_WINDOW_SECONDS,
                )? {
                    transaction.commit()?;
                    return Ok(Err(retry_after(seconds)));
                }

                let account: Option<(i64, String, i64, String, String, String)> = transaction
                    .query_row(
                        "SELECT id, verifier, verified, wrapped_mk_password, wrapped_mk_recovery,
                            public_id
                     FROM account WHERE account_key = ?1",
                        params![key],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                                row.get(5)?,
                            ))
                        },
                    )
                    .optional()?;

                // An unknown address and a wrong verifier answer alike. Registration has to refuse a
                // taken address and therefore leaks one; this route has no such obligation (D4a).
                let Some((
                    account_id,
                    stored,
                    verified,
                    wrapped_password,
                    wrapped_recovery,
                    public_id,
                )) = account
                else {
                    transaction.commit()?;
                    return Ok(Err(wrong_credentials()));
                };

                if let Some(seconds) = window(
                    &transaction,
                    "attempt",
                    &account_id,
                    "login",
                    allowance,
                    LOGIN_WINDOW_SECONDS,
                )? {
                    transaction.commit()?;
                    return Ok(Err(too_many_attempts(seconds)));
                }

                if !same_secret(&stored, &presented) {
                    transaction.commit()?;
                    return Ok(Err(wrong_credentials()));
                }
                if verified != 1 {
                    transaction.commit()?;
                    return Ok(Err(Failure::new(
                        StatusCode::FORBIDDEN,
                        "email-not-verified",
                        "Confirm the address before signing in.",
                    )));
                }

                let device_id = random_token();
                transaction.execute(
                    "INSERT INTO device (id, account_id, name, created_at, last_seen_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                    params![device_id, account_id, device_name, now()],
                )?;
                let session = issue(&transaction, account_id, &device_id)?;
                transaction.commit()?;
                Ok(Ok((
                    session,
                    device_id,
                    wrapped_password,
                    wrapped_recovery,
                    public_id,
                )))
            })
            .await;

    match outcome {
        Err(error) => internal(error).into_response(),
        Ok(Err(failure)) => failure.into_response(),
        // **Both wrapped copies travel here**, after the verifier matched and nowhere else: a fresh
        // install that could not get them would have an account it cannot read, and one handed out
        // before the password was proved is an offline attack waiting to happen (D4a).
        Ok(Ok(((access, refresh), device_id, wrapped_password, wrapped_recovery, public_id))) => {
            Json(json!({
                "accessToken": access,
                "refreshToken": refresh,
                "deviceId": device_id,
                "expiresIn": ACCESS_TOKEN_SECONDS,
                "wrappedMkPassword": wrapped_password,
                "wrappedMkRecovery": wrapped_recovery,
                "accountId": public_id,
            }))
            .into_response()
        }
    }
}

fn wrong_credentials() -> Failure {
    Failure::new(
        StatusCode::UNAUTHORIZED,
        "invalid-credentials",
        "That address and password do not match an account.",
    )
}

fn issue(
    connection: &Connection,
    account_id: i64,
    device_id: &str,
) -> rusqlite::Result<(String, String)> {
    let access = random_token();
    let refresh = random_token();
    let at = now();
    connection.execute(
        "INSERT INTO token (hash, account_id, kind, device_id, expires_at)
         VALUES (?1, ?2, 'access', ?3, ?4)",
        params![
            sha256_hex(&access),
            account_id,
            device_id,
            at + ACCESS_TOKEN_SECONDS
        ],
    )?;
    connection.execute(
        "INSERT INTO token (hash, account_id, kind, device_id, expires_at)
         VALUES (?1, ?2, 'refresh', ?3, ?4)",
        params![
            sha256_hex(&refresh),
            account_id,
            device_id,
            at + REFRESH_TOKEN_SECONDS
        ],
    )?;
    Ok((access, refresh))
}

pub async fn refresh(State(state): State<Arc<AppState>>, body: String) -> Response {
    let fields = match parse(&body) {
        Ok(fields) => fields,
        Err(failure) => return failure.into_response(),
    };
    let Some(presented) = text(&fields, "refreshToken") else {
        return invalid_token().into_response();
    };
    let hash = sha256_hex(presented);

    let outcome = state
        .db
        .call(move |connection| {
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let row: Option<(i64, String, i64)> = transaction
                .query_row(
                    "SELECT account_id, device_id, rotated FROM token
                     WHERE hash = ?1 AND kind = 'refresh' AND expires_at > ?2",
                    params![hash, now()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;

            let Some((account_id, device_id, rotated)) = row else {
                transaction.commit()?;
                return Ok(None);
            };

            if rotated == 1 {
                // Either it was copied, or two clients raced. Both want the person to sign in
                // again rather than continue quietly, so the device's whole chain goes (D4a).
                transaction
                    .execute("DELETE FROM token WHERE device_id = ?1", params![device_id])?;
                transaction.commit()?;
                return Ok(None);
            }

            transaction.execute(
                "UPDATE token SET rotated = 1 WHERE hash = ?1",
                params![hash],
            )?;
            transaction.execute(
                "DELETE FROM token WHERE device_id = ?1 AND kind = 'access'",
                params![device_id],
            )?;
            let session = issue(&transaction, account_id, &device_id)?;
            transaction.execute(
                "UPDATE device SET last_seen_at = ?1 WHERE id = ?2",
                params![now(), device_id],
            )?;
            transaction.commit()?;
            Ok(Some(session))
        })
        .await;

    match outcome {
        Err(error) => internal(error).into_response(),
        Ok(None) => invalid_token().into_response(),
        Ok(Some((access, refresh))) => Json(json!({
            "accessToken": access,
            "refreshToken": refresh,
            "expiresIn": ACCESS_TOKEN_SECONDS,
        }))
        .into_response(),
    }
}
