//! Routes, pools, the services a project needs and the links a site makes.

use mixengine_proto::RouteTarget;

use super::*;

/// **Routes round-trip, replace as a whole, and go when the site does** — roadmap task
/// **T135**.
///
/// One test for three claims because they are one claim about one table: what
/// [`write_routes`] writes is what [`records`] reads, a replacement removes what it does not
/// name (`Change::domains`' rule), and the cascade is the schema's rather than this module's.
#[tokio::test]
async fn routes_round_trip_and_replace_as_a_whole() {
    let (_temp, store, project) = home().await;

    let route = |path: &str, target: RouteTarget| SiteRoute {
        path: path.to_owned(),
        target,
    };

    let site = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: false,
            https_redirect: false,
            domains: vec!["routes.mixengine.test".to_owned()],
            services: Vec::new(),
            routes: vec![
                route(
                    "/api",
                    RouteTarget::Proxy {
                        upstream: "http://127.0.0.1:3003/xyz".to_owned(),
                    },
                ),
                route(
                    "/assets",
                    RouteTarget::Static {
                        root: "dist".to_owned(),
                    },
                ),
            ],
        },
    )
    .await
    .expect("a site with routes");

    let read = records(&store, None).await.expect("the sites");
    let stored = read.first().expect("one site");

    assert_eq!(stored.routes.len(), 2, "both routes came back");
    assert_eq!(
        stored.routes[0].target,
        RouteTarget::Proxy {
            upstream: "http://127.0.0.1:3003/xyz".to_owned()
        },
        "a path in an upstream survives the row it was written into"
    );
    assert_eq!(stored.routes[1].path, "/assets", "in declaration order");

    let changed = update(
        &store,
        site.id,
        &Change {
            routes: Some(vec![route("/api", RouteTarget::PhpFpm { pool: None })]),
            ..Change::default()
        },
    )
    .await
    .expect("an update");

    assert_eq!(
        changed.routes,
        [route("/api", RouteTarget::PhpFpm { pool: None })],
        "a replacement removes what it does not name"
    );

    delete(&store, site.id).await.expect("a delete");

    let left = sqlx::query_scalar!("SELECT COUNT(*) FROM site_routes")
        .fetch_one(store.pool())
        .await
        .expect("a count");

    assert_eq!(left, 0, "the cascade takes the routes with the site");
}

/// **Longest first, and the whole order is total** — roadmap task **T135**.
///
/// The one place an overlap is decided, which is why it is decided here rather than twice in
/// two templates whose native rules differ.
#[test]
fn routes_sort_longest_first() {
    let route = |path: &str| SiteRoute {
        path: path.to_owned(),
        target: RouteTarget::Static {
            root: "dist".to_owned(),
        },
    };

    let mut routes = [
        route("/api"),
        route("/api/v1/items"),
        route("/b"),
        route("/api/v1"),
    ];
    routes.sort_by(by_specificity);

    let order: Vec<&str> = routes.iter().map(|route| route.path.as_str()).collect();
    assert_eq!(order, ["/api/v1/items", "/api/v1", "/api", "/b"]);
}

