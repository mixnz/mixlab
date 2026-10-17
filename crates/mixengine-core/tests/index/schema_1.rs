//! Reading through schema 1, and the limits on what a source may cost.

use super::*;

/// A home from before T196: `index.json`, its signature, and nothing else.
async fn schema_1_home(registry: &MockRegistry, cache: &Path) {
    let old: Client<mixengine_core::index::schema1::Document> =
        Client::with(&registry.url(), registry.public_key(), cache).expect("a client");

    old.catalogue().await.expect("the schema 1 document");
    assert!(cache.join("index.json").exists());
}

/// Nothing is migrated at start: the old cache answers until the set has been read, and is then
/// removed — one layout on disk at a time.
#[tokio::test]
async fn a_schema_1_cache_answers_until_the_set_is_read() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    schema_1_home(&registry, cache.path()).await;

    registry.unplug();
    age(cache.path(), "index.json", TWO_DAYS);
    let offline = client(&registry, cache.path())
        .kinds(&["php"])
        .await
        .expect("the old cache is still an index");
    assert!(offline.freshness.is_stale(), "got {:?}", offline.freshness);
    assert!(offline.index.artifact("php", "8.3.33").is_some());

    registry.plug();
    let online = client(&registry, cache.path())
        .kinds(&["php"])
        .await
        .expect("the set");
    assert_eq!(online.freshness, Freshness::Fetched);
    assert!(online.index.artifact("php", "8.3.33").is_some());
    assert!(!cache.path().join("index.json").exists());
    assert!(!cache.path().join("index.json.minisig").exists());
    assert!(cache.path().join(ROOT).exists());
}

/// The rollback mark is carried across: a home cannot be walked backwards by being upgraded.
#[tokio::test]
async fn an_upgraded_home_cannot_be_walked_backwards() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-10-01T00:00:00Z")).await;
    schema_1_home(&registry, cache.path()).await;

    registry.publish(&index_with("2026-09-01T00:00:00Z", "8.3.1"));
    age(cache.path(), "index.json", TWO_DAYS);

    let kept = client(&registry, cache.path())
        .kinds(&["php"])
        .await
        .expect("the cached index is kept");

    assert!(kept.freshness.is_stale(), "got {:?}", kept.freshness);
    assert_eq!(
        kept.index.generated_at().to_string(),
        "2026-10-01T00:00:00Z"
    );
    assert!(kept.index.artifact("php", "8.3.33").is_some());
    assert!(cache.path().join("index.json").exists());
    assert!(!cache.path().join(ROOT).exists());
}

/// A mirror that copied `index.json` and nothing else is read through it, as before T196.
#[tokio::test]
async fn a_source_with_no_schema_2_is_read_through_schema_1() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("the set, cached");
    registry.without_schema_2();
    registry.publish(&index_with("2026-10-01T00:00:00Z", "8.3.34"));
    age(cache.path(), ROOT, TWO_DAYS);

    let read = client.kinds(&["php"]).await.expect("schema 1");

    assert_eq!(read.freshness, Freshness::Fetched);
    assert!(read.index.artifact("php", "8.3.34").is_some());
    assert!(cache.path().join("index.json").exists());
    assert!(
        cached(cache.path())
            .keys()
            .all(|name| !name.starts_with("index-v2")),
        "one layout on disk at a time"
    );

    // And the mark went with it: that source cannot now offer something older.
    registry.publish(&index_with("2026-09-01T00:00:00Z", "8.3.1"));
    age(cache.path(), "index.json", TWO_DAYS);
    let kept = client.kinds(&["php"]).await.expect("kept");
    assert!(kept.freshness.is_stale());
    assert!(kept.index.artifact("php", "8.3.34").is_some());
}

/// Only "there is no such file" is a reason to fall back. A server that could not be reached is a
/// failed refresh.
#[tokio::test]
async fn an_unreachable_signature_is_not_a_reason_to_fall_back() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("cached");
    age(cache.path(), ROOT, TWO_DAYS);
    registry.unplug();
    registry.forget_requests();

    let kept = client.kinds(&["php"]).await.expect("the cache answers");

    assert!(kept.freshness.is_stale());
    assert_eq!(registry.requests(), ["/index-v2.json.minisig"]);
}

