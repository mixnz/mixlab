//! What `home.previous` and `home.restore` answer — roadmap task **T182h**.
//!
//! An uninstall that keeps the folders `[paths]` moved leaves a copy of the home's state in them.
//! A fresh home pointed at those folders is told what the copy holds and may restore it.

use crate::Timestamp;

/// A copy of an earlier home's state, found in a kept folder.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PreviousCopy {
    /// The copy.
    pub path: String,
    /// When it was written.
    pub taken_at: Timestamp,
    /// Projects it would bring back.
    pub projects: u64,
    /// Sites.
    pub sites: u64,
    /// Services.
    pub services: u64,
    /// Runtimes it names.
    pub runtimes: u64,
    /// Packages it names.
    pub packages: u64,
    /// Whether a newer MixEngine wrote it, which this build does not restore.
    pub newer: bool,
}

/// What `home.previous` answers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HomePrevious {
    /// The newest copy in a kept folder, while this home has nothing of its own yet; absent
    /// otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<PreviousCopy>,
}

/// What `home.restore` did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HomeRestoreReport {
    /// Projects restored.
    pub projects: u64,
    /// Sites restored.
    pub sites: u64,
    /// Services restored, each left stopped.
    pub services: u64,
    /// Runtime rows restored (not counting ones this home already had).
    pub runtimes: u64,
    /// Package rows restored.
    pub packages: u64,
    /// What the copy held and this home could not take, one phrase each.
    pub skipped: Vec<String>,
    /// Steps after the restore that did not finish — a password, the hosts file, a certificate —
    /// one sentence each with what to do. The restore itself stands.
    pub problems: Vec<String>,
}
