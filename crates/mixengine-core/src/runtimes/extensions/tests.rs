use super::*;

fn state(choices: &str) -> State {
    State {
        kind: RuntimeKind::Php,
        version: PackageVersion::parse("8.3.33").expect("a version"),
        install_path: PathBuf::from("/runtimes/php/8.3.33"),
        directory: Some("lib/php/extensions".to_owned()),
        offered: crate::index::Extensions {
            compiled_in: vec!["opcache".to_owned(), "core".to_owned()],
            shared: vec![
                "igbinary".to_owned(),
                "redis".to_owned(),
                "mongodb".to_owned(),
                "xdebug".to_owned(),
            ],
            enabled: vec![
                "igbinary".to_owned(),
                "redis".to_owned(),
                "mongodb".to_owned(),
            ],
        },
        choices: serde_json::from_str(choices).expect("choices"),
        additions: Vec::new(),
        ca_bundle: None,
    }
}

fn rendered(state: &State) -> BTreeMap<String, String> {
    state
        .documents()
        .into_iter()
        .map(|document| {
            (
                document.relative().display().to_string(),
                document.contents().to_owned(),
            )
        })
        .collect()
}

/// **An installed extension's `[recipe] php_ini` reaches every managed PHP.**
///
/// Rendered against *that extension's* install directory, which is the whole reason the
/// substitution happens here rather than in whatever writes the file.
#[tokio::test]
async fn an_extension_recipe_lands_in_a_php_conf_d() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let paths = crate::Paths::new(
        home.path().to_path_buf(),
        &crate::config::PathOverrides::default(),
    );
    let store = Store::open(paths.database_file())
        .await
        .expect("a database");

    sqlx::query(
        "INSERT INTO runtime_installs
               (kind, version, channel, install_path, installed_at, size_bytes, source_url,
                sha256, is_default, extension_dir, extensions_json, extension_choices_json)
             VALUES ('php', '8.3.33', 'stable', '/runtimes/php/8.3.33', '2026-09-02T09:00:00Z',
                     1, 'https://example.invalid/php.zip', 'ab', 1, 'lib/php/extensions', '{}',
                     '{}')",
    )
    .execute(store.pool())
    .await
    .expect("an installed PHP");

    let manifest = crate::extensions::manifest::read(
        Path::new("extension.toml"),
        mixengine_testkit::extension::SENDMAIL,
    )
    .expect("the fixture parses");

    crate::extensions::store::remember(
        &store,
        &crate::extensions::store::Installed {
            id: manifest.extension.id.clone(),
            install_dir: paths.extensions().join("sendmail-to-mailpit"),
            data_dir: paths.data().join("extensions").join("sendmail-to-mailpit"),
            ports: BTreeMap::new(),
            manifest,
            source: crate::extensions::store::Source::Registry,
            signed: true,
            installed_at: mixengine_proto::Timestamp::parse_rfc3339("2026-09-02T09:00:00Z")
                .expect("a timestamp"),
        },
    )
    .await
    .expect("the row");

    // `super::`, because this module's test helper of the same name shadows it.
    let state = super::state(
        &store,
        RuntimeKind::Php,
        &PackageVersion::parse("8.3.33").expect("a version"),
    )
    .await
    .expect("the state");

    let files = rendered(&state);
    let ini = files
        .get("60-sendmail-to-mailpit.ini")
        .expect("the extension's ini");

    assert!(ini.contains("sendmail_path = "), "{ini}");
    assert!(
        ini.contains(
            &paths
                .extensions()
                .join("sendmail-to-mailpit")
                .display()
                .to_string()
        ),
        "the placeholder was not rendered against the extension's own directory: {ini}"
    );
}

/// A runtime that is not PHP is asked for none of them: there is no ini set to add a line to.
#[tokio::test]
async fn a_node_install_carries_no_php_recipe() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let paths = crate::Paths::new(
        home.path().to_path_buf(),
        &crate::config::PathOverrides::default(),
    );
    let store = Store::open(paths.database_file())
        .await
        .expect("a database");

    sqlx::query(
        "INSERT INTO runtime_installs
               (kind, version, channel, install_path, installed_at, size_bytes, source_url,
                sha256, is_default, extension_dir, extensions_json, extension_choices_json)
             VALUES ('node', '22.0.0', 'stable', '/runtimes/node/22.0.0', '2026-09-02T09:00:00Z',
                     1, 'https://example.invalid/node.zip', 'ab', 1, '', '{}', '{}')",
    )
    .execute(store.pool())
    .await
    .expect("an installed Node");

    let state = super::state(
        &store,
        RuntimeKind::Node,
        &PackageVersion::parse("22.0.0").expect("a version"),
    )
    .await
    .expect("the state");

    assert!(state.additions.is_empty());
    assert!(state.documents().is_empty());
}

