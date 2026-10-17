//! Index schema 2: the signature, the root and one file per kind.

use super::*;

// --- Index schema 2: the contract `mixengine-packages` states, kept — roadmap task **T196**. ---
//
// What is asserted on is, as often as not, *what was asked for*: a client that downloaded the whole
// index and answered correctly looks exactly like one that asked for a signature and stopped.

/// A home with nothing cached asks for the signature, the root, and the kinds it was asked for.
#[tokio::test]
async fn a_fresh_home_asks_for_the_signature_the_root_and_only_its_kinds() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;

    let read = client(&registry, cache.path())
        .kinds(&["php"])
        .await
        .expect("the index is readable");

    assert_eq!(read.freshness, Freshness::Fetched);
    assert!(read.index.artifact("php", "8.3.33").is_some());
    assert_eq!(
        registry.requests(),
        [
            "/index-v2.json.minisig",
            "/index-v2.json",
            "/index-v2-php.json"
        ],
        "node is published and nobody asked for it"
    );
}

/// Step 1: a signature that is byte for byte the cached one means nothing was published.
#[tokio::test]
async fn nothing_published_costs_one_signature() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("first fetch");
    age(cache.path(), ROOT, TWO_DAYS);
    registry.forget_requests();

    let again = client.kinds(&["php"]).await.expect("second");
    assert_eq!(
        again.freshness,
        Freshness::Fetched,
        "the cache counts as fetched now"
    );
    assert_eq!(registry.requests(), ["/index-v2.json.minisig"]);

    registry.forget_requests();
    let third = client.kinds(&["php"]).await.expect("third");
    assert!(
        matches!(third.freshness, Freshness::Cached { .. }),
        "the six hours started again, got {:?}",
        third.freshness
    );
    assert!(registry.requests().is_empty());
}

/// Step 3: a release of one kind leaves every other kind's file — and this machine's copy of it —
/// untouched.
#[tokio::test]
async fn a_release_of_one_kind_fetches_that_kind_and_no_other() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php", "node"]).await.expect("both cached");
    registry.publish(&index_with("2026-10-01T00:00:00Z", "8.3.34"));
    age(cache.path(), ROOT, TWO_DAYS);
    registry.forget_requests();

    let read = client.kinds(&["php", "node"]).await.expect("refreshed");

    assert_eq!(read.freshness, Freshness::Fetched);
    assert!(read.index.artifact("php", "8.3.34").is_some());
    assert!(read.index.artifact("node", "22.1.0").is_some());
    assert_eq!(
        registry.requests(),
        [
            "/index-v2.json.minisig",
            "/index-v2.json",
            "/index-v2-php.json"
        ]
    );
}

/// Steps 3 and 4: one kind file that does not check out keeps everything, byte for byte.
#[tokio::test]
async fn a_refresh_that_cannot_finish_keeps_the_old_set() {
    type Break = fn(&MockRegistry, &str);
    let ways: [(&str, Break); 3] = [
        ("a file that does not hash", MockRegistry::corrupt_kind),
        ("a file that is short", MockRegistry::truncate_kind),
        ("a file that is not there", MockRegistry::withhold_kind),
    ];

    for (what, break_it) in ways {
        let cache = tempfile::tempdir().expect("a cache directory");
        let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
        let client = client(&registry, cache.path());

        client.kinds(&["php", "node"]).await.expect("both cached");
        let before = cached(cache.path());

        registry.publish(&index_with("2026-10-01T00:00:00Z", "8.3.34"));
        break_it(&registry, "php");
        age(cache.path(), ROOT, TWO_DAYS);

        let kept = client
            .kinds(&["php", "node"])
            .await
            .expect("the old set answers");

        assert!(
            kept.freshness.is_stale(),
            "{what}: the refusal has to be visible, got {:?}",
            kept.freshness
        );
        assert!(kept.index.artifact("php", "8.3.33").is_some(), "{what}");
        assert!(kept.index.missing().is_empty(), "{what}");
        assert_eq!(
            kept.index.generated_at().to_string(),
            "2026-09-30T00:00:00Z",
            "{what}: the new root must not be the one in use"
        );
        assert_eq!(
            cached(cache.path()),
            before,
            "{what}: nothing on disk may have moved"
        );
    }
}

/// A signature newer than the root beside it: the root does not verify, and nothing changes.
#[tokio::test]
async fn a_signature_ahead_of_its_root_changes_nothing() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("cached");
    let before = cached(cache.path());
    registry.publish_signature_only(&index_with("2026-10-01T00:00:00Z", "8.3.34"));
    age(cache.path(), ROOT, TWO_DAYS);

    let kept = client.kinds(&["php"]).await.expect("the old set answers");

    assert!(kept.freshness.is_stale(), "got {:?}", kept.freshness);
    assert!(kept.index.artifact("php", "8.3.33").is_some());
    assert_eq!(cached(cache.path()), before);
}

