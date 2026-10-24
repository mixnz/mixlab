use super::*;

/// A machine with browser databases, none of which hold the authority yet.
fn a_machine_with_a_browser(home: &std::path::Path) -> Arc<mixengine_platform::mock::Host> {
    Arc::new(mixengine_platform::mock::Host::with_browsers(
        home,
        mixengine_platform::BrowserSurvey::Reached {
            databases: vec![mixengine_platform::DatabaseState {
                path: "/home/someone/.pki/nssdb".to_owned(),
                owner: "Chrome and Chromium".to_owned(),
                installed: false,
                because: Some("it does not hold this authority".to_owned()),
            }],
        },
    ))
}

fn paths_under(home: &std::path::Path) -> mixengine_core::Paths {
    mixengine_core::Paths::new(
        home.to_path_buf(),
        &mixengine_core::config::PathOverrides::default(),
    )
}

/// A site row, built here rather than through a `Store`.
///
/// **`issue` takes records and never a `SiteRef`**, which is what lets this test exist without a
/// database at all — and the reason for the signature is not the test: resolving a reference lives
/// on `crate::sites::Sites`, and *that* struct holds a `Certificates`. A `Certificates` that
/// resolved references would close the loop.
fn a_site(domains: &[&str]) -> mixengine_core::sites::SiteRecord {
    mixengine_core::sites::SiteRecord {
        id: 1,
        owner: mixengine_core::sites::SiteOwner::Project(1),
        doc_root: String::new(),
        kind: mixengine_proto::SiteKind::Static,
        https_enabled: true,
        https_redirect: false,
        state: mixengine_proto::SiteState::Enabled,
        domains: domains.iter().map(|one| (*one).to_owned()).collect(),
        services: Vec::new(),
        routes: Vec::new(),
        sharing: None,
    }
}

fn a_leaf(sans: &[&str], days_left: i64, fingerprint: &str) -> mixengine_proto::SiteCert {
    mixengine_proto::SiteCert {
        subject: "CN=blog.test".to_owned(),
        sans: sans.iter().map(|one| (*one).to_owned()).collect(),
        issuer: "CN=MixEngine Local CA 9dc40c23".to_owned(),
        fingerprint: fingerprint.to_owned(),
        not_before: mixengine_proto::Timestamp(0),
        not_after: mixengine_proto::Timestamp(0),
        days_left,
    }
}

fn on_disk(cert: mixengine_proto::SiteCert) -> mixengine_proto::CertState {
    mixengine_proto::CertState::Present { cert }
}

fn presented(cert: mixengine_proto::SiteCert) -> mixengine_proto::Handshake {
    mixengine_proto::Handshake::Presented {
        cert,
        trust: mixengine_proto::Verdict::Trusted {},
    }
}

fn blog() -> Vec<String> {
    vec!["blog.test".to_owned()]
}

/// **A site that declares no HTTPS has nothing wrong with it** — the same distinction
/// `IssueOutcome::NotWanted` draws, and the third time it has had to be made. Without this rule
/// every plaintext site in the home would be reported as missing a certificate it never asked for.
#[test]
fn a_site_that_declares_no_https_has_nothing_wrong_with_it() {
    assert_eq!(
        problem(
            false,
            &["plain.test".to_owned()],
            &mixengine_proto::CertState::Absent {},
            &mixengine_proto::Handshake::NotAsked {},
        ),
        None
    );
}

#[test]
fn a_site_with_no_certificate_on_disk_is_named_that_way() {
    assert_eq!(
        problem(
            true,
            &blog(),
            &mixengine_proto::CertState::Absent {},
            &mixengine_proto::Handshake::NotServed {
                because: "nothing answered".to_owned()
            },
        ),
        Some(mixengine_proto::CertProblem::NoCertificate)
    );
}

