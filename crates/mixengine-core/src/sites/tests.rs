use super::*;
use crate::ProjectError;
use mixengine_proto::Timestamp;

async fn home() -> (tempfile::TempDir, Store, i64) {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let store = Store::open(&temp.path().join("mixengine.db"))
        .await
        .expect("a database");
    let project = crate::projects::create(
        &store,
        &crate::projects::Registration {
            name: "blog".to_owned(),
            root: temp.path().join("blog"),
            pins: std::collections::BTreeMap::new(),
        },
        Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
    )
    .await
    .expect("a project");

    (temp, store, project.id)
}

fn php() -> SiteKind {
    SiteKind::PhpFpm { pool: None }
}

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

/// A site is created unshared, shares whole, and unshares back to nothing — roadmap task
/// **T74**.
#[tokio::test]
async fn sharing_is_written_and_taken_away_as_one_value() {
    let (_temp, store, project) = home().await;

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");

    assert!(created.sharing.is_none(), "a site is created unshared");

    let sharing = Sharing {
        interface: "Wi-Fi".to_owned(),
        address: std::net::Ipv4Addr::new(192, 168, 1, 10),
        since: Timestamp(1_700_000_000_000),
        until: None,
    };

    let shared = set_sharing(&store, created.id, Some(&sharing))
        .await
        .expect("a share");

    assert_eq!(shared.sharing.as_ref(), Some(&sharing));

    // Read back through the ordinary listing rather than the writer's own answer: what a
    // consumer sees is what is on disk.
    let listed = records(&store, None).await.expect("the sites");
    assert_eq!(listed[0].sharing.as_ref(), Some(&sharing));

    let unshared = set_sharing(&store, created.id, None)
        .await
        .expect("an unshare");

    assert!(unshared.sharing.is_none());
}

/// A deadline rides with the rest of the sharing row and leaves with it — roadmap task **T76**.
///
/// **`until` is the one of the four that is optional on its own.** A share without an expiry is
/// the ordinary case, which is why it sits outside the trigger's all-or-nothing rule rather than
/// inside it — and why an *unshared* site carrying one is still refused.
#[tokio::test]
async fn a_share_carries_a_deadline_and_loses_it_with_the_rest() {
    let (_temp, store, project) = home().await;

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: false,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");

    let sharing = Sharing {
        interface: "Wi-Fi".to_owned(),
        address: std::net::Ipv4Addr::new(192, 168, 1, 10),
        since: Timestamp(1_700_000_000_000),
        until: Some(Timestamp(1_700_000_007_200_000)),
    };

    let shared = set_sharing(&store, created.id, Some(&sharing))
        .await
        .expect("a share");
    assert_eq!(shared.sharing.as_ref(), Some(&sharing));

    // Read back through the ordinary listing: what a consumer sees is what is on disk.
    let listed = records(&store, None).await.expect("the sites");
    assert_eq!(
        listed[0].sharing.as_ref().and_then(|one| one.until),
        Some(Timestamp(1_700_000_007_200_000))
    );

    // A share with no deadline is the ordinary one, and not a half-written row.
    let forever = Sharing {
        until: None,
        ..sharing.clone()
    };
    let shared = set_sharing(&store, created.id, Some(&forever))
        .await
        .expect("a share with no deadline");
    assert_eq!(shared.sharing.as_ref(), Some(&forever));

    let unshared = set_sharing(&store, created.id, None)
        .await
        .expect("an unshare");
    assert!(unshared.sharing.is_none());
}

/// The trigger in `0001_initial.sql`, from the side that would break it: two of the three
/// columns set is a state no reader could make sense of, so the database refuses it outright.
#[tokio::test]
async fn a_half_written_share_is_refused_by_the_database() {
    let (_temp, store, project) = home().await;

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");

    let refused = sqlx::query!(
        "UPDATE sites SET shared_interface = 'Wi-Fi' WHERE id = ?",
        created.id
    )
    .execute(store.pool())
    .await;

    assert!(refused.is_err(), "the database allowed half a share");
}

