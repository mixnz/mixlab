//! `runtime.upgrade*` and `package.upgrade*` — roadmap tasks **T193b** and **T193c**, the design's
//! D4 and D7 (`docs/specs/2026-09-29-t193-a-line-shows-its-newest-and-updates-in-place-design.md`).
//!
//! **One plan type for both namespaces and for both moments.** What a person is shown before they
//! agree and what the finished job reports are the same list, with each entry's outcome moved from
//! `planned` to what happened — so a client draws one thing twice rather than two things.

use crate::{PackageVersion, Requirement, RuntimeKind, ServiceId, VersionConstraint};

/// Which runtime version to update, and to what.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RuntimeUpgradeQuery {
    /// Which language.
    pub kind: RuntimeKind,

    /// The installed version that moves.
    pub from: PackageVersion,

    /// The release it moves to; absent means the newest of `from`'s line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<PackageVersion>,
}

/// `runtime.upgrade`: the query, and what the person agreed to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RuntimeUpgrade {
    /// What moves where.
    #[serde(flatten)]
    pub query: RuntimeUpgradeQuery,

    /// Keep `from` installed afterwards.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep: bool,

    /// As on `runtime.install`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub install_prerequisites: bool,

    /// As on `runtime.install`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignore_requirements: bool,
}

/// Which package version to update, and to what.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PackageUpgradeQuery {
    /// Which package.
    pub package: String,

    /// The installed version that moves.
    pub from: PackageVersion,

    /// The release it moves to; absent means the newest of `from`'s line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<PackageVersion>,
}

/// `package.upgrade`: the query, and what the person agreed to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PackageUpgrade {
    /// What moves where.
    #[serde(flatten)]
    pub query: PackageUpgradeQuery,

    /// Keep `from` installed afterwards.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep: bool,

    /// As on `package.install`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub install_prerequisites: bool,

    /// As on `package.install`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignore_requirements: bool,

    /// Raise the elevation prompt for the port grant a new front-end binary needs on Linux, as
    /// `service.set_front_end`'s `grant` does. Without it the operation is queued and the update
    /// stops with the reason (D6 step 2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub grant: bool,
}

/// One thing an update moves, keeps or says — D5, D6, D7.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "item", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum UpgradeItem {
    /// A PHP site moves from one pool to the other.
    Site {
        /// Its primary domain.
        site: String,
        /// The pool it points at now.
        from: ServiceId,
        /// The pool it will point at.
        to: ServiceId,
    },

    /// A `web-app` extension's own pool, which moves only if `to` satisfies what it requires.
    ExtensionPool {
        /// The pool.
        pool: ServiceId,
        /// Whether it moves.
        moves: bool,
        /// What its manifest requires, where it could be read.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requires: Option<VersionConstraint>,
    },

    /// A PHP extension switched on or off for `from` that `to`'s build does not ship.
    DroppedExtension {
        /// The extension.
        name: String,
    },

    /// A project pin in SQLite that is rewritten.
    Pin {
        /// The project.
        project: String,
        /// The pin as it is.
        from: VersionConstraint,
        /// The pin as it will be.
        to: VersionConstraint,
    },

    /// A `mixengine.toml` pin only `from` answers, which keeps `from` installed.
    Manifest {
        /// The project.
        project: String,
        /// The file.
        path: String,
        /// What it pins.
        constraint: VersionConstraint,
    },

    /// A tool installed into `from` alone (`npm install -g yarn`), which keeps `from` installed.
    Tool {
        /// The command.
        name: String,
    },

    /// `from` is the default of its kind, and `to` becomes it.
    Default {},

    /// A server instance moves to `to`.
    Instance {
        /// The instance.
        service: ServiceId,
        /// Whether it is stopped and started again for it.
        restarts: bool,
    },

    /// The front end restarts, no site answers while it does, and this machine may be asked to let
    /// the new binary answer on 80 and 443.
    FrontEndRestart {
        /// The front end.
        service: ServiceId,
    },

    /// MySQL 8.0 cannot be moved back once a newer patch has opened its data.
    NoDowngrade {
        /// The instance.
        service: ServiceId,
    },
}

