//! What a browser gets from a site with more than one thing behind it.
//!
//! The rendering assertions in the two recipes say what the files contain; this says what is
//! *served*, which is the claim the design makes. Four things the text cannot prove on its own:
//! that a prefix matches at a segment boundary rather than as text, that the upstream's path really
//! replaces the matched prefix, that a longer prefix wins over a shorter one whatever order they
//! were typed in, and that the site's own kind still answers everything no route matched.
//!
//! **One sequence, driven twice**, on `frontend.rs`' rule and `welcome.rs`' arrangement: the claim
//! is the same sentence for both servers and the directives are not, so two files that looked alike
//! would drift and the one that drifted would be the one nobody was reading.
//!
//! **No PHP, and the gap is worth stating.** The targets this suite drives are a proxy and a
//! directory, which is everything the prefix rules can be measured with; a php-fpm route would need
//! a runtime download to measure a `fastcgi_pass` that `php_site.rs` already proves. What is left
//! uncovered is that shape's *text* — the nested `location ~ \.php$` inside `location ^~ /admin/`,
//! which `recipes::nginx` asserts and no server here reads. It is the one thing about routes that
//! is asserted rather than served, and it is the shape the welcome page's source leak came from, so
//! a suite that later has a PHP to hand should drive it.
//!
//! **The upstreams are two `TcpListener`s in this process**, answering the request line back. That
//! is what makes the rewrite visible: an upstream that echoed nothing would let `/abc` → `/xyz`
//! pass as long as *something* answered.
//!
//! `#[ignore]`d rather than skipped, for `php_site.rs`' reason: a test that quietly returns when it
//! finds no front end is a green suite that proved nothing on the day the download broke.

mod harness;

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};

use harness::frontend::{CADDY, FrontEnd, NGINX, free_port, request_as};
use harness::{Home, json};

/// The status line of an answer, for a message that says which one came back.
fn status(answer: &str) -> &str {
    answer.lines().next().unwrap_or("no answer at all")
}

/// An upstream that answers every request with `<name> <path>`.
///
/// **The path it was actually asked for**, which is the whole instrument: a rewrite that did not
/// happen and one that happened twice both answer 200, and only the echoed line tells them apart.
///
/// One thread, accepting forever, dropped when the process ends — a fixture rather than a server.
fn upstream(name: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("an address").port();

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };

            std::thread::spawn(move || answer(name, stream));
        }
    });

    port
}

