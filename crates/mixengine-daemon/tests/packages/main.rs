//! `package.*` against a real `mixengined`, a real signed index and a real archive.
//!
//! [`tests/runtimes.rs`](runtimes.rs)' shape one namespace across: a [`MockRegistry`] serves a
//! document it signs over a loopback socket, the daemon is pointed at both with `--index-url` and
//! `--index-key`, and nothing here touches the network.
//!
//! **What is published is `fakeservice`**, because a debug build has a recipe for it and the archive
//! can be a real executable that needs no server behind it. The index also publishes a
//! [`UNRUNNABLE`], which this build has no recipe for — that is what makes the catalogue filter mean
//! something, since a fixture offering only what is runnable could not tell a filter that works from
//! one that does nothing.

use std::time::Duration;

use mixengine_testkit::{Client, Daemon, FakePackage, Home, MockRegistry, Packed, Packing};
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
/// `redis` until Redis got a recipe and turned all three tests below red — which was the fixture
/// being right about a fact that had an expiry date on it. Every real server the index publishes is
/// one the catalogue is *meant* to grow a recipe for, so naming any of them here is the same clock
/// set for a later day. What the rule is actually about outlives all of them: an index is published
/// on its own schedule and a user's build is routinely older than it, so a package this build has
/// never heard of is the ordinary case and not the contrived one.
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
        let daemon = started(&home, &registry);
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

/// A daemon for `home`, reading its index from `registry`.
fn started(home: &Home, registry: &MockRegistry) -> Daemon {
    let mut command = Daemon::command(env!("CARGO_BIN_EXE_mixengined"), home);
    command
        .arg("--index-url")
        .arg(registry.url())
        .arg("--index-key")
        .arg(registry.public_key());
    Daemon::spawn(command)
}

/// What this suite asks of a client beyond a call.
trait Jobs {
    /// Install a version and wait for the job it produced to end.
    async fn install(&mut self, version: &str) -> Value;
    /// Wait for a job to end, and answer with it as it ended.
    async fn finished(&mut self, job: Value) -> Value;
}

impl Jobs for Client {
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

mod markers;
mod services;
mod upgrades;
