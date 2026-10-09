//! `package.*` against a real `mixengined`, a real signed index and a real archive.
//!
//! Roadmap task **T31a**, and [`tests/runtimes.rs`](runtimes.rs)' shape one namespace across: a
//! [`MockRegistry`] serves a document it signs over a loopback socket, the daemon is pointed at both
//! with `--index-url` and `--index-key`, and nothing here touches the network.
//!
//! **What is published is `fakeservice`**, because a debug build has a recipe for it and the archive
//! can be a real executable that needs no server behind it. The index also publishes a
//! [`UNRUNNABLE`], which this build has no recipe for — that is what makes the catalogue filter mean
//! something, since a fixture offering only what is runnable could not tell a filter that works from
//! one that does nothing.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use http_body_util::{BodyExt as _, Full};
use hyper::body::Bytes;
use hyper::header::{CONTENT_TYPE, HOST};
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use mixengine_platform::ipc::Connection;
use mixengine_testkit::{FakePackage, Home, MockRegistry, Packed, Packing};
use serde_json::{Value, json};

/// How long a job is given to finish before the test gives up on it.
const PATIENCE: Duration = Duration::from_secs(60);

/// The version this suite installs.
const VERSION: &str = "1.0.0";

/// The package it installs, which is the one a debug build has a recipe for.
const PACKAGE: &str = "fakeservice";

/// A package the index publishes and this build cannot run.
///
/// **A name no index will ever carry, and that is the fix rather than the shortcut.** This was
/// `redis` until T35 gave Redis a recipe and turned all three tests below red — which was the
/// fixture being right about a fact that had an expiry date on it. Every real server the index
/// publishes is one the catalogue is *meant* to grow a recipe for, so naming any of them here is the
/// same clock set for a later day. What the rule is actually about outlives all of them: an index is
/// published on its own schedule and a user's build is routinely older than it, so a package this
/// build has never heard of is the ordinary case and not the contrived one.
const UNRUNNABLE: &str = "futureservice";

/// The version the index publishes that under. Nothing reads it as a version; it is here so the
/// entry looks like every other one.
const UNRUNNABLE_VERSION: &str = "1.0.0";

/// The name the archive publishes its executable under, with the suffix this OS needs to spawn it.
fn program_name() -> String {
    format!("fakeservice{}", std::env::consts::EXE_SUFFIX)
}

/// A registry serving one `fakeservice`, a home, and a daemon pointed at both.
struct Fixture {
    home: Home,
    /// Held rather than read: dropping it would stop the server the daemon downloads from.
    _registry: MockRegistry,
    _daemon: Daemon,
    packed: Packed,
}

impl Fixture {
    /// Publish one version of one package and start a daemon that can see it.
    async fn start() -> Self {
        Self::start_with(index).await
    }

