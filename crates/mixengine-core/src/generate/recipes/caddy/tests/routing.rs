//! Pools that wake, routes, HTTPS and redirects.

use super::*;

/// **The three directives are one unit, and any two of them are worse than none** — T70, D2.
///
/// Measured against a real Caddy 2.10.0 before this was written, twenty requests each with the
/// pool's address dead and the activator's live:
///
/// | Rendering | 200s |
/// | --- | --- |
/// | both addresses, nothing else | 8 of 20 — Caddy load-balances between them |
/// | `+ lb_policy first`, `lb_try_duration` | 0 of 20, each burning the full budget |
/// | `+ fail_duration` | 20 of 20 |
///
/// So the bare two-address form is not a missing retry, it is a site sending half its traffic to
/// the activator while the pool is up and well; and `first` without passive health checking keeps
/// choosing the address that is already refusing until the budget runs out. This asserts all
/// three together because that is the only combination that works.
#[test]
fn a_pool_that_can_be_woken_renders_the_activator_after_it_under_a_policy_that_prefers_the_pool() {
    let rendered = render_site(&Served {
        shared: None,
        domains: vec!["php.test".to_owned()],
        routes: Vec::new(),
        doc_root: doc_root(),
        doc_root_relative: "public".to_owned(),
        kind: ServedKind::PhpFpm {
            upstream: Upstream::Tcp("127.0.0.1:9000".parse().expect("an address")),
            activator: Some(Upstream::Tcp("127.0.0.1:9500".parse().expect("an address"))),
        },
        https: false,
        https_redirect: false,
        certificate: None,
        steps: Vec::new(),
    });

    assert!(
        rendered.contains("php_fastcgi 127.0.0.1:9000 127.0.0.1:9500 {"),
        "the pool must come first — the fallback is what is tried when it refuses:\n{rendered}"
    );

    for directive in ["lb_policy first", "lb_try_duration", "fail_duration"] {
        assert!(
            rendered.contains(directive),
            "without `{directive}` the other two are worse than no fallback at all:\n{rendered}"
        );
    }
}

/// **A pool with no activator renders exactly what it rendered before T70.**
///
/// A home whose pool listens on TCP and whose row predates the `activation_port` column has no
/// activator, and the site it serves must be the site it served yesterday — a `lb_policy` over
/// one upstream would be a behaviour change bought for nothing.
#[test]
fn a_pool_with_no_activator_renders_the_one_line_it_always_did() {
    let rendered = render_site(&Served {
        shared: None,
        domains: vec!["php.test".to_owned()],
        routes: Vec::new(),
        doc_root: doc_root(),
        doc_root_relative: "public".to_owned(),
        kind: ServedKind::PhpFpm {
            upstream: Upstream::Tcp("127.0.0.1:9000".parse().expect("an address")),
            activator: None,
        },
        https: false,
        https_redirect: false,
        certificate: None,
        steps: Vec::new(),
    });

    assert!(
        rendered.contains("php_fastcgi 127.0.0.1:9000\n"),
        "a pool nothing can wake renders one address and no block:\n{rendered}"
    );
    assert!(
        !rendered.contains("lb_policy"),
        "there is nothing to choose between:\n{rendered}"
    );
}

/// One site through the real recipe.
///
/// `Caddy.sites` and not `served::render`, because that is the path `sites()` actually takes —
/// a mistake in how the rendering is assembled is caught here rather than only by the
/// integration suite.
/// **T167f: a PHP site answers a pool that is not answering with the starting page**, in both
/// the plaintext and the TLS block, and never with the empty-site welcome page.
#[test]
fn a_php_site_answers_a_dead_pool_with_the_starting_page() {
    let rendered = render_site(&Served {
        kind: ServedKind::PhpFpm {
            upstream: Upstream::Tcp("127.0.0.1:9000".parse().expect("an address")),
            activator: None,
        },
        ..a_site_with_a_certificate()
    });

    assert_eq!(
        rendered.matches("handle_errors 502 504 {").count(),
        2,
        "one handler per block, plaintext and TLS: {rendered}"
    );
    assert_eq!(
        rendered
            .matches("rewrite * /blog.test.starting.html")
            .count(),
        2,
        "{rendered}"
    );
}

/// Three routes and a fallback, as `served` hands them over — roadmap task **T135**.
fn a_site_with_routes() -> Served {
    use crate::generate::served::{Rewrite, ServedRoute, ServedRouteTarget};

    Served {
        routes: vec![
            ServedRoute {
                path: "/api/v1".to_owned(),
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
                path: "/assets".to_owned(),
                target: ServedRouteTarget::Static {
                    root: doc_root().join("dist"),
                },
            },
        ],
        ..a_site_with_a_certificate()
    }
}

