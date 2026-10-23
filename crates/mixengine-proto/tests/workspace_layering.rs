//! Enforces the workspace dependency direction described in
//! `docs/architecture/overview.md`: strictly downward, `core` never depending on `daemon`.
//!
//! The test lives in `mixengine-proto` because proto is the bottom of the graph and therefore the
//! cheapest crate to build — but it checks every member of the workspace, not just this one.

use std::collections::BTreeSet;

use cargo_metadata::{DependencyKind, MetadataCommand};

/// For each workspace crate, the workspace crates it is allowed to depend on.
///
/// Adding an edge here is an architectural decision. Adding one that points upward (anything to
/// `mixengine-daemon`, say) is the bug this test exists to catch.
const ALLOWED_EDGES: &[(&str, &[&str])] = &[
    ("mixengine-proto", &[]),
    ("mixengine-platform", &["mixengine-proto"]),
    (
        "mixengine-supervisor",
        &["mixengine-platform", "mixengine-proto"],
    ),
    ("mixengine-core", &["mixengine-platform", "mixengine-proto"]),
    (
        "mixengine-elevate",
        &["mixengine-platform", "mixengine-proto"],
    ),
    (
        "mixengine-daemon",
        &[
            "mixengine-core",
            "mixengine-platform",
            "mixengine-proto",
            "mixengine-supervisor",
        ],
    ),
    // `platform` is here for `ipc::Connection` and `HomeDirs` alone — the transport `mix` dials and
    // the OS convention that says which home it dials for (roadmap task T10). Narrow on purpose: a
    // client that reached further into that crate would be doing something to the machine, which is
    // the daemon's job, and the ban on business logic in a client holds either way. Notably absent
    // is `mixengine-core`, which the CLI would otherwise want for `Paths`: it carries `sqlx`, and
    // linking a bundled SQLite into `mix` to learn that `run/` sits under the root is a trade
    // nobody would make. See `home.rs` for the one thing that duplicates instead, and for the test
    // that keeps the two answers together.
    // The handbook, and the bottom of the graph beside `proto`. It depends on nothing at all: its
    // front matter is parsed by a build script, so what a binary links is a table of `&'static str`
    // — roadmap task T90. The Markdown renderer that builds the site is a dev-dependency of it,
    // which is what keeps a renderer out of `mix`.
    ("mixengine-docs", &[]),
    (
        "mixengine-cli",
        &["mixengine-docs", "mixengine-platform", "mixengine-proto"],
    ),
    // The client that *does* take `mixengine-core`, and the one that has to: a shim resolves a
    // version in its own process because the whole promise is that it works with no daemon running
    // — see `mixengine-shim/src/main.rs` for why the trade goes the other way here than for `mix`.
    // Still no `mixengine-daemon` and no `mixengine-supervisor`: it starts one program and
    // supervises nothing.
    (
        "mixengine-shim",
        &["mixengine-core", "mixengine-platform", "mixengine-proto"],
    ),
    // What `<root>/bin` holds per command name on Windows (T185), and so the one binary whose size
    // is paid dozens of times over. `platform` alone, and only its `handover` feature: anything
    // else here would be a copy of it in every name.
    ("mixengine-trampoline", &["mixengine-platform"]),
    // Fixtures, and nothing a user runs. It may depend on `platform` because `fakeservice` reaches
    // the same `Signals` and `spawn_detached` the daemon does — using them rather than reimplementing
    // them is what keeps a `#[cfg]` out of the fixture. Nothing may depend on *it* except as a
    // dev-dependency, which is the rule below rather than this table.
    ("mixengine-testkit", &["mixengine-platform"]),
];

/// The crate that may only ever be a dev-dependency.
///
/// A separate rule from [`ALLOWED_EDGES`], because it is about the *kind* of edge rather than its
/// direction: `mixengine-testkit` is allowed to be used by every crate in the workspace and by none
/// of their shipped binaries. Listing it as an ordinary dependency of, say, the daemon would compile
/// perfectly well and put `fakeservice`'s argument parser inside `mixengined`.
const DEV_ONLY: &str = "mixengine-testkit";