    /// Publish what `index_for` says and start a daemon that can see it.
    async fn start_with(index_for: fn(&Packed, &str) -> Value) -> Self {
        // `.zip` on Windows and `.tar.zst` elsewhere, which is what the publishing pipeline produces
        // for each — the point being that the daemon unpacks what its own platform is served.
        let packing = match cfg!(windows) {
            true => Packing::Zip,
            false => Packing::TarZst,
        };
        let packed = FakePackage::new(packing)
            .executable(&program_name())
            .build(&format!("{PACKAGE}-{VERSION}"));

        let registry = MockRegistry::start(&json!({
            "schema": 1,
            "generated_at": "2026-08-19T06:55:12Z",
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
            _registry: registry,
            _daemon: daemon,
            packed,
        }
    }

    async fn client(&self) -> Client {
        Client::connect(&self.home).await
    }

    /// A fixture whose package is already installed, for the tests about services.
    async fn started_with_package() -> Self {
        let fixture = Self::start().await;
        let installed = fixture.client().await.install(VERSION).await;
        assert_eq!(installed["state"], "succeeded", "{installed}");

        fixture
    }

    /// Where a service's generated configuration goes.
    fn etc_for(&self, service: &str) -> std::path::PathBuf {
        self.home.path().join("etc").join(service)
    }

    /// Where the daemon would have put this version.
    fn installed_at(&self, version: &str) -> std::path::PathBuf {
        self.home
            .path()
            .join("packages")
            .join(PACKAGE)
            .join(version)
    }
}

/// An index offering one `fakeservice` for this machine, and one package this build cannot run.
fn index(packed: &Packed, url: &str) -> Value {
    let artifacts = json!([{
        "os": os(),
        "arch": arch(),
        "url": url,
        "sha256": packed.sha256,
        "size": packed.size(),
        "provides": { "fakeservice": program_name() },
    }]);

    json!({
        "schema": 1,
        "generated_at": "2026-08-19T06:55:12Z",
        "packages": [
            {
                "kind": PACKAGE,
                "version": VERSION,
                "channel": "stable",
                "artifacts": artifacts,
            },
            {
                // Published, installable for this machine, and still not offered: this build has no
                // recipe for it, so a download would end in a directory nothing could start.
                "kind": UNRUNNABLE,
                "version": UNRUNNABLE_VERSION,
                "channel": "stable",
                "artifacts": artifacts,
            },
        ],
    })
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
                "package.install",
                json!({"package": PACKAGE, "version": version}),
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

/// The whole of the package half of T31a in one test, because it is one sequence: a version is
/// offered, installed, listed and removed — and the directory on disk agrees at every step.
#[tokio::test]
async fn a_service_package_is_offered_installed_listed_and_removed() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    // Offered, and not yet here.
    let available = client.call("package.list_available", json!({})).await;
    let offered = &available["packages"];
    assert_eq!(
        offered.as_array().map(Vec::len),
        Some(1),
        "the package this build has no recipe for is not offered: {available}"
    );
    assert_eq!(offered[0]["package"], PACKAGE);
    assert_eq!(offered[0]["version"], VERSION);
    assert_eq!(offered[0]["installed"], false);
    assert_eq!(offered[0]["bytes"], fixture.packed.size());
    assert_eq!(
        available["stale"], false,
        "the registry answered, so nothing came out of a cache"
    );

    assert_eq!(
        client.call("package.list", json!({})).await["packages"],
        json!([]),
        "a home that has installed nothing lists nothing rather than failing"
    );

    // Installed.
    let installed = client.install(VERSION).await;
    assert_eq!(installed["state"], "succeeded", "{installed}");

    let package = &installed["outcome"]["result"];
    assert_eq!(package["package"], PACKAGE);
    assert_eq!(package["version"], VERSION);
    assert_eq!(
        package["services"],
        json!([]),
        "nothing can be an instance of a version installed a moment ago"
    );
    assert!(
        fixture.installed_at(VERSION).is_dir(),
        "the archive was unpacked where the row says it is"
    );

    // Listed, and no longer offered as something to install.
    let list = client.call("package.list", json!({})).await;
    assert_eq!(list["packages"].as_array().map(Vec::len), Some(1), "{list}");
    assert_eq!(list["packages"][0]["package"], PACKAGE);

    let available = client.call("package.list_available", json!({})).await;
    assert_eq!(
        available["packages"][0]["installed"], true,
        "the daemon composes this rather than leaving a client to cross-reference: {available}"
    );

    // Removed, directory and row together.
    let removal = client
        .call(
            "package.uninstall",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    assert_eq!(removal["removed"]["version"], VERSION);
    assert!(
        !fixture.installed_at(VERSION).exists(),
        "the directory goes with the row"
    );
    assert_eq!(
        client.call("package.list", json!({})).await["packages"],
        json!([])
    );
}

/// A kind this build has no recipe for is refused at install, not at create — nobody spends a
/// download on a directory MixEngine could never start anything out of.
#[tokio::test]
async fn a_package_this_build_cannot_run_is_refused_with_what_it_can() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let error = client
        .refuse(
            "package.install",
            json!({"package": UNRUNNABLE, "version": UNRUNNABLE_VERSION}),
        )
        .await;

    assert_eq!(error["data"]["code"], "invalid_argument", "{error}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains(UNRUNNABLE)),
        "it names what was asked for: {error}"
    );
    assert!(
        error["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("caddy")),
        "and what exists instead: {error}"
    );
}

/// The same rule seen from the listing side, filtered rather than surveyed.
#[tokio::test]
async fn a_filter_naming_something_unrunnable_is_refused_rather_than_answered_empty() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let error = client
        .refuse("package.list_available", json!({"package": UNRUNNABLE}))
        .await;

    assert_eq!(error["data"]["code"], "invalid_argument", "{error}");
}

/// Installing the same version twice is two terminals, and the second is asking for the outcome the
/// first already reached.
#[tokio::test]
async fn installing_a_version_that_is_already_here_says_so_rather_than_downloading_it_again() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    let error = client
        .refuse(
            "package.install",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;

    assert_eq!(error["data"]["code"], "already_exists", "{error}");
}

/// **A folder deleted by hand reads as missing, and installing it again restores the same row** —
/// roadmap task **T206d**, D1 and D3.
#[tokio::test]
async fn a_deleted_package_folder_is_missing_and_installing_it_again_restores_it() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");
    client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;

    std::fs::remove_dir_all(fixture.installed_at(VERSION)).expect("deleted by hand");

    let list = client.call("package.list", json!({})).await;
    assert_eq!(list["packages"][0]["missing"], true, "{list}");

    let restored = client.install(VERSION).await;
    assert_eq!(restored["state"], "succeeded", "{restored}");
    assert_eq!(
        restored["outcome"]["result"]["missing"], false,
        "{restored}"
    );
    assert_eq!(
        restored["outcome"]["result"]["services"],
        json!(["fakeservice@main"]),
        "the service still points at the same row"
    );
    assert!(fixture.installed_at(VERSION).is_dir());
}

/// **A version whose folder is gone is offered again** — roadmap task **T206d**, D2. This is what
/// locks MixLab's steps panel again over a devkit deleted by hand.
#[tokio::test]
async fn a_deleted_package_folder_is_offered_as_not_installed() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    std::fs::remove_dir_all(fixture.installed_at(VERSION)).expect("deleted by hand");

