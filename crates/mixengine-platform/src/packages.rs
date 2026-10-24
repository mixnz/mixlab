//! What a package database says owns a file.
//!
//! **Pure and compiled on all three systems**, for [`crate::reserved`]'s reason: the half of a
//! per-OS mechanism that is a decision about text is tested everywhere, and only the call that can
//! be made nowhere else stays in `sys::install`.

/// Which database answered, which is how its one line reads.
#[allow(
    dead_code,
    reason = "used by Linux's reader only; compiled on all three so its tests run on all three"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageDatabase {
    /// `dpkg-query -S <path>`: `<package>: <path>`.
    Dpkg,
    /// `rpm -qf --queryformat '%{NAME}\n' <path>`: the package's name alone.
    Rpm,
}

/// The package named in one database's answer, or [`None`] when it names none.
///
/// **Only ever handed the standard output of a run that exited 0**, so "nobody owns it" normally
/// never reaches here. `rpm` says it on standard output anyway, so that sentence is refused by
/// wording as well.
#[allow(
    dead_code,
    reason = "used by Linux's reader only; compiled on all three so its tests run on all three"
)]
pub(crate) fn owning_package(database: PackageDatabase, stdout: &str) -> Option<String> {
    let line = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("diversion by "))?;

    let name = match database {
        PackageDatabase::Dpkg => line.split_once(": ")?.0,
        PackageDatabase::Rpm => line,
    }
    .trim();

    (!name.is_empty() && !name.contains("is not owned by any package")).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELPER: &str = "/usr/local/libexec/mixengine/mixengine-elevate";

    #[test]
    fn dpkg_names_the_package_before_the_path() {
        assert_eq!(
            owning_package(PackageDatabase::Dpkg, &format!("mixengine: {HELPER}\n")),
            Some("mixengine".to_owned())
        );
    }

    /// A multi-arch package carries its architecture after a colon with no space, and the
    /// separator is a colon *and* a space.
    #[test]
    fn dpkg_keeps_an_architecture_in_the_name() {
        assert_eq!(
            owning_package(
                PackageDatabase::Dpkg,
                &format!("mixengine:amd64: {HELPER}\n")
            ),
            Some("mixengine:amd64".to_owned())
        );
    }

    /// `dpkg-divert` adds lines of its own in front of the owner's.
    #[test]
    fn dpkg_skips_a_diversion() {
        let answer = format!(
            "diversion by other from: {HELPER}\ndiversion by other to: {HELPER}.real\nmixengine: {HELPER}\n"
        );
        assert_eq!(
            owning_package(PackageDatabase::Dpkg, &answer),
            Some("mixengine".to_owned())
        );
    }

    #[test]
    fn rpm_names_the_package_alone() {
        assert_eq!(
            owning_package(PackageDatabase::Rpm, "mixengine\n"),
            Some("mixengine".to_owned())
        );
    }

    /// `rpm -qf` prints this on standard output, not on standard error. Its exit code says the
    /// same, and this is the second line of defence.
    #[test]
    fn rpm_saying_nobody_owns_it_is_no_package() {
        assert_eq!(
            owning_package(
                PackageDatabase::Rpm,
                &format!("file {HELPER} is not owned by any package\n")
            ),
            None
        );
    }

    #[test]
    fn nothing_said_is_no_package() {
        assert_eq!(owning_package(PackageDatabase::Dpkg, ""), None);
        assert_eq!(owning_package(PackageDatabase::Rpm, "\n"), None);
        assert_eq!(
            owning_package(PackageDatabase::Dpkg, "no colon here\n"),
            None
        );
    }
}
