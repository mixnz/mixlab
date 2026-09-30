//! The channel between two copies of MixLab, so that a second start hands its command line to the
//! window already open and exits.
//!
//! One line of text each way: the caller writes a line, the listener answers `ok`. What the line
//! means is `crate::launch`'s business — this file only carries it. The transport is a named pipe
//! on Windows and a Unix socket elsewhere, both through tokio, both under
//! `<per-user runtime dir>/<app identifier>`.
//!
//! Written rather than taken from `tauri-plugin-single-instance` because that plugin forwards
//! `std::env::args()` verbatim and nothing else, and the thing that has to cross here besides the
//! URL — a password the second process read out of its environment — must not be on any command
//! line. It also does what the plugin did: a plain second start, with no URL, brings the window up.
//!
//! Every function here is `async` so the real thing can be tested on a tokio runtime; `launch.rs`
//! wraps the one call made before the app exists in `block_on` with a timeout.

use std::path::PathBuf;

/// The longest line accepted: a URL and a password, with room to spare. Anything longer is not a
/// message from a copy of this app.
pub const MAX_LINE: usize = 64 * 1024;

/// Where the listener is: a pipe name on Windows, a socket path elsewhere.
#[derive(Clone, Debug)]
pub struct Endpoint {
    path: PathBuf,
}

impl Endpoint {
    /// The app's own endpoint, named after its bundle identifier — one per user, the same for
    /// every copy of this version.
    ///
    /// A debug build gets its own suffix. Without it, `tauri dev` shares the release build's
    /// identifier, so starting it while an installed release copy is running — its terminal tab
    /// counts, since a shell spawned there is still that process — reads as a second copy of the
    /// *same* endpoint: `launch::forward` hands it off and the dev process exits with no window.
    pub fn for_app(identifier: &str) -> Self {
        if cfg!(debug_assertions) {
            Self::named(&format!("{identifier}.dev"))
        } else {
            Self::named(identifier)
        }
    }

    /// An endpoint under any name; tests use one per test.
    pub fn named(name: &str) -> Self {
        Self {
            path: sys::path_for(name),
        }
    }
}

/// Hands `line` to whoever listens on `endpoint`. `true` when it was taken and acknowledged;
/// `false` for no listener, a listener that would not answer, or an endpoint that is not ours.
pub async fn forward(endpoint: &Endpoint, line: &str) -> bool {
    if line.len() > MAX_LINE || line.contains('\n') {
        return false;
    }
    sys::forward(&endpoint.path, line).await
}

/// Whether somebody is answering on `endpoint`, without saying anything to them — roadmap task
/// **T106**.
///
/// **The one question a relaunched copy asks.** It has to wait for the copy it is replacing to go
/// before it can take the endpoint itself, and [`forward`] is not the way to ask: that delivers a
/// line, which brings a window on its way out to the front and asks it to open a tab. This opens the
/// transport and drops it — the listener's `answer` reads EOF, replies `no`, and nothing happens.
pub async fn listening(endpoint: &Endpoint) -> bool {
    sys::listening(&endpoint.path).await
}

/// Listens on `endpoint` for as long as the future is polled, calling `on_line` with each line
/// received. Returns early, after a line on stderr, when the endpoint cannot be taken — another
/// copy holds it, or the place it lives in is not this user's.
pub async fn serve(endpoint: Endpoint, on_line: impl Fn(String) + Send + Sync + 'static) {
    sys::serve(endpoint.path, on_line).await;
}

/// Removes what the listener leaves behind on disk, for the way out. Nothing on Windows.
pub fn cleanup(endpoint: &Endpoint) {
    sys::cleanup(&endpoint.path);
}

/// One exchange on an already open connection, from the listener's side: read a line, hand it on,
/// acknowledge. Shared by both transports.
async fn answer<S>(stream: S, on_line: &(impl Fn(String) + Send + Sync))
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let (read, mut write) = tokio::io::split(stream);
    let mut line = String::new();
    // One byte past the cap is enough to know the line is too long; the rest is never read.
    let mut reader = BufReader::new(read.take(MAX_LINE as u64 + 1));
    match reader.read_line(&mut line).await {
        Ok(n) if n > 0 && n <= MAX_LINE && line.ends_with('\n') => {
            on_line(line.trim_end_matches(['\r', '\n']).to_string());
            let _ = write.write_all(b"ok\n").await;
        }
        _ => {
            let _ = write.write_all(b"no\n").await;
        }
    }
    let _ = write.shutdown().await;
}

