use super::*;
use crate::Paths;
use crate::config::PathOverrides;

/// An absolute path on whichever system this is compiled for.
fn root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\src\blog")
    } else {
        PathBuf::from("/src/blog")
    }
}

/// A home with a database and one project in it.
async fn home() -> (tempfile::TempDir, Store) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let paths = Paths::new(directory.path().to_path_buf(), &PathOverrides::default());
    let store = Store::open(paths.database_file())
        .await
        .expect("a database");

    sqlx::query(
        "INSERT INTO projects (id, name, root_path, created_at)
             VALUES (1, 'blog', ?, '2026-08-23T00:00:00Z')",
    )
    .bind(root().to_string_lossy().into_owned())
    .execute(store.pool())
    .await
    .expect("a project row");

    (directory, store)
}

/// One site and its primary domain.
async fn site(store: &Store, id: i64, domain: &str, doc_root: &str, kind: &str, state: &str) {
    sqlx::query("INSERT INTO sites (id, project_id, doc_root, kind, state) VALUES (?, 1, ?, ?, ?)")
        .bind(id)
        .bind(doc_root)
        .bind(kind)
        .bind(state)
        .execute(store.pool())
        .await
        .expect("a site row");

    sqlx::query("INSERT INTO site_domains (site_id, domain, is_primary) VALUES (?, ?, 1)")
        .bind(id)
        .bind(domain)
        .execute(store.pool())
        .await
        .expect("a domain row");
}

/// **One rule** — the T135 design, D2: the matched prefix is replaced by the upstream's path,
/// trailing slash removed; an upstream with no path replaces nothing.
///
/// **Two regex shapes**, because the strip-to-root case cannot share the other's: `$1` for the
/// exact prefix is empty there, and a request may not leave with no path at all. Both measured
/// on Caddy 2.11.4 before either was written.
#[test]
fn an_upstream_path_becomes_the_prefix_the_request_leaves_with() {
    assert_eq!(
        split_upstream("/abc", "http://127.0.0.1:3003"),
        ("http://127.0.0.1:3003".to_owned(), None),
        "an upstream with no path replaces nothing"
    );

    assert_eq!(
        split_upstream("/abc", "http://127.0.0.1:3003/xyz"),
        (
            "http://127.0.0.1:3003".to_owned(),
            Some(Rewrite {
                regex: "^/abc(/.*)?$".to_owned(),
                replacement: "/xyz$1".to_owned(),
            })
        )
    );

    assert_eq!(
        split_upstream("/abc", "http://127.0.0.1:3003/xyz/"),
        (
            "http://127.0.0.1:3003".to_owned(),
            Some(Rewrite {
                regex: "^/abc(/.*)?$".to_owned(),
                replacement: "/xyz$1".to_owned(),
            })
        ),
        "a trailing slash on the upstream's path is not a third behaviour"
    );

    assert_eq!(
        split_upstream("/abc", "http://127.0.0.1:3003/"),
        (
            "http://127.0.0.1:3003".to_owned(),
            Some(Rewrite {
                regex: "^/abc/?(.*)$".to_owned(),
                replacement: "/$1".to_owned(),
            })
        ),
        "stripping to the root is the shape that cannot leave an empty path"
    );

    // **A `.` is a regex metacharacter and a path segment may hold one** — the design's D5.
    assert_eq!(
        split_upstream("/v1.0", "http://h:1/x")
            .1
            .expect("a rewrite")
            .regex,
        r"^/v1\.0(/.*)?$"
    );

    // **A whole site is the empty prefix, not `/`** — every path is under it, so what the
    // upstream's path replaces is the string before the first slash.
    assert_eq!(
        split_upstream("", "http://127.0.0.1:3003/xyz"),
        (
            "http://127.0.0.1:3003".to_owned(),
            Some(Rewrite {
                regex: "^(/.*)?$".to_owned(),
                replacement: "/xyz$1".to_owned(),
            })
        )
    );
}