/// One request, answered with the path it asked for.
fn answer(name: &'static str, mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
    let mut line = String::new();

    if reader.read_line(&mut line).is_err() {
        return;
    }

    // `GET /xyz/foo?q=1 HTTP/1.1` — the middle word, which is what the front end decided to ask
    // for and the only thing this fixture is here to report.
    let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
    let body = format!("{name} {path}");

    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// **One site answers from four places, and the same four on both front ends.**
///
/// The whole of routes in one arc: the fallback at `/`, a proxy that passes the path through, a
/// proxy that rewrites it, a directory off disk, and the segment boundary that keeps `/abcdef` out
/// of `/abc`.
async fn one_site_answers_from_four_places(front: &'static FrontEnd) {
    let (home, _daemon, _registry, port, _control) = harness::frontend::declared(front).await;

    let first = upstream("FIRST");
    let second = upstream("SECOND");

    let repository = tempfile::tempdir().expect("a project directory");
    let root = repository.path();
    std::fs::write(root.join("index.html"), "THE SITE ITSELF").expect("an index");
    std::fs::create_dir_all(root.join("dist")).expect("a build directory");
    std::fs::write(root.join("dist").join("app.css"), "THE ASSET").expect("an asset");

    home.mix(&[
        "project",
        "create",
        &root.display().to_string(),
        "--name",
        "blog",
    ]);

    // **Typed shortest-first on purpose.** What decides which route wins is the specificity the
    // daemon sorts by, not the order a person happened to type — and a rendering that leaned on
    // the order would pass every other assertion here and fail this one.
    let created = json(&home.mix(&[
        "site",
        "create",
        "--project",
        "blog",
        "--domain",
        "routes.test",
        "--kind",
        "static",
        "--https",
        "false",
        "--proxy",
        &format!("/api=http://127.0.0.1:{first}"),
        "--proxy",
        &format!("/abc=http://127.0.0.1:{second}/xyz"),
        "--proxy",
        &format!("/api/deep=http://127.0.0.1:{second}"),
        "--files",
        "/assets=dist",
        "--json",
    ]));

    assert_eq!(
        created["site"]["site"]["domain"],
        "routes.test",
        "{created}\n{}",
        home.daemon_log()
    );

    // **Match order, which is the answer `site.show` gives.** Longest first, whatever was typed.
    let paths: Vec<&str> = created["site"]["site"]["routes"]
        .as_array()
        .expect("the routes came back")
        .iter()
        .map(|route| route["path"].as_str().expect("a path"))
        .collect();

    assert_eq!(
        paths,
        ["/api/deep", "/assets", "/abc", "/api"],
        "the answer is in match order rather than in the order they were typed: {created}"
    );

    // **Started after the site exists**, so the first configuration the process ever reads is the
    // one with the routes in it — a start and then a reload would measure two things at once.
    let started = json(&home.mix(&["service", "start", front.package, "--json"]));
    assert_eq!(
        started["complete"],
        true,
        "{started}\n{}",
        home.daemon_log()
    );

    let rendering = home.site_file(front, "routes.test");

    // The process is up before it is listening, and on Windows Defender reads fifty megabytes of
    // it first. Waiting for the first answer rather than for a fixed time.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

    while request_as(port, "/", "routes.test").is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "{} never answered at all\n--- rendering ---\n{rendering}\n--- daemon.log ---\n{}",
            front.package,
            home.daemon_log()
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // What each address must answer, and why the pair is the assertion rather than the status.
    let cases: [(&str, &str, &str); 6] = [
        (
            "/",
            "THE SITE ITSELF",
            "the site's own kind still answers everything no route matched",
        ),
        (
            "/api/x",
            "FIRST /api/x",
            "a route whose upstream carries no path forwards what was asked for",
        ),
        (
            "/abc/foo",
            "SECOND /xyz/foo",
            "the upstream's path replaces the matched prefix",
        ),
        (
            "/abc",
            "SECOND /xyz",
            "and it does so for the exact prefix as well, which is the case a single regex \
             shape leaves with no path at all",
        ),
        (
            "/api/deep/thing",
            "SECOND /api/deep/thing",
            "the longer prefix wins, whatever order it was typed in",
        ),
        (
            "/assets/app.css",
            "THE ASSET",
            "a files route strips the prefix and serves out of its own directory",
        ),
    ];

    for (path, expected, because) in cases {
        let answer = request_as(port, path, "routes.test").unwrap_or_else(|| {
            panic!(
                "the front end answered nothing at all for {path}\n{}",
                home.daemon_log()
            )
        });

        assert!(
            answer.starts_with("HTTP/1.1 200"),
            "{path}: {because} — {}\n--- rendering ---\n{rendering}",
            status(&answer)
        );
        assert!(
            answer.contains(expected),
            "{path}: {because}\n--- answer ---\n{answer}\n--- rendering ---\n{rendering}"
        );
    }

    // **The segment boundary, on its own, because it is the one a prefix match gets wrong.**
    // nginx's `location /abc` takes `/abcdef` as text; Caddy's `path /abc/*` does not. Both
    // renderings are anchored, so this must reach the site's own files and 404 there.
    let past_the_boundary =
        request_as(port, "/abcdef", "routes.test").expect("the front end answered");

    assert!(
        !past_the_boundary.contains("SECOND"),
        "/abcdef is not under /abc, and a prefix matched as text would send it there:\n\
         {past_the_boundary}\n--- rendering ---\n{rendering}"
    );
    assert!(
        past_the_boundary.starts_with("HTTP/1.1 404"),
        "it belongs to the site's own files, which do not hold it: {}\n--- rendering ---\n\
         {rendering}",
        status(&past_the_boundary)
    );

    // **A route removed is a route gone**, which is what `--no-routes` is for and what makes the
    // replace-the-whole-list rule visible from outside.
    let emptied = json(&home.mix(&["site", "update", "routes.test", "--no-routes", "--json"]));

    assert!(
        emptied["site"]["routes"]
            .as_array()
            .is_none_or(std::vec::Vec::is_empty),
        "{emptied}"
    );

    // **Waited for rather than asserted at once.** The row is gone the moment `site.update`
    // answers; the running process learns about it through a re-render and a reload, which is a
    // handful of milliseconds later and is not this suite's to measure.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

    loop {
        let after = request_as(port, "/api/x", "routes.test").unwrap_or_default();

        if !after.contains("FIRST") {
            break;
        }

        assert!(
            std::time::Instant::now() < deadline,
            "a route the site no longer declares is still being served:\n{after}\n\
             --- rendering ---\n{}\n--- daemon.log ---\n{}",
            home.site_file(front, "routes.test"),
            home.daemon_log()
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Held to the end of the test rather than dropped with the fixture that made them.
    drop(repository);
    drop(home);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real Caddy — see the module note, and the `caddy` step in _services.yml"]
async fn caddy_answers_one_site_from_four_places() {
    one_site_answers_from_four_places(&CADDY).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real nginx — see the module note, and the `nginx` step in _services.yml"]
async fn nginx_answers_one_site_from_four_places() {
    one_site_answers_from_four_places(&NGINX).await;
}

/// Kept so an unused-import warning does not hide a real one when the two tests above are filtered
/// out — `free_port` and `Home` are named by the fixture this file drives.
#[allow(dead_code)]
fn _referenced(home: &Home) -> u16 {
    let _ = home.path();

    free_port()
}
