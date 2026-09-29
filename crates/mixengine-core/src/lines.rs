//! What a *line* of versions is, and which release represents one — roadmap task **T193a**, the
//! design's D1 (`docs/specs/2026-09-29-t193-a-line-shows-its-newest-and-updates-in-place-design.md`).
//!
//! **Here and nowhere else.** The daemon composes every answer about lines out of these functions,
//! and no client computes one: two clients comparing versions their own way would be two clients
//! able to disagree about what an update is.
//!
//! **A line is never wider than a data series** (`adopt::instances::series_of`), which is what
//! makes an update within a line safe for a database's files. The test below holds the two together.

use std::collections::BTreeMap;

use mixengine_proto::PackageVersion;

/// How many leading numbers name a line of `name`.
///
/// Node.js and Java number their lines by major; PostgreSQL ties its data directory to the major.
/// Everything else — PHP, Python, Ruby, Go, Composer (whose 2.2 is an LTS line of its own) and every
/// other server — is major.minor, and so is a name this build has never heard of.
fn width(name: &str) -> usize {
    match name {
        "node" | "java" | "postgres" => 1,
        _ => 2,
    }
}

/// The line `version` of `name` belongs to: `8.4` for PHP 8.4.24, `22` for Node.js 22.8.0.
///
/// A pre-release tail is not part of a line, so 8.5.0RC1 is in `8.5`.
#[must_use]
pub fn line_of(name: &str, version: &PackageVersion) -> String {
    version
        .as_str()
        .split('.')
        .take(width(name))
        .map(|part| {
            let digits = part
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(part.len());
            &part[..digits]
        })
        .take_while(|digits| !digits.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

/// Whether two versions of `name` are in one line.
#[must_use]
pub fn same_line(name: &str, left: &PackageVersion, right: &PackageVersion) -> bool {
    line_of(name, left) == line_of(name, right)
}

/// The release each line of `offered` is represented by, keyed by line.
///
/// The newest stable release, or — for a line with none yet — the newest pre-release.
#[must_use]
pub fn newest_of_each_line<'a>(
    name: &str,
    offered: impl IntoIterator<Item = &'a PackageVersion>,
) -> BTreeMap<String, PackageVersion> {
    let mut lines: BTreeMap<String, Vec<&PackageVersion>> = BTreeMap::new();
    for version in offered {
        lines
            .entry(line_of(name, version))
            .or_default()
            .push(version);
    }

    lines
        .into_iter()
        .filter_map(|(line, versions)| {
            let newest = |candidates: &mut dyn Iterator<Item = &&PackageVersion>| {
                candidates
                    .max_by(|left, right| left.cmp_precedence(right))
                    .map(|version| (*version).clone())
            };

            newest(&mut versions.iter().filter(|version| !version.is_pre_release()))
                .or_else(|| newest(&mut versions.iter()))
                .map(|version| (line, version))
        })
        .collect()
}

/// The release of `offered` an installed `installed` would update to, if there is one.
///
/// Only a newer release of the same line. A stable install is never offered a pre-release; a
/// pre-release install is offered whichever is highest, stable or not.
#[must_use]
pub fn update_for<'a>(
    name: &str,
    installed: &PackageVersion,
    offered: impl IntoIterator<Item = &'a PackageVersion>,
) -> Option<PackageVersion> {
    let line = line_of(name, installed);
    let stable_only = !installed.is_pre_release();

    offered
        .into_iter()
        .filter(|candidate| line_of(name, candidate) == line)
        .filter(|candidate| candidate.cmp_precedence(installed).is_gt())
        .filter(|candidate| !stable_only || !candidate.is_pre_release())
        .max_by(|left, right| left.cmp_precedence(right))
        .cloned()
}
#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> PackageVersion {
        PackageVersion::parse(text).expect("a version")
    }

    #[test]
    fn every_kind_and_package_has_the_line_the_design_gives_it() {
        for (name, version, line) in [
            ("php", "8.4.24", "8.4"),
            ("python", "3.12.7", "3.12"),
            ("ruby", "3.3.6", "3.3"),
            ("go", "1.23.4", "1.23"),
            ("composer", "2.2.25", "2.2"),
            ("node", "22.8.0", "22"),
            ("java", "21.0.5", "21"),
            ("postgres", "17.2", "17"),
            ("mariadb", "11.4.5", "11.4"),
            ("mysql", "8.4.3", "8.4"),
            ("redis", "7.4.1", "7.4"),
            ("memcached", "1.6.32", "1.6"),
            ("mongodb", "8.0.4", "8.0"),
            ("caddy", "2.8.4", "2.8"),
            ("nginx", "1.27.3", "1.27"),
            ("fakeservice", "1.0.1", "1.0"),
            ("php", "8.5.0RC1", "8.5"),
            ("node", "22", "22"),
        ] {
            assert_eq!(line_of(name, &v(version)), line, "{name} {version}");
        }
    }

    /// **D1: a line never crosses a data series.** Were `line_of` ever wider than `series_of`, an
    /// update within a line could hand data to a server that cannot open it.
    #[test]
    fn a_line_is_never_wider_than_its_data_series() {
        for (package, version) in [
            ("mariadb", "11.4.5"),
            ("mysql", "8.0.40"),
            ("mysql", "8.4.3"),
            ("postgres", "16.6"),
        ] {
            let series = crate::adopt::instances::series_of(package, &v(version))
                .expect("a package with a series");
            let line = line_of(package, &v(version));

            assert!(
                line == series || line.starts_with(&format!("{series}.")),
                "{package} {version}: line {line} is wider than series {series}"
            );
        }
    }

    #[test]
    fn a_line_is_represented_by_its_newest_stable_release() {
        let offered = [
            v("8.4.23"),
            v("8.4.25"),
            v("8.4.24"),
            v("8.3.30"),
            v("8.5.0RC1"),
        ];
        let newest = newest_of_each_line("php", offered.iter());

        assert_eq!(newest["8.4"], v("8.4.25"));
        assert_eq!(newest["8.3"], v("8.3.30"));
        assert_eq!(
            newest["8.5"],
            v("8.5.0RC1"),
            "a line with only pre-releases is represented by its newest one"
        );
    }

    #[test]
    fn a_line_with_a_stable_release_is_not_represented_by_a_later_candidate() {
        let offered = [v("8.5.0"), v("8.5.1RC1")];
        assert_eq!(
            newest_of_each_line("php", offered.iter())["8.5"],
            v("8.5.0")
        );
    }

    #[test]
    fn a_stable_install_is_offered_the_newest_stable_release_of_its_line_only() {
        let offered = [v("8.4.24"), v("8.4.25"), v("8.4.26RC1"), v("8.5.0")];

        assert_eq!(
            update_for("php", &v("8.4.24"), offered.iter()),
            Some(v("8.4.25"))
        );
        assert_eq!(update_for("php", &v("8.4.25"), offered.iter()), None);
        assert_eq!(
            update_for("php", &v("8.5.0"), offered.iter()),
            None,
            "nothing newer in 8.5"
        );
    }

    #[test]
    fn a_pre_release_install_is_offered_the_highest_of_its_line() {
        assert_eq!(
            update_for("php", &v("8.5.0RC1"), [v("8.5.0RC2")].iter()),
            Some(v("8.5.0RC2"))
        );
        assert_eq!(
            update_for("php", &v("8.5.0RC1"), [v("8.5.0RC2"), v("8.5.0")].iter()),
            Some(v("8.5.0"))
        );
    }

    #[test]
    fn two_versions_are_in_one_line_only_when_their_lines_agree() {
        assert!(same_line("php", &v("8.4.1"), &v("8.4.25")));
        assert!(!same_line("php", &v("8.3.30"), &v("8.4.25")));
        assert!(same_line("node", &v("22.1.0"), &v("22.8.0")));
    }
}