/// The doc root a template gets is absolute and joined onto the project's root, whatever the row
/// stores — the row is relative and forward-slashed on every system, and a Caddyfile needs a
/// path this machine can open.
/// **T204a, D5.** Each site's welcome page lists its own steps, not its neighbour's.
#[tokio::test]
async fn each_site_lists_only_its_own_steps() {
    let (home, store) = home().await;
    site(&store, 1, "blog.test", "", "static", "enabled").await;
    site(&store, 2, "vite.blog.test", "", "static", "enabled").await;

    let manifest = "schema = 3\n\n[blueprint]\nname = \"two\"\ncreated_at = \"x\"\n\n\
            [blueprint.created_on]\nos = \"any\"\nversion = \"0\"\n\n\
            [[sites]]\nkind = \"static\"\ndomain_pattern = \"{project}.test\"\n\n\
            [[sites]]\nkind = \"static\"\ndomain_pattern = \"vite.{project}.test\"\n\n\
            [[next_steps]]\nkind = \"once\"\nrun = \"composer install\"\nsite = \"{project}.test\"\n\n\
            [[next_steps]]\nkind = \"serve\"\nrun = \"npm run dev\"\nsite = \"vite.{project}.test\"\n";
    sqlx::query(
        "INSERT INTO blueprints (id, name, manifest_toml, created_at, source, trusted)
             VALUES ('two', 'two', ?, '2026-10-09T00:00:00Z', 'captured', 1)",
    )
    .bind(manifest)
    .execute(store.pool())
    .await
    .expect("a blueprint row");
    sqlx::query("UPDATE projects SET blueprint_id = 'two' WHERE id = 1")
        .execute(store.pool())
        .await
        .expect("the project points at it");

    let served = served(
        &store,
        &BTreeMap::new(),
        &home.path().join("certs"),
        &BTreeMap::new(),
    )
    .await
    .expect("the sites read");

    let runs = |domain: &str| -> Vec<String> {
        served
            .iter()
            .find(|one| one.primary() == domain)
            .expect("the site")
            .steps
            .iter()
            .map(|step| step.run.clone())
            .collect()
    };
    assert_eq!(runs("blog.test"), vec!["composer install".to_owned()]);
    assert_eq!(runs("vite.blog.test"), vec!["npm run dev".to_owned()]);
}

#[tokio::test]
async fn a_doc_root_comes_back_absolute() {
    let (home, store) = home().await;
    site(&store, 1, "blog.test", "web/public", "static", "enabled").await;

    let served = served(
        &store,
        &BTreeMap::new(),
        &home.path().join("certs"),
        &BTreeMap::new(),
    )
    .await
    .expect("the sites read");

    assert_eq!(served.len(), 1);
    assert_eq!(served[0].doc_root, root().join("web").join("public"));
    assert_eq!(served[0].primary(), "blog.test");
}

/// A doc root of `""` is the project's root itself, and joining it must not leave a trailing
/// separator in a web server's configuration.
#[tokio::test]
async fn an_empty_doc_root_is_the_project_root_itself() {
    let (home, store) = home().await;
    site(&store, 1, "blog.test", "", "static", "enabled").await;

    let served = served(
        &store,
        &BTreeMap::new(),
        &home.path().join("certs"),
        &BTreeMap::new(),
    )
    .await
    .expect("the sites");

    assert_eq!(served[0].doc_root, root());
}

/// "Declared and deliberately not rendered" is the whole of what `Disabled` means: the site is
/// simply not in the set, so no document is rendered and the sweep removes the file it had.
#[tokio::test]
async fn a_disabled_site_is_not_in_the_set() {
    let (home, store) = home().await;
    site(&store, 1, "on.test", "", "static", "enabled").await;
    site(&store, 2, "off.test", "", "static", "disabled").await;

    let served = served(
        &store,
        &BTreeMap::new(),
        &home.path().join("certs"),
        &BTreeMap::new(),
    )
    .await
    .expect("the sites");

    assert_eq!(served.len(), 1);
    assert_eq!(served[0].primary(), "on.test");
}

