//! The page a site answers with when it has nothing behind it — roadmap task **T124**.
//!
//! **One template for both front ends**, because the page is the same page whichever program is
//! serving it: what differs is how each one is told to reach it, and that lives in the two site
//! templates. A second copy would be a second answer to "what does a new site say", and the copy
//! that drifted would be the one nobody was reading on the day it mattered.
//!
//! **It names nothing about this machine** — the T124 design, D5. A shared site binds its interface
//! address (T74) and answers to an mDNS name (T75), so a phone on the local network can fetch this
//! page. An absolute path and a database account are facts about the machine; the document root as
//! the *row* spells it is the one detail that is both useful and already implied by the site itself,
//! which is why this module is handed that string rather than the joined path
//! [`Served::doc_root`](super::served::Served::doc_root) carries.

use mixengine_proto::ServiceId;

use crate::Result;
use crate::generate::served::ServedKind;

/// The page, compiled in — [`crate::blueprints::gallery`]'s D1, for its reason: what this build
/// ships is a constant of this build, not a document it fetches or a file the user is invited to
/// edit and then owns forever.
const PAGE: &str = include_str!("welcome/page.html");

/// The page a site whose backend MixEngine manages answers while that backend is not answering —
/// roadmap task **T167f**. Compiled in for [`PAGE`]'s reason.
const STARTING: &str = include_str!("welcome/starting.html");

/// Where a site's starting page is written, inside the welcome directory.
///
/// Beside its welcome page and named after the same domain, so the two templates route to it with
/// the same `{{ primary }}` they already have.
#[must_use]
pub fn starting_file(primary: &str) -> String {
    format!("{primary}.starting.html")
}

/// What the starting page is told: the domain, and nothing else about this machine (T124's D5 —
/// the page can be fetched from the local network).
#[derive(Debug, serde::Serialize)]
struct StartingPage<'a> {
    domain: &'a str,
}

/// The starting page for `primary`, rendered — or [`None`] for a site of a kind MixEngine does not
/// start the backend of.
///
/// **A php-fpm site only.** A pool is MixEngine's to start: the request that finds it down wakes it,
/// and a 502 here means that wake has not finished or has failed, which is what this page says. A
/// reverse proxy or a Node app points at a program the person runs themselves, and the welcome page
/// already tells them to start it; a static site has no backend to be down.
///
/// # Errors
///
/// [`crate::Error::TemplateBroken`] naming the page, as [`render`].
pub fn render_starting(
    service: &ServiceId,
    primary: &str,
    kind: &ServedKind,
) -> Result<Option<String>> {
    if !matches!(kind, ServedKind::PhpFpm { .. }) {
        return Ok(None);
    }

    crate::generate::served::render(
        STARTING,
        "welcome/starting.html",
        service,
        &StartingPage { domain: primary },
    )
    .map(Some)
}

/// One command the page names — roadmap task **T205**, D6. Never anything about a database.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WelcomeStep {
    /// The command, expanded.
    pub run: String,

    /// Its one sentence, when it has one.
    pub note: Option<String>,
}

/// What the template is told to say about one site.
#[derive(Debug, serde::Serialize)]
pub struct WelcomePage<'a> {
    /// The primary domain, which is the one thing on this page the reader already knows.
    pub domain: &'a str,

    /// The site's kind, in the words a person uses rather than the wire's.
    pub kind: &'static str,

    /// The one sentence that is true for this kind.
    pub what_to_do: String,

    /// What the site's blueprint says to run, in order — roadmap task **T205**, D6. Empty where
    /// it says nothing, and then the kind's sentence stands alone.
    pub steps: &'a [WelcomeStep],
}

