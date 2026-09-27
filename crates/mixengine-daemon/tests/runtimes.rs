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

/// The daemon process, killed when the test ends however it ends.
struct Daemon(Child);

impl Daemon {
    fn start(home: &Home, registry: &MockRegistry) -> Self {
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
                .stderr(Stdio::null())
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

        Self { sender }
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

        let response = self.sender.send_request(request).await.expect("an answer");
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

/// The whole of T23 in one test, because the whole of T23 is one sequence: a version is offered,
/// installed, listed, made the default and removed — and the file on disk agrees at every step.
#[tokio::test]
async fn a_version_the_index_offers_is_installed_listed_chosen_and_removed() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    // Offered, and not yet here.
    let available = client.call("runtime.list_available", json!({})).await;
    let offered = &available["runtimes"];
    assert_eq!(
        offered.as_array().map(Vec::len),
        Some(1),
        "the version published only for another system is not offered here: {available}"
    );
    assert_eq!(offered[0]["version"], VERSION);
    assert_eq!(offered[0]["installed"], false);
    assert_eq!(offered[0]["eol"], "2027-12-31");
    assert_eq!(offered[0]["bytes"], fixture.packed.size());
    assert_eq!(
        available["stale"], false,
        "the registry answered, so nothing came out of a cache"
    );

    assert_eq!(
        client.call("runtime.list_installed", json!({})).await["runtimes"],
        json!([]),
        "a home that has installed nothing lists nothing rather than failing"
    );

    // Installed.
    let installed = client.install(VERSION).await;
    assert_eq!(installed["state"], "succeeded", "{installed}");

    let runtime = &installed["outcome"]["result"];
    assert_eq!(runtime["kind"], "php");
    assert_eq!(runtime["version"], VERSION);
    assert_eq!(runtime["channel"], "stable");
    assert_eq!(
        runtime["default"], true,
        "the first version of a kind becomes its default, or `php` resolves to nothing"
    );

    let on_disk = fixture.installed_at(VERSION);
    assert!(
        on_disk.join(program_name()).is_file(),
        "the archive was unpacked into {}",
        on_disk.display()
    );
    assert_eq!(
        runtime["path"],
        on_disk.display().to_string(),
        "the row names the directory that was actually renamed into place"
    );

    // Listed, and no longer offered as something to install.
    let list = client.call("runtime.list_installed", json!({})).await;
    assert_eq!(list["runtimes"][0]["version"], VERSION);
    assert_eq!(list["runtimes"][0]["default"], true);

    let available = client.call("runtime.list_available", json!({})).await;
    assert_eq!(
        available["runtimes"][0]["installed"], true,
        "which of the two lists a version is on is composed by the daemon, not by a client"
    );

    // A second install of the same version is refused rather than overwriting it.
    let refused = client
        .refuse(
            "runtime.install",
            json!({"kind": "php", "version": VERSION}),
        )
        .await;
    assert_eq!(refused["data"]["code"], "already_exists", "{refused}");

    // Made the default again, which changes nothing and is not an error.
    let chosen = client
        .call(
            "runtime.set_default",
            json!({"kind": "php", "version": VERSION}),
        )
        .await;
    assert_eq!(chosen["default"], true);

    // Removed, directory and row together.
    let removal = client
        .call(
            "runtime.uninstall",
            json!({"kind": "php", "version": VERSION}),
        )
        .await;
    assert_eq!(removal["removed"]["version"], VERSION);
    assert_eq!(
        removal["default_cleared"], true,
        "it was the default, and nothing is promoted in its place"
    );
    assert!(
        !on_disk.exists(),
        "{} is still there after an uninstall",
        on_disk.display()
    );
    assert_eq!(
        client.call("runtime.list_installed", json!({})).await["runtimes"],
        json!([])
    );
}

/// The job is the whole reason an install is not answered inline, so what it reports on the way is
/// part of the contract rather than decoration.
#[tokio::test]
async fn an_install_reports_where_it_has_got_to_and_is_listed_as_a_job() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let installed = client.install(VERSION).await;

