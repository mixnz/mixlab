use std::path::Path;

use russh::ChannelMsg;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::models::{Output, OutputSink, TerminalSize};
use super::state::Session;
use super::stream::{coalesce, QUEUE_DEPTH};
use crate::error::AppError;
use crate::ssh::SshConfig;

/// Opens a shell on the server and returns its handle.
///
/// The same `Session` shape as `local::spawn`, so `commands.rs` cannot tell the two kinds of
/// session apart — and does not need to. The difference is contained in this function: two tokio
/// tasks instead of four threads, an SSH channel instead of a pty.
pub async fn spawn(
    ssh: &SshConfig,
    app_data: &Path,
    size: TerminalSize,
    out: OutputSink,
) -> Result<Session, AppError> {
    let (mut read, writer) = crate::ssh::open_shell(ssh, app_data, size.cols, size.rows)
        .await?
        .split();

    let (raw_tx, raw_rx) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (resize_tx, mut resize_rx) = mpsc::unbounded_channel::<TerminalSize>();
    let (exit_tx, exit_rx) = oneshot::channel::<Option<i32>>();
    let kill = CancellationToken::new();

    // Reads the far end. This is the only place holding `raw_tx`, so this task ending is how the
    // batcher knows the bytes are done — and only then is `Exit` emitted.
    tokio::spawn(async move {
        let mut code = None;
        while let Some(msg) = read.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    if raw_tx.send(data.to_vec()).await.is_err() {
                        break;
                    }
                }
                /* A session with a pty usually mixes stderr into stdout, but the server is still
                allowed to keep them apart — and an error line that does not show on screen is
                worse than one shown mixed into other lines. */
                ChannelMsg::ExtendedData { data, .. } => {
                    if raw_tx.send(data.to_vec()).await.is_err() {
                        break;
                    }
                }
                // The exit code arrives before the channel closes. Keep it rather than emitting it
                // right away: the batching buffer may still hold bytes.
                ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status as i32),
                ChannelMsg::Eof | ChannelMsg::Close => break,
                // The two `request_*`s' `Success`/`Failure`, flow control's `WindowAdjusted`.
                // Nothing to do with them.
                _ => {}
            }
        }
        let _ = exit_tx.send(code);
    });

    /* Writing, resizing, closing — one task, because all three go through the same write half, and
    because this is what keeps the SSH session alive. This task returning means the connection
    closes. */
    tokio::spawn({
        let kill = kill.clone();
        async move {
            loop {
                tokio::select! {
                    bytes = input_rx.recv() => match bytes {
                        Some(bytes) => {
                            if writer.write(bytes).await.is_err() {
                                break;
                            }
                        }
                        // The `Session` has been dropped: the tab closed, or the app exited.
                        None => break,
                    },
                    size = resize_rx.recv() => match size {
                        // A failure is ignored: one missed window_change frame does not make the
                        // session wrong, and the next frame will state the latest size again.
                        Some(size) => { let _ = writer.resize(size.cols, size.rows).await; }
                        None => break,
                    },
                    _ = kill.cancelled() => break,
                }
            }
            writer.close().await;
        }
    });

    // One way out, one order: bytes done → buffer done → only then `Exit`. Exactly like
    // `local::spawn`, and for the same reason.
    tokio::spawn(async move {
        coalesce(raw_rx, |chunk| out(Output::Data(chunk))).await;
        let code = exit_rx.await.ok().flatten();
        out(Output::Exit {
            code,
            message: None,
        });
    });

    Ok(Session {
        input: input_tx,
        resize: resize_tx,
        kill,
    })
}
