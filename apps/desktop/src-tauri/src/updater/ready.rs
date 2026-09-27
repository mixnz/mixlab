//! The mark a download leaves once it is proved — roadmap task **T188**, spec D2.
//!
//! `updates/<version>/ready` is written last, after the SHA-256 check (and on Windows the unpack
//! and the smoke test), so its presence is the whole claim that *Install* has something to install.
//! It names the version and the SHA-256 it proved: a marker left by an older release, or by a
//! different file for the same version, is not this download.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE: &str = "ready";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ready {
    pub version: String,
    pub sha256: String,
}

/// The staged update is missing, stale or incomplete: it has to be downloaded again.
#[derive(Debug, PartialEq, Eq)]
pub struct NotReady;

/// Written beside and renamed into place, so a window that dies mid-write leaves no marker rather
/// than half of one.
pub fn write(staging: &Path, ready: &Ready) -> std::io::Result<()> {
    let text = serde_json::to_vec(ready).map_err(std::io::Error::other)?;
    let partial = staging.join(format!("{FILE}.partial"));
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, staging.join(FILE))
}

pub fn read(staging: &Path) -> Option<Ready> {
    serde_json::from_slice(&std::fs::read(staging.join(FILE)).ok()?).ok()
}

/// The marker, only when it names `version` and `sha256`: the proof is of one file, not of a slot.
pub fn read_for(staging: &Path, version: &str, sha256: &str) -> Option<Ready> {
    read(staging)
        .filter(|ready| ready.version == version && ready.sha256.eq_ignore_ascii_case(sha256))
}

/// Windows: the unpacked directory the swap takes its files from, when the marker is this
/// download's and every file the payload provides is still there.
pub fn staged(
    staging: &Path,
    version: &str,
    sha256: &str,
    provides: &BTreeMap<String, String>,
) -> Result<PathBuf, NotReady> {
    read_for(staging, version, sha256).ok_or(NotReady)?;
    let unpacked = staging.join("unpacked");
    if provides
        .values()
        .all(|relative| unpacked.join(relative).is_file())
    {
        Ok(unpacked)
    } else {
        Err(NotReady)
    }
}

/// At start: every download but the one on offer goes. Directories only: the records and the feed
/// cache are files beside them.
pub fn discard_stale(updates: &Path, keep: Option<&str>) {
    let Ok(entries) = std::fs::read_dir(updates) else {
        return;
    };
    for entry in entries.flatten() {
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if is_dir && keep != entry.file_name().to_str() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn ready(version: &str, sha256: &str) -> Ready {
        Ready {
            version: version.to_owned(),
            sha256: sha256.to_owned(),
        }
    }

    #[test]
    fn a_marker_reads_back_as_written() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &ready("0.0.10", "ab")).unwrap();
        assert_eq!(read(dir.path()), Some(ready("0.0.10", "ab")));
    }

    #[test]
    fn no_marker_or_a_broken_one_is_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()), None);
        std::fs::write(dir.path().join(FILE), b"{not json").unwrap();
        assert_eq!(read(dir.path()), None);
    }

    #[test]
    fn a_marker_for_another_version_or_another_file_is_not_this_download() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &ready("0.0.10", "ab")).unwrap();
        assert!(read_for(dir.path(), "0.0.10", "ab").is_some());
        assert!(
            read_for(dir.path(), "0.0.10", "AB").is_some(),
            "hex case is not a different file"
        );
        assert!(read_for(dir.path(), "0.0.11", "ab").is_none());
        assert!(read_for(dir.path(), "0.0.10", "cd").is_none());
    }

    #[test]
    fn staged_files_that_went_missing_are_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let provides = BTreeMap::from([
            ("mix".to_owned(), "mixengine/mix.exe".to_owned()),
            ("mixlab".to_owned(), "mixengine/mixlab.exe".to_owned()),
        ]);
        let unpacked = dir.path().join("unpacked/mixengine");
        std::fs::create_dir_all(&unpacked).unwrap();
        std::fs::write(unpacked.join("mix.exe"), b"").unwrap();
        std::fs::write(unpacked.join("mixlab.exe"), b"").unwrap();
        assert_eq!(
            staged(dir.path(), "0.0.10", "ab", &provides),
            Err(NotReady),
            "every file is there, but nothing proved them"
        );

        write(dir.path(), &ready("0.0.10", "ab")).unwrap();
        std::fs::remove_file(unpacked.join("mixlab.exe")).unwrap();
        assert_eq!(
            staged(dir.path(), "0.0.10", "ab", &provides),
            Err(NotReady),
            "mixlab.exe is missing"
        );

        std::fs::write(unpacked.join("mixlab.exe"), b"").unwrap();
        assert_eq!(
            staged(dir.path(), "0.0.10", "ab", &provides),
            Ok(dir.path().join("unpacked"))
        );
    }

    #[test]
    fn only_other_versions_directories_are_discarded() {
        let updates = tempfile::tempdir().unwrap();
        for name in ["0.0.9", "0.0.10", "0.0.11"] {
            std::fs::create_dir_all(updates.path().join(name)).unwrap();
        }
        std::fs::write(updates.path().join("decision.json"), b"{}").unwrap();
        discard_stale(updates.path(), Some("0.0.10"));
        assert!(!updates.path().join("0.0.9").exists());
        assert!(updates.path().join("0.0.10").exists());
        assert!(!updates.path().join("0.0.11").exists());
        assert!(
            updates.path().join("decision.json").exists(),
            "files are the records, not downloads"
        );
        discard_stale(updates.path(), None);
        assert!(!updates.path().join("0.0.10").exists());
    }
}
