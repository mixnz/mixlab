//! Keeping a Go to the release a directory resolved to.
//!
//! The archive's `go.env` says `GOTOOLCHAIN=auto`, which lets a `go.mod` asking for a newer Go
//! download that Go and run it instead of the one installed. What is asserted here is what the
//! program a shim starts really inherits: `GOTOOLCHAIN=local` for `go`, nothing for any other
//! language, and a value the person exported arriving exactly as they wrote it.

mod harness;

use std::collections::BTreeMap;

use harness::Home;
use mixengine_proto::RuntimeKind;

/// A machine somebody set up may already export it; each case below starts without it.
const CLEARED: &[&str] = &["GOTOOLCHAIN"];

/// What a Go artifact publishes, spelled as each system really packs it.
fn go_provides() -> BTreeMap<String, String> {
    let at = match cfg!(windows) {
        true => "bin/go.exe",
        false => "bin/go",
    };

    [("go".to_owned(), at.to_owned())].into_iter().collect()
}

fn node_provides() -> BTreeMap<String, String> {
    let at = match cfg!(windows) {
        true => "node.exe",
        false => "bin/node",
    };

    [("node".to_owned(), at.to_owned())].into_iter().collect()
}

/// **The pin means the release that was resolved.** A `go` started through `bin/` is told `local`.
#[test]
fn go_is_kept_to_the_toolchain_it_resolved_to() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Go, "1.25.14", go_provides());

    let recorded = home.record_without("go", home.path(), CLEARED, 0);

    assert!(
        recorded.reached,
        "the shim refused: {}",
        recorded.run.stderr()
    );
    assert_eq!(recorded.recorded("GOTOOLCHAIN"), Some("local"));
}

/// **Only Go.** Another language has no use for the variable, and a shim that exported it to
/// everything would put Go's setting into every Node child process a build spawns.
#[test]
fn no_other_language_is_told_about_a_go_toolchain() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Node, "24.19.0", node_provides());

    let recorded = home.record_without("node", home.path(), CLEARED, 0);

    assert!(
        recorded.reached,
        "the shim refused: {}",
        recorded.run.stderr()
    );
    assert_eq!(recorded.recorded("GOTOOLCHAIN"), None);
}

/// **A value the person set is theirs** — ADR 0034's rule. Somebody who exported
/// `GOTOOLCHAIN=go1.27.1+auto` asked for exactly that.
#[test]
fn a_toolchain_somebody_exported_is_left_alone() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Go, "1.25.14", go_provides());

    let session = [("GOTOOLCHAIN", "go1.27.1+auto".to_owned())]
        .into_iter()
        .collect();
    let recorded = home.record_command("go", home.path(), &session, 0);

    assert_eq!(recorded.recorded("GOTOOLCHAIN"), Some("go1.27.1+auto"));
}

/// **Go is told nothing about trust**: it reads the operating system's store,
/// which is where this home's authority already is.
#[test]
fn go_is_handed_no_trust_variable() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Go, "1.25.14", go_provides());
    home.write_authority();
    home.write_trust_bundle();

    let recorded = home.record_without(
        "go",
        home.path(),
        &[
            "GOTOOLCHAIN",
            "SSL_CERT_FILE",
            "REQUESTS_CA_BUNDLE",
            "NODE_EXTRA_CA_CERTS",
        ],
        0,
    );

    assert!(
        recorded.reached,
        "the shim refused: {}",
        recorded.run.stderr()
    );
    for variable in ["SSL_CERT_FILE", "REQUESTS_CA_BUNDLE", "NODE_EXTRA_CA_CERTS"] {
        assert_eq!(recorded.recorded(variable), None, "{variable}");
    }
}
