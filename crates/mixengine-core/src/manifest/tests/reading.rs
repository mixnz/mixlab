//! Reading a manifest: sites, kinds, services, pins and the errors a file can hold.

use super::*;

/// A round trip that silently dropped half of what a site is would be worse than one that refused,
/// so `[[site.routes]]` is written by an export and read by an import.
#[test]
fn routes_survive_an_export_and_come_back() {
    let home = somewhere();

    let routes = vec![
        mixengine_proto::SiteRoute {
            path: "/api".to_owned(),
            target: mixengine_proto::RouteTarget::Proxy {
                upstream: "http://127.0.0.1:3003/xyz".to_owned(),
            },
        },
        mixengine_proto::SiteRoute {
            path: "/admin".to_owned(),
            target: mixengine_proto::RouteTarget::PhpFpm {
                pool: Some(mixengine_proto::ServiceId::parse("php-fpm@8.3.33").expect("an id")),
            },
        },
        mixengine_proto::SiteRoute {
            path: "/assets".to_owned(),
            target: mixengine_proto::RouteTarget::Static {
                root: "dist".to_owned(),
            },
        },
    ];

    write(
        home.path(),
        &export(vec![ExportSite {
            domain: "blog.test".to_owned(),
            aliases: Vec::new(),
            doc_root: String::new(),
            https: false,
            kind: mixengine_proto::SiteKind::NodeApp { port: 3000 },
            routes: routes.clone(),
            services: Vec::new(),
        }]),
    )
    .expect("a manifest");

    let manifest = read(&at(home.path())).expect("a read").expect("a manifest");
    let site = manifest.sites.into_iter().next().expect("a site");

    assert_eq!(site.routes, routes);
    assert_eq!(
        site.kind,
        Some(mixengine_proto::SiteKind::NodeApp { port: 3000 }),
        "the routes beside the kind do not confuse the kind"
    );
}

/// The export writes the site, and everything the daemon does not own survives it — including a
/// hand-written key inside an entry it *does* update.
#[test]
fn an_export_writes_the_site_and_leaves_every_hand_written_key_alone() {
    let home = somewhere();
    let original = "# the blog\n\
                        [runtimes]\n\
                        php = \"8.2\"\n\n\
                        [[services]]\n\
                        name = \"mariadb\"\n\
                        instance = \"main\"\n\
                        version = \"11.3\"\n\
                        database = \"blog\"      # provisioned by hand, for now\n\n\
                        [[services]]\n\
                        name = \"meilisearch\"   # nothing in this build knows what this is\n";
    std::fs::write(at(home.path()), original).expect("a manifest");

    write(
        home.path(),
        &export(vec![ExportSite {
            domain: "blog.test".to_owned(),
            aliases: vec!["api.blog.test".to_owned()],
            doc_root: "public".to_owned(),
            https: true,
            kind: mixengine_proto::SiteKind::PhpFpm { pool: None },
            routes: Vec::new(),
            services: vec![ExportService {
                link: ServiceId::parse("mariadb@main").expect("an id"),
                name: "mariadb".to_owned(),
                instance: "main".to_owned(),
                version: Some(PackageVersion::parse("11.4.2").expect("a version")),
            }],
        }]),
    )
    .expect("it is written");

    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert!(after.contains("# the blog"), "{after}");
    assert!(after.contains("domain = \"blog.test\""), "{after}");
    assert!(after.contains("aliases = [\"api.blog.test\"]"), "{after}");
    assert!(after.contains("doc_root = \"public\""), "{after}");
    assert!(after.contains("kind = \"php-fpm\""), "{after}");
    assert!(after.contains("https = true"), "{after}");

    // Add and update; never delete.
    assert!(
        after.contains("version = \"11.4.2\""),
        "the link was updated: {after}"
    );
    assert!(
        after.contains("database = \"blog\""),
        "a hand-written key inside an updated entry survives: {after}"
    );
    assert!(
        after.contains("meilisearch"),
        "an entry the daemon knows nothing about is left exactly as it is: {after}"
    );
    assert!(after.contains("# provisioned by hand"), "{after}");
}

