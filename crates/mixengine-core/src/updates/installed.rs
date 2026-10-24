//! The version the daemon binary on disk reports.
//!
//! After Installer.app has run, the file at the daemon's own path is the new binary while this
//! process still runs the old image, as measured on a Mac. Asking that file is how the daemon
//! learns the install happened.

use std::path::Path;

/// `mixengined --version` prints `mixengined 0.0.9`, and a development build appends
/// `(development build)`. The word after the name, or [`None`] for anything else.
#[must_use]
pub fn parse(stdout: &str) -> Option<String> {
    let mut words = stdout.split_whitespace();

    if words.next()? != "mixengined" {
        return None;
    }

    words.next().map(str::to_owned)
}

/// Run `exe --version` and read it. [`None`] when it will not run, fails, or is not `mixengined`.
///
/// **Blocking**: a caller on the runtime runs it through `spawn_blocking`. It is asked once per new
/// file at the daemon's path, not once per status poll.
#[must_use]
pub fn version_of(exe: &Path) -> Option<String> {
    let output = std::process::Command::new(exe)
        .arg("--version")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_the_word_after_the_name() {
        assert_eq!(parse("mixengined 0.0.9\n"), Some("0.0.9".to_owned()));
    }

    /// What a development build prints (`cargo run -p mixengine-daemon -- --version`).
    #[test]
    fn a_development_build_reads_as_its_version() {
        assert_eq!(
            parse("mixengined 0.0.7 (development build)\n"),
            Some("0.0.7".to_owned())
        );
    }

    #[test]
    fn another_program_is_not_read_as_ours() {
        assert_eq!(parse("mix 0.0.9"), None);
        assert_eq!(parse("mixengined"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn a_program_that_is_not_there_has_no_version() {
        assert_eq!(
            version_of(std::path::Path::new("/nonexistent/mixengined")),
            None
        );
    }
}
