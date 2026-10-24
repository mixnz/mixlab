//! A service row rendered: idle policy, settings, overrides, data directories and drift.

use super::*;

/// **A `services` row belonging to an extension renders the manifest it was installed from.**
///
/// And it goes through the same walk everything else does, which is the point of giving an
/// extension a `services` row at all: the ports it holds, the limits its row carries and the
/// idle policy are applied by code that has no idea this one is an extension.
#[tokio::test]
async fn a_service_belonging_to_an_extension_renders_its_manifest() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let paths = Paths::new(directory.path().to_path_buf(), &PathOverrides::default());
    let store = Store::open(paths.database_file())
        .await
        .expect("a database");

    let manifest = crate::extensions::manifest::read(
        std::path::Path::new("extension.toml"),
        mixengine_testkit::extension::MAILPIT,
    )
    .expect("the fixture parses");

    // The ports the *install* handed out, one of them moved off the number the manifest asked
    // for — which is what a spec rendered from the manifest's wishes would get wrong.
    let mut ports = manifest.ports.clone();
    ports.insert("ui_port".to_owned(), 18_025);

    let installed = crate::extensions::store::Installed {
        id: manifest.extension.id.clone(),
        install_dir: paths.extensions().join("mailpit"),
        data_dir: paths.data().join("extensions").join("mailpit"),
        ports,
        manifest,
        source: crate::extensions::store::Source::Registry,
        signed: true,
        installed_at: mixengine_proto::Timestamp::parse_rfc3339("2026-09-02T09:00:00Z")
            .expect("a timestamp"),
    };
    crate::extensions::store::remember(&store, &installed)
        .await
        .expect("the row");

    let data_dir = installed.data_dir.display().to_string();
    sqlx::query(
        "INSERT INTO services (id, extension_id, instance_name, state, port, data_dir)
             VALUES ('mailpit', 'mailpit', 'mailpit', 'stopped', 18025, ?)",
    )
    .bind(&data_dir)
    .execute(store.pool())
    .await
    .expect("a services row");

    let generator = Generator::new(paths, store, Catalogue::builtin(), Vec::new(), true);

    let generated = generator
        .generate(&ServiceId::parse("mailpit").expect("an id"))
        .await
        .expect("the extension's service renders");

    assert!(
        generated
            .spec
            .args()
            .contains(&"127.0.0.1:18025".to_owned()),
        "the spec was rendered from the manifest's wish rather than the allocated port: {:?}",
        generated.spec.args()
    );
    assert!(
        generated.spec.program().ends_with("mailpit"),
        "{:?}",
        generated.spec.program()
    );
}

/// The spec `fakeservice@main` renders to with `idle_minutes` set to `minutes`.
///
/// Writes the column rather than taking a second fixture parameter: the join under test is
/// between a *row* and a recipe, and the three states of that column are the whole question.
async fn idle_of(
    generator: &Generator,
    minutes: Option<i64>,
) -> Option<mixengine_proto::IdlePolicy> {
    sqlx::query("UPDATE services SET idle_minutes = ? WHERE id = 'fakeservice@main'")
        .bind(minutes)
        .execute(generator.store.pool())
        .await
        .expect("the column is written");

    generator
        .generate(&ServiceId::parse("fakeservice@main").expect("an id"))
        .await
        .expect("a generated service")
        .spec
        .idle()
        .cloned()
}

/// The row decides how long, the recipe decides how, and `0` outranks both.
///
/// Three assertions because the column has three states, and the third is the one that exists for a
/// recipe's own default: a default arriving in a later release must reach the home that never
/// touched this setting and must not reach the one whose owner switched it off.
#[tokio::test]
async fn an_idle_policy_is_joined_from_the_row_and_the_recipe() {
    let (_home, generator) = home("{}").await;

    assert_eq!(
        idle_of(&generator, None).await,
        None,
        "a row that has not asked idles nothing, because no recipe here offers a default"
    );

    let policy = idle_of(&generator, Some(30))
        .await
        .expect("the row asked for a policy");

    assert_eq!(policy.after, mixengine_proto::Millis::from_secs(30 * 60));
    assert_eq!(
        policy.probe,
        mixengine_proto::IdleProbe::Connections { port: 4321 },
        "the probe is the recipe's, over the port the row allocated"
    );

    assert_eq!(
        idle_of(&generator, Some(0)).await,
        None,
        "zero minutes is never, not immediately"
    );
}

