//! The index client against a real signed index over a real socket.
//!
//! Everything here goes through [`mixengine_testkit::MockRegistry`], which generates its own keypair
//! and signs with `minisign` — the signing half of the crate pair whose verifying half the client
//! links. So these assert that the client accepts what minisign actually produces rather than what
//! we believe it produces, which matters because the format has a legacy variant the client refuses
//! on purpose and a hand-built fixture would have hidden the difference.

// Every test here fetches over a socket, which `network` carries (ADR 0067).
#![cfg(feature = "network")]

use std::path::Path;
use std::time::{Duration, SystemTime};

use mixengine_core::InstallError;
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

mod schema_1;
mod schema_2;
mod signed;