/// **A route renders one handler, ahead of the fallback, in the order it was given** — roadmap
/// task **T135**.
///
/// The order is the specificity: `core::sites::by_specificity` sorted the list before it got
/// here, and `handle` blocks are taken in the order they are written. What this asserts is that
/// the rendering preserves it and puts the whole set before the site's own handler.
#[test]
fn routes_render_in_order_ahead_of_the_fallback() {
    let rendered = render_site(&a_site_with_routes());

    let first = rendered.find("@mixengine_route_0 path /api/v1 /api/v1/*");
    let second = rendered.find("@mixengine_route_1 path /abc /abc/*");
    let third = rendered.find("@mixengine_route_2 path /assets /assets/*");
    let fallback = rendered.find("@mixengine_welcome");

    assert!(first.is_some(), "the first route is missing:\n{rendered}");
    assert!(
        first < second && second < third,
        "routes keep the order they were sorted into:\n{rendered}"
    );
    assert!(
        third < fallback,
        "every route is written before what answers the rest of the site:\n{rendered}"
    );

    // **Both blocks**, because a site whose `/api` answered on HTTP and not on HTTPS would be a
    // padlock that works everywhere except where the application is.
    assert_eq!(
        rendered
            .matches("@mixengine_route_1 path /abc /abc/*")
            .count(),
        2,
        "the plaintext block and the TLS block both carry the routes:\n{rendered}"
    );
}

