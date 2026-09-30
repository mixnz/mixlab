//! Bringing a standalone-client user's data across, once, on the first launch of MixLab.
//!
//! A changed bundle identifier is a changed application-data directory and a changed keyring
//! namespace, so a person who was using the standalone client opens a window that has never seen any of their
//! saved connections. This module is what puts them back. It is written to the T104 design's D5,
//! which is worth reading before changing anything here; the three rules it turns on are:
//!
//! - **The old directory and the old keyring entries are never written or deleted.** A standalone
//!   The standalone client may still be installed and still be in use, and this is somebody's data either way.
//! - **It runs once.** A marker file in the new directory says that it has, and the presence of
//!   that file alone is what a second launch reads.
//! - **Nothing from the webview's `localStorage` comes across** — theme, accent, the tab strip of
//!   the last session, a skipped version. None of that is something a person made, and it lives in
//!   a profile keyed by the identifier that no code in this process can reach anyway.
//!
//! Two phases, because they have opposite requirements. Copying the files has to happen before
//! anything else can touch the directory, so it runs synchronously inside `setup()`: windows are
//! created before `setup` and the webview cannot deliver an IPC message until the event loop runs,
//! which is after `build()` returns, so that is the one moment in which no `Store.load` can race
//! it. Copying the credentials must not hold the window shut, because on macOS the first read of
//! The standalone client's items raises a Keychain authorization dialog for an application the Keychain has never
//! seen — so it runs on a thread of its own.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

/// The bundle identifier the standalone client is installed under.
///
/// Tauri keys the application-data directory on the identifier and on nothing else, so the standalone client's
/// directory is this application's own with the name swapped — checked on Windows against a real
/// install, [`LEGACY_IDENTIFIER`] under `%APPDATA%` beside `%APPDATA%\io.github.mixnz.mixlab`.
const LEGACY_IDENTIFIER: &str = "io.github.haiquang9994.mixdb";

/// Written into the new directory when the import has run.
///
/// Its presence is the whole of "do not look again". Its contents are for whoever reads a support
/// thread, and for T108, which starts a user whose data came from the standalone client on the *Everything*
/// profile rather than on *MixEngine*.
pub const MARKER: &str = "mixdb-import.json";

/// The largest file this copies. Nothing the store plugin writes comes anywhere near it; the cap
/// is here because this runs on the startup path and a pathological file must not hold the window
/// shut.
const MAX_FILE: u64 = 64 * 1024 * 1024;

/// How deep `ids_in` walks. `rest-history.json` is the deepest of these files by a long way and is
/// nowhere near this; the bound is against a file that was not written by this application.
const MAX_DEPTH: usize = 32;

/// The prefix the REST module files an environment's secrets under — `modules/rest/api.ts`.
const REST_ENV_PREFIX: &str = "rest-env:";

/// What the marker file holds.
#[derive(Serialize, Deserialize)]
struct Marker {
    /// The shape of this document, so a later reader can refuse one it does not understand.
    version: u32,
    #[serde(rename = "importedAt")]
    imported_at: String,
    /// The directory it came from, as it was on the day.
    source: String,
    files: Vec<String>,
    accounts: usize,
    credentials: Credentials,
}

/// How the credential half went: written `Pending` by the synchronous phase and replaced by the
/// thread that does the reading.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Credentials {
    Pending,
    Done { copied: usize, failed: usize },
}

/// What the synchronous phase hands the thread.
struct Plan {
    dir: PathBuf,
    accounts: Vec<String>,
    marker: Marker,
}

/// The standalone client's application-data directory, given this application's own.
fn legacy_dir(app_data: &Path) -> Option<PathBuf> {
    let legacy = app_data.parent()?.join(LEGACY_IDENTIFIER);
    (legacy != app_data).then_some(legacy)
}