    assert_eq!(
        installed["kind"], "runtime.install",
        "a job's kind is the method that produced it"
    );
    assert_eq!(installed["percent"], 100);
    assert_eq!(installed["outcome"]["ending"], "succeeded");

    let jobs = client.call("job.list", json!({})).await;
    assert_eq!(jobs["jobs"][0]["id"], installed["id"]);
    assert_eq!(jobs["jobs"][0]["kind"], "runtime.install");
}

/// **T149.** A version this machine lacks something for is marked in the listing, answered by
/// `runtime.requirements`, and refused before a job exists — naming the release that does run.
#[tokio::test]
async fn a_version_this_machine_cannot_run_is_refused_before_anything_downloads() {
    if cfg!(windows) {
        return;
    }

    let fixture = Fixture::start_with(index_with_one_that_cannot_run).await;
    let mut client = fixture.client().await;

    let listed = client
        .call("runtime.list_available", json!({ "kind": "php" }))
        .await;
    let rows = listed["runtimes"].as_array().expect("a list");
    let newer = rows
        .iter()
        .find(|row| row["version"] == "9.8.0")
        .expect("9.8.0 is offered");
    let runnable = rows
        .iter()
        .find(|row| row["version"] == VERSION)
        .expect("VERSION is offered");
    assert_eq!(newer["needs"].as_array().map(Vec::len), Some(1), "{newer}");
    assert_eq!(runnable["needs"], json!([]), "{runnable}");

    let asked = client
        .call(
            "runtime.requirements",
            json!({ "kind": "php", "version": "9.8.0" }),
        )
        .await;
    assert_eq!(
        asked["unmet"][0]["remedy"]["remedy"], "choose_version",
        "{asked}"
    );
    assert_eq!(asked["unmet"][0]["remedy"]["version"], VERSION, "{asked}");

    let refused = client
        .refuse(
            "runtime.install",
            json!({ "kind": "php", "version": "9.8.0" }),
        )
        .await;
    assert_eq!(refused["data"]["code"], "dependency_missing", "{refused}");
    assert!(
        refused["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains(VERSION)),
        "the hint names the release that runs: {refused}"
    );

    let jobs = client.call("job.list", json!({})).await;
    assert_eq!(jobs["jobs"], json!([]), "no job was started: {jobs}");
    assert!(!fixture.installed_at("9.8.0").exists());

    let forced = client
        .call(
            "runtime.install",
            json!({ "kind": "php", "version": "9.8.0", "ignore_requirements": true }),
        )
        .await;
    assert_eq!(
        forced["kind"], "runtime.install",
        "ignoring the judgement starts the job: {forced}"
    );
}

/// Three disappointments the index client answers `None` to alike, told apart — because they send
/// whoever reads the message to three different places.
#[tokio::test]
async fn a_version_the_index_does_not_publish_for_this_machine_says_which_of_the_two_it_is() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    // Published, but not for this machine: a fact about upstream rather than a bug here.
    let elsewhere = client
        .call(
            "runtime.install",
            json!({"kind": "php", "version": "9.9.9"}),
        )
        .await;
    let ended = client.finished(elsewhere["id"].clone()).await;
    assert_eq!(ended["state"], "failed", "{ended}");
    assert_eq!(
        ended["outcome"]["error"]["code"], "unsupported_platform",
        "{ended}"
    );

    // Not published at all.
    let nowhere = client
        .call(
            "runtime.install",
            json!({"kind": "php", "version": "1.2.3"}),
        )
        .await;
    let ended = client.finished(nowhere["id"].clone()).await;
    assert_eq!(ended["outcome"]["error"]["code"], "not_found", "{ended}");

    assert!(
        !fixture.installed_at("9.9.9").exists() && !fixture.installed_at("1.2.3").exists(),
        "nothing was created for a version that was never downloaded"
    );
}

