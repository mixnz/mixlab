use super::*;
use crate::ProjectError;

fn somewhere() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary directory")
}

fn export(sites: Vec<ExportSite>) -> Export {
    Export {
        name: "blog".to_owned(),
        pins: pins(&[(RuntimeKind::Php, "^8.3")]),
        sites,
    }
}

fn site(domain: &str, aliases: &[&str]) -> ExportSite {
    ExportSite {
        domain: domain.to_owned(),
        aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        doc_root: String::new(),
        https: false,
        kind: mixengine_proto::SiteKind::Static,
        routes: Vec::new(),
        services: Vec::new(),
    }
}

fn linked(id: &str, version: &str) -> ExportService {
    let link = ServiceId::parse(id).expect("an id");
    ExportService {
        name: link.name().to_owned(),
        instance: link.instance().unwrap_or("main").to_owned(),
        link,
        version: Some(PackageVersion::parse(version).expect("a version")),
    }
}

/// **T135, D11.** A round trip that silently dropped half of what a site is would be worse than
/// one that refused, so `[[site.routes]]` is written by an export and read by an import.
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

/// **D9.** The export writes the site, and everything the daemon does not own survives it —
/// including a hand-written key inside an entry it *does* update.
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

/// **D7.** `[site]` with no `kind` is a manifest this build has always accepted, and the type
/// has to keep accepting it.
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

/// **D8.** `[[services]]` is read as a name, an instance and a constraint — and the keys this
/// build does not interpret survive being read past.
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

/// **T77, and the note this overturns.** `database` and `user` used to pass through unread, on
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

/// **What D10 is for.** An export is written into somebody's version-controlled file, so
/// everything it does not own survives it byte for byte.
#[test]
fn writing_a_manifest_keeps_the_comments_the_order_and_the_sections_it_does_not_own() {
    let home = somewhere();
    let original = "# the blog\n\
                        [runtimes]\n\
                        node = \"22\"      # the front end build\n\
                        php = \"8.2\"\n\n\
                        [site]\n\
                        domain = \"blog.test\"\n\n\
                        [[services]]\n\
                        name = \"redis\"\n";
    std::fs::write(at(home.path()), original).expect("a manifest");

    let created = write(home.path(), &export(Vec::new()))
        .expect("it is written")
        .created;

    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert!(!created, "the file was already there");
    assert!(after.contains("# the blog"), "{after}");
    assert!(after.contains("# the front end build"), "{after}");
    assert!(
        after.contains("[site]") && after.contains("blog.test"),
        "{after}"
    );
    assert!(after.contains("[[services]]"), "{after}");
    assert!(
        after.find("node =").expect("node") < after.find("php =").expect("php"),
        "the key order the user chose is theirs: {after}"
    );
    assert!(
        after.contains("php = \"^8.3\""),
        "the owned key changed: {after}"
    );
    assert!(
        after.contains("name = \"blog\""),
        "the name was written: {after}"
    );

    // And what it wrote is what the reader reads back.
    let manifest = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there");
    assert_eq!(
        manifest
            .runtimes
            .get(&RuntimeKind::Php)
            .map(VersionConstraint::as_str),
        Some("^8.3")
    );
}

/// A directory with no manifest gets one, and says that it did.
#[test]
fn a_directory_with_no_manifest_gets_one_written() {
    let home = somewhere();

    let created = write(home.path(), &export(Vec::new()))
        .expect("it is written")
        .created;

    assert!(created);
    assert_eq!(
        read(&at(home.path()))
            .expect("it parses")
            .expect("it is there")
            .project
            .and_then(|project| project.name)
            .as_deref(),
        Some("blog")
    );
}

/// **T204, D1.** `[[sites]]` reads as its entries, in file order.
#[test]
fn several_sites_read_in_the_order_the_file_writes_them() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[[sites]]\ndomain = \"web.test\"\nkind = \"static\"\n\n\
             [[sites]]\ndomain = \"api.test\"\nkind = \"reverse-proxy\"\n\
             upstream = \"http://127.0.0.1:3000\"\n",
    )
    .expect("a manifest");

    let sites = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there")
        .sites;

    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].domain.as_deref(), Some("web.test"));
    assert_eq!(sites[1].domain.as_deref(), Some("api.test"));
}

