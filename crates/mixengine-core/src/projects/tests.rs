use super::*;

const NOW: Timestamp = Timestamp(1_760_000_000_000);

async fn store() -> (tempfile::TempDir, Store) {
    let home = tempfile::tempdir().expect("a temporary directory");
    let store = Store::open(&home.path().join(crate::paths::DATABASE_FILE_NAME))
        .await
        .expect("a database");
    (home, store)
}

/// A keep-warm project reaches the pool its sites name, and nothing else.
///
/// **The second half is the assertion worth having.** Today the only edge from a project to a
/// service is `sites.php_service_id`, so a project's *database* is not reached — and the day
/// T77 gives a project a way to declare what it needs, this test is what says the reach
/// changed rather than a user discovering it.
#[tokio::test]
async fn a_keep_warm_project_reaches_the_pool_its_sites_name() {
    let (_home, store) = store().await;
    let (_tree, root) = tree(&["shop"]);

    let project = create(&store, &registration("shop", &root), NOW)
        .await
        .expect("a project");

    service_row(&store, "php-fpm@8.3").await;
    service_row(&store, "mariadb@main").await;
    site_row(&store, project.id, "php-fpm@8.3").await;

    assert!(
        kept_warm(&store).await.expect("the warm set").is_empty(),
        "a project nobody asked to keep warm keeps nothing warm"
    );

    update(
        &store,
        project.id,
        &Change {
            keep_warm: Some(true),
            ..Change::default()
        },
    )
    .await
    .expect("the project is updated");

    let warm = kept_warm(&store).await.expect("the warm set");

    assert_eq!(
        warm.get(&ServiceId::parse("php-fpm@8.3").expect("an id")),
        Some(&"shop".to_owned()),
        "the pool this project's site names is kept warm, and by a project with a name: {warm:?}"
    );
    assert!(
        !warm.contains_key(&ServiceId::parse("mariadb@main").expect("an id")),
        "no schema relates a project to its database yet — that edge is T77's"
    );
}

/// A `packages` row and a `services` row for it, which is all a site needs to point at one.
async fn service_row(store: &Store, id: &str) {
    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES (?, '1.0.0', '/packages/x', '2026-08-26T00:00:00Z', 'https://example', 'ab')",
    )
    .bind(id)
    .execute(store.pool())
    .await
    .expect("a package");

    sqlx::query(
        "INSERT INTO services (id, package_id, instance_name, state)
             VALUES (?, (SELECT id FROM packages WHERE name = ?), 'main', 'stopped')",
    )
    .bind(id)
    .bind(id)
    .execute(store.pool())
    .await
    .expect("a service");
}

/// A php-fpm site belonging to `project`, pointing at `pool`.
async fn site_row(store: &Store, project: i64, pool: &str) {
    sqlx::query(
        "INSERT INTO sites (project_id, doc_root, kind, php_service_id, state)
             VALUES (?, '/srv/shop/public', 'php-fpm', ?, 'enabled')",
    )
    .bind(project)
    .bind(pool)
    .execute(store.pool())
    .await
    .expect("a site");
}

/// A real directory to register, because `create` normalises what it is given.
fn tree(depth: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("a temporary directory");
    let mut path = root.path().to_path_buf();
    for name in depth {
        path = path.join(name);
    }
    std::fs::create_dir_all(&path).expect("a directory");
    (root, path)
}

fn pins(entries: &[(RuntimeKind, &str)]) -> BTreeMap<RuntimeKind, VersionConstraint> {
    entries
        .iter()
        .map(|(kind, text)| {
            (
                *kind,
                VersionConstraint::parse((*text).to_owned()).expect("a constraint"),
            )
        })
        .collect()
}

fn registration(name: &str, root: &Path) -> Registration {
    Registration {
        name: name.to_owned(),
        root: root.to_path_buf(),
        pins: BTreeMap::new(),
    }
}

/// Write the rows an install would have written, without the eighty megabytes.
async fn install(store: &Store, kind: RuntimeKind, versions: &[&str]) {
    for version in versions {
        crate::runtimes::remember(
            store,
            &crate::runtimes::Installation {
                kind,
                version: PackageVersion::parse(*version).expect("a version"),
                channel: mixengine_proto::PackageChannel::Stable,
                path: PathBuf::from("/home/runtimes")
                    .join(kind.as_str())
                    .join(version),
                bytes: 41_000_000,
                url: format!("https://example.invalid/{kind}-{version}.tar.zst"),
                sha256: "00".to_owned(),
                provides: [(kind.as_str().to_owned(), format!("bin/{kind}"))]
                    .into_iter()
                    .collect(),
                extension_dir: None,
                extensions: crate::index::Extensions::default(),
            },
            NOW,
        )
        .await
        .expect("a row");
    }
}