#[test]
fn dependency_direction_is_downward() {
    let metadata = MetadataCommand::new()
        .manifest_path(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .no_deps()
        .exec()
        .expect("cargo metadata runs inside the workspace");

    let members: BTreeSet<&str> = metadata
        .packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();

    let declared: BTreeSet<&str> = ALLOWED_EDGES.iter().map(|(krate, _)| *krate).collect();
    assert_eq!(
        members, declared,
        "the workspace members and the crates listed in ALLOWED_EDGES have drifted apart"
    );

    for package in &metadata.packages {
        let allowed = ALLOWED_EDGES
            .iter()
            .find(|(krate, _)| *krate == package.name.as_str())
            .map(|(_, allowed)| *allowed)
            .expect("checked above that every member is listed");

        for dependency in &package.dependencies {
            if !members.contains(dependency.name.as_str()) {
                continue; // third-party crates are governed by deny.toml, not by this test
            }

            // A dev-dependency is not part of the shipped graph, so the direction rules do not
            // reach it — a test may use whatever it needs, including a crate above it. The one
            // thing that is not a direction at all is a crate reaching for itself, which cargo
            // accepts and which would make the graph read as a cycle to anything that walked it.
            if dependency.kind == DependencyKind::Development {
                assert_ne!(
                    dependency.name.as_str(),
                    package.name.as_str(),
                    "{} lists itself in [dev-dependencies]",
                    package.name
                );
                continue;
            }

            assert_ne!(
                dependency.name.as_str(),
                DEV_ONLY,
                "{} depends on {DEV_ONLY} outside of [dev-dependencies], which would put fixtures \
                 into a shipped binary",
                package.name
            );
            assert!(
                allowed.contains(&dependency.name.as_str()),
                "{} depends on {}, which the layering does not allow",
                package.name,
                dependency.name
            );
        }
    }
}

/// The crates whose `src/` may not compile a line away on one operating system and keep it on
/// another — `CLAUDE.md`'s "no `#[cfg(windows)]` in core/daemon code", and the half of
/// "cross-platform or not merged" that a compiler cannot state.
///
/// Absent on purpose: `mixengine-platform`, where per-OS code is the entire point;
/// `mixengine-elevate`, which is one audited binary per OS; and `mixengine-testkit`, whose
/// `fakeservice` reaches the same `Signals` and `spawn_detached` the daemon does for exactly this
/// reason — using `platform` rather than reimplementing it is what keeps the `#[cfg]` out.
const NO_OS_CFG: &[&str] = &[
    "mixengine-core",
    "mixengine-daemon",
    "mixengine-cli",
    "mixengine-proto",
    "mixengine-shim",
    "mixengine-supervisor",
    "mixengine-trampoline",
];

/// The files that may hold one anyway, and what it is for.
///
/// Every entry here is test code, and the assertion below is what keeps it that way: a permitted
/// file must carry exactly one `#[cfg(test)]` and every OS `cfg` in it must come after that line.
/// One `#[cfg(test)]` is what makes "after" mean something — a second one would put a test region
/// somewhere in the middle and this test would be measuring nothing, so a file that grows one fails
/// rather than quietly widening its own permission.
///
/// **A `foo/tests.rs` is test code from its first line** (T207e): it is the body of the module
/// `foo.rs` declares as `#[cfg(test)]` directly above `mod tests;`, and the check asserts that pair
/// in `foo.rs` instead of a line inside the file. `foo.rs` itself is not permitted anything, so an OS
/// `cfg` there is an offence like any other.
///
/// **So is a topic `foo/tests/<topic>.rs` under that hub** (T207f, T207o): the hub `foo/tests.rs`
/// declares it with `mod <topic>;`, and `foo.rs` declares the hub as above. The hub is not permitted
/// anything by this — only the topic named here is.
const PERMITTED: &[(&str, &str)] = &[
    (
        "mixengine-core/src/generate/recipes/postgres/tests/server.rs",
        "the tests for the two shapes of `pg_ctl` invocation, which differ by OS",
    ),
    (
        "mixengine-core/src/install/archive.rs",
        "the tests for what an archive's permission bits mean where there are none",
    ),
    (
        "mixengine-core/src/updates/helper.rs",
        "the test for the executable bit a staged privileged helper has to arrive with, which is \
         a mode on two of the three systems and nothing at all on the third",
    ),
    (
        "mixengine-daemon/src/uninstall/inventory/tests.rs",
        "the test for what a dangling symlink means, on the system that has them",
    ),
    (
        "mixengine-daemon/src/disk/measure.rs",
        "the test that a walk counts a symlink as itself rather than following it, which needs a \
         symlink to make — and making one on Windows needs a privilege the runner does not have",
    ),
];

/// **`cfg!` is deliberately not caught, and the difference is not a technicality.** A `cfg!(windows)`
/// is a *value*: both arms are compiled on every OS, type-checked on every OS, and reachable from a
/// test on every OS, which is what "cross-platform or not merged" asks for. A `#[cfg(windows)]`
/// deletes code from the build, so what the other two platforms compile is a different program and
/// no test on them can say anything about it. This workspace uses the first idiom in about forty
/// places on purpose — see `recipes/mysql.rs`, which goes one better and passes `windows` as an
/// argument so the whole table is exercised everywhere.
#[test]
fn no_crate_but_platform_compiles_a_line_away_by_operating_system() {
    let workspace = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR")))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the manifest sits two levels under the workspace root")
        .to_owned();

    let mut offences = Vec::new();

    for krate in NO_OS_CFG {
        let source = workspace.join("crates").join(krate).join("src");
        assert!(
            source.is_dir(),
            "{} has no src/, so this test is checking a crate that moved",
            source.display()
        );

        for file in rust_files(&source) {
            // Spelled with forward slashes whatever this OS uses, because `PERMITTED` is a written
            // list somebody reads and a path that changed shape by runner would be two entries.
            let within = file
                .strip_prefix(&source)
                .expect("walked from this directory")
                .display()
                .to_string()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let relative = format!("{krate}/src/{within}");

            let text = std::fs::read_to_string(&file).expect("a source file this build compiled");
            let lines: Vec<&str> = text.lines().collect();

            let cfgs: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| is_os_cfg_attribute(line))
                .map(|(at, _)| at)
                .collect();

            if cfgs.is_empty() {
                continue;
            }

            let Some((_, why)) = PERMITTED.iter().find(|(path, _)| *path == relative) else {
                for at in cfgs {
                    offences.push(format!("{relative}:{} — {}", at + 1, lines[at].trim()));
                }
                continue;
            };

            // A topic under a test hub: the hub names it, and the hub is a `tests.rs` like below.
            let hub = file
                .parent()
                .filter(|dir| dir.file_name().is_some_and(|name| name == "tests"))
                .map(|dir| dir.with_extension("rs"));
            if let Some(hub) = hub {
                let topic = file
                    .file_stem()
                    .expect("a topic file has a name")
                    .to_string_lossy();
                let named = std::fs::read_to_string(&hub).unwrap_or_default();

                assert!(
                    named
                        .lines()
                        .any(|line| line.trim() == format!("mod {topic};")),
                    "{relative} is permitted an OS cfg for {why} as a test topic, but {} does not \
                     declare it with `mod {topic};`",
                    hub.display()
                );
                assert_declares_tests(&hub, &relative, why);

                continue;
            }

            if relative.ends_with("/tests.rs") {
                assert_declares_tests(&file, &relative, why);

                continue;
            }

            let tests: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.trim() == "#[cfg(test)]")
                .map(|(at, _)| at)
                .collect();

            assert_eq!(
                tests.len(),
                1,
                "{relative} is permitted an OS cfg for {why}, but it now has {} `#[cfg(test)]` \
                 attributes rather than one, so \"after the test module begins\" no longer means \
                 anything here",
                tests.len()
            );

            for at in cfgs {
                assert!(
                    at > tests[0],
                    "{relative}:{} sits before the test module, so it is compiled into the product \
                     rather than into a test: {}",
                    at + 1,
                    lines[at].trim()
                );
            }
        }
    }

    assert!(
        offences.is_empty(),
        "these compile a line away by operating system, which belongs in mixengine-platform behind \
         a trait — or, if both arms must exist everywhere, in a `cfg!` value:\n  {}",
        offences.join("\n  ")
    );
}

