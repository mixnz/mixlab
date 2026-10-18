//! Site blocks: kinds, sockets, the authority, LAN sharing, extension fragments and the welcome page.

use super::*;

/// D8: the row keeps the port a browser asks for, and the file carries the port the process
/// must bind. On macOS those differ; on the other two they are the same number, and neither the
/// row nor this recipe knows which system it is on.
#[test]
fn the_global_block_carries_the_port_the_process_binds_and_the_row_keeps_the_one_it_answers() {
    let redirecting = vec![
        PortBinding {
            answer: 80,
            bind: 8080,
        },
        PortBinding {
            answer: 443,
            bind: 8443,
        },
    ];

    let redirected = context("{}").with_bindings(redirecting);
    let rendered = recipe::render(&Caddy, &redirected).expect("a Caddyfile")[0]
        .contents()
        .to_owned();

    assert!(rendered.contains("http_port 8080"), "{rendered}");
    assert!(rendered.contains("https_port 8443"), "{rendered}");
    assert_eq!(
        redirected.port(),
        Some(80),
        "the row still answers on 80, which is what LAN sharing and `mix site show` read"
    );

    let direct = context("{}");
    let plain = recipe::render(&Caddy, &direct).expect("a Caddyfile")[0]
        .contents()
        .to_owned();

    assert!(plain.contains("http_port 80"), "{plain}");
    assert!(plain.contains("https_port 443"), "{plain}");
}

