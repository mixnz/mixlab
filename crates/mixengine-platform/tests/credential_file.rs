//! The development build's credential store: one file in a home.
//!
//! Driven through the public constructor a daemon uses, so what is proved is what the daemon gets.

use std::path::PathBuf;
use std::sync::Arc;

use mixengine_platform::{Credentials, Error, Host, host_with};

const SERVICE: &str = "mixengine";
const KEY: &str = "0123456789ab/mariadb@main/root";

fn store(directory: &tempfile::TempDir) -> (Arc<dyn Host>, PathBuf) {
    let path = directory.path().join("credentials.json");
    (host_with(Credentials::File(path.clone())), path)
}

#[test]
fn a_credential_is_read_back_from_the_file_it_was_written_to() {
    let directory = tempfile::tempdir().expect("a directory");
    let (host, path) = store(&directory);

    host.keyring()
        .set_secret(SERVICE, KEY, "hunter2")
        .expect("a write");

    assert_eq!(
        host.keyring()
            .secret(SERVICE, KEY)
            .expect("a read")
            .as_deref(),
        Some("hunter2")
    );
    assert!(path.is_file(), "nothing was written to {}", path.display());

    // A second host over the same file is a daemon that restarted.
    let (again, _) = store(&directory);
    assert_eq!(
        again
            .keyring()
            .secret(SERVICE, KEY)
            .expect("a read")
            .as_deref(),
        Some("hunter2")
    );
}

#[test]
fn an_absent_credential_is_none_and_creates_no_file() {
    let directory = tempfile::tempdir().expect("a directory");
    let (host, path) = store(&directory);

    assert_eq!(host.keyring().secret(SERVICE, KEY).expect("a read"), None);
    assert!(!path.exists(), "a read created the file");
}

#[test]
fn forgetting_twice_succeeds_and_leaves_the_neighbour() {
    let directory = tempfile::tempdir().expect("a directory");
    let (host, _) = store(&directory);
    let keyring = host.keyring();
    let neighbour = "0123456789ab/redis@main/default";

    keyring.set_secret(SERVICE, KEY, "one").expect("a write");
    keyring
        .set_secret(SERVICE, neighbour, "two")
        .expect("a write");

    keyring.forget_secret(SERVICE, KEY).expect("a removal");
    keyring
        .forget_secret(SERVICE, KEY)
        .expect("a removal of nothing");

    assert_eq!(keyring.secret(SERVICE, KEY).expect("a read"), None);
    assert_eq!(
        keyring
            .secret(SERVICE, neighbour)
            .expect("a read")
            .as_deref(),
        Some("two")
    );
}

#[test]
fn a_file_that_does_not_parse_is_an_error_and_not_an_empty_store() {
    let directory = tempfile::tempdir().expect("a directory");
    let (host, path) = store(&directory);
    let torn: &[u8] = b"{\"version\":1,\"entries\":{\"mixengine\":{\"a\":\"hun";
    std::fs::write(&path, torn).expect("a torn file");

    let read = host.keyring().secret(SERVICE, KEY);
    assert!(matches!(read, Err(Error::Secret { .. })), "{read:?}");

    // And a write does not paper over it: the torn file is what a person has to look at.
    let write = host.keyring().set_secret(SERVICE, KEY, "new");
    assert!(matches!(write, Err(Error::Secret { .. })), "{write:?}");
    assert_eq!(std::fs::read(&path).expect("still there"), torn);
}

#[test]
fn a_file_of_another_version_is_refused() {
    let directory = tempfile::tempdir().expect("a directory");
    let (host, path) = store(&directory);
    std::fs::write(&path, br#"{"version":2,"entries":{}}"#).expect("a newer file");

    let error = host
        .keyring()
        .secret(SERVICE, KEY)
        .expect_err("a version this build does not read was read");
    let chain = source_chain(&error);
    assert!(chain.contains("version 2"), "{error}: {chain}");
}

#[test]
fn no_error_carries_a_stored_value() {
    let directory = tempfile::tempdir().expect("a directory");
    let (host, path) = store(&directory);
    std::fs::write(
        &path,
        br#"{"version":1,"entries":{"mixengine":{"k":"s3cr3t-value"}} trailing"#,
    )
    .expect("a corrupt file holding a value");

    let error = host
        .keyring()
        .secret(SERVICE, "k")
        .expect_err("a corrupt file");
    let everything = format!("{error} {error:?} {}", source_chain(&error));
    assert!(!everything.contains("s3cr3t-value"), "{everything}");
}

#[cfg(unix)]
#[test]
fn only_this_account_can_read_it() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().expect("a directory");
    let (host, path) = store(&directory);
    host.keyring()
        .set_secret(SERVICE, KEY, "hunter2")
        .expect("a write");

    let mode = std::fs::metadata(&path)
        .expect("the file")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "{mode:o}");
}

#[cfg(windows)]
#[test]
fn only_this_account_can_read_it() {
    let directory = tempfile::tempdir().expect("a directory");
    let (host, path) = store(&directory);
    host.keyring()
        .set_secret(SERVICE, KEY, "hunter2")
        .expect("a write");

    assert!(mixengine_platform::is_private_file(&path).expect("its ACL"));
}

/// Every `source()` below `error`, joined — what a person sees with `{:#}` in anyhow.
fn source_chain(error: &dyn std::error::Error) -> String {
    let mut parts = Vec::new();
    let mut next = error.source();
    while let Some(cause) = next {
        parts.push(cause.to_string());
        next = cause.source();
    }
    parts.join(": ")
}

/// The keys under one service, and none of another's.
#[test]
fn keys_lists_one_service_and_never_another() {
    let directory = tempfile::tempdir().expect("a directory");
    let (host, _) = store(&directory);

    host.keyring()
        .set_secret(SERVICE, KEY, "a")
        .expect("a write");
    host.keyring()
        .set_secret(SERVICE, "0123456789ab/postgres@main/postgres", "b")
        .expect("a write");
    host.keyring()
        .set_secret("elsewhere", "x", "c")
        .expect("a write");

    let mut keys = host.keyring().keys(SERVICE).expect("a listing");
    keys.sort();

    assert_eq!(
        keys,
        [
            "0123456789ab/mariadb@main/root".to_owned(),
            "0123456789ab/postgres@main/postgres".to_owned()
        ]
    );
    assert!(
        host.keyring()
            .keys("nothing-here")
            .expect("a listing")
            .is_empty()
    );
}