    let available = client.call("package.list_available", json!({})).await;
    assert_eq!(available["packages"][0]["installed"], false, "{available}");
}

/// The whole point of the task: an installed package becomes a service a person can start.
#[tokio::test]
async fn an_installed_package_becomes_a_service_and_can_be_deleted_again() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    let created = client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;
    assert_eq!(created["service"]["id"], "fakeservice@main");
    assert_eq!(created["service"]["state"], "stopped", "{created}");
    // A number, and not *the* number: the fixture recipe wishes for 41000, and `free` in this
    // workspace means free on the machine rather than free in a home of its own — anything else on
    // the runner holding it moves this service up, which is the allocator working. What T34c's own
    // suite proves about which number lands is proved in `mixengine_core::services::ports`.
    assert!(
        created["service"]["port"].is_u64(),
        "a service of a recipe that names a port was given none: {created}"
    );
    assert!(
        fixture.etc_for("fakeservice@main").is_dir(),
        "a create renders before it answers"
    );

    let list = client.call("service.list", Value::Null).await;
    assert_eq!(
        list["services"].as_array().map(Vec::len),
        Some(1),
        "a created service is a declared service: {list}"
    );

    // And the package it is an instance of is no longer one that can be removed.
    let held = client
        .refuse(
            "package.uninstall",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    assert_eq!(held["data"]["code"], "precondition_failed", "{held}");
    assert!(
        held["message"]
            .as_str()
            .is_some_and(|message| message.contains("fakeservice@main")),
        "it names what holds it: {held}"
    );

    let removal = client
        .call("service.delete", json!({"service": "fakeservice@main"}))
        .await;
    assert_eq!(removal["removed"]["id"], "fakeservice@main");
    assert!(
        !fixture.etc_for("fakeservice@main").exists(),
        "generated configuration is disposable and goes with the row"
    );
    assert_eq!(
        client.call("service.list", Value::Null).await["services"],
        json!([])
    );

    // Which frees the package.
    let removed = client
        .call(
            "package.uninstall",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    assert_eq!(removed["removed"]["version"], VERSION);
}

/// The recipe says how many instances it has, and the id is where a person meets the answer.
#[tokio::test]
async fn a_named_instance_recipe_refuses_an_id_with_no_instance() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    let error = client
        .refuse("service.create", json!({"id": PACKAGE, "version": VERSION}))
        .await;

    assert_eq!(error["data"]["code"], "invalid_argument", "{error}");
    assert!(
        error["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("fakeservice@")),
        "it shows the shape: {error}"
    );
}

/// A version nobody installed is a missing step rather than a mistake, and the hint is the step.
#[tokio::test]
async fn creating_a_service_from_a_package_that_is_not_installed_names_the_install() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let error = client
        .refuse(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;

    assert_eq!(error["data"]["code"], "precondition_failed", "{error}");
    assert!(
        error["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("package install")),
        "{error}"
    );
}

/// One row that cannot be rendered fails the whole declared set, so a bad row left behind would take
/// `service.list` down with it.
#[tokio::test]
async fn a_create_that_cannot_be_rendered_leaves_the_home_as_it_was() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    let error = client
        .refuse(
            "service.create",
            json!({
                "id": "fakeservice@bad",
                "version": VERSION,
                "overrides": {"exit_afterr": 1},
            }),
        )
        .await;
    assert_eq!(error["data"]["code"], "invalid_argument", "{error}");

    let list = client.call("service.list", Value::Null).await;
    assert_eq!(
        list["services"],
        json!([]),
        "the row went with the failure: {list}"
    );
}

/// A delete keeps the data directory, and says which one it kept.
#[tokio::test]
async fn a_delete_keeps_the_data_directory_and_says_so() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;

    // Created by hand rather than by starting the service: what is being tested is that a delete
    // leaves a directory alone, and the cheapest way to have one is to make one.
    let data = fixture.home.path().join("data").join(PACKAGE).join("main");
    std::fs::create_dir_all(&data).expect("a data directory in a temporary home");

    let removal = client
        .call("service.delete", json!({"service": "fakeservice@main"}))
        .await;

    let kept = removal["data_kept"]
        .as_str()
        .unwrap_or_else(|| panic!("a data directory is named: {removal}"));
    assert!(kept.ends_with("main"), "{kept}");
    assert!(data.is_dir(), "it is named because it is still there");
}