/// **A shared site is not a site whose names differ**, a defect LAN sharing first shipped with.
///
/// Sharing put the LAN address into the certificate and left this comparison reading the bare
/// domain list, so every shared site reported `NamesDiffer` for as long as it was shared. The
/// comparison has to ask the same question the issuer answered, which is what `leaf::covered` is
/// for.
#[test]
fn a_shared_site_whose_certificate_covers_its_address_has_no_problem() {
    let sharing = mixengine_core::sites::Sharing {
        interface: "Wi-Fi".to_owned(),
        address: [192, 168, 1, 10].into(),
        since: mixengine_proto::Timestamp(1),
        until: None,
    };

    let cert = a_leaf(
        &["blog.test", "blog-mixengine.local", "192.168.1.10"],
        80,
        "aa",
    );

    // **The bare domain list is the defect, kept here as the other half of the assertion.** This is
    // what the call site passed before the fix, and it is why every shared site reported a problem
    // it did not have. Asserted rather than described, so that a change that quietly reinstates it
    // fails here.
    assert_eq!(
        problem(
            true,
            &blog(),
            &on_disk(cert.clone()),
            &presented(cert.clone())
        ),
        Some(mixengine_proto::CertProblem::NamesDiffer)
    );

    let covered = mixengine_core::certs::leaf::covered(&blog(), Some(&sharing));

    assert_eq!(
        problem(true, &covered, &on_disk(cert.clone()), &presented(cert)),
        None
    );
}

/// Judged against the **file** rather than the wire, because the fix is a reissue: no server
/// can serve a name nothing has signed.
#[test]
fn a_certificate_that_does_not_cover_the_sites_names_is_named_that_way() {
    let cert = a_leaf(&["blog.test"], 80, "aa");

    assert_eq!(
        problem(
            true,
            &["blog.test".to_owned(), "www.blog.test".to_owned()],
            &on_disk(cert.clone()),
            &presented(cert),
        ),
        Some(mixengine_proto::CertProblem::NamesDiffer)
    );
}

#[test]
fn a_site_nothing_answers_for_is_named_that_way() {
    let cert = a_leaf(&["blog.test"], 80, "aa");

    assert_eq!(
        problem(
            true,
            &blog(),
            &on_disk(cert),
            &mixengine_proto::Handshake::NotServed {
                because: "nothing answered on 127.0.0.1:443".to_owned()
            },
        ),
        Some(mixengine_proto::CertProblem::NotServed)
    );
}

/// **The report this task was built for**: the file is right and the server is still holding
/// the one before it. Every check that reads files calls this machine healthy.
#[test]
fn a_server_holding_the_previous_certificate_is_named_that_way() {
    assert_eq!(
        problem(
            true,
            &blog(),
            &on_disk(a_leaf(&["blog.test"], 80, "new")),
            &presented(a_leaf(&["blog.test"], 80, "old")),
        ),
        Some(mixengine_proto::CertProblem::ServedCertificateDiffers)
    );
}

#[test]
fn a_chain_the_verifier_refused_is_named_that_way() {
    let cert = a_leaf(&["blog.test"], 80, "aa");

    assert_eq!(
        problem(
            true,
            &blog(),
            &on_disk(cert.clone()),
            &mixengine_proto::Handshake::Presented {
                cert,
                trust: mixengine_proto::Verdict::Rejected {
                    because: "unknown issuer".to_owned()
                },
            },
        ),
        Some(mixengine_proto::CertProblem::NotTrusted)
    );
}

/// Last, because it is the only one of these that is not broken yet — and the renewal loop is
/// already replacing it.
#[test]
fn a_certificate_inside_the_renewal_window_is_named_last() {
    let cert = a_leaf(&["blog.test"], 10, "aa");

    assert_eq!(
        problem(true, &blog(), &on_disk(cert.clone()), &presented(cert)),
        Some(mixengine_proto::CertProblem::Expiring)
    );
}

#[test]
fn a_site_with_nothing_wrong_has_no_problem() {
    let cert = a_leaf(&["blog.test"], 80, "aa");

    assert_eq!(
        problem(true, &blog(), &on_disk(cert.clone()), &presented(cert)),
        None
    );
}

