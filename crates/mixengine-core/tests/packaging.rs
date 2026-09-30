//! `packaging/common.sh`'s two arrays, against the names this crate actually looks for.
//!
//! **Roadmap task T85c is what this file exists to stop happening a second time.** `MIX_BINARIES`
//! named three binaries and [`mixengine_core::shims::source`] looked for a fourth beside the
//! running `mixengined`, so every release built from those scripts installed cleanly, started,
//! reported itself healthy, and could not run `php` — the `bin/` it fills was empty and
//! `Error::ShimMissing` was the only sign.
//!
//! The shape is [`mixengine_core::updates`]' own: `include_str!` the committed file, so one that is
//! deleted or moved is a build error rather than a test that reads nothing and passes.

use std::collections::BTreeSet;

/// What `packaging/stage.sh` sources, read at compile time.
const COMMON_SH: &str = include_str!("../../../packaging/common.sh");

/// The workspace manifest, for the membership check below.
const WORKSPACE: &str = include_str!("../../../Cargo.toml");

/// The desktop entry the `.deb` and the `.rpm` install, which is what registers `mixlab://` on Linux.
const MIXLAB_DESKTOP: &str = include_str!("../../../packaging/linux/mixlab.desktop");

/// The entries of a one-line bash array declared in `packaging/common.sh`.
///
/// Panics rather than returning an empty set when the declaration is not there: an array that
/// stopped being declared is a packaging pipeline that stopped working, and a test that quietly
/// compared nothing to nothing would be the failure it is meant to catch.
fn declared(array: &str) -> BTreeSet<String> {
    let opening = format!("{array}=(");

    let line = COMMON_SH
        .lines()
        .find(|line| line.starts_with(&opening))
        .unwrap_or_else(|| panic!("packaging/common.sh declares {array} on one line"));

    line[opening.len()..]
        .trim_end_matches(')')
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// The value of a plain `NAME=value` assignment in `packaging/common.sh`.
///
/// Panics rather than returning `None` for the same reason [`declared`] does: a variable that
/// stopped being declared is a packaging pipeline that stopped working, and every script that reads
/// it would silently treat the window as one more ordinary binary.
fn assigned(name: &str) -> String {
    let opening = format!("{name}=");

    COMMON_SH
        .lines()
        .find(|line| line.starts_with(&opening))
        // Both quotation marks: `MIX_INSTALL_WINDOWS` is single-quoted so bash leaves its
        // backslashes alone, and every other assignment here is bare or double-quoted.
        .map(|line| line[opening.len()..].trim_matches(['"', '\'']).to_owned())
        .unwrap_or_else(|| panic!("packaging/common.sh assigns {name} on one line"))
}

/// The `[package].name` of the desktop application's manifest — the name `cargo` gives the
/// executable, and therefore the name every installer places.
fn desktop_package_name() -> String {
    let parsed: toml::Table = DESKTOP_CARGO_TOML
        .parse()
        .expect("apps/desktop/src-tauri/Cargo.toml is not TOML");

    parsed["package"]["name"]
        .as_str()
        .expect("apps/desktop/src-tauri/Cargo.toml's [package] has no string name")
        .to_owned()
}

/// Every name a release has to contain is one the packaging scripts put in it.
///
/// Set equality and not "contains", because the failure being prevented is a list that drifted —
/// and a list somebody shortened drifts exactly as badly as one nobody lengthened.
#[test]
fn the_release_ships_every_binary_this_code_looks_for() {
    let expected: BTreeSet<String> = [
        // The CLI. The one of the five with no constant to borrow — nothing in `core` resolves it
        // by name — so it is spelled here and nowhere else.
        "mix".to_owned(),
        mixengine_core::updates::apply::SMOKE_EXECUTABLE.to_owned(),
        mixengine_core::shims::BINARY.to_owned(),
        // T185: what `<root>/bin` holds per name on Windows, looked for beside the daemon.
        mixengine_core::shims::TRAMPOLINE.to_owned(),
        // The helper beside the program, which the daemon installs the privileged copy from.
        "mixengine-elevate".to_owned(),
        // The window — T105. Read out of the manifest that produces the file rather than spelled
        // here: `cargo` names the executable after `[package].name` and Tauri leaves that alone
        // (`mainBinaryName` is unset), so this is the one string that cannot drift from the file
        // the installers place.
        desktop_package_name(),
    ]
    .into_iter()
    .collect();

    assert_eq!(
        declared("MIX_BINARIES"),
        expected,
        "packaging/common.sh's MIX_BINARIES and the names this crate looks for have drifted \
         apart; the array is what every packaging script stages and checks, so a release cut \
         while they differ is one that installs and then cannot do what it was installed for"
    );
}

/// And every crate the stage builds is one that exists — one of them not where the others are.
///
/// A typo here is otherwise a `cargo build -p` failure seven minutes into a packaging run, on five
/// runners at once.
///
/// **The desktop application's crate is the exception and has to be**: it is a workspace of its own
/// that this one `exclude`s (ADR 0027, rule 5), so `cargo build -p mixlab` at this root is an error
/// rather than a build — which is why `packaging/stage.sh` hands it to `packaging/desktop.sh`
/// instead. What is checked for it is that the root manifest really does exclude that directory and
/// that the name in `MIX_CRATES` is the one its own manifest gives it.
#[test]
fn every_crate_the_stage_builds_is_a_workspace_member() {
    let desktop = desktop_package_name();

    for name in declared("MIX_CRATES") {
        if name == desktop {
            assert!(
                WORKSPACE.contains("\"apps/desktop/src-tauri\""),
                "packaging/common.sh's MIX_CRATES names {name}, the desktop application's crate, \
                 which the root Cargo.toml neither includes nor excludes"
            );
            continue;
        }

        assert!(
            WORKSPACE.contains(&format!("\"crates/{name}\"")),
            "packaging/common.sh's MIX_CRATES names {name}, which is not a member of this \
             workspace"
        );
    }
}

/// `MIX_WINDOW` names the fifth entry, and the headless archives are the other four.
///
/// **What this stops.** Every headless artifact is `MIX_BINARIES` minus this one name. A
/// `MIX_WINDOW` that named nothing in the array — a rename on one side only — would make every
/// headless archive byte-identical to the payload beside it: a file whose whole purpose is not
/// carrying a webview, quietly carrying one, with no script anywhere in a position to notice.
#[test]
fn the_window_is_one_of_the_binaries_and_is_named_as_such() {
    let window = assigned("MIX_WINDOW");

    assert_eq!(
        window,
        desktop_package_name(),
        "packaging/common.sh's MIX_WINDOW and the desktop application's [package] name have \
         drifted apart"
    );
    assert!(
        declared("MIX_BINARIES").contains(&window),
        "packaging/common.sh's MIX_WINDOW is {window}, which is not one of MIX_BINARIES"
    );
}

/// The two names `updates::apply` holds for the window are the two `packaging/common.sh` declares.
///
/// **What this stops.** `swap` resolves every other payload name by appending this platform's
/// executable suffix; the window is the one entry whose on-disk name that rule does not produce, so
/// it is looked up by these two constants instead. A rename on the packaging side alone would give a
/// feed whose `provides` names something no installed copy ever looks for — no error, no log line,
/// and a window that is never updated again. It is the failure T105 argued the executable's spelling
/// out of, one layer down.
#[test]
fn the_window_is_named_the_same_on_both_sides() {
    assert_eq!(
        mixengine_core::updates::apply::WINDOW,
        assigned("MIX_WINDOW"),
        "updates::apply::WINDOW and packaging/common.sh's MIX_WINDOW have drifted apart; the \
         constant is what an installed MixEngine looks for beside its binaries, and the variable is \
         what every packaging script stages under"
    );
    assert_eq!(
        mixengine_core::updates::apply::WINDOW_BUNDLE,
        assigned("MIX_WINDOW_APP"),
        "updates::apply::WINDOW_BUNDLE and packaging/common.sh's MIX_WINDOW_APP have drifted apart; \
         on macOS that name is the whole of what the swap replaces"
    );
}

/// This system's install location is the one its packaging script writes to — roadmap task
/// **T107**.
///
/// **What this stops.** An installer that moved and a lookup that did not is a window whose Start
/// Menu shortcut works and whose `mix database open` says *MixLab is not installed*, on the machine
/// it is installed on — and a daemon the window offers to install for somebody who already has it.
/// The same failure [`the_window_is_named_the_same_on_both_sides`] was written for, one directory
/// up.
///
/// **`ends_with` and not equality on Windows**, where the base is a folder each side asks its own
/// operating system for: what can drift is the sub-path under it, and that is what is compared.
#[test]
fn this_systems_install_location_is_the_one_packaging_writes_to() {
    let dirs = mixengine_platform::install::program_dirs();
    let first = dirs.first().expect("every supported system names one");

    let declared = assigned(if cfg!(windows) {
        "MIX_INSTALL_WINDOWS"
    } else if cfg!(target_os = "macos") {
        "MIX_INSTALL_MACOS"
    } else {
        "MIX_INSTALL_LINUX"
    });

    assert!(
        first.ends_with(declared.replace('\\', std::path::MAIN_SEPARATOR_STR)),
        "mixengine-platform installs into {} and packaging/common.sh declares {declared}; a \
         release that moved one without the other is a MixEngine every lookup misses",
        first.display()
    );
}

/// The desktop entry that makes a `mixlab://` link reach the window names the scheme the daemon
/// writes into every handoff URL — roadmap task **T107**.
///
/// **What this stops.** The daemon composes `<scheme>://connect?…` and starts the window with it;
/// the `.desktop` file is what makes the operating system hand a link over at all; `handoff.rs`
/// refuses any other scheme. Renaming one of the three leaves a URL nobody answers — and on the two
/// paths that matter it fails silently, as a window that opens with no tab in it.
#[test]
fn the_window_answers_the_scheme_its_desktop_entry_registers() {
    let entry = MIXLAB_DESKTOP
        .lines()
        .find_map(|line| line.strip_prefix("MimeType=x-scheme-handler/"))
        .expect("packaging/linux/mixlab.desktop declares one scheme handler")
        .trim()
        .trim_end_matches(';');

    assert_eq!(
        mixengine_core::window::SCHEME,
        entry,
        "window::SCHEME and packaging/linux/mixlab.desktop have drifted apart; the constant is what \
         the daemon writes into a handoff URL, and the entry is what makes the system hand that URL \
         to the window"
    );
}

/// The window's display name is the product name the application is built under.
#[test]
fn the_window_is_called_what_it_is_built_as() {
    let parsed: serde_json::Value =
        serde_json::from_str(DESKTOP_TAURI_CONF).expect("tauri.conf.json is not JSON");

    assert_eq!(
        Some(mixengine_core::window::NAME),
        parsed
            .get("productName")
            .and_then(serde_json::Value::as_str),
        "window::NAME and tauri.conf.json's productName have drifted apart; the constant is what \
         `mix database client` prints and the manifest is what the title bar says"
    );
    assert_eq!(
        Some(mixengine_core::window::IDENTIFIER),
        parsed.get("identifier").and_then(serde_json::Value::as_str),
        "window::IDENTIFIER and tauri.conf.json's identifier have drifted apart; the constant is \
         how `mix uninstall` finds the window's own folders, and the manifest is where Tauri puts them"
    );
}

/// The three files the desktop build reads its version out of, read at compile time.
///
/// None of them can inherit from `[workspace.package]`: the desktop application's crate is a
/// workspace of its own, excluded from this one (ADR 0027, and the design's D1). So the
/// inheritance is the test below — see the T104 design's D2. `include_str!` for the same reason as
/// the two constants above: a file that moved is a build error rather than a test that reads
/// nothing and passes.
const DESKTOP_PACKAGE_JSON: &str = include_str!("../../../apps/desktop/package.json");
/// The fourth: `npm ci` is what CI installs with, and it reads this file rather than the manifest
/// beside it.
const DESKTOP_PACKAGE_LOCK: &str = include_str!("../../../apps/desktop/package-lock.json");
const DESKTOP_TAURI_CONF: &str = include_str!("../../../apps/desktop/src-tauri/tauri.conf.json");
const DESKTOP_CARGO_TOML: &str = include_str!("../../../apps/desktop/src-tauri/Cargo.toml");

/// The `version` at the top level of a JSON document.
fn json_version(document: &str, what: &str) -> String {
    let parsed: serde_json::Value =
        serde_json::from_str(document).unwrap_or_else(|e| panic!("{what} is not JSON: {e}"));
    parsed
        .get("version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{what} has no top-level string `version`"))
        .to_owned()
}