/// A row deleted out from under a live process would leave the process with nothing describing it.
#[tokio::test]
async fn a_running_service_is_not_deleted_out_from_under_itself() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;

    let walk = client
        .call("service.start", json!({"service": "fakeservice@main"}))
        .await;
    assert_eq!(walk["reached"], json!(["fakeservice@main"]), "{walk}");

    let error = client
        .refuse("service.delete", json!({"service": "fakeservice@main"}))
        .await;

    assert_eq!(error["data"]["code"], "precondition_failed", "{error}");
    assert!(
        error["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("service stop")),
        "{error}"
    );

    // And it goes once it is stopped, which is what makes the refusal a step rather than a wall.
    client
        .call("service.stop", json!({"service": "fakeservice@main"}))
        .await;
    client
        .call("service.delete", json!({"service": "fakeservice@main"}))
        .await;
}

/// **Spec D4.** A site declaring a service is a statement about the future, and `--force` crosses
/// it. After the delete the site is still there with the link gone — the cascade, not a second
/// write — which is the whole reason a pool is an `Option`.
#[tokio::test]
async fn a_service_a_site_declares_is_refused_and_then_forced() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;

    let root = fixture.home.path().join("blog");
    std::fs::create_dir_all(&root).expect("a project directory in a temporary home");

    client
        .call(
            "project.create",
            json!({"root": root.display().to_string(), "name": "blog"}),
        )
        .await;

    // `static`, because what is being tested is the link rather than PHP: a php-fpm site would ask
    // the resolver for a runtime this fixture deliberately has none of.
    client
        .call(
            "site.create",
            json!({
                "project": {"name": "blog"},
                "domains": ["blog.test"],
                "kind": {"kind": "static"},
                "services": ["fakeservice@main"],
            }),
        )
        .await;

    let refused = client
        .refuse("service.delete", json!({"service": "fakeservice@main"}))
        .await;
    assert_eq!(refused["data"]["code"], "precondition_failed", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("blog.test")),
        "it names the site that declares it: {refused}"
    );

    client
        .call(
            "service.delete",
            json!({"service": "fakeservice@main", "force": true}),
        )
        .await;

    let site = client
        .call("site.show", json!({"site": {"domain": "blog.test"}}))
        .await;
    assert_eq!(
        site["services"],
        json!([]),
        "the link went with the service: {site}"
    );
}