/// What the build enables is what is loaded when nobody has said otherwise.
#[test]
fn a_build_with_no_choices_loads_what_it_enables() {
    assert_eq!(state("{}").loaded(), ["igbinary", "mongodb", "redis"]);
}

/// Deviations in both directions, which is the whole point of storing them rather than a set.
#[test]
fn a_choice_turns_one_name_round_and_leaves_the_rest() {
    let loaded = state(r#"{"xdebug": true, "mongodb": false}"#).loaded();

    assert_eq!(loaded, ["igbinary", "redis", "xdebug"]);
}

/// A choice about a name this build does not ship loadable cannot smuggle it in.
#[test]
fn a_choice_is_intersected_with_what_the_build_ships() {
    assert_eq!(
        state(r#"{"imagick": true}"#).loaded(),
        ["igbinary", "mongodb", "redis"]
    );
}

/// A listing says *why* something is on, because the question is asked when the answer is
/// surprising.
#[test]
fn a_listing_says_whether_the_build_or_the_user_decided() {
    let listed = state(r#"{"xdebug": true}"#).listing();
    let of = |name: &str| {
        listed
            .iter()
            .find(|extension| extension.name == name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} is not in the listing"))
    };

    assert_eq!(of("opcache").linkage, Linkage::Static);
    assert!(
        of("opcache").enabled,
        "compiled in and therefore always loaded"
    );
    assert_eq!(of("opcache").source, Source::BuildDefault);

    assert_eq!(of("redis").source, Source::BuildDefault);
    assert!(of("redis").enabled);

    assert_eq!(of("xdebug").source, Source::User);
    assert!(of("xdebug").enabled);
}

/// Compiled into this build, and a different build is what it would take.
#[test]
fn a_compiled_in_extension_cannot_be_turned_off() {
    let error = state("{}")
        .decide("opcache", false)
        .expect_err("a static extension is not disableable");

    assert!(
        matches!(
            error,
            Error::Runtime(RuntimeError::ExtensionCompiledIn { .. })
        ),
        "{error:?}"
    );
    assert!(error.to_string().contains("opcache"));
}

/// A name this build has never heard of is refused rather than written down.
#[test]
fn an_unknown_name_is_refused() {
    let error = state("{}")
        .decide("swoole", true)
        .expect_err("a name no cell carries");

    assert!(
        matches!(
            error,
            Error::NotFound {
                kind: "extension",
                ..
            }
        ),
        "{error:?}"
    );
}

/// Choosing what the build already does is not stored: a deviation that deviates from nothing
/// would survive the upgrade that changes the default, which is what this model exists to avoid.
#[test]
fn a_choice_that_agrees_with_the_build_is_forgotten() {
    let choices = state(r#"{"xdebug": true}"#)
        .decide("xdebug", false)
        .expect("turning it back off is allowed");

    assert!(choices.is_empty(), "{choices:?}");
}

/// `extension_dir` is absolute, and written even where PHP would find its own — upstream PHP for
/// Windows bakes an absolute `C:\php\ext` into the binary that would otherwise be consulted by
/// accident on a machine where that path happens to exist.
#[test]
fn the_extension_directory_is_written_as_an_absolute_path() {
    let files = rendered(&state("{}"));
    let mixengine = &files["00-mixengine.ini"];
    let expected = PathBuf::from("/runtimes/php/8.3.33").join("lib/php/extensions");

    assert!(
        mixengine.contains(&format!("extension_dir = \"{}\"", expected.display())),
        "{mixengine}"
    );
}

/// The dev-tuned block is MixEngine's opinion, and it is written whole.
#[test]
fn a_development_machine_gets_the_settings_it_wants() {
    let files = rendered(&state("{}"));
    let mixengine = &files["00-mixengine.ini"];

    for line in [
        "memory_limit = 512M",
        "upload_max_filesize = 128M",
        "post_max_size = 128M",
        "max_execution_time = 120",
        "display_errors = On",
        "error_reporting = E_ALL",
        "date.timezone = UTC",
        "opcache.enable = 1",
        "opcache.revalidate_freq = 0",
    ] {
        assert!(
            mixengine.contains(line),
            "{line} is missing
{mixengine}"
        );
    }
}

/// `conf.d` is scanned in name order, and order is load order.
#[test]
fn the_file_names_carry_the_load_order() {
    let files = rendered(&state(r#"{"xdebug": true}"#));
    let names: Vec<&str> = files.keys().map(String::as_str).collect();

    assert_eq!(
        names,
        [
            "00-mixengine.ini",
            "20-igbinary.ini",
            "50-mongodb.ini",
            "50-redis.ini",
            "90-xdebug.ini"
        ],
        "igbinary loads before the redis that links against it, and xdebug wraps everything"
    );
}

/// Two names are engine extensions and the rest are not. A `zend_extension` PHP cannot load is a
/// startup warning rather than a refusal to start, which is why this is asserted here and again
/// against a real PHP in `crates/mixengine-core/tests/php_modules.rs`.
///
/// **The filenames are written out rather than built with [`module_file`]**, which is what this
/// test did until the spelling turned out to be wrong: an assertion that calls the function it
/// is judging agrees with whatever that function does, so it stayed green through a name no PHP
/// could open. What is pinned here is the string, and it changes only when somebody means it to.
#[test]
fn the_two_zend_extensions_are_spelled_as_such() {
    let files = rendered(&state(r#"{"xdebug": true}"#));

    let (xdebug, redis) = if cfg!(windows) {
        ("php_xdebug.dll", "php_redis.dll")
    } else {
        ("xdebug.so", "redis.so")
    };

    assert!(files["90-xdebug.ini"].contains(&format!("zend_extension = {xdebug}")));
    assert!(files["50-redis.ini"].contains(&format!("extension = {redis}")));
    assert!(
        !files["50-redis.ini"].contains("zend_extension"),
        "an ordinary extension loaded as an engine one is a PHP that will not start"
    );
}

/// The same state on two systems, decided by the index rather than by a `cfg`: opcache is
/// compiled in on the Unix cells and is a DLL on Windows, so one gets a file and the other does
/// not — while both are told `opcache.enable = 1`, because a static opcache is present and idle
/// until an ini says otherwise.
#[test]
fn opcache_renders_from_what_the_index_says_about_this_artifact() {
    let unix = rendered(&state("{}"));
    assert!(!unix.contains_key("40-opcache.ini"), "compiled in here");
    assert!(unix["00-mixengine.ini"].contains("opcache.enable = 1"));

    let mut windows = state("{}");
    windows.offered.compiled_in = vec!["core".to_owned()];
    windows.offered.shared.push("opcache".to_owned());
    windows.offered.enabled.push("opcache".to_owned());

    let windows = rendered(&windows);
    assert!(
        windows["40-opcache.ini"].contains(&format!("zend_extension = {}", module_file("opcache")))
    );
    assert!(windows["00-mixengine.ini"].contains("opcache.enable = 1"));
}

/// A runtime that can load nothing renders nothing — which is what keeps Node, Python and Ruby
/// out of this without anything asking what kind it is.
#[test]
fn a_runtime_that_declares_no_extension_directory_renders_nothing() {
    let mut node = state("{}");
    node.directory = None;
    node.offered = crate::index::Extensions::default();

    assert!(node.documents().is_empty());
}

/// **The sweep.** `document::install` prunes nothing, so an extension turned off would leave its
/// file behind and go on being loaded.
#[tokio::test]
async fn a_file_left_by_an_earlier_state_is_removed() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let paths = crate::Paths::new(
        home.path().to_path_buf(),
        &crate::config::PathOverrides::default(),
    );

    let on = state(r#"{"xdebug": true}"#);
    assert!(render(&paths, &on).await.expect("a rendering"));

    let directory = conf_d(paths.etc(), RuntimeKind::Php, on.version.as_str());
    assert!(directory.join("90-xdebug.ini").is_file());

    let off = state("{}");
    assert!(
        render(&paths, &off).await.expect("a rendering"),
        "removing a file is a change, and the pool has to hear about it"
    );
    assert!(
        !directory.join("90-xdebug.ini").exists(),
        "xdebug was turned off and its file went on loading it"
    );
    assert!(
        directory.join("50-redis.ini").is_file(),
        "the rest of the set is untouched"
    );

    assert!(
        !render(&paths, &off).await.expect("a rendering"),
        "a second render of the same state changes nothing, or every daemon start reloads pools"
    );
}

/// A store with one PHP in it, recorded the way an install records one.
async fn installed() -> (tempfile::TempDir, Store, crate::Paths) {
    let home = tempfile::tempdir().expect("a temporary directory");
    let paths = crate::Paths::new(
        home.path().to_path_buf(),
        &crate::config::PathOverrides::default(),
    );
    let store = Store::open(paths.database_file())
        .await
        .expect("a database");

    crate::runtimes::remember(
        &store,
        &crate::runtimes::Installation {
            kind: RuntimeKind::Php,
            version: PackageVersion::parse("8.3.33").expect("a version"),
            channel: mixengine_proto::PackageChannel::Stable,
            path: PathBuf::from("/runtimes/php/8.3.33"),
            bytes: 1,
            url: "https://example.invalid/php.tar.zst".to_owned(),
            sha256: "00".to_owned(),
            provides: BTreeMap::new(),
            extension_dir: Some("lib/php/extensions".to_owned()),
            extensions: crate::index::Extensions {
                compiled_in: vec!["opcache".to_owned()],
                shared: vec!["redis".to_owned(), "xdebug".to_owned()],
                enabled: vec!["redis".to_owned()],
            },
        },
        mixengine_proto::Timestamp(1_760_000_000_000),
    )
    .await
    .expect("a row");

    (home, store, paths)
}

/// A choice is stored as a deviation and read back as one, and both refusals survive the round
/// trip through the database.
#[tokio::test]
async fn a_choice_is_written_down_and_the_refusals_survive_the_round_trip() {
    let (_home, store, _paths) = installed().await;
    let version = PackageVersion::parse("8.3.33").expect("a version");

    let after = choose(&store, RuntimeKind::Php, &version, "xdebug", true)
        .await
        .expect("xdebug is shared here");
    assert_eq!(after.loaded(), ["redis", "xdebug"]);

    // `super::` because the helper above shadows the module function inside this block.
    let reread = super::state(&store, RuntimeKind::Php, &version)
        .await
        .expect("the row");
    assert_eq!(
        reread.choices,
        BTreeMap::from([("xdebug".to_owned(), true)])
    );

    let refused = choose(&store, RuntimeKind::Php, &version, "opcache", false)
        .await
        .expect_err("compiled in here");
    assert!(
        matches!(
            refused,
            Error::Runtime(RuntimeError::ExtensionCompiledIn { .. })
        ),
        "{refused:?}"
    );

    let unknown = choose(&store, RuntimeKind::Php, &version, "swoole", true)
        .await
        .expect_err("no cell carries it");
    assert!(
        matches!(
            unknown,
            Error::NotFound {
                kind: "extension",
                ..
            }
        ),
        "{unknown:?}"
    );
}

/// Boot renders every installed runtime, and an uninstall takes the whole version directory.
#[tokio::test]
async fn every_installed_runtime_is_rendered_at_boot_and_removed_with_its_directory() {
    let (_home, store, paths) = installed().await;
    let version = PackageVersion::parse("8.3.33").expect("a version");

    let moved = refresh_all(&store, &paths).await.expect("a walk");
    assert_eq!(moved.len(), 1, "{moved:?}");

    let directory = conf_d(paths.etc(), RuntimeKind::Php, version.as_str());
    assert!(directory.join("00-mixengine.ini").is_file());

    discard(&paths, RuntimeKind::Php, &version)
        .await
        .expect("removed");
    assert!(!directory.exists());
    assert!(
        !directory.parent().expect("the version directory").exists(),
        "the version directory goes with it, or `etc/php/` fills with empty shells"
    );

    // Removing what is not there is not a failure: an uninstall of a runtime that never had a
    // set must not fail on its way out.
    discard(&paths, RuntimeKind::Php, &version)
        .await
        .expect("idempotent");
}
