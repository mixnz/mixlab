//! The MariaDB recipe against a **real** MariaDB.
//!
//! Everything else about this recipe is provable in one process and is proved there: the template
//! renders with every path quoted, the settings merge, the spec builds, the file and the readiness
//! check name one port, the bootstrap SQL says what it does. None of that says the thing the task is
//! about, which is that *a data directory MixEngine bootstrapped becomes a database that answers a
//! query as the root it generated a password for*. That claim can only be made against the server.
//!
//! **It is `#[ignore]`d rather than skipped**, for `caddy.rs`' reason: a test that quietly returns
//! when it finds no MariaDB is a green suite that proved nothing on the day the download broke.
//!
//! **This suite needs a credential store**, which is the one way it differs from the other two: the
//! root password has exactly one home, and a machine with none refuses the bootstrap by design. On
//! Windows and macOS the store is part of the OS; on Linux `.github/scripts/test-no-network.sh`
//! starts a `gnome-keyring` on a session bus of its own, which is why the Linux leg runs this from
//! inside that script rather than beside it.
//!
//! # Why nothing in here ever reads that credential
//!
//! It did, once, and macOS is where that stopped. A keychain item carries an ACL naming the
//! application that created it, so the daemon reads its own credential without a word and **any
//! other process asking for it raises a dialog** — on a CI runner, one nobody can answer. Measured:
//! the whole suite bootstrapped, started and reached `running` in twenty-six seconds and then sat in
//! that read until the job's own timeout killed it, twenty-seven minutes later.
//!
//! It is not worth a `cfg`, because the assertion it was making is made better without it. **The
//! service reaching `running` is the proof**: its ready check is an authenticated `mariadb-admin
//! ping`, whose password the daemon resolves out of the keyring at spawn, so a store that held
//! nothing, or held the wrong thing, could not produce a running service. Everything else this
//! suite asks the server is a connection that must be **refused** — which needs no credential at
//! all, and says what a successful query never could: that there is no way in without one.

#[path = "../harness/mod.rs"]
mod harness;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use harness::{Home, json};
// Every port this suite asks for comes from `harness::frontend::free_port`, which hands out
// numbers no `bind(:0)` on the machine can be given (run 36175716790).
//
// The usual race is the usual price, and it is worth paying here rather than fixing on 3306: a
// developer running this suite very likely has a MariaDB of their own, and a test that took its
// port would be a test that stops somebody's work.
use harness::frontend::free_port;
use mixengine_testkit::{FakePackage, MockRegistry, Packed, Packing};
use serde_json::Value;

/// How long the whole of this suite is given before a hang is reported as the hang it is.
///
/// Twelve minutes, against a Linux run that takes thirty-six seconds and a Windows one that takes
/// four minutes with a cold Defender in the way. It is not a budget anything is expected to come
/// near — it is the line past which *waiting* stops being the answer.
const BUDGET: Duration = Duration::from_secs(720);

/// What this suite is doing, for the thread that has to report a hang.
static STAGE: Mutex<&'static str> = Mutex::new("packing the archive and starting a daemon");

/// Say what is happening now, in the log and to the watchdog.
fn at(stage: &'static str) {
    *STAGE.lock().expect("nothing panics holding this") = stage;
    eprintln!("[mariadb] {stage}");
}

/// Turn a hang into a failure that says where it hung.
///
/// **Measured, and it cost a whole CI run.** The macOS leg of the first run of this suite spent
/// twenty-seven minutes inside it and was killed by the job's own timeout, having printed *nothing*
/// — because libtest holds a running test's output until the test ends, and this one never ended.
/// A thread is what reports it rather than a `tokio::time::timeout`, and the difference is the one
/// that matters here: the calls this suite makes are blocking ones — a client process, a keyring
/// round trip — and an async deadline around a blocked thread never fires. libtest's capture is
/// thread-local, so what this thread prints reaches the log even while the test's own output is
/// still being held.
fn watch(home: &Home) {
    let log = home.path().join("logs").join("daemon.log");

    std::thread::spawn(move || {
        std::thread::sleep(BUDGET);

        let stage = *STAGE.lock().expect("nothing panics holding this");

        eprintln!(
            "\n--- this suite hung ---\nIt was {stage}, and it had been for less than {BUDGET:?}.             \n--- daemon.log ---\n{}",
            std::fs::read_to_string(&log)
                .unwrap_or_else(|error| format!("{} could not be read: {error}", log.display()))
        );

        // The process rather than the thread: a test blocked in a syscall cannot be unwound, and
        // the only thing left to decide is whether the job learns why in twelve minutes or in
        // thirty.
        std::process::exit(101);
    });
}

