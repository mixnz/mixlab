//! `runtime.*` against a real `mixengined`, a real signed index and a real archive.
//!
//! Roadmap task **T23**, and the half no unit test can reach. That a table can be read, that an
//! archive can be unpacked and that a job can be run are each provable in one process; that asking a
//! daemon over a socket for a version it has never heard of *ends with PHP on disk, a row describing
//! it, and a job that says so* is the seam, and the seam is the feature.
//!
//! **Nothing here touches the network.** [`MockRegistry`] serves a document it signs with a keypair
//! it generated, over a loopback socket, and the daemon is pointed at both with `--index-url` and
//! `--index-key` — which is also the only end-to-end proof that those two flags do what they say.
//! What is installed is a [`FakePackage`] containing the `fakeservice` binary under the name the
//! index publishes it as, so the post-install check spawns something that really runs.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use http_body_util::{BodyExt as _, Full};
use hyper::body::Bytes;
use hyper::header::{CONTENT_TYPE, HOST};
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use mixengine_platform::ipc::Connection;
use mixengine_testkit::{FakePackage, Home, MockRegistry, Packed, Packing, declare};
use serde_json::{Value, json};

mod install;
mod listing;
mod upgrade;

/// How long a job is given to finish before the test gives up on it.
///
/// Every install here is a few kilobytes over loopback, so this is a ceiling and not a pause — but
/// the post-install check spawns a freshly written executable, and a CI runner scanning one for the
/// first time is the slow part rather than the download.
const PATIENCE: Duration = Duration::from_secs(60);

/// The version this suite installs, and one it does not.
const VERSION: &str = "8.3.33";

/// The name the archive publishes its executable under, with the suffix this OS needs to spawn it.
///
/// Windows resolves an extensionless name by appending `.exe`, and a fixture that relied on that
/// would be testing the loader rather than the install.
fn program_name() -> String {
    format!("bin/php{}", std::env::consts::EXE_SUFFIX)
}

/// A registry serving one PHP, a home, and a daemon pointed at both.
struct Fixture {
    home: Home,
    /// Held rather than read: dropping it would stop the server the daemon downloads from.
    registry: MockRegistry,
    /// An `Option` so [`reinstall`](Self::reinstall) can stop it and start another.
    daemon: Option<Daemon>,
    packed: Packed,
}

impl Fixture {
    /// Publish one version of PHP and start a daemon that can see it.
    async fn start() -> Self {
        Self::start_with(index).await
    }

    /// Publish what `index_for` says and start a daemon that can see it.
    async fn start_with(index_for: fn(&Packed, &str) -> Value) -> Self {
        // `.zip` on Windows and `.tar.zst` elsewhere, which is what the publishing pipeline
        // produces for each — the point being that the daemon unpacks what its own platform is
        // actually served rather than whichever format a fixture found convenient.
        let packing = match cfg!(windows) {
            true => Packing::Zip,
            false => Packing::TarZst,
        };
        let packed = FakePackage::new(packing)
            .executable(&program_name())
            .file("php.ini-production", b"; nothing")
            .build(&format!("php-{VERSION}"));

        let registry = MockRegistry::start(&json!({
            "schema": 1,
            "generated_at": "2026-08-14T06:55:12Z",
            "packages": [],
        }))
        .await;

        let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
        registry.publish(&index_for(&packed, &url));

        let home = Home::new();
        let daemon = Daemon::start(&home, &registry);
        home.wait_until_listening().await;

        Self {
            home,
            registry,
            daemon: Some(daemon),
            packed,
        }
    }

    async fn client(&self) -> Client {
        Client::connect(&self.home).await
    }

    /// Stop this daemon, remove what the uninstaller removes with the home — the database — and
    /// start a fresh daemon on the same directories: the machine T182f was reported from.
    async fn reinstall(&mut self) {
        self.daemon.take();
        for name in ["mixengine.db", "mixengine.db-wal", "mixengine.db-shm"] {
            let _ = std::fs::remove_file(self.home.path().join(name));
        }
        self.daemon = Some(Daemon::start(&self.home, &self.registry));
        self.home.wait_until_listening().await;
    }