/// **T204, D1.** One `[site]` is a list of one, which every file written before T204 is.
#[test]
fn a_single_site_reads_as_a_list_of_one() {
    let home = somewhere();
    std::fs::write(at(home.path()), "[site]\ndomain = \"blog.test\"\n").expect("a manifest");

    let sites = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there")
        .sites;

    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].domain.as_deref(), Some("blog.test"));
}

/// **T204, D1.** Both forms at once is a file whose author meant one of them, and choosing would
/// apply something they did not write.
#[test]
fn a_file_with_both_forms_is_refused() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[site]\ndomain = \"a.test\"\n\n[[sites]]\ndomain = \"b.test\"\n",
    )
    .expect("a manifest");

    let error = read(&at(home.path())).expect_err("two answers to one question");

    assert!(
        matches!(&error, Error::Project(ProjectError::Manifest { path, .. }) if path.ends_with(FILE_NAME)),
        "{error:?}"
    );
    assert!(format!("{error:?}").contains("[[sites]]"), "{error:?}");
}

/// **T204, D3.** An absent `services` is not an empty one: absent falls through to every
/// `[[services]]` entry, and `[]` links none.
#[test]
fn an_absent_services_list_is_told_apart_from_an_empty_one() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[[sites]]\ndomain = \"a.test\"\n\n\
             [[sites]]\ndomain = \"b.test\"\nservices = []\n\n\
             [[sites]]\ndomain = \"c.test\"\nservices = [\"mariadb@main\", \"redis\"]\n",
    )
    .expect("a manifest");

    let sites = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there")
        .sites;

    assert_eq!(sites[0].services, None);
    assert_eq!(sites[1].services, Some(Vec::new()));
    assert_eq!(
        sites[2].services,
        Some(vec![
            ServiceId::parse("mariadb@main").expect("an id"),
            ServiceId::parse("redis").expect("an id"),
        ])
    );
}

/// **T204, D3.** A `services` entry that cannot be an id is the file being wrong.
#[test]
fn a_services_entry_that_is_not_an_id_is_refused() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[site]\ndomain = \"a.test\"\nservices = [\"Not An Id\"]\n",
    )
    .expect("a manifest");

    assert!(matches!(
        read(&at(home.path())),
        Err(Error::Project(ProjectError::Manifest { .. }))
    ));
}

/// **T204, D5.** A duplicate domain is refused where a site is chosen, never here, so the shim's
/// `resolve` keeps working in that directory.
#[test]
fn a_duplicate_domain_still_reads() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[runtimes]\nphp = \"8.3\"\n\n[[sites]]\ndomain = \"a.test\"\n\n\
             [[sites]]\ndomain = \"a.test\"\n",
    )
    .expect("a manifest");

    let manifest = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there");

    assert_eq!(manifest.sites.len(), 2);
    assert_eq!(manifest.runtimes.len(), 1);
}

/// **T204, D1.** Two sites into a file with none is `[[sites]]`, each with its own links, and
/// `[[services]]` is the union.
#[test]
fn two_sites_are_written_as_many_with_their_own_services() {
    let home = somewhere();
    let mut web = site("web.test", &[]);
    web.services = vec![linked("mariadb@main", "11.4.2")];
    let mut api = site("api.test", &[]);
    api.services = vec![
        linked("mariadb@main", "11.4.2"),
        linked("redis@main", "7.4.1"),
    ];

    let written = write(home.path(), &export(vec![web, api])).expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert!(written.sites_kept.is_empty(), "{written:?}");
    assert!(!after.contains("[site]"), "{after}");
    assert_eq!(after.matches("[[sites]]").count(), 2, "{after}");
    assert!(after.contains("services = [\"mariadb@main\"]"), "{after}");
    assert!(
        after.contains("services = [\"mariadb@main\", \"redis@main\"]"),
        "{after}"
    );
    assert_eq!(
        after.matches("[[services]]").count(),
        2,
        "one entry per service: {after}"
    );

    let sites = read(&at(home.path()))
        .expect("it parses")
        .expect("it is there")
        .sites;
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[1].services.as_ref().map(Vec::len), Some(2));
}

/// **T204, D1.** One site stays `[site]`, the form every released build reads.
#[test]
fn one_site_is_still_written_as_one() {
    let home = somewhere();

    write(home.path(), &export(vec![site("blog.test", &[])])).expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert!(after.contains("[site]"), "{after}");
    assert!(!after.contains("[[sites]]"), "{after}");
    assert!(
        after.contains("services = []"),
        "D3: written in both forms: {after}"
    );
}

