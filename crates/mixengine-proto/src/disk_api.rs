//! What `daemon.disk_usage` and `daemon.cleanup` answer.
//!
//! **Five categories, and four different answers about them.** The screen this exists for asks for
//! runtimes, data, logs and certs; `cache/` is the fifth and it is the only one anybody actually
//! wants back, so a chart drawn from the four would be a chart with nothing to do underneath it.
//! What each row then answers is *whether it can be reclaimed and by what* — and that is the API's
//! to state rather than a client's to derive, which is the policy `CLAUDE.md` keeps out of clients.
//!
//! **The five do not sum to the home, and [`DiskUsage::other_bytes`] is why they do not have to.**
//! `bin/`, `etc/`, `packages/`, `extensions/`, `blueprints/`, `run/` and the database are none of
//! them; a report that stopped at five would be one from which a client draws a pie chart missing
//! its largest slice.
//!
//! **A plan and an act, on `daemon.uninstall_plan`/`daemon.uninstall`'s split** — a call that
//! measured and deleted in one breath would leave no moment in which somebody could be shown what
//! is about to go. [`Reclaim::ByCleanup`] *is* the plan: it is what the act would take, counted
//! before it goes.

use crate::Timestamp;

/// What `daemon.disk_usage` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiskUsageQuery {
    /// Walk the disk now rather than answering from the last reading.
    ///
    /// **Defaults to `false`**, and that default is the point. The dashboard this method exists for
    /// holds the event stream open and re-reads whenever something changes; a walk of `runtimes/` is
    /// tens of thousands of files, so an uncached reading would be a disk walk per event. The daemon
    /// keeps the last one for a minute and [`DiskUsage::measured_at`] says how old it is, so a
    /// client is never guessing. `mix disk` sends `true`: somebody who typed a command is asking
    /// about now.
    #[serde(default)]
    pub refresh: bool,
}

/// Where this home's disk has gone.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiskUsage {
    /// `MIXENGINE_HOME`, for a person to read.
    ///
    /// A `String` and not a `PathBuf` for [`DaemonStatus`](crate::DaemonStatus)' reason — serde
    /// refuses a `PathBuf` that is not valid UTF-8. A category `[paths]` has moved is **not** under
    /// it, which is what [`CategoryUsage::location`] is for.
    pub root: String,

    /// When the walk this answer comes from happened.
    ///
    /// On the answer rather than implied by the call, because the answer may be up to a minute old
    /// — see [`DiskUsageQuery::refresh`].
    pub measured_at: Timestamp,

    /// Exactly five, in [`DiskCategory::ALL`]'s order, whatever each answered.
    pub categories: Vec<CategoryUsage>,

    /// Everything else this home holds: `bin/`, `etc/`, `packages/`, `extensions/`, `blueprints/`,
    /// `run/`, the database with its write-ahead log, and `config.toml`.
    ///
    /// **Dominated by `packages/`** — nginx, MariaDB, Redis — on any home with a database
    /// installed. That is a compromise rather than a conclusion: `packages/` is reclaimable through
    /// `package.uninstall`, which [`Reclaim::ByMethod`] could say, and it is in here only because
    /// the disk-usage design names *five categories*. A sixth is a change to
    /// `docs/specs/2026-09-06-t96-disk-usage-and-cleanup-design.md`.
    ///
    /// Measured from that closed list of directories and not from a walk of whatever happens to be
    /// sitting in the home — `daemon.bundle`'s rule applied in the other direction.
    pub other_bytes: u64,
}

impl DiskUsage {
    /// The five plus the remainder — what MixEngine's home weighs, wherever `[paths]` put it.
    ///
    /// Here so that the one place this sum is defined is the crate that defines the parts. A client
    /// adding six numbers is doing arithmetic; a client deciding *which* six is doing policy.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.categories
            .iter()
            .map(|category| category.bytes)
            .fold(self.other_bytes, u64::saturating_add)
    }

    /// What `daemon.cleanup` would take back right now.
    ///
    /// **The plan half of this task**, and the number `mix disk` ends on. Everything that is not
    /// [`Reclaim::ByCleanup`] contributes nothing, whatever it weighs.
    #[must_use]
    pub fn reclaimable_bytes(&self) -> u64 {
        self.categories
            .iter()
            .filter_map(|category| match category.reclaim {
                Reclaim::ByCleanup { bytes, .. } => Some(bytes),
                _ => None,
            })
            .fold(0, u64::saturating_add)
    }
}