/// **D1 and D6 together.** The manifest outranks the row, so a contradicting row pin is inert —
/// and saying which one is in charge is the whole reason this answer carries a source.
#[tokio::test]
async fn a_manifest_pin_outranks_the_rows_and_says_so() {
    let (_home, store) = store().await;
    install(&store, RuntimeKind::Php, &["8.2.20", "8.3.33"]).await;
    let (_root, blog) = tree(&["blog"]);
    std::fs::write(crate::manifest::at(&blog), "[runtimes]\nphp = \"8.3\"\n").expect("a manifest");

    let mut asked = registration("blog", &blog);
    asked.pins = pins(&[(RuntimeKind::Php, "8.2"), (RuntimeKind::Node, "22")]);
    let project = create(&store, &asked, NOW).await.expect("a project");

    let effective = effective_pins(&store, &project).await.expect("the pins");

    let php = effective
        .iter()
        .find(|pin| pin.kind == RuntimeKind::Php)
        .expect("php is pinned");
    assert_eq!(php.constraint.as_str(), "8.3", "the file wins");
    assert!(matches!(php.source, PinSource::Manifest { .. }), "{php:?}");
    assert_eq!(
        php.resolved.as_ref().map(PackageVersion::as_str),
        Some("8.3.33")
    );
    assert_eq!(php.hint, None, "a pin that resolves needs no advice");

    let node = effective
        .iter()
        .find(|pin| pin.kind == RuntimeKind::Node)
        .expect("node is pinned by the row");
    assert_eq!(node.source, PinSource::Registered);
    assert_eq!(node.resolved, None, "no node is installed");
    assert!(
        node.hint
            .as_deref()
            .is_some_and(|hint| hint.contains("runtime install node")),
        "a pin nothing satisfies carries the command that would fix it: {node:?}"
    );
}

/// **The test the refusal is worth having.** Same pin, same command, two outcomes — and only a
/// re-resolution against what would be left produces both.
#[tokio::test]
async fn a_removal_breaks_a_pin_only_when_it_takes_the_last_version_that_answers_it() {
    let (_home, store) = store().await;
    install(&store, RuntimeKind::Php, &["8.3.33", "8.3.34"]).await;
    let (_root, blog) = tree(&["blog"]);

    let mut asked = registration("blog", &blog);
    asked.pins = pins(&[(RuntimeKind::Php, "^8.3")]);
    create(&store, &asked, NOW).await.expect("a project");

    let first = pins_broken_by(
        &store,
        RuntimeKind::Php,
        &PackageVersion::parse("8.3.33").expect("a version"),
    )
    .await
    .expect("a reading");
    assert!(
        first.is_empty(),
        "^8.3 still has 8.3.34 to answer it: {first:?}"
    );

    crate::runtimes::forget(
        &store,
        RuntimeKind::Php,
        &PackageVersion::parse("8.3.33").expect("a version"),
    )
    .await
    .expect("the row");

    let second = pins_broken_by(
        &store,
        RuntimeKind::Php,
        &PackageVersion::parse("8.3.34").expect("a version"),
    )
    .await
    .expect("a reading");
    assert_eq!(second.len(), 1, "{second:?}");
    assert_eq!(second[0].project, "blog");
    assert_eq!(second[0].constraint.as_str(), "^8.3");
}

/// A pin that is already unsatisfiable must refuse nothing — otherwise one stale pin would make
/// unremovable a runtime it never mentions.
#[tokio::test]
async fn a_pin_nothing_already_satisfies_breaks_over_nothing() {
    let (_home, store) = store().await;
    install(&store, RuntimeKind::Php, &["8.3.33"]).await;
    let (_root, blog) = tree(&["blog"]);

    let mut asked = registration("blog", &blog);
    asked.pins = pins(&[(RuntimeKind::Php, "8.1")]);
    create(&store, &asked, NOW).await.expect("a project");

    let broken = pins_broken_by(
        &store,
        RuntimeKind::Php,
        &PackageVersion::parse("8.3.33").expect("a version"),
    )
    .await
    .expect("a reading");

    assert!(broken.is_empty(), "{broken:?}");
}

