//! A `web-app` extension's site, against a real `mixengined` — roadmap task **T81b**.
//!
//! `core`'s tests prove the rows; what only a daemon can be wrong about is the surface: that the
//! site an install wrote is listed with its owner, that `site.*` refuses to edit it and allows
//! stopping it, that removing the PHP it runs on is refused by name, and that an uninstall takes
//! it away and says so. The PHP is a row and a pool row (`declare::php_pool`), not a download.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use http_body_util::{BodyExt as _, Full};
use hyper::body::Bytes;
use hyper::header::{CONTENT_TYPE, HOST};
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use mixengine_platform::ipc::Connection;
use mixengine_testkit::{Home, declare};
use serde_json::{Value, json};

/// How long an install of a directory copy may take.
const PATIENCE: Duration = Duration::from_secs(30);

struct Fixture {
    home: Home,
    _daemon: Daemon,
}

impl Fixture {
    async fn start() -> Self {
        let home = Home::new();
        let daemon = Daemon::start(&home);
        home.wait_until_listening().await;

        Self {
            home,
            _daemon: daemon,
        }
    }

    async fn client(&self) -> Client {
        Client::connect(&self.home).await
    }
}

struct Daemon(Child);

impl Daemon {
    fn start(home: &Home) -> Self {
        Self(
            Command::new(env!("CARGO_BIN_EXE_mixengined"))
                .arg("--home")
                .arg(home.path())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("the daemon binary runs"),
        )
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One connection to the daemon. The same helpers `tests/runtimes.rs` carries.
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

    async fn call(&mut self, method: &str, params: Value) -> Value {
        let answer = self.ask(method, params).await;
        assert!(answer.get("error").is_none(), "{method}: {answer}");
        answer["result"].clone()
    }

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

/// A directory holding the phpMyAdmin fixture and the doc root it names.
fn web_app() -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix("mixengine-web-app")
        .tempdir()
        .expect("a temporary directory");
    std::fs::write(
        directory.path().join("extension.toml"),
        mixengine_testkit::extension::PHPMYADMIN,
    )
    .expect("a manifest");
    // **The archive's own top level** — roadmap task **T82**. `--path` copies what a download would
    // have unpacked, so the directory here is the one `[web-app].root` names.
    std::fs::create_dir_all(directory.path().join("phpMyAdmin-5.2.3-all-languages"))
        .expect("a doc root");

    directory
}

/// Plan, consent, install, wait: the extension is installed and the job succeeded. Answers with
/// the plan, which is what the person read.
async fn installed(client: &mut Client, path: &str, log: &Home) -> Value {
    let source = json!({"type": "path", "path": path});
    let plan = client
        .call("extension.plan", json!({"source": source}))
        .await;
    let started = client
        .call(
            "extension.install",
            json!({
                "source": source,
                "consent": {
                    "id": plan["id"],
                    "version": plan["version"],
                    "signed": plan["signed"],
                    "network": plan["permissions"]["network"],
                },
            }),
        )
        .await;

    let finished = client.finished(started["id"].clone()).await;
    assert_eq!(
        finished["state"],
        "succeeded",
        "{finished}\n{}",
        log.daemon_log()
    );

    plan
}

/// **D4, D5, D6, D8 in one walk.**
#[tokio::test(flavor = "multi_thread")]
async fn a_web_app_is_served_on_a_site_only_its_extension_may_edit() {
    let fixture = Fixture::start().await;
    declare::php_pool(&fixture.home.database_file(), "8.3.34").await;
    // **T82.** The fixture web-app declares a database, so this home has to run one — a plan
    // refused before anything is fetched is the point of that declaration, not an accident here.
    declare::database(
        &fixture.home.database_file(),
        "mariadb@main",
        "mariadb",
        3306,
    )
    .await;
    let mut client = fixture.client().await;
    let directory = web_app();
    let path = directory.path().display().to_string();

    let plan = installed(&mut client, &path, &fixture.home).await;
    assert_eq!(
        plan["site"]["domain"], "phpmyadmin.mixengine.test",
        "{plan}"
    );
    // **The pool is the extension's own** — roadmap task **T82a**, that design's D1. The PHP it
    // runs out of is still `8.3.34`, which is what `declare::php_pool` put there; what changed is
    // that a `web-app` is not served from the process every project site is served from.
    assert_eq!(plan["site"]["pool"], "php-fpm@phpmyadmin", "{plan}");

    // Listed with its owner, HTTPS on, enabled.
    let listed = client.call("site.list", json!({})).await;
    let sites = listed["sites"].as_array().expect("a list");
    assert_eq!(sites.len(), 1, "{listed}");
    assert_eq!(sites[0]["domain"], "phpmyadmin.mixengine.test");
    assert_eq!(
        sites[0]["owner"],
        json!({"type": "extension", "id": "phpmyadmin"})
    );
    assert_eq!(sites[0]["https"], true);
    assert_eq!(sites[0]["state"], "enabled");

    let extensions = client.call("extension.list", json!({})).await;
    assert_eq!(
        extensions["extensions"][0]["site"], "phpmyadmin.mixengine.test",
        "{extensions}"
    );
    // **T200, D5.** What it is for travels with the listing, read from the manifest it was
    // installed from.
    assert_eq!(
        extensions["extensions"][0]["description"], plan["description"],
        "{extensions}"
    );

    // Its root is the install directory, and its pool is the one the plan named.
    let site = json!({"site": {"domain": "phpmyadmin.mixengine.test"}});
    let shown = client.call("site.show", site.clone()).await;
    assert_eq!(shown["root"], plan["install_dir"], "{shown}");
    // **The archive's own top level** — roadmap task **T82**. `doc_root_exists` is what would
    // report a `[web-app].root` naming a directory the artifact does not unpack to, so it is
    // asserted here beside the path rather than left to a run somebody has to do by hand.
    assert!(
        shown["doc_root_full"]
            .as_str()
            .is_some_and(|full| full.ends_with("phpMyAdmin-5.2.3-all-languages")),
        "{shown}"
    );
    assert_eq!(shown["doc_root_exists"], true, "{shown}");
    assert_eq!(shown["pool"]["declared"], "php-fpm@phpmyadmin", "{shown}");

    // **The credential reaches this pool's workers and no other pool's** — roadmap task **T82a**,
    // its design's D3. The rule lives in the template and is unit-tested there; what only a real
    // install can say is that the whole chain arrives — the manifest's `signs_in`, the link the
    // install froze, the credential the generator resolved, and the file on disk. This is the
    // assertion that would catch a database superuser's password being handed to every project's
    // PHP.
    let owned = fixture
        .home
        .path()
        .join("etc")
        .join("php-fpm@phpmyadmin")
        .join("php-fpm.conf");
    let shared = fixture
        .home
        .path()
        .join("etc")
        .join("php-fpm@8.3.34")
        .join("php-fpm.conf");

    let owned = std::fs::read_to_string(&owned)
        .unwrap_or_else(|error| panic!("the extension's pool file at {owned:?}: {error}"));
    let shared = std::fs::read_to_string(&shared)
        .unwrap_or_else(|error| panic!("the shared pool file at {shared:?}: {error}"));

    assert!(owned.contains("clear_env = no"), "{owned}");
    assert!(
        !shared.contains("clear_env"),
        "every other pool leaves php-fpm's own `clear_env = yes`\n{shared}"
    );

    // Every edit is refused with the one sentence; start and stop are not.
    for (method, params) in [
        (
            "site.update",
            json!({"site": {"domain": "phpmyadmin.mixengine.test"}, "https": false}),
        ),
        ("site.delete", site.clone()),
        (
            "site.share",
            json!({"site": {"domain": "phpmyadmin.mixengine.test"}}),
        ),
        (
            "domain.add",
            json!({"site": {"domain": "phpmyadmin.mixengine.test"}, "domain": "pma.test"}),
        ),
    ] {
        let refused = client.refuse(method, params).await;
        assert_eq!(
            refused["data"]["code"], "precondition_failed",
            "{method}: {refused}"
        );
        assert!(
            refused["message"]
                .as_str()
                .is_some_and(|said| said.contains("belongs to the phpmyadmin extension")),
            "{method}: {refused}"
        );
        assert!(
            refused["data"]["hint"]
                .as_str()
                .is_some_and(|hint| hint.contains("mix extension uninstall phpmyadmin")),
            "{method}: {refused}"
        );
    }

    let stopped = client.call("site.stop", site.clone()).await;
    assert_eq!(stopped["site"]["state"], "disabled", "{stopped}");
    let started = client.call("site.start", site.clone()).await;
    assert_eq!(started["site"]["state"], "enabled", "{started}");

    // `extension.start` says what controls it instead.
    let refused = client
        .refuse("extension.start", json!({"id": "phpmyadmin"}))
        .await;
    assert!(
        refused["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("mix site stop phpmyadmin.mixengine.test")),
        "{refused}"
    );

    // Uninstall releases the name, and the site is gone.
    let removed = client
        .call("extension.uninstall", json!({"id": "phpmyadmin"}))
        .await;
    assert_eq!(removed["site"], "phpmyadmin.mixengine.test", "{removed}");
    let listed = client.call("site.list", json!({})).await;
    assert!(
        listed["sites"].as_array().is_some_and(Vec::is_empty),
        "{listed}"
    );
}

/// **D9.** The PHP a web-app is frozen on is refused by name, without `--force`.
#[tokio::test(flavor = "multi_thread")]
async fn runtime_uninstall_refuses_for_the_extension_frozen_on_it() {
    let fixture = Fixture::start().await;
    declare::php_pool(&fixture.home.database_file(), "8.3.34").await;
    // **T82.** The fixture web-app declares a database, so this home has to run one — a plan
    // refused before anything is fetched is the point of that declaration, not an accident here.
    declare::database(
        &fixture.home.database_file(),
        "mariadb@main",
        "mariadb",
        3306,
    )
    .await;
    let mut client = fixture.client().await;
    let directory = web_app();
    installed(
        &mut client,
        &directory.path().display().to_string(),
        &fixture.home,
    )
    .await;

    let refused = client
        .refuse(
            "runtime.uninstall",
            json!({"kind": "php", "version": "8.3.34", "force": false}),
        )
        .await;

    assert_eq!(refused["data"]["code"], "precondition_failed", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|said| said.contains("phpmyadmin (extension)")),
        "{refused}"
    );
}

/// **D5.** No PHP inside `requires`: refused at plan, naming what to install.
#[tokio::test(flavor = "multi_thread")]
async fn a_web_app_with_no_matching_php_is_refused_at_plan() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    let directory = web_app();

