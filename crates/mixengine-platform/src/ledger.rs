//! The helper's ledger of the homes it serves, on disk — ADR 0066.
//!
//! `homes.json` beside `elevate.log`: root-owned, world-readable, replaced atomically under a lock of
//! its own. The helper records every batch's home in it before any operation, and decides from it
//! whether another home still uses the helper and the log. The daemon reads it to plan the same
//! answer before a prompt, and to find a hosts block whose home is gone.
//!
//! Under `elevated` because the file lives in [`crate::elevated::audit_directory`], which is a
//! question only that feature answers; the daemon builds with it.

use std::path::Path;

use mixengine_proto::privileged::{HomesLedger, LedgerEntry};

use crate::{Error, Result};

/// The ledger's name in the audit directory.
pub const FILE_NAME: &str = "homes.json";

/// The lock that keeps two helpers from recording at once.
pub const LOCK_NAME: &str = "homes.lock";

/// What one [`record`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recorded {
    /// Whether the ledger was there before this call wrote it.
    ///
    /// A ledger this batch created has seen only this home, and says nothing about homes from
    /// before it — so it removes nothing (ADR 0066).
    pub existed_before: bool,
}

/// The ledger in `directory`, [`None`] where there is none yet.
///
/// # Errors
///
/// [`Error::Io`] for a ledger that cannot be read or does not parse. Callers keep
/// what they would have removed: keeping a file is recoverable, removing another home's is not.
pub fn read(directory: &Path) -> Result<Option<HomesLedger>> {
    let path = directory.join(FILE_NAME);

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                action: "read",
                path,
                source,
            });
        }
    };

    serde_json::from_str(&text)
        .map(Some)
        .map_err(|parse| Error::Io {
            action: "parse",
            path,
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, parse),
        })
}

/// Whether `home` is a home that still exists: a real directory, not a symlink to one.
///
/// A symlink counts as gone, on `mixengine-elevate`'s own rule for a request's home: the helper
/// never follows a link somebody else may have planted.
#[must_use]
pub fn alive(home: &Path) -> bool {
    std::fs::symlink_metadata(home).is_ok_and(|metadata| metadata.is_dir())
}

/// Record that `home`, asked by `caller`, used the helper at `now`.
///
/// # Errors
///
/// [`Error::Io`] when the ledger cannot be read or replaced, and
/// [`Error::Os`] when another helper holds its lock.
pub fn record(directory: &Path, home: &Path, caller: &str, now: u64) -> Result<Recorded> {
    // Held across the read and the replace, on `hosts::apply`'s reasoning: two helpers that both
    // read before either wrote would each drop the other's home.
    let _held = match crate::lock::Lock::acquire(&directory.join(LOCK_NAME))? {
        crate::lock::Acquired::Held(held) => held,
        crate::lock::Acquired::Taken(holder) => {
            return Err(Error::Os {
                action: "take the lock on the ledger of homes",
                source: std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    format!("{holder} is already recording a home"),
                ),
            });
        }
    };

    let existing = read(directory)?;
    let existed_before = existing.is_some();
    let mut ledger = existing.unwrap_or_default();

    match ledger.homes.iter_mut().find(|entry| entry.home == home) {
        Some(entry) => {
            entry.caller = caller.to_owned();
            entry.last = now;
        }
        None => ledger.homes.push(LedgerEntry {
            home: home.to_path_buf(),
            caller: caller.to_owned(),
            last: now,
        }),
    }

    // Cannot fail: the ledger is paths, strings and numbers. Mapped rather than unwrapped because
    // nothing in this crate panics.
    let text = serde_json::to_string_pretty(&ledger).map_err(|error| Error::Io {
        action: "write",
        path: directory.join(FILE_NAME),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
    })?;

    crate::sys::replace::atomically(&directory.join(FILE_NAME), &text)?;

    Ok(Recorded { existed_before })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two batches from one home are one entry, whose `last` moves.
    #[test]
    fn recording_twice_keeps_one_entry() {
        let directory = tempfile::TempDir::new().expect("a directory");
        let home = Path::new("/homes/a");

        record(directory.path(), home, "uid 501", 10).expect("recorded");
        record(directory.path(), home, "uid 501", 20).expect("recorded again");

        let ledger = read(directory.path()).expect("readable").expect("there");
        assert_eq!(ledger.homes.len(), 1);
        assert_eq!(ledger.homes[0].last, 20);
    }

    /// The first record is the one that made the file.
    #[test]
    fn the_first_record_says_there_was_none() {
        let directory = tempfile::TempDir::new().expect("a directory");

        let first = record(directory.path(), Path::new("/homes/a"), "uid 501", 1).expect("first");
        let second = record(directory.path(), Path::new("/homes/b"), "uid 501", 2).expect("second");

        assert!(!first.existed_before);
        assert!(second.existed_before);
        assert_eq!(read(directory.path()).unwrap().unwrap().homes.len(), 2);
    }

    /// A ledger that does not parse is an error, never an empty ledger.
    #[test]
    fn an_unreadable_ledger_keeps_both() {
        let directory = tempfile::TempDir::new().expect("a directory");
        std::fs::write(directory.path().join(FILE_NAME), "{not json").expect("written");

        assert!(read(directory.path()).is_err());
    }

    /// A missing ledger is no ledger, which is not an error.
    #[test]
    fn a_missing_ledger_reads_as_none() {
        let directory = tempfile::TempDir::new().expect("a directory");

        assert_eq!(read(directory.path()).unwrap(), None);
    }

    /// A directory is a home; a symlink to one, and nothing at all, are not.
    #[test]
    fn a_symlinked_home_counts_as_gone() {
        let directory = tempfile::TempDir::new().expect("a directory");
        let home = directory.path().join("home");
        std::fs::create_dir(&home).expect("a home");

        assert!(alive(&home));
        assert!(!alive(&directory.path().join("missing")));

        #[cfg(unix)]
        {
            let link = directory.path().join("link");
            std::os::unix::fs::symlink(&home, &link).expect("a symlink");
            assert!(!alive(&link));
        }
    }
}
