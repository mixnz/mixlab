//! The desktop crate reaches into the MixEngine workspace through two crates and no more — ADR
//! 0027, rule 1. The root workspace's `workspace_layering.rs` cannot see this crate, because this
//! crate is excluded from that workspace on purpose (the design's D1), so the guard lives here and
//! reads its own manifest.

use std::collections::BTreeSet;

const MANIFEST: &str = include_str!("../Cargo.toml");

/// Crates this manifest may name as ordinary dependencies by `path`.
const ALLOWED: &[&str] = &["mixengine-proto", "mixengine-platform"];

/// And the one it may name only under `[dev-dependencies]` — fixtures, never shipped.
const DEV_ONLY: &str = "mixengine-testkit";

/// The names in one `[…dependencies]` table that are declared with a `path`.
fn path_dependencies(table: &toml::Value) -> BTreeSet<String> {
    table
        .as_table()
        .into_iter()
        .flat_map(|deps| deps.iter())
        .filter(|(_, spec)| spec.get("path").is_some())
        .map(|(name, _)| name.clone())
        .collect()
}

/// Every table of the given kind: the plain one, and one per `[target.'cfg(…)']` block.
fn tables<'a>(manifest: &'a toml::Value, key: &str) -> Vec<&'a toml::Value> {
    let mut found = Vec::new();
    if let Some(table) = manifest.get(key) {
        found.push(table);
    }
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            if let Some(table) = target.get(key) {
                found.push(table);
            }
        }
    }
    found
}

fn allowed() -> BTreeSet<String> {
    ALLOWED.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn the_desktop_crate_depends_on_proto_and_platform_and_nothing_else_here() {
    let manifest: toml::Value = toml::from_str(MANIFEST).expect("Cargo.toml parses");
    for key in ["dependencies", "build-dependencies"] {
        for table in tables(&manifest, key) {
            let extra: Vec<String> = path_dependencies(table)
                .difference(&allowed())
                .cloned()
                .collect();
            assert!(
                extra.is_empty(),
                "[{key}] names path dependencies outside ADR 0027's allowance: {extra:?}"
            );
        }
    }
}

#[test]
fn testkit_may_only_ever_be_a_dev_dependency() {
    let manifest: toml::Value = toml::from_str(MANIFEST).expect("Cargo.toml parses");
    let mut dev_allowed = allowed();
    dev_allowed.insert(DEV_ONLY.to_owned());
    for table in tables(&manifest, "dev-dependencies") {
        let extra: Vec<String> = path_dependencies(table)
            .difference(&dev_allowed)
            .cloned()
            .collect();
        assert!(
            extra.is_empty(),
            "[dev-dependencies] names path dependencies outside the allowance: {extra:?}"
        );
    }
    for key in ["dependencies", "build-dependencies"] {
        for table in tables(&manifest, key) {
            assert!(
                !path_dependencies(table).contains(DEV_ONLY),
                "{DEV_ONLY} is a dev-dependency and never anything else"
            );
        }
    }
}

/// ADR 0056 rule 3: MixLab's updater is MixLab's code. Nothing under `src/updater/` names a
/// MixEngine crate, except inside a file's test module, which may check a contract against one.
#[test]
fn the_updater_names_no_mixengine_crate() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/updater");
    for entry in std::fs::read_dir(&root).expect("src/updater exists") {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let shipped = text.split("#[cfg(test)]").next().unwrap_or_default();
        assert!(
            !shipped.contains("mixengine_"),
            "{} names a mixengine crate outside its tests (ADR 0056 rule 3)",
            path.display()
        );
    }
}

/// T182a: removing MixLab is the shell's, as updating it is (ADR 0056 rule 3). Nothing under
/// `src/uninstall/` names a MixEngine crate; what it needs from the daemon it asks
/// `modules::mixengine::for_uninstall` for.
#[test]
fn the_removal_names_no_mixengine_crate() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/uninstall");
    for entry in std::fs::read_dir(&root).expect("src/uninstall exists") {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let shipped = text.split("#[cfg(test)]").next().unwrap_or_default();
        assert!(
            !shipped.contains("mixengine_"),
            "{} names a mixengine crate outside its tests (ADR 0056 rule 3)",
            path.display()
        );
    }
}
