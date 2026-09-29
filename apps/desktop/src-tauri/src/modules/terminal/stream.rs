use std::time::Duration;

use tokio::sync::mpsc::Receiver;
use tokio::time::Instant;

/// The largest frame sent over IPC at once.
pub const MAX_CHUNK: usize = 64 * 1024;

/// How many read batches queue up before the reader has to stop.
///
/// This is where a session's memory gets its cap. Before this the channel was *unbounded*: `cat`
/// on a file of a few GB, or `yes` while the webview draws slower than the pty produces bytes, grew
/// the queue without limit — nothing stopped it, because an unbounded channel's `send` never
/// waits.
///
/// With a cap, `send` waits, the reader stops, the pty's buffer fills, and the operating system
/// blocks the very process that is writing. That is real backpressure, all the way down to `yes`.
///
/// 64 batches × `READ_BUFFER` (8 KiB) is half a MiB per session: enough not to stall on a hiccup of
/// the webview, small enough that ten tabs running `cat` only cost a few MiB.
pub const QUEUE_DEPTH: usize = 64;

/// The longest the buffer waits before flushing. Short enough that typing shows no lag, long enough
/// that `yes` does not produce thousands of frames a second.
pub const FLUSH_AFTER: Duration = Duration::from_millis(5);

/// Batches the bytes read into frames and hands them to `emit`: flushes right away once there is
/// `MAX_CHUNK`, otherwise flushes once the buffer has sat there for `FLUSH_AFTER`.
///
/// The deadline counts from when the buffer goes from empty to non-empty. Resetting the deadline
/// every time more bytes arrive is a Nagle-style bug: a steady, slow stream would never reach it.
///
/// Returns when `rx` closes — that is, when the reader is done — after flushing what is left. That
/// is what makes `Exit` go out after the last byte rather than before it.
///
/// The incoming channel is capped at {@link QUEUE_DEPTH}: if this function is slower than the
/// reader, the reader has to wait, rather than the queue swelling.
pub async fn coalesce<F>(mut rx: Receiver<Vec<u8>>, mut emit: F)
where
    F: FnMut(Vec<u8>),
{
    let mut buffer: Vec<u8> = Vec::new();
    let mut deadline = Instant::now();

    loop {
        if buffer.is_empty() {
            match rx.recv().await {
                Some(chunk) => {
                    deadline = Instant::now() + FLUSH_AFTER;
                    buffer.extend_from_slice(&chunk);
                }
                None => break,
            }
        } else {
            tokio::select! {
                received = rx.recv() => match received {
                    Some(chunk) => buffer.extend_from_slice(&chunk),
                    None => break,
                },
                _ = tokio::time::sleep_until(deadline) => {
                    emit(std::mem::take(&mut buffer));
                    continue;
                }
            }
        }

        while buffer.len() >= MAX_CHUNK {
            let rest = buffer.split_off(MAX_CHUNK);
            emit(std::mem::replace(&mut buffer, rest));
        }
    }

    if !buffer.is_empty() {
        emit(buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::{coalesce, FLUSH_AFTER, MAX_CHUNK, QUEUE_DEPTH};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;

    type Emitted = Arc<Mutex<Vec<Vec<u8>>>>;

    fn sink() -> (Emitted, impl FnMut(Vec<u8>)) {
        let seen: Emitted = Arc::new(Mutex::new(Vec::new()));
        let handle = seen.clone();
        (seen, move |chunk: Vec<u8>| {
            handle.lock().unwrap().push(chunk)
        })
    }

    #[tokio::test(start_paused = true)]
    async fn flushes_a_small_write_after_the_idle_window() {
        let (tx, rx) = mpsc::channel(QUEUE_DEPTH);
        let (seen, emit) = sink();
        let task = tokio::spawn(coalesce(rx, emit));

        tx.send(b"hi".to_vec()).await.unwrap();
        tokio::time::sleep(FLUSH_AFTER * 2).await;
        assert_eq!(*seen.lock().unwrap(), vec![b"hi".to_vec()]);

        drop(tx);
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn flushes_a_full_chunk_without_waiting() {
        let (tx, rx) = mpsc::channel(QUEUE_DEPTH);
        let (seen, emit) = sink();
        let task = tokio::spawn(coalesce(rx, emit));

        tx.send(vec![b'x'; MAX_CHUNK]).await.unwrap();
        // Let the task run, but do not advance the clock: a full 64KB is flushed right away.
        tokio::task::yield_now().await;
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(seen.lock().unwrap()[0].len(), MAX_CHUNK);

        drop(tx);
        task.await.unwrap();
    }

    /// The 5ms deadline counts from the first byte, not the latest. A steady stream that reset the
    /// deadline on every arrival would never be flushed.
    #[tokio::test(start_paused = true)]
    async fn keeps_the_deadline_from_the_first_byte() {
        let (tx, rx) = mpsc::channel(QUEUE_DEPTH);
        let (seen, emit) = sink();
        let task = tokio::spawn(coalesce(rx, emit));

        for _ in 0..6 {
            tx.send(b"a".to_vec()).await.unwrap();
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(
            !seen.lock().unwrap().is_empty(),
            "12ms trôi qua mà chưa đẩy lần nào"
        );

        drop(tx);
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn never_emits_an_empty_chunk() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
        let (seen, emit) = sink();
        let task = tokio::spawn(coalesce(rx, emit));

        tokio::time::sleep(FLUSH_AFTER * 10).await;
        drop(tx);
        task.await.unwrap();

        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn flushes_what_is_left_when_the_source_ends() {
        let (tx, rx) = mpsc::channel(QUEUE_DEPTH);
        let (seen, emit) = sink();
        let task = tokio::spawn(coalesce(rx, emit));

        tx.send(b"bye".to_vec()).await.unwrap();
        drop(tx);
        task.await.unwrap();

        assert_eq!(*seen.lock().unwrap(), vec![b"bye".to_vec()]);
    }

    /// R30: what an unbounded channel does not have.
    ///
    /// The pty reader writes into this channel. Unbounded, `cat` on a file of a few GB grows the
    /// queue as large as it likes, because `send` never waits. With a cap it waits — and waiting
    /// there means the pty fills up, and then it is the operating system's turn to block the very
    /// process that is writing.
    ///
    /// Runs on a multi-threaded runtime because the consumer here *blocks* its thread, just like a
    /// webview that has stopped drawing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_reader_cannot_get_further_ahead_than_the_queue_is_deep() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
        // The first frame parks the consumer and never lets go.
        let (_hold, parked) = std::sync::mpsc::channel::<()>();
        tokio::spawn(coalesce(rx, move |_| {
            let _ = parked.recv();
        }));

        // Any cap has to be hit before this number; with no cap it is never hit.
        let ceiling = QUEUE_DEPTH * 10;
        let mut sent = 0;
        while sent < ceiling {
            // Wait for the other side to move a little, so that what is measured is "the queue is
            // full" rather than "nobody has had time to take anything yet".
            if tx.try_send(vec![0u8; MAX_CHUNK]).is_err() {
                tokio::time::sleep(Duration::from_millis(20)).await;
                if tx.try_send(vec![0u8; MAX_CHUNK]).is_err() {
                    break;
                }
            }
            sent += 1;
        }

        assert!(
            sent < ceiling,
            "đầu đọc gửi được {sent} lô mà không bị chặn — hàng đợi không có trần"
        );
    }
}
