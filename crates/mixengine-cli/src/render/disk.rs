//! `mix disk` and `mix cleanup`.

use super::size::size;
use super::table::table;
use mixengine_proto::{Cleanup, CleanupReport, DiskUsage, Reclaim};

/// `daemon.disk_usage`, as a person reads it.
///
/// **Every row carries what would take it back**, because the question somebody has when they look
/// at a disk table is not *how big* but *what can I do about it* — and four of the five rows have a
/// different answer. The line at the end is the only number that leads to a command.
pub(crate) fn disk_usage(usage: &DiskUsage) -> String {
    let mut rows: Vec<[String; 3]> = usage
        .categories
        .iter()
        .map(|category| {
            [
                category.id.as_str().to_owned(),
                size(category.bytes),
                reclaim(&category.reclaim),
            ]
        })
        .collect();

    // Not a sixth category: the five are what the API answers, and this is the arithmetic that keeps
    // the table adding up to what the file manager says about the same directory.
    rows.push([
        "other".to_owned(),
        size(usage.other_bytes),
        "packages, generated config, the database".to_owned(),
    ]);

    // The summary line is not a row — it would read as a sixth category — so it lines its own label
    // up against the ones above rather than against a width written down twice.
    let label = rows
        .iter()
        .map(|row| row[0].chars().count())
        .max()
        .unwrap_or_default();

    let mut out = table(["", "size", "reclaimed by"], &rows);

    out.push_str(&format!(
        "\n{:<label$}  {}\n",
        "total",
        size(usage.total_bytes())
    ));

    for category in &usage.categories {
        if let Some(note) = &category.unreadable {
            out.push_str(&format!("{}: {note}\n", category.id.as_str()));
        }
    }

    let reclaimable = usage.reclaimable_bytes();
    out.push_str(&match reclaimable {
        0 => "\nthere is nothing `mix cleanup` would take back\n".to_owned(),
        _ => format!("\n`mix cleanup` would take back {}\n", size(reclaimable)),
    });

    out
}

/// What would reclaim one row, in a phrase.
fn reclaim(reclaim: &Reclaim) -> String {
    match reclaim {
        Reclaim::Never { because } | Reclaim::AtACost { because } => because.clone(),
        Reclaim::ByMethod { method, because } => format!("{method}: {because}"),
        Reclaim::ByCleanup { bytes, files } => {
            format!("`mix cleanup`: {} in {files} file(s)", size(*bytes))
        }
    }
}

