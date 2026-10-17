use super::*;

/// **A devkit whose folder is gone is no devkit**, and one that is there beside it still
/// answers — roadmap task **T206d**, D2.
#[tokio::test]
async fn a_devkit_whose_folder_is_gone_is_skipped() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let store = Store::open(&home.path().join(mixengine_core::paths::DATABASE_FILE_NAME))
        .await
        .expect("a database");
    let record = |version: &str| mixengine_core::packages::Installation {
        package: "msys2".to_owned(),
        version: mixengine_proto::PackageVersion::parse(version).expect("a version"),
        path: home.path().join("packages").join("msys2").join(version),
        bytes: 1,
        url: "https://example.invalid/msys2".to_owned(),
        sha256: "00".to_owned(),
        provides: std::collections::BTreeMap::new(),
    };
    let at = mixengine_proto::Timestamp(1_760_000_000_000);

    let older = record("2026.9.30");
    std::fs::create_dir_all(&older.path).expect("a folder");
    mixengine_core::packages::remember(&store, &older, at)
        .await
        .expect("recorded");
    // The newer one is recorded and its folder is not there.
    mixengine_core::packages::remember(&store, &record("2026.10.08"), at)
        .await
        .expect("recorded");

    assert_eq!(
        installed_devkit(&store, RuntimeKind::Ruby).await,
        Some(older.path.clone())
    );

    std::fs::remove_dir_all(&older.path).expect("deleted by hand");
    assert_eq!(installed_devkit(&store, RuntimeKind::Ruby).await, None);
}

/// **A Ruby is pointed at the installed devkit** — roadmap task **T206a**, D6.
#[test]
fn a_ruby_is_pointed_at_the_installed_devkit() {
    let mut environment = BTreeMap::new();
    devkit(
        RuntimeKind::Ruby,
        Some(Path::new("/home/packages/msys2/2026.10.08")),
        None,
        &mut environment,
    );
    assert_eq!(
        environment.get(MSYS2_PATH),
        Some(&OsString::from("/home/packages/msys2/2026.10.08"))
    );
}

#[test]
fn no_devkit_no_variable() {
    let mut environment = BTreeMap::new();
    devkit(RuntimeKind::Ruby, None, None, &mut environment);
    assert!(!environment.contains_key(MSYS2_PATH));
}

/// **The session's own value stands**, on `toolchain`'s rule.
#[test]
fn a_session_msys2_path_stands() {
    let mut environment = BTreeMap::new();
    devkit(
        RuntimeKind::Ruby,
        Some(Path::new("/home/packages/msys2/2026.10.08")),
        Some(OsStr::new("C:/msys64")),
        &mut environment,
    );
    assert!(
        !environment.contains_key(MSYS2_PATH),
        "the session's own value is left to stand"
    );
}

#[test]
fn other_languages_are_told_nothing() {
    let mut environment = BTreeMap::new();
    devkit(
        RuntimeKind::Php,
        Some(Path::new("/home/packages/msys2/x")),
        None,
        &mut environment,
    );
    assert!(environment.is_empty());
}

/// **Release order, not text order**: `2026.10.08` is newer than `2026.9.30`.
#[test]
fn the_newest_devkit_is_chosen() {
    let chosen = newest_devkit(&[
        ("2026.9.30", "/p/msys2/2026.9.30"),
        ("2026.10.08", "/p/msys2/2026.10.08"),
    ]);
    assert_eq!(chosen.as_deref(), Some(Path::new("/p/msys2/2026.10.08")));
}

