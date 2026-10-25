//! Renewal on a clock, against a real `mixengined`.
//!
//! What the unit tests beside `certs::renewal` prove is that a report becomes the right decision.
//! What is proved here is the part only a daemon can be wrong about: that the loop runs on its own
//! timer at all, and that a certificate it replaces is handed to the generator rather than left in
//! a directory nothing re-reads.
//!
//! The daemon handle is held by each test rather than by a fixture, because one of them has to stop
//! it and start another against the same home.

use std::path::Path;

use mixengine_testkit::{Client, Daemon, Home};
use serde_json::json;

/// What the renewal loop says once it has replaced a certificate and handed it to the generator.
const RENEWED: &str = "renewed certificates and told the front end";

/// A directory to register a project against.
fn repository() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("mixengine-project")
        .tempdir()
        .expect("a temporary directory")
}

/// The certificates directory of a home, spelled the way the daemon spells it.
fn certs_of(home: &Home) -> std::path::PathBuf {
    mixengine_core::Paths::new(
        home.path().to_path_buf(),
        &mixengine_core::config::PathOverrides::default(),
    )
    .certs()
    .to_path_buf()
}

/// Leave this site holding a certificate with twenty days on it, and answer with its bytes and how
/// long the daemon's log was once they were on disk.
///
/// **The pair is removed before it is written**, which is not tidiness. `leaf::ensure` asks whether
/// what is already there is reusable *as of the `now` it is given*, and a certificate issued today
/// has a hundred and sixty days left as of seventy days ago — so backdating over one writes nothing
/// at all. That is what the guard below caught the first time this test ran.
///
/// **And it retries**, because the loop under test is running the whole time this happens: it can
/// issue a fresh certificate between the removal and the write, which is the state being arranged
/// rather than a failure.
///
/// **That same pass says it renewed**, which is why the length of the log is part of the answer. A
/// renewal line written before it is about a certificate this test removed, and only one written
/// after it can be about the one this test arranged. Taking it after the write rather than before
/// the removal is what leaves those earlier passes out: on a loaded machine one landed in the
/// window in about one run of five.
fn backdate(home: &Home, certs: &Path, certificate: &Path, key: &Path) -> (Vec<u8>, usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

    loop {
        let _ = std::fs::remove_file(certificate);
        let _ = std::fs::remove_file(key);

        mixengine_core::certs::leaf::ensure(
            certs,
            &["blog.test".to_owned()],
            None,
            std::time::SystemTime::now() - std::time::Duration::from_secs(70 * 24 * 60 * 60),
        )
        .expect("a backdated certificate is written");

        let written = std::fs::read(certificate).expect("the backdated certificate is readable");
        let state =
            mixengine_core::certs::leaf::read(certs, "blog.test", std::time::SystemTime::now());

        if matches!(
            &state,
            mixengine_proto::CertState::Present { cert }
                if cert.days_left <= mixengine_core::certs::leaf::RENEW_WITHIN_DAYS
        ) {
            return (written, home.daemon_log().len());
        }

        assert!(
            std::time::Instant::now() < deadline,
            "this site could not be left holding a certificate that is running out: {state:?}"
        );
    }
}

/// A project, and one site under it that declares HTTPS.
async fn a_site_with_https(home: &Home, repository: &Path) {
    let mut client = Client::connect(home).await;

    client
        .call(
            "project.create",
            json!({"root": repository.display().to_string(), "name": "blog"}),
        )
        .await;

    client
        .call(
            "site.create",
            json!({
                "project": {"name": "blog"},
                "domains": ["blog.test"],
                "kind": {"kind": "static"},
                "https": true,
            }),
        )
        .await;
}