/// Whether this is one of the files the import carries.
///
/// By pattern rather than by list, so a store a module adds later comes across too. A leading dot
/// is what keeps `.window-state.json` — the window plugin's maximized flag, which is chrome and
/// not anything a person made — out of it.
fn is_store_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    !name.starts_with('.') && name != MARKER && name.ends_with(".json")
}

/// The store files at the top of `dir`, in a stable order.
///
/// A directory that cannot be read is an empty one: there is nothing to import from it, and
/// nothing has gone wrong that the user could act on.
fn store_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| is_store_file(path))
        .collect();
    files.sort();
    files
}

/// Every string under an `"id"` key, at any depth.
fn ids_in(value: &Value, into: &mut BTreeSet<String>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                // Nested rather than a `let` chain: this crate is on edition 2021.
                if key == "id" {
                    if let Value::String(id) = child {
                        if !id.is_empty() {
                            into.insert(id.clone());
                        }
                    }
                }
                ids_in(child, into, depth + 1);
            }
        }
        Value::Array(items) => {
            for item in items {
                ids_in(item, into, depth + 1);
            }
        }
        _ => {}
    }
}

/// The keyring accounts those ids can be filed under.
///
/// Both shapes for every id rather than a rule per file: `db` and `terminal` use the id itself and
/// `rest` prefixes it, and a module that invents a third shape would be a silent loss the day it
/// ships. An account that is not there reads as "nothing stored" and costs one lookup.
fn accounts_of(ids: &BTreeSet<String>) -> Vec<String> {
    ids.iter()
        .flat_map(|id| [id.clone(), format!("{REST_ENV_PREFIX}{id}")])
        .collect()
}

/// The synchronous half: the store files, and the marker.
///
/// `None` — nothing to do — for every one of: the marker is already there, the new directory
/// already holds a store file, there is no directory above this one, the standalone client's is not there, or it
/// holds no store files. None of those is a failure.
fn copy_stores(new_dir: &Path) -> Option<Plan> {
    if new_dir.join(MARKER).exists() || !store_files(new_dir).is_empty() {
        return None;
    }
    let old_dir = legacy_dir(new_dir)?;
    let files = store_files(&old_dir);
    if files.is_empty() {
        return None;
    }
    if let Err(e) = std::fs::create_dir_all(new_dir) {
        log::warn!("import: {} could not be created: {e}", new_dir.display());
        return None;
    }

    let mut copied = Vec::new();
    let mut ids = BTreeSet::new();
    for file in &files {
        let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let target = new_dir.join(name);
        if target.exists() {
            continue;
        }
        match std::fs::metadata(file) {
            Ok(meta) if meta.len() > MAX_FILE => {
                log::warn!("import: {name} is larger than this copies, and was left behind");
                continue;
            }
            Ok(_) => {}
            Err(e) => {
                log::warn!("import: {name} could not be looked at: {e}");
                continue;
            }
        }
        if let Err(e) = std::fs::copy(file, &target) {
            log::warn!("import: {name} could not be copied: {e}");
            continue;
        }
        copied.push(name.to_owned());

        // A file that will not parse is still the user's and is still copied; it only names no
        // accounts.
        match std::fs::read_to_string(&target)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()))
        {
            Ok(document) => ids_in(&document, &mut ids, 0),
            Err(e) => {
                log::warn!("import: {name} is not readable as JSON, so it named no accounts: {e}");
            }
        }
    }

    let marker = Marker {
        version: 1,
        imported_at: chrono::Utc::now().to_rfc3339(),
        source: old_dir.display().to_string(),
        files: copied,
        accounts: ids.len(),
        credentials: Credentials::Pending,
    };
    write_marker(new_dir, &marker);

    Some(Plan {
        dir: new_dir.to_path_buf(),
        accounts: accounts_of(&ids),
        marker,
    })
}