/// One site per file, named after the primary domain — D12. The row's integer id was the
/// alternative: `etc/caddy/sites/7.caddy` tells whoever is reading the directory nothing, and
/// the directory is one of the first places somebody looks when a site does not answer.
#[test]
fn each_kind_renders_a_block_naming_what_it_was_given() {
    let served = vec![
        Served {
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
        },
        Served {
            shared: None,
            domains: vec!["php.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::PhpFpm {
                upstream: Upstream::Tcp("127.0.0.1:9000".parse().expect("an address")),
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

    let documents = Caddy
        .sites(&context("{}"), &served)
        .expect("four site files");

    // Four configurations, then the four welcome pages T124 appends after them, and the one
    // starting page the PHP site among them gets (T167f).
    assert_eq!(documents.len(), 9);
    assert!(
        documents
            .iter()
            .any(|document| document.relative()
                == Path::new("welcome").join("php.test.starting.html")),
        "the PHP site's starting page is written"
    );
    assert_eq!(
        documents[0].relative(),
        Path::new("sites").join("blog.test.caddy")
    );

    let statically = documents[0].contents();
    assert!(statically.contains("http://blog.test"), "{statically}");
    assert!(
        statically.contains("http://www.blog.test"),
        "an alias is not reachable: {statically}"
    );
    assert!(
        statically.contains("file_server"),
        "a static site serves files: {statically}"
    );

    let php = documents[1].contents();
    assert!(php.contains("php_fastcgi 127.0.0.1:9000"), "{php}");

    let proxy = documents[2].contents();
    assert!(
        proxy.contains("reverse_proxy http://127.0.0.1:4000"),
        "{proxy}"
    );

    // D7, asserted rather than described: a `node-app` is a reverse proxy to loopback and
    // nothing else. Nothing in this build starts `npm run dev`.
    let node = documents[3].contents();
    assert!(
        node.contains("reverse_proxy http://127.0.0.1:3000"),
        "{node}"
    );
}

/// A socket is spelled Caddy's way, which is why `Upstream` is a value and not a string: nginx
/// writes the same socket differently.
#[test]
fn a_pool_on_a_socket_is_spelled_the_way_caddy_spells_one() {
    let served = vec![Served {
        shared: None,
        domains: vec!["php.test".to_owned()],
        routes: Vec::new(),
        doc_root: doc_root(),
        doc_root_relative: "public".to_owned(),
        kind: ServedKind::PhpFpm {
            upstream: Upstream::Socket(std::path::PathBuf::from("/home/me/run/php-fpm-8.3.sock")),
            activator: None,
        },
        https: true,
        https_redirect: false,
        certificate: None,
        steps: Vec::new(),
    }];

    let rendered = Caddy.sites(&context("{}"), &served).expect("one site")[0]
        .contents()
        .to_owned();

    assert!(
        rendered.contains("php_fastcgi `unix//home/me/run/php-fpm-8.3.sock`"),
        "{rendered}"
    );
}

/// The socket sits under the home, and macOS's default home has a space in it. Bare, the
/// Caddyfile lexer splits the address at the space and the half after it is read as a network
/// called `Support` — measured as a 502 on every PHP request, with `dial support: unknown
/// network support` in the log. Backticks make it one token, activator included.
#[test]
fn a_socket_under_a_home_with_a_space_in_it_is_one_token() {
    let home = "/Users/me/Library/Application Support/MixEngine/run";
    let served = vec![Served {
        shared: None,
        domains: vec!["php.test".to_owned()],
        routes: Vec::new(),
        doc_root: doc_root(),
        doc_root_relative: "public".to_owned(),
        kind: ServedKind::PhpFpm {
            upstream: Upstream::Socket(std::path::PathBuf::from(format!(
                "{home}/php-fpm-8.3.sock"
            ))),
            activator: Some(Upstream::Socket(std::path::PathBuf::from(format!(
                "{home}/php-fpm-8.3.activate.sock"
            )))),
        },
        https: true,
        https_redirect: false,
        certificate: None,
        steps: Vec::new(),
    }];

    let rendered = Caddy.sites(&context("{}"), &served).expect("one site")[0]
        .contents()
        .to_owned();

    assert!(
        rendered.contains(&format!(
            "php_fastcgi `unix/{home}/php-fpm-8.3.sock` `unix/{home}/php-fpm-8.3.activate.sock` {{"
        )),
        "{rendered}"
    );
}

/// **The authority is rendered into a directory that holds only it** — the T75 design, D9.
///
/// `certs/ca/root.key` sits beside `certs/ca/root.crt`, so a front end pointed at the
/// certificates directory would serve this home's signing key to the local network. The copy is
/// what makes the served directory provably harmless, and this is where that is checked.
#[test]
fn the_front_end_renders_the_public_authority_and_nothing_beside_it() {
    let context = context("{}").with_authority(Some(
        "-----BEGIN CERTIFICATE-----
zz
"
        .to_owned(),
    ));

    let documents = Caddy.sites(&context, &[]).expect("the authority");

    let authority: Vec<_> = documents
        .iter()
        .filter(|document| document.relative().starts_with("public"))
        .collect();

    assert_eq!(authority.len(), 1, "{documents:?}");
    assert_eq!(
        authority[0].relative(),
        std::path::Path::new("public/ca.crt")
    );
    assert!(authority[0].contents().contains("BEGIN CERTIFICATE"));
    assert!(!authority[0].contents().contains("PRIVATE KEY"));
}

/// A home with no authority renders no file, rather than an empty one a phone would download
/// and fail to install.
#[test]
fn a_home_with_no_authority_renders_no_authority_file() {
    let documents = Caddy.sites(&context("{}"), &[]).expect("no sites");

    assert!(
        !documents
            .iter()
            .any(|document| document.relative().starts_with("public")),
        "{documents:?}"
    );
}

/// **The CA is downloadable from a shared site and from no other** — the T75 design, D9.
///
/// "Served only while sharing is on" is a property of the rendering, which is what makes it
/// something a test can hold rather than something a reviewer has to believe.
#[test]
fn a_shared_site_serves_the_authority() {
    let context = context("{}").with_authority(Some("-----BEGIN CERTIFICATE-----".to_owned()));

    let rendered = Caddy
        .sites(&context, &[a_shared_site([192, 168, 1, 10])])
        .expect("one site")[0]
        .contents()
        .to_owned();

    assert!(rendered.contains("/__mixengine/ca.crt"), "{rendered}");
    assert!(
        rendered.contains("application/x-x509-ca-cert"),
        "{rendered}"
    );
}

/// **The route never names the certificates directory**, which is where the signing key is.
///
/// A test rather than a comment, because the difference between safe and catastrophic here is
/// one word in a path.
#[test]
fn the_authority_route_never_names_the_certificates_directory() {
    let context = context("{}").with_authority(Some("-----BEGIN CERTIFICATE-----".to_owned()));

    let rendered = Caddy
        .sites(&context, &[a_shared_site([192, 168, 1, 10])])
        .expect("one site")[0]
        .contents()
        .to_owned();

    // **Directives only.** The comment above the route names `certs/ca/` in order to say why
    // it is not what the route points at, and a whole-file search would read that as the
    // opposite of what it says.
    let directives: String = rendered
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join(
            "
",
        );

    assert!(!directives.contains("root.key"), "{directives}");
    assert!(!directives.contains("certs"), "{directives}");
}

/// An unshared site beside a shared one serves nothing of the sort.
#[test]
fn an_unshared_site_serves_no_authority() {
    let context = context("{}").with_authority(Some("-----BEGIN CERTIFICATE-----".to_owned()));

    let rendered = Caddy
        .sites(
            &context,
            &[Served {
                shared: None,
                domains: vec!["shop.test".to_owned()],
                routes: Vec::new(),
                doc_root: doc_root(),
                doc_root_relative: "public".to_owned(),
                kind: ServedKind::Static,
                https: false,
                https_redirect: false,
                certificate: None,
                steps: Vec::new(),
            }],
        )
        .expect("one site")[0]
        .contents()
        .to_owned();

    assert!(!rendered.contains("__mixengine"), "{rendered}");
}

/// **Every address on one directive, loopback first** — the T74 design, D2. Caddy's `bind`
/// replaces the default rather than adding to it, so a block naming only the LAN address would
/// come up on the phone and go down in the browser on this machine.
#[test]
fn a_shared_site_binds_loopback_and_the_lan_address() {
    let served = vec![a_shared_site([192, 168, 1, 10])];

    let rendered = Caddy.sites(&context("{}"), &served).expect("one site")[0]
        .contents()
        .to_owned();

    assert!(
        rendered.contains("bind 127.0.0.1 ::1 192.168.1.10"),
        "{rendered}"
    );
}

/// **A shared site answers to its address by name as well as binding it.**
///
/// The bug a phone found. Binding the interface makes the connection arrive, and a site block
/// matches on `Host` — a phone sends the address it was handed, so without that address in the
/// block's own list Caddy matches no site and answers 200 with an empty body. Which is what the
/// URL `mix site share` had just printed did.
#[test]
fn a_shared_site_answers_to_the_address_a_phone_sends() {
    let served = vec![a_shared_site([192, 168, 1, 10])];

    let rendered = Caddy.sites(&context("{}"), &served).expect("one site")[0]
        .contents()
        .to_owned();

    assert!(
        rendered.contains("http://blog.test, http://192.168.1.10"),
        "{rendered}"
    );
}

/// **The name is in the block's address list, not only on the network** — the T75 design, D3.
///
/// T74 learned this with an address: binding an interface says where a connection is accepted,
/// and the block's address list says which site replies. A phone that resolves the name and is
/// answered 200 with an empty body is the slowest failure this feature has, and it is the one
/// this line prevents.
#[test]
fn a_shared_site_answers_to_its_mdns_name() {
    let served = vec![a_shared_site([192, 168, 1, 10])];

    let rendered = Caddy.sites(&context("{}"), &served).expect("one site")[0]
        .contents()
        .to_owned();

    assert!(
        rendered.contains("http://blog-mixengine.local"),
        "{rendered}"
    );
}

/// **Opt-in per site, and this is where it is actually enforced.**
///
/// Caddy's default is every interface, so an unshared site was already listening on the network
/// before T74 — it simply matched no `Host` anyone would send. T74 opens the port deliberately,
/// which turns that from an accident nobody could reach into a promise this rendering has to
/// keep: loopback, both families, and nothing else.
#[test]
fn a_site_that_is_not_shared_binds_loopback_only() {
    let served = vec![Served {
        shared: None,
        ..a_shared_site([192, 168, 1, 10])
    }];

    let rendered = Caddy.sites(&context("{}"), &served).expect("one site")[0]
        .contents()
        .to_owned();

    assert!(rendered.contains("bind 127.0.0.1 ::1"), "{rendered}");
    assert!(!rendered.contains("192.168.1.10"), "{rendered}");
}

/// A shared site with a certificate renders two blocks and both carry the addresses — a padlock
/// that worked everywhere except the device the site was shared for would be worse than none.
#[test]
fn both_blocks_of_a_shared_https_site_carry_the_addresses() {
    let served = vec![Served {
        shared: Some(Shared {
            address: [192, 168, 1, 10].into(),
            name: Some("blog-mixengine.local".to_owned()),
        }),
        ..a_site_with_a_certificate()
    }];

    let rendered = Caddy.sites(&context("{}"), &served).expect("one site")[0]
        .contents()
        .to_owned();

    assert!(
        rendered.contains("https://blog.test, https://192.168.1.10"),
        "{rendered}"
    );
    assert_eq!(
        rendered.matches("bind 127.0.0.1 ::1 192.168.1.10").count(),
        2,
        "{rendered}"
    );
}

/// **One file per extension, named after it, and headed like every other generated file** —
/// roadmap task **T81c**. The fragment arrives already substituted; what this adds is the
/// sentence saying where the file came from and where to change it.
#[test]
fn an_extension_s_fragment_is_a_file_of_its_own() {
    let context = context("{}").with_fragments(vec![FrontEndAddition {
        extension: "probe".to_owned(),
        fragment: "(probe) {\n\trespond 204\n}".to_owned(),
    }]);

    let documents = Caddy.fragments(&context).expect("the fragments render");

    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].relative(),
        Path::new("extensions").join("probe.caddy")
    );
    assert!(
        documents[0]
            .contents()
            .starts_with("# Generated by MixEngine"),
        "{}",
        documents[0].contents()
    );
    assert!(
        documents[0]
            .contents()
            .ends_with("(probe) {\n\trespond 204\n}\n"),
        "{}",
        documents[0].contents()
    );
}

/// The front end is the only recipe that sweeps, and it sweeps the three directories whose
/// contents follow a table: `sites/` the `sites` one, `extensions/` the `extensions` one, and
/// `welcome/` the `sites` one a second time — roadmap task **T124**. A deleted site that kept
/// its welcome page would be a page served for a site that no longer exists.
#[test]
fn the_front_end_sweeps_the_directories_that_follow_a_table() {
    assert_eq!(Caddy.swept(), &["sites", "extensions", "welcome"]);
}

/// **A dead upstream is answered, at every path** — the T124 design, D4. Wide is safe here in
/// a way it is not for the file kinds: a 502 the front end generated means nothing was
/// listening, so there is no application whose answer is being overwritten.
#[test]
fn a_proxy_site_answers_a_dead_upstream_with_the_welcome_page() {
    let rendered = render_site(&Served {
        kind: ServedKind::ReverseProxy {
            upstream: "http://127.0.0.1:8000".to_owned(),
            rewrite: None,
        },
        ..a_site_with_a_certificate()
    });

    assert_eq!(
        rendered.matches("handle_errors 502 504 {").count(),
        2,
        "one handler per block, plaintext and TLS:
{rendered}"
    );
    assert!(rendered.contains("/blog.test.html"), "{rendered}");
    assert!(
        rendered.contains("header Cache-Control \"no-store\""),
        "{rendered}"
    );
}

/// **And `handle /` is not how a proxy site does it.** The root-only rule belongs to the kinds
/// that serve files; a proxy site with a working upstream serves `/` from that upstream, and a
/// handler in front of it would take the site's own home page away.
#[test]
fn a_proxy_site_has_no_root_only_welcome_handler() {
    let rendered = render_site(&Served {
        kind: ServedKind::NodeApp { port: 3000 },
        ..a_site_with_a_certificate()
    });

    assert!(
        !rendered.contains("handle @mixengine_welcome {"),
        "a proxy site's `/` belongs to its upstream:
{rendered}"
    );
}

/// **Off renders neither the page nor the route** — the T124 design, D6. Either half alone is
/// worse than neither: a page nothing routes to is a file nobody reads, and a route with no
/// page behind it is a 404 on top of the 404 this feature exists to replace.
#[test]
fn the_switch_turned_off_renders_no_welcome_at_all() {
    let documents = Caddy
        .sites(
            &context("{}").with_welcome(false),
            &[a_site_with_a_certificate()],
        )
        .expect("a rendering");

    assert_eq!(documents.len(), 1, "only the site's own configuration");
    assert!(
        !documents[0].contents().contains("@mixengine_welcome"),
        "{}",
        documents[0].contents()
    );
}

/// A static site at `blog.test` with a certificate — roadmap task **T51**.
///
/// Paths that do not exist, deliberately: this module renders text and never reads a disk.
/// Whether the pair is there was decided in `generate::served`, and asking again here would be a
/// second answer to one question.
/// **Two documents per site now** — roadmap task T124: the site's configuration, and the page
/// it answers with when it has nothing to serve.
#[test]
fn a_site_renders_a_welcome_page_beside_its_configuration() {
    let documents = Caddy
        .sites(&context("{}"), &[a_site_with_a_certificate()])
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
        names.contains(&"sites/blog.test.caddy".to_owned()),
        "{names:?}"
    );
    assert!(
        names.contains(&"welcome/blog.test.html".to_owned()),
        "{names:?}"
    );
}

