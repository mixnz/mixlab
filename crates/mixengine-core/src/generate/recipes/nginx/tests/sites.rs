//! What a site renders to: its server blocks, routes, welcome page, pools, the shared authority,
//! LAN sharing, TLS and redirects.

use super::*;

/// D6: nginx's `fastcgi_params` comes out of the package rather than being written into this
/// template by hand. `mixengine-packages`' `tools/nginx.py` publishes it under `CONF_FILES` for
/// exactly this reason, and copying seventeen `fastcgi_param` lines in here would be this
/// repository maintaining a second copy of a file the server already reads.
#[test]
fn the_package_supplies_the_fastcgi_parameters_a_php_site_needs() {
    let endpoints = Nginx
        .endpoints(&context("{}"))
        .expect("a package publishing what a generated configuration includes");

    assert!(
        endpoints.includes.contains_key("fastcgi_params"),
        "a generated nginx configuration has no conf/ beside it, so the file has to be reached \
             where the artifact keeps it"
    );
}

/// D8, nginx's half. Caddy takes the port from its global block; nginx has none, so each site's
/// `server` block declares its own `listen` — and it is the port the process **binds**, which on
/// macOS is 8080 for a front end answering on 80.
#[test]
fn a_site_listens_on_the_port_the_process_binds() {
    let context = context_on("{}", Some(80)).with_bindings(vec![PortBinding {
        answer: 80,
        bind: 8080,
    }]);

    let served = vec![Served {
        shared: None,
        domains: vec!["blog.test".to_owned(), "www.blog.test".to_owned()],
        routes: Vec::new(),
        doc_root: doc_root(),
        doc_root_relative: "public".to_owned(),
        kind: ServedKind::Static,
        https: true,
        https_redirect: false,
        certificate: None,
        steps: Vec::new(),
    }];

    let rendered = Nginx.sites(&context, &served).expect("one site")[0]
        .contents()
        .to_owned();

    assert!(rendered.contains("listen 127.0.0.1:8080;"), "{rendered}");
    assert!(
        rendered.contains("server_name blog.test www.blog.test;"),
        "{rendered}"
    );
}

/// Each kind renders the directive that kind needs, and a `node-app` renders exactly what a
/// reverse proxy to loopback renders — D7 asserted rather than described.
#[test]
fn each_kind_renders_a_server_block_naming_what_it_was_given() {
    let served = vec![
        Served {
            shared: None,
            domains: vec!["php.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::PhpFpm {
                upstream: Upstream::Socket(PathBuf::from("/home/me/run/php-fpm-8.3.sock")),
                activator: None,
            },
            https: true,
            https_redirect: false,
            certificate: None,
            steps: Vec::new(),
        },
        Served {
            shared: None,
            domains: vec!["proxy.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::ReverseProxy {
                upstream: "http://127.0.0.1:4000".to_owned(),
                rewrite: None,
            },
            https: true,
            https_redirect: false,
            certificate: None,
            steps: Vec::new(),
        },
        Served {
            shared: None,
            domains: vec!["node.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::NodeApp { port: 3000 },
            https: true,
            https_redirect: false,
            certificate: None,
            steps: Vec::new(),
        },
    ];

    let documents = Nginx
        .sites(&context("{}"), &served)
        .expect("three site files");

    assert_eq!(
        documents[0].relative(),
        Path::new("sites").join("php.test.conf")
    );

    let php = documents[0].contents();
    assert!(
        php.contains("fastcgi_pass \"unix:/home/me/run/php-fpm-8.3.sock\";"),
        "a socket is spelled nginx's way, which is not Caddy's: {php}"
    );
    assert!(php.contains("include \""), "{php}");

    assert!(
        documents[1]
            .contents()
            .contains("proxy_pass http://127.0.0.1:4000;")
    );
    assert!(
        documents[2]
            .contents()
            .contains("proxy_pass http://127.0.0.1:3000;")
    );
}

/// **One file per extension, named after it** — roadmap task **T81c**. The fragment arrives
/// already substituted and already forward-slashed; what this adds is the header.
#[test]
fn an_extension_s_fragment_is_a_file_of_its_own() {
    let context = context("{}").with_fragments(vec![FrontEndAddition {
        extension: "probe".to_owned(),
        fragment: "map $a $b { default 0; }".to_owned(),
    }]);

    let documents = Nginx.fragments(&context).expect("the fragments render");

    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].relative(),
        Path::new("extensions").join("probe.conf")
    );
    assert!(
        documents[0]
            .contents()
            .ends_with("map $a $b { default 0; }\n"),
        "{}",
        documents[0].contents()
    );
}

/// One site per file and one fragment per extension, and all three directories are swept —
/// `welcome/` since roadmap task **T124**, because a deleted site that kept its page would be a
/// page served for a site that no longer exists.
#[test]
fn the_front_end_sweeps_the_directories_that_follow_a_table() {
    assert_eq!(Nginx.swept(), &["sites", "extensions", "welcome"]);
}

