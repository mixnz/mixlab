//! One JSON-RPC 2.0 call to `POST /rpc`, and what comes out when it fails.
//!
//! **HTTP status is about the envelope; a JSON-RPC error is about the call.** A failed method is a
//! `200` carrying an `error` member — the request arrived, was parsed, and was answered. The
//! statuses that actually appear are all envelope matters: `400` unreadable body, `404` no such
//! route, `405` with `Allow`, `413` over 1 MiB.
//!
//! **Branch on `error.data.code`, never on the wording.** MixEngine's closed set of codes:
//! `not_found · already_exists · invalid_argument · conflict · precondition_failed · port_in_use ·
//! privileged_required · unsupported_platform · dependency_missing · process_failed · io ·
//! internal`. `message` is not translated: it is the daemon speaking, and a string people can look
//! up — the same rule `error.rs` sets for driver messages.

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::error::AppError;

use super::transport;

/// Calls a method and decodes its result.
pub async fn call<T: DeserializeOwned>(method: &str, params: Value) -> Result<T, AppError> {
    let body = request(
        "POST",
        "/rpc",
        Some(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })),
    )
    .await?;

    let answer: Value =
        serde_json::from_slice(&body).map_err(|e| err!("error.mixengineProtocol", message = e))?;

    if let Some(error) = map_rpc_error(&answer) {
        return Err(error);
    }

    let result = answer
        .get("result")
        .cloned()
        .ok_or_else(|| err!("error.mixengineProtocol", message = "no result member"))?;

    serde_json::from_value(result).map_err(|e| err!("error.mixengineProtocol", message = e))
}

/// An answer's `error` member, as an `AppError`. `None` when the answer is a result.
pub fn map_rpc_error(body: &Value) -> Option<AppError> {
    let error = body.get("error")?;
    let data = error.get("data");
    let code = data
        .and_then(|data| data.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("internal");
    let message = error.get("message").and_then(Value::as_str).unwrap_or("");

    let mut mapped = err!("error.mixengineRefused", code = code, message = message);
    // Absent rather than empty: the UI can tell "no hint" from "an empty hint".
    if let Some(hint) = data
        .and_then(|data| data.get("hint"))
        .and_then(Value::as_str)
    {
        mapped = mapped.with("hint", hint);
    }
    Some(mapped)
}

/// One HTTP/1.1 request over the local transport; returns the raw body.
///
/// Opens a connection per call, then closes it. On a local socket the handshake costs
/// microseconds, and in return there is no pool to maintain and no dead connection to detect.
pub async fn request(verb: &str, path: &str, body: Option<Value>) -> Result<Vec<u8>, AppError> {
    let io = transport::connect().await?;
    let payload = body.map(|value| value.to_string()).unwrap_or_default();

    let outgoing = Request::builder()
        .method(verb)
        .uri(path)
        // An HTTP/1.1 request needs `Host`, and the daemon does not care what it says: there is no
        // name to resolve at the other end of a socket.
        .header("host", "mixengine")
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(payload)))
        .map_err(|e| err!("error.mixengineProtocol", message = e))?;

    // `_sender` has to live until the body is read: dropping it closes the connection, and the
    // body is still flowing over it. Named with an underscore rather than a bare `_` — `_` drops
    // on the spot.
    let (_sender, response) = send(TokioIo::new(io), outgoing).await?;

    let status = response.status();
    let collected = response
        .into_body()
        .collect()
        .await
        .map_err(|e| err!("error.mixengineProtocol", message = e))?
        .to_bytes();

    // A 200 carrying `error` is still a failed call, and it is handled in `call`. Only a status
    // outside the success range is an envelope matter.
    if !status.is_success() {
        return Err(err!(
            "error.mixengineProtocol",
            message = format!("HTTP {status}")
        ));
    }
    Ok(collected.to_vec())
}

/// Performs the HTTP/1.1 handshake on an already open IO and sends one request.
///
/// Generic over the IO type: there used to be two `#[cfg]` branches with two types (pipe and
/// socket) calling in here; since T102 `transport::Io` is a single type from `mixengine-platform`,
/// and the generic stays so this function does not know what that type is.
type Sent = (
    hyper::client::conn::http1::SendRequest<Full<Bytes>>,
    hyper::Response<hyper::body::Incoming>,
);