/// **D7 reads the pin in effective order**, so a refusal is never based on a row the file
/// overrides.
#[tokio::test]
async fn a_row_pin_the_manifest_overrides_refuses_nothing() {
    let (_home, store) = store().await;
    install(&store, RuntimeKind::Php, &["8.2.20", "8.3.33"]).await;
    let (_root, blog) = tree(&["blog"]);
    std::fs::write(crate::manifest::at(&blog), "[runtimes]\nphp = \"8.2\"\n").expect("a manifest");

    let mut asked = registration("blog", &blog);
    asked.pins = pins(&[(RuntimeKind::Php, "8.3.33")]);
    create(&store, &asked, NOW).await.expect("a project");

    let broken = pins_broken_by(
        &store,
        RuntimeKind::Php,
        &PackageVersion::parse("8.3.33").expect("a version"),
    )
    .await
    .expect("a reading");

    assert!(
        broken.is_empty(),
        "the file pins 8.2, so removing 8.3.33 breaks nothing that would ever take effect: \
             {broken:?}"
    );
}

/// What was written is what comes back.
#[tokio::test]
async fn a_registered_project_is_listed_with_the_pins_it_was_given() {
    let (_home, store) = store().await;
    let (_root, blog) = tree(&["blog"]);

    let mut asked = registration("blog", &blog);
    asked.pins = pins(&[(RuntimeKind::Php, "^8.3")]);

    let written = create(&store, &asked, NOW).await.expect("a project");

    assert_eq!(written.name, "blog");
    assert_eq!(written.pins, asked.pins);
    assert_eq!(written.created_at, NOW.to_rfc3339());
    assert_eq!(records(&store).await.expect("a listing"), vec![written]);
}

/// **D3.** The walk `resolve::in_a_project` used to hold: the nearest registered root at or
/// above the directory, and never a further one when a nearer one exists.
#[tokio::test]
async fn a_directory_finds_the_nearest_project_above_it() {
    let (_home, store) = store().await;
    let (root, deep) = tree(&["blog", "packages", "theme", "src"]);
    let inner = root.path().join("blog").join("packages").join("theme");

    create(&store, &registration("outer", root.path()), NOW)
        .await
        .expect("the outer project");
    create(&store, &registration("theme", &inner), NOW)
        .await
        .expect("the inner project");

    let found = find(&store, &ProjectRef::Path(deep.display().to_string()))
        .await
        .expect("a lookup")
        .expect("something above it");

    assert_eq!(found.name, "theme", "the nearer root wins");

    // And a root inside another project's root is allowed: nesting has a defined answer.
    let outer = find(
        &store,
        &ProjectRef::Path(root.path().join("blog").display().to_string()),
    )
    .await
    .expect("a lookup")
    .expect("the outer project");
    assert_eq!(outer.name, "outer");
}

/// **D5, the half that would otherwise be silently broken on Windows.** A row normalised on the
/// way in and a directory that was not are two strings for one directory — so the query side is
/// normalised too, and the test asks it the way a caller does.
#[tokio::test]
async fn a_directory_is_found_however_this_filesystem_spells_it() {
    let (_home, store) = store().await;
    let (root, blog) = tree(&["blog"]);

    create(&store, &registration("blog", &blog), NOW)
        .await
        .expect("a project");

    // The temporary root as the OS handed it over — which on Windows may be an 8.3 alias — and
    // the same directory spelled in full. Both are the project.
    for spelling in [root.path().join("blog"), in_full(&root.path().join("blog"))] {
        assert!(
            find(&store, &ProjectRef::Path(spelling.display().to_string()))
                .await
                .expect("a lookup")
                .is_some(),
            "{} did not find the project",
            spelling.display()
        );
    }
}

/// One directory is one project, and the refusal names the project already holding it.
#[tokio::test]
async fn the_same_directory_twice_is_refused_by_the_name_that_has_it() {
    let (_home, store) = store().await;
    let (_root, blog) = tree(&["blog"]);

    create(&store, &registration("blog", &blog), NOW)
        .await
        .expect("the first");
    let error = create(&store, &registration("other", &blog), NOW)
        .await
        .expect_err("the second");

    assert!(
        matches!(&error, Error::Project(ProjectError::ProjectRootTaken { holder, .. }) if holder == "blog"),
        "{error:?}"
    );
}

