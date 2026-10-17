use std::time::{Duration, Instant};

use base64::Engine;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use tauri::State;
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;

use super::models::{RestResponse, WireBody, WireRequest};
use super::state::RestState;
use crate::error::AppError;

/// How much of a response body is kept. Past this it is counted and dropped: everything read has
/// to cross the IPC boundary as base64 and then sit in the webview's memory, and 16 MB of that is
/// already generous. The per-request timeout is what stops an endless body outright.
const MAX_BODY: usize = 16 * 1024 * 1024;

/// Hops before a redirect chain is called a loop.
const MAX_REDIRECTS: usize = 10;

/// The client for these two settings, built once and kept.
fn client_for(
    state: &RestState,
    follow: bool,
    insecure: bool,
) -> Result<reqwest::Client, AppError> {
    let key = (follow, insecure);
    if let Some(client) = state
        .clients
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
    {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .redirect(if follow {
            Policy::limited(MAX_REDIRECTS)
        } else {
            Policy::none()
        })
        .danger_accept_invalid_certs(insecure)
        .build()
        .map_err(|e| err!("error.restBuildFailed", message = e))?;
    state
        .clients
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, client.clone());
    Ok(client)
}

/// Which of the failures reqwest can report this is.
///
/// DNS, a refused connection and a rejected certificate are all `is_connect()` and are not told
/// apart here: doing so means walking `source()` and matching the text of whichever library is
/// underneath, which breaks silently at the next upgrade. The original message travels along
/// instead — "dns error", "certificate verify failed" — where it is worth reading.
fn classify(e: reqwest::Error) -> AppError {
    if e.is_timeout() {
        err!("error.restTimeout", message = e)
    } else if e.is_redirect() {
        err!("error.restRedirectLoop", message = e)
    } else if e.is_connect() {
        err!("error.restConnect", message = e)
    } else {
        err!("error.restBuildFailed", message = e)
    }
}

async fn file_body(path: &str) -> Result<reqwest::Body, AppError> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| err!("error.restFileUnreadable", path = path, message = e))?;
    Ok(reqwest::Body::wrap_stream(ReaderStream::new(file)))
}

/// Sends the request and reads all of it. Split out so `tokio::select!` has one future to race
/// against the cancellation token — dropping this one is what aborts the request.
async fn collect(
    builder: reqwest::RequestBuilder,
    started: Instant,
) -> Result<RestResponse, AppError> {
    let mut res = builder.send().await.map_err(classify)?;
    // `send` returns once the headers are in, so this is when the response began.
    let ttfb_ms = started.elapsed().as_millis() as u64;

    let status = res.status();
    let http_version = format!("{:?}", res.version());
    let final_url = res.url().to_string();
    /* `from_utf8_lossy` and not `to_str`: a header value is bytes, and `to_str` refuses anything
    outside ASCII. A `Content-Disposition` carrying an accented filename — the commonest one by
    far — came back as an empty string, which reads as a header the server did not send rather
    than one this client could not spell. Lossy keeps everything it can and marks the rest. */
    let headers = res
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect::<Vec<_>>();

    // Read in chunks rather than with `bytes()`, so the size can be counted past the point where
    // the bytes stop being kept.
    let mut body: Vec<u8> = Vec::new();
    let mut body_size: u64 = 0;
    while let Some(chunk) = res.chunk().await.map_err(classify)? {
        body_size += chunk.len() as u64;
        if body.len() < MAX_BODY {
            let room = MAX_BODY - body.len();
            body.extend_from_slice(&chunk[..room.min(chunk.len())]);
        }
    }

    Ok(RestResponse {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default().to_string(),
        http_version,
        headers,
        body_base64: base64::engine::general_purpose::STANDARD.encode(&body),
        body_size,
        truncated: body_size > body.len() as u64,
        final_url,
        total_ms: started.elapsed().as_millis() as u64,
        ttfb_ms,
    })
}

/// Sends one request and hands back everything that came of it.
///
/// A `500` is a success here — the send worked, and the response is a response. Only a failure to
/// get one at all is an `Err`.
#[tauri::command]
pub async fn rest_send(
    state: State<'_, RestState>,
    req: WireRequest,
) -> Result<RestResponse, AppError> {
    let client = client_for(&state, req.follow_redirects, req.accept_invalid_certs)?;
    let method = reqwest::Method::from_bytes(req.method.as_bytes())
        .map_err(|e| err!("error.restBuildFailed", message = e))?;
    let url =
        reqwest::Url::parse(&req.url).map_err(|e| err!("error.restInvalidUrl", message = e))?;

    let mut headers = HeaderMap::new();
    for (name, value) in &req.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| err!("error.restBuildFailed", message = e))?;
        let value =
            HeaderValue::from_str(value).map_err(|e| err!("error.restBuildFailed", message = e))?;
        // `append`, not `insert`: two Cookie headers are two headers, and the frontend already
        // decided there should be two.
        headers.append(name, value);
    }

    let mut builder = client.request(method, url).headers(headers);
    // Zero means no limit, the way it does everywhere a timeout is written down. Passed through it
    // would be a deadline of nothing at all, and every request would fail before it was sent.
    if req.timeout_ms > 0 {
        builder = builder.timeout(Duration::from_millis(req.timeout_ms));
    }

    builder = match req.body {
        WireBody::None => builder,
        WireBody::Text { text } => builder.body(text),
        WireBody::File { path } => builder.body(file_body(&path).await?),
        WireBody::Multipart { parts } => {
            let mut form = reqwest::multipart::Form::new();
            for part in parts {
                form = match part.path {
                    Some(path) => {
                        let file_name = std::path::Path::new(&path)
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "file".to_string());
                        let body = file_body(&path).await?;
                        form.part(
                            part.name,
                            reqwest::multipart::Part::stream(body).file_name(file_name),
                        )
                    }
                    None => form.text(part.name, part.value.unwrap_or_default()),
                };
            }
            builder.multipart(form)
        }
    };

    let token = CancellationToken::new();
    state
        .inflight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(req.request_id.clone(), token.clone());
    let started = Instant::now();

    let outcome = tokio::select! {
        // Dropping `collect` is what actually aborts the request; the token only wins the race.
        _ = token.cancelled() => Err(err!("error.restCancelled")),
        result = collect(builder, started) => result,
    };

    state
        .inflight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&req.request_id);
    outcome
}

/// Cuts a send short. Nothing to cancel is not an error — the request finished between the click
/// and this call, which is the commonest way for a Cancel button to be pressed too late.
#[tauri::command]
pub async fn rest_cancel(state: State<'_, RestState>, request_id: String) -> Result<(), AppError> {
    let token = state
        .inflight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&request_id);
    if let Some(token) = token {
        token.cancel();
    }
    Ok(())
}

/// Puts an HTML response where the preview scheme can serve it, and hands back the id it lives
/// under. The frontend turns that into a URL with `convertFileSrc`.
///
/// The two flags are the pane's two switches, and they pick the policy the document is served
/// with — see `preview.rs`. They are not a hint the document can talk its way out of: the
/// document never sees them.
#[tauri::command]
pub fn rest_preview_open(
    state: State<'_, RestState>,
    html: String,
    external: bool,
    scripts: bool,
) -> String {
    super::preview::open(&state, html, external, scripts)
}

/// Drops one, for when the pane closes or either switch is flipped. An id that is already gone is
/// not an error.
#[tauri::command]
pub fn rest_preview_close(state: State<'_, RestState>, id: String) {
    super::preview::close(&state, &id);
}
