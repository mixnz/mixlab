//! `mix runtime`, and its PHP extensions.

use super::catalogue::{
    Lines, MORE_HEADING, NEEDS_HEADING, RUNS_HEADING, any_lacking, emulation_column, more, needs,
    runs, update_lines,
};
use super::size::size;
use super::table::{MISSING, table, table_of};
use super::time::ago;
use mixengine_proto::{
    ExtensionChange, ExtensionList, ExtensionSource, Linkage, PoolOutcome, ResolvedRuntime,
    RuntimeCatalogue, RuntimeList, RuntimeRelease, RuntimeRemoval, RuntimeSource, RuntimeSummary,
};
use std::time::SystemTime;

/// `mix runtime list`, for a person.
///
/// The default is a column rather than a mark beside the version, because the question somebody
/// scanning this asks is "which one does `php` mean" and a `*` is a footnote they have to look up.
pub(crate) fn runtime_list(list: &RuntimeList) -> String {
    if list.runtimes.is_empty() {
        return "no runtimes are installed; `mix runtime available` lists what can be\n".to_owned();
    }

    let now = SystemTime::now();
    let rows: Vec<[String; 5]> = list
        .runtimes
        .iter()
        .map(|runtime| {
            [
                runtime.kind.to_string(),
                runtime.version.to_string(),
                match runtime.default {
                    true => "yes".to_owned(),
                    false => MISSING.to_owned(),
                },
                size(runtime.bytes),
                // **Gone reads as gone** — roadmap task **T206d**, D4: the date of an install that
                // is not there would read as one that is.
                match runtime.is_missing() {
                    true => "missing".to_owned(),
                    false => ago(runtime.installed_at, now),
                },
            ]
        })
        .collect();

    let mut said = table(
        ["RUNTIME", "VERSION", "DEFAULT", "SIZE", "INSTALLED"],
        &rows,
    );
    // The same words as `mixengine_core::resolve::restore_command`, written here because `mix`
    // depends on no core crate.
    if let Some(gone) = list.runtimes.iter().find(|runtime| runtime.is_missing()) {
        let (kind, version) = (gone.kind, &gone.version);
        said.push_str(&format!(
            "{kind} {version}'s folder is gone: `mix runtime install {kind} {version}` puts it \
             back; `mix runtime uninstall {kind} {version}` forgets it\n"
        ));
    }
    said
}

/// `mix runtime ext list`, for a person.
///
/// One row per extension: what it is called, whether it can be turned off, and who decided. The last
/// column is the one the command is usually run for — *on because the build says so* and *on because
/// you turned it on* are different answers to why xdebug is loaded.
pub(crate) fn extension_list(list: &ExtensionList) -> String {
    if list.extensions.is_empty() {
        return "this build declares no extensions; nothing to turn on or off\n".to_owned();
    }

    let rows: Vec<[String; 4]> = list
        .extensions
        .iter()
        .map(|extension| {
            [
                extension.name.clone(),
                match extension.linkage {
                    Linkage::Static => "compiled in".to_owned(),
                    Linkage::Shared => "module".to_owned(),
                    _ => MISSING.to_owned(),
                },
                match extension.enabled {
                    true => "on".to_owned(),
                    false => "off".to_owned(),
                },
                match extension.source {
                    ExtensionSource::BuildDefault => "this build".to_owned(),
                    ExtensionSource::User => "you".to_owned(),
                    _ => MISSING.to_owned(),
                },
            ]
        })
        .collect();

    table(["EXTENSION", "KIND", "STATE", "DECIDED BY"], &rows)
}

/// `mix runtime ext enable` and `disable`, for a person.
///
/// Says what it deliberately did *not* do to the pool, because the alternative is a client guessing
/// from the operating system it happens to be running on.
pub(crate) fn extension_change(change: &ExtensionChange) -> String {
    let state = match change.extension.enabled {
        true => "enabled",
        false => "disabled",
    };

    let pool = match change.pool {
        PoolOutcome::Reloaded => "its pool re-read its configuration",
        PoolOutcome::RestartRequired => {
            "the running pool is still using the previous set; restart it to pick this up"
        }
        PoolOutcome::PoolNotRunning => "its pool is not running and will read this when it starts",
        _ => "what its pool did is not something this build can describe",
    };

    format!("{} {state}; {pool}\n", change.extension.name)
}