/// **T204, D1.** A file already in `[[sites]]` stays there when only one site is left, so the
/// diff shows the sites changing rather than the form.
#[test]
fn a_file_in_many_stays_in_many_with_one_site() {
    let home = somewhere();
    std::fs::write(at(home.path()), "[[sites]]\ndomain = \"blog.test\"\n").expect("a manifest");

    write(home.path(), &export(vec![site("blog.test", &[])])).expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert_eq!(after.matches("[[sites]]").count(), 1, "{after}");
    assert!(!after.contains("[site]"), "{after}");
}

/// **T204, D1.** A `[site]` a person wrote becomes the first `[[sites]]`, with its comment, its
/// unknown key and their order.
#[test]
fn a_single_site_becomes_the_first_of_many_with_its_comment_and_keys() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[project]\nname = \"blog\"\n\n# the main site\n[site]\ndomain = \"blog.test\"\n\
             # nothing in this build reads this\nlegacy = 1\n",
    )
    .expect("a manifest");

    write(
        home.path(),
        &export(vec![site("blog.test", &[]), site("shop.test", &[])]),
    )
    .expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert!(!after.contains("[site]"), "{after}");
    let first = after.find("[[sites]]").expect("an entry");
    let comment = after.find("# the main site").expect("the comment survives");
    assert!(
        comment < first,
        "the comment stays above its header: {after}"
    );
    assert!(
        after.contains("# nothing in this build reads this\nlegacy = 1"),
        "{after}"
    );
    let blog = after.find("domain = \"blog.test\"").expect("blog");
    let shop = after.find("domain = \"shop.test\"").expect("shop");
    assert!(blog < shop, "the converted entry is first: {after}");
}

/// **T204, D2.** An entry belongs to the site holding its `domain` among any of its names, so a
/// renamed primary updates the entry rather than adding a second one.
#[test]
fn an_entry_is_found_by_an_alias_after_the_primary_moved() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[[sites]]\ndomain = \"blog.test\"\nnote = \"kept\"\n\n\
             [[sites]]\ndomain = \"shop.test\"\n",
    )
    .expect("a manifest");

    let written = write(
        home.path(),
        &export(vec![
            site("www.blog.test", &["blog.test"]),
            site("shop.test", &[]),
        ]),
    )
    .expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert!(written.sites_kept.is_empty(), "{written:?}");
    assert_eq!(after.matches("[[sites]]").count(), 2, "{after}");
    assert!(after.contains("domain = \"www.blog.test\""), "{after}");
    assert!(after.contains("aliases = [\"blog.test\"]"), "{after}");
    assert!(
        after.contains("note = \"kept\""),
        "an unowned key survives: {after}"
    );
}

/// **T204, D2 and D4.** An entry no site holds is left byte for byte and named; so is a second
/// entry for a site an earlier one already took.
#[test]
fn an_entry_no_site_holds_is_kept_and_named() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[[sites]]\ndomain = \"old.test\"   # deleted here, still in git\n\n\
             [[sites]]\ndomain = \"blog.test\"\n\n[[sites]]\ndomain = \"www.blog.test\"\n",
    )
    .expect("a manifest");

    let written = write(
        home.path(),
        &export(vec![
            site("blog.test", &["www.blog.test"]),
            site("new.test", &[]),
        ]),
    )
    .expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert_eq!(written.sites_kept, ["old.test", "www.blog.test"]);
    assert!(
        after.contains("domain = \"old.test\"   # deleted here, still in git"),
        "{after}"
    );
    assert!(after.contains("domain = \"new.test\""), "{after}");
}

/// **T204, D4.** A project with no site leaves a hand-written one and names it.
#[test]
fn no_site_names_every_entry_it_left() {
    let home = somewhere();
    std::fs::write(at(home.path()), "[site]\ndomain = \"typed.test\"\n").expect("a manifest");

    let written = write(home.path(), &export(Vec::new())).expect("it is written");

    assert_eq!(written.sites_kept, ["typed.test"]);
}

