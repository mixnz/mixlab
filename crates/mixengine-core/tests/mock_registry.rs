//! What `MockRegistry` serves as index schema 2 — roadmap task **T196**.
//!
//! The registry is the other half of every test in `tests/index.rs`, and it encodes the set with
//! code of its own rather than with this crate's. So these hold the fixture to the format before
//! anything holds the client to the fixture: a root that names each kind file by hash, an artifact
//! reachable at the address schema 2 composes for it, and each state a real publish passes through.
//!
//! They live here rather than beside the registry because `mixengine-testkit` has no HTTP client
//! and gets none for the sake of four tests.

use mixengine_testkit::MockRegistry;
use sha2::Digest as _;

fn index(origin: &str) -> serde_json::Value {
    serde_json::json!({
        "schema": 1, "generated_at": "2026-09-30T00:00:00Z",
        "packages": [
            { "kind": "php", "version": "8.4.1", "channel": "stable", "artifacts": [
                { "os": "linux", "arch": "x86_64",
                  "url": format!("{origin}/php-8.4.1.tar.zst"),
                  "sha256": "aa", "size": 3, "provides": { "php": "bin/php" } },
                { "os": "linux", "arch": "aarch64",
                  "url": "https://example.invalid/php.tar.zst",
                  "sha256": "bb", "size": 3, "provides": { "php": "bin/php" } } ] },
            { "kind": "node", "version": "22.1.0", "channel": "stable", "artifacts": [
                { "os": "windows", "arch": "x86_64",
                  "url": "https://example.invalid/node.zip",
                  "sha256": "cc", "size": 3, "provides": { "node": "node.exe" } } ] }
        ]
    })
}

fn origin(registry: &MockRegistry) -> String {
    registry.url().trim_end_matches("/index.json").to_owned()
}

async fn body(registry: &MockRegistry, path: &str) -> (u16, Vec<u8>) {
    let response = reqwest::get(format!("{}{path}", origin(registry)))
        .await
        .expect("the registry answers");

    (
        response.status().as_u16(),
        response.bytes().await.expect("a body").to_vec(),
    )
}

#[tokio::test]
async fn the_set_is_a_root_naming_each_kind_file_by_hash() {
    let registry = MockRegistry::start(&serde_json::json!({"schema": 1})).await;
    let origin = origin(&registry);
    registry.publish_asset("/php-8.4.1.tar.zst", b"php".to_vec());
    registry.publish(&index(&origin));

    let (status, root) = body(&registry, "/index-v2.json").await;
    assert_eq!(status, 200);
    let root: serde_json::Value = serde_json::from_slice(&root).expect("the root is JSON");
    assert_eq!(root["schema"], 2);
    assert_eq!(root["generated_at"], "2026-09-30T00:00:00Z");
    assert_eq!(root["base_url"], format!("{origin}/assets"));

    let (_, php) = body(&registry, "/index-v2-php.json").await;
    assert_eq!(root["kinds"]["php"]["size"], php.len());
    assert_eq!(
        root["kinds"]["php"]["sha256"],
        format!("{:x}", sha2::Sha256::digest(&php))
    );

    let php: serde_json::Value = serde_json::from_slice(&php).expect("a kind file is JSON");
    assert_eq!(php["kind"], "php");
    assert_eq!(
        php["shapes"].as_array().map(Vec::len),
        Some(1),
        "two cells that say the same thing share one shape"
    );
    let artifact = &php["packages"][0]["artifacts"][0];
    assert!(artifact.get("url").is_none(), "schema 2 states no URL");
    assert_eq!(artifact["format"], "tar.zst");
    assert!(php["packages"][0].get("kind").is_none());

    // What was published at the fixture's own path answers at the one schema 2 composes.
    let (status, asset) = body(
        &registry,
        "/assets/php-8.4.1/php-8.4.1-linux-x86_64.tar.zst",
    )
    .await;
    assert_eq!((status, asset.as_slice()), (200, b"php".as_slice()));

    assert_eq!(body(&registry, "/index-v2.json.minisig").await.0, 200);
    assert_eq!(body(&registry, "/index-v2-node.json").await.0, 200);
}

/// Some suites start a registry only for its assets, with `{"schema": 1}` and nothing else.
#[tokio::test]
async fn a_value_that_is_not_an_index_gets_no_set() {
    let registry = MockRegistry::start(&serde_json::json!({"schema": 1})).await;

    assert_eq!(body(&registry, "/index-v2.json.minisig").await.0, 404);
    assert_eq!(body(&registry, "/index-v2.json").await.0, 404);
    assert_eq!(body(&registry, "/index.json").await.0, 200);
}

#[tokio::test]
async fn the_states_in_between_are_each_one_call() {
    let registry = MockRegistry::start(&index("https://example.invalid")).await;
    let (_, whole) = body(&registry, "/index-v2-php.json").await;

    registry.corrupt_kind("php");
    let (_, corrupt) = body(&registry, "/index-v2-php.json").await;
    assert_eq!(corrupt.len(), whole.len(), "corrupt, not truncated");
    assert_ne!(corrupt, whole);

    registry.publish(&index("https://example.invalid"));
    assert_eq!(body(&registry, "/index-v2-php.json").await.1, whole);
    registry.truncate_kind("php");
    assert_eq!(
        body(&registry, "/index-v2-php.json").await.1.len(),
        whole.len() - 1
    );

    registry.withhold_kind("node");
    assert_eq!(body(&registry, "/index-v2-node.json").await.0, 404);

    registry.forget_requests();
    registry.without_schema_2();
    assert_eq!(body(&registry, "/index-v2.json.minisig").await.0, 404);
    assert_eq!(body(&registry, "/index.json").await.0, 200);
    assert_eq!(
        registry.requests(),
        ["/index-v2.json.minisig", "/index.json"]
    );
}

#[tokio::test]
async fn a_signature_can_be_published_ahead_of_its_root_and_a_root_ahead_of_its_signature() {
    let registry = MockRegistry::start(&index("https://example.invalid")).await;
    let (_, signature) = body(&registry, "/index-v2.json.minisig").await;
    let (_, root) = body(&registry, "/index-v2.json").await;

    let mut later = index("https://example.invalid");
    later["generated_at"] = serde_json::json!("2026-10-01T00:00:00Z");

    registry.publish_signature_only(&later);
    let (_, ahead) = body(&registry, "/index-v2.json.minisig").await;
    assert_ne!(ahead, signature);
    assert_eq!(body(&registry, "/index-v2.json").await.1, root);

    registry.publish(&index("https://example.invalid"));
    let (_, signature) = body(&registry, "/index-v2.json.minisig").await;
    registry.publish_unsigned(&later);
    assert_eq!(body(&registry, "/index-v2.json.minisig").await.1, signature);
    assert_ne!(body(&registry, "/index-v2.json").await.1, root);
}

#[tokio::test]
async fn a_root_can_be_published_as_it_is_written() {
    let registry = MockRegistry::start(&index("https://example.invalid")).await;

    registry.publish_root(&serde_json::json!({ "schema": 3, "kinds": {} }));

    let (_, root) = body(&registry, "/index-v2.json").await;
    let root: serde_json::Value = serde_json::from_slice(&root).expect("JSON");
    assert_eq!(root["schema"], 3);
    assert_eq!(
        body(&registry, "/index-v2-php.json").await.0,
        200,
        "the kind files stay"
    );
}