/// A php-fpm site carries the address its pool listens on, resolved through the map the
/// generator built from every recipe's own answer.
#[tokio::test]
async fn a_php_site_carries_the_address_its_pool_listens_on() {
    let (home, store) = home().await;

    sqlx::query(
        r#"INSERT INTO runtime_installs
                   (id, kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256, provides_json)
               VALUES (1, 'php', '8.3.33', 'stable', '/runtimes/php/8.3.33',
                       '2026-08-23T00:00:00Z', 1, 'https://example.invalid/php', 'ab',
                       '{"php-fpm":"sbin/php-fpm"}')"#,
    )
    .execute(store.pool())
    .await
    .expect("a runtime install");

    sqlx::query(
        "INSERT INTO services (id, runtime_install_id, instance_name, state, port)
             VALUES ('php-fpm@8.3.33', 1, '8.3.33', 'stopped', 9000)",
    )
    .execute(store.pool())
    .await
    .expect("a pool row");

    sqlx::query(
        "INSERT INTO sites (id, project_id, doc_root, kind, php_service_id, state)
             VALUES (1, 1, 'public', 'php-fpm', 'php-fpm@8.3.33', 'enabled')",
    )
    .execute(store.pool())
    .await
    .expect("a php site");

    sqlx::query("INSERT INTO site_domains (site_id, domain, is_primary) VALUES (1, 'p.test', 1)")
        .execute(store.pool())
        .await
        .expect("a domain");

    let pool = ServiceId::parse("php-fpm@8.3.33").expect("an id");
    let address = "127.0.0.1:9000".parse().expect("an address");
    let waking = "127.0.0.1:9500".parse().expect("an address");
    let upstreams = BTreeMap::from([(
        pool,
        Upstreams {
            listen: Upstream::Tcp(address),
            activator: Some(Upstream::Tcp(waking)),
        },
    )]);

    let served = served(
        &store,
        &upstreams,
        &home.path().join("certs"),
        &BTreeMap::new(),
    )
    .await
    .expect("the sites");

    assert_eq!(
        served[0].kind,
        ServedKind::PhpFpm {
            upstream: Upstream::Tcp(address),
            activator: Some(Upstream::Tcp(waking)),
        }
    );
}

/// D5's honest half: `service.delete --force` is allowed to cross a site's declaration, so a
/// pool that is gone is a state this build can reach. The site is **left out** rather than
/// rendered with an empty address and rather than failing the render — a render that failed
/// would leave a daemon that cannot render anything at all, which is far worse than the one site
/// it was about. `mix doctor` (T47) is what reports it to a person.
#[tokio::test]
async fn a_site_whose_pool_is_gone_is_left_out_rather_than_failing_the_render() {
    let (home, store) = home().await;

    sqlx::query(
        "INSERT INTO sites (id, project_id, doc_root, kind, php_service_id, state)
             VALUES (1, 1, 'public', 'php-fpm', NULL, 'enabled')",
    )
    .execute(store.pool())
    .await
    .expect("a php site with no pool");

    sqlx::query("INSERT INTO site_domains (site_id, domain, is_primary) VALUES (1, 'p.test', 1)")
        .execute(store.pool())
        .await
        .expect("a domain");

    site(&store, 2, "ok.test", "", "static", "enabled").await;

    let served = served(
        &store,
        &BTreeMap::new(),
        &home.path().join("certs"),
        &BTreeMap::new(),
    )
    .await
    .expect("a missing pool does not fail the render");

    assert_eq!(served.len(), 1);
    assert_eq!(served[0].primary(), "ok.test");
}