/// A site gets one, and the second ask writes nothing.
#[tokio::test]
async fn issuing_for_a_site_is_idempotent() {
    let home = tempfile::tempdir().expect("a temp home");
    let host = a_machine_with_a_browser(home.path());
    let paths = paths_under(home.path());
    std::fs::create_dir_all(paths.certs()).expect("the certs directory is made");

    let certificates = Certificates::reading(&paths, host);
    certificates.ensure().await.expect("an authority is made");

    let first = certificates
        .issue(Some(a_site(&["blog.test"])))
        .await
        .expect("it issues");

    assert_eq!(first.sites.len(), 1, "{first:?}");
    assert_eq!(first.sites[0].domain, "blog.test");
    assert!(
        matches!(first.sites[0].outcome, IssueOutcome::Issued {}),
        "{first:?}"
    );

    let second = certificates
        .issue(Some(a_site(&["blog.test"])))
        .await
        .expect("it answers");

    assert!(
        matches!(second.sites[0].outcome, IssueOutcome::Reused {}),
        "{second:?}"
    );
}

/// **A home with no authority refuses per site rather than failing the call**, so the answer
/// still names the site and says what is wrong with it.
#[tokio::test]
async fn a_home_with_no_authority_refuses_each_site_by_name() {
    let home = tempfile::tempdir().expect("a temp home");
    let host = a_machine_with_a_browser(home.path());
    let paths = paths_under(home.path());

    let report = Certificates::reading(&paths, host)
        .issue(Some(a_site(&["blog.test"])))
        .await
        .expect("it answers");

    assert_eq!(report.sites[0].domain, "blog.test");
    let IssueOutcome::Refused { because } = &report.sites[0].outcome else {
        panic!("a home with no authority issued something: {report:?}");
    };
    assert!(!because.is_empty());
}

/// **A site that declares no HTTPS wanted nothing, and did not refuse.**
///
/// The distinction is not cosmetic, which is why this test changed rather than being deleted. The
/// renewal loop announces every failure it finds, and with this outcome spelled `Refused` it would
/// announce one per plaintext site, once an hour, for as long as the daemon runs.
#[tokio::test]
async fn a_site_without_https_wanted_nothing_rather_than_being_refused() {
    let home = tempfile::tempdir().expect("a temp home");
    let host = a_machine_with_a_browser(home.path());
    let paths = paths_under(home.path());
    std::fs::create_dir_all(paths.certs()).expect("the certs directory is made");

    let certificates = Certificates::reading(&paths, host);
    certificates.ensure().await.expect("an authority is made");

    let plain = mixengine_core::sites::SiteRecord {
        https_enabled: false,
        https_redirect: false,
        ..a_site(&["blog.test"])
    };
    let report = certificates.issue(Some(plain)).await.expect("it answers");

    let IssueOutcome::NotWanted { because } = &report.sites[0].outcome else {
        panic!("a plaintext site is not a refusal: {report:?}");
    };
    assert!(because.contains("HTTPS"), "{because}");
    assert!(!mixengine_core::certs::leaf::certificate_path(paths.certs(), "blog.test").exists());
}

/// **A `Certificates` with no store answers for no sites at all rather than failing.** That is
/// the one built at start before the database is open, and a start that crashed there would be
/// a home nobody can reach to fix.
#[tokio::test]
async fn a_certificates_with_no_store_issues_for_nothing() {
    let home = tempfile::tempdir().expect("a temp home");
    let paths = paths_under(home.path());

    let report = Certificates::new(&paths)
        .issue(None)
        .await
        .expect("it answers");

    assert!(report.sites.is_empty(), "{report:?}");
}

/// **Every start asks the browsers to hold it, and asking costs no prompt.** The system store needs
/// an elevation batch; these databases are the user's own.
#[tokio::test]
async fn a_start_asks_this_machines_browsers_to_hold_the_authority() {
    let home = tempfile::tempdir().expect("a temp home");
    let host = a_machine_with_a_browser(home.path());
    let paths = paths_under(home.path());
    std::fs::create_dir_all(paths.certs()).expect("the certs directory is made");

    let certificates = Certificates::reading(&paths, host.clone());
    let made = certificates.ensure().await.expect("an authority is made");

    let change = certificates.install_in_browsers(&made.state).await;

    assert_eq!(change.written.len(), 1, "refused: {:?}", change.refused);
    assert_eq!(
        host.browsers_installed().len(),
        1,
        "the browsers were not asked"
    );
}