/// **Two documents per site now** — roadmap task T124: the site's configuration, and the page
/// it answers with when it has nothing to serve.
#[test]
fn a_site_renders_a_welcome_page_beside_its_configuration() {
    let documents = Nginx
        .sites(&context("{}"), &[a_static_site()])
        .expect("a rendering");

    let names: Vec<_> = documents
        .iter()
        .map(|document| {
            document
                .relative()
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/")
        })
        .collect();

    assert!(
        names.contains(&"sites/blog.test.conf".to_owned()),
        "{names:?}"
    );
    assert!(
        names.contains(&"welcome/blog.test.html".to_owned()),
        "{names:?}"
    );
}

/// **The root path only, and after the site's own index files** — the T124 design, D3. A
/// catch-all `error_page 404` would replace an application's own 404, which is MixEngine lying
/// about somebody else's program.
#[test]
fn a_static_site_falls_back_to_the_welcome_page_only_at_the_root() {
    let rendered = render_site(&a_static_site());

    assert!(
        rendered.contains("location = / {"),
        "the welcome route must be nginx's exact match:
{rendered}"
    );
    assert!(
        rendered.contains("try_files /index.html /index.htm @mixengine_welcome;"),
        "the site's own index files must be named before the fallback:
{rendered}"
    );
    assert!(
        !rendered.contains("error_page 404 ="),
        "a file-serving site never replaces an application's own 404:
{rendered}"
    );
    assert!(
        rendered.contains("add_header Cache-Control \"no-store\""),
        "{rendered}"
    );
}

/// **Both `server` blocks carry it.** `site.conf` renders the serving locations twice — once in
/// the block `https_redirect` splits out and once in the combined one — and the copy that is
/// missed is the one a user with the redirect turned on meets, with both suites still green.
#[test]
fn both_server_blocks_fall_back_to_the_welcome_page() {
    let combined = render_site(&a_static_site());
    let redirecting = render_site(&Served {
        https_redirect: true,
        ..a_static_site()
    });

    assert_eq!(
        combined.matches("location @mixengine_welcome").count(),
        1,
        "one serving block, one fallback:
{combined}"
    );
    assert_eq!(
        redirecting.matches("location @mixengine_welcome").count(),
        1,
        "the redirecting block serves nothing, so only the TLS block gets it:
{redirecting}"
    );
}

/// **A dead upstream is answered, at every path** — the T124 design, D4. Wide is safe here in
/// a way it is not for the file kinds: a 502 nginx generated means nothing was listening, so
/// there is no application whose answer is being overwritten.
#[test]
fn a_proxy_site_answers_a_dead_upstream_with_the_welcome_page() {
    let rendered = render_site(&Served {
        kind: ServedKind::NodeApp { port: 3000 },
        ..a_static_site()
    });

    assert!(
        rendered.contains("error_page 502 504 = @mixengine_welcome;"),
        "{rendered}"
    );
    assert!(
        rendered.contains("location @mixengine_welcome"),
        "{rendered}"
    );
}

/// **And an upstream's own 502 passes through untouched** — D4's negative half, asserted
/// because the failure it guards against is a directive somebody adds later believing it
/// belongs. With `proxy_intercept_errors on;` a user's own gateway reporting its own upstream
/// would be replaced by MixEngine's page, which is this feature overwriting an application's
/// answer — the exact thing D3 refuses to do to a 404.
///
/// **The directive and not the word**, on `two_sites_on_one_pool_declare_two_differently_named_groups`'
/// rule: the comment above it in `site.conf` names it too, and a search for the word would be
/// green forever whatever the configuration said.
#[test]
fn an_upstreams_own_gateway_error_is_not_intercepted() {
    let rendered = render_site(&Served {
        kind: ServedKind::NodeApp { port: 3000 },
        ..a_static_site()
    });

    let directive = rendered
        .lines()
        .find(|line| line.trim_start().starts_with("proxy_intercept_errors"));

    assert!(
        directive.is_none(),
        "this directive would capture a 502 the application sent: {directive:?}"
    );
}

/// **The root-only rule belongs to the kinds that serve files.** A proxy site with a working
/// upstream serves `/` from that upstream, and an exact-match location in front of it would
/// take the site's own home page away.
#[test]
fn a_proxy_site_has_no_root_only_welcome_location() {
    let rendered = render_site(&Served {
        kind: ServedKind::ReverseProxy {
            upstream: "http://127.0.0.1:8000".to_owned(),
            rewrite: None,
        },
        ..a_static_site()
    });

    assert!(
        !rendered.contains("location = / {"),
        "a proxy site's `/` belongs to its upstream:
{rendered}"
    );
}