/// One exchange from the caller's side, on an already open connection.
async fn ask<S>(stream: S, line: &str) -> bool
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (read, mut write) = tokio::io::split(stream);
    if write.write_all(line.as_bytes()).await.is_err() || write.write_all(b"\n").await.is_err() {
        return false;
    }
    let _ = write.flush().await;
    let mut reply = String::new();
    BufReader::new(read).read_line(&mut reply).await.is_ok() && reply.trim_end() == "ok"
}

#[cfg(windows)]
mod sys {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};

    /// `ERROR_PIPE_BUSY`: every instance of the pipe is taken this instant.
    const PIPE_BUSY: i32 = 231;

    pub fn path_for(name: &str) -> PathBuf {
        PathBuf::from(format!(r"\\.\pipe\{name}"))
    }

    pub async fn forward(path: &Path, line: &str) -> bool {
        // The listener makes the next instance before it reads the current one, so busy is a
        // moment, not a state — a few tries cover it.
        for _ in 0..10 {
            match ClientOptions::new().open(path) {
                Ok(client) => return super::ask(client, line).await,
                Err(e) if e.raw_os_error() == Some(PIPE_BUSY) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(_) => return false,
            }
        }
        false
    }

    pub async fn listening(path: &Path) -> bool {
        // `PIPE_BUSY` means every instance of the name is taken this instant, which is a listener
        // rather than the absence of one — the same reading `forward` gives it.
        match ClientOptions::new().open(path) {
            Ok(_) => true,
            Err(e) => e.raw_os_error() == Some(PIPE_BUSY),
        }
    }

    pub async fn serve(path: PathBuf, on_line: impl Fn(String) + Send + Sync + 'static) {
        // `first_pipe_instance` is the whole of the "is another copy running" question: creating
        // the first instance of a name somebody else owns is refused.
        let mut server = match ServerOptions::new().first_pipe_instance(true).create(&path) {
            Ok(server) => server,
            Err(e) => {
                eprintln!("mixlab: not listening for other copies: {e}");
                return;
            }
        };
        loop {
            if let Err(e) = server.connect().await {
                eprintln!("mixlab: a copy could not reach this one: {e}");
                continue;
            }
            let connected = server;
            // The next instance exists before this one is read, so a caller arriving now finds a
            // pipe rather than `PIPE_BUSY`.
            server = match ServerOptions::new().create(&path) {
                Ok(server) => server,
                Err(e) => {
                    eprintln!("mixlab: stopped listening for other copies: {e}");
                    super::answer(connected, &on_line).await;
                    return;
                }
            };
            super::answer(connected, &on_line).await;
        }
    }

    pub fn cleanup(_path: &Path) {}
}

#[cfg(unix)]
mod sys {
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    /// `$XDG_RUNTIME_DIR` (Linux, per user and `0700`), else `$TMPDIR` (macOS, per user), else
    /// `/tmp` — which is shared, and why [`ours`] exists.
    pub fn path_for(name: &str) -> PathBuf {
        let dir = ["XDG_RUNTIME_DIR", "TMPDIR"]
            .iter()
            .find_map(|var| std::env::var_os(var).filter(|value| !value.is_empty()))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        dir.join(format!("{name}.sock"))
    }

    /// Whether the file at `path` is this user's, or there is no file. A socket somebody else put
    /// at this path is neither connected to — it would receive the line — nor removed.
    fn ours(path: &Path) -> bool {
        match std::fs::metadata(path) {
            // SAFETY: `getuid` takes nothing and cannot fail.
            Ok(meta) => meta.uid() == unsafe { libc::getuid() },
            Err(_) => true,
        }
    }

    pub async fn forward(path: &Path, line: &str) -> bool {
        if !ours(path) {
            return false;
        }
        match tokio::net::UnixStream::connect(path).await {
            Ok(stream) => super::ask(stream, line).await,
            Err(_) => false,
        }
    }