    /// Stop this daemon and start another on the same home, database and all.
    async fn restart(&mut self) {
        self.daemon.take();
        self.daemon = Some(Daemon::start(&self.home, &self.registry));
        self.home.wait_until_listening().await;
    }

    /// Record a Node.js in this home the way an install leaves one: its program where `provides`
    /// says, and a row. Written straight into the database, so the daemon hears nothing of it.
    async fn record_node(&self, version: &str) {
        let install = self.node_at(version);
        let program = match cfg!(windows) {
            true => "node.exe",
            false => "bin/node",
        };

        let file = install.join(program);
        std::fs::create_dir_all(file.parent().expect("a directory")).expect("a directory");
        std::fs::copy(mixengine_testkit::package::executable_source(), &file)
            .unwrap_or_else(|error| panic!("copy to {}: {error}", file.display()));

        let store = mixengine_core::Store::open(&self.home.database_file())
            .await
            .expect("the database");

        mixengine_core::runtimes::remember(
            &store,
            &mixengine_core::runtimes::Installation {
                kind: mixengine_proto::RuntimeKind::Node,
                version: mixengine_proto::PackageVersion::parse(version).expect("a version"),
                channel: mixengine_proto::PackageChannel::Stable,
                path: install,
                bytes: 30_000_000,
                url: format!("https://example.invalid/node-{version}.tar.zst"),
                sha256: "00".to_owned(),
                provides: [("node".to_owned(), program.to_owned())]
                    .into_iter()
                    .collect(),
                extension_dir: None,
                extensions: mixengine_core::index::Extensions::default(),
            },
            mixengine_proto::Timestamp(1_760_000_000_000),
        )
        .await
        .expect("a row");

        store.close().await;
    }

    /// What `npm install -g <name>` leaves in this Node's bindir, and nothing else.
    fn install_globally(&self, version: &str, name: &str) {
        let bindir = self.bindir(version);
        let file = bindir.join(match cfg!(windows) {
            true => format!("{name}.cmd"),
            false => name.to_owned(),
        });

        std::fs::create_dir_all(&bindir).expect("a bindir");
        std::fs::copy(mixengine_testkit::package::executable_source(), &file)
            .unwrap_or_else(|error| panic!("copy to {}: {error}", file.display()));
    }

    /// And `npm uninstall -g <name>`.
    fn uninstall_globally(&self, version: &str, name: &str) {
        let file = self.bindir(version).join(match cfg!(windows) {
            true => format!("{name}.cmd"),
            false => name.to_owned(),
        });

        std::fs::remove_file(&file)
            .unwrap_or_else(|error| panic!("remove {}: {error}", file.display()));
    }

    fn node_at(&self, version: &str) -> std::path::PathBuf {
        self.home.path().join("runtimes").join("node").join(version)
    }

    fn bindir(&self, version: &str) -> std::path::PathBuf {
        mixengine_core::runtimes::globals::directory(
            mixengine_proto::RuntimeKind::Node,
            &self.node_at(version),
        )
        .expect("Node has a bindir")
    }

    /// `<root>/bin/<name>`, as this system spells a command there.
    fn command(&self, name: &str) -> std::path::PathBuf {
        self.home
            .path()
            .join("bin")
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    }

    /// Wait until `name` is in `bin/` or is not, as `present` says — or fail, saying what `bin/`
    /// held instead and what the daemon logged.
    async fn command_becomes(&self, name: &str, present: bool) {
        let command = self.command(name);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);