/// A version that cannot be a directory name is refused by the wire type, before any of this is
/// reached — which is what makes `runtimes/<kind>/<version>` a join rather than an escaping problem.
#[tokio::test]
async fn a_version_that_could_leave_its_own_directory_is_refused_by_the_parameters() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    for version in ["../../escape", "..", ""] {
        let refused = client
            .refuse(
                "runtime.install",
                json!({"kind": "php", "version": version}),
            )
            .await;

        assert_eq!(
            refused["data"]["code"], "invalid_argument",
            "{version:?}: {refused}"
        );
        assert_eq!(
            refused["code"], -32602,
            "refused as parameters rather than by the method: {refused}"
        );
    }
}

/// Removing something that is not there names it rather than failing obscurely, and the hint sends
/// somebody to the command that would have listed it.
#[tokio::test]
async fn uninstalling_something_that_was_never_installed_says_what_was_looked_for() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let refused = client
        .refuse(
            "runtime.uninstall",
            json!({"kind": "php", "version": VERSION}),
        )
        .await;

    assert_eq!(refused["data"]["code"], "not_found", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("php 8.3.33")),
        "{refused}"
    );
    assert!(
        refused["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("mix runtime list")),
        "{refused}"
    );
}

/// **T24 over the wire**, and the whole of the order it settles: the same directory answers
/// differently as each source appears above the last, and every answer says which one decided it.
///
/// Only the manifest and the default are exercised end to end here — a project record needs
/// `project.create`, which is Phase 4's, and `core::resolve`'s own tests write that row by hand.
#[tokio::test]
async fn which_version_a_directory_uses_is_answered_by_the_source_that_decided_it() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    // Nothing installed at all: the one question this method cannot answer, and the code the
    // feature spec names for it.
    let refused = client
        .refuse("runtime.resolve", json!({"kind": "php"}))
        .await;
    assert_eq!(refused["data"]["code"], "dependency_missing", "{refused}");

    client.install(VERSION).await;

    // 4 — the default, which the first install became.
    let resolved = client.call("runtime.resolve", json!({"kind": "php"})).await;
    assert_eq!(resolved["runtime"]["version"], VERSION);
    assert_eq!(resolved["source"]["from"], "default", "{resolved}");
    assert!(
        resolved.get("constraint").is_none(),
        "a default names no constraint: {resolved}"
    );

    // 2 — a `mixengine.toml` above the directory the question is asked from.
    let project = tempfile::tempdir().expect("a temporary directory");
    let public = project.path().join("public");
    std::fs::create_dir(&public).expect("a directory");
    std::fs::write(
        project.path().join("mixengine.toml"),
        "[project]\nname = \"blog\"\n\n[runtimes]\nphp = \"^8.3\"\n",
    )
    .expect("a manifest");

    let resolved = client
        .call(
            "runtime.resolve",
            json!({"kind": "php", "cwd": public.display().to_string()}),
        )
        .await;
    assert_eq!(resolved["runtime"]["version"], VERSION);
    assert_eq!(resolved["source"]["from"], "manifest", "{resolved}");
    assert!(
        resolved["source"]["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("mixengine.toml")),
        "the file that decided it is named, because that is what somebody would go and edit: \
         {resolved}"
    );
    assert_eq!(resolved["constraint"], "^8.3");

    // 1 — what the caller was told, which beats the file.
    let resolved = client
        .call(
            "runtime.resolve",
            json!({"kind": "php", "cwd": public.display().to_string(), "version": VERSION}),
        )
        .await;
    assert_eq!(resolved["source"]["from"], "explicit", "{resolved}");
}

