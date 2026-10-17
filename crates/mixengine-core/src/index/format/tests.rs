use super::*;

/// **What a cell cannot do reaches MixEngine** — roadmap task **T206**, D2.
#[test]
fn an_artifact_carries_what_it_lacks() {
    let artifact: Artifact = serde_json::from_value(serde_json::json!({
        "os": "windows", "arch": "x86_64", "url": "https://x.invalid/r.zip",
        "sha256": "00", "size": 1, "provides": { "ruby": "bin/ruby.exe" },
        "lacks": { "native gems": "no compiler" }
    }))
    .expect("reads");
    assert_eq!(
        artifact.lacks.get("native gems").map(String::as_str),
        Some("no compiler")
    );
}

/// An entry written before T206 lacks nothing, and reads.
#[test]
fn an_artifact_without_lacks_lacks_nothing() {
    let artifact: Artifact = serde_json::from_value(serde_json::json!({
        "os": "linux", "arch": "x86_64", "url": "https://x.invalid/r.tar.zst",
        "sha256": "00", "size": 1, "provides": { "ruby": "bin/ruby" }
    }))
    .expect("reads");
    assert!(artifact.lacks.is_empty());
}
use crate::index::schema1::Document;

#[test]
fn a_cpu_requirement_is_kept_rather_than_dropped() {
    let requires: Requires =
        serde_json::from_str(r#"{"cpu": "avx", "glibc": "2.34"}"#).expect("the MongoDB shape");
    assert_eq!(requires.cpu.as_deref(), Some("avx"));
    assert!(!requires.is_empty());
}

/// **`libraries` is read** — roadmap task **T27e**, its design's D13: the sonames a Linux JDK
/// links and does not ship.
#[test]
fn requires_reads_the_libraries_a_build_links() {
    let requires: Requires =
        serde_json::from_str(r#"{"glibc": "2.17", "libraries": ["libz.so.1", "libasound.so.2"]}"#)
            .expect("the Java shape");

    assert_eq!(
        requires.libraries,
        vec!["libz.so.1".to_owned(), "libasound.so.2".to_owned()]
    );
    assert!(!requires.is_empty());
    assert!(
        !Requires {
            libraries: vec!["libz.so.1".to_owned()],
            ..Requires::default()
        }
        .is_empty(),
        "a build that asks only for a library still asks for something"
    );
    assert!(Requires::default().is_empty());
}

#[test]
fn timestamp_accepts_what_the_generator_writes() {
    let stamp = Timestamp::parse("2026-08-14T06:55:12Z").expect("the published shape");
    assert_eq!(stamp.to_string(), "2026-08-14T06:55:12Z");
}

/// The door the publishing pipeline comes through — roadmap task **T81a**.
#[test]
fn timestamp_parses_from_a_string() {
    let stamp: Timestamp = "2026-09-02T11:00:00Z".parse().expect("a UTC second parses");

    assert_eq!(stamp.to_string(), "2026-09-02T11:00:00Z");
}

/// What `FromStr` refuses quotes what it was handed: the caller is a person who typed it.
#[test]
fn what_from_str_refuses_names_the_text() {
    let refused = "2026-09-02 11:00:00"
        .parse::<Timestamp>()
        .expect_err("a space is not a T");

    assert!(
        refused.to_string().contains("2026-09-02 11:00:00"),
        "the message should quote what was handed over: {refused}"
    );
}

#[test]
fn timestamp_orders_by_moment_and_not_by_text() {
    let earlier = Timestamp::parse("2026-08-14T06:55:12Z").expect("valid");
    let later = Timestamp::parse("2026-09-02T00:00:00Z").expect("valid");
    assert!(earlier < later);
}

#[test]
fn timestamp_refuses_every_other_rfc_3339_spelling() {
    // All of these are valid RFC 3339 and none of them compares the way a reader expects
    // against the shape above, which is the entire reason this is parsed at all.
    for text in [
        "2026-08-14T06:55:12+00:00",
        "2026-08-14T06:55:12.5Z",
        "2026-08-14 06:55:12Z",
        "2026-8-14T06:55:12Z",
        "+026-08-14T06:55:12Z",
        "",
    ] {
        assert!(
            Timestamp::parse(text).is_none(),
            "{text:?} should be refused"
        );
    }
}

#[test]
fn timestamp_refuses_a_field_out_of_range() {
    assert!(Timestamp::parse("2026-13-14T06:55:12Z").is_none());
    assert!(Timestamp::parse("2026-08-14T24:55:12Z").is_none());
}

#[test]
fn an_artifact_carries_where_its_binaries_are_rather_than_only_their_names() {
    let index: Document = serde_json::from_str(
        r#"{
              "schema": 1,
              "generated_at": "2026-08-14T06:55:12Z",
              "packages": [{
                "kind": "php", "version": "8.3.33", "channel": "stable",
                "artifacts": [{
                  "os": "windows", "arch": "x86_64",
                  "url": "https://example.invalid/php.zip",
                  "sha256": "00", "size": 1,
                  "provides": { "php": "php.exe", "php-cgi": "php-cgi.exe" },
                  "requires": { "vcredist": "2019" },
                  "extension_dir": "ext",
                  "extensions": { "static": ["Core"], "shared": ["curl"] }
                }]
              }]
            }"#,
    )
    .expect("the published shape parses");

    let artifact = &index.packages[0].artifacts[0];
    assert_eq!(artifact.provides["php-cgi"], "php-cgi.exe");
    assert_eq!(artifact.requires.vcredist.as_deref(), Some("2019"));
    assert_eq!(artifact.extensions.compiled_in, ["Core"]);
}

#[test]
fn a_field_this_build_does_not_know_is_not_an_error() {
    // The opposite of `config.toml`'s rule, and deliberately: this document is written by us and
    // read by builds older than it, so adding a field must not break what is already deployed.
    let index: Document = serde_json::from_str(
        r#"{
              "schema": 1,
              "generated_at": "2026-08-14T06:55:12Z",
              "packages": [],
              "mirrors": ["https://example.invalid/"]
            }"#,
    )
    .expect("an unknown field is tolerated");
    assert!(index.packages.is_empty());
}