/// Puts the marker in place.
///
/// A marker that cannot be written is logged and nothing more: the import has already happened,
/// and the store files now in the directory are what stop it happening again.
fn write_marker(dir: &Path, marker: &Marker) {
    match serde_json::to_string_pretty(marker) {
        Ok(json) => {
            if let Err(e) = std::fs::write(dir.join(MARKER), json) {
                log::warn!("import: the marker could not be written: {e}");
            }
        }
        Err(e) => log::warn!("import: the marker could not be built: {e}"),
    }
}

/// The half that reaches the credential store, on a thread of its own.
fn copy_credentials(mut plan: Plan) {
    let (found, failed) = crate::secrets::read_legacy(&plan.accounts);
    let mut copied = 0usize;
    let mut failures = failed.len();
    for (account, secrets) in found {
        match crate::secrets::save(&account, &secrets) {
            Ok(()) => copied += 1,
            Err(e) => {
                failures += 1;
                log::warn!("import: a credential could not be written: {e:?}");
            }
        }
    }
    log::info!("import: {copied} credentials came across, {failures} did not");

    plan.marker.credentials = Credentials::Done {
        copied,
        failed: failures,
    };
    write_marker(&plan.dir, &plan.marker);
}

/// Brings a standalone-client user's data across, if there is any and if this is the first launch.
///
/// Everything here is best effort: an import that cannot run leaves a window that opens on an
/// empty connection list, which is what a new machine looks like anyway. Nothing propagates out,
/// and nothing here can keep the window from opening.
pub fn on_first_launch<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Ok(new_dir) = crate::platform::app_data_dir(app) else {
        return;
    };
    let Some(plan) = copy_stores(&new_dir) else {
        return;
    };
    log::info!(
        "import: {} store files came from {}",
        plan.marker.files.len(),
        plan.marker.source
    );
    std::thread::spawn(move || copy_credentials(plan));
}

/// Whether the marker is in `dir`.
///
/// A free function over a path so it can be checked without a Tauri runtime, the way the rest of
/// this module is.
fn marker_in(dir: &Path) -> bool {
    dir.join(MARKER).exists()
}

/// Whether a standalone-client user's data was brought across on this machine.
///
/// T108's third question. The window asks it on exactly one launch — a webview profile with no
/// shell settings at all — to tell a fresh install from an imported one: the import copies store
/// files and deliberately copies no `localStorage`, so those two look identical from up there and
/// want opposite answers. A machine whose application-data directory cannot even be named has
/// nothing imported in it, which is `false` and not an error worth showing anyone.
#[tauri::command]
pub fn import_happened(app: AppHandle) -> bool {
    crate::platform::app_data_dir(&app).is_ok_and(|dir| marker_in(&dir))
}

#[cfg(test)]
mod tests {
    use super::{
        accounts_of, copy_stores, ids_in, is_store_file, legacy_dir, marker_in, LEGACY_IDENTIFIER,
        MARKER,
    };
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// The pattern is "a store file", not "a file": `.window-state.json` is the maximized flag the
    /// window plugin keeps, the marker is this module's own, and neither is something a person
    /// made.
    #[test]
    fn the_pattern_takes_store_files_and_leaves_the_rest() {
        assert!(is_store_file(Path::new("/x/connections.json")));
        assert!(is_store_file(Path::new("/x/known_hosts.json")));
        assert!(!is_store_file(Path::new("/x/.window-state.json")));
        assert!(!is_store_file(&Path::new("/x").join(MARKER)));
        assert!(!is_store_file(Path::new("/x/old.log")));
        assert!(!is_store_file(Path::new("/x/tools")));
    }

    /// The standalone client's directory is this one's with the identifier swapped — Tauri keys it on the
    /// identifier and on nothing else.
    #[test]
    fn the_legacy_directory_is_the_sibling_named_after_the_old_identifier() {
        let ours = Path::new("/home/a/.local/share/io.github.mixnz.mixlab");
        assert_eq!(
            legacy_dir(ours).unwrap(),
            Path::new("/home/a/.local/share").join(LEGACY_IDENTIFIER)
        );
        assert!(legacy_dir(Path::new("/")).is_none());
    }

