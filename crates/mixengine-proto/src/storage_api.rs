//! Where a home's four growing directories are, and whether that can still change.
//!
//! **Not an RPC, and the types are here anyway.** Everything this describes is decided *before* a
//! daemon is listening: a window drawing its "choose a disk" screen has nothing to call, because the
//! thing it is asking about is where the daemon it has not started yet will put its files. So the
//! answer arrives from `mixengined --storage`, a one-shot that prints this and exits.
//!
//! It lives in `mixengine-proto` for the reason every other shape does: the window is typed against
//! `bindings/`, and a JSON document a client parsed by hand would be a second definition of the same
//! thing. `apps/desktop/src-tauri` may depend on this crate and on nothing else in the workspace.

/// One of the four directories, and whether it is still where the home would put it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StorageDirectory {
    /// Where it is now, resolved — so a relative `[paths]` value arrives as the directory it names.
    ///
    /// A `String` and not a `PathBuf` for [`DiskUsage`](crate::DiskUsage)' reason: serde refuses a
    /// `PathBuf` that is not valid UTF-8, and this is a document a person reads.
    pub path: String,

    /// Is this outside the home?
    ///
    /// The same test `daemon.uninstall_plan` uses to decide whether a directory needs a row of its
    /// own: what `[paths]` has moved is what does not lie under the root. Stated by the daemon
    /// rather than worked out by a client comparing two strings, because a client that got the
    /// comparison wrong would draw a home as relocated and offer to move it again.
    pub relocated: bool,
}

/// The four directories `[paths]` can move, in the order the configuration file lists them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StoragePaths {
    /// Installed language runtimes.
    pub runtimes: StorageDirectory,
    /// Installed servers and databases.
    pub packages: StorageDirectory,
    /// Service data — the databases themselves.
    pub data: StorageDirectory,
    /// The daemon's log and the services'.
    pub logs: StorageDirectory,
}

/// Whether the four may still be moved without moving any files.
///
/// **Internally tagged**, so a client matches on a word rather than working out which fields
/// arrived — [`Reclaim`](crate::Reclaim)'s rule.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "changeable", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum StorageChoice {
    /// Nothing is installed, so every key may still be set.
    Free,

    /// Something is, and moving it is a file move and a rewrite of the rows that name it — which
    /// this version does not do.
    Taken {
        /// Rows in `runtime_installs`.
        runtimes: u32,
        /// Rows in `packages`.
        packages: u32,
        /// Rows in `services`.
        services: u32,

        /// What is installed, in a sentence — *"3 runtimes and 1 service are installed"*.
        ///
        /// **The counts as well as the sentence, and neither is derivable from the other here.** A
        /// client that had only the sentence could not put a number in a badge, and one that had
        /// only the counts would be writing the sentence a second time — which is the thing the
        /// daemon owns. The window renders this the way it renders every other sentence the daemon
        /// sends, beside labels of its own.
        explanation: String,
    },
}

impl StorageChoice {
    /// May the four still be set?
    #[must_use]
    pub fn is_free(&self) -> bool {
        matches!(self, Self::Free)
    }
}

/// What `mixengined --storage` prints: where this home's directories are, and whether that is
/// still a question.
///
/// **It describes a home that may not exist yet.** A machine before its first start has no
/// `config.toml` and no database, and the honest answer for it is the default layout and
/// [`StorageChoice::Free`] — which is also the answer a window needs in order to offer the choice
/// at all. Producing it therefore creates nothing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StorageReport {
    /// `MIXENGINE_HOME`, for a person to read.
    pub root: String,

    /// Where each of the four is.
    pub paths: StoragePaths,

    /// Whether that can still be changed by a flag.
    pub changeable: StorageChoice,
}