/// A pin nothing installed satisfies is the failure people will actually meet — after a `git clone`
/// of a repository that asks for a version this machine has never had — so what it *says* is the
/// feature.
#[tokio::test]
async fn a_pin_this_machine_cannot_satisfy_says_what_to_install() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.install(VERSION).await;

    let project = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(
        project.path().join("mixengine.toml"),
        "[runtimes]\nphp = \"8.1.30\"\n",
    )
    .expect("a manifest");

    let refused = client
        .refuse(
            "runtime.resolve",
            json!({"kind": "php", "cwd": project.path().display().to_string()}),
        )
        .await;

    assert_eq!(refused["data"]["code"], "dependency_missing", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("8.1.30") && message.contains("mixengine.toml")),
        "the message names both the version and the file that asked for it: {refused}"
    );
    assert!(
        refused["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("mix runtime install php 8.1.30")),
        "an exact pin becomes the exact command: {refused}"
    );

    // A range cannot, because the version that would satisfy it is one nobody has published yet as
    // far as this machine knows.
    std::fs::write(
        project.path().join("mixengine.toml"),
        "[runtimes]\nphp = \"^9.0\"\n",
    )
    .expect("a manifest");

    let refused = client
        .refuse(
            "runtime.resolve",
            json!({"kind": "php", "cwd": project.path().display().to_string()}),
        )
        .await;
    assert!(
        refused["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("mix runtime available")),
        "{refused}"
    );
}

/// Two ways a caller can ask something a daemon cannot answer, told apart from a version that is
/// simply not here: both are the *client's* mistake and neither is `dependency_missing`.
#[tokio::test]
async fn a_question_a_daemon_cannot_make_sense_of_is_refused_as_a_bad_argument() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.install(VERSION).await;

    // A relative directory would be walked from wherever `mixengined` was started.
    let refused = client
        .refuse(
            "runtime.resolve",
            json!({"kind": "php", "cwd": "blog/public"}),
        )
        .await;
    assert_eq!(refused["data"]["code"], "invalid_argument", "{refused}");

    // A manifest that does not parse, which is the user's file rather than their machine.
    let project = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(
        project.path().join("mixengine.toml"),
        "[runtimes]\nphhp = \"8.3\"\n",
    )
    .expect("a manifest");

    let refused = client
        .refuse(
            "runtime.resolve",
            json!({"kind": "php", "cwd": project.path().display().to_string()}),
        )
        .await;
    assert_eq!(refused["data"]["code"], "invalid_argument", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("mixengine.toml")),
        "{refused}"
    );

    // And a constraint that is not one is refused by the parameters, before any of this is reached.
    let refused = client
        .refuse("runtime.resolve", json!({"kind": "php", "version": "~8.3"}))
        .await;
    assert_eq!(refused["code"], -32602, "{refused}");
}
/// An installed PHP arrives with the pool that serves its sites, and nobody asked for one.
///
/// **The post-install hook, seen from the outside** — roadmap task T32.
/// `docs/features/runtime-versions.md` decided this before there was a pool to create: a PHP
/// without one is a language no site can be served by, so `runtime.install` makes the record and
/// `service.create` refuses to. The other half of the pair is here too, because the two are one
/// promise: the runtime cannot be removed while its pool is a row, and removing it takes the row.
#[tokio::test]
async fn an_installed_php_arrives_with_its_pool_and_leaves_without_it() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.install(VERSION).await;

    let pool = format!("php-fpm@{VERSION}");

    let listed = client.call("service.list", json!({})).await;
    assert!(
        listed["services"]
            .as_array()
            .is_some_and(|services| services.iter().any(|service| service["id"] == pool)),
        "the install created no pool: {listed}"
    );

    // And `service.create` will not write a second one by hand: the row it would need points at a
    // `runtime_installs` row this call has no way to name.
    let refused = client
        .refuse("service.create", json!({ "id": pool, "version": VERSION }))
        .await;
    assert_eq!(refused["data"]["code"], "invalid_argument", "{refused}");
    assert!(
        refused["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("mix runtime install php")),
        "the refusal names the command that does work: {refused}"
    );

    // Removing the runtime removes the pool with it — the row before the directory, so that a
    // `services` row never outlives the install it points at.
    client
        .call(
            "runtime.uninstall",
            json!({"kind": "php", "version": VERSION}),
        )
        .await;

    let listed = client.call("service.list", json!({})).await;
    assert!(
        listed["services"]
            .as_array()
            .is_some_and(|services| services.iter().all(|service| service["id"] != pool)),
        "the pool outlived the PHP it ran out of: {listed}"
    );
}