/// **ADR 0041.** A row nobody set is idle-stopped only while the home saves resources, and then on
/// the recipe's saving number; a row somebody set means what they said either way.
#[tokio::test]
async fn a_row_nobody_set_idles_only_while_the_home_saves_resources() {
    let (_home, generator) = home("{}").await;
    let minutes = |policy: Option<mixengine_proto::IdlePolicy>| policy.map(|p| p.after);

    assert_eq!(
        idle_of(&generator, None).await,
        None,
        "off: nothing is stopped"
    );

    crate::services::save_resources::set(&generator.store, true)
        .await
        .expect("the switch is written");

    assert_eq!(
        minutes(idle_of(&generator, None).await),
        Some(mixengine_proto::Millis::from_secs(45 * 60)),
        "on: the recipe's saving number"
    );
    assert_eq!(
        minutes(idle_of(&generator, Some(10)).await),
        Some(mixengine_proto::Millis::from_secs(10 * 60)),
        "a person's number outranks the switch"
    );
    assert_eq!(
        idle_of(&generator, Some(0)).await,
        None,
        "never stays never"
    );
}

#[tokio::test]
async fn a_row_becomes_a_spec_and_a_file_on_disk() {
    let (_home, generator) = home("{}").await;

    let generated = generator
        .generate(&ServiceId::parse("fakeservice@main").expect("an id"))
        .await
        .expect("a generated service");

    assert_eq!(generated.spec.id().as_str(), "fakeservice@main");
    assert!(generated.changed(), "nothing was written on a first render");

    // The recipe's answer reaches the spec. This fixture says `true` where the trait's default is
    // `false`, so an assertion here fails if the generator stops asking rather than passing for the
    // wrong reason.
    assert!(
        generated.spec.restart_over_memory(),
        "the recipe's permission is carried onto the spec it renders"
    );

    let rendered = std::fs::read_to_string(
        generator
            .paths
            .etc()
            .join("fakeservice@main")
            .join("fakeservice.conf"),
    )
    .expect("the rendered file");

    assert!(rendered.contains("say = hello"), "{rendered}");
    assert!(rendered.contains("port = 4321"), "{rendered}");
}

