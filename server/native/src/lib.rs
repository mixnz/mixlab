//! MixLab's sync server over a SQLite file — the implementation somebody runs themselves.
//!
//! **This exists so that `/v1` is a protocol rather than a description of one codebase.** The
//! Worker in `../worker/` is the default instance; this speaks the same protocol over different
//! machinery, and `../conformance/` is written against the document both answer to rather than
//! against either of them. Each implementation is the other's proof, and the suite is what makes
//! that claim checkable rather than asserted.
//!
//! The contract is normative in `docs/specs/2026-09-20-t177-a-copy-only-you-can-read-design.md`,
//! D2 to D4a. When this disagrees with that document, this is the one with the bug.
//!
//! **Nothing here parses a ciphertext.** What the server necessarily sees is listed in full in D1.
//!
//! # Why this is a library with a binary on top
//!
//! The binary is a `main` that reads the environment and serves [`router`]. Everything else is
//! here, which keeps it reachable from a test in this crate without a port and a process.

pub mod accounts;
pub mod config;
pub mod crypto;
pub mod db;
pub mod devices;
pub mod email;
pub mod freeze;
pub mod http;
pub mod reaper;
pub mod records;
pub mod recovery;
pub mod validate;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Query, Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use config::Config;
use db::Db;
use rusqlite::params;
use serde_json::json;

pub struct AppState {
    pub config: Config,
    pub db: Arc<Db>,
}

/// Every route this server answers. Taking a [`Config`] rather than reading the environment is
/// what lets a test run two of these with different limits in one process.
/// The header `crate::accounts::source_for` reads, and the only one it reads. It is written by
/// `resolve_source` on the way in and stripped from whatever arrived, so it names an address this
/// server worked out rather than one a caller asked to be counted as.
pub(crate) const SOURCE_HEADER: &str = "x-mixlab-source";

/// **Who a request is from is decided here, and never taken from the request** (D4a).
///
/// `X-Forwarded-For` is a header like any other: a server reachable directly that believed it
/// would let anybody mint a fresh bucket per request by writing a different value, which is not a
/// weakened rate limit but no rate limit at all. So the peer address is what counts, unless the
/// deployment says it is behind a proxy.
/// What a request is counted under. **An IPv6 address counts by its /64**: that is what one
/// subscriber is handed, and every address in it is theirs to use, so a counter keyed by the full
/// address started from zero on every request. An IPv4 address counts alone, however it is
/// written; anything else is its own bucket.
fn source_bucket(address: &str) -> String {
    match address.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4.to_string(),
        Ok(IpAddr::V6(v6)) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let segments = v6.segments();
                format!(
                    "{:x}:{:x}:{:x}:{:x}::/64",
                    segments[0], segments[1], segments[2], segments[3]
                )
            }
        },
        Err(_) => address.to_owned(),
    }
}

async fn resolve_source(request: Request, next: Next, trust_forwarded_for: bool) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip().to_string());

    let forwarded = if trust_forwarded_for {
        request
            .headers()
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            // **The last entry, not the first.** A proxy that appends leaves the address it saw at
            // the end; a client that writes its own value leaves it at the front. The end is the
            // only part of this header a client cannot choose.
            .and_then(|value| value.rsplit(',').next())
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    } else {
        None
    };

    // **One shared bucket is the honest answer** to a request whose address this server cannot
    // see, rather than not counting it at all.
    let source = crypto::sha256_hex(&source_bucket(
        &forwarded.or(peer).unwrap_or_else(|| "local".to_owned()),
    ));

    let mut request = request;
    let headers = request.headers_mut();
    headers.remove(SOURCE_HEADER);
    if let Ok(value) = axum::http::HeaderValue::from_str(&source) {
        headers.insert(SOURCE_HEADER, value);
    }
    next.run(request).await
}