/// **The rewrite is one directive** — roadmap task **T135**, that design's D4.
///
/// `uri strip_prefix` + `rewrite` reads better and runs backwards: Caddy sorts directives into
/// its own standard order inside a block and `rewrite` comes *before* `uri`, so the pair would
/// hand the upstream `/xyz/abc/foo`. A route with no rewrite renders no `uri` line at all.
#[test]
fn a_rewriting_route_uses_one_directive() {
    let rendered = render_site(&a_site_with_routes());

    assert!(
        rendered.contains("uri path_regexp ^/abc(/.*)?$ /xyz$1"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("uri strip_prefix /abc"),
        "the two-directive form is the one that runs backwards:\n{rendered}"
    );
    assert!(
        rendered.contains("reverse_proxy http://127.0.0.1:4000"),
        "a route whose upstream has no path forwards it unchanged:\n{rendered}"
    );

    // A static route is the one target that *does* strip, because mounting a directory at a URL
    // is the whole of what it says.
    assert!(rendered.contains("uri strip_prefix /assets"), "{rendered}");
}

/// **A site whose own upstream carries a path renders a rewrite rather than a configuration
/// Caddy refuses** — roadmap task **T135**.
///
/// Measured against 2.11.4 before this was written: `reverse_proxy http://127.0.0.1:3003/xyz`
/// is *"for now, URLs for proxy upstreams only support scheme, host, and port components"*, and
/// the whole rendering is judged by one `caddy validate` — so before this, one site with a path
/// in its upstream cost **every** site on the machine its new configuration.
#[test]
fn a_proxy_site_with_a_path_in_its_upstream_renders_a_rewrite() {
    let rendered = render_site(&Served {
        kind: ServedKind::ReverseProxy {
            upstream: "http://127.0.0.1:3003".to_owned(),
            rewrite: Some(crate::generate::served::Rewrite {
                regex: "^(/.*)?$".to_owned(),
                replacement: "/xyz$1".to_owned(),
            }),
        },
        ..a_site_with_a_certificate()
    });

    assert!(
        rendered.contains("reverse_proxy http://127.0.0.1:3003"),
        "the address reaches the directive:\n{rendered}"
    );
    assert!(
        !rendered.contains("reverse_proxy http://127.0.0.1:3003/xyz"),
        "a path in an upstream is what Caddy refuses outright:\n{rendered}"
    );
    assert!(
        rendered.contains("uri path_regexp ^(/.*)?$ /xyz$1"),
        "the path it carried became a rewrite:\n{rendered}"
    );
}

/// A site with a certificate renders **two** blocks: one plaintext, one TLS — the T51 design,
/// D2. One block naming both schemes is refused by Caddy itself, which is why this asserts the
/// shape and not the directive alone.
#[test]
fn an_https_site_renders_a_plaintext_block_and_a_tls_block() {
    let rendered = render_site(&a_site_with_a_certificate());

    assert!(rendered.contains("http://blog.test"), "{rendered}");
    assert!(rendered.contains("https://blog.test"), "{rendered}");
    assert_eq!(rendered.matches("\n\ttls ").count(), 1, "{rendered}");
    assert_eq!(rendered.matches("\n\troot *").count(), 2, "{rendered}");
}

/// **The plaintext block redirects and serves nothing itself** — roadmap task **T98**. The TLS
/// block is untouched: it still carries the one `root *` a site with a certificate has always
/// had, which is the assertion that this is an addition to the plaintext block and not a second
/// change smuggled into the one T51 shipped.
#[test]
fn a_site_with_redirect_on_sends_its_plaintext_block_straight_to_https() {
    let rendered = render_site(&Served {
        https_redirect: true,
        ..a_site_with_a_certificate()
    });

    assert!(
        rendered.contains("redir https://{host}{uri} 307"),
        "{rendered}"
    );
    assert_eq!(
        rendered.matches("\n\troot *").count(),
        1,
        "only the TLS block serves a document root: {rendered}"
    );
}

/// **Off by default, and a site that never asked still renders exactly what T51 shipped** —
/// the regression this whole feature must not be.
#[test]
fn a_site_with_redirect_off_renders_the_same_plaintext_block_as_before() {
    let rendered = render_site(&a_site_with_a_certificate());

    assert!(!rendered.contains("redir "), "{rendered}");
    assert_eq!(rendered.matches("\n\troot *").count(), 2, "{rendered}");
}

/// **A redirect needs a usable certificate, not only the flag** — the T51 design's D4 applied a
/// second time. A site that asked for HTTPS but has nothing on disk to serve it with already
/// renders plaintext alone; asking for a redirect too must not turn that into a 307 into a TLS
/// listener nothing is bound to.
#[test]
fn a_site_with_redirect_on_but_no_certificate_renders_plaintext_exactly_as_before() {
    let mut site = a_site_with_a_certificate();
    site.https_redirect = true;
    site.certificate = None;

    let rendered = render_site(&site);

    assert!(!rendered.contains("redir "), "{rendered}");
    assert!(rendered.contains("file_server"), "{rendered}");
    assert_eq!(rendered.matches("\n\troot *").count(), 1, "{rendered}");
}

/// **The CA route is reachable over plaintext on a redirecting shared site, and the redirect is
/// what everything else gets** — roadmap task **T98**, the design's D3. A phone that has not
/// yet trusted this home's authority can only reach `/__mixengine/ca.crt` before that trust
/// exists; a redirect that caught it too would send the phone into a handshake TLS refuses, for
/// the one document that would have fixed that.
#[test]
fn a_redirecting_shared_site_still_serves_its_ca_route_over_plaintext() {
    let context = context("{}").with_authority(Some("-----BEGIN CERTIFICATE-----".to_owned()));
    let site = Served {
        https_redirect: true,
        certificate: Some(crate::generate::served::SiteCertificate {
            certificate: std::path::PathBuf::from(
                "/home/someone/.mixengine/certs/sites/blog.test.crt",
            ),
            key: std::path::PathBuf::from("/home/someone/.mixengine/certs/sites/blog.test.key"),
            fingerprint: "ab".repeat(32),
        }),
        ..a_shared_site([192, 168, 1, 10])
    };

    let rendered = Caddy.sites(&context, &[site]).expect("one site")[0]
        .contents()
        .to_owned();

    assert!(
        rendered.contains("handle /__mixengine/ca.crt {"),
        "{rendered}"
    );
    assert!(
        rendered.contains("redir https://{host}{uri} 307"),
        "{rendered}"
    );

    // The CA route's own `handle` comes before the generic one Caddy matches in order, so the
    // redirect is never reached for it — asserted on the order rather than trusted from it.
    let ca_route = rendered.find("/__mixengine/ca.crt").expect("the route");
    let redirect = rendered.find("redir ").expect("the redirect");
    assert!(ca_route < redirect, "{rendered}");
}

/// **A site with no certificate renders one block and no `tls`** — the T51 design, D4. A `tls`
/// at a path that is not there fails `caddy validate`, and validation judges the whole staged
/// rendering, so this one site would cost every other site its new configuration.
#[test]
fn a_site_with_no_certificate_renders_no_tls_at_all() {
    let mut site = a_site_with_a_certificate();
    site.certificate = None;

    let rendered = render_site(&site);

    assert!(rendered.contains("http://blog.test"), "{rendered}");
    assert!(!rendered.contains("https://"), "{rendered}");
    assert!(!rendered.contains("\n\ttls "), "{rendered}");
}

/// A site that never declared HTTPS renders exactly what it rendered before T51.
#[test]
fn a_plain_site_renders_one_block() {
    let mut site = a_site_with_a_certificate();
    site.https = false;
    site.certificate = None;

    let rendered = render_site(&site);

    assert_eq!(rendered.matches("\n\troot *").count(), 1, "{rendered}");
    assert!(!rendered.contains("\n\ttls "), "{rendered}");
}

/// **The fingerprint is what makes a reissue reload** — the T51 design, D5. T50 writes to the
/// same path, so without it the rendering after a reissue is byte-identical, `install` finds no
/// difference, and the server goes on serving the certificate it holds in memory.
#[test]
fn a_different_certificate_renders_a_different_file() {
    let site = a_site_with_a_certificate();
    let before = render_site(&site);

    let mut reissued = site.clone();
    reissued
        .certificate
        .as_mut()
        .expect("a certificate")
        .fingerprint = "cd".repeat(32);

    assert_ne!(
        before,
        render_site(&reissued),
        "a reissued certificate rendered the same bytes, so nothing would reload"
    );
}

/// **And the other half, which matters as much**: nothing changing renders identical bytes. A
/// rendering that had become unstable would reload the front end on every unrelated
/// `service.*` call.
#[test]
fn rendering_twice_with_nothing_changed_is_identical() {
    let site = a_site_with_a_certificate();

    assert_eq!(render_site(&site), render_site(&site));
}
