//! What a browser gets from a site with nothing behind it.
//!
//! The rendering assertions in the two recipes say what the file contains; this says what is
//! *served*, which is the claim the design makes. Three things the text cannot prove on its own:
//! that the exact-path route really matches one path, that an application's own 404 survives it,
//! and that the page stops appearing the moment the site has an index — **with no re-render and no
//! reload**, which is what makes "it disappears by itself" a property rather than a promise.
//!
//! **One sequence, driven twice**, on `frontend.rs`' rule: the claim is the same sentence for both
//! servers and the directives are not, so two files that looked alike would drift and the one that
//! drifted would be the one nobody was reading. It is also the arrangement this task needed — the
//! nginx rendering had to be written twice, the first serving a php-fpm site's own source as text,
//! and a suite that could only drive Caddy would have left the second version asserted and never
//! served.
//!
//! **The site starts with an `index.php` and this suite deletes it**, rather than building a fixture
//! with an empty document root. That is the stronger arrangement: the file is the only thing that
//! changes between the two answers, so a page that appeared for any other reason would not appear
//! here, and the front end is never told anything happened.
//!
//! `#[ignore]`d rather than skipped, for `php_site.rs`' reason: a test that quietly returns when it
//! finds no PHP is a green suite that proved nothing on the day the download broke.

mod harness;

use harness::frontend::{CADDY, FrontEnd, NGINX, request_as};
use harness::php_site;

/// The status line of an answer, for a message that says which one came back.
fn status(answer: &str) -> &str {
    answer.lines().next().unwrap_or("no answer at all")
}

/// **A site whose document root has nothing in it answers the welcome page at `/`**, and every other
/// path still answers what the application answers.
///
/// The whole of the welcome page in one arc: 200 rather than 404, this site's own name on the page,
/// `no-store` so the answer cannot outlive the index that replaces it — and then the index written
/// back, with the site answering for itself again and nothing reloaded in between.
async fn a_site_with_nothing_in_it_says_so(front: &'static FrontEnd) {
    let served = php_site::served(front, &php_site::runtimes()[..1]).await;
    let site = &served.sites[0];
    let index = site.root.join("index.php");

    // **Kept for the failure messages below**, because what goes wrong here is a rendering: the
    // first nginx one served this file's own bytes, and a bare status line would not have said
    // which of the two mistakes it was.
    let rendering = served.home.site_file(front, &site.domain);

    std::fs::remove_file(&index).expect("the fixture's index");

    let answer = request_as(served.port, "/", &site.domain).unwrap_or_else(|| {
        panic!(
            "the front end answered nothing at all\n{}",
            served.home.daemon_log()
        )
    });

    assert!(
        answer.starts_with("HTTP/1.1 200"),
        "a site with nothing in it must not answer 404: {}\n--- rendering ---\n{rendering}",
        status(&answer)
    );
    assert!(
        answer.contains(&site.domain),
        "the page names the site it is for:\n{answer}"
    );
    assert!(
        answer.to_ascii_lowercase().contains("no-store"),
        "without no-store a cached welcome page outlives the index that replaced it:\n{answer}"
    );

    // **Only the site's root gets the page, and this is the assertion that rule exists for.** A
    // catch-all error handler passes
    // everything above and fails here.
    for path in ["/missing", "/api/anything"] {
        let elsewhere = request_as(served.port, path, &site.domain)
            .unwrap_or_else(|| panic!("the front end answered nothing at all for {path}"));

        assert!(
            elsewhere.starts_with("HTTP/1.1 404"),
            "{path} must still be this application's own 404: {}\n--- rendering ---\n{rendering}",
            status(&elsewhere)
        );
    }

    // **And it goes away by itself.** Nothing is re-rendered and nothing is reloaded between these
    // two requests; the only thing that changed is the file.
    std::fs::write(&index, format!("<?php echo \"{}\";\n", site.says)).expect("an index");

    let mine = request_as(served.port, "/", &site.domain).expect("an answer");

    assert!(
        mine.contains(&site.says),
        "the site's own index must answer once it exists:\n{mine}\n--- rendering ---\n{rendering}"
    );
    assert!(
        !mine.to_ascii_lowercase().contains("no-store"),
        "the welcome page outlived the index that replaced it:\n{mine}"
    );
}

/// **A php-fpm site's own source is never what is served.**
///
/// Asserted on its own rather than as a clause above, because the failure is a *disclosure*: the
/// first nginx rendering named `/index.php` in a `try_files` inside a location with no
/// `fastcgi_pass`, and `try_files` serves what it finds in the current context. That answers 200
/// with the site's own bytes, so every assertion about status and about the welcome page passes.
async fn a_sites_home_page_is_never_its_own_source(front: &'static FrontEnd) {
    let served = php_site::served(front, &php_site::runtimes()[..1]).await;
    let site = &served.sites[0];

    let answer = request_as(served.port, "/", &site.domain).expect("an answer");

    assert!(
        !answer.contains("<?php"),
        "the home page served this site's source instead of running it:\n{answer}"
    );
    assert!(
        answer.contains(&site.says),
        "and it must be the script's output:\n{answer}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real Caddy and a real PHP — see the module note, and the `caddy` and `php` steps in _services.yml"]
async fn caddy_says_so_when_a_site_has_nothing_in_it() {
    a_site_with_nothing_in_it_says_so(&CADDY).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real nginx and a real PHP — see the module note, and the `nginx` and `php` steps in _services.yml"]
async fn nginx_says_so_when_a_site_has_nothing_in_it() {
    a_site_with_nothing_in_it_says_so(&NGINX).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real Caddy and a real PHP"]
async fn caddy_never_serves_a_sites_source() {
    a_sites_home_page_is_never_its_own_source(&CADDY).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real nginx and a real PHP"]
async fn nginx_never_serves_a_sites_source() {
    a_sites_home_page_is_never_its_own_source(&NGINX).await;
}
