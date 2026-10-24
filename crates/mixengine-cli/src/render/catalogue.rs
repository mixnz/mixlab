//! What `mix runtime available` and `mix package available` share: the rows, and the columns.

use super::table::MISSING;
use mixengine_proto::{Execution, PackageVersion, Requirement};

/// Which rows of a catalogue to print.
///
/// **Filtering on what the daemon sent**, which is rendering: `line` and `newest_in_line` are the
/// daemon's answers, and nothing here compares two versions.
#[derive(Debug, Clone, Default)]
pub(crate) struct Lines {
    /// Every release, one row each, as the listing printed before it grouped them by line.
    pub(crate) all: bool,

    /// Every release of this one line.
    pub(crate) line: Option<String>,
}

impl Lines {
    /// Whether a release with these two members is printed.
    pub(super) fn shows(&self, line: Option<&str>, newest: Option<bool>) -> bool {
        match (&self.line, line) {
            (Some(wanted), Some(have)) => wanted == have,
            (Some(_), None) => true,
            // A daemon from before lines marks nothing, and its list is printed whole.
            (None, _) => self.all || newest != Some(false),
        }
    }

    /// Whether the `MORE` column is printed: only on the one-row-per-line view of a daemon that
    /// said which line each row is in.
    pub(super) fn counts(&self, any_line: bool) -> bool {
        !self.all && self.line.is_none() && any_line
    }
}

/// How many other releases share `line`, as a `MORE` cell. `lines` is every row of one name.
pub(super) fn more(lines: &[Option<&str>], line: Option<&str>) -> String {
    let Some(line) = line else {
        return MISSING.to_owned();
    };

    match lines.iter().filter(|other| **other == Some(line)).count() {
        0 | 1 => MISSING.to_owned(),
        count => format!("+{}", count - 1),
    }
}

/// The lines above a table that name each update, with the command that applies it.
pub(super) fn update_lines<'a>(
    command: &str,
    updates: impl Iterator<Item = (&'a str, &'a PackageVersion, &'a PackageVersion)>,
) -> String {
    let mut said = String::new();
    for (name, from, to) in updates {
        said.push_str(&format!(
            "update: {name} {from} → {to}   mix {command} upgrade {name} {from}\n"
        ));
    }
    said
}

/// The column that counts a line's other releases.
pub(super) const MORE_HEADING: &str = "MORE";

/// The seventh column's heading, on both listings.
pub(super) const RUNS_HEADING: &str = "RUNS";

/// The column that says what a release lacks on this machine.
pub(super) const NEEDS_HEADING: &str = "NEEDS";

/// What one release lacks, in the few words a cell has room for; blank for a release lacking nothing.
pub(super) fn needs(needs: Option<&Vec<Requirement>>) -> String {
    needs
        .map(|needs| {
            needs
                .iter()
                .map(|requirement| requirement.need.label())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// Whether any release in a listing lacks something here — the one case its `NEEDS` column appears.
pub(super) fn any_lacking<'a>(needs: impl Iterator<Item = Option<&'a Vec<Requirement>>>) -> bool {
    needs
        .into_iter()
        .any(|needs| needs.is_some_and(|needs| !needs.is_empty()))
}

/// The note that goes above a listing with an emulated row in it, or [`None`] for one without.
///
/// **A column rather than a line per row, and only when a row needs it.** On five of the six
/// targets MixEngine ships a build for, every release is native and a column reading `native` forty
/// times would be noise; on the sixth it is the answer to the question that machine's owner is
/// about to ask. The note is what makes the word mean something the first time somebody sees it,
/// and it goes above the table for the reason the staleness line does: it is true of the answer
/// rather than of any one row.
pub(super) fn emulation_column(
    executions: impl Iterator<Item = Option<Execution>>,
) -> Option<String> {
    let emulated = executions.into_iter().any(|execution| {
        execution.is_some_and(|execution| matches!(execution, Execution::Emulated))
    });

    emulated.then(|| {
        "emulated; nothing is published for this machine's own architecture, so the x86_64 \
         build is installed and the operating system runs it\n"
            .to_owned()
    })
}

/// What the seventh column says about one release.
///
/// [`None`] is a daemon that predates the member rather than one that could not decide, per
/// [ADR 0019](../../../../docs/decisions/0019-an-added-response-member-is-optional.md), so it reads
/// as the same dash every unstated value in these tables does.
pub(super) fn runs(execution: Option<Execution>) -> String {
    execution.map_or_else(|| MISSING.to_owned(), |execution| execution.to_string())
}

/// `mix runtime found` and `mix package found`, for a person: one version per line, where it is,
/// and why it is not listed.
pub(crate) fn on_disk<'a>(
    rows: impl Iterator<Item = (String, &'a String, &'a String)>,
    noun: &str,
) -> String {
    let mut rendered = String::new();
    for (name, path, why) in rows {
        rendered.push_str(&format!("{name}  {why}\n  {path}\n"));
    }

    if rendered.is_empty() {
        return "nothing on disk is waiting to be listed\n".to_owned();
    }

    rendered.push_str(&format!("`mix {noun} adopt` records one\n"));
    rendered
}