/// A site with a certificate on disk carries the two paths a template has to write, and a
/// fingerprint — roadmap task **T51**.
#[tokio::test]
async fn a_site_with_a_certificate_carries_its_paths_and_fingerprint() {
    let (home, store) = home().await;
    site(&store, 1, "blog.test", "", "static", "enabled").await;

    let certs = home.path().join("certs");
    std::fs::create_dir_all(&certs).expect("the certs directory");
    crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");
    crate::certs::leaf::ensure(
        &certs,
        &["blog.test".to_owned()],
        None,
        std::time::SystemTime::now(),
    )
    .expect("a leaf");

    let served = served(&store, &BTreeMap::new(), &certs, &BTreeMap::new())
        .await
        .expect("the sites are read");

    let certificate = served[0]
        .certificate
        .as_ref()
        .unwrap_or_else(|| panic!("no certificate on a site that has one: {served:?}"));

    assert_eq!(
        certificate.certificate,
        crate::certs::leaf::certificate_path(&certs, "blog.test")
    );
    assert_eq!(
        certificate.key,
        crate::certs::leaf::key_path(&certs, "blog.test")
    );
    assert_eq!(certificate.fingerprint.len(), 64);
}

/// **`https_redirect` mirrors the record, plainly** — roadmap task **T98**. No transformation
/// happens between the column and this field, unlike `certificate`, which is why this is its own
/// test rather than folded into the certificate ones above: a regression here would not be one a
/// missing or present `certificate` would catch.
#[tokio::test]
async fn https_redirect_mirrors_the_record() {
    let (home, store) = home().await;
    site(&store, 1, "blog.test", "", "static", "enabled").await;
    sqlx::query("UPDATE sites SET https_redirect = 1 WHERE id = 1")
        .execute(store.pool())
        .await
        .expect("the row is updated");

    let served = served(
        &store,
        &BTreeMap::new(),
        &home.path().join("certs"),
        &BTreeMap::new(),
    )
    .await
    .expect("the sites are read");

    assert!(served[0].https_redirect, "{served:?}");
}

/// **And carries no certificate along with it when there is none to carry** — T98 read beside
/// T51's D4. A site can declare the redirect and still have nothing on disk to serve HTTPS
/// with; the two recipes gate the redirect on both being true together for exactly this reason.
#[tokio::test]
async fn a_redirecting_site_with_no_certificate_still_carries_none() {
    let (home, store) = home().await;
    site(&store, 1, "blog.test", "", "static", "enabled").await;
    sqlx::query("UPDATE sites SET https_redirect = 1 WHERE id = 1")
        .execute(store.pool())
        .await
        .expect("the row is updated");

    let served = served(
        &store,
        &BTreeMap::new(),
        &home.path().join("certs"),
        &BTreeMap::new(),
    )
    .await
    .expect("the sites are read");

    assert!(served[0].https_redirect, "{served:?}");
    assert!(served[0].certificate.is_none(), "{served:?}");
}

/// **A site with no certificate is `None` and not an error** — the T51 design, D4. This is the
/// state a home with no authority is in, and rendering has to keep working there.
#[tokio::test]
async fn a_site_with_no_certificate_carries_none() {
    let (home, store) = home().await;
    site(&store, 1, "blog.test", "", "static", "enabled").await;

    let served = served(
        &store,
        &BTreeMap::new(),
        &home.path().join("certs"),
        &BTreeMap::new(),
    )
    .await
    .expect("the sites are read");

    assert!(served[0].certificate.is_none(), "{served:?}");
}

/// **Half a pair is `None` too.** `leaf::read` answers `Unusable` for a certificate with no key,
/// and a template that wrote `tls` against it would fail `caddy validate` — which is the whole
/// reason this reads through `leaf::read` rather than asking whether a file exists.
#[tokio::test]
async fn a_site_with_half_a_pair_carries_none() {
    let (home, store) = home().await;
    site(&store, 1, "blog.test", "", "static", "enabled").await;

    let certs = home.path().join("certs");
    std::fs::create_dir_all(certs.join("sites")).expect("the sites directory");
    std::fs::write(
        crate::certs::leaf::certificate_path(&certs, "blog.test"),
        "not a certificate",
    )
    .expect("written");

    let served = served(&store, &BTreeMap::new(), &certs, &BTreeMap::new())
        .await
        .expect("the sites are read");

    assert!(served[0].certificate.is_none(), "{served:?}");
}