/// One category, what it weighs, and what would take it back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CategoryUsage {
    /// A stable name for it, which a client renders and a test asserts on.
    pub id: DiskCategory,

    /// Where it actually is, which `[paths]` may have moved out of the root.
    pub location: String,

    /// **Apparent** bytes — what `ls -l` and Explorer's *Size* column say, not blocks allocated.
    ///
    /// Stated here because it is the figure somebody will compare against `du`, which says the other
    /// one.
    pub bytes: u64,

    /// Regular files counted, symbolic links included as themselves.
    pub files: u64,

    /// Whether it can be reclaimed and by what.
    pub reclaim: Reclaim,

    /// Set when part of it could not be read, so [`bytes`](Self::bytes) is a floor and not a total.
    ///
    /// **A note and never an error.** A `[paths]` override onto a disk that is not mounted, or a
    /// directory an old install left unreadable, must not cost the whole answer — the same
    /// distinction `daemon.bundle` draws between an omission and an empty member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<String>,
}

/// The five things this build measures separately.
///
/// **Closed rather than a string**, on [`ResidueId`](crate::ResidueId)'s rule: a client keying off a
/// spelling is a client that silently stops matching, and a row nothing produces does not compile.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum DiskCategory {
    /// Installed language runtimes, one directory per `kind/version`.
    Runtimes,

    /// Per-instance service data — somebody's MySQL and Postgres instances.
    Data,

    /// `daemon.log`, a directory per supervised service, and the crash reports.
    Logs,

    /// The internal CA and the per-site certificates it issues.
    Certs,

    /// Downloaded answers that can always be asked for again.
    Cache,
}

impl DiskCategory {
    /// Every one of them, in the order a report lists them.
    ///
    /// Written out rather than derived: a list a macro produced would be as wrong as the enum on the
    /// day somebody gave two variants one `rename`, which is the mistake it exists to catch.
    pub const ALL: &'static [Self] = &[
        Self::Runtimes,
        Self::Data,
        Self::Logs,
        Self::Certs,
        Self::Cache,
    ];

    /// The one spelling: the wire form, and what a table prints.
    ///
    /// A method rather than a second table in the client, on
    /// [`JobState::as_str`](crate::JobState::as_str)'s rule — a renderer with its own list of names
    /// is a renderer that stops matching the day a category is added.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Runtimes => "runtimes",
            Self::Data => "data",
            Self::Logs => "logs",
            Self::Certs => "certs",
            Self::Cache => "cache",
        }
    }
}

/// Whether a category can be reclaimed, and by what.
///
/// **Four answers for five categories, and the API states them rather than letting a client work
/// them out.** A client that derived *"runtimes go through `runtime.uninstall`"* would be holding a
/// copy of a table the daemon owns — and a cleanup button that deleted a runtime directly would be
/// the back door around the refusal to uninstall a runtime under a running pool.
///
/// **Internally tagged**, so a client matches on a word rather than working out which fields
/// arrived — [`Outcome`](crate::Outcome)'s rule.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "reclaim", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Reclaim {
    /// Nothing here removes it, and nothing should. `data/`'s answer.
    Never {
        /// Why not, in a sentence.
        because: String,
    },

    /// Another method removes it, named — with the refusal that method carries. `runtimes/`'s
    /// answer, and the method is `runtime.uninstall`.
    ByMethod {
        /// The RPC method that does it, spelled as [`rpc::method`](crate::rpc::method) spells it.
        method: String,

        /// What that method refuses, so a client can say why the button may not work.
        because: String,
    },

    /// It can go, and what it costs is not disk. `certs/`'s answer.
    ///
    /// **Its own variant rather than a [`ByMethod`](Self::ByMethod) naming `daemon.uninstall`.** No
    /// method in this API deletes a certificate file — `cert.ca_uninstall` takes the *trust* out and
    /// says so — and telling a client that the way to get forty kilobytes back is to remove the
    /// product would be worse than saying nothing.
    AtACost {
        /// What is given up, in a sentence.
        because: String,
    },

    /// `daemon.cleanup` removes it, and this much of it is what it would take now. `logs/`'s and
    /// `cache/`'s answer.
    ///
    /// **Not the category's own figures.** `logs/` is the live `daemon.log`, each service's
    /// `current.log`, the crash reports and the rotated copies, and only the last of those goes.
    ByCleanup {
        /// Apparent bytes that would be reclaimed.
        bytes: u64,

        /// Files that would be removed.
        files: u64,
    },
}

/// What `daemon.cleanup` takes.
///
/// **Two negative flags and no list**, on [`UninstallQuery`](crate::UninstallQuery)'s idiom: `{}` is
/// the complete cleanup, and there is no field in which `data` or `runtimes` could be spelled at
/// all. A validator refusing them would be a refusal that can be forgotten in a later edit; a field
/// that does not exist cannot be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CleanupQuery {
    /// Leave the rotated log files where they are.
    #[serde(default)]
    pub keep_logs: bool,

    /// Leave the download cache where it is.
    #[serde(default)]
    pub keep_cache: bool,
}