/// Where an unpacked MariaDB is, as the CI step and a developer both set it.
///
/// The directory the archive unpacks to — `bin/mariadbd` inside it — which is also what a `packages`
/// row's `install_path` is.
const PACKAGE: &str = "MIXENGINE_MARIADB_PACKAGE";

/// The version the index publishes it as, and the one `mix service create` names.
const VERSION: &str = "11.4.12";

/// The service this suite drives. **An `@`**, which is the instancing rule seen from a recipe that
/// has it: a home may hold two databases, so every one of them is named.
const SERVICE: &str = "mariadb@main";

/// The instance the credential-reset test drives, which is **not** [`SERVICE`].
///
/// A name of its own, so the two ignored tests in this file, which run at once in two homes, read
/// apart in a log. It used to be what kept their bootstraps apart as well: a Unix bootstrap named
/// its space-free view `/tmp/mixengine-init-<id>`, one view for every home bootstrapping that id,
/// and the second ritual's cleanup took the first one's basedir away. The name carries the home id
/// now.
const RESET: &str = "mariadb@reset";

/// The instance the adopt test keeps across a reinstall — a name of its own for [`RESET`]'s reason.
const KEPT: &str = "mariadb@kept";

/// The MariaDB this suite is about, or the reason there is none.
fn package() -> PathBuf {
    let directory = std::env::var_os(PACKAGE).unwrap_or_else(|| {
        panic!(
            "{PACKAGE} is not set, so there is no MariaDB to judge this recipe against. The \
             `mariadb` step in .github/workflows/_services.yml fetches one; by hand, unpack any MariaDB \
             11.4 from mixengine-packages' releases and point {PACKAGE} at the directory it \
             unpacked to."
        )
    });

    PathBuf::from(directory)
}

/// What the artifact publishes, as an index entry says it.
///
/// **Probed rather than written down**, because the layout is the publisher's and upstream renamed
/// every one of these between 10.4 and 10.6 — a suite that hard-coded either spelling would pass on
/// one series while describing the other wrongly.
fn provides(root: &Path) -> serde_json::Map<String, Value> {
    let mut found = serde_json::Map::new();

    for (name, candidates) in [
        ("mariadbd", ["bin/mariadbd", "bin/mysqld"].as_slice()),
        ("mariadb", ["bin/mariadb", "bin/mysql"].as_slice()),
        (
            "mariadb-admin",
            ["bin/mariadb-admin", "bin/mysqladmin"].as_slice(),
        ),
        (
            "mariadb-install-db",
            [
                "scripts/mariadb-install-db",
                "bin/mariadb-install-db",
                "scripts/mysql_install_db",
                "bin/mysql_install_db",
            ]
            .as_slice(),
        ),
    ] {
        for candidate in candidates {
            let relative = format!("{candidate}{}", std::env::consts::EXE_SUFFIX);

            if root.join(&relative).is_file() {
                found.insert(name.to_owned(), Value::String(relative));
                break;
            }
        }

        // Named here rather than left to the recipe: `ServiceProvidesNothing` arriving from three
        // layers down at `service start` says the same thing much later and much less clearly.
        assert!(
            found.contains_key(name),
            "{} publishes no {name}, so this suite has nothing to drive — it needs the MariaDB \
             mixengine-packages builds, not a system one",
            root.display()
        );
    }

    found
}

/// An index offering exactly this MariaDB, for this machine.
fn index(packed: &Packed, url: &str, provides: serde_json::Map<String, Value>) -> Value {
    serde_json::json!({
        "schema": 1,
        "generated_at": "2026-08-19T06:55:12Z",
        "packages": [{
            "kind": "mariadb",
            "version": VERSION,
            "channel": "stable",
            "artifacts": [{
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "url": url,
                "sha256": packed.sha256,
                "size": packed.size(),
                "provides": Value::Object(provides),
            }],
        }],
    })
}

/// Where this instance's data directory is: `data/<package>/<instance>`, because it is named.
fn data_directory(home: &Home, instance: &str) -> PathBuf {
    home.path().join("data").join("mariadb").join(instance)
}