/// The index says which of `shared` an installer is expected to switch on, and dropping it is
/// the difference between a Windows PHP that behaves like its Unix twin and one that starts
/// without `mbstring`.
#[test]
fn an_artifact_says_which_shared_extensions_are_on_by_default() {
    let artifact: Artifact = serde_json::from_value(serde_json::json!({
        "os": "windows",
        "arch": "x86_64",
        "url": "https://example.invalid/php-8.3.33-windows-x86_64.zip",
        "sha256": "00",
        "size": 1,
        "provides": {"php": "php.exe"},
        "extension_dir": "ext",
        "extensions": {
            "static": ["core", "date"],
            "shared": ["curl", "mbstring", "xdebug"],
            "enabled": ["curl", "mbstring"]
        }
    }))
    .expect("an artifact the published schema allows");

    assert_eq!(artifact.extensions.enabled, ["curl", "mbstring"]);
    assert!(
        !artifact.extensions.enabled.contains(&"xdebug".to_owned()),
        "a shared extension the publisher does not switch on is not enabled by being shipped"
    );
}

/// An index shaped like the published one where it matters: PHP is on five targets and not on
/// ARM64 Windows, which is true of every branch upstream has ever built.
fn php_as_published() -> Document {
    let artifact = |os: Os, arch: Arch| {
        serde_json::json!({
            "os": os.as_str(), "arch": arch.as_str(),
            "url": format!("https://example.invalid/php-{}-{}.zip", os.as_str(), arch.as_str()),
            "sha256": "00", "size": 1,
            "provides": { "php": "php.exe" }
        })
    };

    serde_json::from_value(serde_json::json!({
        "schema": 1,
        "generated_at": "2026-08-31T07:40:07Z",
        "packages": [{
            "kind": "php", "version": "8.3.33", "channel": "stable",
            "artifacts": [
                artifact(Os::Windows, Arch::X86_64),
                artifact(Os::Macos, Arch::X86_64),
                artifact(Os::Macos, Arch::Aarch64),
                artifact(Os::Linux, Arch::X86_64),
                artifact(Os::Linux, Arch::Aarch64),
            ]
        }]
    }))
    .expect("the published shape parses")
}

