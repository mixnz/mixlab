use super::*;

/// T191 D3: a static route's root reaches the wire as this system spells a path.
#[test]
fn a_static_route_root_is_spelled_natively() {
    let route = SiteRoute {
        path: "/assets".to_owned(),
        target: RouteTarget::Static {
            root: "dist/css".to_owned(),
        },
    };

    let RouteTarget::Static { root } = natively(route).target else {
        panic!("still a static route");
    };
    assert_eq!(root, ["dist", "css"].join(std::path::MAIN_SEPARATOR_STR));
}

/// Review focus 1: what MixLab sends back after reading the native spelling is stored with `/`.
#[test]
fn a_natively_spelled_doc_root_is_stored_with_slashes() {
    let root = tempfile::tempdir().expect("a project directory");
    let sent = mixengine_core::paths::native_relative("public/assets");

    assert_eq!(
        sites::relative_doc_root(root.path(), &sent).expect("inside"),
        "public/assets"
    );
}

/// **T81b, D6.** The sentence an extension-owned site refuses every edit with — one sentence,
/// naming the one thing a person can do.
#[test]
fn an_extension_site_refuses_with_the_command_that_removes_it() {
    let refusal = owned_by_an_extension(
        "phpmyadmin.mixengine.test",
        &ExtensionId::parse("phpmyadmin").expect("an id"),
    );

    assert_eq!(refusal.code, ErrorCode::PreconditionFailed);
    assert_eq!(
        refusal.message,
        "phpmyadmin.mixengine.test belongs to the phpmyadmin extension"
    );
    assert_eq!(
        refusal.hint.as_deref(),
        Some("`mix extension uninstall phpmyadmin` removes it")
    );
}

/// The fourth check, which is the whole of what this build knows about a proxy target.
#[test]
fn an_upstream_is_an_absolute_http_address_with_a_host() {
    for good in [
        "http://127.0.0.1:8080",
        "https://localhost:5173",
        "http://127.0.0.1:3000/api",
    ] {
        upstream_is_an_address(good).expect(good);
    }

    for bad in [
        "127.0.0.1:8080",
        "ftp://127.0.0.1",
        "http://",
        "http://127.0.0.1:8080?a=b",
        "http://127.0.0.1:8080#top",
    ] {
        assert!(upstream_is_an_address(bad).is_err(), "{bad} was accepted");
    }
}

/// **T135, D10.** An upstream is rendered verbatim into a Caddyfile and into `nginx.conf`, so
/// what it may contain is a whitelist rather than a list of things somebody thought of.
///
/// The first case is the hole this closes: a newline, a closing brace and a second server on
/// Caddy's admin port, in one string a JSON member can carry.
#[test]
fn an_upstream_may_not_carry_configuration() {
    for bad in [
        "http://127.0.0.1:8080\n}\nadmin 0.0.0.0:2019 {",
        "http://127.0.0.1:8080 extra",
        "http://127.0.0.1:8080`",
        "http://127.0.0.1:8080\"",
        "http://127.0.0.1:8080;",
        "http://127.0.0.1:8080{",
        "http://127.0.0.1:8080\\x",
    ] {
        assert!(upstream_is_an_address(bad).is_err(), "{bad:?} was accepted");
    }

    for good in [
        "http://127.0.0.1:8080",
        "https://localhost:5173",
        "http://127.0.0.1:3003/xyz",
    ] {
        upstream_is_an_address(good).expect(good);
    }
}

/// **T135, D10.** A route path is a prefix, at a segment boundary, drawn from an alphabet that
/// cannot reach a Caddy matcher or an nginx regex as anything but itself.
#[test]
fn a_route_path_is_an_absolute_prefix() {
    for good in ["/api", "/api/v1", "/v1.0", "/a-b_c~d%20e"] {
        route_path_is_a_prefix(good).expect(good);
    }

    for bad in [
        "/",         // what answers `/` is the site's kind — D1
        "api",       // not absolute
        "/api/",     // a trailing slash is not part of a prefix
        "/api//v1",  // an empty segment
        "/api/../x", // traversal
        "/api/*",    // a Caddy matcher wildcard
        "/api?x=1",  // a query
        "/api#top",  // a fragment
        "/api x",    // whitespace
        "/api\n{",   // configuration
    ] {
        assert!(route_path_is_a_prefix(bad).is_err(), "{bad:?} was accepted");
    }
}

/// **T135, D10.** The whole list, which is where a duplicate and a flood are visible.
#[test]
fn a_route_list_refuses_a_duplicate_and_a_flood() {
    let one = |path: &str| SiteRoute {
        path: path.to_owned(),
        target: RouteTarget::Static {
            root: "dist".to_owned(),
        },
    };

    assert!(
        route_list_is_writable(&[one("/a"), one("/a")]).is_err(),
        "a duplicate path"
    );
    assert!(
        route_list_is_writable(&(0..33).map(|n| one(&format!("/r{n}"))).collect::<Vec<_>>())
            .is_err(),
        "thirty-three routes"
    );
    assert!(
        route_list_is_writable(&[SiteRoute {
            path: "/a".to_owned(),
            target: RouteTarget::Proxy {
                upstream: "127.0.0.1:1".to_owned()
            },
        }])
        .is_err(),
        "a route's upstream is an address, checked by the one function that knows what that is"
    );

    route_list_is_writable(&[one("/a"), one("/b")]).expect("two routes");
}

/// A doc root joins onto the root with whatever separator this OS uses, and `""` is the root.
#[test]
fn a_stored_doc_root_joins_back_onto_its_root() {
    let root = Path::new("/srv/blog");

    assert_eq!(doc_root_full(root, ""), PathBuf::from("/srv/blog"));
    assert_eq!(
        doc_root_full(root, "public"),
        PathBuf::from("/srv/blog").join("public")
    );
    assert_eq!(
        doc_root_full(root, "web/public"),
        PathBuf::from("/srv/blog").join("web").join("public")
    );
}
