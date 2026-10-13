//! `mix site`.

use super::time::in_time;
use mixengine_proto::{
    ServiceId, ServiceList, SiteDetail, SiteKind, SiteList, SiteOwner, SiteRemoval, SiteSharing,
};
use std::time::SystemTime;

/// The services `mix site show` cannot start for this site, and why — roadmap task **T200b**, D6.
///
/// The site's pool and every service it is linked to, read against the listing. An empty string
/// when nothing it needs has failed, so the caller can append it whatever it holds.
pub(crate) fn site_failures(detail: &SiteDetail, services: &ServiceList) -> String {
    let pool = detail
        .pool
        .as_ref()
        .and_then(|pool| pool.resolved.as_ref().or(pool.declared.as_ref()));
    let needed = pool
        .into_iter()
        .chain(detail.services.iter().map(|link| &link.service));

    let mut out = String::new();

    for id in needed {
        let note = services
            .services
            .iter()
            .find(|service| &service.id == id)
            .and_then(|service| service.last_failure.as_ref());

        if let Some(note) = note {
            out.push_str(&format!(
                "{id} could not start: {}
",
                note.detail
            ));
        }
    }

    out
}

/// `mix site list` — every site, and what serves it.
pub(crate) fn site_list(list: &SiteList) -> String {
    if list.sites.is_empty() {
        return "no sites are declared; `mix site create` adds one\n".to_owned();
    }

    // **A count and not the routes themselves** — roadmap task **T135**. What a listing is for is
    // "does this site have more behind it"; which prefix answers what is `mix site show`'s, where
    // there is room to print the address beside it.
    let mut out = format!(
        "{:<28}  {:<14}  {:<7}  {:<9}  {}\n",
        "DOMAIN", "KIND", "ROUTES", "STATE", "OWNER"
    );

    for site in &list.sites {
        let routes = match site.routes.len() {
            0 => "—".to_owned(),
            count => count.to_string(),
        };

        out.push_str(&format!(
            "{:<28}  {:<14}  {routes:<7}  {:<9}  {}\n",
            site.domain,
            kind_word(&site.kind),
            site.state.as_str(),
            owner_word(&site.owner)
        ));
    }

    out
}

/// Who a site belongs to, as a column reads it — roadmap task **T81b**.
fn owner_word(owner: &SiteOwner) -> String {
    match owner {
        SiteOwner::Project { name } => name.clone(),
        SiteOwner::Extension { id } => format!("extension {id}"),
    }
}

/// What a route's target is printed as — roadmap task **T135**.
///
/// The target *and* its address, because a path on its own answers nothing a person came to find
/// out: `/api  →  http://127.0.0.1:3003/xyz` is the line, and the arrow is what makes the direction
/// readable at a glance.
fn route_target_word(target: &mixengine_proto::RouteTarget) -> String {
    match target {
        mixengine_proto::RouteTarget::Proxy { upstream } => format!("→ {upstream}"),
        mixengine_proto::RouteTarget::PhpFpm { pool } => format!(
            "php-fpm {}",
            pool.as_ref()
                .map_or("(the service it named is gone)", ServiceId::as_str)
        ),
        mixengine_proto::RouteTarget::Static { root } => format!("files in {root}"),
    }
}

/// The word a person typed for a kind, which is the word the wire uses.
fn kind_word(kind: &SiteKind) -> &'static str {
    match kind {
        SiteKind::PhpFpm { .. } => "php-fpm",
        SiteKind::Static => "static",
        SiteKind::ReverseProxy { .. } => "reverse-proxy",
        SiteKind::NodeApp { .. } => "node-app",
    }
}

/// The `https` line's value, with the redirect folded in rather than given a row of its own —
/// roadmap task **T98**. A redirect is never reachable without HTTPS already being on, so it is not
/// a fact worth asking a reader to check two lines to get.
fn https_word(https: bool, redirect: bool) -> String {
    match (https, redirect) {
        (true, true) => "yes (redirect)".to_owned(),
        (true, false) => "yes".to_owned(),
        (false, _) => "no".to_owned(),
    }
}