/// **A site that is not an extension's may not run on that extension's pool** — roadmap task
/// **T82a**, that design's D5.
///
/// That process holds a database superuser's password, read from the keyring at spawn; the
/// whole point of giving a `web-app` a pool of its own is that no project's PHP is in it. Both
/// halves are asserted together, because a refusal that also refused the extension's own site
/// would have made the feature unbuildable rather than safe.
#[tokio::test]
async fn only_an_extensions_own_site_may_run_on_its_pool() {
    let (_temp, store, project) = home().await;
    an_extension_with_a_pool(&store, "phpmyadmin").await;
    let pool = ServiceId::parse("php-fpm@phpmyadmin").expect("an id");

    let refusal = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::PhpFpm {
                pool: Some(pool.clone()),
            },
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.mixengine.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect_err("a project site on an extension's pool");

    let said = refusal.to_string();
    assert!(said.contains("php-fpm@phpmyadmin"), "{said}");
    assert!(said.contains("phpmyadmin extension"), "{said}");

    create(
        &store,
        &NewSite {
            owner: SiteOwner::Extension(ExtensionId::parse("phpmyadmin").expect("an extension id")),
            doc_root: String::new(),
            kind: SiteKind::PhpFpm { pool: Some(pool) },
            https_enabled: true,
            https_redirect: false,
            domains: vec!["phpmyadmin.mixengine.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("its own site names its own pool");
}

/// **A pool nothing answers to is refused in words, not by the schema** — roadmap task **T122**.
///
/// `pool_is_free_for`'s own doc already asserts *a site must name a service that exists*, and
/// until this task nothing here enforced it: the `INSERT` reached SQLite, `php_service_id`'s
/// foreign key refused it, and what a person was handed was
/// `(code: 787) FOREIGN KEY constraint failed` at the end of an apply that had just downloaded
/// a runtime, a database and a web server. The name of the missing pool is the whole of what
/// they needed and the only thing that sentence did not carry.
#[tokio::test]
async fn a_site_may_not_name_a_pool_no_service_answers_to() {
    let (_temp, store, project) = home().await;
    let pool = ServiceId::parse("php-fpm@8.4.24").expect("an id");

    let refusal = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::PhpFpm {
                pool: Some(pool.clone()),
            },
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.mixengine.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect_err("a site on a pool that is not declared");

    let said = refusal.to_string();
    assert!(said.contains("php-fpm@8.4.24"), "{said}");
    assert!(
        !said.contains("FOREIGN KEY"),
        "the schema answered instead of this check: {said}"
    );
}

/// **Both halves, and nothing else's** — roadmap task **T125**. What a project needs is the
/// link table *and* the pool the kind names; a caller reading one of the two starts a database
/// with nothing to run the code, or the other way round. The second project is there because
/// the answer is a project's and not a home's — the set this replaces was every service the
/// home declared.
#[tokio::test]
async fn the_services_of_a_project_are_its_sites_links_and_their_pool() {
    let (_temp, store, project) = home().await;
    a_service_row(&store, "mariadb@main").await;
    a_service_row(&store, "redis@main").await;
    let pool = a_php_pool(&store, "8.3.34").await;

    create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::PhpFpm {
                pool: Some(pool.clone()),
            },
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.mixengine.test".to_owned()],
            services: vec![ServiceId::parse("mariadb@main").expect("an id")],
            routes: Vec::new(),
        },
    )
    .await
    .expect("a php site");

    let other = crate::projects::create(
        &store,
        &crate::projects::Registration {
            name: "shop".to_owned(),
            root: _temp.path().join("shop"),
            pins: std::collections::BTreeMap::new(),
        },
        Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
    )
    .await
    .expect("a second project");

    create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(other.id),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["shop.mixengine.test".to_owned()],
            services: vec![ServiceId::parse("redis@main").expect("an id")],
            routes: Vec::new(),
        },
    )
    .await
    .expect("a second project's site");

    let needed = services_of(&store, project)
        .await
        .expect("what the project needs");

    assert_eq!(
        needed,
        vec![ServiceId::parse("mariadb@main").expect("an id"), pool,],
        "the link and the pool, in id order, and nothing the other project declares"
    );
}

/// A project with no sites needs nothing, which is an answer and not an error.
#[tokio::test]
async fn a_project_with_no_sites_needs_no_services() {
    let (_temp, store, project) = home().await;

    assert!(
        services_of(&store, project)
            .await
            .expect("an answer")
            .is_empty()
    );
}

/// One `services` row and the package it belongs to, enough for `declared_services_exist` to
/// let a link through.
async fn a_service_row(store: &Store, id: &str) {
    let package = id.split('@').next().expect("a package name");

    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES (?, '1.0.0', '/packages/x', '2026-09-03T00:00:00Z', 'https://example', 'ab')",
    )
    .bind(package)
    .execute(store.pool())
    .await
    .expect("a package for the service to belong to");

    sqlx::query(
        "INSERT INTO services (id, package_id, instance_name, state)
             VALUES (?, (SELECT id FROM packages WHERE name = ?), 'main', 'stopped')",
    )
    .bind(id)
    .bind(package)
    .execute(store.pool())
    .await
    .expect("a service row");
}

