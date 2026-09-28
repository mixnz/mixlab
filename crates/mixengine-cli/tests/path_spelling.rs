//! Roadmap task **T191**: every path `mix --json` hands a person is spelled the way this system
//! spells one.
//!
//! One test for the rule rather than one per field, so a field added later cannot bring the mixed
//! spelling back (`C:\…\data/mariadb`) without failing here. It is meaningful on Windows only,
//! where the two separators differ, and says so when it is skipped elsewhere.
//!
//! **Multi-threaded on purpose**, for `tests/package.rs`' reason: `MockRegistry` serves the daemon
//! `mix` is talking to from this same process.

mod harness;

use harness::{Home, json};
use mixengine_testkit::{FakePackage, MockRegistry, Packed, Packing};
use serde_json::{Value, json as document};

/// The version this suite installs.
const VERSION: &str = "1.0.0";

/// The package it installs, which is the one a debug build has a recipe for.
const PACKAGE: &str = "fakeservice";

/// Under `bin/`, unlike `tests/package.rs`, so the install has a `provides` path holding a `/`.
fn program_name() -> String {
    format!("bin/{PACKAGE}{}", std::env::consts::EXE_SUFFIX)
}

/// An index offering exactly one version, for this machine — `tests/package.rs`' own.
fn index(packed: &Packed, url: &str) -> Value {
    document!({
        "schema": 1,
        "generated_at": "2026-09-28T00:00:00Z",
        "packages": [{
            "kind": PACKAGE,
            "version": VERSION,
            "channel": "stable",
            "artifacts": [{
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "url": url,
                "sha256": packed.sha256,
                "size": packed.size(),
                "provides": { "fakeservice": program_name() },
            }],
        }],
    })
}

/// Every string in `value` that is a drive or share path holding a `/`, with where it was found.
fn mixed(value: &Value, at: &str, found: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            let bytes = text.as_bytes();
            let drive = bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
            if (drive || text.starts_with(r"\\")) && text.contains('/') {
                found.push(format!("{at} = {text}"));
            }
        }
        Value::Array(items) => {
            for (position, item) in items.iter().enumerate() {
                mixed(item, &format!("{at}[{position}]"), found);
            }
        }
        Value::Object(fields) => {
            for (key, field) in fields {
                mixed(field, &format!("{at}.{key}"), found);
            }
        }
        _ => {}
    }
}

#[tokio::test(flavor = "multi_thread")]
#[cfg_attr(
    any(not(windows), not(debug_assertions)),
    ignore = "runs on Windows in a debug build only: the two separators differ only there, and a \
              release daemon has no fakeservice recipe to install against"
)]
async fn every_path_mix_hands_back_is_spelled_the_way_this_system_spells_one() {
    let packed = FakePackage::new(Packing::Zip)
        .executable(&program_name())
        .build(&format!("{PACKAGE}-{VERSION}"));
    let registry = MockRegistry::start(&document!({
        "schema": 1, "generated_at": "2026-09-28T00:00:00Z", "packages": []
    }))
    .await;
    let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
    registry.publish(&index(&packed, &url));

    // The relocation is written with `/`, as the configuration template advises.
    let bulk = tempfile::tempdir().expect("somewhere to move packages to");
    let packages = bulk
        .path()
        .join("packages")
        .display()
        .to_string()
        .replace('\\', "/");

    let home = Home::new();
    let _daemon = home.start_daemon_with(&[
        "--index-url",
        &registry.url(),
        "--index-key",
        registry.public_key(),
        "--packages",
        &packages,
    ]);

    let installed = json(&home.mix(&["package", "install", PACKAGE, VERSION, "--json"]));
    assert_eq!(
        installed["state"],
        "succeeded",
        "{installed}\n{}",
        home.daemon_log()
    );

    let project = tempfile::tempdir().expect("a project directory");
    let root = project.path();
    std::fs::create_dir_all(root.join("public").join("assets")).expect("a doc root");
    std::fs::create_dir_all(root.join("dist").join("css")).expect("a static root");
    home.mix(&[
        "project",
        "create",
        &root.display().to_string(),
        "--name",
        "blog",
    ]);
    json(&home.mix(&[
        "site",
        "create",
        "--project",
        "blog",
        "--domain",
        "blog.test",
        "--kind",
        "static",
        "--https",
        "false",
        "--doc-root",
        "public/assets",
        "--files",
        "/css=dist/css",
        "--json",
    ]));

    let asked: [&[&str]; 5] = [
        &["status", "--json"],
        &["package", "list", "--json"],
        &["site", "list", "--json"],
        &["site", "show", "blog.test", "--json"],
        &["storage", "--json"],
    ];

    let mut found = Vec::new();
    for arguments in asked {
        let answer = json(&home.mix(arguments));
        mixed(&answer, &format!("mix {}", arguments.join(" ")), &mut found);
    }

    let shown = json(&home.mix(&["site", "show", "blog.test", "--json"]));
    for relative in [
        &shown["site"]["doc_root"],
        // A route's target is flattened into it: `{"path":"/css","target":"static","root":…}`.
        &shown["site"]["routes"][0]["root"],
    ] {
        let relative = relative
            .as_str()
            .unwrap_or_else(|| panic!("a relative path where expected in {shown}"));
        if relative.contains('/') {
            found.push(format!("mix site show: relative path {relative}"));
        }
    }

    assert!(
        found.is_empty(),
        "paths spelled with a `/` inside a Windows path:\n{}\n--- daemon log ---\n{}",
        found.join("\n"),
        home.daemon_log()
    );
}