/// Assert that the file beside `tests` — `foo.rs` for `foo/tests.rs` — declares it with
/// `#[cfg(test)]` directly above `mod tests;`, which is what makes all of `tests` test code.
fn assert_declares_tests(tests: &std::path::Path, relative: &str, why: &str) {
    let parent = tests
        .parent()
        .expect("a tests.rs sits in its module's directory")
        .with_extension("rs");
    let declared = std::fs::read_to_string(&parent).unwrap_or_default();
    let declared: Vec<&str> = declared.lines().collect();

    assert!(
        declared
            .windows(2)
            .any(|pair| pair[0].trim() == "#[cfg(test)]" && pair[1].trim() == "mod tests;"),
        "{relative} is permitted an OS cfg for {why} as a test module, but {} does not declare \
         its tests with `#[cfg(test)]` directly above `mod tests;`",
        parent.display()
    );
}

/// Whether `line` is a `#[cfg(…)]` attribute that names an operating system.
///
/// Prose is skipped rather than parsed: a doc comment in this workspace may well quote
/// `#[cfg(windows)]` while arguing against it, and a lint that failed on its own rationale would be
/// worse than no lint. Nothing else here needs to understand Rust — an attribute is one line, and
/// the words that make it an OS attribute cannot appear in it by accident.
fn is_os_cfg_attribute(line: &str) -> bool {
    let trimmed = line.trim_start();

    if trimmed.starts_with("//") {
        return false;
    }

    (trimmed.starts_with("#[cfg(") || trimmed.starts_with("#[cfg_attr("))
        && ["windows", "unix", "target_os", "target_family"]
            .iter()
            .any(|name| trimmed.contains(name))
}