pub fn router(state: Arc<AppState>) -> Router {
    let max_batch_bytes = state.config.capabilities.max_batch_bytes;
    let gate = state.config.access_token.clone();
    let trust_forwarded_for = state.config.trust_forwarded_for;
    let gate_state = Arc::clone(&state);
    Router::new()
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/auth/params", get(accounts::params))
        .route("/v1/auth/register", post(accounts::register))
        // **There is no GET here.** The letter carries a code the person types, not a link they
        // click (D4a), so there is no page to serve and no public address to get wrong.
        .route("/v1/auth/verify", post(accounts::verify))
        .route("/v1/auth/login", post(accounts::login))
        .route("/v1/auth/refresh", post(accounts::refresh))
        .route("/v1/auth/password", post(recovery::change_password))
        .route("/v1/auth/reset", post(recovery::reset))
        .route("/v1/devices", get(devices::list))
        .route("/v1/devices/{id}", delete(devices::remove))
        .route("/v1/account/freeze", get(freeze::read).post(freeze::set))
        .route("/v1/account/delete", post(accounts::delete_account))
        .route("/v1/account/check", post(accounts::check_verifier))
        .route("/v1/records", get(records::list))
        .route("/v1/records/batch", post(records::batch))
        .route("/v1/records/heads", post(records::heads))
        .route(
            "/v1/records/{collection}/{id}",
            put(records::write).delete(records::write),
        )
        .route("/__test__/outbox", get(outbox))
        // An unknown route answers in the one error shape like everything else, rather than with
        // the bare 404 a framework gives for free (D4a).
        .fallback(async || http::not_found())
        // **The body this server will buffer, derived from what it advertises.** Without this the
        // framework's own two-megabyte default refuses batches that every published limit calls
        // legal — a server promising something it cannot honour, and saying so in plain text.
        // Exactly the advertised figure, with no slack: `maxBatchBytes` is the size of the whole
        // encoded body (D4a), so a byte over it is refused here and a byte over it is refused by
        // `../worker/` — which checks the same number before it routes. Slack would have made the
        // two disagree about a band of request sizes that nothing else covers.
        .layer(DefaultBodyLimit::max(
            state.config.capabilities.max_batch_bytes as usize,
        ))
        .layer(middleware::from_fn(move |request, next| {
            always_json(request, next, max_batch_bytes)
        }))
        // Outermost, so a caller who has not been let in never reaches a route and never has a
        // body buffered for them.
        .layer(middleware::from_fn(move |request, next| {
            let gate = gate.clone();
            let state = Arc::clone(&gate_state);
            async move { closed_host(request, next, gate, state).await }
        }))
        // Outermost of all, because the layer above counts wrong access tokens per source and
        // cannot do that before there is a source.
        .layer(middleware::from_fn(move |request, next| {
            resolve_source(request, next, trust_forwarded_for)
        }))
        .with_state(state)
}

/// **The whole host, or none of it** (D4a). A capabilities document that answered anybody would
/// tell somebody who found the address that the server is there, what it allows, and that it is
/// worth coming back to. A client is given the token before it makes its first request, so there
/// is no order-of-operations problem to solve.
///
/// `None` means the deployment is open, which is what the hosted instances are.
async fn closed_host(
    request: Request,
    next: Next,
    gate: Option<String>,
    state: Arc<AppState>,
) -> Response {
    let Some(expected) = gate else {
        return next.run(request).await;
    };

    let presented = request
        .headers()
        .get("x-mixlab-access")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    if crypto::same_secret(&presented, &expected) {
        return next.run(request).await;
    }

    // The operator picked this string and may have picked a short one, so guessing costs.
    let source = accounts::source_for(request.headers());
    let allowance = state.config.limits.auth_per_hour;
    let throttled = state
        .db
        .call(move |connection| {
            accounts::window(
                connection,
                "source_window",
                &source,
                "access",
                allowance,
                3600,
            )
        })
        .await;
    if let Ok(Some(seconds)) = throttled {
        return accounts::retry_after(seconds).into_response();
    }

    http::Failure::new(
        axum::http::StatusCode::UNAUTHORIZED,
        "invalid-access-token",
        "This server is private. Ask its operator for the access token.",
    )
    .into_response()
}