/// The columns `mix runtime available` prints when nothing is emulated.
const RUNTIME_HEADINGS: [&str; 6] = ["RUNTIME", "VERSION", "CHANNEL", "SIZE", "INSTALLED", "EOL"];

/// The column that says what a release cannot do on this cell — roadmap task **T206**, D3.
const LACKS_HEADING: &str = "LACKS";

/// The keys of what a release lacks, in the publisher's words; blank when it lacks nothing.
fn lacks(lacks: &std::collections::BTreeMap<String, String>) -> String {
    lacks.keys().cloned().collect::<Vec<_>>().join(", ")
}

/// Where the devkit stands, for the line after `mix runtime install` — roadmap task **T206**, D3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Devkit<'a> {
    /// One is installed here and its folder is there, so the Ruby builds native gems with it.
    Installed,
    /// None is installed, and the index offers this version, the newest.
    Offered(&'a str),
    /// None is installed and none is offered for this machine.
    NotOffered,
}

/// The line `mix runtime install` ends with when the release cannot build native gems — roadmap
/// task **T206**, D3. It names the devkit only where one is offered for this machine, with the
/// newest version offered, since `mix package install` takes one, and says nothing where one is
/// already installed: that Ruby builds native gems.
pub(crate) fn lacks_after_install(
    lacks: &std::collections::BTreeMap<String, String>,
    devkit: Devkit<'_>,
) -> Option<String> {
    if !lacks.contains_key("native gems") {
        return None;
    }

    match devkit {
        Devkit::Installed => None,
        Devkit::Offered(version) => Some(format!(
            "This Ruby cannot build gems with C extensions. `mix package install msys2 {version}` \
             adds the toolchain.\n"
        )),
        Devkit::NotOffered => {
            Some("This Ruby cannot build gems with C extensions on this machine.\n".to_owned())
        }
    }
}

/// `mix runtime available`, for a person.
///
/// **The staleness is a line above the table and not a column**, because it is true of the whole
/// answer: every row came out of the same document, and repeating "from a cached index" against each
/// of forty versions would say it forty times.
pub(crate) fn runtime_catalogue(catalogue: &RuntimeCatalogue, lines: &Lines) -> String {
    let mut rendered = String::new();

    if catalogue.stale {
        rendered.push_str(
            "this list is from a cached index; mixengined could not reach the package index, so \
             versions published since then are missing\n",
        );
    }

    for gap in catalogue.unavailable.iter().flatten() {
        rendered.push_str(&format!(
            "{} is missing from this list, its part of the package index could not be read \
             ({}); `mix runtime available --refresh` tries again\n",
            gap.name, gap.reason
        ));
    }

    if catalogue.runtimes.is_empty() {
        rendered.push_str("the package index offers nothing for this machine\n");
        return rendered;
    }

    rendered.push_str(&update_lines(
        "runtime",
        catalogue
            .updates
            .iter()
            .flatten()
            .map(|update| (update.kind.as_str(), &update.from, &update.to)),
    ));

    let shown: Vec<&RuntimeRelease> = catalogue
        .runtimes
        .iter()
        .filter(|release| lines.shows(release.line.as_deref(), release.newest_in_line))
        .collect();
    let counted = lines.counts(
        catalogue
            .runtimes
            .iter()
            .any(|release| release.line.is_some()),
    );

    let cells = |release: &RuntimeRelease| {
        [
            release.kind.to_string(),
            release.version.to_string(),
            release.channel.to_string(),
            size(release.bytes),
            match release.installed {
                true => "yes".to_owned(),
                false => MISSING.to_owned(),
            },
            release.eol.clone().unwrap_or_else(|| MISSING.to_owned()),
        ]
    };

    let emulated = emulation_column(shown.iter().map(|release| release.execution));
    let lacking = any_lacking(shown.iter().map(|release| release.needs.as_ref()));
    let lacked = shown.iter().any(|release| !release.lacks.is_empty());

    if let Some(note) = &emulated {
        rendered.push_str(note);
    }

    let mut headings = RUNTIME_HEADINGS.to_vec();
    if counted {
        headings.push(MORE_HEADING);
    }
    if emulated.is_some() {
        headings.push(RUNS_HEADING);
    }
    if lacking {
        headings.push(NEEDS_HEADING);
    }
    if lacked {
        headings.push(LACKS_HEADING);
    }

    let rows: Vec<Vec<String>> = shown
        .iter()
        .map(|release| {
            let mut row = cells(release).to_vec();
            if counted {
                let same: Vec<Option<&str>> = catalogue
                    .runtimes
                    .iter()
                    .filter(|other| other.kind == release.kind)
                    .map(|other| other.line.as_deref())
                    .collect();
                row.push(more(&same, release.line.as_deref()));
            }
            if emulated.is_some() {
                row.push(runs(release.execution));
            }
            if lacking {
                row.push(needs(release.needs.as_ref()));
            }
            if lacked {
                row.push(lacks(&release.lacks));
            }
            row
        })
        .collect();

    rendered.push_str(&table_of(&headings, &rows));
    rendered
}

