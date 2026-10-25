//! The listing every table in `mix` is printed with, and the dash a missing value prints as.

/// What is printed where a service has no value for something.
pub(super) const MISSING: &str = "—";

/// A listing with its headings, every column as wide as its widest cell.
///
/// Generic over the number of columns rather than written once per table: four commands here answer
/// with a listing now, and the alternative is four copies of the same width calculation drifting
/// apart in how they pad and where they trim.
pub(super) fn table<const N: usize>(headings: [&str; N], rows: &[[String; N]]) -> String {
    let rows: Vec<Vec<String>> = rows.iter().map(|row| row.to_vec()).collect();
    table_of(&headings, &rows)
}

/// [`table`] for a column count decided at run time, where a listing may grow `RUNS`, `NEEDS`, both
/// or neither.
pub(super) fn table_of(headings: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..headings.len())
        .map(|column| {
            rows.iter()
                .map(|row| row.get(column).map_or(0, |cell| cell.chars().count()))
                .chain(std::iter::once(headings[column].chars().count()))
                .max()
                .unwrap_or_default()
        })
        .collect();

    let mut rendered = String::new();
    let headings: Vec<String> = headings
        .iter()
        .map(|heading| (*heading).to_owned())
        .collect();

    for row in std::iter::once(&headings).chain(rows.iter()) {
        let line = row
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:width$}"))
            .collect::<Vec<_>>()
            .join("  ");

        // Trimmed, so a table's last column carries no trailing run of spaces into whatever a
        // person pastes it into.
        rendered.push_str(line.trim_end());
        rendered.push('\n');
    }

    rendered
}