/// **A running daemon renews, and the front end is told.**
///
/// The certificate assertion on its own would pass for a renewal wired only to the certificate
/// directory, which is exactly the failure this task exists to prevent — so the line saying the
/// generator ran is the half that matters, and it is one line rather than two so that it cannot be
/// written by a renewal that never got that far.
#[tokio::test]
async fn a_running_daemon_renews_a_certificate_that_is_running_out() {
    let home = Home::configured("[certs]\nrenew_check_seconds = 1\n");
    let _daemon = Daemon::start(env!("CARGO_BIN_EXE_mixengined"), &home);
    home.wait_until_listening().await;

    let repository = repository();
    a_site_with_https(&home, repository.path()).await;

    let certs = certs_of(&home);
    let certificate = mixengine_core::certs::leaf::certificate_path(&certs, "blog.test");
    let key = mixengine_core::certs::leaf::key_path(&certs, "blog.test");
    assert!(certificate.exists(), "the site was issued no certificate");

    // **The state the race leaves, made certain.** `backdate` removes the pair before it writes, and
    // a pass of the loop that lands in between finds the certificate missing, issues one and says
    // it renewed — before the certificate this test waits on exists. Under a loaded machine that
    // was 69 runs in 320; here it is every run, so the wait below is proved against it each time.
    std::fs::remove_file(&certificate).expect("the certificate is removed");
    std::fs::remove_file(&key).expect("the key is removed");
    home.wait_until_daemon_log_says(RENEWED).await;

    let (backdated, mark) = backdate(&home, &certs, &certificate, &key);

    // **Both halves, waited for together.** A certificate on disk that is not the backdated one says
    // a renewal ran after it was written; a renewal line after `mark` says one was handed to the
    // generator after that. Either alone is satisfied by the pass the lines above arranged.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let on_disk = std::fs::read(&certificate).ok();
        let log = home.daemon_log();
        let replaced = on_disk.as_ref().is_some_and(|bytes| *bytes != backdated);
        let told = said_after(&log, mark, RENEWED);

        if replaced && told {
            return;
        }

        assert!(
            std::time::Instant::now() < deadline,
            "waited 30s for a renewal of the backdated certificate: replaced on disk {replaced}, \
             {RENEWED:?} after byte {mark} of the log {told}\n--- daemon.log ---\n{log}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Whether `log` says `wanted` anywhere from byte `mark` on.
///
/// Bytes rather than a `&str` slice, because `mark` is the length of a log the daemon was still
/// writing, which can fall inside a character.
fn said_after(log: &str, mark: usize, wanted: &str) -> bool {
    log.as_bytes().get(mark..).is_some_and(|tail| {
        tail.windows(wanted.len())
            .any(|window| window == wanted.as_bytes())
    })
}

/// **The half of renewal that already worked and had no test behind it.**
///
/// Every start issues for every site that declares HTTPS, which is what keeps a machine switched
/// off more than it is on current forever. Nothing asserted it: every suite that exercises issuance
/// creates its site through the API, which issues on its own path, so a start that stopped issuing
/// would have gone unnoticed until somebody's padlock went red.
///
/// **The daemon is killed rather than asked to stop**, on `tests/lifecycle.rs`' reasoning: what is
/// under test is what a *start* does, and a stop that ran its destructors would be a second thing
/// this test depends on.
#[tokio::test]
async fn a_start_issues_for_a_site_whose_certificate_is_missing() {
    let home = Home::new();
    let daemon = Daemon::start(env!("CARGO_BIN_EXE_mixengined"), &home);
    home.wait_until_listening().await;

    let repository = repository();
    a_site_with_https(&home, repository.path()).await;

    let certs = certs_of(&home);
    let certificate = mixengine_core::certs::leaf::certificate_path(&certs, "blog.test");
    let key = mixengine_core::certs::leaf::key_path(&certs, "blog.test");
    assert!(certificate.exists(), "the site was never issued one");

    // The state a home restored from a backup that skipped `certs/sites/` comes back in.
    std::fs::remove_file(&certificate).expect("the certificate is removed");
    std::fs::remove_file(&key).expect("the key is removed");

    daemon.kill();
    home.wait_until_gone().await;

    let _second = Daemon::start(env!("CARGO_BIN_EXE_mixengined"), &home);

    // **Waited for by the log and not by the endpoint.** The listener is bound before the start
    // issues anything, so a connection can be answered while this is still to come. The line
    // appears only on this second start: the first issued nothing, because the site did not exist
    // yet when it ran.
    home.wait_until_daemon_log_says("signed certificates for this home's sites")
        .await;

    assert!(
        certificate.exists(),
        "a start left a site declaring HTTPS with no certificate"
    );
}