    /// The store files say which accounts exist. An `id` anywhere in one is an account, however
    /// deeply the module that wrote it nested its own shape.
    #[test]
    fn every_id_in_a_store_file_is_an_account() {
        let document = json!({
            "saved": [
                { "id": "conn-1", "config": { "host": "localhost", "ssh": { "id": "tunnel-1" } } },
                { "id": "conn-2", "keyringRef": "mariadb/root" }
            ]
        });

        let mut ids = BTreeSet::new();
        ids_in(&document, &mut ids, 0);

        assert_eq!(
            ids,
            ["conn-1", "conn-2", "tunnel-1"]
                .into_iter()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        );
    }

    /// The REST module files an environment's secrets under `rest-env:<id>`, so both forms are
    /// asked for. One that is not there costs a single lookup that finds nothing.
    #[test]
    fn an_id_is_asked_for_under_both_shapes() {
        let ids: BTreeSet<String> = ["env-1".to_owned()].into_iter().collect();
        assert_eq!(accounts_of(&ids), vec!["env-1", "rest-env:env-1"]);
    }

    /// The whole synchronous half, over two directories: what is copied, what is not, and the
    /// marker that stops it happening twice.
    #[test]
    fn an_import_copies_the_stores_once() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join("io.github.mixnz.mixlab");
        std::fs::create_dir_all(old.join("tools")).unwrap();
        std::fs::write(
            old.join("connections.json"),
            r#"{"saved":[{"id":"conn-1","config":{"host":"localhost"}}]}"#,
        )
        .unwrap();
        std::fs::write(old.join("known_hosts.json"), r#"{"h:22":"SHA256:x"}"#).unwrap();
        std::fs::write(old.join(".window-state.json"), r#"{"main":{}}"#).unwrap();
        std::fs::write(old.join("old.log"), "noise").unwrap();

        let plan = copy_stores(&new)
            .expect("a directory with the standalone client's beside it is imported");

        assert!(new.join("connections.json").is_file());
        assert!(new.join("known_hosts.json").is_file());
        assert!(!new.join(".window-state.json").exists());
        assert!(!new.join("old.log").exists());
        assert!(new.join(MARKER).is_file());
        assert_eq!(
            plan.accounts,
            vec!["conn-1".to_owned(), "rest-env:conn-1".to_owned()]
        );

        assert!(
            copy_stores(&new).is_none(),
            "the marker is what says it has already run"
        );
    }

    /// A directory somebody is already using is not imported into, marker or no marker: whatever
    /// is in there is newer than anything the standalone client has.
    #[test]
    fn a_directory_already_in_use_is_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join("io.github.mixnz.mixlab");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("connections.json"), r#"{"saved":[]}"#).unwrap();
        std::fs::write(new.join("connections.json"), r#"{"saved":[]}"#).unwrap();

        assert!(copy_stores(&new).is_none());
        assert!(!new.join(MARKER).exists());
    }

    /// No standalone client on this machine is the ordinary case, and it is not a failure.
    #[test]
    fn nothing_to_import_is_not_a_failure() {
        let root = tempfile::tempdir().unwrap();
        let new = root.path().join("io.github.mixnz.mixlab");
        std::fs::create_dir_all(&new).unwrap();

        assert!(copy_stores(&new).is_none());
    }

    /// T108 asks one question of this module: did a standalone-client user's data come across on this machine?
    /// The marker is the whole answer, and it is written inside `setup()` — before the event loop
    /// that carries the question — so by the time the window can ask, the answer is final.
    #[test]
    fn the_marker_is_the_whole_answer() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!marker_in(dir.path()));
        std::fs::write(dir.path().join(MARKER), "{}").unwrap();
        assert!(marker_in(dir.path()));
    }
}