/// `mix site show` — one site, and the two answers about its pool.
///
/// The **pool** lines are the whole value of the rendering: a site's pool is frozen at create while
/// the shell in the same directory keeps following the default, so somebody looking at a PHP
/// version they did not expect is looking for which of the two is in charge.
pub(crate) fn site_detail(detail: &SiteDetail) -> String {
    let mut out = format!(
        "{}\n  owner     {}\n  kind      {}\n  root      {}\n  doc root  {}{}\n  https     {}\n  \
         state     {}\n",
        detail.site.domain,
        owner_word(&detail.site.owner),
        kind_word(&detail.site.kind),
        detail.root,
        detail.doc_root_full,
        if detail.doc_root_exists {
            ""
        } else {
            "  (not there yet)"
        },
        https_word(detail.site.https, detail.site.https_redirect),
        detail.site.state.as_str()
    );

    match &detail.site.kind {
        SiteKind::ReverseProxy { upstream } => {
            out.push_str(&format!("  upstream  {upstream}\n"));
        }
        SiteKind::NodeApp { port } => out.push_str(&format!("  port      {port}\n")),
        SiteKind::PhpFpm { .. } | SiteKind::Static => {}
    }

    if let Some(pool) = &detail.pool {
        out.push_str(&format!(
            "  pool      {}\n",
            pool.declared
                .as_ref()
                .map_or("(the service it named is gone)", ServiceId::as_str)
        ));

        if pool.declared != pool.resolved {
            out.push_str(&format!(
                "  resolves  {}; this directory resolves to a different PHP than the site was \
                 declared with\n",
                pool.resolved.as_ref().map_or("—", ServiceId::as_str)
            ));
        }
    }

    if let Some(sharing) = &detail.site.sharing {
        out.push_str(&format!(
            "  shared    {} on {}\n",
            sharing.url, sharing.interface
        ));

        if let Some(until) = sharing.until {
            out.push_str(&format!(
                "  ends      {}\n",
                in_time(until, SystemTime::now())
            ));
        }
    }

    if detail.domains.len() > 1 {
        out.push_str(&format!("\naliases: {}\n", detail.domains[1..].join(", ")));
    }

    // **In match order, which is what the daemon answers with** — roadmap task **T135**. Not the
    // order somebody typed: the front end resolves an overlap by specificity, and a listing showing
    // declaration order would be showing something no server does.
    if !detail.site.routes.is_empty() {
        out.push_str(&format!("\n{:<24}  {}\n", "PATH", "ANSWERED BY"));

        for route in &detail.site.routes {
            out.push_str(&format!(
                "{:<24}  {}\n",
                route.path,
                route_target_word(&route.target)
            ));
        }
    }

    if !detail.services.is_empty() {
        out.push_str(&format!("\n{:<24}  {}\n", "SERVICE", "STATE"));

        for link in &detail.services {
            out.push_str(&format!(
                "{:<24}  {}\n",
                link.service.as_str(),
                link.state.as_str()
            ));
        }
    }

    out
}

/// `mix site delete` — what was freed, and what was not touched.
pub(crate) fn site_removal(removal: &SiteRemoval) -> String {
    let mut out = format!("{} is no longer declared\n", removal.removed.domain);

    out.push_str(&format!(
        "  the files are kept:    {}\n",
        removal.doc_root_kept
    ));
    out.push_str(&format!(
        "  free for another site: {}\n",
        removal.domains_released.join(", ")
    ));

    out
}