/// One installed runtime, for a person: what `mix runtime default` answers and what a finished
/// install produced.
pub(crate) fn runtime_summary(runtime: &RuntimeSummary) -> String {
    let mut rendered = format!(
        "{} {}{}\n",
        runtime.kind,
        runtime.version,
        match runtime.default {
            true => ", the default for its kind",
            false => "",
        }
    );

    for (label, value) in [
        ("path", runtime.path.clone()),
        ("size", size(runtime.bytes)),
        ("installed", ago(runtime.installed_at, SystemTime::now())),
    ] {
        rendered.push_str(&format!("  {label:9} {value}\n"));
    }

    rendered
}

/// `mix runtime uninstall`, for a person.
///
/// The second line is the whole reason the answer is not just the runtime: a kind left with no
/// default is a kind whose shim resolves to nothing, and the person who caused it is the one who
/// should hear about it.
/// `mix runtime resolve`, for a person.
///
/// **The version is the first line and the reason is the last**, in that order because they are read
/// in that order: somebody who already knows which version they expect stops after the first line,
/// and somebody surprised by it reads on to find out which file did it. The path is between them
/// because it is what a person copies.
pub(crate) fn runtime_resolved(resolved: &ResolvedRuntime) -> String {
    let runtime = &resolved.runtime;

    let mut rendered = format!("{} {}\n", runtime.kind, runtime.version);
    rendered.push_str(&format!("  {:9} {}\n", "path", runtime.path));

    if let Some(constraint) = &resolved.constraint {
        rendered.push_str(&format!("  {:9} {constraint}\n", "asked for"));
    }

    let because = match &resolved.source {
        RuntimeSource::Explicit => "what you asked for on this command".to_owned(),
        RuntimeSource::Manifest { path } => path.clone(),
        RuntimeSource::Project { root } => format!("the project registered at {root}"),
        RuntimeSource::Default => format!(
            "the default for {}; nothing here pins a version",
            runtime.kind
        ),
    };
    rendered.push_str(&format!("  {:9} {because}\n", "chosen by"));

    rendered
}

