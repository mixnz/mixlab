//! `extension.available` against a real `mixengined` and a real signed registry.
//!
//! [`tests/runtimes.rs`](runtimes.rs)'s shape, for the second document: [`MockRegistry`] serves
//! `extensions.json` beside `index.json` under one key, and the daemon is pointed at the index URL
//! alone — `mixengine_daemon::runtimes::IndexSource::registry_url` derives the second document's URL
//! from the first, so pointing the daemon at one points it at both.

use std::path::Path;

use mixengine_testkit::{Client, Daemon, Home, MockRegistry};
use serde_json::{Value, json};

/// A registry document holding `entries`, generated at `generated_at` — the shape
/// `mixengine_core::extensions::registry::Registry` reads.
fn registry_at(generated_at: &str, entries: Vec<Value>) -> Value {
    json!({
        "schema": 1,
        "generated_at": generated_at,
        "extensions": entries,
    })
}

/// One published entry: a fixture manifest, in the shape the document carries it.
fn entry(text: &str) -> Value {
    let manifest = mixengine_core::extensions::manifest::read(Path::new("extension.toml"), text)
        .expect("a fixture parses");

    mixengine_core::extensions::manifest::to_value(&manifest)
}

/// A registry serving one extension, a home, and a daemon pointed at it.
struct Fixture {
    home: Home,
    /// Held rather than read: dropping it would stop the server the daemon reads the registry from.
    _registry: MockRegistry,
    _daemon: Daemon,
}

impl Fixture {
    async fn start() -> Self {
        let registry = MockRegistry::start(&json!({
            "schema": 1,
            "generated_at": "2026-09-02T09:00:00Z",
            "packages": [],
        }))
        .await;
        registry.publish_extensions(&registry_at(
            "2026-09-02T09:00:00Z",
            vec![entry(mixengine_testkit::extension::MAILPIT)],
        ));

        let home = Home::new();
        let mut command = Daemon::command(env!("CARGO_BIN_EXE_mixengined"), &home);
        command
            .arg("--index-url")
            .arg(registry.url())
            .arg("--index-key")
            .arg(registry.public_key());
        let daemon = Daemon::spawn(command);
        home.wait_until_listening().await;

        Self {
            home,
            _registry: registry,
            _daemon: daemon,
        }
    }

    async fn client(&self) -> Client {
        Client::connect(&self.home).await
    }
}

/// The ordinary path: what the registry publishes is what `extension.available` answers.
#[tokio::test]
async fn what_the_registry_publishes_is_offered() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let available = client.call("extension.available", json!({})).await;
    let offered = &available["extensions"];
    assert_eq!(offered.as_array().map(Vec::len), Some(1), "{available}");
    assert_eq!(offered[0]["id"], "mailpit");
    assert_eq!(available["unreadable"], 0);
    assert_eq!(available["stale"], false);
}

/// **`refresh` reaches the registry instead of answering from a still-fresh cache.**
///
/// [`tests/runtimes.rs`](runtimes.rs)'s test of the same name, for `extension.available` — its own
/// `mixengine_core::index::Client<Registry>`, separate from the package index's, and separately
/// cached (`.claude`'s note beside `Registry::CACHE_FILE`: two documents must not share one file).
#[tokio::test]
async fn refresh_bypasses_a_fresh_cache() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    // Fetches and caches the registry the fixture published.
    let first = client.call("extension.available", json!({})).await;
    assert_eq!(
        first["extensions"].as_array().map(Vec::len),
        Some(1),
        "{first}"
    );

    // Republished with nothing listed. The cache is still fresh, so an ordinary call keeps
    // answering from it — the behaviour `refresh` exists to bypass.
    fixture
        ._registry
        .publish_extensions(&registry_at("2026-09-02T09:00:01Z", Vec::new()));

    let cached = client.call("extension.available", json!({})).await;
    assert_eq!(
        cached["extensions"].as_array().map(Vec::len),
        Some(1),
        "a fresh cache is not asked about again: {cached}"
    );

    let refreshed = client
        .call("extension.available", json!({"refresh": true}))
        .await;
    assert_eq!(
        refreshed["extensions"].as_array().map(Vec::len),
        Some(0),
        "`refresh` reaches the registry instead of answering from the cache: {refreshed}"
    );
}