    let refused = client
        .refuse(
            "extension.plan",
            json!({"source": {"type": "path", "path": directory.path().display().to_string()}}),
        )
        .await;

    assert_eq!(refused["data"]["code"], "dependency_missing", "{refused}");
    assert!(
        refused["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("mix runtime")),
        "{refused}"
    );
}

/// Whether something accepts a TCP connection on this loopback port.
fn accepts(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_secs(2),
    )
    .is_ok()
}

/// **T200, D1.** A web-app's pool can be woken by a request the moment the install ends.
///
/// The pool is created by the install, outside boot, so it has neither an activation port nor a
/// listener until something gives it one — and a site naming a pool that nothing can wake answers
/// 502 until the daemon is restarted. That is what a person installing Adminer met. A pool on a
/// Unix socket derives its activator from its own path, so only a TCP pool has a port to assert.
#[tokio::test(flavor = "multi_thread")]
async fn a_web_app_can_be_woken_the_moment_it_is_installed() {
    let fixture = Fixture::start().await;
    declare::php_pool(&fixture.home.database_file(), "8.3.34").await;
    declare::database(
        &fixture.home.database_file(),
        "mariadb@main",
        "mariadb",
        3306,
    )
    .await;
    let mut client = fixture.client().await;
    let directory = web_app();
    let path = directory.path().display().to_string();

    installed(&mut client, &path, &fixture.home).await;

    let port = declare::activation_port(&fixture.home.database_file(), "php-fpm@phpmyadmin").await;

    if cfg!(windows) {
        let port = port.unwrap_or_else(|| {
            panic!(
                "a TCP pool created by an install has no activation port
{}",
                fixture.home.daemon_log()
            )
        });
        assert!(
            accepts(port),
            "nothing holds the activator's address 127.0.0.1:{port} after the install
{}",
            fixture.home.daemon_log()
        );
    } else {
        assert_eq!(
            port, None,
            "a socket pool's activator is derived from its path, never allocated"
        );
    }
}