/// **A site's own configuration stays first** — the authority below depends on `documents[n]`
/// meaning the nth site and says so, so the pages are appended after every site rather than
/// interleaved with them.
#[test]
fn the_welcome_pages_come_after_every_site() {
    let documents = Caddy
        .sites(
            &context("{}"),
            &[a_site_with_a_certificate(), a_shared_site([192, 0, 2, 10])],
        )
        .expect("a rendering");

    for (position, document) in documents.iter().take(2).enumerate() {
        assert!(
            document.relative().starts_with("sites"),
            "document {position} is not a site file: {:?}",
            document.relative()
        );
    }
}

/// **The root path only, and after the site's own handlers** — the T124 design, D3. A
/// catch-all error handler would replace an application's own 404, which is MixEngine lying
/// about somebody else's program.
#[test]
fn a_static_site_falls_back_to_the_welcome_page_only_at_the_root() {
    let rendered = render_site(&a_site_with_a_certificate());

    assert!(
        rendered.contains("handle @mixengine_welcome {"),
        "the welcome route must match one exact path:
{rendered}"
    );
    assert!(
        !rendered.contains("handle_errors 502"),
        "a file-serving site never replaces an application's own error:
{rendered}"
    );
    assert!(rendered.contains("/blog.test.html"), "{rendered}");
    assert!(
        rendered.contains("header Cache-Control \"no-store\""),
        "without no-store a cached welcome page outlives the index that replaced it:
{rendered}"
    );

    // **The condition is the matcher's, not the ordering's** — measured against Caddy 2.11.4:
    // `file_server` and `php_fastcgi` are not in the mutually exclusive group `handle` blocks
    // form, so a route placed after them is never reached. `not file` is what keeps a site with
    // its own index from ever rendering this page.
    assert!(
        rendered.contains("not file {"),
        "the welcome route must ask the disk:
{rendered}"
    );
    assert!(
        rendered.contains("try_files index.html index.htm"),
        "and it must ask about the index files this kind would have served:
{rendered}"
    );
}

/// **Both blocks carry it.** A site with a certificate renders two — plaintext and TLS, the T51
/// design's D2 — and the one that was missed is the one whichever scheme the developer typed
/// happens to reach.
#[test]
fn both_blocks_carry_the_welcome_route() {
    let rendered = render_site(&a_site_with_a_certificate());

    assert_eq!(
        rendered.matches("handle @mixengine_welcome {").count(),
        2,
        "one route per block, plaintext and TLS:
{rendered}"
    );
}