/// **`refresh` reaches the registry instead of answering from a still-fresh cache.**
///
/// [`tests/runtimes.rs`](../../mixengine-daemon/tests/runtimes.rs)'s test of the same name, for
/// `package.list_available` — the two share one `mixengine_core::index::Client<Index>` under the
/// daemon, so this is also proof that `runtime available --refresh` and `package available
/// --refresh` do not each refresh only the other's half of one document.
#[tokio::test]
async fn refresh_bypasses_a_fresh_cache() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    // Fetches and caches the index the fixture published.
    let first = client.call("package.list_available", json!({})).await;
    assert_eq!(
        first["packages"].as_array().map(Vec::len),
        Some(1),
        "{first}"
    );

    // Republished with nothing offered. The cache is still fresh, so an ordinary call keeps
    // answering from it — the behaviour `refresh` exists to bypass.
    fixture._registry.publish(&json!({
        "schema": 1,
        "generated_at": "2026-08-19T06:55:13Z",
        "packages": [],
    }));

    let cached = client.call("package.list_available", json!({})).await;
    assert_eq!(
        cached["packages"].as_array().map(Vec::len),
        Some(1),
        "a fresh cache is not asked about again: {cached}"
    );

    let refreshed = client
        .call("package.list_available", json!({"refresh": true}))
        .await;
    assert_eq!(
        refreshed["packages"].as_array().map(Vec::len),
        Some(0),
        "`refresh` reaches the registry instead of answering from the cache: {refreshed}"
    );
}

/// **T182f, D1.** A package install leaves its marker too, for the same reason a runtime does.
#[tokio::test]
async fn an_installed_package_carries_its_marker() {
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

    assert_eq!(marker["what"], "package", "{marker}");
    assert_eq!(marker["package"], PACKAGE, "{marker}");
    assert_eq!(marker["version"], VERSION, "{marker}");
}

/// **T182f.** `package.adopt` answers a recorded version's row, and refuses one not on disk by name.
#[tokio::test]
async fn adopting_a_package_answers_its_row_or_says_it_is_not_there() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    let installed = client.install(VERSION).await;
    assert_eq!(installed["state"], "succeeded", "{installed}");

    let adopted = client
        .call(
            "package.adopt",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    assert_eq!(adopted["version"], VERSION, "{adopted}");

    let refused = client
        .refuse(
            "package.adopt",
            json!({"package": PACKAGE, "version": "9.9.7"}),
        )
        .await;
    assert_eq!(refused["data"]["code"], "not_found", "{refused}");
}

