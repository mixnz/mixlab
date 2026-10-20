//! One connection to a home's daemon, for a suite that drives it over the API.
//!
//! Distinct from [`crate::call`], which opens a connection per call for a suite with no client of
//! its own. This one is kept, and carries as many requests as the test sends.

use std::path::{Path, PathBuf};

use http_body_util::{BodyExt as _, Full};
use hyper::body::{Bytes, Incoming};
use hyper::client::conn::http1::SendRequest;
use hyper::header::{CONTENT_TYPE, HOST};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use mixengine_platform::ipc::Connection;
use serde_json::{Value, json};

use crate::Home;

/// One HTTP/1.1 connection to the daemon serving a [`Home`].
#[derive(Debug)]
pub struct Client {
    sender: SendRequest<Full<Bytes>>,
    /// `logs/daemon.log`, for what a request with no answer prints.
    daemon_log: PathBuf,
    /// [`Home::daemon_stderr_file`], printed too when a suite has pointed the daemon's stderr at it.
    daemon_stderr: PathBuf,
}

impl Client {
    /// Connect to the daemon serving `home`.
    ///
    /// # Panics
    ///
    /// If nothing is listening, or what is does not speak HTTP/1.1.
    pub async fn connect(home: &Home) -> Self {
        let connection = Connection::connect(home.endpoint())
            .await
            .expect("the daemon is listening");

        let (sender, driver) = hyper::client::conn::http1::handshake(TokioIo::new(connection))
            .await
            .expect("the daemon speaks HTTP/1.1");

        tokio::spawn(driver);

        Self {
            sender,
            daemon_log: home.daemon_log_file(),
            daemon_stderr: home.daemon_stderr_file(),
        }
    }

    /// Send `request` over this connection and hand back the answer, its status unchecked and its
    /// body unread — for a stream read as it arrives.
    ///
    /// # Panics
    ///
    /// If the connection is closed, or no answer arrives; the daemon's log is attached, and its
    /// stderr when a file holds it.
    pub async fn send(&mut self, request: Request<Full<Bytes>>) -> Response<Incoming> {
        let uri = request.uri().clone();

        // Waited for rather than assumed: `hyper`'s dispatcher allows one request through before it
        // has said it wants one, and every request after that only once the connection task has been
        // polled since the last response. A connection that carries a second request and sends it
        // straight away races that task and fails with `canceled: connection was not ready` —
        // rarely, and more often on the machine with the least to spare, which is CI.
        self.sender
            .ready()
            .await
            .expect("the connection is still open");

        match self.sender.send_request(request).await {
            Ok(response) => response,
            Err(error) => panic!("{uri}: no answer: {error:?}\n{}", self.said()),
        }
    }

    /// Call `method` and hand back the whole JSON-RPC response, `result` or `error`.
    ///
    /// # Panics
    ///
    /// As [`Client::send`], and if the answer is not `200 OK` with a JSON body.
    pub async fn ask(&mut self, method: &str, params: Value) -> Value {
        let body = json!({ "jsonrpc": "2.0", "method": method, "params": params, "id": 1 });

        let request = Request::builder()
            .method(Method::POST)
            .uri("/rpc")
            .header(HOST, "mixengine")
            .header(CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(
                serde_json::to_vec(&body).expect("a request serialises"),
            )))
            .expect("a well formed request");

        let response = self.send(request).await;
        assert_eq!(response.status(), StatusCode::OK, "{method}");

        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a whole body")
            .to_bytes();

        serde_json::from_slice(&bytes).expect("a JSON-RPC response")
    }

    /// Call `method` and hand back its `result`, insisting it succeeded.
    ///
    /// # Panics
    ///
    /// As [`Client::ask`], and if the answer is an `error`.
    pub async fn call(&mut self, method: &str, params: Value) -> Value {
        let answer = self.ask(method, params).await;
        assert!(answer.get("error").is_none(), "{method}: {answer}");

        answer["result"].clone()
    }

    /// Call `method` and hand back its `error`, insisting it did not succeed.
    ///
    /// # Panics
    ///
    /// As [`Client::ask`], and if the answer is a `result`.
    pub async fn refuse(&mut self, method: &str, params: Value) -> Value {
        let answer = self.ask(method, params).await;
        assert!(answer.get("result").is_none(), "{method}: {answer}");

        answer["error"].clone()
    }

    /// What the daemon said, for a request that got no answer: anything it logged on the way there
    /// is in its log, and a panic is in its stderr when a suite kept it.
    fn said(&self) -> String {
        let read = |path: &Path| {
            std::fs::read_to_string(path).unwrap_or_else(|error| format!("(unreadable: {error})"))
        };

        let mut said = format!("--- daemon.log ---\n{}", read(&self.daemon_log));
        if self.daemon_stderr.exists() {
            said.push_str(&format!(
                "\n--- the daemon's stderr ---\n{}",
                read(&self.daemon_stderr)
            ));
        }
        said
    }
}