/// What `mix site share` prints: where the site is, and a code a phone can point at.
///
/// **The QR is drawn here and the URL is answered by the daemon** - the T74 design, D10. A terminal
/// is one client's rendering of one string; a graphical client draws its own from the same string,
/// and the daemon knows about neither.
pub(crate) fn site_shared(sharing: &SiteSharing) -> String {
    let mut out = format!(
        "shared on the local network\n  url        {}\n  interface  {} ({})\n",
        sharing.url, sharing.interface, sharing.address
    );

    // **The name is printed, and whether anything answers for it is printed beside it** - roadmap
    // task T75. A name that resolves nowhere looks exactly like one that does until somebody types
    // it into a phone, so a home that could not bind UDP 5353 says so here rather than there.
    if let Some(name) = sharing.name.as_deref() {
        match sharing.advertised {
            true => out.push_str(&format!("  name       {name}\n")),
            false => out.push_str(&format!(
                "  name       {name} (not being advertised: this home could not answer mDNS)\n"
            )),
        }
    }

    out.push_str(&format!("  authority  {}\n", sharing.ca_url));

    // **Printed only when there is one** — roadmap task T76. A share with no deadline is the
    // ordinary case, and a line reading "ends  never" is a line every reader has to skip.
    if let Some(until) = sharing.until {
        out.push_str(&format!(
            "  ends       {}\n",
            in_time(until, SystemTime::now())
        ));
    }

    // **The QR carries the address and not the name** - the T75 design, D11. Android's resolver
    // does not answer `.local` for a browser, so a code carrying the name would be a broken URL for
    // a large share of the phones this feature exists for.
    if let Some(code) = qr(&sharing.url) {
        out.push('\n');
        out.push_str(&code);
    }

    out.push_str(
        "\nover http: a phone does not trust this home's certificate authority until it has \
         installed it. Open the authority URL on the device, then, on iOS, turn it on under \
         Settings > General > About > Certificate Trust Settings\n",
    );

    out
}