/// The two methods, against a runtime the fixture recorded rather than downloaded.
///
/// A row and not an install for the reason [`declare::runtime_with_extensions`] gives: what is being
/// proved here is the state model and the wire shape. That the generated files a real PHP then loads
/// say what this answers is `crates/mixengine-cli/tests/php_extensions.rs`.
#[tokio::test(flavor = "multi_thread")]
async fn extensions_are_listed_with_a_reason_and_turned_round_one_at_a_time() {
    let fixture = Fixture::start().await;
    declare::runtime_with_extensions(&fixture.home.database_file(), "8.4.1").await;
    let mut client = fixture.client().await;

    let target = json!({"kind": "php", "version": "8.4.1"});
    let listed = client.call("runtime.list_extensions", target.clone()).await;
    let of = |name: &str| {
        listed["extensions"]
            .as_array()
            .expect("a list")
            .iter()
            .find(|extension| extension["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} is missing from {listed}"))
    };

    assert_eq!(of("opcache")["linkage"], "static");
    assert_eq!(of("xdebug")["enabled"], false);
    assert_eq!(
        of("xdebug")["source"],
        "build_default",
        "off because this build says so, and nobody has said otherwise yet"
    );

    let changed = client
        .call(
            "runtime.set_extension",
            json!({"kind": "php", "version": "8.4.1", "name": "xdebug", "enabled": true}),
        )
        .await;

    assert_eq!(changed["extension"]["enabled"], true);
    assert_eq!(changed["extension"]["source"], "user");
    assert_eq!(
        changed["pool"], "pool_not_running",
        "nothing was started, so nothing was reloaded and nothing has to be restarted"
    );

    let refused = client
        .refuse(
            "runtime.set_extension",
            json!({"kind": "php", "version": "8.4.1", "name": "opcache", "enabled": false}),
        )
        .await;

    // The domain code, under `data`, where every other refusal in this suite reads it.
    assert_eq!(refused["data"]["code"], "unsupported_platform", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|said| said.contains("compiled into")),
        "a refusal has to say that a different build is what it would take: {refused}"
    );
}

/// **The other half of the promise [runtime-versions.md] made.** T32 delivered the running-pool
/// refusal and left this one written down in a doc comment; this is the test that comment named.
///
/// [runtime-versions.md]: ../../../docs/features/runtime-versions.md
#[tokio::test]
async fn a_runtime_a_project_pins_is_not_removed_by_accident() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.install(VERSION).await;

    let repository = tempfile::tempdir().expect("a temporary directory");
    client
        .call(
            "project.create",
            json!({
                "root": repository.path().display().to_string(),
                "name": "blog",
                "pins": {"php": VERSION},
            }),
        )
        .await;

    let refused = client
        .refuse(
            "runtime.uninstall",
            json!({"kind": "php", "version": VERSION}),
        )
        .await;

    assert_eq!(refused["data"]["code"], "precondition_failed", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|said| said.contains("blog")),
        "the refusal names the project, because that is what a person has to go and change: \
         {refused}"
    );
    assert!(
        refused["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("--force")),
        "{refused}"
    );

    // Still installed: a refusal that removed the directory anyway would be the worst of both.
    let listed = client
        .call("runtime.list_installed", json!({"kind": "php"}))
        .await;
    assert_eq!(listed["runtimes"][0]["version"], VERSION, "{listed}");

    // And the flag is what a person types once they have been shown what they are breaking.
    let removed = client
        .call(
            "runtime.uninstall",
            json!({"kind": "php", "version": VERSION, "force": true}),
        )
        .await;
    assert_eq!(removed["removed"]["version"], VERSION, "{removed}");

    // The project is untouched: what breaks is the next resolution, which says what to install.
    let shown = client
        .call("project.show", json!({"project": {"name": "blog"}}))
        .await;
    assert!(shown["pins"][0]["resolved"].is_null(), "{shown}");
    assert!(
        shown["pins"][0]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("runtime install php")),
        "{shown}"
    );
}

