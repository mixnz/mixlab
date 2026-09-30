//! The index client against a real signed index over a real socket.
//!
//! Everything here goes through [`mixengine_testkit::MockRegistry`], which generates its own keypair
//! and signs with `minisign` — the signing half of the crate pair whose verifying half the client
//! links. So these assert that the client accepts what minisign actually produces rather than what
//! we believe it produces, which matters because the format has a legacy variant the client refuses
//! on purpose and a hand-built fixture would have hidden the difference.

use std::path::Path;
use std::time::{Duration, SystemTime};

use mixengine_core::generate::Catalogue;
use mixengine_core::generate::recipes::php_fpm;
use mixengine_core::index::{Arch, Client, Freshness, Os, PackageIndex, TARGETS};
use mixengine_proto::{Execution, RuntimeKind};
use mixengine_testkit::MockRegistry;

/// The root of the schema 2 set, as it is cached. Its modification time is the age of the index.
const ROOT: &str = "index-v2.json";

const TWO_DAYS: Duration = Duration::from_secs(48 * 60 * 60);

/// One version of one kind, with an artifact for every platform CI runs on, so `artifact()`
/// answers wherever this test is executed rather than only on the machine it was written on.
fn package(kind: &str, version: &str, binary: &str) -> serde_json::Value {
    let artifact = |os: &str, arch: &str| {
        serde_json::json!({
            "os": os, "arch": arch,
            "url": format!("https://example.invalid/{kind}-{version}-{os}-{arch}.zip"),
            "sha256": "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
            "size": 34_718_139u64,
            "provides": { kind: binary }
        })
    };

    serde_json::json!({
        "kind": kind,
        "version": version,
        "channel": "stable",
        "eol": "2027-12-31",
        "artifacts": [
            artifact("windows", "x86_64"),
            artifact("macos", "aarch64"),
            artifact("linux", "x86_64"),
            artifact("linux", "aarch64"),
        ]
    })
}

/// PHP at `php` and one Node.js, generated at `generated_at`. Two kinds, because most of what
/// schema 2 promises is about the kind that was *not* touched.
fn index_with(generated_at: &str, php: &str) -> serde_json::Value {
    serde_json::json!({
        "schema": 1,
        "generated_at": generated_at,
        "packages": [package("php", php, "bin/php"), package("node", "22.1.0", "bin/node")]
    })
}

fn index_at(generated_at: &str) -> serde_json::Value {
    index_with(generated_at, "8.3.33")
}

/// Move a cached file's mtime backwards.
///
/// The freshness window is six hours and a test cannot wait one, so the clock is not what moves —
/// the file is. That also exercises the real reading: the client takes the age from the root's
/// mtime rather than from a sidecar of its own.
fn age(cache: &Path, file: &str, by: Duration) {
    let file = std::fs::File::options()
        .write(true)
        .open(cache.join(file))
        .expect("the cache was written");
    file.set_modified(SystemTime::now() - by)
        .expect("set the cache mtime");
}

fn client(registry: &MockRegistry, cache: &Path) -> PackageIndex {
    PackageIndex::with(&registry.url(), registry.public_key(), cache).expect("build a client")
}

/// Everything the index has cached, by file name.
fn cached(cache: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(cache)
        .expect("a cache directory")
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("index")
                .then(|| (name, std::fs::read(entry.path()).expect("a readable file")))
        })
        .collect()
}

#[tokio::test]
async fn a_signed_index_is_fetched_verified_and_read() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-08-14T06:55:12Z")).await;

    let catalogue = client(&registry, cache.path())
        .kinds(&["php"])
        .await
        .expect("the index is readable");

    assert_eq!(catalogue.freshness, Freshness::Fetched);
    let chosen = catalogue
        .index
        .artifact("php", "8.3.33")
        .expect("an artifact for the platform this test runs on");
    assert_eq!(chosen.artifact.size, 34_718_139);
    assert!(chosen.artifact.provides.contains_key("php"));

    // Every platform `test` runs on has its own artifact in the fixture above, so nothing here is
    // reached by emulation — which is the reading on five of the six targets and the one this
    // assertion pins, so that a change to the preference order shows up as a failure here too.
    assert_eq!(chosen.execution, mixengine_proto::Execution::Native);
}