/// **T182g.** Service data an earlier home left under `data/` is listed with the version that would
/// open it, and adopting it makes a stopped service whose data directory is that one. A directory
/// whose first run never finished is listed with why it cannot be adopted, and left alone.
#[tokio::test]
async fn a_found_instance_is_listed_and_adopting_it_makes_it_a_stopped_service() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    let kept = fixture.home.path().join("data").join(PACKAGE).join("main");
    std::fs::create_dir_all(&kept).expect("a kept data directory");
    std::fs::write(kept.join("somebody.db"), b"data").expect("its data");
    std::fs::write(kept.join(".mixengine-ready"), format!("{VERSION}\n")).expect("its marker");

    let half = fixture.home.path().join("data").join(PACKAGE).join("half");
    std::fs::create_dir_all(&half).expect("a half-made data directory");
    std::fs::write(half.join("somebody.db"), b"data").expect("its data");

    let found = client.call("service.found", Value::Null).await;
    let rows = found["found"].as_array().expect("a list");
    let row = |id: &str| {
        rows.iter()
            .find(|row| row["service"] == id)
            .unwrap_or_else(|| panic!("{id} is listed: {found}"))
            .clone()
    };
    assert_eq!(row("fakeservice@main")["opens_with"], VERSION, "{found}");
    assert!(row("fakeservice@half")["why_not"].is_string(), "{found}");

    let doctor = client.call("daemon.doctor", Value::Null).await.to_string();
    assert!(
        doctor.contains("mix service found"),
        "the doctor names found service data: {doctor}"
    );

    let adopted = client
        .call("service.adopt", json!({"service": "fakeservice@main"}))
        .await;
    assert_eq!(adopted["id"], "fakeservice@main", "{adopted}");
    assert_eq!(adopted["state"], "stopped", "{adopted}");
    assert!(
        kept.join("somebody.db").is_file(),
        "its data is where it was"
    );

    let again = client
        .call("service.adopt", json!({"service": "fakeservice@main"}))
        .await;
    assert_eq!(
        again["id"], "fakeservice@main",
        "adopting twice answers the service: {again}"
    );

    let found = client.call("service.found", Value::Null).await;
    assert!(
        found["found"]
            .as_array()
            .is_some_and(|rows| rows.iter().all(|row| row["service"] != "fakeservice@main")),
        "an adopted instance is no longer found: {found}"
    );

    let refused = client
        .refuse("service.adopt", json!({"service": "fakeservice@half"}))
        .await;
    assert_eq!(refused["data"]["code"], "precondition_failed", "{refused}");
    assert!(
        half.join("somebody.db").is_file(),
        "a refused adopt touches nothing"
    );

    let refused = client
        .refuse("service.adopt", json!({"service": "fakeservice@nope"}))
        .await;
    assert_eq!(refused["data"]["code"], "not_found", "{refused}");
}

/// **T182i.** `package.found` lists a package directory with no row, with why.
#[tokio::test]
async fn a_package_directory_with_no_row_is_listed_with_why() {
    let fixture = Fixture::start().await;
    let stray = fixture.installed_at("9.9.9");
    std::fs::create_dir_all(&stray).expect("a directory nothing installed");

    let mut client = fixture.client().await;
    let found = client.call("package.found", json!({})).await;
    let row = &found["found"][0];
    assert_eq!(row["package"], PACKAGE, "{found}");
    assert_eq!(row["version"], "9.9.9", "{found}");
    assert!(
        row["why"].as_str().is_some_and(|why| why.contains("index")),
        "{found}"
    );
    assert!(stray.is_dir(), "listing touches nothing");
}

/// **T182h over T182g.** While a copy of the earlier home can be restored, its service data is not
/// offered for adopting one by one: adopting a database first would give this home a service of its
/// own, and the restore that brings back the projects and sites would then be refused.
#[tokio::test]
async fn service_data_is_not_offered_while_a_restore_is() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    let kept = fixture.home.path().join("data").join(PACKAGE).join("main");
    std::fs::create_dir_all(&kept).expect("a kept data directory");
    std::fs::write(kept.join("somebody.db"), b"data").expect("its data");
    std::fs::write(kept.join(".mixengine-ready"), format!("{VERSION}\n")).expect("its marker");

    let scratch = tempfile::tempdir().expect("a scratch home");
    let other = mixengine_core::Store::open(&scratch.path().join("mixengine.db"))
        .await
        .expect("an earlier home's database");
    let data = fixture.home.path().join("data");
    mixengine_core::adopt::snapshot::write(&other, &data)
        .await
        .expect("a copy");
    other.close().await;

    let found = client.call("service.found", Value::Null).await;
    assert_eq!(
        found["found"],
        json!([]),
        "a restore is offered instead: {found}"
    );

    std::fs::remove_file(data.join(".mixengine-state.db")).expect("the copy");
    let found = client.call("service.found", Value::Null).await;
    assert_eq!(
        found["found"][0]["service"], "fakeservice@main",
        "with no copy, the data is offered again: {found}"
    );
}