/// **A number read without writing anything.**
///
/// `mix cert status` connects to the front end's TLS port, and the two other ways to learn it
/// are both refused. [`Generator::generate`] goes through [`Generator::declared`], which
/// *installs*: a read-only status command would rewrite this home's whole configuration in
/// order to read one number, and can reload a running server as a side effect of being asked a
/// question. Parsing the rendered file back is what `CLAUDE.md` forbids outright.
#[tokio::test]
async fn a_services_settings_can_be_read_without_writing_anything() {
    let (_home, generator) = home(r##"{"greeting": "guten tag"}"##).await;
    let id = ServiceId::parse("fakeservice@main").expect("an id");

    let settings = generator.settings(&id).await.expect("its settings");

    assert_eq!(settings.text(GREETING), "guten tag");

    // The guarantee, and the whole reason this is a method rather than a field on `Generated`.
    assert!(
        !generator.paths.etc().join("fakeservice@main").exists(),
        "reading a setting installed a configuration"
    );
}

/// And a service nothing declares is `NotFound` rather than a default nobody set.
#[tokio::test]
async fn the_settings_of_a_service_that_does_not_exist_are_not_found() {
    let (_home, generator) = home("{}").await;
    let id = ServiceId::parse("fakeservice@other").expect("an id");

    assert!(matches!(
        generator.settings(&id).await,
        Err(Error::NotFound {
            kind: "service",
            ..
        })
    ));
}

#[tokio::test]
async fn an_override_reaches_the_rendered_file() {
    let (_home, generator) = home(r##"{"greeting": "guten tag", "extra": "# mine\n"}"##).await;

    generator.declared().await.expect("the declared set");

    let rendered = std::fs::read_to_string(
        generator
            .paths
            .etc()
            .join("fakeservice@main")
            .join("fakeservice.conf"),
    )
    .expect("the rendered file");

    assert!(rendered.contains("say = guten tag"), "{rendered}");
    assert!(rendered.contains("# mine"), "{rendered}");
}

/// The second walk over an unchanged home is what the registry does on every `service.*` call.
#[tokio::test]
async fn generating_twice_changes_nothing_the_second_time() {
    let (_home, generator) = home("{}").await;

    generator.declared().await.expect("a first walk");
    let again = generator
        .generate(&ServiceId::parse("fakeservice@main").expect("an id"))
        .await
        .expect("a second walk");

    assert!(!again.changed(), "an unchanged home rewrote its config");
}

/// The failure a home meets after an upgrade that dropped a recipe, and the one every home
/// meets today for a package this build does not know.
#[tokio::test]
async fn a_package_with_no_recipe_names_itself_and_what_is_known() {
    let (_home, generator) = home("{}").await;

    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES ('meilisearch', '1.0.0', '/packages/x', '2026-08-15T00:00:00Z',
                     'https://example', 'ab');
             INSERT INTO services (id, package_id, instance_name, state)
             VALUES ('meilisearch@main', (SELECT id FROM packages WHERE name = 'meilisearch'),
                     'main', 'stopped')",
    )
    .execute(generator.store.pool())
    .await
    .expect("a service belonging to something this build cannot run");

    let error = generator
        .declared()
        .await
        .expect_err("a package with no recipe");

    let message = error.to_string();
    assert!(message.contains("meilisearch"), "{message}");
}

/// A singleton's data directory is `data/<package>`, and not `data/<package>/<package>`.
///
/// The fallback was written for the case that has an instance name to spend. A recipe that
/// exists once has no such half, and repeating the package name reads as a mistake to whoever
/// meets it in a directory listing.
#[tokio::test]
async fn a_single_instance_recipe_keeps_its_data_directly_under_the_package() {
    let (_home, generator) = home_of(Arc::new(Solo), "solo", "solo", "{}").await;

    let generated = generator
        .generate(&ServiceId::parse("solo").expect("an id"))
        .await
        .expect("a generated service");

    assert_eq!(generated.spec.cwd(), generator.paths.data().join("solo"));
}

/// A named-instance recipe keeps the shape it always had: siblings under one package.
#[tokio::test]
async fn a_named_instance_recipe_keeps_its_data_under_the_instance() {
    let (_home, generator) = home("{}").await;

    let generated = generator
        .generate(&ServiceId::parse("fakeservice@main").expect("an id"))
        .await
        .expect("a generated service");

    assert_eq!(
        generated.spec.cwd(),
        generator.paths.data().join("fakeservice").join("main")
    );
}

/// A row that is fine except for its overrides fails as *that service*, not as a broken home.
#[tokio::test]
async fn a_misspelled_override_names_the_service_it_is_on() {
    let (_home, generator) = home(r#"{"greting": "hello"}"#).await;

    let error = generator
        .declared()
        .await
        .expect_err("a misspelled setting");
    let message = error.to_string();

    assert!(message.contains("fakeservice@main"), "{message}");
    assert!(message.contains("greting"), "{message}");
}
/// A service whose binary comes from an installed runtime renders exactly like one whose binary
/// comes from a package.
///
/// What is being asserted is the join and nothing else: the recipe is the same [`Fake`], the
/// context it receives carries the runtime's version and install path, and **the name the recipe
/// was found under is the id's own** — a pool is `php-fpm@8.3.33` and the row beneath it says
/// `php`, which is the one place those two differ.
#[tokio::test]
async fn a_runtime_backed_row_renders_from_the_runtime_it_names() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let paths = Paths::new(directory.path().to_path_buf(), &PathOverrides::default());
    let store = Store::open(paths.database_file())
        .await
        .expect("a database");

    sqlx::query(
        r#"INSERT INTO runtime_installs
                   (kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256, provides_json)
               VALUES ('php', '8.3.33', 'stable', '/runtimes/php/8.3.33', '2026-08-19T00:00:00Z',
                       1, 'https://example.invalid/php', 'abc', '{"php-fpm":"sbin/php-fpm"}')"#,
    )
    .execute(store.pool())
    .await
    .expect("a runtime install");

    sqlx::query(
        "INSERT INTO services (id, runtime_install_id, instance_name, state, port)
             VALUES ('fakeservice@8.3.33', (SELECT id FROM runtime_installs LIMIT 1),
                     '8.3.33', 'stopped', 9000)",
    )
    .execute(store.pool())
    .await
    .expect("a service over it");

    let generator = Generator::new(
        paths.clone(),
        store,
        Catalogue::default().with(Arc::new(Fake)),
        Vec::new(),
        true,
    );

    let generated = generator.declared().await.expect("one rendered service");

    assert_eq!(
        generated.len(),
        1,
        "a row with no packages parent was dropped by the join"
    );
    assert_eq!(generated[0].spec.id().as_str(), "fakeservice@8.3.33");

    let rendered = std::fs::read_to_string(
        paths
            .etc()
            .join("fakeservice@8.3.33")
            .join("fakeservice.conf"),
    )
    .expect("the rendered file");

    assert!(rendered.contains("port = 9000"), "{rendered}");
}
/// The question `mix doctor`'s tenth check asks: is what is on disk what these rows render to?
///
/// **The control is the first half.** A home that has never been rendered must drift, or an
/// empty answer after the render is evidence of a blind comparison rather than of a clean home.
#[tokio::test]
async fn drift_is_something_before_a_render_and_nothing_after_one() {
    let (_home, generator) = home("{}").await;

    let before = generator.drift().await.expect("a drift");
    assert!(
        before.iter().any(|one| !one.drift.is_empty()),
        "a home that was never rendered reported no drift: {before:?}"
    );

    generator.declared().await.expect("a rendering");

    let after = generator.drift().await.expect("a second drift");
    assert!(
        after.iter().all(|one| one.drift.is_empty()),
        "a home that was just rendered still drifts: {after:?}"
    );
}
