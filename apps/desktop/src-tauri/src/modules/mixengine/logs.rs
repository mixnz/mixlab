//! Keeps `GET /logs/{subject}/{id}` open and pushes each frame up to the UI — a mirror of
//! `events.rs`, differing only in the route and the state (`LogsState`, not `MixEngineState`).
//! `Frames` (the SSE parser) is shared, not rewritten. `subject` is `"service"` or `"job"` — the
//! only two kinds `LogSubject` (on MixEngine's side) defines.

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::error::AppError;

use super::sse::Frames;
use super::state::LogsState;
use super::transport;

/// Opens `GET /logs/{subject}/{id}?tail=N&follow=1` and runs until cancelled or until the
/// connection drops.
///
/// `subject` is `"service"` or `"job"` — exactly the two route segments `LogSubject` (in
/// `bindings/`) names, with no third. The route says which kind it is, so there is no need to guess
/// whether a job id is a service name.
pub async fn stream_logs(
    subject: &str,
    id: String,
    tail: u32,
    follow: bool,
    on_line: Channel<String>,
    state: &LogsState,
) -> Result<(), AppError> {
    let io = transport::connect().await?;

    let uri = format!(
        "/logs/{subject}/{id}?tail={tail}&follow={}",
        if follow { 1 } else { 0 }
    );
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", "mixengine")
        .header("accept", "text/event-stream")
        .body(Full::new(Bytes::new()))
        .map_err(|e| err!("error.mixengineProtocol", message = e))?;

    open(TokioIo::new(io), request, on_line, state).await
}

async fn open<I>(
    io: TokioIo<I>,
    request: Request<Full<Bytes>>,
    on_line: Channel<String>,
    state: &LogsState,
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
    state.keep(token.clone());

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
                        if on_line.send(message).is_err() {
                            return;
                        }
                    }
                }
            }
        }
        // The connection dropped: unlike `/events`, there is no "resync" for logs — a Logs tab that
        // reopens sends `tail`/`follow` again from the start, and that is already this stream's
        // "read again".
    });

    Ok(())
}