/// **T200, D2.** Uninstalling gives the activator's address back, and a reinstall is woken too.
///
/// Before T200 the daemon kept every activator until it exited and skipped any service it already
/// held, by id alone. An uninstall left the old port listening, and a reinstall under the same id
/// was given a new port that nothing ever bound — so "uninstall it and install it again" failed
/// exactly like the first install.
#[tokio::test(flavor = "multi_thread")]
async fn a_reinstalled_web_app_can_be_woken_and_the_old_address_is_free() {
    let fixture = Fixture::start().await;
    declare::php_pool(&fixture.home.database_file(), "8.3.34").await;
    declare::database(
        &fixture.home.database_file(),
        "mariadb@main",
        "mariadb",
        3306,
    )
    .await;
    let mut client = fixture.client().await;
    let directory = web_app();
    let path = directory.path().display().to_string();
    let database = fixture.home.database_file();

    installed(&mut client, &path, &fixture.home).await;
    let first = declare::activation_port(&database, "php-fpm@phpmyadmin").await;

    client
        .call(
            "extension.uninstall",
            json!({"id": "phpmyadmin", "delete_data": true}),
        )
        .await;

    if let Some(port) = first {
        assert!(
            !accepts(port),
            "the activator at 127.0.0.1:{port} outlived the extension it belonged to
{}",
            fixture.home.daemon_log()
        );
    }

    installed(&mut client, &path, &fixture.home).await;
    let second = declare::activation_port(&database, "php-fpm@phpmyadmin").await;

    if cfg!(windows) {
        let port = second.unwrap_or_else(|| {
            panic!(
                "the reinstalled pool has no activation port
{}",
                fixture.home.daemon_log()
            )
        });
        assert!(
            accepts(port),
            "nothing holds the reinstalled pool's activator at 127.0.0.1:{port}
{}",
            fixture.home.daemon_log()
        );
    }
}