/// The `version` of a manifest's `[package]` or `[workspace.package]` table.
fn toml_version(manifest: &str, table: &[&str], what: &str) -> String {
    // `Table` rather than `Value`: `Value`'s own `FromStr` reads a TOML *value*, and a manifest is
    // a document — it stops at the first section header with "unexpected content".
    let parsed: toml::Table = manifest
        .parse()
        .unwrap_or_else(|e| panic!("{what} is not TOML: {e}"));
    let document = toml::Value::Table(parsed);

    let mut at = &document;
    for key in table {
        at = at
            .get(key)
            .unwrap_or_else(|| panic!("{what} has no [{}] table", table.join(".")));
    }

    at.get("version")
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("{what}'s [{}] has no string version", table.join(".")))
        .to_owned()
}

/// The window reports the version of the release it was cut with, and never one of its own.
///
/// **What this stops.** `apps/desktop` carried the standalone client's own version through phase 11, and a bump of
/// the root manifest could not reach it — the crate is not a member of this workspace and cannot
/// inherit from it. A window built from a release tag while one of these three disagreed would
/// name the wrong version in Settings, offer the wrong one to the updater and put it in every
/// crash report, none of which is visible until somebody reads a support thread. Cutting a release
/// stays a bump of the root `Cargo.toml`; this is what makes that bump enough.
#[test]
fn the_desktop_application_carries_the_workspace_version() {
    let workspace = toml_version(WORKSPACE, &["workspace", "package"], "the root Cargo.toml");

    for (what, found) in [
        (
            "apps/desktop/package.json",
            json_version(DESKTOP_PACKAGE_JSON, "apps/desktop/package.json"),
        ),
        (
            "apps/desktop/package-lock.json",
            json_version(DESKTOP_PACKAGE_LOCK, "apps/desktop/package-lock.json"),
        ),
        (
            "apps/desktop/src-tauri/tauri.conf.json",
            json_version(DESKTOP_TAURI_CONF, "apps/desktop/src-tauri/tauri.conf.json"),
        ),
        (
            "apps/desktop/src-tauri/Cargo.toml",
            toml_version(
                DESKTOP_CARGO_TOML,
                &["package"],
                "apps/desktop/src-tauri/Cargo.toml",
            ),
        ),
    ] {
        assert_eq!(
            found, workspace,
            "{what} says {found} and the workspace says {workspace}; the desktop application's \
             version is the workspace's, and a release cut while the two differ is a window that \
             reports the wrong one"
        );
    }
}