/// Two patches of `fakeservice`, the same archive — roadmap task **T193a**.
fn two_patches(packed: &Packed, url: &str) -> Value {
    let mut index = index(packed, url);
    let mut next = index["packages"][0].clone();
    next["version"] = json!("1.0.1");
    index["packages"].as_array_mut().expect("a list").push(next);
    index
}

/// **T193a.** An installed package with a newer patch of its line is named with that patch.
#[tokio::test]
async fn a_package_catalogue_says_lines_and_updates_too() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    let catalogue = client.call("package.list_available", json!({})).await;
    assert_eq!(
        catalogue["updates"],
        json!([{
            "package": PACKAGE, "from": "1.0.0", "to": "1.0.1", "to_installed": false, "needs": []
        }]),
        "{catalogue}"
    );
}

/// An update that kept its old version is not offered again, as for runtimes.
#[tokio::test]
async fn a_package_update_that_kept_its_version_is_not_offered_again() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    let job = client
        .call(
            "package.upgrade",
            json!({"package": PACKAGE, "from": VERSION, "keep": true}),
        )
        .await;
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(finished["state"], "succeeded", "{finished}");
    assert_eq!(
        finished["outcome"]["result"]["old"]["state"], "kept",
        "{finished}"
    );

    let catalogue = client.call("package.list_available", json!({})).await;
    assert_eq!(catalogue["updates"], json!([]), "{catalogue}");
}

/// `fakeservice@main` on `version`, started.
async fn a_running_instance(client: &mut Client, version: &str) {
    client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": version}),
        )
        .await;
    let started = client
        .call(
            "service.start",
            json!({"service": "fakeservice@main", "wait": true}),
        )
        .await;
    assert_eq!(started["complete"], true, "{started}");
}

/// **T193c, D6.** The instance moves, runs again, and the old version goes.
#[tokio::test]
async fn a_package_upgrade_moves_a_running_instance_and_removes_the_old_version() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");
    a_running_instance(&mut client, VERSION).await;

    let plan = client
        .call(
            "package.upgrade_plan",
            json!({"package": PACKAGE, "from": VERSION}),
        )
        .await;
    assert_eq!(plan["to"], "1.0.1", "{plan}");
    assert_eq!(plan["entries"][0]["item"]["item"], "instance", "{plan}");
    assert_eq!(plan["entries"][0]["item"]["restarts"], true, "{plan}");

    let job = client
        .call(
            "package.upgrade",
            json!({"package": PACKAGE, "from": VERSION}),
        )
        .await;
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(finished["state"], "succeeded", "{finished}");
    assert_eq!(
        finished["outcome"]["result"]["old"]["state"], "removed",
        "{finished}"
    );

    let services = client.call("service.list", json!({})).await;
    let main = services["services"]
        .as_array()
        .expect("T193 fixture")
        .iter()
        .find(|service| service["id"] == "fakeservice@main")
        .cloned()
        .unwrap_or_else(|| panic!("{services}"));
    assert_eq!(main["version"], "1.0.1", "{main}");
    assert_eq!(main["state"], "running", "{main}");
}

/// **T193c, Error handling.** A package being moved cannot be uninstalled.
#[tokio::test]
async fn a_package_being_upgraded_cannot_be_uninstalled() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    let upgrading = client
        .call(
            "package.upgrade",
            json!({"package": PACKAGE, "from": VERSION}),
        )
        .await;
    let refused = client
        .refuse(
            "package.uninstall",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    if refused["data"]["code"] != "not_found" {
        assert_eq!(refused["data"]["code"], "conflict", "{refused}");
    }
    client.finished(upgrading["id"].clone()).await;
}