/// How one entry went. Every entry of a plan is `planned`; a finished job's are the rest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum UpgradeOutcome {
    /// Not done yet.
    Planned {},
    /// Done.
    Done {},
    /// Not done, because an earlier step stopped the update.
    Skipped {},
    /// Tried, and it did not work.
    Failed {
        /// Why, in a sentence.
        because: String,
    },
}

/// One entry of a plan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpgradeEntry {
    /// What.
    pub item: UpgradeItem,
    /// How it went.
    pub outcome: UpgradeOutcome,
}

/// What becomes of `from`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum OldVersion {
    /// A plan: it will be removed, unless the person asks to keep it.
    WillBeRemoved {},
    /// A plan: it will be kept whatever is asked, for these reasons.
    WillBeKept {
        /// One sentence each.
        because: Vec<String>,
    },
    /// A result: it was removed.
    Removed {},
    /// A result: it was kept, for these reasons.
    Kept {
        /// One sentence each.
        because: Vec<String>,
    },
}

/// What an update will do (`*.upgrade_plan`) or did (the job's result) — D7.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpgradePlan {
    /// The runtime kind or package name.
    pub subject: String,
    /// The installed version that moves.
    pub from: PackageVersion,
    /// The release it moves to.
    pub to: PackageVersion,
    /// Whether `to` was already installed, so nothing is downloaded.
    pub to_installed: bool,
    /// The download, in bytes; `0` when `to_installed`.
    pub bytes: u64,
    /// Whether the index this was judged against is a stale cached copy.
    pub stale: bool,
    /// What this machine lacks for `to`.
    pub needs: Vec<Requirement>,
    /// Everything that moves, is kept or is said.
    pub entries: Vec<UpgradeEntry>,
    /// What becomes of `from`.
    pub old: OldVersion,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> PackageVersion {
        PackageVersion::parse(text).unwrap()
    }

    #[test]
    fn an_upgrade_reads_flat_and_leaves_out_what_was_not_asked() {
        let asked: RuntimeUpgrade =
            serde_json::from_str(r#"{"kind":"php","from":"8.4.24"}"#).unwrap();

        assert_eq!(asked.query.kind, RuntimeKind::Php);
        assert_eq!(asked.query.to, None, "absent means the newest of the line");
        assert!(!asked.keep && !asked.install_prerequisites && !asked.ignore_requirements);

        let encoded = serde_json::to_value(&asked).unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({"kind": "php", "from": "8.4.24"})
        );
    }

    #[test]
    fn a_plan_entry_says_what_and_how_it_went_as_two_tags() {
        let entry = UpgradeEntry {
            item: UpgradeItem::Site {
                site: "blog.test".to_owned(),
                from: ServiceId::parse("php-fpm@8.4.24").unwrap(),
                to: ServiceId::parse("php-fpm@8.4.25").unwrap(),
            },
            outcome: UpgradeOutcome::Failed {
                because: "the front end refused".to_owned(),
            },
        };

        let encoded = serde_json::to_value(&entry).unwrap();
        assert_eq!(encoded["item"]["item"], "site");
        assert_eq!(encoded["outcome"]["outcome"], "failed");
        assert_eq!(
            serde_json::from_value::<UpgradeEntry>(encoded).unwrap(),
            entry
        );
    }

    #[test]
    fn a_plan_round_trips() {
        let plan = UpgradePlan {
            subject: "php".to_owned(),
            from: version("8.4.24"),
            to: version("8.4.25"),
            to_installed: false,
            bytes: 30_000_000,
            stale: false,
            needs: Vec::new(),
            entries: vec![UpgradeEntry {
                item: UpgradeItem::Default {},
                outcome: UpgradeOutcome::Planned {},
            }],
            old: OldVersion::WillBeKept {
                because: vec!["blog pins php 8.4.24 in mixengine.toml".to_owned()],
            },
        };

        let encoded = serde_json::to_value(&plan).unwrap();
        assert_eq!(encoded["old"]["state"], "will_be_kept");
        assert_eq!(
            serde_json::from_value::<UpgradePlan>(encoded).unwrap(),
            plan
        );
    }
}
