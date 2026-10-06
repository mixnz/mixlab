//! Removing MixLab from a Mac the `.pkg` placed it on — roadmap task **T182a**, spec D1 and D4.
//!
//! The shell's, not the `mixengine` module's: a person who never started MixEngine removes MixLab
//! too. What it needs from MixEngine it asks `crate::modules::mixengine::for_uninstall` for, and
//! nothing here names a MixEngine crate (`tests/layering.rs`).

pub mod commands;
pub mod menu;

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::updater::placement::Placement;

/// Whether this window offers the removal: a released build, on macOS, that the `.pkg` placed.
/// Everywhere else something else removes the program (spec D1, D6).
pub fn offered(os: &str, debug_build: bool, placement: &Placement) -> bool {
    !debug_build
        && os == "macos"
        && matches!(placement, Placement::Installer { installer, .. } if installer == "pkg")
}

/// The directories of the window's own the report says the daemon removed, or removes as it exits:
/// the rows `window_data` and `window_cache` whose outcome is `removed` or `on_exit`.
pub fn own_directories(report: &Value) -> Vec<PathBuf> {
    report["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|row| matches!(row["id"].as_str(), Some("window_data" | "window_cache")))
                .filter(|row| {
                    matches!(
                        row["outcome"]["removal"].as_str(),
                        Some("removed" | "on_exit")
                    )
                })
                .filter_map(|row| row["location"].as_str().map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the daemon said the uninstall finished: nothing still waiting for permission and nothing
/// left behind. Only a finished one ends the daemon (ADR 0051, decision 1), so only then is there
/// an ending to wait for — `mix uninstall`'s `finished_uninstall`, read off the same report.
pub fn finished(report: &Value) -> bool {
    !report["items"].as_array().is_some_and(|items| {
        items.iter().any(|row| {
            matches!(
                row["outcome"]["removal"].as_str(),
                Some("enqueued" | "failed")
            )
        })
    })
}

/// What the window can see for itself once the daemon has gone (spec D4, step 6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadBack {
    /// The bundle this window was started from is still on disk.
    pub bundle_left: bool,
    /// The package database still holds the receipt.
    pub receipt_left: bool,
    /// The window's own directories that could not be removed again.
    pub left: Vec<String>,
}

/// Remove the window's own directories once more, then read the bundle and the receipt.
pub fn read_back(bundle: &Path, receipt: &str, own: &[PathBuf]) -> ReadBack {
    let left = own
        .iter()
        .filter(|directory| match std::fs::remove_dir_all(directory) {
            Ok(()) => false,
            Err(error) => error.kind() != std::io::ErrorKind::NotFound,
        })
        .map(|directory| directory.display().to_string())
        .collect();

    ReadBack {
        bundle_left: bundle.symlink_metadata().is_ok(),
        receipt_left: receipt_known(receipt),
        left,
    }
}

/// `pkgutil --pkg-info`: exit 0 is a receipt the database still holds. A machine with no `pkgutil`
/// holds no receipts.
fn receipt_known(receipt: &str) -> bool {
    let mut command = std::process::Command::new("/usr/sbin/pkgutil");
    command.args(["--pkg-info", receipt]);
    crate::platform::hide_console(&mut command)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// What `uninstall_run` hands the dialog when the window is still here to draw it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// The daemon's report, as the plan's rows settled.
    pub report: Value,
    /// Whether the daemon's process ended.
    pub gone: bool,
    pub bundle_left: bool,
    pub receipt_left: bool,
    pub left: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The webview writes its storage without being asked, so a directory the daemon removed may
    /// be back by the time the window exits: it is removed once more, and not reported.
    #[test]
    fn the_windows_own_directories_are_removed_again_before_exit() {
        let root = tempfile::tempdir().expect("a directory");
        let data = root.path().join("data");
        std::fs::create_dir_all(data.join("WebKit")).expect("a directory that came back");

        let read = read_back(
            &root.path().join("MixLab.app"),
            "no.such.receipt",
            std::slice::from_ref(&data),
        );

        assert!(!read.bundle_left);
        assert!(!read.receipt_left);
        assert!(read.left.is_empty(), "{:?}", read.left);
        assert!(!data.exists());
    }

    #[test]
    fn a_bundle_still_there_is_reported() {
        let root = tempfile::tempdir().expect("a directory");
        let bundle = root.path().join("MixLab.app");
        std::fs::create_dir_all(&bundle).expect("the bundle");

        assert!(read_back(&bundle, "no.such.receipt", &[]).bundle_left);
    }

    /// Only the rows the daemon removed or will remove as it exits: a kept directory stays kept.
    #[test]
    fn the_report_names_the_windows_own_directories() {
        let report = serde_json::json!({ "items": [
            { "id": "window_data", "location": "/u/Library/Application Support/x", "outcome": { "removal": "on_exit", "what": "" } },
            { "id": "window_cache", "location": "/u/Library/Caches/x", "outcome": { "removal": "removed", "what": "" } },
            { "id": "home", "location": "/u/home", "outcome": { "removal": "on_exit", "what": "" } },
            { "id": "window_cache", "location": "/u/kept", "outcome": { "removal": "kept", "because": "" } },
        ]});

        assert_eq!(
            own_directories(&report),
            vec![
                PathBuf::from("/u/Library/Application Support/x"),
                PathBuf::from("/u/Library/Caches/x"),
            ]
        );
    }

    /// A declined prompt or a row left behind does not finish the uninstall, and the daemon stays
    /// up for the next run (ADR 0051): the window must not wait two minutes for it to end.
    #[test]
    fn only_a_report_with_nothing_waiting_or_left_is_finished() {
        let report = |removal: &str| {
            serde_json::json!({ "items": [
                { "id": "home", "location": "/h", "outcome": { "removal": "on_exit", "what": "" } },
                { "id": "hosts_block", "location": "/etc/hosts", "outcome": { "removal": removal, "what": "" } },
            ]})
        };

        assert!(finished(&report("removed")));
        assert!(finished(&report("absent")));
        assert!(!finished(&report("enqueued")), "a declined prompt");
        assert!(!finished(&report("failed")), "a row still there");
    }

    /// The item exists only on a released macOS copy the `.pkg` placed.
    #[test]
    fn the_item_is_offered_only_on_a_pkg_copy_of_a_release() {
        use crate::updater::placement::Placement;
        let pkg = Placement::Installer {
            installer: "pkg".to_owned(),
            package: None,
        };
        let deb = Placement::Installer {
            installer: "deb".to_owned(),
            package: Some("mixlab".to_owned()),
        };

        assert!(offered("macos", false, &pkg));
        assert!(!offered("macos", true, &pkg), "a debug build");
        assert!(!offered("linux", false, &deb));
        assert!(!offered("macos", false, &Placement::Elsewhere));
    }
}
