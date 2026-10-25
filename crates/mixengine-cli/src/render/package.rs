//! `mix package`.

use super::catalogue::{
    Lines, MORE_HEADING, NEEDS_HEADING, RUNS_HEADING, any_lacking, emulation_column, more, needs,
    runs, update_lines,
};
use super::size::size;
use super::table::{MISSING, table, table_of};
use super::time::ago;
use mixengine_proto::{PackageCatalogue, PackageList, PackageRelease, PackageRemoval};
use std::time::SystemTime;

/// `mix package list`, for a person.
///
/// The last column is what a person opens this listing to find out when an uninstall was refused:
/// which services are instances of this version, and therefore what has to go first.
#[must_use]
pub(crate) fn package_list(list: &PackageList) -> String {
    if list.packages.is_empty() {
        return "no packages are installed; `mix package available` lists what can be
"
        .to_owned();
    }

    let now = SystemTime::now();
    let rows: Vec<[String; 5]> = list
        .packages
        .iter()
        .map(|package| {
            [
                package.package.clone(),
                package.version.to_string(),
                size(package.bytes),
                // **Gone reads as gone.**
                match package.is_missing() {
                    true => "missing".to_owned(),
                    false => ago(package.installed_at, now),
                },
                match package.services.is_empty() {
                    true => MISSING.to_owned(),
                    false => package
                        .services
                        .iter()
                        .map(|service| service.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                },
            ]
        })
        .collect();

    let mut said = table(
        ["PACKAGE", "VERSION", "SIZE", "INSTALLED", "SERVICES"],
        &rows,
    );
    if let Some(gone) = list.packages.iter().find(|package| package.is_missing()) {
        let (name, version) = (&gone.package, &gone.version);
        said.push_str(&format!(
            "{name} {version}'s folder is gone: `mix package install {name} {version}` puts it \
             back; `mix package uninstall {name} {version}` forgets it\n"
        ));
    }
    said
}

/// `mix package available`, for a person — one row per line unless `lines` says otherwise.
#[must_use]
pub(crate) fn package_catalogue(catalogue: &PackageCatalogue, lines: &Lines) -> String {
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
             ({}); `mix package available --refresh` tries again\n",
            gap.name, gap.reason
        ));
    }

    if catalogue.packages.is_empty() {
        rendered.push_str("the package index offers nothing this build can run on this machine\n");
        return rendered;
    }

    rendered.push_str(&update_lines(
        "package",
        catalogue
            .updates
            .iter()
            .flatten()
            .map(|update| (update.package.as_str(), &update.from, &update.to)),
    ));

    let shown: Vec<&PackageRelease> = catalogue
        .packages
        .iter()
        .filter(|release| lines.shows(release.line.as_deref(), release.newest_in_line))
        .collect();
    let counted = lines.counts(
        catalogue
            .packages
            .iter()
            .any(|release| release.line.is_some()),
    );

    let cells = |release: &PackageRelease| {
        [
            release.package.clone(),
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

    if let Some(note) = &emulated {
        rendered.push_str(note);
    }

    let mut headings = PACKAGE_HEADINGS.to_vec();
    if counted {
        headings.push(MORE_HEADING);
    }
    if emulated.is_some() {
        headings.push(RUNS_HEADING);
    }
    if lacking {
        headings.push(NEEDS_HEADING);
    }

    let rows: Vec<Vec<String>> = shown
        .iter()
        .map(|release| {
            let mut row = cells(release).to_vec();
            if counted {
                let same: Vec<Option<&str>> = catalogue
                    .packages
                    .iter()
                    .filter(|other| other.package == release.package)
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
            row
        })
        .collect();

    rendered.push_str(&table_of(&headings, &rows));
    rendered
}

/// The columns `mix package available` prints when nothing is emulated.
const PACKAGE_HEADINGS: [&str; 6] = ["PACKAGE", "VERSION", "CHANNEL", "SIZE", "INSTALLED", "EOL"];

/// `mix package uninstall`, for a person.
#[must_use]
pub(crate) fn package_removal(removal: &PackageRemoval) -> String {
    format!(
        "removed {} {}
",
        removal.removed.package, removal.removed.version
    )
}

/// `mix package adopt`, for a person: [`runtime_summary`]'s layout, for a package.
///
/// [`runtime_summary`]: crate::render::runtime::runtime_summary
pub(crate) fn package_summary(package: &mixengine_proto::PackageSummary) -> String {
    let mut rendered = format!("{} {}\n", package.package, package.version);

    for (label, value) in [
        ("path", package.path.clone()),
        ("size", size(package.bytes)),
        ("installed", ago(package.installed_at, SystemTime::now())),
    ] {
        rendered.push_str(&format!("  {label:9} {value}\n"));
    }

    rendered
}

#[cfg(test)]
mod tests {
    use mixengine_proto::Timestamp;

    use super::*;

    /// The same for a package.
    #[test]
    fn a_package_whose_folder_is_gone_reads_missing_and_names_the_way_back() {
        let package = mixengine_proto::PackageSummary {
            package: "msys2".to_owned(),
            version: mixengine_proto::PackageVersion::parse("2026.10.08").expect("a version"),
            path: "/h/packages/msys2/2026.10.08".to_owned(),
            installed_at: Timestamp(1_760_000_000_000),
            bytes: 1,
            services: Vec::new(),
            missing: Some(true),
        };
        let said = package_list(&mixengine_proto::PackageList {
            packages: vec![package],
        });
        assert!(said.contains("missing"), "{said}");
        assert!(
            said.contains("`mix package install msys2 2026.10.08` puts it back"),
            "{said}"
        );
        assert!(
            said.contains("`mix package uninstall msys2 2026.10.08` forgets it"),
            "{said}"
        );
    }
}