/// The window a real publish has: the root is uploaded and its signature is not yet. The signature
/// is still the cached one, so as far as a client can tell nothing was published — and it asks for
/// nothing else.
#[tokio::test]
async fn a_root_ahead_of_its_signature_is_not_looked_at() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("cached");
    registry.publish_unsigned(&index_with("2026-10-01T00:00:00Z", "8.3.34"));
    age(cache.path(), ROOT, TWO_DAYS);
    registry.forget_requests();

    let read = client.kinds(&["php"]).await.expect("an answer");

    assert!(read.index.artifact("php", "8.3.33").is_some());
    assert!(read.index.artifact("php", "8.3.34").is_none());
    assert_eq!(registry.requests(), ["/index-v2.json.minisig"]);
}

/// Step 5: a kind that is wrong costs that kind and no other, is not asked for again on every call,
/// and is asked for again when somebody says refresh.
#[tokio::test]
async fn a_kind_on_first_use_that_is_wrong_is_missing_and_the_others_answer() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client
        .kinds(&["node"])
        .await
        .expect("node cached, root fresh");
    registry.corrupt_kind("php");
    registry.forget_requests();

    let read = client.kinds(&["php", "node"]).await.expect("an answer");

    assert!(read.index.artifact("node", "22.1.0").is_some());
    assert_eq!(read.index.installable("php").count(), 0);
    let missing = read.index.unread("php").expect("php is named as unread");
    assert!(
        missing.reason.contains("does not hash"),
        "the reason is the refusal's own sentence: {}",
        missing.reason
    );
    assert_eq!(
        registry.requests(),
        ["/index-v2-php.json", "/index-v2.json.minisig"],
        "one try, one refresh that found the signature unchanged, and no second try"
    );

    // Remembered against this root: asking again costs nothing.
    registry.forget_requests();
    let again = client.kinds(&["php"]).await.expect("an answer");
    assert!(again.index.unread("php").is_some());
    assert!(registry.requests().is_empty());

    // Until somebody asks for a refresh, which finds the file repaired.
    registry.publish(&index_at("2026-10-01T00:00:00Z"));
    let repaired = client.refresh(&["php"]).await.expect("refreshed");
    assert!(repaired.index.unread("php").is_none());
    assert!(repaired.index.artifact("php", "8.3.33").is_some());
}

/// Kinds asked for the first time are fetched at once, not one after another: a cold Packages
/// screen asks for every kind there is, behind the one lock every listing takes, and one round trip
/// per kind was seconds of Loading.
#[tokio::test]
async fn kinds_on_first_use_are_fetched_at_once() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    registry.slow_kinds(Duration::from_millis(200));

    let read = client(&registry, cache.path())
        .kinds(&["php", "node"])
        .await
        .expect("the index is readable");

    assert!(read.index.artifact("php", "8.3.33").is_some());
    assert!(read.index.artifact("node", "22.1.0").is_some());
    assert_eq!(
        registry.most_kinds_in_flight(),
        2,
        "php and node were fetched one after another; requests: {:?}",
        registry.requests()
    );
}

/// Step 5, the other half: a kind that does not match the cached root means the root is behind.
#[tokio::test]
async fn a_kind_behind_a_stale_root_is_found_after_one_refresh() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client
        .kinds(&["node"])
        .await
        .expect("node cached, root fresh");
    registry.publish(&index_with("2026-10-01T00:00:00Z", "8.3.34"));
    registry.forget_requests();

    let read = client.kinds(&["php"]).await.expect("an answer");

    assert!(read.index.artifact("php", "8.3.34").is_some());
    assert_eq!(read.freshness, Freshness::Fetched);
    assert_eq!(
        read.index.generated_at().to_string(),
        "2026-10-01T00:00:00Z"
    );
    assert_eq!(
        registry.requests(),
        [
            "/index-v2-php.json",
            "/index-v2.json.minisig",
            "/index-v2.json",
            "/index-v2-php.json"
        ]
    );
}

/// The cache is an ordinary file in the user's home: a kind file is hashed against the root every
/// time it is read from disk, and one that does not match is simply not there.
#[tokio::test]
async fn a_kind_file_somebody_rewrote_is_fetched_again() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("cached");
    std::fs::write(cache.path().join("index-v2-php.json"), b"{}").expect("rewrite the cache");
    registry.forget_requests();

    let read = client.kinds(&["php"]).await.expect("an answer");

    assert!(read.index.artifact("php", "8.3.33").is_some());
    assert!(read.index.missing().is_empty());
    assert_eq!(registry.requests(), ["/index-v2-php.json"]);
}