/// The Linux install page, in each language, and the operations page that states the same floors.
const INSTALL_EN: &str = include_str!("../../../docs/guide/en/install.md");
const INSTALL_VI: &str = include_str!("../../../docs/guide/vi/install.md");
const BUILD_AND_RELEASE: &str = include_str!("../../../docs/operations/build-and-release.md");

/// Every document that promises the window's floor promises the one `packaging/common.sh` declares.
///
/// **T105a, and the reason this is a test rather than a habit.** ADR 0028 settled that the AppImage
/// does not carry WebKitGTK, which turns the floor into a promise to a person rather than an
/// implementation detail: three documents name a glibc version, two of them name three package
/// names, `packaging/linux/window-floor.sh` holds the binary to the first of those on every Linux
/// build leg — and nothing but this connects the two ends. A floor raised in `common.sh` without the
/// documents following is somebody told their distribution is supported by a page, and told
/// otherwise by a window that does not open.
#[test]
fn every_document_promises_the_floor_the_packaging_declares() {
    let glibc = format!("glibc {}", assigned("MIX_WINDOW_GLIBC"));

    for (what, text) in [
        ("docs/guide/en/install.md", INSTALL_EN),
        ("docs/guide/vi/install.md", INSTALL_VI),
        ("docs/operations/build-and-release.md", BUILD_AND_RELEASE),
    ] {
        assert!(
            text.contains(&glibc),
            "{what} does not say '{glibc}', which is the floor packaging/common.sh declares"
        );
    }

    // The three package names are one fact spelled three ways, and the fact is the API version in
    // the soname the window links. The day a Tauri release moves to the `webkitgtk-6.0` API, every
    // one of those names is wrong — silently, in a document, on the one page a person reads when
    // nothing works. `window-floor.sh` refuses the build; this refuses the documents.
    let soname = assigned("MIX_WINDOW_WEBKIT");
    let api = soname
        .strip_prefix("libwebkit2gtk-")
        .and_then(|rest| rest.split(".so").next())
        .unwrap_or_else(|| {
            panic!("MIX_WINDOW_WEBKIT is {soname}, which is not a webkit2gtk soname")
        });

    let packages = [
        format!("libwebkit2gtk-{api}-0"),
        format!("webkit2gtk{api}"),
        format!("libwebkit2gtk-{}-0", api.replace('.', "_")),
    ];

    for (what, text) in [
        ("docs/guide/en/install.md", INSTALL_EN),
        ("docs/guide/vi/install.md", INSTALL_VI),
    ] {
        for package in &packages {
            assert!(
                text.contains(package),
                "{what} does not name the package {package}, which MIX_WINDOW_WEBKIT says is what \
                 the window needs"
            );
        }
    }
}

