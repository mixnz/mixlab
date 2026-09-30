//! Keeps `GET /events` open and pushes each message up to the UI.
//!
//! **Nothing is interpreted here** — it carries raw JSON. MixEngine's events are internally tagged,
//! and a variant born in a later version has to reach an older MixLab as an object it can ignore,
//! not as a parse error. Understanding the payload is the frontend's job.
//!
//! **Events are best-effort and never the only way to know the state.** Two things the frontend
//! has to handle both come through here as a message: `{"type":"resync","missed":N}` when the bus
//! on the other side overflows, and [`DISCONNECTED`] when the connection drops. Both mean "read
//! `*.list` again".

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::error::AppError;

use super::sse::Frames;
use super::state::MixEngineState;
use super::transport;

/// The message the window emits itself when the stream drops. The name has a `mixlab_` prefix so
/// it can never collide with a MixEngine `type`, including one added in a later version: the daemon
/// names no event after the window.
pub const DISCONNECTED: &str = r#"{"type":"mixlab_disconnected"}"#;

/// Opens the stream and runs until cancelled or until the connection drops.
///
/// Returns as soon as the stream is open; the reading runs on a task of its own.
pub async fn stream_events(
    on_event: Channel<String>,
    window: &str,
    state: &MixEngineState,
) -> Result<(), AppError> {
    let io = transport::connect().await?;

    let request = Request::builder()
        .method("GET")
        .uri("/events")
        .header("host", "mixengine")
        .header("accept", "text/event-stream")
        .body(Full::new(Bytes::new()))
        .map_err(|e| err!("error.mixengineProtocol", message = e))?;

    open(TokioIo::new(io), request, on_event, window, state).await
}

/// Handshakes, sends, then hands the reading over to a task.
async fn open<I>(
    io: TokioIo<I>,
    request: Request<Full<Bytes>>,
    on_event: Channel<String>,
    window: &str,
    state: &MixEngineState,
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

    // Only keep the token once the stream has opened: cancelling the running one and only then
    // finding the new one cannot open would leave the user with no stream at all.
    let token = CancellationToken::new();
    state.keep(window, token.clone());

    tauri::async_runtime::spawn(async move {
        // `sender` has to live as long as this task: dropping it closes the connection the body
        // is flowing over.
        let _sender = sender;
        let mut frames = Frames::new();
        loop {
            tokio::select! {
                _ = token.cancelled() => return,
                next = response.frame() => {
                    let Some(Ok(frame)) = next else { break };
                    let Some(bytes) = frame.data_ref() else { continue };
                    for message in frames.push(&String::from_utf8_lossy(bytes)) {
                        if on_event.send(message).is_err() {
                            return;
                        }
                    }
                }
            }
        }
        // A dropped connection is a message, not silence: the frontend reads `*.list` again when
        // it sees it.
        let _ = on_event.send(DISCONNECTED.to_string());
    });

    Ok(())
}