        while command.exists() != present {
            if tokio::time::Instant::now() > deadline {
                let held: Vec<String> = std::fs::read_dir(self.home.path().join("bin"))
                    .map(|entries| {
                        entries
                            .flatten()
                            .map(|entry| entry.file_name().to_string_lossy().into_owned())
                            .collect()
                    })
                    .unwrap_or_default();

                panic!(
                    "{name} was {} bin/ fifteen seconds later\n--- bin/ ---\n{held:?}\n--- daemon \
                     ---\n{}",
                    match present {
                        true => "still not in",
                        false => "still in",
                    },
                    self.home.daemon_log()
                );
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Where the daemon would have put this version.
    fn installed_at(&self, version: &str) -> std::path::PathBuf {
        self.home.path().join("runtimes").join("php").join(version)
    }
}

/// An index offering one PHP for this machine, and one for a machine this is not.
///
/// The second entry is what makes `list_available` mean anything: a version published only for
/// another operating system must not be offered here, and a fixture with one artifact could not tell
/// a filter that works from one that does nothing.
fn index(packed: &Packed, url: &str) -> Value {
    let elsewhere = match cfg!(target_os = "linux") {
        true => "macos",
        false => "linux",
    };

    json!({
        "schema": 1,
        "generated_at": "2026-08-14T06:55:12Z",
        "packages": [
            {
                "kind": "php",
                "version": VERSION,
                "channel": "stable",
                "eol": "2027-12-31",
                "artifacts": [{
                    "os": os(),
                    "arch": arch(),
                    "url": url,
                    "sha256": packed.sha256,
                    "size": packed.size(),
                    "provides": { "php": program_name(), sapi(): program_name() },
                }],
            },
            {
                "kind": "php",
                "version": "9.9.9",
                "channel": "stable",
                "artifacts": [{
                    "os": elsewhere,
                    "arch": "x86_64",
                    "url": "https://example.invalid/php-9.9.9.tar.zst",
                    "sha256": "00",
                    "size": 1,
                    "provides": { "php": "bin/php" },
                }],
            },
        ],
    })
}

/// [`index`], plus a newer PHP this machine can never run — roadmap task **T149**.
///
/// A requirement no runner meets: glibc 99 on Linux, macOS 99 on a Mac. Windows has no such
/// requirement to give — every runner has a Visual C++ runtime new enough — so the test that uses
/// this stops there, and the Windows half is the core unit tests and M18.
fn index_with_one_that_cannot_run(packed: &Packed, url: &str) -> Value {
    let mut published = index(packed, url);
    let requires = match os() {
        "linux" => json!({ "glibc": "99.0" }),
        "macos" => json!({ "macos": "99.0" }),
        _ => json!({}),
    };

    published["packages"]
        .as_array_mut()
        .expect("packages is a list")
        .push(json!({
            "kind": "php",
            "version": "9.8.0",
            "channel": "stable",
            "artifacts": [{
                "os": os(),
                "arch": arch(),
                "url": url,
                "sha256": packed.sha256,
                "size": packed.size(),
                "provides": { "php": program_name(), sapi(): program_name() },
                "requires": requires,
            }],
        }));

    published
}

/// The SAPI a real PHP publishes on this system — roadmap task T32.
///
/// **One and not both**, which is the shape of the index rather than a convenience: a Unix PHP has
/// `php-fpm` and a Windows one has `php-cgi`, and publishing both here would give the php-fpm recipe
/// a `php-fpm` on Windows to run `--test` with, against a fixture that is not php-fpm. The pool the
/// install creates has to be able to build a spec, or every `service.*` call after it fails.
fn sapi() -> &'static str {
    if cfg!(windows) { "php-cgi" } else { "php-fpm" }
}

/// What the index calls the system these tests are running on — `std`'s own spelling, on all
/// three of the systems this project ships for.
fn os() -> &'static str {
    std::env::consts::OS
}

/// And its architecture.
fn arch() -> &'static str {
    std::env::consts::ARCH
}

/// Where every daemon this home has run writes its standard error, appended rather than replaced
/// so a [`Fixture::reinstall`] keeps what the daemon before it said.
///
/// **Kept because a panic goes nowhere else.** `daemon.log` is written by the daemon's own logger,
/// which a panic bypasses, and run 37414570632 lost a daemon between two requests with nothing
/// left to read: `ConnectionReset` and two `null`s where the reason should have been.
fn stderr_file(home: &Path) -> PathBuf {
    home.join("daemon.stderr")
}

/// The daemon process, killed when the test ends however it ends.
struct Daemon(Child);

impl Daemon {
    fn start(home: &Home, registry: &MockRegistry) -> Self {
        let stderr = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stderr_file(home.path()))
            .expect("a file for the daemon's standard error");