/// **Off renders neither the page nor the route** — the T124 design, D6. Either half alone is
/// worse than neither: a page nothing routes to is a file nobody reads, and a route with no
/// page behind it is a 404 on top of the 404 this feature exists to replace.
#[test]
fn the_switch_turned_off_renders_no_welcome_at_all() {
    let documents = Nginx
        .sites(&context("{}").with_welcome(false), &[a_static_site()])
        .expect("a rendering");

    assert_eq!(documents.len(), 1, "only the site's own configuration");
    assert!(
        !documents[0].contents().contains("location = / {"),
        "{}",
        documents[0].contents()
    );
}

/// **A php-fpm site is answered through the index module, never through `try_files`** — roadmap
/// task **T124a**, and the assertion that keeps T124's source leak out.
///
/// `try_files` serves the first file it finds *in the current context*, and this location has no
/// `fastcgi_pass`, so a `location = /` naming `/index.php` would answer the site's home page
/// with the site's own source, as text. The index module makes an **internal redirect** instead,
/// so the same file is re-matched by `location ~ \.php$` and runs as PHP.
#[test]
fn a_php_site_is_answered_through_the_index_module_and_never_through_try_files() {
    let rendered = render_site(&a_php_site());

    assert!(
        rendered.contains("location = / {"),
        "the welcome route is nginx's exact match on the root:
{rendered}"
    );
    assert!(
        rendered.contains("error_page 403 404 = @mixengine_welcome;"),
        "both statuses the index module reports when it finds nothing:
{rendered}"
    );
    assert!(
        rendered.contains("location @mixengine_welcome"),
        "{rendered}"
    );
}

/// **The leak, asserted as an absence** — roadmap task **T124a**. This is the rendering T124
/// shipped and had to take back, and the failure it causes is a disclosure rather than a wrong
/// page, so it is worth a test of its own rather than a clause in the one above.
#[test]
fn no_try_files_in_this_rendering_ever_names_a_php_file() {
    let rendered = render_site(&a_php_site());

    let offending = rendered.lines().map(str::trim_start).find(|line| {
        line.starts_with("try_files") && line.contains(".php") && {
            // The front controller is the *last* element, which try_files reaches by internal
            // redirect — that one is correct and is what `location /` has always rendered.
            let items: Vec<_> = line
                .trim_end_matches(';')
                .split_whitespace()
                .skip(1)
                .collect();
            items
                .iter()
                .take(items.len().saturating_sub(1))
                .any(|item| item.contains(".php"))
        }
    });

    assert!(
        offending.is_none(),
        "a .php file before the last element of try_files is served in place, as text: {offending:?}"
    );
}

/// **An application's own error is still its own** — the T124 design, D3, and T167f.
///
/// The `error_page` for a 404 lives in an exact-match location on `/`, so no other path reaches
/// it. The php handler carries exactly one, for the 502 and 504 nginx produces when the pool
/// cannot be reached — the starting page — and never `fastcgi_intercept_errors`, which is what
/// would make an application's own 502 reach it too.
#[test]
fn the_error_page_is_scoped_to_the_root_and_not_to_the_php_handler() {
    let rendered = render_site(&a_php_site());

    let php_block = rendered
        .split(r"location ~ \.php$ {")
        .nth(1)
        .and_then(|rest| rest.split("\n    }").next())
        .expect("a php location");

    assert_eq!(
        php_block
            .lines()
            .filter(|line| line.trim_start().starts_with("error_page"))
            .count(),
        1,
        "the php handler answers with whatever the application said, except nginx's own 502:
{php_block}"
    );
    assert!(
        php_block.contains("error_page 502 504 = @mixengine_starting;"),
        "{php_block}"
    );
    assert!(
        rendered.contains("location @mixengine_starting {"),
        "the named location the error page names: {rendered}"
    );
    assert!(
        !rendered.contains("fastcgi_intercept_errors"),
        "this directive would capture an error the application sent:
{rendered}"
    );
}

/// **`alias` is refused inside a named location, and nginx refuses the whole file over it** —
/// roadmap task **T124a**, measured against nginx 1.31.3: *the "alias" directive cannot be used
/// inside the named location*. One page's mistake would take every site on the machine down
/// with it, which is why this is asserted here rather than left to the serving suite.
#[test]
fn the_welcome_location_is_served_with_root_and_never_with_alias() {
    for site in [a_static_site(), a_php_site()] {
        let rendered = render_site(&site);
        let named = rendered
            .split("location @mixengine_welcome {")
            .nth(1)
            .expect("the welcome location");
        let body = named
            .split(
                "
    }",
            )
            .next()
            .expect("its body");

        assert!(
            !body.contains("alias "),
            "nginx refuses this configuration outright:
{body}"
        );
        assert!(body.contains("root \""), "{body}");
        assert!(body.contains("try_files /blog.test.html =404;"), "{body}");
    }
}