/// Every `.rs` file under `directory`, depth first.
fn rust_files(directory: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();

    for entry in std::fs::read_dir(directory).expect("a directory this build compiled from") {
        let path = entry.expect("a readable directory entry").path();

        if path.is_dir() {
            found.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }

    found
}

/// **The shim never builds a `Host`.** `mixengine_platform::host()` is a trait object holding every
/// capability — the keyring, the trust stores, the PATH, the ports, `sysinfo` — and its vtable keeps
/// every implementation, so LTO keeps every DLL they import. On Windows that was a dozen (`user32`,
/// `shell32`, `crypt32`, `pdh`, `iphlpapi` …), loaded at the start and unloaded at the end of every
/// `php` a person types: ~8 ms of each run, measured on 2026-09-25, for a program that only needs to
/// know where the home is. `paths::resolve_root_default` answers that without one.
#[test]
fn the_shim_never_builds_a_host() {
    let shim = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR")))
        .parent()
        .expect("crates/")
        .join("mixengine-shim")
        .join("src");

    for file in rust_files(&shim) {
        let text = std::fs::read_to_string(&file).expect("a source file");
        assert!(
            !text.contains("mixengine_platform::host("),
            "{} builds a platform Host; use `paths::resolve_root_default` instead",
            file.display()
        );
    }
}

/// **Nothing that fetches reaches the shim.** ADR 0067: the shim reads the database itself and links
/// no HTTP client, so `mixengine-core` comes without `network`. `cargo metadata` cannot answer this:
/// its resolve is the workspace's, with `network` unified on by the daemon. `cargo tree -p` resolves
/// the shim's features alone, which is what `packaging/stage.sh` builds it with.
///
/// `sqlx` is asserted present rather than absent. The shim keeps it on purpose, and a tree that
/// printed nothing at all would otherwise pass for one without `reqwest` in it.
#[test]
fn nothing_that_fetches_reaches_the_shim() {
    let workspace = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR")))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the workspace root");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());

    let output = std::process::Command::new(cargo)
        .current_dir(workspace)
        .args([
            "tree",
            "-p",
            "mixengine-shim",
            "-e",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
            "--locked",
            "--offline",
        ])
        .output()
        .expect("cargo runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let names: BTreeSet<&str> = stdout
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert!(
        names.contains("mixengine-core") && names.contains("sqlx"),
        "cargo tree printed no tree to check:\n{stdout}"
    );
    for forbidden in ["reqwest", "hyper"] {
        assert!(
            !names.contains(forbidden),
            "{forbidden} reaches mixengine-shim; ADR 0067 keeps every fetch out of it"
        );
    }
}
