//! The helper's record of the homes it serves — roadmap task **T211g**, ADR 0066.
//!
//! **A file format, not an API.** `mixengine-elevate` writes it beside its audit log and is the
//! only writer; the daemon reads the same file to plan an uninstall and to find a hosts block whose
//! home is gone. Here rather than in either crate because both sides parse it, and one definition
//! is what keeps them reading the same thing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Every home that has asked this machine's helper for anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomesLedger {
    /// One entry per home path, in the order they first asked.
    pub homes: Vec<LedgerEntry>,
}

/// One home, as its last request named it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// `MIXENGINE_HOME` as the helper resolved it.
    pub home: PathBuf,

    /// The account that asked, as the audit log writes it.
    pub caller: String,

    /// When it last asked, in seconds since the Unix epoch.
    pub last: u64,
}

impl HomesLedger {
    /// The homes other than `this` that `alive` says still exist.
    ///
    /// **`alive` is the caller's**, because what counts as a home that still exists is a question
    /// about a filesystem, and this crate reads none.
    #[must_use]
    pub fn living_others(&self, this: &Path, alive: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
        self.homes
            .iter()
            .filter(|entry| entry.home != this && alive(&entry.home))
            .map(|entry| entry.home.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(home: &str) -> LedgerEntry {
        LedgerEntry {
            home: PathBuf::from(home),
            caller: "uid 501".to_owned(),
            last: 0,
        }
    }

    /// The home asking is never one of the others, and a home that is gone is nobody's.
    #[test]
    fn living_others_leaves_out_this_home_and_the_dead() {
        let ledger = HomesLedger {
            homes: vec![entry("/a"), entry("/b"), entry("/gone")],
        };

        let others = ledger.living_others(Path::new("/a"), |home| home != Path::new("/gone"));

        assert_eq!(others, vec![PathBuf::from("/b")]);
    }
}