/// What a cleanup took, and what it did not.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CleanupReport {
    /// One entry per reclaimable category, in a fixed order, whatever each answered.
    ///
    /// Two of them, always: [`DiskCategory::Logs`] and [`DiskCategory::Cache`]. A row appears even
    /// when it was kept or had nothing to take, on [`UninstallReport`](crate::UninstallReport)'s
    /// rule — a report that printed only what it removed would leave somebody unable to tell *"there
    /// were no rotated logs"* from *"the logs were not looked at"*.
    pub items: Vec<Cleaned>,
}

impl CleanupReport {
    /// What actually went, summed.
    #[must_use]
    pub fn reclaimed_bytes(&self) -> u64 {
        self.items
            .iter()
            .map(|item| match item.outcome {
                Cleanup::Reclaimed { bytes, .. } | Cleanup::Partial { bytes, .. } => bytes,
                _ => 0,
            })
            .fold(0, u64::saturating_add)
    }

    /// Was anything acted on and still there?
    ///
    /// **What `mix cleanup`'s exit code is.** [`Cleanup::Empty`] and [`Cleanup::Kept`] are
    /// deliberately not failures: one had nothing to take and the other was told not to take it.
    #[must_use]
    pub fn left_behind(&self) -> bool {
        self.items.iter().any(|item| {
            matches!(
                item.outcome,
                Cleanup::Partial { .. } | Cleanup::Failed { .. }
            )
        })
    }
}

/// One reclaimable category, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Cleaned {
    /// Which one. Only [`DiskCategory::Logs`] and [`DiskCategory::Cache`] ever appear.
    pub id: DiskCategory,

    /// Where it is, for a person.
    pub location: String,

    /// What became of it.
    pub outcome: Cleanup,
}

