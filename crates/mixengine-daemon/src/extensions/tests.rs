use super::*;

/// A web-app runs no process, and the refusal says what controls it instead.
#[test]
fn a_web_app_is_controlled_as_a_site() {
    let refusal = served_as_a_site(
        &ExtensionId::parse("phpmyadmin").expect("an id"),
        "phpmyadmin.mixengine.test",
    );

    assert_eq!(refusal.code, ErrorCode::PreconditionFailed);
    assert!(
        refusal.message.contains("served as a site"),
        "{}",
        refusal.message
    );
    assert!(
        refusal
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("mix site stop phpmyadmin.mixengine.test")),
        "{refusal:?}"
    );
}

/// The one thing this type decides for itself.
#[test]
fn a_relative_path_is_refused() {
    let outcome = absolute("mailpit");

    let error = outcome.expect_err("a relative path is not something the daemon can resolve");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// **A consent is spent on what it named, and on nothing else.**
#[test]
fn a_consent_for_another_version_is_refused() {
    let manifest = manifest::read(
        Path::new("extension.toml"),
        mixengine_testkit::extension::MAILPIT,
    )
    .expect("a fixture parses");

    let agreed = ExtensionConsent {
        id: manifest.extension.id.clone(),
        version: mixengine_proto::PackageVersion::parse("0.0.1".to_owned()).expect("a version"),
        signed: true,
        network: manifest.permissions.network,
    };

    let refusal = agrees(&agreed, &manifest, true).expect_err("refused");
    assert_eq!(refusal.code, ErrorCode::PreconditionFailed);
}

/// Including when what changed is whether anything vouches for it.
#[test]
fn a_consent_given_for_a_signed_extension_is_not_spent_on_an_unsigned_one() {
    let manifest = manifest::read(
        Path::new("extension.toml"),
        mixengine_testkit::extension::MAILPIT,
    )
    .expect("a fixture parses");

    let agreed = ExtensionConsent {
        id: manifest.extension.id.clone(),
        version: manifest.extension.version.clone(),
        signed: true,
        network: manifest.permissions.network,
    };

    let refusal = agrees(&agreed, &manifest, false).expect_err("refused");
    assert_eq!(refusal.code, ErrorCode::PreconditionFailed);
}

/// And the ordinary case passes.
#[test]
fn a_consent_naming_what_is_installed_agrees() {
    let manifest = manifest::read(
        Path::new("extension.toml"),
        mixengine_testkit::extension::MAILPIT,
    )
    .expect("a fixture parses");

    let agreed = ExtensionConsent {
        id: manifest.extension.id.clone(),
        version: manifest.extension.version.clone(),
        signed: false,
        network: manifest.permissions.network,
    };

    agrees(&agreed, &manifest, false).expect("the consent describes it");
}
