//! Which entry a domain picks.

use super::*;

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