/// What became of one reclaimable category.
///
/// **Internally tagged**, for [`Reclaim`]'s reason.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "cleanup", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Cleanup {
    /// There was nothing of that kind to take.
    ///
    /// An empty struct variant rather than a unit one, for [`Outcome::Ok`](crate::Outcome::Ok)'s
    /// reason: a unit variant of an internally tagged enum is read through `deserialize_any`, where
    /// `deny_unknown_fields` never gets a chance to fire.
    Empty {},

    /// Gone — counted from the size taken immediately before each successful removal, so the figure
    /// is of files that are actually not there any more.
    Reclaimed {
        /// Files removed.
        files: u64,

        /// Apparent bytes reclaimed.
        bytes: u64,
    },

    /// Some of it went and some of it would not.
    ///
    /// **`Partial` rather than `Failed` when anything at all went**, so that somebody who reclaimed
    /// three hundred megabytes and lost one file to an antivirus is told they reclaimed three
    /// hundred megabytes.
    Partial {
        /// Files removed.
        files: u64,

        /// Apparent bytes reclaimed.
        bytes: u64,

        /// Files that would not go.
        left_behind: u64,

        /// What the machine said about the first of them — one reason, because a directory that will
        /// not empty gives the same one for every file in it.
        because: String,
    },

    /// Deliberately left, because the caller asked.
    Kept {
        /// Why it was left, in a sentence.
        because: String,
    },

    /// None of it went, and why.
    Failed {
        /// What the machine said.
        because: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A category travels with its reclaim tagged, so a client matches on a word rather than on
    /// which fields arrived — `Removal`'s rule in `uninstall_api`, and for its reason.
    #[test]
    fn a_category_travels_tagged_and_says_what_would_take_it_back() {
        let category = CategoryUsage {
            id: DiskCategory::Logs,
            location: "/home/a/.mixengine/logs".to_owned(),
            bytes: 2048,
            files: 7,
            reclaim: Reclaim::ByCleanup {
                bytes: 1024,
                files: 3,
            },
            unreadable: None,
        };

        let wire = serde_json::to_string(&category).expect("a category serialises");

        assert_eq!(
            wire,
            r#"{"id":"logs","location":"/home/a/.mixengine/logs","bytes":2048,"files":7,"reclaim":{"reclaim":"by_cleanup","bytes":1024,"files":3}}"#
        );
    }

    /// `unreadable` is absent when there was nothing to say, so the ordinary answer carries no field
    /// a client has to decide the meaning of.
    #[test]
    fn a_readable_category_omits_the_note_and_an_unreadable_one_carries_it() {
        let wire = r#"{"id":"data","location":"/d","bytes":1,"files":1,"reclaim":{"reclaim":"never","because":"they are your databases"},"unreadable":"1 entry could not be read"}"#;

        let category: CategoryUsage = serde_json::from_str(wire).expect("a category");

        assert_eq!(
            category.unreadable.as_deref(),
            Some("1 entry could not be read")
        );
        assert_eq!(serde_json::to_string(&category).expect("back"), wire);
    }

    /// Five categories, each with its own spelling: `mix` matches on these, and a repeated spelling
    /// is two rows a renderer cannot tell apart.
    #[test]
    fn every_category_has_its_own_spelling() {
        let spellings: Vec<String> = DiskCategory::ALL
            .iter()
            .map(|id| serde_json::to_string(id).expect("an id"))
            .collect();

        let mut unique = spellings.clone();
        unique.sort();
        unique.dedup();

        assert_eq!(unique.len(), spellings.len(), "{spellings:?}");
        assert_eq!(spellings.len(), 5);
        assert_eq!(spellings[0], r#""runtimes""#);
        assert_eq!(spellings[4], r#""cache""#);

        // The renderer's spelling and the wire's are one string, not two lists that agree today.
        for (id, spelling) in DiskCategory::ALL.iter().zip(&spellings) {
            assert_eq!(format!("\"{}\"", id.as_str()), *spelling);
        }
    }

    /// The five do not sum to the home, so the remainder is on the wire and the total is the six of
    /// them — see `docs/specs/2026-09-06-t96-disk-usage-and-cleanup-design.md`, D2.
    #[test]
    fn the_total_is_the_five_and_the_remainder() {
        let usage = DiskUsage {
            root: "/home/a/.mixengine".to_owned(),
            measured_at: Timestamp::from_system_time(std::time::UNIX_EPOCH),
            categories: vec![
                category(
                    DiskCategory::Runtimes,
                    100,
                    Reclaim::AtACost {
                        because: "x".to_owned(),
                    },
                ),
                category(
                    DiskCategory::Logs,
                    10,
                    Reclaim::ByCleanup { bytes: 4, files: 1 },
                ),
                category(
                    DiskCategory::Cache,
                    20,
                    Reclaim::ByCleanup {
                        bytes: 20,
                        files: 2,
                    },
                ),
            ],
            other_bytes: 5,
        };

        assert_eq!(usage.total_bytes(), 135);
        assert_eq!(usage.reclaimable_bytes(), 24);
    }

    /// A query that leaves both fields out is the complete cleanup, and nothing else can be asked
    /// for at all: `data` and `runtimes` are not fields — see
    /// `docs/specs/2026-09-06-t96-disk-usage-and-cleanup-design.md`, D4.
    #[test]
    fn a_query_with_no_fields_is_the_complete_cleanup_and_nothing_else_is_spellable() {
        let query: CleanupQuery = serde_json::from_str("{}").expect("no options is a shape");

        assert!(!query.keep_logs);
        assert!(!query.keep_cache);

        serde_json::from_str::<CleanupQuery>(r#"{"runtimes":true}"#)
            .expect_err("a category this method may not reach is not a field");

        let refresh: DiskUsageQuery = serde_json::from_str("{}").expect("no options is a shape");
        assert!(!refresh.refresh);
    }

    /// The exit code of `mix cleanup`. `Empty` and `Kept` are answers and not failures: one had
    /// nothing to take and the other was told not to.
    #[test]
    fn only_something_that_would_not_go_is_left_behind() {
        let mut report = CleanupReport {
            items: vec![
                cleaned(Cleanup::Reclaimed {
                    files: 3,
                    bytes: 300,
                }),
                cleaned(Cleanup::Empty {}),
                cleaned(Cleanup::Kept {
                    because: "you asked".to_owned(),
                }),
            ],
        };

        assert!(!report.left_behind());
        assert_eq!(report.reclaimed_bytes(), 300);

        report.items.push(cleaned(Cleanup::Partial {
            files: 1,
            bytes: 100,
            left_behind: 2,
            because: "the file is open".to_owned(),
        }));

        assert!(report.left_behind());
        assert_eq!(report.reclaimed_bytes(), 400);
    }

    /// `Empty` carries nothing and must still be spellable in both directions — `Removal::Absent`'s
    /// rule: a unit variant of an internally tagged enum is read through `deserialize_any`, where
    /// `deny_unknown_fields` never gets a chance to fire.
    #[test]
    fn an_empty_outcome_round_trips() {
        let wire = r#"{"id":"cache","location":"/c","outcome":{"cleanup":"empty"}}"#;

        let cleaned: Cleaned = serde_json::from_str(wire).expect("a cleaned row");

        assert_eq!(cleaned.outcome, Cleanup::Empty {});
        assert_eq!(serde_json::to_string(&cleaned).expect("back"), wire);
    }

    fn category(id: DiskCategory, bytes: u64, reclaim: Reclaim) -> CategoryUsage {
        CategoryUsage {
            id,
            location: "/somewhere".to_owned(),
            bytes,
            files: 1,
            reclaim,
            unreadable: None,
        }
    }

    fn cleaned(outcome: Cleanup) -> Cleaned {
        Cleaned {
            id: DiskCategory::Logs,
            location: "/somewhere".to_owned(),
            outcome,
        }
    }
}