async fn send<I>(io: TokioIo<I>, request: Request<Full<Bytes>>) -> Result<Sent, AppError>
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, connection) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| err!("error.mixengineProtocol", message = e))?;

    // The connection has to be driven while the request is in flight; it ends when `sender` is
    // dropped.
    tauri::async_runtime::spawn(async move {
        let _ = connection.await;
    });

    let response = sender
        .send_request(request)
        .await
        .map_err(|e| err!("error.mixengineProtocol", message = e))?;

    Ok((sender, response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A failed method is HTTP 200 carrying an `error` member. What must come out is `data.code` —
    /// the stable code — not the wording, and `hint` is what the UI draws as a suggested action.
    #[test]
    fn a_refusal_carries_the_stable_code_and_the_hint() {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "mariadb@main is not installed",
                "data": { "code": "not_found", "hint": "mix service create mariadb" }
            }
        });
        let error = map_rpc_error(&body).expect("an error member must map");
        assert_eq!(error.code, "error.mixengineRefused");
        assert_eq!(error.params.get("code"), Some(&"not_found".to_string()));
        assert_eq!(
            error.params.get("hint"),
            Some(&"mix service create mariadb".to_string())
        );
        assert_eq!(
            error.params.get("message"),
            Some(&"mariadb@main is not installed".to_string())
        );
    }

    /// With no `hint` the parameter is absent, not an empty string.
    #[test]
    fn a_refusal_without_a_hint_carries_none() {
        let body = json!({
            "jsonrpc": "2.0", "id": 1,
            "error": { "code": -32601, "message": "no such method", "data": { "code": "not_found" } }
        });
        let error = map_rpc_error(&body).unwrap();
        assert_eq!(error.params.get("hint"), None);
    }

    /// An `error` without `data.code` must still produce a readable error, not `None`.
    #[test]
    fn an_error_without_a_data_code_still_maps() {
        let body = json!({
            "jsonrpc": "2.0", "id": 1,
            "error": { "code": -32700, "message": "parse error" }
        });
        let error = map_rpc_error(&body).unwrap();
        assert_eq!(error.code, "error.mixengineRefused");
        assert_eq!(error.params.get("code"), Some(&"internal".to_string()));
    }

    /// Talks for real to a daemon running on this machine.
    ///
    /// `#[ignore]` because it needs an installed, running MixEngine, which CI does not have — run
    /// it with `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture`. This is
    /// the only test that proves the whole chain: the pipe name is derived correctly, the owner
    /// matches, HTTP/1.1 handshakes, and JSON-RPC returns something readable. No pure test can
    /// replace it.
    #[tokio::test]
    #[ignore]
    async fn a_live_daemon_answers_its_own_status() {
        let address = super::super::transport::current_address().expect("an address");
        println!("endpoint: {address}");

        let status: Value = call("daemon.status", json!({}))
            .await
            .expect("daemon.status");
        println!("status: {status:#}");
        assert!(
            status.get("version").and_then(Value::as_str).is_some(),
            "{status}"
        );
        assert!(
            status.get("home").and_then(Value::as_str).is_some(),
            "{status}"
        );

        // `service.list` returns `{ services: [...] }` — an object, not a bare array. This is
        // exactly what only a real daemon tells you, and why the frontend types against
        // `ServiceList`.
        let services: Value = call("service.list", json!({})).await.expect("service.list");
        println!("services: {services:#}");
        assert!(
            services.get("services").is_some_and(Value::is_array),
            "{services}"
        );
    }

    /// Several calls in a row, exactly what broke the previous version.
    ///
    /// On Windows the daemon keeps exactly one pipe instance waiting and only sets up its
    /// replacement *after* it has accepted a client. A client dialling in quick succession — which
    /// `presence()` and then the Dashboard do as soon as the tab opens — falls into that gap, and
    /// both the owner read and the open return `ERROR_PIPE_BUSY`. The previous version reported
    /// "daemon not responding" on a machine where the daemon was running normally.
    #[tokio::test]
    #[ignore]
    async fn a_live_daemon_answers_several_calls_in_a_row() {
        for round in 0..5 {
            request("GET", "/health", None)
                .await
                .unwrap_or_else(|e| panic!("/health round {round}: {e:?}"));
            let _: Value = call("daemon.status", json!({}))
                .await
                .unwrap_or_else(|e| panic!("daemon.status round {round}: {e:?}"));
        }
        println!("five rounds of /health + daemon.status, no busy pipe");
    }

    /// A successful answer is not an error.
    #[test]
    fn a_result_is_not_an_error() {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "result": { "version": "0.1.0" } });
        assert!(map_rpc_error(&body).is_none());
    }
}