        Self(
            Command::new(env!("CARGO_BIN_EXE_mixengined"))
                .arg("--home")
                .arg(home.path())
                // Passed as arguments rather than through the environment, per rule 2 in
                // `docs/standards/testing.md`: two of these running at once under `cargo test`
                // would otherwise be pointed at each other's registry.
                .arg("--index-url")
                .arg(registry.url())
                .arg("--index-key")
                .arg(registry.public_key())
                .stdout(Stdio::null())
                .stderr(stderr)
                .spawn()
                .expect("the daemon binary runs"),
        )
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // Killed rather than asked: a test that failed halfway must not leave a process holding the
        // temporary home open, which on Windows would make the directory unremovable.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One connection to the daemon.
struct Client {
    sender: hyper::client::conn::http1::SendRequest<Full<Bytes>>,
    /// The home's directory, for what a failed request prints: the daemon's log and its stderr.
    home: PathBuf,
    /// `daemon.log`, read the same way.
    daemon_log: PathBuf,
}

impl Client {
    async fn connect(home: &Home) -> Self {
        let connection = Connection::connect(home.endpoint())
            .await
            .expect("the daemon is listening");

        let (sender, driver) = hyper::client::conn::http1::handshake(TokioIo::new(connection))
            .await
            .expect("the daemon speaks HTTP/1.1");

        tokio::spawn(driver);

        Self {
            sender,
            home: home.path().to_path_buf(),
            daemon_log: home.daemon_log_file(),
        }
    }

    /// What the daemon said, for a request that got no answer: a panic is in its stderr, and
    /// anything it logged on the way there is in its log.
    fn said(&self) -> String {
        let read = |path: &Path| {
            std::fs::read_to_string(path).unwrap_or_else(|error| format!("(unreadable: {error})"))
        };
        format!(
            "--- the daemon's stderr ---\n{}\n--- daemon.log ---\n{}",
            read(&stderr_file(&self.home)),
            read(&self.daemon_log)
        )
    }

    /// Call a method and hand back its `result`, insisting it succeeded.
    async fn call(&mut self, method: &str, params: Value) -> Value {
        let answer = self.ask(method, params).await;
        assert!(answer.get("error").is_none(), "{method}: {answer}");

        answer["result"].clone()
    }

    /// Call a method and hand back its `error`, insisting it did not succeed.
    async fn refuse(&mut self, method: &str, params: Value) -> Value {
        let answer = self.ask(method, params).await;
        assert!(answer.get("result").is_none(), "{method}: {answer}");

        answer["error"].clone()
    }

    async fn ask(&mut self, method: &str, params: Value) -> Value {
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

        // Waited for rather than assumed: `hyper`'s dispatcher allows one request through before
        // it has said it wants one, and every request after that only once the connection task has
        // been polled since the last response. This suite reuses a connection, so sending straight
        // away raced that task and failed with `canceled: connection was not ready` — rarely, and
        // more often on the machine with the least to spare, which is CI.
        self.sender
            .ready()
            .await
            .expect("the connection is still open");

        let response = match self.sender.send_request(request).await {
            Ok(response) => response,
            Err(error) => panic!("{method}: no answer: {error:?}\n{}", self.said()),
        };
        assert_eq!(response.status(), StatusCode::OK, "{method}");

        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a whole body")
            .to_bytes();

        serde_json::from_slice(&bytes).expect("a JSON-RPC response")
    }

    /// Install a version and wait for the job it produced to end.
    async fn install(&mut self, version: &str) -> Value {
        let started = self
            .call(
                "runtime.install",
                json!({"kind": "php", "version": version}),
            )
            .await;

        assert_eq!(
            started["state"], "running",
            "an install is answered as accepted, not as finished: {started}"
        );

        self.finished(started["id"].clone()).await
    }

    /// Wait for a job to end, and answer with it as it ended.
    async fn finished(&mut self, job: Value) -> Value {
        let deadline = tokio::time::Instant::now() + PATIENCE;

        loop {
            let waited = self
                .call("job.wait", json!({"job": job, "timeout": 2_000}))
                .await;

            if waited["state"] != "running" {
                return waited;
            }

            assert!(
                tokio::time::Instant::now() < deadline,
                "the install never finished: {waited}"
            );
        }
    }
}