/// **D8's asymmetry, which the schema does not enforce for free.** A stopped pool is deleted with
/// the runtime; a *running* one refuses, and `--force` does not buy a live process with no files.
///
/// **The pool is recorded as running rather than started**, which is `tests/lifecycle.rs`' own
/// device and here it is the only one available: the PHP this fixture installs is `fakeservice`,
/// which refuses php-fpm's `--nodaemonize` and crash-loops into `failed` — a state the uninstall is
/// right to remove. What the refusal reads is the `services` row's state, so a row saying `running`
/// against this process's own pid is exactly the state a real pool would present.
#[tokio::test]
async fn a_running_pool_refuses_an_uninstall_even_when_it_is_forced() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.install(VERSION).await;

    let pool = format!("php-fpm@{VERSION}");
    let pid = std::process::id();
    let began = mixengine_platform::process::started_at(pid)
        .expect("this system can be asked when a process began")
        .expect("this process is running")
        .stored();

    declare::running(&fixture.home.database_file(), &pool, pid, began).await;

    // Read back through the daemon before the refusal is asked for. The check under test reads this
    // row and nothing else, so a row that did not land would fail the assertion below for a reason
    // that has nothing to do with the flag — and this says which of the two happened.
    let seen = client
        .call("service.status", json!({ "service": pool }))
        .await;
    assert_eq!(seen["state"], "running", "{seen}");

    let refused = client
        .refuse(
            "runtime.uninstall",
            json!({"kind": "php", "version": VERSION, "force": true}),
        )
        .await;

    assert_eq!(refused["data"]["code"], "precondition_failed", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|said| said.contains(&pool)),
        "a flag that crossed this would buy a live process with no files under it: {refused}"
    );

    // And it is still installed, which is the half a refusal that removed anyway would fail.
    let listed = client
        .call("runtime.list_installed", json!({"kind": "php"}))
        .await;
    assert_eq!(listed["runtimes"][0]["version"], VERSION, "{listed}");
}

/// **`refresh` reaches the registry instead of answering from a still-fresh cache.**
///
/// `mixengine_core::index::Client::catalogue` only asks the network again once the cache is older
/// than `FRESH_FOR` — six hours, which no test can wait out. `refresh` is the escape hatch: a person
/// who just watched a new version get published should not have to wait for the old one's six hours
/// to run out, and this is the only way to prove the flag does that rather than nothing.
#[tokio::test]
async fn refresh_bypasses_a_fresh_cache() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    // Fetches and caches the index the fixture published.
    let first = client.call("runtime.list_available", json!({})).await;
    assert_eq!(
        first["runtimes"].as_array().map(Vec::len),
        Some(1),
        "{first}"
    );

    // Republished with the one offered version gone. The cache is still fresh, so an ordinary call
    // keeps answering from it — the behaviour `refresh` exists to bypass, proved here so the test
    // below is proof of the flag and not of a registry that always answers the same way.
    fixture.registry.publish(&json!({
        "schema": 1,
        "generated_at": "2026-08-14T06:55:13Z",
        "packages": [],
    }));

    let cached = client.call("runtime.list_available", json!({})).await;
    assert_eq!(
        cached["runtimes"].as_array().map(Vec::len),
        Some(1),
        "a fresh cache is not asked about again: {cached}"
    );

    let refreshed = client
        .call("runtime.list_available", json!({"refresh": true}))
        .await;
    assert_eq!(
        refreshed["runtimes"].as_array().map(Vec::len),
        Some(0),
        "`refresh` reaches the registry instead of answering from the cache: {refreshed}"
    );
}

