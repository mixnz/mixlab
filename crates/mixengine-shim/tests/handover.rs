//! The shim hands over to the next program on the PATH when MixEngine has nothing to say — roadmap
//! task **T185b**, its design's D3.
//!
//! `bin/` is first on the PATH, so a `php` there stands in front of any PHP the person installed
//! themselves. When the directory asked for no version and there is no default, the shim runs the
//! next `php` on the PATH instead of refusing — and never when a pin asked for one nothing matches,
//! because running another version than the pin named is worse than an error.

mod harness;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use harness::Home;

/// A directory holding a program named `php`, standing for one the person installed themselves.
fn their_own_php(dir: &Path) -> PathBuf {
    let program = dir.join(format!("php{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(mixengine_testkit::package::executable_source(), &program)
        .unwrap_or_else(|error| panic!("copy a program to {}: {error}", program.display()));
    dir.to_path_buf()
}

/// A `PATH` of exactly these entries.
fn path_of(entries: &[PathBuf]) -> String {
    std::env::join_paths(entries)
        .expect("joinable entries")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn with_nothing_asked_the_next_php_on_the_path_runs() {
    let home = Home::with(&[]);
    let elsewhere = tempfile::tempdir().expect("a directory of their own");
    let theirs = their_own_php(elsewhere.path());
    // This home's `bin/` spelled with a trailing separator: still this home's, still skipped.
    let bin = PathBuf::from(format!(
        "{}{}",
        home.path().join("bin").display(),
        std::path::MAIN_SEPARATOR
    ));

    let recorded = home.record(
        elsewhere.path(),
        &BTreeMap::from([("PATH", path_of(&[bin, theirs]))]),
        7,
    );

    assert!(
        recorded.reached,
        "their php never ran\n--- the shim said ---\n{}",
        recorded.run.stderr()
    );
    assert_eq!(recorded.run.code(), 7, "{}", recorded.run.stderr());
    assert_eq!(
        recorded.recorded("MIXENGINE_SHIM_HANDED_OVER"),
        Some("1"),
        "the handed-over program is told, so a second shim does not hand over again"
    );
}

#[test]
fn with_nothing_further_down_it_refuses_as_before() {
    let home = Home::with(&[]);
    let cwd = tempfile::tempdir().expect("a directory");

    let recorded = home.record(
        cwd.path(),
        &BTreeMap::from([("PATH", path_of(&[home.path().join("bin")]))]),
        0,
    );

    assert!(!recorded.reached, "{}", recorded.run.stderr());
    assert_eq!(recorded.run.code(), 127, "{}", recorded.run.stderr());
}

#[test]
fn a_pin_nothing_matches_never_hands_over() {
    let home = Home::with(&["8.3.1"]);
    let project = home.project("pinned", Some("[runtimes]\nphp = \"9.9\"\n"));
    let elsewhere = tempfile::tempdir().expect("a directory of their own");
    let theirs = their_own_php(elsewhere.path());

    let recorded = home.record(
        &project,
        &BTreeMap::from([("PATH", path_of(&[home.path().join("bin"), theirs]))]),
        0,
    );

    assert!(
        !recorded.reached,
        "a pin for 9.9 ran somebody else's php\n--- the shim said ---\n{}",
        recorded.run.stderr()
    );
    assert_eq!(recorded.run.code(), 127, "{}", recorded.run.stderr());
}

#[test]
fn a_shim_that_was_handed_over_to_does_not_hand_over_again() {
    let home = Home::with(&[]);
    let elsewhere = tempfile::tempdir().expect("a directory of their own");
    let theirs = their_own_php(elsewhere.path());

    let recorded = home.record(
        elsewhere.path(),
        &BTreeMap::from([
            ("PATH", path_of(&[home.path().join("bin"), theirs])),
            ("MIXENGINE_SHIM_HANDED_OVER", "1".to_owned()),
        ]),
        0,
    );

    assert!(!recorded.reached, "{}", recorded.run.stderr());
    assert_eq!(recorded.run.code(), 127, "{}", recorded.run.stderr());
}

#[test]
fn two_homes_on_one_path_do_not_bounce() {
    let home = Home::with(&[]);
    let other = Home::with(&[]);
    let cwd = tempfile::tempdir().expect("a directory");

    let recorded = home.record(
        cwd.path(),
        &BTreeMap::from([(
            "PATH",
            path_of(&[home.path().join("bin"), other.path().join("bin")]),
        )]),
        0,
    );

    assert!(!recorded.reached, "{}", recorded.run.stderr());
    assert_eq!(recorded.run.code(), 127, "{}", recorded.run.stderr());
}