/// The URL as a QR code, in half-height blocks, or [`None`] where it will not encode.
///
/// **Never an error.** An address and a port is thirty characters at most, so nothing a home
/// produces comes close to the limit - but a code that would not fit is still no reason to fail a
/// share that already worked. The URL above it is the answer; the code is the convenience.
fn qr(url: &str) -> Option<String> {
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;

    Some(
        code.render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{ServiceState, Timestamp};

    use super::*;
    use crate::render::service::tests::{failed_with, summary};

    /// **T81b.** A listing says who owns each site, and an extension's site says so in words a
    /// person can act on.
    #[test]
    fn a_site_listing_names_the_owner() {
        let list = SiteList {
            sites: vec![
                mixengine_proto::SiteSummary {
                    domain: "blog.test".to_owned(),
                    owner: SiteOwner::Project {
                        name: "blog".to_owned(),
                    },
                    kind: SiteKind::Static,
                    doc_root: String::new(),
                    https: true,
                    https_redirect: false,
                    state: mixengine_proto::SiteState::Enabled,
                    routes: Vec::new(),
                    sharing: None,
                },
                mixengine_proto::SiteSummary {
                    domain: "phpmyadmin.mixengine.test".to_owned(),
                    owner: SiteOwner::Extension {
                        id: mixengine_proto::ExtensionId::parse("phpmyadmin").expect("an id"),
                    },
                    kind: SiteKind::PhpFpm { pool: None },
                    doc_root: "app".to_owned(),
                    https: true,
                    https_redirect: false,
                    state: mixengine_proto::SiteState::Enabled,
                    routes: Vec::new(),
                    sharing: None,
                },
            ],
        };

        let rendered = site_list(&list);

        assert!(rendered.contains("OWNER"), "{rendered}");
        assert!(rendered.contains("  blog\n"), "{rendered}");
        assert!(rendered.contains("extension phpmyadmin"), "{rendered}");
    }

    /// A share as the daemon answers one, advertised or not — roadmap tasks **T74** and **T75**.
    fn a_share(advertised: bool) -> SiteSharing {
        SiteSharing {
            interface: "Wi-Fi".to_owned(),
            address: "192.168.1.10".to_owned(),
            url: "http://192.168.1.10".to_owned(),
            name: Some("blog-mixengine.local".to_owned()),
            advertised,
            ca_url: "http://192.168.1.10/__mixengine/ca.crt".to_owned(),
            since: Timestamp(1_700_000_000_000),
            until: None,
        }
    }

    /// What `mix site share` prints once the phone has a name and somewhere to get the authority —
    /// roadmap task **T75**.
    ///
    /// **The QR stays on the address.** Android's resolver does not answer `.local` for a browser,
    /// so a code carrying the name would be a broken URL for a large share of phones — the T75
    /// design, D11.
    #[test]
    fn a_shared_site_prints_its_name_and_where_to_get_the_authority() {
        let rendered = super::site_shared(&a_share(true));

        assert!(rendered.contains("blog-mixengine.local"), "{rendered}");
        assert!(rendered.contains("/__mixengine/ca.crt"), "{rendered}");
        assert!(
            rendered.contains("Certificate Trust Settings"),
            "{rendered}"
        );
    }

    /// **A deadline is printed as a wait and not as a timestamp** — roadmap task **T76**. Somebody
    /// who has just typed `--for 2h` wants to know it took, and "in 2h" is the answer to that; an
    /// instant in milliseconds is a number they would have to convert.
    #[test]
    fn a_share_with_a_deadline_says_when_it_ends() {
        let Timestamp(millis) = Timestamp::from_system_time(SystemTime::now());

        let sharing = SiteSharing {
            until: Some(Timestamp(millis + 7_200_000)),
            ..a_share(true)
        };

        let rendered = super::site_shared(&sharing);

        // **The wait and not the figure.** `site_shared` reads its own clock, so the minutes have
        // already moved by the time this line runs; asserting `in 2h 0m` would be asserting that
        // no time passed between two statements, which is the shape of a test that fails once a
        // month on a loaded machine and is then marked flaky rather than read.
        assert!(rendered.contains("  ends       in "), "{rendered}");
    }

    /// **A share with no deadline prints no line about one.** Every line in this block is one a
    /// reader has to take in, and "ends never" is one they would have to learn to skip.
    #[test]
    fn a_share_with_no_deadline_says_nothing_about_one() {
        assert!(!super::site_shared(&a_share(true)).contains("ends"));
    }

    /// **A name nothing is answering for is said, not hidden.** The site still works by address,
    /// and a name printed as though it resolved is the slowest kind of wrong.
    #[test]
    fn a_name_that_is_not_advertised_says_so() {
        let rendered = super::site_shared(&a_share(false));

        assert!(rendered.contains("not being advertised"), "{rendered}");
    }

    /// What `mix site share` prints — roadmap task **T74**.
    ///
    /// **The URL is above the code, and both are there.** A QR is unreadable to anyone reading a
    /// transcript, piping the output, or working over a connection that mangles block characters,
    /// so the string a person could type by hand is never replaced by a picture of it.
    #[test]
    fn a_shared_site_prints_its_url_the_interface_and_a_code() {
        let rendered = super::site_shared(&a_share(true));

        assert!(rendered.contains("http://192.168.1.10"), "{rendered}");
        assert!(rendered.contains("Wi-Fi"), "{rendered}");

        // The code itself: half-height blocks, and enough of them to be a QR rather than a stray
        // character in a sentence.
        assert!(
            rendered.matches(['█', '▀', '▄']).count() > 100,
            "{rendered}"
        );

        // And why it is http, which is the question the URL raises for a site declaring HTTPS.
        assert!(rendered.contains("certificate authority"), "{rendered}");
    }

    /// **T200b, D6.** `mix site show` names the service its site cannot start, and why.
    #[test]
    fn a_site_whose_pool_failed_says_which_and_why() {
        let detail: SiteDetail = serde_json::from_value(serde_json::json!({
            "site": {
                "domain": "phpmyadmin.mixengine.test",
                "owner": {"type": "extension", "id": "phpmyadmin"},
                "kind": {"kind": "php-fpm", "pool": "php-fpm@phpmyadmin"},
                "doc_root": "",
                "https": true,
                "https_redirect": false,
                "state": "enabled"
            },
            "root": "/x",
            "doc_root_full": "/x",
            "doc_root_exists": true,
            "domains": ["phpmyadmin.mixengine.test"],
            "pool": {"declared": "php-fpm@phpmyadmin", "resolved": "php-fpm@phpmyadmin"},
            "services": [{"service": "mariadb@main", "state": "running"}]
        }))
        .expect("a site detail");

        let services = ServiceList {
            services: vec![
                failed_with("php-fpm@phpmyadmin", "no credential is stored at x"),
                summary("mariadb@main", Some(ServiceState::Running)),
            ],
        };

        assert_eq!(
            site_failures(&detail, &services),
            "php-fpm@phpmyadmin could not start: no credential is stored at x
"
        );
        assert_eq!(
            site_failures(
                &detail,
                &ServiceList {
                    services: vec![summary("php-fpm@phpmyadmin", Some(ServiceState::Running))]
                }
            ),
            ""
        );
    }
}