/// A client built against a transport the caller already had reads exactly what one that built its
/// own would have — roadmap task **T72b**, [`Client::with_transport`]'s reason for existing.
#[tokio::test]
async fn a_client_built_from_a_shared_transport_reads_the_same_document() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-08-14T06:55:12Z")).await;

    let transport =
        mixengine_core::index::default_transport().expect("an HTTP client can be built");

    let catalogue = PackageIndex::with_transport(
        &registry.url(),
        registry.public_key(),
        cache.path(),
        transport,
    )
    .expect("build a client from a shared transport")
    .kinds(&["php"])
    .await
    .expect("the index is readable");

    assert_eq!(catalogue.freshness, Freshness::Fetched);
    assert!(catalogue.index.artifact("php", "8.3.33").is_some());
}

#[tokio::test]
async fn a_document_the_signature_does_not_cover_is_refused() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-08-14T06:55:12Z")).await;

    // The one move a real attacker gets against a client that checks nothing: change the bytes and
    // leave the old signature in place.
    registry.publish_unsigned(&index_at("2026-09-01T00:00:00Z"));

    let refusal = client(&registry, cache.path())
        .kinds(&["php"])
        .await
        .expect_err("a document nobody signed is not an index");
    assert!(
        matches!(refusal, mixengine_core::Error::IndexSignature { .. }),
        "expected a signature refusal, got {refusal:?}"
    );
}

#[tokio::test]
async fn an_index_signed_by_somebody_else_is_refused() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let ours = MockRegistry::start(&index_at("2026-08-14T06:55:12Z")).await;
    let theirs = MockRegistry::start(&index_at("2026-08-14T06:55:12Z")).await;

    // Perfectly valid, and signed by the wrong key — a mirror serving somebody else's index.
    let client =
        PackageIndex::with(&theirs.url(), ours.public_key(), cache.path()).expect("a client");

    let refusal = client
        .kinds(&["php"])
        .await
        .expect_err("another key is not this build's key");
    assert!(
        matches!(refusal, mixengine_core::Error::IndexSignature { .. }),
        "expected a signature refusal, got {refusal:?}"
    );
}

#[tokio::test]
async fn a_fresh_cache_is_used_without_asking_the_network() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-08-14T06:55:12Z")).await;
    let client = client(&registry, cache.path());

    assert_eq!(
        client.kinds(&["php"]).await.expect("first fetch").freshness,
        Freshness::Fetched
    );

    // If the second call went to the network it would now fail, so answering at all is the proof.
    registry.unplug();

    let second = client.kinds(&["php"]).await.expect("served from the cache");
    assert!(
        matches!(second.freshness, Freshness::Cached { .. }),
        "expected a cache hit, got {:?}",
        second.freshness
    );
    assert!(second.index.artifact("php", "8.3.33").is_some());
}

#[tokio::test]
async fn a_stale_cache_is_served_when_the_network_is_gone() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-08-14T06:55:12Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("first fetch");
    age(cache.path(), ROOT, TWO_DAYS);
    registry.unplug();

    let stale = client
        .kinds(&["php"])
        .await
        .expect("an old index is still an index");
    assert!(
        stale.freshness.is_stale(),
        "expected staleness to be reported, got {:?}",
        stale.freshness
    );
    // The whole point of serving it: a version list from two days ago still installs PHP 8.3.33.
    assert!(stale.index.artifact("php", "8.3.33").is_some());
}

#[tokio::test]
async fn an_index_from_before_the_cached_one_is_refused_and_the_cache_kept() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-01T00:00:00Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("first fetch");

    // Correctly signed, by the right key, and older: a stale CDN edge, or a copy replayed from
    // before a security release. The signature cannot tell it apart from the current one.
    registry.publish(&index_at("2026-08-14T06:55:12Z"));
    age(cache.path(), ROOT, TWO_DAYS);

    let kept = client
        .kinds(&["php"])
        .await
        .expect("the cached index is kept");
    assert!(
        kept.freshness.is_stale(),
        "the refusal has to be visible, got {:?}",
        kept.freshness
    );
    assert_eq!(
        kept.index.generated_at().to_string(),
        "2026-09-01T00:00:00Z",
        "the newer document must survive being offered an older one"
    );
}

