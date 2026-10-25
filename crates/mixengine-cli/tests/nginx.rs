//! The nginx recipe against a **real** nginx.
//!
//! [`caddy.rs`](../caddy.rs)'s file for the other front end, and deliberately almost all of it is
//! this page's four constants: the sequence a front end has to walk lives in [`harness::frontend`]
//! and is driven twice, which is what the roadmap means by "a parity test suite running both
//! generators". A copy of the arc written out again here would be two things to keep in step, and
//! the one that drifted would be green while it drifted.
//!
//! **It is `#[ignore]`d rather than skipped**, for Caddy's reason: a test that quietly returns when
//! it cannot find an nginx is a green suite that proved nothing on the day the download broke. The
//! `nginx` step in `.github/workflows/_services.yml` fetches a real archive on all three systems.
//!
//! # What only a real nginx can answer
//!
//! Three of this recipe's decisions are guesses until this suite runs, and each one fails silently
//! in a different way if it is wrong:
//!
//! - **Whether `nginx -t` accepts a generated configuration at all**, with a Windows path in every
//!   directive that names one. That is the question the forward-slashed quoting exists for, and only
//!   nginx's own parser answers it.
//! - **Whether the prefix makes `include sites/*.conf` resolve where the recipe says it does** — in
//!   the staging directory while the rendering is being judged, and in `etc/nginx/` once it is
//!   installed. Get it wrong and validation passes over a directory nothing is in.
//! - **Whether `-s reload` reaches the master this daemon started.** A signal that found no pid file
//!   exits non-zero and is reported; one that found the *wrong* one would be worse, which is why the
//!   pid path is written into the same configuration every invocation is given.
//!
//! # The archive, whole
//!
//! Unlike Caddy, the fixture packs the entire unpacked tree rather than one binary: a generated
//! `nginx.conf` `include`s the archive's own `conf/mime.types` by absolute path, and a package
//! without it is one this recipe refuses while rendering. Packing the whole tree is also what makes
//! the `provides` map the suite publishes the same shape `mixengine-packages` publishes.

mod harness;

use harness::frontend;

/// **The whole of the recipe, in the order a user meets it.**
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real nginx — see the module note, and the `nginx` step in _services.yml"]
async fn nginx_is_generated_validated_started_reloaded_and_stopped() {
    frontend::is_generated_validated_started_reloaded_and_stopped(&frontend::NGINX).await;
}

/// **nginx judges and then serves an extension's front-end fragment.**
///
/// The parity half of Caddy's test of the same name, and the one that proves the path spelling: an
/// nginx fragment's `{install_dir}` is forward-slashed whatever system this is, because
/// `ngx_conf_read_token` eats a backslash — which no unit test on Linux could ever have shown.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real nginx — see the module note, and the `nginx` step in _services.yml"]
async fn nginx_serves_what_an_extension_s_fragment_adds() {
    frontend::serves_what_an_extension_s_fragment_adds(&frontend::NGINX).await;
}

/// **A redirecting site sends a plaintext request straight to HTTPS, against a real nginx.** This
/// is the shape TLS wiring did not need: nginx groups a plaintext and a TLS listener into one
/// `server` block for HTTPS alone, but a redirect cannot share that block (see the module note this
/// recipe's own source carries), so this suite is what proves the two-block rendering is a
/// configuration nginx actually accepts and runs — not only one this repository's unit tests can
/// parse.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real nginx — see the module note, and the `nginx` step in _services.yml"]
async fn nginx_redirects_a_site_that_asks_for_it() {
    let (home, _daemon, _registry, site_port, _status) = frontend::declared(&frontend::NGINX).await;

    let repository = tempfile::Builder::new()
        .prefix("mixengine-t98")
        .tempdir()
        .expect("a temporary directory");
    let root = repository.path().display().to_string();

    home.mix(&["project", "create", &root, "--name", "blog"]);
    home.mix_in(
        repository.path(),
        &[],
        &[
            "site",
            "create",
            "--domain",
            "blog.test",
            "--kind",
            "static",
            "--https-redirect",
            "true",
        ],
    );

    let started =
        harness::json(&home.mix(&["service", "start", frontend::NGINX.package, "--json"]));
    assert_eq!(
        started["complete"],
        true,
        "{started}\n{}",
        home.daemon_log()
    );

    let answer = frontend::request_as(site_port, "/", "blog.test").unwrap_or_else(|| {
        panic!(
            "no answer from nginx on the plaintext port\n{}",
            home.daemon_log()
        )
    });

    assert!(answer.starts_with("HTTP/1.1 307"), "{answer}");
    assert!(answer.contains("Location: https://blog.test/"), "{answer}");
}

/// **And a home that has one front end is refused the other** — the rule `Recipe::role` exists for.
///
/// Here rather than in a unit test because what the unit tests know is that
/// `core::services::front_end` finds a front end by its role; what a *user* meets is a
/// `service.create` that says which one is already there. Nothing is started: this costs one
/// install and one create.
///
/// **No Caddy is installed, and that is the assertion.** The refusal is deliberately ordered before
/// the check that the named package exists, because installing the second front end would not help
/// — so a home with no Caddy at all still hears about the nginx it has, rather than being told to go
/// and fetch the thing it is about to be refused.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real nginx — see the module note, and the `nginx` step in _services.yml"]
async fn a_home_that_already_has_a_front_end_is_refused_the_other_one() {
    let (home, _daemon, _registry, _site, _status) = frontend::declared(&frontend::NGINX).await;

    let refused = home.mix(&["service", "create", "caddy", "2.x", "--json"]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );

    assert!(
        !refused.status.success(),
        "a second front end was created beside the first: {said}"
    );
    assert!(
        said.contains("nginx"),
        "the refusal does not name the front end this home already has: {said}"
    );
    assert!(
        !said.contains("is not installed"),
        "the refusal was about the package rather than about there being one front end, which is \
         the ordering this test is for: {said}"
    );
}