/// The URL is what somebody types into a phone, so 80 is left off and anything else is not.
#[test]
fn a_shared_url_carries_the_port_only_when_it_is_not_eighty() {
    let address = std::net::Ipv4Addr::new(192, 168, 1, 10);

    assert_eq!(shared_url(address, 80), "http://192.168.1.10");
    assert_eq!(shared_url(address, 8080), "http://192.168.1.10:8080");
}

/// A site, its ordered domains and its links are one write.    /// A site, its ordered domains and its links are one write.
#[tokio::test]
async fn a_site_is_created_whole_and_read_back_whole() {
    let (_temp, store, project) = home().await;

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: "public".to_owned(),
            kind: php(),
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.test".to_owned(), "www.blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");

    assert_eq!(created.domains, ["blog.test", "www.blog.test"]);
    assert_eq!(created.state, SiteState::Enabled);

    let found = by_domain(&store, "www.blog.test")
        .await
        .expect("a read")
        .expect("an alias reaches the site as surely as the primary does");
    assert_eq!(found.id, created.id);
    assert_eq!(found.domains[0], "blog.test", "the head is the primary");
}

/// **T98.** `https_redirect: true` is refused at `create` when `https_enabled` is `false`,
/// before a row is written — the schema's own `CHECK` would refuse it a step later, in words
/// about the column rather than the request.
#[tokio::test]
async fn https_redirect_needs_https_enabled_to_create_a_site() {
    let (_temp, store, project) = home().await;

    let refusal = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: false,
            https_redirect: true,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect_err("a redirect with nothing to redirect to");

    assert!(
        matches!(
            refusal,
            Error::Project(ProjectError::HttpsRedirectNeedsHttps)
        ),
        "{refusal}"
    );
    assert!(
        by_domain(&store, "blog.test")
            .await
            .expect("a read")
            .is_none(),
        "the refusal left no row behind"
    );
}