/// Every download link on the install pages carries the prefix of what it installs — T176f,
/// ADR 0049.
///
/// **The handbook links unversioned aliases, and nothing but this connects them to the scripts that
/// publish them.** A link left on the old prefix is a `404` on the one page a person reads before
/// they have anything installed, and the release itself would look perfectly healthy. The test
/// cannot tell whether a file exists; it catches the drift a rename leaves behind, which is one
/// link, in one language, still spelled the old way.
#[test]
fn every_download_link_carries_the_prefix_of_what_it_installs() {
    const LINK: &str = "releases/latest/download/";

    let product = assigned("MIX_ARTIFACT");
    let headless = assigned("MIX_HEADLESS_ARTIFACT");

    for (what, text) in [
        ("docs/guide/en/install.md", INSTALL_EN),
        ("docs/guide/vi/install.md", INSTALL_VI),
    ] {
        let names: Vec<&str> = text
            .match_indices(LINK)
            .map(|(at, _)| {
                let rest = &text[at + LINK.len()..];
                rest.split([')', ' ', '\n']).next().unwrap_or(rest)
            })
            .collect();

        assert!(!names.is_empty(), "{what} links no download at all");

        for name in names {
            let prefix = if name.contains("-headless") {
                &headless
            } else {
                &product
            };
            let rest = name.strip_prefix(prefix.as_str());
            assert!(
                rest.is_some_and(|rest| rest.starts_with(['-', '_'])),
                "{what} links {name}, which does not start with {prefix}; packaging/common.sh \
                 names an artifact with the window MIX_ARTIFACT and one without it \
                 MIX_HEADLESS_ARTIFACT"
            );
        }
    }
}