/// A php-fpm pool and the runtime it belongs to, as an install would have left them.
async fn a_php_pool(store: &Store, version: &str) -> ServiceId {
    sqlx::query(
        "INSERT INTO runtime_installs (kind, version, channel, install_path, installed_at,
                                           size_bytes, source_url, sha256, provides_json)
             VALUES ('php', ?1, 'stable', '/runtimes/php', '2026-09-03T00:00:00Z', 1,
                     'https://example.invalid/php', 'ab', '{}')",
    )
    .bind(version)
    .execute(store.pool())
    .await
    .expect("a runtime row");

    sqlx::query(
        "INSERT INTO services (id, runtime_install_id, instance_name, state)
             VALUES ('php-fpm@' || ?1, (SELECT id FROM runtime_installs WHERE version = ?1), ?1,
                     'stopped')",
    )
    .bind(version)
    .execute(store.pool())
    .await
    .expect("a pool row");

    ServiceId::parse(format!("php-fpm@{version}")).expect("an id")
}

/// The same rule at the link table, which carries the other half of a site's services.
#[tokio::test]
async fn a_site_may_not_link_a_service_no_row_answers_to() {
    let (_temp, store, project) = home().await;

    let refusal = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.mixengine.test".to_owned()],
            services: vec![ServiceId::parse("mariadb@main").expect("an id")],
            routes: Vec::new(),
        },
    )
    .await
    .expect_err("a site linking a service that is not declared");

    let said = refusal.to_string();
    assert!(said.contains("mariadb@main"), "{said}");
    assert!(
        !said.contains("FOREIGN KEY"),
        "the schema answered instead of this check: {said}"
    );
}

/// And `site.update` is held to the same rule, because `blueprint.apply` reaches it without a
/// CLI — T81b's D6 arriving at a second field.
#[tokio::test]
async fn an_update_cannot_move_a_project_site_onto_an_extensions_pool() {
    let (_temp, store, project) = home().await;
    an_extension_with_a_pool(&store, "phpmyadmin").await;

    let site = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.mixengine.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a static site");

    let refusal = update(
        &store,
        site.id,
        &Change {
            kind: Some(SiteKind::PhpFpm {
                pool: Some(ServiceId::parse("php-fpm@phpmyadmin").expect("an id")),
            }),
            ..Change::default()
        },
    )
    .await
    .expect_err("moved onto an extension's pool");

    assert!(
        refusal.to_string().contains("php-fpm@phpmyadmin"),
        "{refusal}"
    );
}

/// An installed extension and the php-fpm pool it owns, as an install would have left them.
async fn an_extension_with_a_pool(store: &Store, id: &str) {
    sqlx::query(
        "INSERT INTO extensions
                 (id, name, version, kind, manifest_json, install_dir, data_dir, source, signed,
                  installed_at)
             VALUES (?1, ?1, '1.0.0', 'web-app', '{}', '/extensions/x', '/data/x', 'registry', 1,
                     '2026-09-03T00:00:00Z')",
    )
    .bind(id)
    .execute(store.pool())
    .await
    .expect("an extensions row");

    sqlx::query(
        "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url,
                  sha256, provides_json)
             VALUES ('php', '8.3.34', 'stable', '/runtimes/php', '2026-09-03T00:00:00Z', 1,
                     'https://example.invalid/php', 'ab', '{}')",
    )
    .execute(store.pool())
    .await
    .expect("a runtime row");

    sqlx::query(
        "INSERT INTO services (id, runtime_install_id, instance_name, state)
             VALUES ('php-fpm@' || ?1, (SELECT id FROM runtime_installs LIMIT 1), ?1, 'stopped')",
    )
    .bind(id)
    .execute(store.pool())
    .await
    .expect("the extension's pool row");
}

/// **D2, and the bug T39 paid for once.** A doc root is stored relative to the root, and both
/// sides of the comparison are normalised — a `/tmp` project and a `/private/tmp` doc root are
/// one directory on macOS, and a temporary directory is exactly where that shows up.
#[test]
fn a_doc_root_is_relativised_through_whatever_the_filesystem_calls_the_root() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let root = temp.path().join("blog");
    std::fs::create_dir_all(root.join("public")).expect("a doc root");

    assert_eq!(relative_doc_root(&root, "public").unwrap(), "public");
    assert_eq!(
        relative_doc_root(&root, &root.join("public").display().to_string()).unwrap(),
        "public"
    );
    assert_eq!(relative_doc_root(&root, "").unwrap(), "");
    assert_eq!(
        relative_doc_root(&root, &root.display().to_string()).unwrap(),
        "",
        "the root itself is the empty string, not \".\""
    );

    // Outside the root is a site whose files belong to somebody else.
    assert!(matches!(
        relative_doc_root(&root, "../elsewhere"),
        Err(Error::Project(ProjectError::DocRootOutsideProject { .. }))
    ));
}