/// **T182f, D1.** An install leaves its marker beside what it unpacked, naming what was installed,
/// so a home that loses its database can record the directory again.
#[tokio::test]
async fn an_installed_runtime_carries_its_marker() {
    let fixture = Fixture::start().await;
    let installed = fixture.client().await.install(VERSION).await;
    assert_eq!(installed["state"], "succeeded", "{installed}");

    let marker: Value = serde_json::from_slice(
        &std::fs::read(
            fixture
                .installed_at(VERSION)
                .join(".mixengine-install.json"),
        )
        .expect("the marker is in the install directory"),
    )
    .expect("the marker is JSON");

    assert_eq!(marker["what"], "runtime", "{marker}");
    assert_eq!(marker["kind"], "php", "{marker}");
    assert_eq!(marker["version"], VERSION, "{marker}");
    assert_eq!(marker["sha256"], fixture.packed.sha256, "{marker}");
}

/// **T182f, the machine it was reported from.** A PHP installed by one home is on disk when the
/// next home starts without its database: it is listed as installed and as the default.
#[tokio::test]
async fn a_runtime_the_last_home_installed_is_recorded_by_the_next_one() {
    let mut fixture = Fixture::start().await;
    let installed = fixture.client().await.install(VERSION).await;
    assert_eq!(installed["state"], "succeeded", "{installed}");

    fixture.reinstall().await;
    let mut client = fixture.client().await;

    let list = client.call("runtime.list_installed", json!({})).await;
    assert_eq!(list["runtimes"][0]["version"], VERSION, "{list}");
    assert_eq!(list["runtimes"][0]["default"], true, "{list}");
}

/// **And one installed before markers existed**: recorded from the index once its files are there
/// and it runs, in the background so the start is not held up by the network.
#[tokio::test]
async fn a_runtime_from_before_markers_is_recorded_from_the_index() {
    let mut fixture = Fixture::start().await;
    fixture.client().await.install(VERSION).await;
    std::fs::remove_file(
        fixture
            .installed_at(VERSION)
            .join(".mixengine-install.json"),
    )
    .expect("the marker");

    fixture.reinstall().await;
    let mut client = fixture.client().await;

    // Polled, because the pass runs in the background and there is no job to wait on; bounded by
    // PATIENCE so a pass that never records fails with the listing it last saw.
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let list = client.call("runtime.list_installed", json!({})).await;
        if list["runtimes"][0]["version"] == VERSION {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never recorded: {list}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        fixture
            .installed_at(VERSION)
            .join(".mixengine-install.json")
            .is_file(),
        "and it was given its marker, so the next start does not ask the index"
    );
}

