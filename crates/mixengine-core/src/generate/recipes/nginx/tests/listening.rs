//! How a site is reached: pools, the authority, LAN sharing and TLS.

use super::*;

/// A static site at `blog.test` with a certificate.
///
/// Paths that do not exist, deliberately: this module renders text and never reads a disk.
/// Whether the pair is there was decided in `generate::served`.
fn a_site_with_a_certificate() -> Served {
    Served {
        shared: None,
        domains: vec!["blog.test".to_owned()],
        routes: Vec::new(),
        doc_root: doc_root(),
        doc_root_relative: "public".to_owned(),
        kind: ServedKind::Static,
        https: true,
        https_redirect: false,
        certificate: Some(crate::generate::served::SiteCertificate {
            certificate: PathBuf::from("/home/someone/.mixengine/certs/sites/blog.test.crt"),
            key: PathBuf::from("/home/someone/.mixengine/certs/sites/blog.test.key"),
            fingerprint: "ab".repeat(32),
        }),
        steps: Vec::new(),
    }
}

/// **nginx says in one directive what Caddy needs three for.**
///
/// `backup` in an `upstream` group *is* "only when the others have refused", so there is no
/// policy to state and no load balancing to switch off. Measured against a real nginx 1.24.0
/// with the pool's address dead: 200 on the first request, in 7.9 ms. What has to be right here
/// is the shape — a group, the pool plain, the activator marked `backup`, and `fastcgi_pass`
/// pointing at the group rather than at either address.
#[test]
fn a_pool_that_can_be_woken_renders_a_group_whose_second_server_is_a_backup() {
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
        rendered.contains("server 127.0.0.1:9000;"),
        "the pool is the ordinary member of the group:\n{rendered}"
    );
    assert!(
        rendered.contains("server 127.0.0.1:9500 backup;"),
        "the activator is reached only once the pool has refused:\n{rendered}"
    );
    assert!(
        !rendered.contains("fastcgi_pass 127.0.0.1:9000;"),
        "passing straight to the pool bypasses the group that makes the fallback work:\n\
             {rendered}"
    );
}

/// **Two sites sharing one pool must not declare one `upstream` name twice.**
///
/// nginx refuses a configuration with a duplicate upstream name outright, so the group is named
/// after the *site* and not after the pool — and this is what would catch a name derived from
/// the pool instead. The whole front end fails to start when it is wrong, not the one site.
#[test]
fn two_sites_on_one_pool_declare_two_differently_named_groups() {
    let pool = Upstream::Tcp("127.0.0.1:9000".parse().expect("an address"));
    let activator = Some(Upstream::Tcp("127.0.0.1:9500".parse().expect("an address")));

    let served: Vec<Served> = ["one.test", "two.test"]
        .into_iter()
        .map(|domain| Served {
            shared: None,
            domains: vec![domain.to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::PhpFpm {
                upstream: pool.clone(),
                activator: activator.clone(),
            },
            https: false,
            https_redirect: false,
            certificate: None,
            steps: Vec::new(),
        })
        .collect();

    let documents = Nginx
        .sites(&context("{}"), &served)
        .expect("two site files");

    let names: Vec<String> = documents
        .iter()
        // Site configurations only: a welcome page is rendered beside each of them,
        // and an HTML file declares no `upstream` group.
        .filter(|document| document.relative().starts_with(SITES))
        .map(|document| {
            // The *directive* and not the word: this file explains itself in prose that says
            // "upstream" too, and a search that found the comment would compare two identical
            // sentences and pass whatever the names were.
            document
                .contents()
                .lines()
                .find(|line| line.starts_with("upstream "))
                .expect("a group at the top level")
                .trim_end_matches(" {")
                .to_owned()
        })
        .collect();

    assert_ne!(
        names[0], names[1],
        "nginx refuses a duplicate upstream name and the whole front end fails to start"
    );
}

/// A pool with no activator renders what it rendered before on-demand activation: a `fastcgi_pass`
/// straight at the pool, and no group at all.
#[test]
fn a_pool_with_no_activator_is_passed_to_directly() {
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
        rendered.contains("fastcgi_pass 127.0.0.1:9000;"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("upstream "),
        "a group of one is a group for nothing:\n{rendered}"
    );
}

/// **The authority is rendered into a directory that holds only it.**
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

    let documents = Nginx.sites(&context, &[]).expect("the authority");

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
    let documents = Nginx.sites(&context("{}"), &[]).expect("no sites");

    assert!(
        !documents
            .iter()
            .any(|document| document.relative().starts_with("public")),
        "{documents:?}"
    );
}