/// T25 left this note: "No `PHPRC`, no `GEM_HOME` — the rest are files T28's `conf.d` model
/// generates, and a variable pointing at a file nothing writes is worse than no variable."
/// Something writes them now.
#[test]
fn a_php_shim_is_told_where_its_ini_set_is() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let root = home.path();
    let version = PackageVersion::parse("8.3.33").expect("a version");

    let command = shims::COMMANDS
        .iter()
        .find(|command| command.name == "php")
        .expect("this build fronts php");

    let conf_d = mixengine_core::runtimes::extensions::conf_d(
        &root.join("etc"),
        command.kind,
        version.as_str(),
    );
    std::fs::create_dir_all(&conf_d).expect("a generated set");

    let environment = surroundings(
        command.kind,
        &root.join("runtimes/php/8.3.33/bin/php"),
        root,
        &version,
        None,
        None,
    );

    assert!(
        environment.contains_key("PATH"),
        "the runtime's own tools still reach each other"
    );
    let scan = environment
        .get(mixengine_core::runtimes::extensions::SCAN_DIR_ENV)
        .expect("a php that is told where its extensions are");
    assert!(
        scan.to_string_lossy().contains("8.3.33"),
        "the shim is pointing at another version's set: {scan:?}"
    );
}

/// A runtime with no generated set gets no variable, rather than one pointing at nothing.
#[test]
fn a_runtime_with_no_generated_set_is_told_nothing() {
    let home = tempfile::tempdir().expect("a temporary directory");
    let version = PackageVersion::parse("20.11.0").expect("a version");

    let command = shims::COMMANDS
        .iter()
        .find(|command| command.name == "node")
        .expect("this build fronts node");

    let environment = surroundings(
        command.kind,
        &home.path().join("runtimes/node/20.11.0/bin/node"),
        home.path(),
        &version,
        None,
        None,
    );

    assert!(!environment.contains_key(JAVA_HOME));

    assert!(!environment.contains_key(mixengine_core::runtimes::extensions::SCAN_DIR_ENV));
}

/// **The pin means the release that was resolved** — roadmap task **T27d**, its design's D5.
/// `go.env` inside the archive says `auto`, which would let a `go.mod` swap the toolchain.
#[test]
fn a_go_is_kept_to_the_toolchain_it_resolved_to() {
    let mut environment = BTreeMap::new();

    toolchain(RuntimeKind::Go, None, &mut environment);

    assert_eq!(environment.get(GOTOOLCHAIN), Some(&OsString::from("local")));
}

/// Go reads an empty variable as unset and falls through to `go.env`, so this does too.
#[test]
fn an_empty_session_value_is_unset_to_go_and_so_to_this() {
    let mut environment = BTreeMap::new();

    toolchain(RuntimeKind::Go, Some(OsStr::new("")), &mut environment);

    assert_eq!(environment.get(GOTOOLCHAIN), Some(&OsString::from("local")));
}

/// **A value the person set is theirs** — ADR 0034's rule, for one more variable.
#[test]
fn a_session_value_is_left_to_the_session() {
    let mut environment = BTreeMap::new();

    toolchain(
        RuntimeKind::Go,
        Some(OsStr::new("go1.27.1+auto")),
        &mut environment,
    );

    assert_eq!(
        environment.get(GOTOOLCHAIN),
        None,
        "inherited, not overwritten"
    );
}

#[test]
fn no_other_kind_is_told_about_a_go_toolchain() {
    for kind in RuntimeKind::ALL
        .into_iter()
        .filter(|kind| *kind != RuntimeKind::Go)
    {
        let mut environment = BTreeMap::new();

        toolchain(kind, None, &mut environment);

        assert!(environment.is_empty(), "{kind}");
    }
}

/// **A Java command names its own JDK**, over whatever the session said — T27e, D5.
#[test]
fn a_java_command_is_handed_its_own_java_home() {
    let mut environment = BTreeMap::new();
    let root = Path::new("runtimes").join("java").join("21.0.12.1");

    java_home(Some(&root.join("bin").join("java")), &mut environment);

    assert_eq!(environment.get(JAVA_HOME), Some(&root.into_os_string()));
}

#[test]
fn a_command_with_no_jdk_is_told_nothing_about_one() {
    let mut environment = BTreeMap::new();

    java_home(None, &mut environment);

    assert!(environment.is_empty());
}