/// Cells of the published index that are empty, and whose reason is written down elsewhere.
///
/// Per *cell* rather than per kind, deliberately: a kind-wide allowance would have hidden the forty
/// other empty ARM64 Windows cells that [`Target::runnable`] now fills.
///
/// [`Target::runnable`]: mixengine_core::index::Target::runnable
const KNOWN_EMPTY: &[(&str, &str, Os, Arch)] = &[
    // The packaging repository's **P12b** — Redis 7.2 builds on Windows and cannot start there, so
    // no Windows artifact exists and an ARM64 Windows machine has nothing to emulate either.
    ("redis", "7.2.15", Os::Windows, Arch::X86_64),
    ("redis", "7.2.15", Os::Windows, Arch::Aarch64),
    // The same line's next patch, published since, for the same reason.
    ("redis", "7.2.16", Os::Windows, Arch::X86_64),
    ("redis", "7.2.16", Os::Windows, Arch::Aarch64),
];

/// The kinds this build can install: `RuntimeKind::ALL`, and every recipe's package but `php-fpm`.
///
/// `php-fpm`'s process comes out of a PHP install rather than out of a package of its own — the
/// header of `mixengine_core::generate::recipes` says so — so the index never publishes one, and a
/// coverage reading that expected it would report a hole that is not there. Read off the same two
/// lists the daemon reads rather than kept as a third.
fn installable_kinds() -> Vec<String> {
    let mut kinds: Vec<String> = RuntimeKind::ALL.iter().map(ToString::to_string).collect();
    kinds.extend(
        Catalogue::builtin()
            .packages()
            .filter(|package| *package != php_fpm::PACKAGE)
            .map(str::to_owned),
    );
    kinds.sort();
    kinds.dedup();
    kinds
}

/// The compiled-in key against the index that is actually published, and what the pipeline has
/// produced for each of the six targets — roadmap task **T92**.
///
/// **`#[ignore]`d, and it is the only test in this workspace that reaches the internet.** The suite
/// runs with egress blocked on purpose, so this cannot be part of it — but the things it checks are
/// exactly the things every other test here cannot: that [`mixengine_core::index::PUBLIC_KEY`] and
/// [`mixengine_core::index::DEFAULT_URL`] still describe reality, and that
/// `docs/operations/runtime-packaging.md`'s claim of *"all runtimes across six OS/arch targets"*
/// is still true of the document rather than of a plan. `MockRegistry` proves the client accepts a
/// correctly signed index; only this proves it accepts *ours*, and only this reads what ours says.
///
/// What it prints is two matrices: what the pipeline **published** for each exact target, and what a
/// MixEngine build on that target can **install** once [`Target::runnable`] is applied. The second
/// is what it fails on — a cell nothing can be installed from, and no reason in [`KNOWN_EMPTY`].
///
/// It is a test rather than an example because there was already a test here reaching the same
/// document with the same key; a second door to one question is a second thing to keep in step.
/// Run it deliberately — after a key rotation, a change to the publishing pipeline, or before
/// cutting a release, where `docs/operations/build-and-release.md` now asks for it:
///
/// ```text
/// cargo test -p mixengine-core --test index -- --ignored --nocapture
/// ```
///
/// [`Target::runnable`]: mixengine_core::index::Target::runnable
#[tokio::test]
#[ignore = "reaches the internet; the suite runs with egress blocked"]
async fn the_published_index_verifies_against_the_key_in_this_build() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let catalogue = PackageIndex::new(cache.path())
        .expect("the compiled-in key parses")
        .all()
        .await
        .expect("the published schema 2 set verifies against the compiled-in key");

    let index = catalogue.index;
    assert!(
        index.missing().is_empty(),
        "every kind the root names is readable: {:?}",
        index.missing()
    );

    // **The client's half of the publisher's `verify.py`** — roadmap task T196: the set decodes to
    // what `index.json` lists, value for value. Both are read in the same second, and a publish
    // landing between the two reads is the one way this can differ without anything being wrong.
    let other = tempfile::tempdir().expect("a second cache directory");
    let schema1 = Client::<mixengine_core::index::schema1::Document>::with(
        mixengine_core::index::DEFAULT_URL,
        mixengine_core::index::PUBLIC_KEY,
        other.path(),
    )
    .expect("a client")
    .catalogue()
    .await
    .expect("index.json verifies against the compiled-in key")
    .index;
    assert_eq!(
        schema1.generated_at,
        index.generated_at(),
        "the two encodings were published together"
    );

    let by_name =
        |package: &&mixengine_core::index::Package| (package.kind.clone(), package.version.clone());
    let mut from_set: Vec<&mixengine_core::index::Package> = index.packages().collect();
    let mut from_document: Vec<&mixengine_core::index::Package> = schema1.packages.iter().collect();
    from_set.sort_by_key(by_name);
    from_document.sort_by_key(by_name);
    assert!(
        from_set == from_document,
        "schema 2 decodes to something index.json does not say"
    );

    let kinds = installable_kinds();
    println!(
        "index generated {} — {} packages, {} artifacts, {} kinds this build can install",
        index.generated_at(),
        index.packages().count(),
        index
            .packages()
            .map(|package| package.artifacts.len())
            .sum::<usize>(),
        kinds.len(),
    );

    for (title, native_only) in [("published", true), ("installable", false)] {
        println!("\n{title}");
        print!("{:<12}", "kind");
        for target in TARGETS {
            print!(
                "{:>16}",
                format!("{}/{}", target.os.as_str(), target.arch.as_str())
            );
        }
        println!();

        for kind in &kinds {
            let published = || index.packages().filter(|package| package.kind == *kind);
            print!("{kind:<12}");

            for target in TARGETS {
                let have = published()
                    .filter(|package| {
                        package.select(target).is_some_and(|chosen| {
                            !native_only || chosen.execution == Execution::Native
                        })
                    })
                    .count();
                print!("{:>16}", format!("{have}/{}", published().count()));
            }
            println!();
        }
    }

    // A kind the index publishes and this build cannot run is not a failure — the pipeline may
    // publish ahead of a release — but a kind this build offers and the index does not is a
    // command that can only refuse.
    for kind in &kinds {
        assert!(
            index.packages().any(|package| package.kind == *kind),
            "this build can install {kind} and the published index has none"
        );
    }

    let mut holes = Vec::new();
    for package in index
        .packages()
        .filter(|package| kinds.contains(&package.kind))
    {
        for target in TARGETS {
            let known = KNOWN_EMPTY.iter().any(|(kind, version, os, arch)| {
                *kind == package.kind
                    && *version == package.version
                    && *os == target.os
                    && *arch == target.arch
            });

            if package.select(target).is_none() && !known {
                holes.push(format!(
                    "{} {} on {}/{}",
                    package.kind,
                    package.version,
                    target.os.as_str(),
                    target.arch.as_str()
                ));
            }
        }
    }

    assert!(
        holes.is_empty(),
        "nothing to install and no reason written down for {} cell(s):\n  {}",
        holes.len(),
        holes.join("\n  ")
    );
}