/// What was parsed is kept: a second call asks nobody, and needs nobody.
#[tokio::test]
async fn what_was_parsed_is_kept_until_the_file_changes() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("cached");
    registry.unplug();
    registry.forget_requests();

    for _ in 0..3 {
        let read = client.kinds(&["php"]).await.expect("from memory");
        assert!(read.index.artifact("php", "8.3.33").is_some());
    }
    assert!(registry.requests().is_empty());
}

/// `daemon.cleanup` removes the cached documents while the daemon still holds them in memory.
/// Memory describes files; with the files gone it describes nothing.
#[tokio::test]
async fn a_cache_that_was_emptied_is_fetched_again() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("cached");
    for name in cached(cache.path()).keys() {
        std::fs::remove_file(cache.path().join(name)).expect("empty the cache");
    }
    registry.forget_requests();

    let read = client.kinds(&["php"]).await.expect("fetched again");

    assert_eq!(read.freshness, Freshness::Fetched);
    assert_eq!(
        registry.requests(),
        [
            "/index-v2.json.minisig",
            "/index-v2.json",
            "/index-v2-php.json"
        ]
    );
}

/// A recipe with no package, a fixture no index publishes: a kind the root does not name does not
/// exist, and finding that out costs nothing.
#[tokio::test]
async fn a_kind_the_root_does_not_name_is_empty_and_costs_nothing() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("cached");
    registry.forget_requests();

    let read = client.kinds(&["fakeservice"]).await.expect("an answer");

    assert_eq!(read.index.installable("fakeservice").count(), 0);
    assert!(read.index.missing().is_empty());
    assert!(registry.requests().is_empty());
}

/// A kind name out of a document becomes a file name and a URL. One that is not a name becomes
/// neither.
#[tokio::test]
async fn a_kind_name_that_is_not_a_name_is_skipped() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    registry.publish_root(&serde_json::json!({
        "schema": 2, "generated_at": "2026-09-30T00:00:00Z",
        "base_url": "https://example.invalid",
        "kinds": { "../evil": { "sha256": "00", "size": 1 } }
    }));

    let read = client(&registry, cache.path())
        .all()
        .await
        .expect("an answer");

    assert_eq!(read.index.packages().count(), 0);
    assert_eq!(
        registry.requests(),
        ["/index-v2.json.minisig", "/index-v2.json"],
        "no request was made for it"
    );
    assert_eq!(
        cached(cache.path()).keys().collect::<Vec<_>>(),
        ["index-v2.json", "index-v2.json.minisig"],
        "and no file was written for it"
    );
}

/// A root stamped in the future is a clock that moved, not a fresh file.
#[tokio::test]
async fn a_cache_stamped_in_the_future_is_not_fresh() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("cached");
    std::fs::File::options()
        .write(true)
        .open(cache.path().join(ROOT))
        .expect("the root was cached")
        .set_modified(SystemTime::now() + TWO_DAYS)
        .expect("set the mtime");
    registry.forget_requests();

    client.kinds(&["php"]).await.expect("an answer");

    assert_eq!(
        registry.requests(),
        ["/index-v2.json.minisig"],
        "the network decides, not the clock"
    );
}

/// A cache directory that cannot be written: the refresh cannot be committed, so it did not
/// happen, and the old set answers.
#[cfg(unix)]
#[tokio::test]
async fn a_commit_that_cannot_write_keeps_the_old_set() {
    use std::os::unix::fs::PermissionsExt as _;

    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("cached");
    let before = cached(cache.path());
    registry.publish(&index_with("2026-10-01T00:00:00Z", "8.3.34"));
    age(cache.path(), ROOT, TWO_DAYS);
    std::fs::set_permissions(cache.path(), std::fs::Permissions::from_mode(0o555))
        .expect("make the cache read-only");

    let kept = client.kinds(&["php"]).await;

    std::fs::set_permissions(cache.path(), std::fs::Permissions::from_mode(0o755))
        .expect("make the cache writable again");
    let kept = kept.expect("the old set answers");
    assert!(kept.freshness.is_stale(), "got {:?}", kept.freshness);
    assert!(kept.index.artifact("php", "8.3.33").is_some());
    assert_eq!(cached(cache.path()), before);
}

/// A root this build cannot read is refused by its number, not field by field.
#[tokio::test]
async fn a_root_this_build_cannot_read_is_refused() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-08-14T06:55:12Z")).await;
    registry.publish_root(&serde_json::json!({
        "schema": 3, "generated_at": "2026-08-14T06:55:12Z",
        "base_url": "https://example.invalid", "kinds": {}
    }));

    let refusal = client(&registry, cache.path())
        .kinds(&["php"])
        .await
        .expect_err("a newer document version is not readable");

    assert!(
        matches!(
            refusal,
            mixengine_core::Error::Install(InstallError::IndexSchema {
                found: 3,
                expected: 2,
                ..
            })
        ),
        "expected a schema refusal, got {refusal:?}"
    );
}