/// A kind's payload is written from an exhaustive match, so a fifth kind cannot be forgotten.
#[test]
fn a_proxy_is_written_with_the_address_it_forwards_to() {
    let home = somewhere();

    write(
        home.path(),
        &export(vec![ExportSite {
            domain: "app.test".to_owned(),
            aliases: Vec::new(),
            doc_root: String::new(),
            https: false,
            kind: mixengine_proto::SiteKind::ReverseProxy {
                upstream: "http://127.0.0.1:5173".to_owned(),
            },
            routes: Vec::new(),
            services: Vec::new(),
        }]),
    )
    .expect("it is written");

    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert!(after.contains("kind = \"reverse-proxy\""), "{after}");
    assert!(
        after.contains("upstream = \"http://127.0.0.1:5173\""),
        "{after}"
    );

    // And it reads back as the kind it was written from.
    let read_back = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there")
        .sites
        .into_iter()
        .next()
        .expect("a [site]");
    assert_eq!(
        read_back.kind,
        Some(mixengine_proto::SiteKind::ReverseProxy {
            upstream: "http://127.0.0.1:5173".to_owned()
        })
    );
}

/// A project with no site writes no `[site]`, and does not delete one somebody wrote by hand.
#[test]
fn an_export_with_no_site_leaves_a_hand_written_one_alone() {
    let home = somewhere();
    std::fs::write(at(home.path()), "[site]\ndomain = \"typed.test\"\n").expect("a manifest");

    write(home.path(), &export(Vec::new())).expect("it is written");

    let after = std::fs::read_to_string(at(home.path())).expect("the file");
    assert!(after.contains("typed.test"), "{after}");
}

/// `[site]` with no `kind` is a manifest this build has always accepted, and the type has to keep
/// accepting it.
#[test]
fn a_site_with_no_kind_reads_as_one_that_named_none() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[site]\ndomain = \"blog.test\"\naliases = [\"api.blog.test\"]\ndoc_root = \"public\"\n",
    )
    .expect("a manifest");

    let site = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there")
        .sites
        .into_iter()
        .next()
        .expect("a [site]");

    assert_eq!(site.domain.as_deref(), Some("blog.test"));
    assert_eq!(site.aliases, ["api.blog.test"]);
    assert_eq!(site.doc_root.as_deref(), Some("public"));
    assert_eq!(site.kind, None, "no kind is not the same as php-fpm");
    assert_eq!(site.https, None);
}

/// A kind reads out of the flat table beside the keys it has no use for.
#[test]
fn a_kind_reads_from_the_table_it_shares_with_the_rest_of_the_site() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[site]\ndomain = \"blog.test\"\nkind = \"reverse-proxy\"\n\
             upstream = \"http://127.0.0.1:5173\"\nhttps = true\n",
    )
    .expect("a manifest");

    let site = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there")
        .sites
        .into_iter()
        .next()
        .expect("a [site]");

    assert_eq!(
        site.kind,
        Some(mixengine_proto::SiteKind::ReverseProxy {
            upstream: "http://127.0.0.1:5173".to_owned()
        })
    );
    assert_eq!(site.https, Some(true));
}

/// And a kind that cannot be one is refused by the enum, naming the file.
#[test]
fn a_proxy_with_no_upstream_is_refused_by_the_definition() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[site]\ndomain = \"blog.test\"\nkind = \"reverse-proxy\"\n",
    )
    .expect("a manifest");

    let error = read(&at(home.path())).expect_err("a proxy with nowhere to go");

    assert!(
        matches!(&error, Error::Project(ProjectError::Manifest { path, .. }) if path.ends_with(FILE_NAME)),
        "{error:?}"
    );
}