    pub async fn listening(path: &Path) -> bool {
        // A socket file another user owns is not a copy of this app, and one left behind by a copy
        // that crashed is a file nobody answers on — `connect` says so by failing.
        ours(path) && tokio::net::UnixStream::connect(path).await.is_ok()
    }

    pub async fn serve(path: PathBuf, on_line: impl Fn(String) + Send + Sync + 'static) {
        if !ours(&path) {
            eprintln!(
                "mixlab: not listening for other copies: {} is not this user's",
                path.display()
            );
            return;
        }
        // A file left by a copy that crashed is a file nobody answers on. One that is answered on
        // belongs to a copy that is running — `forward` should have reached it, but a race between
        // two starts can land here, and the answer is the same: this copy does not listen.
        if path.exists() {
            if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                eprintln!("mixlab: another copy is already listening");
                return;
            }
            let _ = std::fs::remove_file(&path);
        }
        let listener = match tokio::net::UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("mixlab: not listening for other copies: {e}");
                return;
            }
        };
        loop {
            match listener.accept().await {
                Ok((stream, _)) => super::answer(stream, &on_line).await,
                Err(e) => eprintln!("mixlab: a copy could not reach this one: {e}"),
            }
        }
    }

    pub fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_endpoint() -> Endpoint {
        Endpoint::named(&format!("mixlab-test-{}", uuid::Uuid::new_v4()))
    }

    /// Keeps trying for a moment: the listener binds on a task of its own and may not be there on
    /// the first attempt.
    async fn forward_soon(endpoint: &Endpoint, line: &str) -> bool {
        for _ in 0..100 {
            if forward(endpoint, line).await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    #[tokio::test]
    async fn a_line_reaches_the_listener_and_is_acknowledged() {
        let endpoint = test_endpoint();
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(serve(endpoint.clone(), move |line| {
            let _ = sent.send(line);
        }));

        assert!(forward_soon(&endpoint, r#"{"url":"mixlab://connect","secret":null}"#).await);
        assert_eq!(
            received.recv().await.as_deref(),
            Some(r#"{"url":"mixlab://connect","secret":null}"#)
        );

        // A second copy, a second line, the same listener.
        assert!(forward(&endpoint, "{}").await);
        assert_eq!(received.recv().await.as_deref(), Some("{}"));

        cleanup(&endpoint);
    }

    #[tokio::test]
    async fn nobody_listening_is_a_quick_no() {
        let endpoint = test_endpoint();
        let started = std::time::Instant::now();
        assert!(!forward(&endpoint, "{}").await);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    /// T106. A relaunched copy has to know when the copy it is replacing has gone, and asking must
    /// not do anything to it on the way: `forward` would bring a dying window to the front and hand
    /// it a start it is in no position to serve.
    #[tokio::test]
    async fn listening_answers_without_saying_anything() {
        let endpoint = test_endpoint();
        assert!(
            !listening(&endpoint).await,
            "nobody is there yet, and the answer is no rather than a wait"
        );

        let taken = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = taken.clone();
        tokio::spawn(serve(endpoint.clone(), move |_| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));

        // The listener binds on a task of its own and may not be there on the first attempt.
        let mut answered = false;
        for _ in 0..100 {
            if listening(&endpoint).await {
                answered = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert!(answered, "a listener is listening");
        assert_eq!(
            taken.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the probe delivered no line, so nothing was brought to the front"
        );

        cleanup(&endpoint);
    }

    /// A line longer than the cap, or one with a line break in it, is not a message and is never
    /// sent; the listener is untouched and takes the next one.
    #[tokio::test]
    async fn what_is_not_a_line_is_never_sent() {
        let endpoint = test_endpoint();
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(serve(endpoint.clone(), move |line| {
            let _ = sent.send(line);
        }));

        let huge = "x".repeat(MAX_LINE + 1);
        assert!(!forward(&endpoint, &huge).await);
        assert!(!forward(&endpoint, "{}\n{}").await);
        assert!(forward_soon(&endpoint, "{}").await);
        assert_eq!(received.recv().await.as_deref(), Some("{}"));

        cleanup(&endpoint);
    }
}
