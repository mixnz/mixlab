//! What `--version` says about the build it came out of.
//!
//! **This is the only thing outside the binary that can tell the two apart**, which is why
//! `packaging/*/build.sh` reads it: a staged artifact that admits to being a development build is
//! one whose users would find their home directory renamed under them.

use std::process::Command;

#[test]
fn a_build_that_is_not_a_release_says_so_in_its_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_mix"))
        .arg("--version")
        .output()
        .expect("mix runs");

    let printed = String::from_utf8_lossy(&output.stdout).into_owned();

    // The number itself is not the interesting half and is asserted anyway: a `--version` that
    // stopped naming the version would pass every other assertion here.
    assert!(printed.contains(env!("CARGO_PKG_VERSION")), "{printed}");
    assert_eq!(
        printed.contains("(development build)"),
        !mixengine_platform::RELEASE,
        "{printed}"
    );
}