/// `mix …` for a call that is expected to work, with the daemon's own log in the failure.
///
/// [`harness::json`] panics on a non-zero exit with only `mix`'s stderr, which for a start that
/// failed is one sentence about a service; what says *why* is `daemon.log`, and a bootstrap that
/// went wrong is the case where the difference is an afternoon.
fn expect(home: &Home, args: &[&str]) -> Value {
    let output = home.mix(args);

    assert!(
        output.status.success(),
        "`mix {}` exited {}\n--- stdout ---\n{}\n--- stderr ---\n{}\n--- daemon.log ---\n{}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        home.daemon_log()
    );

    json(&output)
}

/// What `mix service status mariadb@main` says.
fn status(home: &Home, service: &str) -> Value {
    json(&home.mix(&["service", "status", service, "--json"]))
}

/// Every `service.first_run` job this home has run.
fn first_runs(home: &Home) -> Vec<Value> {
    let listed = json(&home.mix(&["job", "list", "--json"]));

    listed["jobs"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|job| job["kind"] == "service.first_run")
        .collect()
}

/// What the server itself wrote about this start: `<home>/logs/services/<id>/mariadb.err`.
///
/// **The server's own account, and the only one there is.** Windows `mariadbd` sends nothing to
/// standard output, so a supervisor reading the process's streams finds an empty file — which is
/// why the recipe points it at a log of its own, and why this is what both the version and the
/// clean shutdown are read out of.
fn server_log(home: &Home) -> String {
    let path = home
        .path()
        .join("logs")
        .join("services")
        .join(SERVICE)
        .join("mariadb.err");

    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()))
}

/// Whether `key` is this home's keyring address for `address`.
///
/// **The whole string cannot be written down in a test.** An address begins with an id the home
/// mints for itself at its first migration — six random bytes, so twelve hex characters — which is
/// what stopped every `MIXENGINE_HOME` on a machine from sharing one entry. What a test can state
/// is the shape: this home's id, a slash, and the address as it was spelled before homes were
/// named.
///
/// Asserted rather than ignored, because the prefix is the fix: a key that is *only* the old
/// spelling is the defect ADR 0032 removed.
/// Linux-only, because its one caller is: the handoff test is the only place a client is
/// handed an address, and it is the only system where this suite can name a data directory
/// through the daemon's environment.
#[cfg(target_os = "linux")]
fn is_this_homes_address(key: &str, address: &str) -> bool {
    key.strip_suffix(address)
        .and_then(|home| home.strip_suffix('/'))
        .is_some_and(|home| home.len() == 12 && home.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Try to log in as `user` with no password, and hand back what the client said.
///
/// The shipped client rather than a Rust driver: the workspace has no MySQL protocol implementation
/// and has no reason to grow one for a test, and the client is in the archive this suite installed.
///
/// **Every connection this suite makes is one that must be refused**, and `--password=` is how: an
/// empty password rather than none at all, so the client sends credentials and is turned away
/// instead of falling back to a socket or prompting for one. See the module note for why this is
/// the shape of the assertion.
fn without_a_password(root: &Path, port: u16, user: &str) -> String {
    let client = root.join(format!("bin/mariadb{}", std::env::consts::EXE_SUFFIX));
    let client = if client.is_file() {
        client
    } else {
        root.join(format!("bin/mysql{}", std::env::consts::EXE_SUFFIX))
    };

    let refused = Command::new(client)
        .args([
            "--protocol=TCP",
            "--host=127.0.0.1",
            &format!("--port={port}"),
            &format!("--user={user}"),
            "--password=",
            "--batch",
            "--skip-column-names",
            "-e",
            "SELECT 1;",
        ])
        .output()
        .expect("the client in the archive can be run");

    // **A refusal, and not merely a failure.** Every one of these assertions is a negative, so a
    // client that could not run, could not resolve the host, or was handed an option this series
    // does not know would satisfy them all while proving nothing. The server's own `Access denied`
    // — with the account it decided the connection was for — is the only answer that means what
    // this suite needs it to mean, so it is what is returned and what the caller matches on.
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );

    assert!(
        !refused.status.success(),
        "the server let a passwordless connection in as `{user}`: {said}"
    );
    assert!(
        said.contains("Access denied"),
        "the client failed before the server could refuse it, so this proves nothing about \
         `{user}`: {said}"
    );

    said
}

/// The positive counterpart of [`without_a_password`]. A query that succeeds with exactly the
/// password this test chose *is* the proof the daemon stored and used it: nothing here ever reads
/// the keyring, because nothing needs to — the value was never MixEngine's to generate.
///
/// **And it proves a second thing: the connection was encrypted.** An 11.4 client with a password
/// on its command line verifies the server's certificate by default and refuses a server that
/// offers no TLS at all (`ERROR 2026`) — which is how this call found `skip-ssl` in the recipe. The
/// recipe now renders a leaf this home's authority signed, and `Ssl_cipher` is what separates *the
/// pair was written* from *the pair was served*: the client negotiates TLS on its own, so the only
/// way the cipher comes back empty is a server that loaded no certificate and fell back to
/// plaintext — or made its own, which is the seconds-per-start a certificate from this home's
/// authority removes.
fn with_the_password(root: &Path, port: u16, user: &str, password: &str) -> String {
    let client = root.join(format!("bin/mariadb{}", std::env::consts::EXE_SUFFIX));
    let client = if client.is_file() {
        client
    } else {
        root.join(format!("bin/mysql{}", std::env::consts::EXE_SUFFIX))
    };

    let answered = Command::new(client)
        .args([
            "--protocol=TCP",
            "--host=127.0.0.1",
            &format!("--port={port}"),
            &format!("--user={user}"),
            &format!("--password={password}"),
            "--batch",
            "--skip-column-names",
            "-e",
            "SELECT 1; SHOW STATUS LIKE 'Ssl_cipher';",
        ])
        .output()
        .expect("the client in the archive can be run");

    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&answered.stdout),
        String::from_utf8_lossy(&answered.stderr)
    );

    assert!(
        answered.status.success(),
        "the server refused the password this test chose for `{user}`: {said}"
    );

    // `--batch --skip-column-names` prints `Ssl_cipher<TAB><cipher>` on its own line; a plaintext
    // connection prints the name and nothing after the tab.
    let cipher = String::from_utf8_lossy(&answered.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Ssl_cipher\t"))
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    assert!(
        !cipher.is_empty(),
        "the login as `{user}` was not encrypted, so the certificate the recipe rendered was not \
         served: {said}"
    );

    said
}