/// **Nothing leaves here without a code.** A framework answers some things on its own — a method a
/// route does not have, a body it will not buffer — and it answers them in plain text. MixLab picks
/// its sentence by `code` so that it can be translated (D4a), and an answer with no code is an
/// answer it can only show in English or not at all.
async fn always_json(request: Request, next: Next, max_batch_bytes: u64) -> Response {
    let response = next.run(request).await;
    let status = response.status();
    if !status.is_client_error() && !status.is_server_error() {
        return response;
    }
    let already = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("json"));
    if already {
        return response;
    }

    let mut members = serde_json::Map::new();
    let (code, message) = match status {
        axum::http::StatusCode::METHOD_NOT_ALLOWED => (
            "method-not-allowed",
            "That route does not accept this method.",
        ),
        axum::http::StatusCode::PAYLOAD_TOO_LARGE => {
            // The number a client needs in order to chunk differently next time.
            members.insert("limit".to_owned(), json!(max_batch_bytes));
            (
                "request-too-large",
                "That request is larger than this server accepts.",
            )
        }
        axum::http::StatusCode::NOT_FOUND => ("not-found", "No such route."),
        axum::http::StatusCode::BAD_REQUEST => {
            ("invalid-request", "Something in that request is malformed.")
        }
        _ => ("server-error", "Something went wrong on the server."),
    };
    let mut failure = http::Failure::new(status, code, message);
    failure.members = members;
    failure.into_response()
}

/// Answered from configuration, without touching the database and without an account: a client
/// reads this before it has one (D4a).
async fn capabilities(State(state): State<Arc<AppState>>) -> Response {
    (
        [(axum::http::header::CACHE_CONTROL, "public, max-age=3600")],
        axum::Json(state.config.capabilities.clone()),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
struct Link {
    email: Option<String>,
}

/// Verification arrives by email, which no HTTP suite can read, so a server under test hands the
/// tokens back here — **and answers `404` unless it was started with that mode deliberately**. It
/// is outside `/v1` so the frozen surface stays frozen and a deployed server cannot be asked
/// for it.
async fn outbox(State(state): State<Arc<AppState>>, Query(link): Query<Link>) -> Response {
    if !state.config.test_outbox {
        return http::not_found().into_response();
    }
    let Some(email) = link.email.filter(|email| validate::is_email(email)) else {
        return http::not_found().into_response();
    };
    let key = crypto::account_key(&email);

    let messages = state
        .db
        .call(move |connection| {
            let mut statement = connection.prepare(
                "SELECT o.kind, o.token, o.sent_at FROM outbox o
                 JOIN account a ON a.id = o.account_id
                 WHERE a.account_key = ?1 ORDER BY o.id ASC",
            )?;
            statement
                .query_map(params![key], |row| {
                    Ok(json!({
                        "kind": row.get::<_, String>(0)?,
                        "token": row.get::<_, String>(1)?,
                        "sentAt": row.get::<_, i64>(2)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .await;

    match messages {
        Ok(messages) => axum::Json(json!({ "messages": messages })).into_response(),
        Err(error) => {
            tracing::error!("database error: {error}");
            http::not_found().into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::source_bucket;

    /// One IPv6 subscriber holds a whole /64 and can use a fresh address for every request; keyed
    /// by the full address, every per-source counter started from zero each time.
    #[test]
    fn an_ipv6_address_is_counted_by_its_64() {
        assert_eq!(
            source_bucket("2001:db8:1:2:aaaa::1"),
            source_bucket("2001:db8:1:2:ffff:ffff:ffff:ffff")
        );
        assert_ne!(
            source_bucket("2001:db8:1:2::1"),
            source_bucket("2001:db8:1:3::1")
        );
    }

    #[test]
    fn an_ipv4_address_is_counted_alone_however_it_is_written() {
        assert_ne!(source_bucket("192.0.2.1"), source_bucket("192.0.2.2"));
        assert_eq!(
            source_bucket("192.0.2.1"),
            source_bucket("::ffff:192.0.2.1")
        );
    }

    /// Something that is not an address is still one bucket of its own, never none.
    #[test]
    fn what_is_not_an_address_keeps_its_own_bucket() {
        assert_eq!(source_bucket("local"), "local");
    }
}