/// A directory that fails the check is left, named by `mix doctor`, and nothing is deleted.
#[tokio::test]
async fn a_directory_that_cannot_be_recorded_is_named_by_doctor_and_kept() {
    let mut fixture = Fixture::start().await;
    let stray = fixture.installed_at("9.9.8");
    std::fs::create_dir_all(&stray).expect("a directory nothing installed");
    fixture.reinstall().await;

    let mut client = fixture.client().await;
    // Polled for the reason the test above gives.
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let report = client.call("daemon.doctor", json!({})).await;
        let text = report.to_string();
        if text.contains("9.9.8") {
            assert!(text.contains("not recorded"), "{report}");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "doctor never named it: {report}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(stray.is_dir(), "nothing was deleted");
}

/// **T182f, by hand.** A directory the start walk left — here because its marker names another
/// version — is recorded by `runtime.adopt` once the index vouches for it, and its marker is put
/// right. A version already recorded answers its row, and one not on disk is refused by name.
#[tokio::test]
async fn adopting_one_version_checks_it_against_the_index() {
    let mut fixture = Fixture::start().await;
    fixture.client().await.install(VERSION).await;
    let marker = fixture
        .installed_at(VERSION)
        .join(".mixengine-install.json");
    let written = std::fs::read_to_string(&marker).expect("the marker");
    std::fs::write(&marker, written.replace(VERSION, "8.3.32")).expect("a wrong marker");

    fixture.reinstall().await;
    let mut client = fixture.client().await;
    assert_eq!(
        client.call("runtime.list_installed", json!({})).await["runtimes"],
        json!([]),
        "a marker naming another version is not recorded on its own"
    );

    let adopted = client
        .call("runtime.adopt", json!({"kind": "php", "version": VERSION}))
        .await;
    assert_eq!(adopted["version"], VERSION, "{adopted}");
    assert_eq!(adopted["default"], true, "{adopted}");
    assert!(
        std::fs::read_to_string(&marker)
            .expect("the marker")
            .contains(VERSION),
        "the marker now names what is there"
    );

    let again = client
        .call("runtime.adopt", json!({"kind": "php", "version": VERSION}))
        .await;
    assert_eq!(
        again["version"], VERSION,
        "adopting it twice answers its row: {again}"
    );

    let refused = client
        .refuse("runtime.adopt", json!({"kind": "php", "version": "9.9.7"}))
        .await;
    assert_eq!(refused["data"]["code"], "not_found", "{refused}");
}

/// **T182i.** `runtime.found` lists a directory the start left, with why — what the window draws
/// beside **Adopt** — and after the adopt it is gone.
#[tokio::test]
async fn a_runtime_the_start_left_is_listed_with_why_until_it_is_adopted() {
    let mut fixture = Fixture::start().await;
    fixture.client().await.install(VERSION).await;
    let marker = fixture
        .installed_at(VERSION)
        .join(".mixengine-install.json");
    let written = std::fs::read_to_string(&marker).expect("the marker");
    std::fs::write(&marker, written.replace(VERSION, "8.3.32")).expect("a wrong marker");

    fixture.reinstall().await;
    let mut client = fixture.client().await;

    let found = client.call("runtime.found", json!({})).await;
    let row = &found["found"][0];
    assert_eq!(row["kind"], "php", "{found}");
    assert_eq!(row["version"], VERSION, "{found}");
    assert!(
        row["why"]
            .as_str()
            .is_some_and(|why| why.contains("another install")),
        "{found}"
    );

    client
        .call("runtime.adopt", json!({"kind": "php", "version": VERSION}))
        .await;
    let found = client.call("runtime.found", json!({})).await;
    assert_eq!(
        found["found"],
        json!([]),
        "an adopted version is no longer listed: {found}"
    );
}

/// **T182h.** A copy of the earlier home's state in a kept folder is offered by the next home and
/// restored: the project comes back, the pool the start already made for the recorded PHP is kept
/// rather than duplicated, and the copy is not offered twice.
#[tokio::test]
async fn a_copy_of_the_last_home_is_offered_and_restored() {
    let mut fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.install(VERSION).await;
    let root = fixture.home.path().join("work").join("shop");
    std::fs::create_dir_all(&root).expect("a project folder");
    client
        .call(
            "project.create",
            json!({"name": "shop", "root": root.display().to_string()}),
        )
        .await;
    drop(client);

    // What the uninstaller does when it keeps a folder: a copy of the state, written into it.
    let data = fixture.home.path().join("data");
    std::fs::create_dir_all(&data).expect("the data folder");
    fixture.daemon.take();
    let store = mixengine_core::Store::open(&fixture.home.path().join("mixengine.db"))
        .await
        .expect("the old home's database");
    mixengine_core::adopt::snapshot::write(&store, &data)
        .await
        .expect("a copy");
    store.close().await;

    fixture.reinstall().await;
    let mut client = fixture.client().await;

    let previous = client.call("home.previous", json!({})).await;
    assert_eq!(previous["copy"]["projects"], 1, "{previous}");

    let restored = client.call("home.restore", json!({})).await;
    assert_eq!(restored["projects"], 1, "{restored}");

    let projects = client.call("project.list", json!({})).await;
    assert!(projects.to_string().contains("shop"), "{projects}");

    let again = client.call("home.previous", json!({})).await;
    assert!(
        again["copy"].is_null(),
        "a restored copy is not offered again: {again}"
    );
}