/// **T204, D2.** The kind's payload keys are owned, so a proxy that became static loses its
/// `upstream` in either form.
#[test]
fn a_kind_that_changed_takes_its_old_payload_with_it() {
    for original in [
        "[site]\ndomain = \"app.test\"\nkind = \"reverse-proxy\"\n\
             upstream = \"http://127.0.0.1:1\"\n",
        "[[sites]]\ndomain = \"app.test\"\nkind = \"node-app\"\nport = 3000\n",
    ] {
        let home = somewhere();
        std::fs::write(at(home.path()), original).expect("a manifest");

        write(home.path(), &export(vec![site("app.test", &[])])).expect("it is written");
        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        assert!(after.contains("kind = \"static\""), "{after}");
        assert!(!after.contains("upstream"), "{after}");
        assert!(!after.contains("port"), "{after}");
    }
}

fn declaring(text: &str) -> Manifest {
    toml::from_str(text).expect("a manifest")
}

const THREE: &str = "[[sites]]\ndomain = \"web.test\"\naliases = [\"www.web.test\"]\n\n\
                         [[sites]]\ndomain = \"api.test\"\nservices = [\"redis\"]\n\n\
                         [[sites]]\ndomain = \"admin.test\"\nservices = []\n";

/// **T204, D5, step 1.** `from` picks by any of an entry's names.
#[test]
fn from_picks_an_entry_by_any_of_its_names() {
    let manifest = declaring(THREE);

    let chosen = manifest
        .choose(Some("www.web.test"), None, "blog.test")
        .expect("chosen");

    assert_eq!(
        chosen.site.and_then(|site| site.domain.as_deref()),
        Some("web.test")
    );
    assert_eq!(
        chosen.services,
        ServicesFrom::Every,
        "no `services` key: every one"
    );
}

/// **T204, D5, step 1.** A `from` naming nothing the file declares is `NotDeclared`, carrying
/// what it does declare.
#[test]
fn from_naming_nothing_is_not_declared() {
    let manifest = declaring(THREE);

    assert_eq!(
        manifest.choose(Some("nope.test"), None, "blog.test"),
        Err(Unchosen::NotDeclared {
            name: "nope.test".to_owned(),
            declared: vec!["web.test".into(), "api.test".into(), "admin.test".into()],
        })
    );
}

/// **T204, D5, step 2.** A named domain picks its entry; one the file does not describe picks
/// none, and no services fall through.
#[test]
fn a_named_domain_picks_its_entry_or_none() {
    let manifest = declaring(THREE);
    let redis = [ServiceId::parse("redis").expect("an id")];

    let api = manifest
        .choose(None, Some(&["api.test".to_owned()]), "blog.test")
        .expect("chosen");
    assert_eq!(api.services, ServicesFrom::Listed(&redis));

    let other = manifest
        .choose(None, Some(&["other.test".to_owned()]), "blog.test")
        .expect("chosen");
    assert_eq!(other.site, None);
    assert_eq!(other.services, ServicesFrom::Nothing);
}

/// **T204, D5, steps 2 and 3.** One entry is today's behaviour: chosen whatever the domain.
#[test]
fn one_entry_is_chosen_as_it_always_was() {
    let manifest = declaring("[site]\ndomain = \"blog.test\"\n");

    for domains in [None, Some(vec!["other.test".to_owned()])] {
        let chosen = manifest
            .choose(None, domains.as_deref(), "blog.test")
            .expect("chosen");
        assert!(chosen.site.is_some());
        assert_eq!(chosen.services, ServicesFrom::Every);
    }
}

/// **T204, D5, step 3.** Several and nothing named is refused rather than guessed.
#[test]
fn several_and_nothing_named_is_ambiguous() {
    assert!(matches!(
        declaring(THREE).choose(None, None, "blog.test"),
        Err(Unchosen::Ambiguous { declared }) if declared.len() == 3
    ));
}

/// **T204, D5.** No site at all: the defaults, with `[[services]]` as before.
#[test]
fn no_entry_falls_through_to_every_service() {
    let manifest = Manifest::default();

    let chosen = manifest.choose(None, None, "blog.test").expect("chosen");

    assert_eq!(chosen.site, None);
    assert_eq!(chosen.services, ServicesFrom::Every);
}