/// What to say about a site of this kind.
///
/// **The sentence comes from the kind and not from a blueprint.** Every site has a kind; a
/// blueprint's own words are an addition, deferred, and a page that could only speak for the sites a
/// blueprint made would be silent in the ordinary case — which is `mix site create` against a
/// directory somebody has not written anything into yet.
///
/// `doc_root` is the row's own relative spelling: empty for a site served from the project root,
/// `public` for Laravel, `web` for Drupal. **Never the joined absolute path** (D5).
///
/// A php-fpm site's upstream is deliberately absent from what this returns: it is a socket path on
/// two of the three systems, which is exactly the kind of fact D5 keeps off a page the local
/// network can fetch. The sentence that kind needs is about a file anyway.
pub fn page<'a>(
    primary: &'a str,
    kind: &ServedKind,
    doc_root: &str,
    steps: &'a [WelcomeStep],
) -> WelcomePage<'a> {
    let where_files_go = where_files_go(doc_root);

    let (name, what_to_do) = match kind {
        ServedKind::PhpFpm { .. } => (
            "PHP",
            format!("Put an index.php in {where_files_go} and reload this page."),
        ),
        ServedKind::Static => (
            "static files",
            format!("Put an index.html in {where_files_go} and reload this page."),
        ),
        ServedKind::NodeApp { port } => (
            "a Node application",
            format!(
                "Nothing is listening on port {port}. Start your development server, then reload \
                 this page."
            ),
        ),
        ServedKind::ReverseProxy { upstream, .. } => (
            "a reverse proxy",
            format!(
                "Nothing answered at {upstream}. Start the program that serves it, then reload \
                 this page."
            ),
        ),
    };

    // **The blueprint's own words, where it has them** — roadmap task **T205**, D6. The kind's
    // sentence says *start your development server*; the steps say which command that is.
    let what_to_do = match steps.is_empty() {
        true => what_to_do,
        false => "Run these in the project directory, then reload this page.".to_owned(),
    };

    WelcomePage {
        domain: primary,
        kind: name,
        what_to_do,
        steps,
    }
}

/// How the document root is named in the sentence.
///
/// The empty string is a site served from the project root, which is what `sites.doc_root` holds for
/// one — and "put an index.php in " with nothing after it is the sentence this exists to prevent.
fn where_files_go(doc_root: &str) -> String {
    if doc_root.is_empty() {
        "the project root".to_owned()
    } else {
        doc_root.to_owned()
    }
}

