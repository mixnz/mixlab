//! Recipes that want a certificate, and the drift around issuing one.

use super::*;

/// The id [`CertifiedFake`] renders under, and where its pair lands.
fn fakeservice() -> ServiceId {
    ServiceId::parse("fakeservice@main").expect("an id")
}

/// The one file [`Fake`] and [`CertifiedFake`] render, as installed.
fn rendered_file(home: &std::path::Path) -> String {
    std::fs::read_to_string(
        home.join("etc")
            .join("fakeservice@main")
            .join("fakeservice.conf"),
    )
    .expect("the rendered file is on disk")
}

/// **A recipe that asks gets a pair this home's authority signed, and the file names it** —
/// roadmap task **T99**, the design's D4 and D5.
#[tokio::test]
async fn a_recipe_that_wants_a_certificate_gets_one_on_a_home_with_an_authority() {
    let (home, generator) =
        home_of(Arc::new(CertifiedFake), "fakeservice@main", "main", "{}").await;
    let certs = generator.paths.certs().to_path_buf();
    std::fs::create_dir_all(&certs).expect("the certs directory");
    crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");

    generator.declared().await.expect("a rendering");

    let key = crate::certs::service::key_path(&certs, &fakeservice());
    let certificate = crate::certs::service::certificate_path(&certs, &fakeservice());
    assert!(key.is_file(), "{}", key.display());
    assert!(certificate.is_file(), "{}", certificate.display());

    let mixengine_proto::CertState::Present { cert } =
        crate::certs::service::read(&certs, &fakeservice(), std::time::SystemTime::now())
    else {
        panic!("the pair on disk is not usable");
    };

    let file = rendered_file(home.path());
    assert!(
        file.contains(&format!("cert = {}", certificate.display())),
        "{file}"
    );
    assert!(file.contains(&format!("key = {}", key.display())), "{file}");
    assert!(file.contains(&format!("# {}", cert.fingerprint)), "{file}");
}

/// **No authority, no certificate, and still a rendering** — the design's D4: the failure mode
/// is the status quo, never a service that will not start.
#[tokio::test]
async fn a_recipe_that_wants_a_certificate_still_renders_on_a_home_with_none() {
    let (home, generator) =
        home_of(Arc::new(CertifiedFake), "fakeservice@main", "main", "{}").await;

    generator.declared().await.expect("a rendering");

    let file = rendered_file(home.path());
    assert!(!file.contains("cert ="), "{file}");
    assert!(
        !generator.paths.certs().join("services").exists(),
        "something was written under certs/services without an authority"
    );
}

/// A recipe that does not ask is never issued one, authority or not.
#[tokio::test]
async fn a_recipe_that_wants_none_gets_none() {
    let (home, generator) = home("{}").await;
    let certs = generator.paths.certs().to_path_buf();
    std::fs::create_dir_all(&certs).expect("the certs directory");
    crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");

    generator.declared().await.expect("a rendering");

    assert!(!certs.join("services").exists());
    assert!(!rendered_file(home.path()).contains("cert ="));
}

/// **Reading in `prepare` is what keeps `drift` honest** — the design's D4. A drift check
/// renders without issuing, so it has to see the pair the install saw, or every home with a
/// database would drift for ever.
#[tokio::test]
async fn drift_is_nothing_after_a_render_that_issued() {
    let (_home, generator) =
        home_of(Arc::new(CertifiedFake), "fakeservice@main", "main", "{}").await;
    let certs = generator.paths.certs().to_path_buf();
    std::fs::create_dir_all(&certs).expect("the certs directory");
    crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");

    let before = generator.drift().await.expect("a drift");
    assert!(before.iter().any(|one| !one.drift.is_empty()), "{before:?}");

    generator.declared().await.expect("a rendering");

    let after = generator.drift().await.expect("a second drift");
    assert!(after.iter().all(|one| one.drift.is_empty()), "{after:?}");
}

/// The four questions hold across renders: a second one changes neither the pair nor the file.
#[tokio::test]
async fn a_second_generate_reuses_the_pair() {
    let (home, generator) =
        home_of(Arc::new(CertifiedFake), "fakeservice@main", "main", "{}").await;
    let certs = generator.paths.certs().to_path_buf();
    std::fs::create_dir_all(&certs).expect("the certs directory");
    crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");

    generator.declared().await.expect("a rendering");
    let first = rendered_file(home.path());

    let again = generator.declared().await.expect("a second rendering");
    assert!(!again[0].changed(), "a reused pair rewrote the file");
    assert_eq!(first, rendered_file(home.path()));
}

/// Asking must not install, or the check would repair what it was sent to report.
#[tokio::test]
async fn asking_for_drift_installs_nothing() {
    let (_home, generator) = home("{}").await;

    generator.drift().await.expect("a drift");

    let rendering = generator.declared().await.expect("a rendering");

    assert!(
        rendering[0].changed(),
        "the drift call had already written the configuration"
    );
}

/// A generated file somebody edited by hand is drift, and names the service it belongs to.
#[tokio::test]
async fn a_generated_file_edited_by_hand_drifts_under_its_own_service() {
    let (home_dir, generator) = home("{}").await;

    let rendered = generator.declared().await.expect("a rendering");
    let (file, _) = rendered[0]
        .files
        .first()
        .expect("the recipe renders at least one file");

    let quiet = generator.drift().await.expect("a drift");
    let service = quiet[0].service.clone();
    assert!(quiet[0].drift.is_empty(), "{quiet:?}");

    std::fs::write(
        home_dir
            .path()
            .join("etc")
            .join(service.as_str())
            .join(file),
        "tampered\n",
    )
    .expect("the generated file is writable");

    let after = generator.drift().await.expect("a second drift");

    assert_eq!(after[0].service, service);
    assert_eq!(after[0].drift.changed, vec![file.clone()]);
}