/// The archive every test in this suite installs, packed by the first test that asks for it.
///
/// Every test packs the same [`package`] directory into the same archive, and on Windows deflating
/// a real server is the slowest thing this suite does. The tests run in parallel, so the others
/// wait on the lock rather than packing a copy each.
fn packed() -> Packed {
    static PACKED: OnceLock<Packed> = OnceLock::new();

    PACKED
        .get_or_init(|| {
            let packing = if cfg!(windows) {
                Packing::Zip
            } else {
                Packing::TarZst
            };
            FakePackage::new(packing)
                .directory(&package())
                .build(&format!("mariadb-{VERSION}"))
        })
        .clone()
}

/// A home with a real MariaDB installed in it, a service created over it, and the port it will use.
///
/// The archive is packed here out of the directory the CI step unpacked, served by a registry that
/// signs its own index, and installed through `package.install` — so this suite covers the whole
/// package install path against a real artifact on all three systems at no extra cost.
async fn created() -> (Home, harness::Daemon, MockRegistry, PathBuf, u16) {
    created_as(SERVICE).await
}

/// [`created`], for an instance of the caller's naming — see [`RESET`] for why one test needs that.
async fn created_as(service: &str) -> (Home, harness::Daemon, MockRegistry, PathBuf, u16) {
    let root = package();
    let port = free_port();

    let packed = packed();

    let registry = MockRegistry::start(&serde_json::json!({
        "schema": 1, "generated_at": "2026-08-19T06:55:12Z", "packages": []
    }))
    .await;
    let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
    registry.publish(&index(&packed, &url, provides(&root)));

    let home = Home::new();
    let daemon = home.start_daemon_reading_index(&registry.url(), registry.public_key());
    watch(&home);

    at("installing the package");
    let installed = expect(&home, &["package", "install", "mariadb", VERSION, "--json"]);
    assert_eq!(
        installed["state"],
        "succeeded",
        "{installed}\n{}",
        home.daemon_log()
    );

    at("creating the service");
    let created = expect(
        &home,
        &[
            "service",
            "create",
            service,
            VERSION,
            "--port",
            &port.to_string(),
            "--json",
        ],
    );
    assert_eq!(
        created["service"]["id"],
        service,
        "{created}\n{}",
        home.daemon_log()
    );

    // The install path this home unpacked into, which is where the client comes from.
    let installed_at = home.path().join("packages").join("mariadb").join(VERSION);

    (home, daemon, registry, installed_at, port)
}

mod first_run;
mod kept;