/// The finding this task exists for — roadmap task **T92**.
#[test]
fn an_arm64_windows_machine_is_offered_the_x86_64_build_and_told_so() {
    let index = Index::from(php_as_published());

    let chosen = index
        .select(Target::new(Os::Windows, Arch::Aarch64), "php", "8.3.33")
        .expect("the x86_64 build is what that machine can run");

    assert_eq!(chosen.artifact.arch, Arch::X86_64);
    assert_eq!(chosen.execution, Execution::Emulated);
}

#[test]
fn every_other_target_is_offered_its_own_build() {
    let index = Index::from(php_as_published());

    for target in TARGETS {
        let chosen = index
            .select(target, "php", "8.3.33")
            .expect("reachable from all six");

        match target == Target::new(Os::Windows, Arch::Aarch64) {
            true => assert_eq!(chosen.execution, Execution::Emulated),
            false => {
                assert_eq!(chosen.execution, Execution::Native);
                assert_eq!(chosen.artifact.os, target.os);
                assert_eq!(chosen.artifact.arch, target.arch);
            }
        }
    }
}

/// A native build wins over one that would have to be emulated, and it wins whichever order the
/// generator wrote the two artifacts in — which is the whole reason the search walks the
/// preference list on the outside.
#[test]
fn a_native_build_wins_over_one_that_would_have_to_be_emulated() {
    for first in [false, true] {
        let mut document = php_as_published();
        let native: Artifact = serde_json::from_value(serde_json::json!({
            "os": "windows", "arch": "aarch64",
            "url": "https://example.invalid/php-windows-aarch64.zip",
            "sha256": "00", "size": 1, "provides": { "php": "php.exe" }
        }))
        .expect("an artifact");

        match first {
            true => document.packages[0].artifacts.insert(0, native),
            false => document.packages[0].artifacts.push(native),
        }
        let index = Index::from(document);

        let chosen = index
            .select(Target::new(Os::Windows, Arch::Aarch64), "php", "8.3.33")
            .expect("published");

        assert_eq!(chosen.execution, Execution::Native);
        assert_eq!(chosen.artifact.arch, Arch::Aarch64);
    }
}

/// `redis 7.2.15` is the shape of this: no Windows artifact at all, so no Windows machine is
/// offered one and the emulation rule changes nothing.
#[test]
fn a_version_with_no_windows_build_is_offered_to_no_windows_machine() {
    let mut document = php_as_published();
    document.packages[0]
        .artifacts
        .retain(|artifact| artifact.os != Os::Windows);
    let index = Index::from(document);

    for arch in [Arch::X86_64, Arch::Aarch64] {
        let target = Target::new(Os::Windows, arch);
        assert!(index.select(target, "php", "8.3.33").is_none());
        assert_eq!(index.installable_for(target, "php").count(), 0);
    }
}

/// The listing follows the selection, so a version reachable only by emulation is listed.
#[test]
fn a_version_reachable_only_by_emulation_is_listed_as_installable() {
    let index = Index::from(php_as_published());

    assert_eq!(
        index
            .installable_for(Target::new(Os::Windows, Arch::Aarch64), "php")
            .count(),
        1
    );
    assert_eq!(
        index
            .installable_for(Target::new(Os::Windows, Arch::X86_64), "node")
            .count(),
        0,
        "a kind the index has nothing for is still nothing"
    );
}

