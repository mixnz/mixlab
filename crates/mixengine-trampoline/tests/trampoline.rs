//! The trampoline on its own, with no shim and no home behind it.
//!
//! What it does *with* a shim is `crates/mixengine-shim/tests/shim.rs`', which fills a real `bin/`
//! and runs every case through it. This file is for the one thing it answers by itself: a `bin/`
//! with no pointer to the resolver.
//!
//! **It is also what gets the binary built.** `cargo test --workspace --all-targets` puts a
//! package's binary in `target/<profile>/` only for a package with an integration test, and the
//! shim's and the CLI's suites look for `mixengine-trampoline` there; without this file a clean CI
//! build never made one (found by the trampoline's first CI run).

use std::process::Command;

/// Copied into a directory of its own under a command's name, the way `shims::refresh` places it,
/// with nothing beside it: the pointer is missing, so it must say which file and exit as a missing
/// command does, and never start anything.
#[test]
fn a_trampoline_with_no_pointer_names_the_file_and_exits_127() {
    let bin = tempfile::tempdir().expect("a bin directory");
    let php = bin
        .path()
        .join(format!("php{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(env!("CARGO_BIN_EXE_mixengine-trampoline"), &php).expect("the trampoline");

    let ran = Command::new(&php)
        .arg("-v")
        .output()
        .expect("the trampoline runs");

    let said = String::from_utf8_lossy(&ran.stderr);
    assert_eq!(ran.status.code(), Some(127), "{said}");
    assert!(
        said.starts_with("php: "),
        "named as the command typed: {said}"
    );
    assert!(said.contains("mixengine-shim.path"), "{said}");
    assert!(ran.stdout.is_empty(), "nothing ran");
}