/// `daemon.cleanup`, as a person reads it.
///
/// **Every row, whatever it answered**, on [`uninstall_report`]'s rule: a report that printed only
/// what it removed would leave somebody unable to tell *"there were no rotated log files"* from
/// *"the logs were not looked at"*.
///
/// [`uninstall_report`]: crate::render::uninstall::uninstall_report
pub(crate) fn cleanup_report(report: &CleanupReport) -> String {
    let mut out = String::new();

    for item in &report.items {
        let (mark, sentence) = match &item.outcome {
            Cleanup::Empty {} => ("nothing  ", None),
            Cleanup::Reclaimed { files, bytes } => (
                "took     ",
                Some(format!("{} in {files} file(s)", size(*bytes))),
            ),
            Cleanup::Partial {
                files,
                bytes,
                left_behind,
                because,
            } => (
                "LEFT     ",
                Some(format!(
                    "{} in {files} file(s); {left_behind} file(s) would not go: {because}",
                    size(*bytes)
                )),
            ),
            Cleanup::Kept { because } => ("kept     ", Some(because.clone())),
            Cleanup::Failed { because } => ("LEFT     ", Some(because.clone())),
        };

        out.push_str(&format!("{mark}{}\n", item.id.as_str()));
        out.push_str(&format!("         {}\n", item.location));

        if let Some(sentence) = sentence {
            out.push_str(&format!("         {sentence}\n"));
        }
    }

    out.push_str(&format!("\ntook back {}\n", size(report.reclaimed_bytes())));

    out
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{CategoryUsage, Cleaned, DiskCategory, Timestamp};

    use super::*;

    /// Every row says what would take it back, so a person reading the table never has to know
    /// which of five directories is safe to empty.
    #[test]
    fn the_disk_table_says_what_would_reclaim_each_row() {
        let usage = DiskUsage {
            root: "/home/a/.mixengine".to_owned(),
            measured_at: Timestamp::from_system_time(std::time::UNIX_EPOCH),
            categories: vec![
                CategoryUsage {
                    id: DiskCategory::Runtimes,
                    location: "/home/a/.mixengine/runtimes".to_owned(),
                    bytes: 700 << 20,
                    files: 40_000,
                    reclaim: Reclaim::ByMethod {
                        method: "runtime.uninstall".to_owned(),
                        because: "one at a time".to_owned(),
                    },
                    unreadable: None,
                },
                CategoryUsage {
                    id: DiskCategory::Cache,
                    location: "/home/a/.mixengine/cache".to_owned(),
                    bytes: 90 << 20,
                    files: 12,
                    reclaim: Reclaim::ByCleanup {
                        bytes: 90 << 20,
                        files: 12,
                    },
                    unreadable: None,
                },
            ],
            other_bytes: 10 << 20,
        };

        let rendered = disk_usage(&usage);

        assert!(rendered.contains("runtimes"), "{rendered}");
        assert!(rendered.contains("700 MiB"), "{rendered}");
        assert!(rendered.contains("runtime.uninstall"), "{rendered}");
        assert!(rendered.contains("other"), "{rendered}");
        assert!(rendered.contains("10 MiB"), "{rendered}");
        assert!(rendered.contains("mix cleanup"), "{rendered}");
        assert!(rendered.contains("90 MiB"), "{rendered}");

        // The summary line sits under the column it summarises, whatever the longest label is.
        assert!(rendered.contains("\ntotal     800 MiB\n"), "{rendered}");
    }

    /// A category that could only be read in part says so in the table, not only in the JSON: a
    /// figure that is a floor and reads as a total is the one thing this must not do.
    #[test]
    fn an_unreadable_category_says_so_in_the_table() {
        let usage = DiskUsage {
            root: "/home/a/.mixengine".to_owned(),
            measured_at: Timestamp::from_system_time(std::time::UNIX_EPOCH),
            categories: vec![CategoryUsage {
                id: DiskCategory::Data,
                location: "/mnt/bulk/data".to_owned(),
                bytes: 0,
                files: 0,
                reclaim: Reclaim::Never {
                    because: "these are your databases".to_owned(),
                },
                unreadable: Some("1 entry could not be read (permission denied)".to_owned()),
            }],
            other_bytes: 0,
        };

        let rendered = disk_usage(&usage);

        assert!(rendered.contains("could not be read"), "{rendered}");
        assert!(
            rendered.contains("there is nothing `mix cleanup` would take back"),
            "{rendered}"
        );
    }

    /// A row appears whatever it answered, so a person can tell "there were no rotated logs" from
    /// "the logs were not looked at" — `uninstall_report`'s rule, and its reason.
    #[test]
    fn a_cleanup_prints_every_row_and_ends_on_the_total() {
        let report = CleanupReport {
            items: vec![
                Cleaned {
                    id: DiskCategory::Logs,
                    location: "/home/a/.mixengine/logs".to_owned(),
                    outcome: Cleanup::Kept {
                        because: "you asked for the logs to be left".to_owned(),
                    },
                },
                Cleaned {
                    id: DiskCategory::Cache,
                    location: "/home/a/.mixengine/cache".to_owned(),
                    outcome: Cleanup::Reclaimed {
                        files: 12,
                        bytes: 90 << 20,
                    },
                },
            ],
        };

        let rendered = cleanup_report(&report);

        assert!(rendered.contains("kept"), "{rendered}");
        assert!(
            rendered.contains("you asked for the logs to be left"),
            "{rendered}"
        );
        assert!(rendered.contains("90 MiB"), "{rendered}");
        assert!(rendered.contains("took back 90 MiB"), "{rendered}");
    }

    /// A file that would not go is named, because a total that quietly excluded it would be a
    /// cleanup somebody runs twice wondering why the number never moves.
    #[test]
    fn a_partial_cleanup_says_what_would_not_go() {
        let report = CleanupReport {
            items: vec![Cleaned {
                id: DiskCategory::Logs,
                location: "/l".to_owned(),
                outcome: Cleanup::Partial {
                    files: 1,
                    bytes: 1 << 20,
                    left_behind: 2,
                    because: "/l/daemon.log.1: the file is open".to_owned(),
                },
            }],
        };

        let rendered = cleanup_report(&report);

        assert!(rendered.contains("LEFT"), "{rendered}");
        assert!(rendered.contains("2 file(s)"), "{rendered}");
        assert!(rendered.contains("the file is open"), "{rendered}");
    }
}