impl Fixture {
    /// `fakeservice` 1.0.0, and a 1.0.1 whose program is not one — T193c, added at execution.
    ///
    /// **A second archive rather than a second name in the first**, because a server's program is
    /// found by its package's name inside the install (`generate::program`), not through
    /// `provides`; and `fakeservice` has no smoke test, so the broken build installs and fails only
    /// when it is started — which is the moment D6 step 4 is about.
    async fn start_with_a_broken_next_patch() -> Self {
        let packing = match cfg!(windows) {
            true => Packing::Zip,
            false => Packing::TarZst,
        };
        let packed = FakePackage::new(packing)
            .executable(&program_name())
            .build(&format!("{PACKAGE}-{VERSION}"));
        let broken = FakePackage::new(packing)
            .file(&program_name(), b"not a program")
            .build(&format!("{PACKAGE}-1.0.1"));

        let registry = MockRegistry::start(&json!({
            "schema": 1,
            "generated_at": "2026-09-29T06:55:12Z",
            "packages": [],
        }))
        .await;

        let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
        let broken_url = registry.publish_asset(&broken.path(), broken.bytes.clone());

        let mut published = index(&packed, &url);
        let mut next = published["packages"][0].clone();
        next["version"] = json!("1.0.1");
        next["artifacts"][0]["url"] = json!(broken_url);
        next["artifacts"][0]["sha256"] = json!(broken.sha256);
        next["artifacts"][0]["size"] = json!(broken.size());
        published["packages"]
            .as_array_mut()
            .expect("a list")
            .push(next);
        registry.publish(&published);

        let home = Home::new();
        let daemon = Daemon::start(&home, &registry);
        home.wait_until_listening().await;

        Self {
            home,
            _registry: registry,
            _daemon: daemon,
            packed,
        }
    }
}

/// **T193c, D6 step 4 — added at execution.** An instance that does not start on the new patch is
/// put back on the old one and started again, and the old version is kept.
///
/// **And the job fails**, saying so — the final review's finding: a job that ended `succeeded`
/// here let `mix package upgrade` exit zero and MixLab say nothing, over an update that did not
/// happen.
#[tokio::test]
async fn a_package_that_does_not_start_on_the_new_patch_goes_back() {
    let fixture = Fixture::start_with_a_broken_next_patch().await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");
    a_running_instance(&mut client, VERSION).await;

    let job = client
        .call(
            "package.upgrade",
            json!({"package": PACKAGE, "from": VERSION}),
        )
        .await;
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(
        finished["state"],
        "failed",
        "an update that moved nothing is a failed job\n{finished}\n--- daemon ---\n{}",
        fixture.home.daemon_log()
    );
    let said = finished["outcome"]["error"]["message"]
        .as_str()
        .unwrap_or_default();
    assert!(
        said.contains("fakeservice@main") && said.contains(VERSION),
        "the error names the instance and the version it is back on: {finished}"
    );

    let services = client.call("service.list", json!({})).await;
    let main = services["services"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|service| service["id"] == "fakeservice@main")
        .cloned()
        .unwrap_or_else(|| panic!("{services}"));
    assert_eq!(main["version"], VERSION, "back on the old patch: {main}");
    assert_eq!(main["state"], "running", "and running again: {main}");
}

/// `package.list_available` names the package whose file could not be read — roadmap task
/// **T196**, its design's D5, the twin of the runtime test.
#[tokio::test]
async fn a_package_that_cannot_be_read_is_named() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    fixture._registry.corrupt_kind(PACKAGE);

    let listed = client
        .call("package.list_available", json!({"refresh": true}))
        .await;

    assert_eq!(
        listed["packages"].as_array().map(Vec::len),
        Some(0),
        "{listed}"
    );
    let gaps = listed["unavailable"]
        .as_array()
        .expect("a daemon that says what it could not read");
    assert_eq!(gaps.len(), 1, "{listed}");
    assert_eq!(gaps[0]["name"], PACKAGE);

    let refused = client
        .refuse(
            "package.requirements",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("could not be read for")),
        "{refused}"
    );
}