#[tokio::test]
async fn a_cache_somebody_rewrote_is_ignored_rather_than_trusted() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-08-14T06:55:12Z")).await;
    let client = client(&registry, cache.path());

    client.kinds(&["php"]).await.expect("first fetch");

    // The cache is an ordinary file in the user's home. Anything on this machine can rewrite it,
    // which is why it is verified on the way in rather than trusted because we wrote it once.
    std::fs::write(
        cache.path().join(ROOT),
        serde_json::to_vec(&serde_json::json!({
            "schema": 2, "generated_at": "2099-01-01T00:00:00Z",
            "base_url": "https://example.invalid", "kinds": {}
        }))
        .expect("serialise"),
    )
    .expect("rewrite the cache");

    let recovered = client
        .kinds(&["php"])
        .await
        .expect("the network still answers");
    assert_eq!(
        recovered.freshness,
        Freshness::Fetched,
        "a tampered cache must send the client back to the network"
    );
    assert_eq!(
        recovered.index.generated_at().to_string(),
        "2026-08-14T06:55:12Z"
    );
}

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
            mixengine_core::Error::IndexSchema {
                found: 3,
                expected: 2,
                ..
            }
        ),
        "expected a schema refusal, got {refusal:?}"
    );
}

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

/// Each file has its own limit. The registry cannot stall, so the limit is what shrinks: one no
/// request can meet.
#[tokio::test]
async fn a_file_is_given_up_on_after_its_own_limit() {
    let cache = tempfile::tempdir().expect("a cache directory");
    let registry = MockRegistry::start(&index_at("2026-09-30T00:00:00Z")).await;

    let refusal = client(&registry, cache.path())
        .with_timeout(Duration::from_nanos(1))
        .kinds(&["php"])
        .await
        .expect_err("nothing arrives in a nanosecond");

    assert!(
        matches!(refusal, mixengine_core::Error::IndexTransport { .. }),
        "expected a transport failure, got {refusal:?}"
    );
}