/// **The CA is downloadable from a shared site and from no other.**
///
/// "Served only while sharing is on" is a property of the rendering, which is what makes it
/// something a test can hold rather than something a reviewer has to believe.
#[test]
fn a_shared_site_serves_the_authority() {
    let context = context("{}").with_authority(Some("-----BEGIN CERTIFICATE-----".to_owned()));

    let rendered = Nginx
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

    let rendered = Nginx
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

    let rendered = Nginx
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

/// A site shared on the LAN, without a certificate.
fn a_shared_site(address: [u8; 4]) -> Served {
    Served {
        shared: Some(Shared {
            address: address.into(),
            name: Some("blog-mixengine.local".to_owned()),
        }),
        domains: vec!["blog.test".to_owned()],
        routes: Vec::new(),
        doc_root: doc_root(),
        doc_root_relative: "public".to_owned(),
        kind: ServedKind::Static,
        https: false,
        https_redirect: false,
        certificate: None,
        steps: Vec::new(),
    }
}

/// **A second `listen`, not a changed one.** The browser on this machine and the phone are looking
/// at the same site at the same time, so loopback stays.
#[test]
fn a_shared_site_listens_on_loopback_and_the_lan_address() {
    let rendered = render_site(&a_shared_site([192, 168, 1, 10]));

    assert!(rendered.contains("listen 127.0.0.1:80;"), "{rendered}");
    assert!(rendered.contains("listen 192.168.1.10:80;"), "{rendered}");
    assert_eq!(
        rendered
            .matches(
                "
    listen "
            )
            .count(),
        2,
        "{rendered}"
    );
}

/// **The name joins `server_name`.** nginx picks a listener group by address and only then consults
/// the names, so this is the half that decides which site replies to `blog-mixengine.local` once
/// the responder has said where it resolves.
#[test]
fn a_shared_site_names_itself_in_server_name() {
    let rendered = render_site(&a_shared_site([192, 168, 1, 10]));

    assert!(
        rendered.contains("server_name blog.test blog-mixengine.local;"),
        "{rendered}"
    );
}

/// **An unshared site carries no name at all**, which is what makes "opt-in per site" a
/// property of the rendering rather than a promise made about it.
#[test]
fn an_unshared_site_carries_no_mdns_name() {
    let rendered = render_site(&Served {
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
    });

    // The directive rather than the whole file: the comment above it names the shape a
    // shared site's name takes, and a comment is not a name this site answers to.
    assert!(rendered.contains("server_name shop.test;"), "{rendered}");
}

/// **Two sites shared on one address** — the assumption LAN sharing recorded and could not yet
/// reach.
///
/// nginx groups servers by listen address before it consults `server_name`, so a request
/// carrying `Host: <ip>` is answered by that group's *default* — the first server block on the
/// address — while each site's own mDNS name is matched by name. Asserted so that a later
/// change cannot quietly move which site an address-shaped `Host` reaches.
#[test]
fn two_sites_shared_on_one_address_are_told_apart_by_name() {
    let shop = Served {
        shared: Some(Shared {
            address: [192, 168, 1, 10].into(),
            name: Some("shop-mixengine.local".to_owned()),
        }),
        domains: vec!["shop.test".to_owned()],
        routes: Vec::new(),
        doc_root: doc_root(),
        doc_root_relative: "public".to_owned(),
        kind: ServedKind::Static,
        https: false,
        https_redirect: false,
        certificate: None,
        steps: Vec::new(),
    };

    let rendered = Nginx
        .sites(&context("{}"), &[a_shared_site([192, 168, 1, 10]), shop])
        .expect("two site files");

    assert!(
        rendered[0].contents().contains("blog-mixengine.local"),
        "{}",
        rendered[0].contents()
    );
    assert!(
        rendered[1].contents().contains("shop-mixengine.local"),
        "{}",
        rendered[1].contents()
    );

    // Both on the one address, which is what makes the group's default the question it is.
    assert_eq!(
        rendered
            .iter()
            .filter(|document| document.contents().contains("listen 192.168.1.10:80;"))
            .count(),
        2
    );
}

/// Sharing is opt-in per site: the site beside it is untouched.
#[test]
fn an_unshared_site_listens_once_per_scheme() {
    let rendered = render_site(&Served {
        shared: None,
        ..a_shared_site([192, 168, 1, 10])
    });

    assert_eq!(
        rendered
            .matches(
                "
    listen "
            )
            .count(),
        1,
        "{rendered}"
    );
    assert!(!rendered.contains("192.168.1.10"), "{rendered}");
}

/// A shared HTTPS site listens on the LAN address over TLS as well, because the certificate
/// covers that address as an IP SAN.
#[test]
fn a_shared_https_site_offers_tls_on_the_lan_address_too() {
    let rendered = render_site(&Served {
        shared: Some(Shared {
            address: [192, 168, 1, 10].into(),
            name: Some("blog-mixengine.local".to_owned()),
        }),
        ..a_site_with_a_certificate()
    });

    assert_eq!(
        rendered
            .matches(
                "
    listen "
            )
            .count(),
        4,
        "{rendered}"
    );
    assert_eq!(
        rendered.matches("192.168.1.10").count(),
        2,
        "one plaintext listener and one TLS one:
{rendered}"
    );
}

/// **The consequence of D2 and D3, asserted rather than discovered.**
///
/// nginx groups servers by listen address before it consults `server_name`, so the LAN address
/// has exactly one server block in its group: a request arriving from the network with another
/// site's `Host` is answered by the shared site as that group's default, not by the site it
/// named. That is the intended outcome — no unshared site is served over the LAN — and this
/// test exists so a later change cannot quietly turn it into the other one.
#[test]
fn the_lan_address_belongs_to_the_shared_site_alone() {
    let shared = a_shared_site([192, 168, 1, 10]);
    let other = Served {
        shared: None,
        domains: vec!["shop.test".to_owned()],
        ..a_shared_site([192, 168, 1, 10])
    };

    let documents = Nginx
        .sites(&context("{}"), &[shared, other])
        .expect("two site files");

    // One file per site, so the question is which files carry the address rather than which
    // blocks do — and the answer has to be exactly the shared one.
    let carrying: Vec<&str> = documents
        .iter()
        .map(|document| document.contents())
        .filter(|contents| contents.contains("192.168.1.10"))
        .collect();

    assert_eq!(carrying.len(), 1, "{documents:?}");
    assert!(carrying[0].contains("blog.test"), "{}", carrying[0]);
    assert!(!carrying[0].contains("shop.test"), "{}", carrying[0]);
}

/// An HTTPS site listens twice and names its certificate.
///
/// **One `server` block and not two, unlike Caddy**, and that was measured rather than assumed:
/// nginx 1.24 answers `syntax is ok` to a block carrying a plaintext listener and a TLS one,
/// where Caddy refuses the equivalent outright. TLS attaches to a listener here and to a site
/// block there.
#[test]
fn an_https_site_listens_on_tls_and_names_its_certificate() {
    let rendered = render_site(&a_site_with_a_certificate());

    assert_eq!(rendered.matches("\n    listen ").count(), 2, "{rendered}");
    assert!(rendered.contains(" ssl;"), "{rendered}");
    assert!(
        rendered
            .contains("ssl_certificate \"/home/someone/.mixengine/certs/sites/blog.test.crt\";"),
        "{rendered}"
    );
    assert!(
        rendered.contains(
            "ssl_certificate_key \"/home/someone/.mixengine/certs/sites/blog.test.key\";"
        ),
        "{rendered}"
    );
    assert_eq!(rendered.matches("server {").count(), 1, "{rendered}");
}

/// **Two `server` blocks, where a site that does not redirect needs only one.** The block that
/// answers on the plaintext listener returns 307 and carries none of the site's own content;
/// everything `an_https_site_listens_on_tls_and_names_its_certificate` asserts about a single-block
/// HTTPS site is still true of the *second* block here.
#[test]
fn a_site_with_redirect_on_renders_a_redirecting_block_and_a_serving_block() {
    let rendered = render_site(&Served {
        https_redirect: true,
        ..a_site_with_a_certificate()
    });

    assert_eq!(rendered.matches("server {").count(), 2, "{rendered}");
    assert!(
        rendered.contains("return 307 https://$host$request_uri;"),
        "{rendered}"
    );
    assert_eq!(
        rendered.matches("try_files $uri $uri/ =404;").count(),
        1,
        "only the serving block has the site's own content: {rendered}"
    );
    assert!(
        rendered
            .contains("ssl_certificate \"/home/someone/.mixengine/certs/sites/blog.test.crt\";"),
        "{rendered}"
    );
}

/// **Off by default, and a site that never asked still renders the one block it always did** —
/// the regression this whole feature must not be.
#[test]
fn a_site_with_redirect_off_renders_one_server_block_as_before() {
    let rendered = render_site(&a_site_with_a_certificate());

    assert_eq!(rendered.matches("server {").count(), 1, "{rendered}");
    assert!(!rendered.contains("return 307"), "{rendered}");
}

/// **A redirect needs a usable certificate, not only the flag** — the rule that a site with no
/// usable certificate renders no TLS, applied a second time, on nginx's own shape: a site that
/// asked for HTTPS but has nothing on disk to serve it with already renders one plaintext listener;
/// asking for a redirect too must not turn that into a 307 toward a TLS listener nothing is bound
/// to.
#[test]
fn a_site_with_redirect_on_but_no_certificate_renders_one_block_as_before() {
    let mut site = a_site_with_a_certificate();
    site.https_redirect = true;
    site.certificate = None;

    let rendered = render_site(&site);

    assert_eq!(rendered.matches("server {").count(), 1, "{rendered}");
    assert!(!rendered.contains("return 307"), "{rendered}");
    assert!(
        rendered.contains("try_files $uri $uri/ =404;"),
        "{rendered}"
    );
}

/// **The CA route is reachable over plaintext on a redirecting shared site**, on nginx's own shape:
/// the route lives in the redirecting block beside the `location /` that returns 307, and nowhere
/// in the serving block behind it — a phone that has not yet trusted this home's authority cannot
/// reach that block at all.
#[test]
fn a_redirecting_shared_site_still_serves_its_ca_route_over_plaintext() {
    let context = context("{}").with_authority(Some("-----BEGIN CERTIFICATE-----".to_owned()));
    let site = Served {
        https_redirect: true,
        certificate: Some(crate::generate::served::SiteCertificate {
            certificate: PathBuf::from("/home/someone/.mixengine/certs/sites/blog.test.crt"),
            key: PathBuf::from("/home/someone/.mixengine/certs/sites/blog.test.key"),
            fingerprint: "ab".repeat(32),
        }),
        ..a_shared_site([192, 168, 1, 10])
    };

    let rendered = Nginx.sites(&context, &[site]).expect("one site")[0]
        .contents()
        .to_owned();

    assert!(
        rendered.contains("location = /__mixengine/ca.crt {"),
        "{rendered}"
    );
    assert!(
        rendered.contains("return 307 https://$host$request_uri;"),
        "{rendered}"
    );

    // The CA route has to sit in the *redirecting* block, which a plaintext request actually
    // reaches — not in the serving block behind the TLS listener a phone cannot get to yet.
    let ca_route = rendered.find("/__mixengine/ca.crt").expect("the route");
    let second_server = rendered.rfind("server {").expect("the second block");
    assert!(ca_route < second_server, "{rendered}");
}

/// A site with no certificate listens once and names none.
#[test]
fn a_site_with_no_certificate_has_one_listener() {
    let mut site = a_site_with_a_certificate();
    site.certificate = None;

    let rendered = render_site(&site);

    assert_eq!(rendered.matches("\n    listen ").count(), 1, "{rendered}");
    assert!(!rendered.contains("ssl"), "{rendered}");
}

/// **The fingerprint is what makes a reissue reload.**
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

/// And nothing changing renders identical bytes — a rendering that had become unstable would
/// reload the front end on every unrelated `service.*` call.
#[test]
fn rendering_twice_with_nothing_changed_is_identical() {
    let site = a_site_with_a_certificate();

    assert_eq!(render_site(&site), render_site(&site));
}
