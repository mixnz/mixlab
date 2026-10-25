//! The API, against a real `mixengined` on a real endpoint.
//!
//! Not a component test with the router called directly: `mixengine-daemon` is a binary crate with
//! no library target, so an integration test cannot reach inside it — and reaching inside is what
//! would be worth avoiding anyway. What is proved here is the part the unit tests next to the code
//! cannot reach: that a daemon started the way a user starts one binds the endpoint its home
//! implies, speaks HTTP over a socket that is not a network socket, and answers each route the way
//! `docs/architecture/daemon-and-ipc.md` says it does.
//!
//! Every test gets its own `MIXENGINE_HOME` in a `TempDir` **passed as `--home`** — rule 2 in
//! `docs/standards/testing.md`: the environment is process-global, and two of these running at
//! once under `cargo test` would rewrite each other's home. Nothing here touches the network; a
//! Unix socket and a named pipe are neither.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use http_body_util::{BodyExt as _, Full};
use hyper::body::Bytes;
use hyper::header::{ALLOW, CONTENT_TYPE};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use mixengine_core::Paths;
use mixengine_core::config::PathOverrides;
use mixengine_platform::ipc::{Connection, Endpoint};
use serde_json::Value;
use tempfile::TempDir;

/// How long a freshly spawned daemon is given to bind its endpoint.
///
/// Generous, because the first start of a daemon creates its home, runs the migrations and opens
/// SQLite — and because a loaded CI runner is the machine this has to be reliable on. It is a
/// ceiling and not a wait: the poll below returns the moment the endpoint answers.
///
/// Sixty rather than thirty since CI run 35996827418 (windows-latest): two daemons of this suite
/// were still writing the blueprint gallery at about one a second when thirty ran out, and their
/// neighbours took 46 s to finish. The daemon log shows it working, not stuck.
const STARTUP: Duration = Duration::from_secs(60);

/// A `mixengined` running against a throwaway home, killed when the test ends.
struct Daemon {
    child: Child,
    home: TempDir,
    endpoint: Endpoint,
}

impl Daemon {
    /// Start one and wait until it answers.
    async fn start() -> Self {
        let home = tempfile::tempdir().expect("a temporary home");

        // `--home` below is handed the directory exactly as the system named it, alias and all,
        // which is what a user hands over. The daemon spells it in full before it derives anything
        // from it, so a fixture that wants to reach the same endpoint has to do the same.
        let root = mixengine_platform::paths::in_full(home.path());
        let paths = Paths::new(root, &PathOverrides::default());
        let endpoint = Endpoint::in_run_dir(paths.run()).expect("an endpoint for this home");

        // No `--detach`: the daemon stays in the foreground, so this `Child` is the daemon itself
        // and killing it at the end of the test kills the thing holding the temporary home.
        let child = Command::new(env!("CARGO_BIN_EXE_mixengined"))
            .arg("--home")
            .arg(home.path())
            // Silenced rather than inherited: a passing test should print nothing, and the daemon's
            // own `logs/daemon.log` inside the home is a better record than interleaved stderr —
            // `wait_until_listening` reads it when a start goes wrong.
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the daemon binary runs");

        let daemon = Self {
            child,
            home,
            endpoint,
        };

        daemon.wait_until_listening().await;
        daemon
    }