/// `[[services]]` is read as a name, an instance and a constraint — and the keys this build does
/// not interpret survive being read past.
#[test]
fn services_are_read_as_names_instances_and_constraints() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[[services]]\nname = \"mariadb\"\ninstance = \"main\"\nversion = \"11.4\"\n\
             database = \"blog\"\n\n[[services]]\nname = \"redis\"\n",
    )
    .expect("a manifest");

    let services = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there")
        .services;

    assert_eq!(services.len(), 2);
    assert_eq!(services[0].name, "mariadb");
    assert_eq!(services[0].instance.as_deref(), Some("main"));
    assert_eq!(
        services[0].version.as_ref().map(VersionConstraint::as_str),
        Some("11.4")
    );
    assert_eq!(
        services[1].instance, None,
        "absent is not the same as \"main\""
    );
    assert_eq!(services[1].version, None);
}

/// **The note this overturns.** `database` and `user` used to pass through unread, on
/// the rule that a key nothing acts on is a promise not kept. `blueprint.capture` is what acts
/// on them: without this, a blueprint captured from a Laravel project carries no database name
/// and the site it creates elsewhere connects to nothing.
#[test]
fn a_service_entry_carries_the_database_and_the_account_it_names() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[[services]]
name = \"mariadb\"
instance = \"main\"
database = \"blog\"
             user = \"blog\"

[[services]]
name = \"redis\"
",
    )
    .expect("a manifest");

    let services = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there")
        .services;

    assert_eq!(services[0].database.as_deref(), Some("blog"));
    assert_eq!(services[0].user.as_deref(), Some("blog"));
    assert_eq!(
        services[1].database, None,
        "a service that names no database has none"
    );
}

/// A `version` whose *syntax* is wrong is the file being wrong, and is refused. Whether
/// anything installed satisfies it is a different question, asked by the daemon and reported.
#[test]
fn a_version_that_is_not_a_constraint_is_refused() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[[services]]\nname = \"mariadb\"\nversion = \"~11.4\"\n",
    )
    .expect("a manifest");

    assert!(matches!(
        read(&at(home.path())),
        Err(Error::Project(ProjectError::Manifest { .. }))
    ));
}

/// The whole file, where `resolve` used to read a third of it — and the sections this build has
/// no types for still must not make it refuse the file.
#[test]
fn a_manifest_declaring_more_than_runtimes_is_read_whole() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[project]\nname = \"blog\"\n\n[runtimes]\nphp = \"^8.3\"\n\n\
             [site]\ndomain = \"blog.test\"\n\n[[services]]\nname = \"redis\"\n",
    )
    .expect("a manifest");

    let manifest = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there");

    assert_eq!(
        manifest.project.and_then(|project| project.name).as_deref(),
        Some("blog")
    );
    assert_eq!(
        manifest
            .runtimes
            .get(&RuntimeKind::Php)
            .map(VersionConstraint::as_str),
        Some("^8.3")
    );
}

/// A directory with no manifest is not a failure — it is the ordinary case.
#[test]
fn a_directory_with_no_manifest_answers_nothing_rather_than_failing() {
    let home = somewhere();

    assert_eq!(read(&at(home.path())).expect("no manifest is fine"), None);
}

/// A pin that does nothing looks exactly like a pin that does not work, so the file is refused
/// by name — `ProjectError::Manifest`'s own reasoning, kept when the reader moved here.
#[test]
fn a_manifest_that_does_not_parse_names_itself() {
    let home = somewhere();

    for body in [
        "[runtimes]\nphp = \"~8.3\"\n",
        "[runtimes]\nphhp = \"8.3\"\n",
        "[runtimes\n",
    ] {
        std::fs::write(at(home.path()), body).expect("a manifest");

        let error = read(&at(home.path())).expect_err("the manifest is wrong");

        assert!(
            matches!(&error, Error::Project(ProjectError::Manifest { path, .. }) if path.ends_with(FILE_NAME)),
            "{error:?} for {body:?}"
        );
    }
}