/// The other unique column, whose repair is different: a name is not freed by moving anything.
#[tokio::test]
async fn the_same_name_twice_is_refused() {
    let (_home, store) = store().await;
    let (_first, one) = tree(&["blog"]);
    let (_second, two) = tree(&["shop"]);

    create(&store, &registration("blog", &one), NOW)
        .await
        .expect("the first");
    let error = create(&store, &registration("blog", &two), NOW)
        .await
        .expect_err("the second");

    assert!(
        matches!(error, Error::Project(ProjectError::ProjectNameTaken { .. })),
        "{error:?}"
    );
}

/// **D4.** A handle typed on a command line, and T39a will make a domain out of it.
#[test]
fn a_name_is_a_handle_rather_than_free_text() {
    assert_eq!(validated_name("  blog  ").expect("trimmed"), "blog");

    for refused in [
        "",
        "   ",
        "blog/site",
        "blog\\site",
        "a\u{7}b",
        &"x".repeat(65),
    ] {
        assert!(
            validated_name(refused).is_err(),
            "{refused:?} should not be a project name"
        );
    }
}

/// **D6.** An absent map leaves the pins alone; an empty one clears them.
#[tokio::test]
async fn updating_pins_replaces_them_and_an_empty_map_clears_them() {
    let (_home, store) = store().await;
    let (_root, blog) = tree(&["blog"]);
    let mut asked = registration("blog", &blog);
    asked.pins = pins(&[(RuntimeKind::Php, "8.3"), (RuntimeKind::Node, "22")]);
    let written = create(&store, &asked, NOW).await.expect("a project");

    let renamed = update(
        &store,
        written.id,
        &Change {
            name: Some("weblog".to_owned()),
            root: None,
            pins: None,
            keep_warm: None,
        },
    )
    .await
    .expect("a rename");
    assert_eq!(renamed.name, "weblog");
    assert_eq!(renamed.pins, asked.pins, "an absent map changed nothing");

    let replaced = update(
        &store,
        written.id,
        &Change {
            name: None,
            root: None,
            pins: Some(pins(&[(RuntimeKind::Php, "^8.4")])),
            keep_warm: None,
        },
    )
    .await
    .expect("a replacement");
    assert_eq!(
        replaced.pins,
        pins(&[(RuntimeKind::Php, "^8.4")]),
        "replacing is not merging: node is gone"
    );

    let cleared = update(
        &store,
        written.id,
        &Change {
            name: None,
            root: None,
            pins: Some(BTreeMap::new()),
            keep_warm: None,
        },
    )
    .await
    .expect("a clearing");
    assert!(cleared.pins.is_empty());
}

/// A row written by a build that manages a language this one does not must not stop this one
/// reading the project — `resolve`'s own rule, kept where the reading moved to. `kotlin`, since Go
/// (T27d) and Java (T27e) became kinds of this build's own.
#[tokio::test]
async fn a_pin_naming_a_language_this_build_does_not_manage_is_ignored_rather_than_fatal() {
    let (_home, store) = store().await;
    let (_root, blog) = tree(&["blog"]);
    let root = in_full(&blog).display().to_string();

    sqlx::query(
        "INSERT INTO projects (name, root_path, runtime_pins_json, created_at)
             VALUES ('blog', ?, '{\"php\": \"8.3\", \"kotlin\": \"2.2\"}', '2026-08-22T06:55:12Z')",
    )
    .bind(&root)
    .execute(store.pool())
    .await
    .expect("a project");

    let found = find(&store, &ProjectRef::Name("blog".to_owned()))
        .await
        .expect("a lookup")
        .expect("the project");

    assert_eq!(found.pins, pins(&[(RuntimeKind::Php, "8.3")]));
}

/// A reference matching nothing is nothing, not an error — the daemon is what turns it into
/// `not_found`, because only it knows which of the two words a person typed.
#[tokio::test]
async fn a_reference_that_matches_nothing_answers_nothing() {
    let (_home, store) = store().await;

    assert!(
        find(&store, &ProjectRef::Name("blog".to_owned()))
            .await
            .expect("a lookup")
            .is_none()
    );
}