    /// Poll the endpoint until something is behind it.
    ///
    /// Dialling rather than watching for the socket file: on Unix the file exists a moment before
    /// `accept` is running, and on Windows there is no file to watch at all.
    async fn wait_until_listening(&self) {
        let deadline = tokio::time::Instant::now() + STARTUP;

        while tokio::time::Instant::now() < deadline {
            if Connection::connect(&self.endpoint).await.is_ok() {
                return;
            }

            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        panic!(
            "the daemon did not start listening on {} within {STARTUP:?}\n--- daemon.log ---\n{}",
            self.endpoint,
            std::fs::read_to_string(self.home.path().join("logs").join("daemon.log"))
                .unwrap_or_else(|error| format!("(unreadable: {error})"))
        );
    }

    /// The home this daemon was started with.
    fn home(&self) -> &Path {
        self.home.path()
    }

    /// Send one request over its own connection.
    ///
    /// A connection per request rather than a pooled client. Keep-alive is worth having in `mix`
    /// and is `hyper`'s to provide; here it would only mean one test's connection state could
    /// affect the next assertion in the same test.
    async fn send(&self, request: Request<Full<Bytes>>) -> Response<hyper::body::Incoming> {
        let connection = Connection::connect(&self.endpoint)
            .await
            .expect("the daemon is listening");

        let (mut sender, driver) = hyper::client::conn::http1::handshake(TokioIo::new(connection))
            .await
            .expect("the daemon speaks HTTP/1.1");

        // The driver owns the socket and must be polled for the request to make progress. It ends
        // on its own when the response is done or the connection closes, so it is not awaited.
        tokio::spawn(driver);

        sender
            .send_request(request)
            .await
            .expect("the daemon answers")
    }

    /// A `POST /rpc` whose body the daemon stops reading, and its answer.
    ///
    /// Not through [`Self::send`]. The daemon answers `413` once the limit is reached and closes the
    /// connection with the rest of the body unread, and a client still writing that rest meets a
    /// broken pipe. `hyper`'s client then reports the failed write and drops the answer it was never
    /// asked to read yet, which is a race this test lost on macOS in CI. Here the request is written on a task of its own, its failure ignored, while the
    /// answer is read as it arrives: what is proved is what the daemon says, not how a client copes
    /// with being cut off.
    async fn send_unread(&self, body: Bytes) -> (StatusCode, Value) {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let connection = Connection::connect(&self.endpoint)
            .await
            .expect("the daemon is listening");
        let (mut reader, mut writer) = tokio::io::split(connection);

        let head = format!(
            "POST /rpc HTTP/1.1\r\nhost: mixengine\r\ncontent-type: application/json\r\n\
             content-length: {}\r\n\r\n",
            body.len()
        );
        let writing = tokio::spawn(async move {
            let _ = writer.write_all(head.as_bytes()).await;
            let _ = writer.write_all(&body).await;
        });

        // Read until the whole answer is in rather than until the connection ends: on Windows a
        // closed pipe is an error to its reader, not an end of file.
        let mut answer = Vec::new();
        let mut chunk = [0_u8; 8192];
        let (end_of_head, length) = loop {
            let read = reader.read(&mut chunk).await.expect("the daemon answers");
            assert_ne!(
                read,
                0,
                "the connection ended mid-answer:\n{}",
                String::from_utf8_lossy(&answer)
            );
            answer.extend_from_slice(&chunk[..read]);

            let Some(end) = answer.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            let head = String::from_utf8_lossy(&answer[..end]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or_else(|| panic!("the answer names its length:\n{head}"));
            if answer.len() >= end + 4 + length {
                break (end, length);
            }
        };
        writing.abort();

        let status_line = String::from_utf8_lossy(&answer[..end_of_head]);
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .and_then(|code| StatusCode::from_u16(code).ok())
            .unwrap_or_else(|| panic!("an HTTP status line: {status_line}"));
        let body = &answer[end_of_head + 4..end_of_head + 4 + length];
        let body = serde_json::from_slice(body).unwrap_or_else(|error| {
            panic!(
                "the daemon answers JSON: {error}\n{}",
                String::from_utf8_lossy(body)
            )
        });
        (status, body)
    }

    /// A `GET`, answered.
    async fn get(&self, path: &str) -> Response<hyper::body::Incoming> {
        self.send(build(Method::GET, path, Bytes::new())).await
    }

    /// A `HEAD`, answered.
    async fn head(&self, path: &str) -> Response<hyper::body::Incoming> {
        self.send(build(Method::HEAD, path, Bytes::new())).await
    }

    /// A JSON-RPC call, answered and decoded. Panics if the daemon answered no body at all.
    async fn rpc(&self, body: &str) -> Value {
        let response = self
            .send(build(Method::POST, "/rpc", Bytes::from(body.to_owned())))
            .await;

        assert_eq!(response.status(), StatusCode::OK, "for {body}");
        json(response).await
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // Killed rather than asked to stop: an interrupt cannot be delivered to a child portably.
        // The endpoint goes with the process on Windows and with the `TempDir` on Unix, so nothing
        // survives either way.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A request with the one header HTTP/1.1 makes mandatory.
///
/// There is no host to name — the endpoint is a socket, not an address — so the value is a constant
/// the daemon never reads. It is sent because a request without it is not a valid HTTP/1.1 request,
/// and a client that omitted it would be relying on the server not to care.
fn build(method: Method, path: &str, body: Bytes) -> Request<Full<Bytes>> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(hyper::header::HOST, "mixengine")
        .header(CONTENT_TYPE, "application/json")
        .body(Full::new(body))
        .expect("a well-formed request")
}

/// The body of a response, as JSON.
async fn json(response: Response<hyper::body::Incoming>) -> Value {
    let body = response
        .into_body()
        .collect()
        .await
        .expect("the whole body arrives")
        .to_bytes();

    serde_json::from_slice(&body).unwrap_or_else(|error| {
        panic!(
            "the daemon answers JSON: {error}\n{}",
            String::from_utf8_lossy(&body)
        )
    })
}

mod home;
mod protocol;
