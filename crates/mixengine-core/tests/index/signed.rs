//! A signed index: fetched, verified, cached and refused.

use super::*;

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
/// own would have — [`Client::with_transport`]'s reason for existing.
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
        matches!(
            refusal,
            mixengine_core::Error::Install(InstallError::IndexSignature { .. })
        ),
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
        matches!(
            refusal,
            mixengine_core::Error::Install(InstallError::IndexSignature { .. })
        ),
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
    // The same line's next patches, published since, for the same reason.
    ("redis", "7.2.16", Os::Windows, Arch::X86_64),
    ("redis", "7.2.16", Os::Windows, Arch::Aarch64),
    ("redis", "7.2.17", Os::Windows, Arch::X86_64),
    ("redis", "7.2.17", Os::Windows, Arch::Aarch64),
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
/// produced for each of the six targets.
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

    // **The client's half of the publisher's `verify.py`**: the set decodes to
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