/// **The acceptance criterion, enumerated**, and the control that makes the enumeration mean
/// something.
///
/// `docs/features/tls.md` asks that `mix cert ca-uninstall` leave no MixEngine certificate in any
/// store, *"verified by an integration test that enumerates the stores"*. A machine running
/// `cargo test` has a real trust store it must not touch (testing rule 1) and may have no
/// `certutil` at all, so the enumeration happens here — against the mock's `browsers_removed`,
/// which exists for exactly this.
///
/// **The control is the first assertion.** "The list does not contain it" passes just as well when
/// the list was never built, which is the failure mode that fooled three measurements of the
/// browsers' stores before a handshake corrected them.
#[tokio::test]
async fn a_removal_names_the_authority_every_browser_was_asked_to_hold() {
    let home = tempfile::tempdir().expect("a temp home");
    let host = a_machine_with_a_browser(home.path());
    let paths = paths_under(home.path());
    std::fs::create_dir_all(paths.certs()).expect("the certs directory is made");

    let certificates = Certificates::reading(&paths, host.clone());
    let made = certificates.ensure().await.expect("an authority is made");
    let CaState::Present { ca } = &made.state else {
        panic!("this home has an authority: {made:?}");
    };

    certificates.install_in_browsers(&made.state).await;

    assert_eq!(
        host.browsers_installed().len(),
        1,
        "the control: the browsers were asked to hold it in the first place"
    );

    let change = certificates.remove_from_browsers(&ca.key_id).await;

    assert_eq!(
        host.browsers_removed(),
        vec![ca.key_id.clone()],
        "the removal names this home's authority, by key-id and never by fingerprint; \
             refused: {:?}",
        change.refused
    );
}

/// **A home with no authority asks for nothing**, which is what a start whose generation failed
/// gets. Writing something a browser would have to be told to forget is worse than writing
/// nothing.
#[tokio::test]
async fn a_home_with_no_authority_asks_the_browsers_for_nothing() {
    let home = tempfile::tempdir().expect("a temp home");
    let host = a_machine_with_a_browser(home.path());
    let paths = paths_under(home.path());

    let change = Certificates::reading(&paths, host.clone())
        .install_in_browsers(&CaState::Absent {})
        .await;

    // The mock records everything it is asked, so an empty log is the assertion: a home with no
    // authority never reaches the browsers at all.
    assert!(change.written.is_empty(), "wrote: {change:?}");
    assert!(host.browsers_installed().is_empty());
}

/// **And a damaged one is not written either**, which is the half an `Absent` home does not cover:
/// `Unusable` is a certificate that exists, and installing one into somebody's browser would be
/// spending their trust on something a rotation has to replace anyway.
#[tokio::test]
async fn a_damaged_authority_is_not_written_into_a_browser() {
    let home = tempfile::tempdir().expect("a temp home");
    let host = a_machine_with_a_browser(home.path());
    let paths = paths_under(home.path());

    let change = Certificates::reading(&paths, host.clone())
        .install_in_browsers(&CaState::Unusable {
            because: mixengine_proto::Unusable::KeyMissing,
        })
        .await;

    assert!(change.written.is_empty(), "wrote: {change:?}");
    assert!(host.browsers_installed().is_empty());
}

/// And the status carries what the browsers said, beside what the machine said.
#[tokio::test]
async fn a_status_reports_the_browsers_beside_the_trust_store() {
    let home = tempfile::tempdir().expect("a temp home");
    let host = a_machine_with_a_browser(home.path());
    let paths = paths_under(home.path());
    std::fs::create_dir_all(paths.certs()).expect("the certs directory is made");

    let certificates = Certificates::reading(&paths, host);
    certificates.ensure().await.expect("an authority is made");

    let status = certificates.status().await.expect("a status");

    let Browsers::Reached { databases } = status.browsers else {
        panic!(
            "the fixture's databases were not reported: {:?}",
            status.browsers
        );
    };

    assert_eq!(databases.len(), 1);
    assert!(!databases[0].installed);
    assert_eq!(databases[0].owner, "Chrome and Chromium");
}
