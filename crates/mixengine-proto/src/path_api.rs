//! What `path.*` answers: whether a terminal opened a minute from now will find `php`.
//!
//! **One answer type for all three methods**, on [`RuntimeSummary`](crate::RuntimeSummary)'s
//! precedent: `path.status`, `path.install` and `path.uninstall` are the same sentence about the
//! same thing, and a client renders them with one function. What differs between them is only
//! [`PathPlace::changed`], which is how "already done" is told apart from "done just now".
//!
//! **None of the three takes parameters.** There is exactly one directory this can be about —
//! `<root>/bin`, which the daemon knows and a client does not get to choose — and a `dir` argument
//! would be an API for putting arbitrary directories on somebody's PATH.

/// One place this machine keeps a PATH that survives a reboot, and what it says.
///
/// A list of these rather than a boolean because Unix has more than one: a home with both a
/// `.bash_profile` and a `.zprofile` needs the line in both, and being told "the PATH is set up"
/// while one's own login shell disagrees is the confusing half-state this exists to name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PathPlace {
    /// What to show: a shell profile's path, or the registry value's full name.
    ///
    /// A string and not a `PathBuf` for [`DaemonStatus`](crate::DaemonStatus)' reason, and here it
    /// is not always a path at all — on Windows it is `HKEY_CURRENT_USER\Environment\Path`.
    pub name: String,

    /// Whether `<root>/bin` is in it as things stand.
    pub present: bool,

    /// Whether *this call* is what put it there or took it away.
    ///
    /// Always `false` from `path.status`. It is what lets a client say "already set up" rather than
    /// claiming a write it did not perform — the two are indistinguishable to somebody reading the
    /// output, and only one of them is true.
    pub changed: bool,
}

/// What every `path.*` method answers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PathReport {
    /// The directory in question: `<root>/bin`, as a string for display.
    pub directory: String,

    /// Whether every place this OS reads a persisted PATH from carries it.
    ///
    /// **Every one and not any of them** — see [`PathPlace`]. Composed by the daemon rather than
    /// left for each client to fold the list itself, which is business logic and a place for two
    /// clients to disagree about what "set up" means.
    pub on_path: bool,

    /// Each of those places, in the order this OS reads them.
    pub places: Vec<PathPlace>,

    /// The commands `<root>/bin` answers to, as the files in it are named.
    ///
    /// Carried by all three methods because it is the other half of the same question: a PATH entry
    /// naming a directory with no `php` in it is a PATH entry that does nothing, and the two are
    /// set up together.
    pub commands: Vec<String>,

    /// What was in `<root>/bin` that no command answers to and could not be removed.
    ///
    /// Empty on every ordinary call. It is here rather than only in the daemon's log because a name
    /// left behind is a name on the user's PATH running something this build no longer understands
    /// — on Windows, most often a shim a shell in another window is still holding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale: Vec<String>,

    /// Where each of those commands came from.
    ///
    /// **Beside [`commands`](Self::commands) rather than replacing it**, because the two answer
    /// different questions and only one of them is the old one: `commands` is what `bin/` holds,
    /// read off the directory, and this is what MixEngine believes put each name there. A name in
    /// one and not the other is itself information — a command with no origin is a copy left behind
    /// by something that has since been uninstalled.
    ///
    /// In `commands`' order, and empty from a build or a client that does not fill it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<CommandOrigin>,

    /// Commands more than one installed package claimed, and who got each.
    ///
    /// Empty on nearly every machine: the only conflict this build can produce is a home with both
    /// MariaDB and MySQL installed, where `mysql`, `mysqladmin` and `mysqldump` are spellings
    /// MariaDB stands in for and MySQL owns outright.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<CommandConflict>,
}

/// One command in `<root>/bin`, and what put it there.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CommandOrigin {
    /// The command, spelled as the file in `bin/` is named.
    pub command: String,

    /// Where it came from.
    pub source: CommandSource,
}

/// The three things that can put a name in `<root>/bin`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum CommandSource {
    /// A command compiled into this build: `php`, `node`, `composer`.
    ///
    /// Present whether or not anything is installed, which is deliberate — a `node` on a machine
    /// with no Node.js says which command to type, where `node: command not found` says nothing.
    BuiltIn {},

    /// A client of an installed service package: `mysqldump`, `psql`, `redis-cli`.
    Client {
        /// Which package's recipe declared it.
        package: String,
    },

    /// A tool somebody installed into a runtime: `yarn`, `poetry`, `rails`.
    Global {
        /// Which language's version resolution decides which copy of it runs.
        kind: crate::RuntimeKind,
    },
}

/// A command name more than one installed package claimed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CommandConflict {
    /// The contested command.
    pub command: String,

    /// The package whose program `bin/` fronts under it.
    pub won: String,

    /// The packages that also claimed it, in the order the tie-break rejected them.
    pub lost: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> PathReport {
        PathReport {
            directory: "/home/me/.local/share/mixengine/bin".to_owned(),
            on_path: true,
            places: vec![PathPlace {
                name: "/home/me/.profile".to_owned(),
                present: true,
                changed: true,
            }],
            commands: vec!["php".to_owned(), "node".to_owned(), "mysqldump".to_owned()],
            stale: Vec::new(),
            origins: vec![
                CommandOrigin {
                    command: "php".to_owned(),
                    source: CommandSource::BuiltIn {},
                },
                CommandOrigin {
                    command: "mysqldump".to_owned(),
                    source: CommandSource::Client {
                        package: "mariadb".to_owned(),
                    },
                },
            ],
            conflicts: Vec::new(),
        }
    }

    /// The three sources are told apart by a tag rather than by which fields are present, on
    /// [`RuntimeSource`](crate::RuntimeSource)'s precedent: a client matches on one word.
    #[test]
    fn where_a_command_came_from_is_one_word() {
        let encoded = serde_json::to_value(report()).unwrap();

        assert_eq!(encoded["origins"][0]["source"]["from"], "built_in");
        assert_eq!(encoded["origins"][1]["source"]["from"], "client");
        assert_eq!(encoded["origins"][1]["source"]["package"], "mariadb");
    }

    /// A build or a client that fills neither list carries neither, which is what keeps a report
    /// from an older daemon readable by this one.
    #[test]
    fn a_report_with_nothing_to_say_about_origins_says_nothing() {
        let bare = PathReport {
            origins: Vec::new(),
            ..report()
        };

        let encoded = serde_json::to_value(bare).unwrap();
        assert!(encoded.get("origins").is_none(), "{encoded}");
        assert!(encoded.get("conflicts").is_none(), "{encoded}");
    }

    #[test]
    fn a_report_round_trips_through_the_wire() {
        let encoded = serde_json::to_value(report()).unwrap();

        assert_eq!(encoded["on_path"], true);
        assert_eq!(encoded["places"][0]["changed"], true);
        assert!(
            encoded.get("stale").is_none(),
            "the ordinary answer carries no empty list: {encoded}"
        );

        assert_eq!(
            serde_json::from_value::<PathReport>(encoded).unwrap(),
            report()
        );
    }

    /// The field a client reads to say "already set up" instead of claiming a write.
    #[test]
    fn an_install_that_changed_nothing_says_so() {
        let already = serde_json::json!({
            "directory": "/home/me/.local/share/mixengine/bin",
            "on_path": true,
            "places": [{"name": "/home/me/.profile", "present": true, "changed": false}],
            "commands": ["php"]
        });

        let report: PathReport = serde_json::from_value(already).expect("a complete answer");

        assert!(report.on_path);
        assert!(!report.places[0].changed);
        assert!(report.stale.is_empty(), "an absent list decodes as empty");
    }
}