/// The page, rendered.
///
/// # Errors
///
/// [`crate::Error::TemplateBroken`] naming the page: the template is this build's, so a refusal here
/// is a bug of ours rather than a configuration a user can fix.
pub fn render(service: &ServiceId, page: &WelcomePage<'_>) -> Result<String> {
    crate::generate::served::render(PAGE, "welcome/page.html", service, page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> ServiceId {
        ServiceId::parse("caddy").expect("an id")
    }

    /// **T167f: a PHP site gets a starting page, and it says nothing about the machine.** Anything
    /// else gets none — its backend is either the person's own program or absent.
    #[test]
    fn only_a_php_site_gets_a_starting_page_and_it_names_only_the_domain() {
        let php = ServedKind::PhpFpm {
            upstream: crate::generate::recipe::Upstream::Tcp(
                "127.0.0.1:9000".parse().expect("an address"),
            ),
            activator: None,
        };

        let page = render_starting(&id(), "blog.test", &php)
            .expect("the page renders")
            .expect("a php site has one");
        assert!(page.contains("blog.test"), "{page}");
        // **T200b, D7.** Two places that say why — MixLab's Sites, and `mix site show` for a home
        // with no window — and never the reason itself: a shared site is served to the LAN.
        assert!(page.contains("Sites screen"), "{page}");
        assert!(page.contains("mix site show blog.test"), "{page}");
        assert!(!page.contains("credential"), "{page}");
        for fact in ["127.0.0.1", "9000", "/Users/", "/home/", "root@"] {
            assert!(!page.contains(fact), "the page names {fact}: {page}");
        }

        assert!(
            render_starting(&id(), "blog.test", &ServedKind::Static)
                .expect("renders")
                .is_none()
        );
        assert!(
            render_starting(&id(), "app.test", &ServedKind::NodeApp { port: 3000 })
                .expect("renders")
                .is_none()
        );
    }

    /// **A site served from the project root still gets a sentence.** The row holds an empty string
    /// for one, and the naive rendering ends mid-sentence.
    #[test]
    fn an_empty_doc_root_is_named_rather_than_left_blank() {
        assert_eq!(where_files_go(""), "the project root");
        assert_eq!(where_files_go("public"), "public");
    }

    /// **Each kind says what is actually missing.** A php-fpm site is missing a file; a node-app
    /// site is missing a process, and no file in any directory would change that.
    #[test]
    fn each_kind_says_what_is_actually_missing() {
        let php = page(
            "blog.test",
            &ServedKind::PhpFpm {
                upstream: crate::generate::recipe::Upstream::Tcp(
                    "127.0.0.1:9000".parse().expect("an address"),
                ),
                activator: None,
            },
            "public",
            &[],
        );
        assert!(php.what_to_do.contains("index.php"), "{}", php.what_to_do);
        assert!(php.what_to_do.contains("public"), "{}", php.what_to_do);

        let node = page("app.test", &ServedKind::NodeApp { port: 3000 }, "", &[]);
        assert!(
            node.what_to_do.contains("3000"),
            "the page must name the port nothing is listening on: {}",
            node.what_to_do
        );

        let proxy = page(
            "api.test",
            &ServedKind::ReverseProxy {
                upstream: "http://127.0.0.1:8000".to_owned(),
                rewrite: None,
            },
            "",
            &[],
        );
        assert!(
            proxy.what_to_do.contains("http://127.0.0.1:8000"),
            "{}",
            proxy.what_to_do
        );
    }

    /// **A site with steps names the commands and nothing else** — roadmap task **T205**, D6.
    #[test]
    fn a_site_with_steps_names_the_commands_and_nothing_else() {
        let steps = vec![
            WelcomeStep {
                run: "npm install".to_owned(),
                note: None,
            },
            WelcomeStep {
                run: "npm run dev".to_owned(),
                note: Some("port 3000".to_owned()),
            },
        ];
        let page = page("shop.test", &ServedKind::NodeApp { port: 3000 }, "", &steps);
        let rendered = render(&id(), &page).expect("renders");

        assert!(rendered.contains("npm install"), "{rendered}");
        assert!(rendered.contains("npm run dev"), "{rendered}");
        assert!(rendered.contains("port 3000"), "{rendered}");
        assert!(
            !rendered.contains("Start your development server"),
            "{rendered}"
        );
    }

    #[test]
    fn a_site_without_steps_keeps_the_kinds_sentence() {
        let page = page("shop.test", &ServedKind::NodeApp { port: 3000 }, "", &[]);
        let rendered = render(&id(), &page).expect("renders");
        assert!(
            rendered.contains("Start your development server"),
            "{rendered}"
        );
    }

    /// **A php-fpm site's upstream never reaches the page** — D5. It is a socket path on two of the
    /// three systems this product runs on.
    #[test]
    fn a_pools_address_is_not_on_the_page() {
        let rendered = render(
            &id(),
            &page(
                "blog.test",
                &ServedKind::PhpFpm {
                    upstream: crate::generate::recipe::Upstream::Socket(
                        "/run/php-fpm-8.3.sock".into(),
                    ),
                    activator: None,
                },
                "public",
                &[],
            ),
        )
        .expect("a page");

        assert!(
            !rendered.contains("php-fpm-8.3.sock"),
            "the welcome page leaked the pool's socket:\n{rendered}"
        );
    }

    /// **No absolute path, no database identifier, no service id** — D5, asserted against a
    /// rendering whose inputs would all show up if the template leaked them.
    #[test]
    fn the_rendered_page_names_nothing_about_this_machine() {
        let rendered = render(
            &id(),
            &page("blog.test", &ServedKind::Static, "the project root", &[]),
        )
        .expect("a page");

        for leak in ["/srv/blog", "C:\\", "mariadb", "caddy"] {
            assert!(
                !rendered.contains(leak),
                "the welcome page leaked {leak}:\n{rendered}"
            );
        }
        assert!(rendered.contains("blog.test"), "{rendered}");
    }

    /// **One file, no external reference** — D7. Every one of these would be a broken page on a
    /// machine with no connection, at the exact moment this feature exists to make an impression.
    #[test]
    fn the_page_fetches_nothing() {
        let rendered =
            render(&id(), &page("blog.test", &ServedKind::Static, "", &[])).expect("a page");

        for fetch in ["http://", "https://", "<script", "<img", "@import"] {
            assert!(
                !rendered.contains(fetch),
                "the welcome page reaches outside itself with {fetch}:\n{rendered}"
            );
        }
    }
}
