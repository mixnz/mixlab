//! Writing a manifest: one site or many, comments kept, entries found by alias.

use super::*;

/// **Why the writer edits rather than serialises.** An export is written into somebody's
/// version-controlled file, so everything it does not own survives it byte for byte.
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

/// `[[sites]]` reads as its entries, in file order.
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

/// One `[site]` is a list of one, which every file written before `[[sites]]` existed is.
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

/// Both forms at once is a file whose author meant one of them, and choosing would apply something
/// they did not write.
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

/// An absent `services` is not an empty one: absent falls through to every `[[services]]` entry,
/// and `[]` links none.
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

/// A `services` entry that cannot be an id is the file being wrong.
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

/// A duplicate domain is refused where a site is chosen, never here, so the shim's `resolve` keeps
/// working in that directory.
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

/// Two sites into a file with none is `[[sites]]`, each with its own links, and `[[services]]` is
/// the union.
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

/// One site stays `[site]`, the form every released build reads.
#[test]
fn one_site_is_still_written_as_one() {
    let home = somewhere();

    write(home.path(), &export(vec![site("blog.test", &[])])).expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert!(after.contains("[site]"), "{after}");
    assert!(!after.contains("[[sites]]"), "{after}");
    assert!(
        after.contains("services = []"),
        "a site's links are written in both forms: {after}"
    );
}

/// A file already in `[[sites]]` stays there when only one site is left, so the diff shows the
/// sites changing rather than the form.
#[test]
fn a_file_in_many_stays_in_many_with_one_site() {
    let home = somewhere();
    std::fs::write(at(home.path()), "[[sites]]\ndomain = \"blog.test\"\n").expect("a manifest");

    write(home.path(), &export(vec![site("blog.test", &[])])).expect("it is written");
    let after = std::fs::read_to_string(at(home.path())).expect("the file");

    assert_eq!(after.matches("[[sites]]").count(), 1, "{after}");
    assert!(!after.contains("[site]"), "{after}");
}

/// A `[site]` a person wrote becomes the first `[[sites]]`, with its comment, its unknown key and
/// their order.
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

/// An entry belongs to the site holding its `domain` among any of its names, so a renamed primary
/// updates the entry rather than adding a second one.
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

/// An entry no site holds is left byte for byte and named; so is a second entry for a site an
/// earlier one already took.
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

/// A project with no site leaves a hand-written one and names it.
#[test]
fn no_site_names_every_entry_it_left() {
    let home = somewhere();
    std::fs::write(at(home.path()), "[site]\ndomain = \"typed.test\"\n").expect("a manifest");

    let written = write(home.path(), &export(Vec::new())).expect("it is written");

    assert_eq!(written.sites_kept, ["typed.test"]);
}

/// The kind's payload keys are owned, so a proxy that became static loses its `upstream` in either
/// form.
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