/// The whole of the emulation rule, asserted per target — roadmap task **T92**.
#[test]
fn only_an_arm64_windows_machine_can_run_something_that_is_not_its_own_build() {
    for target in TARGETS {
        let runs = target.runnable();
        assert_eq!(runs[0], target, "a machine prefers its own build, always");

        match (target.os, target.arch) {
            (Os::Windows, Arch::Aarch64) => assert_eq!(
                runs,
                [target, Target::new(Os::Windows, Arch::X86_64)],
                "Windows on ARM runs an x86_64 build under the operating system's emulation"
            ),
            _ => assert_eq!(runs.len(), 1, "{target:?} runs nothing but its own build"),
        }
    }
}

/// Six, and the matrix in `runtime-packaging.md` is read in this order.
#[test]
fn the_targets_are_the_six_mixengine_ships_a_build_for() {
    assert_eq!(TARGETS[0], Target::new(Os::Windows, Arch::X86_64));
    assert_eq!(
        TARGETS
            .iter()
            .filter(|target| target.os == Os::Macos)
            .count(),
        2
    );
    for (at, target) in TARGETS.iter().enumerate() {
        assert!(
            !TARGETS[at + 1..].contains(target),
            "no target is named twice: {target:?}"
        );
    }
}

/// An index from before this field, and an artifact that loads nothing, are both silent.
#[test]
fn an_artifact_with_no_extensions_at_all_stays_empty() {
    let extensions: Extensions = serde_json::from_str("{}").expect("an empty object");

    assert!(extensions.is_empty());
    assert_eq!(serde_json::to_string(&extensions).expect("json"), "{}");
}

fn package(kind: &str, version: &str) -> Package {
    serde_json::from_value(serde_json::json!({
        "kind": kind, "version": version, "channel": "stable",
        "artifacts": [{
            "os": "linux", "arch": "x86_64",
            "url": format!("https://example.invalid/{kind}-{version}.zip"),
            "sha256": "00", "size": 1, "provides": { kind: "bin/x" }
        }]
    }))
    .expect("a package")
}

/// A view holds the kinds it was asked for — roadmap task **T196**, its design's D2.
#[test]
fn a_view_answers_for_the_kinds_it_was_built_for() {
    let stamp: Timestamp = "2026-09-30T00:00:00Z".parse().expect("valid");
    let mut kinds: BTreeMap<String, Arc<[Package]>> = BTreeMap::new();
    kinds.insert("php".to_owned(), Arc::from(vec![package("php", "8.4.1")]));
    kinds.insert("go".to_owned(), Arc::from(Vec::<Package>::new()));
    let view = Index::from_kinds(
        stamp,
        kinds,
        vec![Missing {
            kind: "node".to_owned(),
            reason: "does not hash to what the root says".to_owned(),
        }],
    );

    let linux = Target::new(Os::Linux, Arch::X86_64);
    assert_eq!(view.installable_for(linux, "php").count(), 1);
    assert_eq!(
        view.installable_for(linux, "go").count(),
        0,
        "a kind the root does not name is empty"
    );
    assert_eq!(view.installable_for(linux, "node").count(), 0);
    assert_eq!(view.packages().count(), 1);
    assert_eq!(
        view.unread("node").map(|missing| missing.kind.as_str()),
        Some("node")
    );
    assert!(view.unread("php").is_none());
    assert_eq!(view.generated_at().to_string(), "2026-09-30T00:00:00Z");
}

#[test]
fn a_whole_document_answers_for_every_kind() {
    let stamp: Timestamp = "2026-09-30T00:00:00Z".parse().expect("valid");
    let index = Index::from_packages(
        stamp,
        vec![package("php", "8.4.1"), package("node", "22.1.0")],
    );

    let linux = Target::new(Os::Linux, Arch::X86_64);
    assert_eq!(index.installable_for(linux, "node").count(), 1);
    assert_eq!(index.installable_for(linux, "ruby").count(), 0);
    assert!(index.missing().is_empty());
}