pub(crate) fn runtime_removal(removal: &RuntimeRemoval) -> String {
    let mut rendered = format!(
        "removed {} {}\n",
        removal.removed.kind, removal.removed.version
    );

    if removal.default_cleared {
        rendered.push_str(&format!(
            "  it was the default for {}, and nothing was promoted in its place; \
             `mix runtime default {} <version>` chooses one\n",
            removal.removed.kind, removal.removed.kind
        ));
    }

    rendered
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{
        Execution, Need, PackageVersion, RedistributableArch, Remedy, Requirement, RuntimeKind,
        Timestamp,
    };

    use super::*;

    /// **A folder that is gone reads `missing`, and the way back is named** — T206d, D4.
    #[test]
    fn a_runtime_whose_folder_is_gone_reads_missing_and_names_the_way_back() {
        let mut runtime = mixengine_proto::RuntimeSummary {
            kind: RuntimeKind::Ruby,
            version: mixengine_proto::PackageVersion::parse("3.4.11").expect("a version"),
            channel: mixengine_proto::PackageChannel::Stable,
            path: "/h/runtimes/ruby/3.4.11".to_owned(),
            installed_at: Timestamp(1_760_000_000_000),
            bytes: 1,
            default: true,
            missing: Some(true),
        };
        let said = runtime_list(&mixengine_proto::RuntimeList {
            runtimes: vec![runtime.clone()],
        });
        assert!(said.contains("missing"), "{said}");
        assert!(
            said.contains("`mix runtime install ruby 3.4.11` puts it back"),
            "{said}"
        );
        assert!(
            said.contains("`mix runtime uninstall ruby 3.4.11` forgets it"),
            "{said}"
        );

        runtime.missing = Some(false);
        let said = runtime_list(&mixengine_proto::RuntimeList {
            runtimes: vec![runtime],
        });
        assert!(!said.contains("puts it back"), "{said}");
    }

    /// One offered release, at whatever execution the daemon reported — roadmap task **T92**.
    fn offered(version: &str, execution: Option<Execution>) -> RuntimeRelease {
        RuntimeRelease {
            kind: RuntimeKind::Php,
            version: PackageVersion::parse(version.to_owned()).expect("a version"),
            channel: mixengine_proto::PackageChannel::Stable,
            eol: None,
            bytes: 34_718_139,
            installed: false,
            execution,
            needs: None,
            line: None,
            newest_in_line: None,
            lacks: std::collections::BTreeMap::new(),
        }
    }

    fn in_line(release: RuntimeRelease, line: &str, newest: bool) -> RuntimeRelease {
        RuntimeRelease {
            line: Some(line.to_owned()),
            newest_in_line: Some(newest),
            ..release
        }
    }

    fn by_line() -> RuntimeCatalogue {
        RuntimeCatalogue {
            runtimes: vec![
                in_line(offered("8.4.25", None), "8.4", true),
                in_line(offered("8.4.24", None), "8.4", false),
                in_line(offered("8.4.23", None), "8.4", false),
                in_line(offered("8.3.30", None), "8.3", true),
            ],
            stale: false,
            unavailable: None,
            updates: Some(vec![mixengine_proto::RuntimeUpdate {
                kind: RuntimeKind::Php,
                from: PackageVersion::parse("8.4.24").expect("a version"),
                to: PackageVersion::parse("8.4.25").expect("a version"),
                to_installed: false,
                needs: Some(Vec::new()),
            }]),
        }
    }

    const EVERY_LINE: Lines = Lines {
        all: false,
        line: None,
    };

    /// **T193a, D3.** One row stands for each line, with a count of the rest.
    #[test]
    fn one_row_stands_for_each_line_with_a_count_of_the_rest() {
        let rendered = runtime_catalogue(&by_line(), &EVERY_LINE);

        assert!(rendered.contains("8.4.25"), "{rendered}");
        assert!(rendered.contains("8.3.30"), "{rendered}");
        assert!(
            !rendered.contains("8.4.23"),
            "an older patch is not a row: {rendered}"
        );
        assert!(rendered.contains("MORE"), "{rendered}");
        assert!(
            rendered.contains("+2"),
            "two other 8.4 releases: {rendered}"
        );
    }

    #[test]
    fn all_prints_every_release() {
        let rendered = runtime_catalogue(
            &by_line(),
            &Lines {
                all: true,
                line: None,
            },
        );
        assert!(rendered.contains("8.4.23"), "{rendered}");
        assert!(!rendered.contains("MORE"), "{rendered}");
    }

    #[test]
    fn naming_a_line_prints_every_release_of_it_and_nothing_else() {
        let rendered = runtime_catalogue(
            &by_line(),
            &Lines {
                all: false,
                line: Some("8.4".to_owned()),
            },
        );
        assert!(rendered.contains("8.4.23"), "{rendered}");
        assert!(!rendered.contains("8.3.30"), "{rendered}");
    }

    #[test]
    fn an_update_is_named_above_the_table_with_the_command_that_applies_it() {
        let rendered = runtime_catalogue(&by_line(), &EVERY_LINE);
        assert!(rendered.contains("php 8.4.24 → 8.4.25"), "{rendered}");
        assert!(
            rendered.contains("mix runtime upgrade php 8.4.24"),
            "{rendered}"
        );
    }

    /// **A daemon from before T193 sends no lines**, and its list is printed as it always was
    /// rather than as nothing — the plan's review focus 1.
    #[test]
    fn an_old_daemon_s_catalogue_is_printed_flat() {
        let rendered = runtime_catalogue(
            &RuntimeCatalogue {
                runtimes: vec![offered("8.4.25", None), offered("8.4.24", None)],
                stale: false,
                updates: None,
                unavailable: None,
            },
            &EVERY_LINE,
        );
        assert!(
            rendered.contains("8.4.25") && rendered.contains("8.4.24"),
            "{rendered}"
        );
        assert!(!rendered.contains("MORE"), "{rendered}");
    }

    fn lacking(release: RuntimeRelease, need: Need, remedy: Remedy) -> RuntimeRelease {
        RuntimeRelease {
            needs: Some(vec![Requirement { need, remedy }]),
            ..release
        }
    }

    /// **T196.** A kind the index could not be read for is said above the table, where the
    /// staleness line is said, and a list that read everything says nothing.
    #[test]
    fn a_kind_the_index_could_not_be_read_for_is_said_above_the_table() {
        let mut catalogue = RuntimeCatalogue {
            runtimes: vec![offered("8.3.33", Some(Execution::Native))],
            stale: false,
            updates: None,
            unavailable: Some(vec![mixengine_proto::CatalogueGap {
                name: "node".to_owned(),
                reason: "it does not hash to what the signed root says".to_owned(),
            }]),
        };

        let rendered = runtime_catalogue(&catalogue, &EVERY_LINE);
        let first = rendered.lines().next().expect("a line above the table");
        assert!(
            first.starts_with("node is missing from this list"),
            "{rendered}"
        );
        assert!(first.contains("does not hash"), "{rendered}");
        assert!(
            first.ends_with("`mix runtime available --refresh` tries again"),
            "a line ends with the command that fixes it: {rendered}"
        );

        for silent in [None, Some(Vec::new())] {
            catalogue.unavailable = silent;
            assert!(
                !runtime_catalogue(&catalogue, &EVERY_LINE).contains("missing from this list"),
                "nothing unread, nothing said"
            );
        }
    }

    /// **What a cell cannot do has a column of its own** — roadmap task **T206**, D3, on `NEEDS`'
    /// rule: drawn only when some row lacks something.
    #[test]
    fn a_lacks_column_appears_only_when_a_row_lacks_something() {
        let catalogue_of = |runtimes| RuntimeCatalogue {
            runtimes,
            stale: false,
            updates: None,
            unavailable: None,
        };

        let mut ruby = offered("3.4.11", None);
        ruby.lacks
            .insert("native gems".to_owned(), "no compiler".to_owned());
        ruby.lacks.insert("yjit".to_owned(), "not built".to_owned());

        let rendered = runtime_catalogue(&catalogue_of(vec![ruby]), &EVERY_LINE);
        assert!(rendered.contains("LACKS"), "{rendered}");
        assert!(rendered.contains("native gems, yjit"), "{rendered}");

        let plain = runtime_catalogue(&catalogue_of(vec![offered("3.4.11", None)]), &EVERY_LINE);
        assert!(!plain.contains("LACKS"), "{plain}");
    }

    /// **The install names the devkit where one is offered**, and only the gap where none is.
    #[test]
    fn the_install_names_the_devkit_where_one_is_offered() {
        let lacks = std::collections::BTreeMap::from([("native gems".to_owned(), "x".to_owned())]);
        // With the version: `mix package install` takes one, and a line naming a command that
        // refuses to run is worse than none.
        let said = lacks_after_install(&lacks, Devkit::Offered("2026.10.08")).expect("a line");
        assert!(
            said.contains("`mix package install msys2 2026.10.08`"),
            "{said}"
        );
        let alone = lacks_after_install(&lacks, Devkit::NotOffered).expect("a line");
        assert!(!alone.contains("msys2"), "{alone}");
    }

    /// **A devkit already here leaves nothing to say** — the Ruby builds native gems with it.
    #[test]
    fn the_install_says_nothing_when_the_devkit_is_installed() {
        let lacks = std::collections::BTreeMap::from([("native gems".to_owned(), "x".to_owned())]);
        assert_eq!(lacks_after_install(&lacks, Devkit::Installed), None);
    }

    /// Nothing lacked, nothing said — every Ruby on macOS and Linux.
    #[test]
    fn no_line_when_nothing_is_lacked() {
        assert_eq!(
            lacks_after_install(&std::collections::BTreeMap::new(), Devkit::Offered("1")),
            None
        );
        let yjit_only = std::collections::BTreeMap::from([("yjit".to_owned(), "x".to_owned())]);
        assert_eq!(lacks_after_install(&yjit_only, Devkit::Offered("1")), None);
    }

    /// **T151.** The column appears only when a row lacks something, on `RUNS`' reasoning.
    #[test]
    fn a_needs_column_appears_only_when_a_row_lacks_something() {
        let plain = RuntimeCatalogue {
            runtimes: vec![offered("8.3.33", Some(Execution::Native))],
            stale: false,
            updates: None,
            unavailable: None,
        };
        assert!(!runtime_catalogue(&plain, &EVERY_LINE).contains("NEEDS"));

        let lacking_one = RuntimeCatalogue {
            runtimes: vec![
                lacking(
                    offered("8.4.24", Some(Execution::Native)),
                    Need::VisualCpp {
                        year: "2022".to_owned(),
                        arch: RedistributableArch::X64,
                        found: None,
                    },
                    Remedy::InstallVisualCpp {
                        arch: RedistributableArch::X64,
                    },
                ),
                offered("8.3.33", Some(Execution::Native)),
            ],
            stale: false,
            updates: None,
            unavailable: None,
        };
        let rendered = runtime_catalogue(&lacking_one, &EVERY_LINE);
        assert!(rendered.contains("NEEDS"), "{rendered}");
        assert!(rendered.contains("Visual C++ 2022 (x64)"), "{rendered}");
    }

    /// The column exists only where it says something — roadmap task **T92**.
    ///
    /// Five of the six targets MixEngine ships a build for see this rendering, and a column reading
    /// `native` against every row would be noise on all five.
    #[test]
    fn a_catalogue_of_native_releases_has_no_column_about_it() {
        let rendered = runtime_catalogue(
            &RuntimeCatalogue {
                runtimes: vec![offered("8.3.33", Some(Execution::Native))],
                stale: false,
                updates: None,
                unavailable: None,
            },
            &EVERY_LINE,
        );

        assert!(!rendered.contains("RUNS"), "no column: {rendered}");
        assert!(!rendered.contains("emulated"), "and no note: {rendered}");
    }

    /// A daemon from before the member reports nothing, which is not a claim that anything is
    /// emulated — [ADR 0019](../../../../docs/decisions/0019-an-added-response-member-is-optional.md).
    #[test]
    fn a_daemon_that_reports_no_execution_brings_no_column_either() {
        let rendered = runtime_catalogue(
            &RuntimeCatalogue {
                runtimes: vec![offered("8.3.33", None)],
                stale: false,
                updates: None,
                unavailable: None,
            },
            &EVERY_LINE,
        );

        assert!(!rendered.contains("RUNS"), "{rendered}");
    }

    #[test]
    fn one_emulated_release_brings_the_column_and_the_sentence_that_explains_it() {
        let rendered = runtime_catalogue(
            &RuntimeCatalogue {
                runtimes: vec![
                    offered("8.3.33", Some(Execution::Emulated)),
                    offered("8.4.24", Some(Execution::Native)),
                ],
                stale: false,
                updates: None,
                unavailable: None,
            },
            &EVERY_LINE,
        );

        assert!(rendered.contains("RUNS"), "the column: {rendered}");
        assert!(rendered.contains("emulated"), "the word: {rendered}");
        assert!(
            rendered.contains("native"),
            "and its opposite, so the column reads: {rendered}"
        );
        assert!(
            rendered
                .lines()
                .next()
                .is_some_and(|line| line.starts_with("emulated;")),
            "the note comes before the table: {rendered}"
        );
    }
}
