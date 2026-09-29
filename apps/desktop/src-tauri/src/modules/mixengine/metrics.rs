//! Keeps `GET /metrics` open and pushes each `MetricsFrame` up to the UI — a mirror of
//! `events.rs`/`logs.rs`, differing in the route (fixed, no parameters) and the state
//! (`MetricsState`, not `LogsState`/`MixEngineState`).
//!
//! **Opening this connection is the subscription.** MixEngine samples at 1 Hz while someone holds
//! `/metrics`, and falls back to once a minute when nobody does — closing this connection as soon
//! as the screen no longer needs the "now" figures (see `MetricsState`) is required to keep that
//! invariant, not a cleanup detail.

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::error::AppError;

use super::sse::Frames;
use super::state::MetricsState;
use super::transport;

/// Opens `GET /metrics` and runs until cancelled or until the connection drops.
pub async fn stream_metrics(
    on_frame: Channel<String>,
    window: &str,
    state: &MetricsState,
) -> Result<(), AppError> {
    let io = transport::connect().await?;

    let request = Request::builder()
        .method("GET")
        .uri("/metrics")
        .header("host", "mixengine")
        .header("accept", "text/event-stream")
        .body(Full::new(Bytes::new()))
        .map_err(|e| err!("error.mixengineProtocol", message = e))?;

    open(TokioIo::new(io), request, on_frame, window, state).await
}

async fn open<I>(
    io: TokioIo<I>,
    request: Request<Full<Bytes>>,
    on_frame: Channel<String>,
    window: &str,
    state: &MetricsState,
) -> Result<(), AppError>
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, connection) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| err!("error.mixengineProtocol", message = e))?;

    tauri::async_runtime::spawn(async move {
        let _ = connection.await;
    });

    let mut response = sender
        .send_request(request)
        .await
        .map_err(|e| err!("error.mixengineProtocol", message = e))?;

    let token = CancellationToken::new();
    state.keep(window, token.clone());

    tauri::async_runtime::spawn(async move {
        let _sender = sender;
        let mut frames = Frames::new();
        loop {
            tokio::select! {
                _ = token.cancelled() => return,
                next = response.frame() => {
                    let Some(Ok(frame)) = next else { break };
                    let Some(bytes) = frame.data_ref() else { continue };
                    for message in frames.push(&String::from_utf8_lossy(bytes)) {
                        if on_frame.send(message).is_err() {
                            return;
                        }
                    }
                }
            }
        }
        // The connection dropped: nothing of its own to emit — the frontend treats "no new frames"
        // as the end of the figures itself, and reopens exactly when `active` goes back to `true`.
    });

    Ok(())
}