/// Each file has its own limit, and a server that never answers is given up on when it runs out —
/// not after the thirty seconds a whole request had before T196.
///
/// **A socket that accepts and never answers**, rather than a limit too short for anything: a limit
/// of one nanosecond was this test until CI's ubuntu runner answered over loopback before the timer
/// was ever looked at (run 36798571660). A connection the operating system accepts into the backlog
/// and nobody reads is a request that cannot succeed on any machine.
#[tokio::test]
async fn a_file_is_given_up_on_after_its_own_limit() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let silent = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("bind a loopback port that nobody will read");
    let url = format!(
        "http://{}/index.json",
        silent.local_addr().expect("the bound port")
    );

    let started = std::time::Instant::now();
    let refusal = PackageIndex::with(&url, registry.public_key(), cache.path())
        .expect("a client")
        .with_timeout(Duration::from_millis(200))
        .kinds(&["php"])
        .await
        .expect_err("a server that never answers is not an index");
    let took = started.elapsed();

    assert!(
        matches!(
            refusal,
            mixengine_core::Error::Install(InstallError::IndexTransport { .. })
        ),
        "expected a transport failure, got {refusal:?}"
    );
    assert!(
        took < Duration::from_secs(10),
        "the file's own limit ended it, not the transport's thirty seconds: took {took:?}"
    );
    drop(silent);
}

/// Falling back to schema 1 removes the schema 2 set, and only the set: a file somebody else put in
/// the cache is theirs, by the same rule `daemon.cleanup` keeps.
#[tokio::test]
async fn a_source_with_no_schema_2_leaves_files_that_are_not_the_index_alone() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("the set, cached");
    let theirs = cache.path().join("index-v2-notes.txt");
    std::fs::write(&theirs, b"somebody's notes").expect("a file of somebody else's");

    registry.without_schema_2();
    registry.publish(&index_with("2026-10-01T00:00:00Z", "8.3.34"));
    age(cache.path(), ROOT, TWO_DAYS);
    client.kinds(&["php"]).await.expect("schema 1");

    assert!(!cache.path().join(ROOT).exists(), "the set went");
    assert!(theirs.exists(), "and what was not the set stayed");
}

/// A kind that verified and could not be stored is still an answer, and the write that failed
/// leaves nothing half done behind it. A directory where the file should be makes the rename fail
/// on every system.
#[tokio::test]
async fn a_kind_that_cannot_be_stored_is_answered_and_leaves_nothing_half_written() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["node"]).await.expect("the root, cached");
    std::fs::create_dir(cache.path().join("index-v2-php.json")).expect("a directory in the way");

    let read = client.kinds(&["php"]).await.expect("an answer");

    assert!(read.index.artifact("php", "8.3.33").is_some());
    assert!(read.index.missing().is_empty());
    let left: Vec<String> = std::fs::read_dir(cache.path())
        .expect("a cache directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".part"))
        .collect();
    assert!(left.is_empty(), "a failed write left {left:?}");
}

/// A network that failed a moment ago is not asked again at once: the calls queued behind a slow
/// failure are answered from the cache instead of each waiting out a timeout of their own. A
/// person asking for a refresh is asked on behalf of, whatever happened a moment ago.
#[tokio::test]
async fn an_unreachable_index_is_not_asked_again_at_once() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("cached");
    age(cache.path(), ROOT, TWO_DAYS);
    registry.unplug();

    registry.forget_requests();
    let first = client.kinds(&["php"]).await.expect("the cache answers");
    assert!(first.freshness.is_stale());
    assert_eq!(registry.requests(), ["/index-v2.json.minisig"]);

    registry.forget_requests();
    let second = client.kinds(&["php"]).await.expect("the cache answers");
    assert!(second.freshness.is_stale());
    assert!(second.index.artifact("php", "8.3.33").is_some());
    assert!(
        registry.requests().is_empty(),
        "asked again straight after it failed"
    );

    registry.forget_requests();
    let forced = client.refresh(&["php"]).await.expect("the cache answers");
    assert!(forced.freshness.is_stale());
    assert_eq!(registry.requests(), ["/index-v2.json.minisig"]);
}
