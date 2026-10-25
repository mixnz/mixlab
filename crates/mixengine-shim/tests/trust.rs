//! Telling a runtime about this home's certificate authority.
//!
//! The third complaint this phase answers, end to end: a Node process fetching
//! `https://ezweb.test` reported `unable to verify the first certificate` while a browser on the
//! same machine showed a padlock on the same site. Both were right. MixEngine installs its
//! authority into the **operating system's** trust store, and no language runtime reads that store
//! — Node ships a set of its own, Ruby's OpenSSL resolves a file inside the moved tree, Python uses
//! OpenSSL's defaults and `certifi` above them, and PHP's Windows artifact ships no CA file at all.
//!
//! What is asserted here is which variable each language is handed and which file it names, because
//! the difference between the two files is the whole design: `NODE_EXTRA_CA_CERTS` **adds** to what
//! Node already trusts, so Node is handed the authority itself; every other mechanism **replaces** a
//! trust store, so those are handed the merged bundle and keep the public internet.

mod harness;

use std::collections::BTreeMap;

use harness::Home;
use mixengine_proto::RuntimeKind;

/// Variables a machine may already export, taken out of the child before every case below.
///
/// **`ubuntu-latest` exports `SSL_CERT_FILE`**, so a shim running there leaves it exactly as it
/// found it, and a case asserting MixEngine's own value arrives would be asserting about the
/// runner. `a_variable_somebody_set_is_left_alone` below is that behaviour's own case, and it sets
/// the variable itself rather than hoping a machine does.
const CLEARED: &[&str] = &["SSL_CERT_FILE", "REQUESTS_CA_BUNDLE", "NODE_EXTRA_CA_CERTS"];

/// What a Node artifact publishes, spelled as each system really packs it.
fn node_provides() -> BTreeMap<String, String> {
    let at = match cfg!(windows) {
        true => "node.exe",
        false => "bin/node",
    };

    [("node".to_owned(), at.to_owned())].into_iter().collect()
}

/// **Node keeps its own roots and gains ours.** `NODE_EXTRA_CA_CERTS` is the one mechanism here
/// that is additive, so it names the authority itself rather than the merged bundle — Node's own
/// curated set is left exactly as Node shipped it.
#[test]
fn node_is_handed_the_authority_and_keeps_its_own_roots() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Node, "24.19.0", node_provides());
    home.write_authority();
    home.write_trust_bundle();

    let recorded = home.record_without("node", home.path(), CLEARED, 0);

    let named = recorded
        .recorded("NODE_EXTRA_CA_CERTS")
        .expect("node is told about this home's authority");

    assert!(named.ends_with("root.crt"), "{named}");
    assert!(!named.contains("bundle.pem"), "{named}");
}

/// **Ruby replaces rather than adds**, so it is handed the bundle — which holds this machine's own
/// roots, so `gem install` still reaches the public internet.
#[test]
fn ruby_is_handed_the_bundle_and_not_the_authority_alone() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Ruby, "3.4.10", ruby_provides());
    home.write_authority();
    home.write_trust_bundle();

    let recorded = home.record_without("ruby", home.path(), CLEARED, 0);

    let named = recorded
        .recorded("SSL_CERT_FILE")
        .expect("ruby is told which file to trust");

    assert!(named.ends_with("bundle.pem"), "{named}");
}

/// Python reads two, because `requests` ignores the OpenSSL one and consults `certifi` instead.
#[test]
fn python_is_handed_both_of_the_variables_it_reads() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Python, "3.13.15", python_provides());
    home.write_authority();
    home.write_trust_bundle();

    let recorded = home.record_without("python", home.path(), CLEARED, 0);

    for variable in ["SSL_CERT_FILE", "REQUESTS_CA_BUNDLE"] {
        let named = recorded
            .recorded(variable)
            .unwrap_or_else(|| panic!("python is told {variable}"));

        assert!(named.ends_with("bundle.pem"), "{variable} is {named}");
    }
}

/// **A variable the person set is theirs.** Somebody who exported `SSL_CERT_FILE` for a corporate
/// authority meant it, and a tool that overrode it would be one that cannot be used inside the
/// company that installed it.
#[test]
fn a_variable_somebody_set_is_left_alone() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Ruby, "3.4.10", ruby_provides());
    home.write_authority();
    home.write_trust_bundle();

    let session = [("SSL_CERT_FILE", "/etc/corporate/roots.pem".to_owned())]
        .into_iter()
        .collect();
    let recorded = home.record_command("ruby", home.path(), &session, 0);

    assert_eq!(
        recorded.recorded("SSL_CERT_FILE"),
        Some("/etc/corporate/roots.pem")
    );
}

/// **And nothing is named for a file that is not there** — `surroundings`' existing rule, which is
/// why `PHP_INI_SCAN_DIR` is behind an `is_dir`. A home whose daemon has never run, or whose trust
/// store could not be read, leaves every runtime exactly as it was.
#[test]
fn a_bundle_that_is_not_there_is_named_to_nobody() {
    let home = Home::with(&["8.3.33"]);
    home.install(RuntimeKind::Ruby, "3.4.10", ruby_provides());
    home.install(RuntimeKind::Node, "24.19.0", node_provides());

    let ruby = home.record_without("ruby", home.path(), CLEARED, 0);
    assert_eq!(ruby.recorded("SSL_CERT_FILE"), None);

    let node = home.record_without("node", home.path(), CLEARED, 0);
    assert_eq!(node.recorded("NODE_EXTRA_CA_CERTS"), None);
}

/// PHP is told through the generated ini set instead, so that a terminal and a pool agree — which
/// is what `crates/mixengine-core/src/runtimes/extensions.rs` asserts. What is checked here is the
/// other half of that decision: no variable is exported for it, so there is no second answer.
#[test]
fn php_is_told_through_its_ini_set_and_not_through_a_variable() {
    let home = Home::with(&["8.3.33"]);
    home.write_authority();
    home.write_trust_bundle();

    let recorded = home.record_without("php", home.path(), CLEARED, 0);

    assert_eq!(recorded.recorded("SSL_CERT_FILE"), None);
    assert_eq!(recorded.recorded("NODE_EXTRA_CA_CERTS"), None);
}

fn ruby_provides() -> BTreeMap<String, String> {
    let at = match cfg!(windows) {
        true => "bin/ruby.exe",
        false => "bin/ruby",
    };

    [("ruby".to_owned(), at.to_owned())].into_iter().collect()
}

fn python_provides() -> BTreeMap<String, String> {
    let at = match cfg!(windows) {
        true => "python.exe",
        false => "bin/python3",
    };

    [("python".to_owned(), at.to_owned())].into_iter().collect()
}