/// **T98, D2.** `https_redirect: Some(true)` is refused whether the `false` it collides with
/// comes from this same `update` turning HTTPS off, or from the site already being
/// plaintext-only — the two cases the design calls out by name.
#[tokio::test]
async fn https_redirect_needs_https_enabled_to_update_a_site_either_way_it_arrives() {
    let (_temp, store, project) = home().await;

    let plaintext = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: false,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a plaintext-only site");

    let already_off = update(
        &store,
        plaintext.id,
        &Change {
            https_redirect: Some(true),
            ..Change::default()
        },
    )
    .await
    .expect_err("the site was already plaintext-only");
    assert!(
        matches!(
            already_off,
            Error::Project(ProjectError::HttpsRedirectNeedsHttps)
        ),
        "{already_off}"
    );

    let https = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["shop.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("an https site");

    let turned_off_in_the_same_call = update(
        &store,
        https.id,
        &Change {
            https_enabled: Some(false),
            https_redirect: Some(true),
            ..Change::default()
        },
    )
    .await
    .expect_err("this update turns https off and asks for a redirect in the same breath");
    assert!(
        matches!(
            turned_off_in_the_same_call,
            Error::Project(ProjectError::HttpsRedirectNeedsHttps)
        ),
        "{turned_off_in_the_same_call}"
    );
}

/// **T98, D2.** Turning `https_enabled` off while saying nothing about the redirect carries the
/// redirect to `false` as well, because leaving it `true` on a plaintext-only site is a row the
/// schema's `CHECK` refuses to hold — and the caller asked for neither that refusal nor a
/// silent one either.
#[tokio::test]
async fn turning_https_off_silently_carries_an_untouched_redirect_to_false() {
    let (_temp, store, project) = home().await;

    let site = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: true,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("an https site with its redirect on");
    assert!(site.https_redirect, "the fixture's own precondition");

    let changed = update(
        &store,
        site.id,
        &Change {
            https_enabled: Some(false),
            ..Change::default()
        },
    )
    .await
    .expect("https turns off and the redirect is carried with it, not refused");

    assert!(!changed.https_enabled);
    assert!(
        !changed.https_redirect,
        "a stale redirect on a plaintext-only site is not \"leave it\""
    );
}

/// **T98.** Turning HTTPS on leaves an untouched redirect exactly where it was — the ordinary
/// "leave it" reading, asserted so the cascade above cannot be mistaken for a rule about every
/// `https_enabled` change rather than only the one that turns it off.
#[tokio::test]
async fn turning_https_on_leaves_an_untouched_redirect_alone() {
    let (_temp, store, project) = home().await;

    let site = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: false,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a plaintext-only site");

    let changed = update(
        &store,
        site.id,
        &Change {
            https_enabled: Some(true),
            ..Change::default()
        },
    )
    .await
    .expect("turning https on");

    assert!(changed.https_enabled);
    assert!(!changed.https_redirect, "nothing asked for a redirect");
}

/// **T98.** Both columns round-trip through `create`, `update` and `records` together, the way
/// `https_enabled` alone already does.
#[tokio::test]
async fn https_redirect_round_trips_through_create_update_and_records() {
    let (_temp, store, project) = home().await;

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: true,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("an https site with its redirect on");
    assert!(created.https_redirect);

    let found = by_domain(&store, "blog.test")
        .await
        .expect("a read")
        .expect("the site");
    assert!(found.https_redirect, "created is not the only writer");

    let changed = update(
        &store,
        created.id,
        &Change {
            https_redirect: Some(false),
            ..Change::default()
        },
    )
    .await
    .expect("turning the redirect off alone");
    assert!(changed.https_enabled, "untouched");
    assert!(!changed.https_redirect);
}

/// **D6.** Replacing the list is how a domain is removed, and the deletes run before the
/// inserts so a domain can move from one site to another in two calls.
#[tokio::test]
async fn a_domain_moves_between_sites_because_the_deletes_come_first() {
    let (_temp, store, project) = home().await;

    let blog = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: php(),
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.test".to_owned(), "api.blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("the first site");

    let shop = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: false,
            https_redirect: false,
            domains: vec!["shop.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("the second site");

    // Taken off the first...
    update(
        &store,
        blog.id,
        &Change {
            domains: Some(vec!["blog.test".to_owned()]),
            ..Change::default()
        },
    )
    .await
    .expect("the alias is dropped");

    // ...and claimed by the second, which the unique index would refuse the other way round.
    let moved = update(
        &store,
        shop.id,
        &Change {
            domains: Some(vec!["shop.test".to_owned(), "api.blog.test".to_owned()]),
            ..Change::default()
        },
    )
    .await
    .expect("the alias moves");

    assert_eq!(moved.domains, ["shop.test", "api.blog.test"]);
    assert_eq!(
        by_domain(&store, "api.blog.test")
            .await
            .unwrap()
            .unwrap()
            .id,
        shop.id
    );
}

/// A domain another site owns is refused by name, so the answer can say who has it.
#[tokio::test]
async fn a_domain_another_site_owns_names_that_site() {
    let (_temp, store, project) = home().await;

    create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: php(),
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("the first site");

    let refused = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect_err("one domain, one site");

    assert!(
        matches!(&refused, Error::Project(ProjectError::DomainTaken { holder, .. }) if holder == "blog.test"),
        "{refused:?}"
    );

    // And nothing was left behind by the write that failed halfway.
    assert_eq!(records(&store, Some(project)).await.unwrap().len(), 1);
}

/// The kind survives a round trip, payload and all — which is what `config_json` is for.
#[tokio::test]
async fn a_kinds_payload_survives_the_row_it_is_stored_in() {
    let (_temp, store, project) = home().await;

    for (kind, domain) in [
        (
            SiteKind::ReverseProxy {
                upstream: "http://127.0.0.1:5173".to_owned(),
            },
            "proxy.test",
        ),
        (SiteKind::NodeApp { port: 3000 }, "node.test"),
        (SiteKind::Static, "static.test"),
    ] {
        let created = create(
            &store,
            &NewSite {
                owner: SiteOwner::Project(project),
                doc_root: String::new(),
                kind: kind.clone(),
                https_enabled: true,
                https_redirect: false,
                domains: vec![domain.to_owned()],
                services: Vec::new(),
                routes: Vec::new(),
            },
        )
        .await
        .expect("a site");

        assert_eq!(by_domain(&store, domain).await.unwrap().unwrap().kind, kind);
        assert_eq!(created.kind, kind);
    }
}

/// The name a phone resolves — roadmap task **T75**.
///
/// **One label before `.local`.** Measured on Windows: `blog-mixengine.local` resolves and
/// `blog.mixengine.local` does not, same responder, same interface. The T75 design, D1.
#[test]
fn the_shared_name_is_one_label_under_local() {
    assert_eq!(
        shared_name("blog.test").as_deref(),
        Some("blog-mixengine.local")
    );
}

/// A hand-written domain need not already be a slug, so the label goes through the one
/// definition of a slug this crate has.
#[test]
fn the_label_is_slugged_rather_than_trusted() {
    assert_eq!(
        shared_name("My_Shop.test").as_deref(),
        Some("my-shop-mixengine.local")
    );
}

/// A domain with no dot is its own label rather than an error.
#[test]
fn a_single_label_domain_is_its_own_label() {
    assert_eq!(shared_name("blog").as_deref(), Some("blog-mixengine.local"));
}

/// **Never `-mixengine.local`.** `slug` answers [`None`] when nothing is left, and so does this.
#[test]
fn a_domain_with_no_usable_label_has_no_name() {
    assert_eq!(shared_name("---.test"), None);
}

/// A record as `name_taken` reads one: an id, a primary domain, and whether it is shared.
fn a_namesake(id: i64, primary: &str, shared: bool) -> SiteRecord {
    SiteRecord {
        id,
        owner: SiteOwner::Project(1),
        doc_root: String::new(),
        kind: SiteKind::Static,
        https_enabled: false,
        https_redirect: false,
        state: SiteState::Enabled,
        domains: vec![primary.to_owned()],
        services: Vec::new(),
        routes: Vec::new(),
        sharing: shared.then(|| Sharing {
            interface: "Wi-Fi".to_owned(),
            address: [192, 168, 1, 10].into(),
            since: Timestamp(1),
            until: None,
        }),
    }
}

/// Two shared sites whose first labels agree cannot both hold the name.
#[test]
fn a_second_shared_site_with_the_same_label_is_taken() {
    let records = vec![
        a_namesake(1, "blog.test", true),
        a_namesake(2, "blog.dev", false),
    ];

    assert_eq!(
        name_taken(&records, 2, "blog-mixengine.local"),
        Some("blog.test")
    );
}

/// **An unshared namesake is not a collision.** The name exists on the network only while a
/// site is shared, so a home full of `blog.*` sites is ordinary until two are shared at once.
#[test]
fn an_unshared_namesake_does_not_take_the_name() {
    let records = vec![
        a_namesake(1, "blog.test", false),
        a_namesake(2, "blog.dev", false),
    ];

    assert_eq!(name_taken(&records, 2, "blog-mixengine.local"), None);
}

/// **A site never collides with itself**, which is what makes re-sharing and `site.update`
/// idempotent rather than a refusal — the T75 design, D2.
#[test]
fn a_site_does_not_take_its_own_name() {
    let records = vec![a_namesake(1, "blog.test", true)];

    assert_eq!(name_taken(&records, 1, "blog-mixengine.local"), None);
}

/// **T81b, D1 and D3.** A site owned by an extension has no project, is reachable by its
/// extension, is invisible to a project's listing, and goes when its extension goes.
#[tokio::test]
async fn an_extension_owns_a_site_the_way_a_project_does() {
    let (_temp, store, project) = home().await;
    let id = ExtensionId::parse("phpmyadmin").expect("an id");

    sqlx::query(
        "INSERT INTO extensions (id, name, version, kind, manifest_json, install_dir, data_dir,
                                     source, signed, installed_at)
             VALUES ('phpmyadmin', 'phpMyAdmin', '5.2.1', 'web-app', '{}', '/ext/phpmyadmin',
                     '/data/extensions/phpmyadmin', 'path', 0, '2026-09-03T00:00:00Z')",
    )
    .execute(store.pool())
    .await
    .expect("an extension row");

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Extension(id.clone()),
            doc_root: "app".to_owned(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["phpmyadmin.mixengine.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("an extension-owned site");
    assert_eq!(created.owner, SiteOwner::Extension(id.clone()));

    let found = of_extension(&store, &id)
        .await
        .expect("a read")
        .expect("the site is found by its extension");
    assert_eq!(found.id, created.id);
    assert_eq!(found.domains, vec!["phpmyadmin.mixengine.test".to_owned()]);

    assert!(
        records(&store, Some(project))
            .await
            .expect("a read")
            .is_empty(),
        "a project's listing showed an extension's site"
    );
    assert_eq!(records(&store, None).await.expect("a read").len(), 1);

    sqlx::query("DELETE FROM extensions WHERE id = 'phpmyadmin'")
        .execute(store.pool())
        .await
        .expect("the delete");
    assert!(
        of_extension(&store, &id).await.expect("a read").is_none(),
        "the cascade did not take the site"
    );
}

/// **T81b, D9.** The extensions whose site is frozen on a pool, for `runtime.uninstall`'s
/// refusal — and a project's site on the same pool is not one of them.
#[tokio::test]
async fn frozen_on_names_the_extensions_a_pool_serves() {
    let (_temp, store, project) = home().await;
    let id = ExtensionId::parse("phpmyadmin").expect("an id");
    let pool = ServiceId::parse("php-fpm@8.3.34").expect("an id");

    for statement in [
        "INSERT INTO runtime_installs (id, kind, version, channel, install_path, installed_at,
                                           size_bytes, source_url, sha256, provides_json)
             VALUES (1, 'php', '8.3.34', 'stable', '/runtimes/php/8.3.34', '2026-09-03T00:00:00Z',
                     1, 'https://example.invalid/php', 'ab', '{\"php-fpm\":\"sbin/php-fpm\"}')",
        "INSERT INTO services (id, runtime_install_id, instance_name, state, port)
             VALUES ('php-fpm@8.3.34', 1, '8.3.34', 'stopped', 9000)",
        "INSERT INTO extensions (id, name, version, kind, manifest_json, install_dir, data_dir,
                                     source, signed, installed_at)
             VALUES ('phpmyadmin', 'phpMyAdmin', '5.2.1', 'web-app', '{}', '/ext/phpmyadmin',
                     '/data/extensions/phpmyadmin', 'path', 0, '2026-09-03T00:00:00Z')",
    ] {
        sqlx::query(statement)
            .execute(store.pool())
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }

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
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a project site");

    assert!(frozen_on(&store, &pool).await.expect("a read").is_empty());

    create(
        &store,
        &NewSite {
            owner: SiteOwner::Extension(id.clone()),
            doc_root: "app".to_owned(),
            kind: SiteKind::PhpFpm {
                pool: Some(pool.clone()),
            },
            https_enabled: true,
            https_redirect: false,
            domains: vec!["phpmyadmin.mixengine.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("an extension site");

    assert_eq!(frozen_on(&store, &pool).await.expect("a read"), vec![id]);
}
