//! The one thing the platform layer decides about paths: where the root goes by default.
//!
//! This reads the environment but never writes it, and never creates a directory — `default_home`
//! answers a question, it does not act on the machine.
//!
//! The D-numbers below are those of
//! `docs/specs/2026-09-06-t95-a-build-that-is-not-a-release-keeps-its-own-home-design.md`.

use mixengine_platform::{Host as _, host, mock};

#[test]
fn the_default_root_follows_this_os_convention() {
    let host = host();

    let root = host.home_dirs().default_home().unwrap();

    assert!(root.is_absolute(), "{} is not absolute", root.display());
    // Windows and macOS name application directories the way their users see them; XDG is
    // lowercase.
    let released = if cfg!(target_os = "linux") {
        "mixengine"
    } else {
        "MixEngine"
    };
    // **This suite is compiled by cargo and so never carries the release marker.** Written against
    // `RELEASE` rather than against the development name, because a test that asserted the released
    // one could only pass in a build no test run produces, and a test that asserted the development
    // one would stop describing the product.
    let expected = if mixengine_platform::RELEASE {
        released.to_owned()
    } else {
        format!("{released}-dev")
    };
    assert_eq!(root.file_name().unwrap(), expected.as_str());
    // Under the user's data directory, not at the top of a drive or a filesystem.
    assert!(
        root.parent()
            .is_some_and(|parent| parent.parent().is_some())
    );
}

/// The claim D3 makes, which is about where a person will look rather than about a string.
#[test]
fn the_development_home_is_a_sibling_of_the_released_one() {
    let root = host().home_dirs().default_home().unwrap();
    let leaf = root.file_name().unwrap().to_string_lossy().into_owned();
    let released = if cfg!(target_os = "linux") {
        "mixengine"
    } else {
        "MixEngine"
    };

    // Same parent directory and a leaf that differs only by a suffix, so a developer wondering
    // where their sites went finds the answer in the folder they were already looking at.
    assert!(leaf.starts_with(released), "{leaf}");
    assert_eq!(leaf == released, mixengine_platform::RELEASE, "{leaf}");
}

#[test]
fn asking_twice_gives_the_same_answer() {
    let host = host();

    assert_eq!(
        host.home_dirs().default_home().unwrap(),
        host.home_dirs().default_home().unwrap()
    );
}

#[test]
fn the_mock_answers_with_whatever_the_test_chose() {
    let host = mock::Host::with_home("/somewhere/for/this/test");

    assert_eq!(
        host.home_dirs().default_home().unwrap(),
        std::path::Path::new("/somewhere/for/this/test")
    );
}

#[test]
fn a_mock_without_a_home_reports_it_instead_of_guessing() {
    let host = mock::Host::without_home();

    let error = host.home_dirs().default_home().unwrap_err();

    assert!(
        matches!(error, mixengine_platform::Error::NoHomeDirectory { .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("MIXENGINE_HOME"), "{error}");
}