/// **T204, D5.** Two entries answering to one name is refused at the point of choosing,
/// numbered the way a person counts them.
#[test]
fn named_twice() {
    let manifest = declaring(
        "[[sites]]\ndomain = \"a.test\"\n\n\
             [[sites]]\ndomain = \"b.test\"\naliases = [\"a.test\"]\n",
    );

    assert_eq!(
        manifest.choose(Some("a.test"), None, "blog.test"),
        Err(Unchosen::NamedTwice {
            name: "a.test".into(),
            first: 1,
            second: 2
        })
    );
}

/// **T204, D7.** An entry with no `domain` answers to the name `site.create` would give it.
#[test]
fn an_entry_with_no_domain_answers_to_the_default() {
    let manifest =
        declaring("[[sites]]\ndoc_root = \"public\"\n\n[[sites]]\ndomain = \"b.test\"\n");

    let chosen = manifest
        .choose(Some("blog.test"), None, "blog.test")
        .expect("chosen");

    assert_eq!(
        chosen.site.and_then(|site| site.doc_root.as_deref()),
        Some("public")
    );
}

/// **T204, final review.** A domain is a name whatever its case, as the daemon stores it, so a
/// hand-written `Blog.test` answers the `from` MixLab sends, which is lowercase.
#[test]
fn from_matches_a_name_whatever_its_case() {
    let manifest = declaring(
        "[[sites]]\ndomain = \"Blog.test\"\n\n[[sites]]\ndomain = \"shop.test\"\n\
             aliases = [\"WWW.shop.test\"]\n",
    );

    for name in ["blog.test", "www.shop.test"] {
        assert!(
            manifest
                .choose(Some(name), None, "x.test")
                .expect("chosen")
                .site
                .is_some(),
            "{name}"
        );
    }
}

/// **T204, final review.** An export finds the entry a person wrote in another case, rather
/// than keeping it and adding a second one for the same site.
#[test]
fn an_entry_is_matched_whatever_the_case_of_its_domain() {
    let home = somewhere();
    std::fs::write(
        at(home.path()),
        "[[sites]]\ndomain = \"Blog.test\"\n\n[[sites]]\ndomain = \"shop.test\"\n",
    )
    .expect("a manifest");

    let written = write(
        home.path(),
        &export(vec![site("blog.test", &[]), site("shop.test", &[])]),
    )
    .expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert!(written.sites_kept.is_empty(), "{written:?}");
    assert_eq!(after.matches("[[sites]]").count(), 2, "{after}");
}

/// **T204, checked by hand.** A kind that did not change keeps its payload key where the file
/// had it, so an export with nothing new to say shows no moved line in a diff.
#[test]
fn a_kind_that_stayed_keeps_its_payload_where_it_was() {
    for original in [
        "[site]\ndomain = \"app.test\"\nkind = \"reverse-proxy\"\n\
             upstream = \"http://127.0.0.1:5173\"\nhttps = false\n",
        "[[sites]]\ndomain = \"app.test\"\nkind = \"reverse-proxy\"\n\
             upstream = \"http://127.0.0.1:5173\"\nhttps = false\n",
    ] {
        let home = somewhere();
        std::fs::write(at(home.path()), original).expect("a manifest");

        let mut proxy = site("app.test", &[]);
        proxy.kind = mixengine_proto::SiteKind::ReverseProxy {
            upstream: "http://127.0.0.1:5173".to_owned(),
        };
        write(home.path(), &export(vec![proxy])).expect("it is written");
        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        let upstream = after.find("upstream =").expect("the upstream");
        let https = after.find("https =").expect("https");
        assert!(
            upstream < https,
            "the upstream stayed where it was: {after}"
        );
    }
}

/// **T204, checked by hand.** A project with no pins writes no empty `[runtimes]`, and leaves a
/// hand-written one as it is.
#[test]
fn no_pins_write_no_runtimes_table() {
    let nothing = |sites| Export {
        name: "blog".to_owned(),
        pins: BTreeMap::new(),
        sites,
    };

    let home = somewhere();
    write(home.path(), &nothing(Vec::new())).expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");
    assert!(!after.contains("[runtimes]"), "{after}");

    let home = somewhere();
    std::fs::write(at(home.path()), "[runtimes]\nphp = \"8.2\"\n").expect("a manifest");
    write(home.path(), &nothing(Vec::new())).expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");
    assert!(after.contains("[runtimes]\nphp = \"8.2\""), "{after}");
}
