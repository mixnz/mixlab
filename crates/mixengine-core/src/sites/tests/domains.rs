//! Domains, payloads, shared names and the sites an extension owns.

use super::*;

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