/// A php-fpm site with three routes, as `served` hands them over — roadmap task **T135**.
fn a_site_with_routes() -> Served {
    use crate::generate::served::{Rewrite, ServedRoute, ServedRouteTarget};

    Served {
        routes: vec![
            ServedRoute {
                path: "/v1.0".to_owned(),
                target: ServedRouteTarget::Proxy {
                    address: "http://127.0.0.1:4000".to_owned(),
                    rewrite: None,
                },
            },
            ServedRoute {
                path: "/abc".to_owned(),
                target: ServedRouteTarget::Proxy {
                    address: "http://127.0.0.1:3003".to_owned(),
                    rewrite: Some(Rewrite {
                        regex: "^/abc(/.*)?$".to_owned(),
                        replacement: "/xyz$1".to_owned(),
                    }),
                },
            },
            ServedRoute {
                path: "/admin".to_owned(),
                target: ServedRouteTarget::PhpFpm {
                    upstream: Upstream::Tcp("127.0.0.1:9001".parse().expect("an address")),
                    activator: None,
                },
            },
        ],
        ..a_php_site()
    }
}

/// **Every route is rendered before the site's own `location ~ \.php$`** — roadmap task
/// **T135**.
///
/// nginx takes regex locations in the order it read them, so a route written after that
/// handler would never be reached for a `.php` under its prefix.
#[test]
fn a_route_is_rendered_before_the_php_handler() {
    let rendered = render_site(&a_site_with_routes());

    let route = rendered
        .find("location ~ ^/abc(/.*)?$")
        .expect("the route location");
    // The site's own handler is at the server block's own indentation; a route's nested one is
    // four spaces deeper, which is what tells the two apart in the text.
    let php = rendered
        .find("\n    location ~ \\.php$ {")
        .expect("the site's own php handler");

    assert!(
        route < php,
        "a route rendered after the php handler is a route nginx never reaches:\n{rendered}"
    );

    // **Where the site renders two blocks, both carry the routes.** A site whose `/api`
    // answered on HTTP and not on HTTPS would be a padlock that works everywhere except where
    // the application is — and the redirecting shape is the one that has two serving blocks to
    // get wrong.
    let redirecting = render_site(&Served {
        https_redirect: true,
        ..a_site_with_routes()
    });

    assert_eq!(
        redirecting.matches("location ~ ^/abc(/.*)?$").count(),
        1,
        "the block that only redirects carries no routes; the one that serves carries them all:
{redirecting}"
    );
    assert!(
        redirecting.contains("return 307 https://$host$request_uri;"),
        "{redirecting}"
    );
}

/// **A file-serving route is a pair of prefix locations with its PHP handler nested inside** —
/// roadmap task **T135**, that design's D4.
///
/// `^~` stops the sibling regex search, so a handler written beside this location would never
/// be reached and `try_files $uri` would answer `/admin/x.php` with its own source, as text —
/// T124a's leak, in a new place.
#[test]
fn a_php_route_nests_its_handler_inside_the_prefix_location() {
    let rendered = render_site(&a_site_with_routes());

    assert!(
        rendered.contains("location = /admin { return 301 /admin/; }"),
        "the exact match is the segment boundary:\n{rendered}"
    );
    assert!(rendered.contains("location ^~ /admin/ {"), "{rendered}");

    let prefix = rendered
        .find("location ^~ /admin/ {")
        .expect("the prefix location");
    let nested = rendered[prefix..]
        .find("fastcgi_pass 127.0.0.1:9001;")
        .expect("the route's own pool");
    let closed = rendered[prefix..]
        .find("\n    }")
        .expect("its closing brace");

    assert!(
        nested < closed,
        "the php handler is inside the prefix location, not beside it:\n{rendered}"
    );
    assert!(
        rendered.contains("try_files $uri $uri/ /admin/index.php?$query_string;"),
        "{rendered}"
    );
}

/// **A `.` in a path is escaped where it reaches a regex** — roadmap task **T135**, that
/// design's D5. Unescaped, `/v1.0` would also take `/v1X0`.
#[test]
fn a_dot_in_a_route_path_is_escaped() {
    let rendered = render_site(&a_site_with_routes());

    assert!(
        rendered.contains(r"location ~ ^/v1\.0(/.*)?$"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("location ~ ^/v1.0(/.*)?$"),
        "an unescaped dot matches any character:\n{rendered}"
    );
    assert!(
        rendered.contains("proxy_pass http://127.0.0.1:4000;"),
        "a route whose upstream has no path forwards it unchanged:\n{rendered}"
    );
    assert!(
        rendered.contains("rewrite ^/abc(/.*)?$ /xyz$1 break;"),
        "{rendered}"
    );
}
