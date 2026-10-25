//! `keytool` against a real JDK — ADR 0039.
//!
//! **`#[ignore]`d, and pointed at a JDK by `MIXENGINE_TEST_JDK`** (its home, which is copied before
//! anything is written): a script standing in for `keytool` would prove the arguments and not the
//! store, and the store is the whole question — whether a `cacerts` written with the published
//! password is one a JVM still reads.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use mixengine_core::runtimes::java;
use mixengine_proto::CaState;

fn copied_jdk(into: &Path) -> PathBuf {
    let from = PathBuf::from(
        std::env::var_os("MIXENGINE_TEST_JDK").expect("MIXENGINE_TEST_JDK names a JDK home"),
    );
    copy_tree(&from, into);

    let executable = if cfg!(windows) {
        "keytool.exe"
    } else {
        "keytool"
    };
    into.join("bin").join(executable)
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a directory");

    for entry in std::fs::read_dir(from).expect("a readable JDK") {
        let entry = entry.expect("an entry");
        let target = to.join(entry.file_name());

        if entry.file_type().expect("a type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("a copy");
        }
    }
}

/// Held, asked about, let go, and let go again — the whole of what the daemon asks a JDK for.
#[tokio::test]
#[ignore = "needs a JDK: MIXENGINE_TEST_JDK names its home"]
async fn a_jdk_holds_and_lets_go_of_this_homes_authority() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let keytool = copied_jdk(&temp.path().join("jdk"));

    let certs = temp.path().join("certs");
    std::fs::create_dir_all(&certs).expect("a certificates directory");
    let made = mixengine_core::certs::ca::ensure(&certs, SystemTime::now()).expect("an authority");
    let CaState::Present { ca } = made else {
        panic!("this home has an authority: {made:?}");
    };
    let der = mixengine_core::certs::ca::der(&ca.certificate_pem).expect("the authority's DER");
    let certificate = mixengine_core::certs::ca::certificate_path(&certs);

    assert!(
        !java::holds(&keytool, &ca.key_id, &der)
            .await
            .expect("keytool answered"),
        "the control: a JDK holds nothing of ours before it is asked to"
    );

    java::hold(&keytool, &ca.key_id, &certificate)
        .await
        .expect("it holds the authority");
    assert!(
        java::holds(&keytool, &ca.key_id, &der)
            .await
            .expect("keytool answered")
    );

    java::release(&keytool, &ca.key_id)
        .await
        .expect("it lets the authority go");
    assert!(
        !java::holds(&keytool, &ca.key_id, &der)
            .await
            .expect("keytool answered")
    );

    java::release(&keytool, &ca.key_id)
        .await
        .expect("an alias that is not there is already let go");
}