/// A site that does not declare HTTPS carries none whatever is on disk — the certificate is a
/// property of what the site asked for, not of what a previous declaration left behind.
#[tokio::test]
async fn a_site_that_declares_no_https_carries_none() {
    let (home, store) = home().await;
    site(&store, 1, "blog.test", "", "static", "enabled").await;
    sqlx::query("UPDATE sites SET https_enabled = 0 WHERE id = 1")
        .execute(store.pool())
        .await
        .expect("the row is updated");

    let certs = home.path().join("certs");
    std::fs::create_dir_all(&certs).expect("the certs directory");
    crate::certs::ca::ensure(&certs, std::time::SystemTime::now()).expect("an authority");
    crate::certs::leaf::ensure(
        &certs,
        &["blog.test".to_owned()],
        None,
        std::time::SystemTime::now(),
    )
    .expect("a leaf");

    let served = served(&store, &BTreeMap::new(), &certs, &BTreeMap::new())
        .await
        .expect("the sites are read");

    assert!(served[0].certificate.is_none(), "{served:?}");
}

/// **T81b, D3.** An extension's site is rooted at its install directory, joined component by
/// component like a project's.
#[tokio::test]
async fn an_extension_site_is_rooted_at_its_install_dir() {
    let (home, store) = home().await;
    let install_dir = if cfg!(windows) {
        PathBuf::from(r"C:\mixengine\extensions\phpmyadmin")
    } else {
        PathBuf::from("/mixengine/extensions/phpmyadmin")
    };

    sqlx::query(
        "INSERT INTO extensions (id, name, version, kind, manifest_json, install_dir, data_dir,
                                     source, signed, installed_at)
             VALUES ('phpmyadmin', 'phpMyAdmin', '5.2.1', 'web-app', '{}', ?, '/data/x', 'path',
                     0, '2026-09-03T00:00:00Z')",
    )
    .bind(install_dir.to_string_lossy().into_owned())
    .execute(store.pool())
    .await
    .expect("an extension row");
    sqlx::query(
        "INSERT INTO sites (id, extension_id, doc_root, kind, state)
             VALUES (7, 'phpmyadmin', 'app/public', 'static', 'enabled')",
    )
    .execute(store.pool())
    .await
    .expect("a site row");
    sqlx::query(
        "INSERT INTO site_domains (site_id, domain, is_primary)
             VALUES (7, 'phpmyadmin.mixengine.test', 1)",
    )
    .execute(store.pool())
    .await
    .expect("a domain row");

    let manifest = crate::extensions::manifest::read(
        Path::new("extension.toml"),
        mixengine_testkit::extension::PHPMYADMIN,
    )
    .expect("the fixture parses");
    let installed = BTreeMap::from([(
        "phpmyadmin".to_owned(),
        crate::extensions::store::Installed {
            id: manifest.extension.id.clone(),
            manifest,
            install_dir: install_dir.clone(),
            data_dir: PathBuf::from("/data/x"),
            source: crate::extensions::store::Source::Path,
            signed: false,
            installed_at: mixengine_proto::Timestamp(0),
            ports: BTreeMap::new(),
        },
    )]);

    let served = served(
        &store,
        &BTreeMap::new(),
        &home.path().join("certs"),
        &installed,
    )
    .await
    .expect("the sites");

    assert_eq!(served.len(), 1);
    assert_eq!(served[0].doc_root, install_dir.join("app").join("public"));
    assert_eq!(served[0].primary(), "phpmyadmin.mixengine.test");
}