/// A directory holding a `service` extension that really runs: this crate's `fakeservice`, told to
/// hold the port its ready check dials.
fn a_running_service() -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix("mixengine-service-extension")
        .tempdir()
        .expect("a temporary directory");

    let program = mixengine_testkit::FakeService::program();
    std::fs::copy(
        &program,
        directory
            .path()
            .join(program.file_name().expect("a file name")),
    )
    .expect("the fixture program is copied in");

    std::fs::write(
        directory.path().join("extension.toml"),
        r#"schema = 1

[extension]
id = "catcher"
name = "Catcher"
version = "1.0.0"
kind = "service"
description = "A service that runs until it is stopped"

[ports]
ui_port = 18025

[service]
program = "{install_dir}/fakeservice"
cwd = "{data_dir}"
args = ["--listen", "{listen}:{ui_port}"]
ready = { type = "tcp", addr = "{listen}:{ui_port}", timeout = "10s" }

[permissions]
network = "loopback"
filesystem = ["own-data"]

[ui]
port = "ui_port"
path = "/inbox"
"#,
    )
    .expect("a manifest");

    directory
}

/// **Uninstalling a running `service` extension stops it first, so it can be installed again** —
/// found by T200's hand check.
///
/// The daemon stopped a `web-app`'s pool before the rows went (T82a) and nothing else: a running
/// Mailpit had its row deleted from under it, its process went on holding its own executable, the
/// install directory could not be removed (`Access is denied` on Windows), and the next install
/// was refused because that directory was still there.
#[tokio::test(flavor = "multi_thread")]
async fn a_running_service_extension_is_stopped_by_its_uninstall() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    let directory = a_running_service();
    let path = directory.path().display().to_string();

    let plan = installed(&mut client, &path, &fixture.home).await;
    let install_dir = std::path::PathBuf::from(
        plan["install_dir"]
            .as_str()
            .expect("the plan names where it installs"),
    );

    // **T200a, D2.** The page a person opens, rendered by the daemon from the port it allocated.
    let listed = client.call("extension.list", json!({})).await;
    let catcher = &listed["extensions"][0];
    let port = catcher["ports"][0]["wanted"]
        .as_u64()
        .expect("the port it holds");
    assert_eq!(
        catcher["ui"],
        format!("http://127.0.0.1:{port}/inbox"),
        "{listed}"
    );

    client
        .call("extension.start", json!({"id": "catcher"}))
        .await;

    let answer = client
        .ask(
            "extension.uninstall",
            json!({"id": "catcher", "delete_data": true}),
        )
        .await;
    assert!(
        answer.get("error").is_none(),
        "uninstalling a running service extension was refused: {answer}\n{}",
        fixture.home.daemon_log()
    );
    assert!(
        !install_dir.exists(),
        "the install directory outlived the uninstall: {}\n{}",
        install_dir.display(),
        fixture.home.daemon_log()
    );

    installed(&mut client, &path, &fixture.home).await;
}

/// **Deleting a pool gives its activator's address back** — T200b's follow-through on T200's D2.
///
/// `extension.uninstall` and `runtime.uninstall` released an activator with its service;
/// `service.delete` did not, so a deleted pool's port stayed bound until the daemon exited. The
/// activator here is the one boot gives a pool, which is why the daemon is started twice.
#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_pool_gives_its_activators_address_back() {
    let home = Home::new();
    {
        let _first = Daemon::start(&home);
        home.wait_until_listening().await;
        declare::php_pool(&home.database_file(), "8.3.34").await;
    }

    let _daemon = Daemon::start(&home);
    home.wait_until_listening().await;
    let mut client = Client::connect(&home).await;

    if cfg!(windows) {
        // Boot gives ports and binds them in a task of its own, after the API is already answering.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let port = loop {
            if let Some(port) =
                declare::activation_port(&home.database_file(), "php-fpm@8.3.34").await
                && accepts(port)
            {
                break port;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "boot never bound the pool's activator\n{}",
                home.daemon_log()
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        };

        // The probe above is a connection, and a connection is what wakes a pool: stop it, since
        // `service.delete` refuses a running service.
        client
            .call("service.stop", json!({"service": "php-fpm@8.3.34"}))
            .await;
        client
            .call("service.delete", json!({"service": "php-fpm@8.3.34"}))
            .await;

        assert!(
            !accepts(port),
            "the activator at 127.0.0.1:{port} outlived the pool it belonged to\n{}",
            home.daemon_log()
        );
    }
}
