//! `mix blueprint capture` and `mix blueprint apply` against a real daemon — roadmap task **T78**.
//!
//! What is proved here is what no unit test can: that a project captured on this machine can be
//! applied under another name, that applying it twice is applying it once, and that a capture of the
//! applied project is the blueprint it came from.
//!
//! **Offline by construction.** The fixture is a static site with no runtime and no services, so
//! every install step plans as `Satisfied` and nothing reaches the package index. What that costs is
//! coverage of the install path, which is `tests/runtime.rs`' and `tests/package.rs`' already; what
//! it buys is a suite that says the same thing on a laptop and on a runner with no network.

mod harness;

use harness::{Home, json, stdout};
use mixengine_testkit::Service;
use serde_json::Value;

/// A directory to register.
fn repository() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("mixengine-blueprint")
        .tempdir()
        .expect("a temporary directory")
}

/// A project with one static site, which is the smallest thing worth capturing.
fn a_project_with_a_site(home: &Home, directory: &std::path::Path, name: &str, domain: &str) {
    let root = directory.display().to_string();

    home.mix(&["project", "create", &root, "--name", name]);
    home.mix_in(
        directory,
        &[],
        &["site", "create", "--domain", domain, "--kind", "static"],
    );
}

/// The rendered blueprint, minus the `[blueprint]` block.
///
/// The header is what a second capture is *expected* to differ in — its name, its description and
/// the moment it was taken — and everything after it is what has to be the same.
fn body(rendered: &str) -> String {
    rendered
        .split("\n[")
        .skip(1)
        .filter(|block| !block.starts_with("blueprint]"))
        .map(|block| format!("[{block}"))
        .collect()
}

/// The feature's own acceptance criterion: capture a working project, apply it under a new name,
/// and both are there afterwards with the names they should have.
#[tokio::test(flavor = "multi_thread")]
async fn a_captured_project_is_applied_under_a_new_name() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let first = repository();
    a_project_with_a_site(&home, first.path(), "blog", "blog.test");

    let captured = json(&home.mix(&[
        "blueprint",
        "capture",
        "blog-stack",
        "--project",
        "blog",
        "--json",
    ]));
    assert_eq!(captured["slug"], "blog-stack", "{captured}");

    let second = repository();
    let into = second.path().join("shop").display().to_string();

    let applied = stdout(&home.mix(&[
        "blueprint",
        "apply",
        "blog-stack",
        "--project",
        "shop",
        "--path",
        &into,
    ]));
    assert!(applied.contains("shop"), "{applied}");

    // The project is registered, and at the directory the apply was told to use — which it had to
    // create, because `project.create` takes a root that exists and an apply's does not yet.
    let shown = json(&home.mix(&["project", "show", "shop", "--json"]));
    assert_eq!(shown["project"]["name"], "shop", "{shown}");

    // **`{project}` was expanded**, which is the whole of what a blueprint is for: the captured
    // domain was `blog.test`, tokenised to `{project}.test`, and it comes back as the new name.
    let sites = json(&home.mix(&["site", "list", "--json"]));
    let listed = sites["sites"].as_array().expect("a list of sites");
    assert!(
        listed.iter().any(|site| site["domain"] == "shop.test"),
        "{sites}"
    );

    // And the first site is untouched: applying a blueprint makes a project, it does not move one.
    assert!(
        listed.iter().any(|site| site["domain"] == "blog.test"),
        "{sites}"
    );
}

/// **The proof of D2 and D3 at once.** A second apply finds nothing left to do, which is what makes
/// a failed apply resumable rather than restartable — and it is asserted on the *steps*, because
/// "it did not fail" would also be true of an apply that did everything twice.
#[tokio::test(flavor = "multi_thread")]
async fn applying_the_same_blueprint_twice_leaves_nothing_to_do_the_second_time() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let first = repository();
    a_project_with_a_site(&home, first.path(), "blog", "blog.test");
    home.mix(&["blueprint", "capture", "blog-stack", "--project", "blog"]);

    let second = repository();
    let into = second.path().join("shop").display().to_string();
    let apply = [
        "blueprint",
        "apply",
        "blog-stack",
        "--project",
        "shop",
        "--path",
        &into,
        "--json",
    ];

    home.mix(&apply);

    // **Asserted on the results and not on the dispositions.** A plan reads this home's tables and
    // a certificate is a file, so the certificate step is planned as work either way — and what
    // matters is that carrying it out found there was none. Every step reporting `already true` is
    // the whole claim.
    let again = stdout(&home.mix(&apply));
    assert!(
        !again.contains("\"result\":\"done\""),
        "a second apply did work the first one should have done: {again}"
    );
    assert!(
        again.contains("\"result\":\"already_true\""),
        "a second apply reported nothing at all: {again}"
    );
}

/// **An apply resumed after its directory was deleted makes the directory again** — and does not
/// register the project a second time. Found by a Laravel apply whose folder had been removed by
/// hand: the project step planned `already true`, nothing made the folder, and the scaffold that
/// followed could not be started in it.
#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_apply_makes_the_directory_somebody_deleted() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let first = repository();
    a_project_with_a_site(&home, first.path(), "blog", "blog.test");
    home.mix(&["blueprint", "capture", "blog-stack", "--project", "blog"]);

    let second = repository();
    let root = second.path().join("shop");
    let into = root.display().to_string();
    let apply = [
        "blueprint",
        "apply",
        "blog-stack",
        "--project",
        "shop",
        "--path",
        &into,
        "--json",
    ];

    home.mix(&apply);
    std::fs::remove_dir_all(&root).expect("the directory is deleted by hand");

    // An apply's `--json` is a stream — its log lines, the job, then the result — so the result is
    // the last line.
    let printed = stdout(&home.mix(&apply));
    let again: Value = printed
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| serde_json::from_str(line).ok())
        .unwrap_or_else(|| panic!("the apply's result is its last line: {printed}"));
    let registered = again["steps"]
        .as_array()
        .expect("a list of steps")
        .iter()
        .find(|step| step["action"]["action"] == "register_project")
        .expect("the project step");

    assert_eq!(registered["result"]["result"], "done", "{again}");
    assert!(root.is_dir(), "the directory was not made again");

    // Still one project, at the same place: the directory was made, the row was not written twice.
    let listed = json(&home.mix(&["project", "list", "--json"]));
    let named_shop = listed["projects"]
        .as_array()
        .expect("a list of projects")
        .iter()
        .filter(|project| project["name"] == "shop")
        .count();
    assert_eq!(named_shop, 1, "{listed}");
}

/// **The round trip**, which is what catches D7 and D14 cheaply: a capture of the applied project is
/// the blueprint that made it, header aside. T77 made the renderer byte-identical on purpose, and
/// this is the assertion that spends it.
#[tokio::test(flavor = "multi_thread")]
async fn a_capture_of_an_applied_project_is_the_blueprint_it_came_from() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let first = repository();
    a_project_with_a_site(&home, first.path(), "blog", "blog.test");

    let captured = json(&home.mix(&[
        "blueprint",
        "capture",
        "blog-stack",
        "--project",
        "blog",
        "--json",
    ]));

    let second = repository();
    let into = second.path().join("shop").display().to_string();
    home.mix(&[
        "blueprint",
        "apply",
        "blog-stack",
        "--project",
        "shop",
        "--path",
        &into,
    ]);

    let round_trip = json(&home.mix(&[
        "blueprint",
        "capture",
        "shop-stack",
        "--project",
        "shop",
        "--json",
    ]));

    let before = std::fs::read_to_string(captured["file"].as_str().expect("a path"))
        .expect("the blueprint that was captured");
    let after = std::fs::read_to_string(round_trip["file"].as_str().expect("a path"))
        .expect("the blueprint the applied project makes");

    assert_eq!(
        body(&before),
        body(&after),
        "applying a blueprint and capturing the result gave a different blueprint"
    );
}

/// **The gallery is applied, not just listed** — roadmap task **T79**. `static` is the one that
/// needs no runtime and no service, so this stays offline exactly as this suite's own fixture does.
///
/// **What is asserted about the certificate step is that it did not fail.** `https = true` puts one
/// in this plan and an apply really runs it; a certificate that could not be issued comes back
/// `NotRun` with a reason, on `site.create`'s standing position that a site is worth more than a
/// certificate. Pinning an outcome here would be pinning whether the machine running the suite has
/// an authority.
#[tokio::test(flavor = "multi_thread")]
async fn the_static_blueprint_from_the_gallery_applies() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let directory = repository();
    let into = directory.path().join("shop").display().to_string();

    // `stdout` rather than `json`: an apply prints three documents — the plan, the job as it runs,
    // and the result — which is what this suite's own first test reads too.
    let applied = stdout(&home.mix(&[
        "blueprint",
        "apply",
        "static",
        "--project",
        "shop",
        "--path",
        &into,
        "--json",
    ]));

    assert!(!applied.contains("\"failed\""), "a step failed: {applied}");

    let shown = json(&home.mix(&["project", "show", "shop", "--json"]));
    assert_eq!(shown["project"]["name"], "shop", "{shown}");

    let sites = json(&home.mix(&["site", "list", "--json"]));
    let listed = sites["sites"].as_array().expect("a list of sites");
    assert!(
        listed.iter().any(|site| site["domain"] == "shop.test"),
        "{sites}"
    );
}

/// A gallery blueprint says where it came from, and that this build vouches for it.
#[tokio::test(flavor = "multi_thread")]
async fn a_gallery_blueprint_is_listed_as_this_builds_own() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let listed = json(&home.mix(&["blueprint", "list", "--json"]));
    let found = listed["blueprints"]
        .as_array()
        .expect("a listing")
        .iter()
        .find(|one| one["slug"] == "laravel")
        .unwrap_or_else(|| panic!("the gallery is not listed: {listed}"));

    assert_eq!(found["source"], "builtin", "{listed}");
    assert_eq!(found["trusted"], true, "{listed}");
}

/// **A blueprint captured on Windows applies here** — the feature's own acceptance criterion, and
/// roadmap task **T79**, its design's D11.
///
/// The fixture is a real capture taken on a Windows machine and committed, rather than a gallery
/// file: a gallery manifest is hand-written and byte-identical on all three systems, so applying one
/// says nothing whatever about what a Windows machine writes. What this asserts is the property the
/// criterion names — no absolute paths, no OS-specific service names — by applying those bytes on
/// whatever system is running the suite.
#[tokio::test(flavor = "multi_thread")]
async fn a_blueprint_captured_on_windows_applies_on_this_system() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let directory = repository();
    let into = directory.path().join("shop").display().to_string();

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/captured-on-windows.toml");
    let imported = json(&home.mix(&[
        "blueprint",
        "import",
        &fixture.display().to_string(),
        "--json",
    ]));
    assert_eq!(imported["source"], "imported", "{imported}");

    let slug = imported["slug"].as_str().expect("a slug").to_owned();
    let applied = stdout(&home.mix(&[
        "blueprint",
        "apply",
        &slug,
        "--project",
        "shop",
        "--path",
        &into,
        "--json",
    ]));

    assert!(!applied.contains("\"failed\""), "a step failed: {applied}");

    let sites = json(&home.mix(&["site", "list", "--json"]));
    let listed = sites["sites"].as_array().expect("a list of sites");
    assert!(
        listed.iter().any(|site| site["domain"] == "shop.test"),
        "{sites}"
    );
}

/// **A manifest with a `[site]` plans a front end on a home that has none** — roadmap task T115.
///
/// `core::sites` is explicit that "a home with no front end renders nothing and this succeeds", so
/// an apply that stopped at the site row left a project nothing serves — which is the whole of the
/// complaint this phase is written against. `--dry-run` so the suite stays offline: what is
/// asserted is the plan, and installing Caddy is what the plan *says* rather than what this test
/// does.
#[tokio::test(flavor = "multi_thread")]
async fn a_site_blueprint_plans_a_front_end_for_a_home_that_has_none() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let first = repository();
    a_project_with_a_site(&home, first.path(), "blog", "blog.test");

    home.mix(&["blueprint", "capture", "blog-stack", "--project", "blog"]);

    let second = repository();
    let into = second.path().join("shop").display().to_string();

    let planned = json(&home.mix(&[
        "blueprint",
        "apply",
        "blog-stack",
        "--project",
        "shop",
        "--path",
        &into,
        "--with-front-end",
        "--dry-run",
        "--json",
    ]));

    let steps = planned["steps"]
        .as_array()
        .unwrap_or_else(|| panic!("a plan has steps: {planned}"));

    let front_end: Vec<&serde_json::Value> = steps
        .iter()
        .filter(|step| step["action"]["package"] == "caddy")
        .collect();

    assert_eq!(
        front_end.len(),
        2,
        "a home with no front end gets an install and an ensure: {planned}"
    );
    assert!(
        front_end
            .iter()
            .all(|step| step["disposition"]["disposition"] == "create"),
        "both are work on a home that has neither: {planned}"
    );
}

/// **And plans nothing without the flag** — roadmap task T115.
///
/// The other half, and the one that keeps an apply from provisioning a machine nobody asked it to:
/// the same blueprint on the same home, planned twice, differs only by `--with-front-end`. What a
/// home that *has* a front end plans is asserted where a real web server is available — this suite
/// is offline by construction and has none.
#[tokio::test(flavor = "multi_thread")]
async fn a_site_blueprint_plans_no_front_end_unless_it_is_asked_to() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    let first = repository();
    a_project_with_a_site(&home, first.path(), "blog", "blog.test");

    home.mix(&["blueprint", "capture", "blog-stack", "--project", "blog"]);

    let second = repository();
    let into = second.path().join("shop").display().to_string();

    let planned = json(&home.mix(&[
        "blueprint",
        "apply",
        "blog-stack",
        "--project",
        "shop",
        "--path",
        &into,
        "--dry-run",
        "--json",
    ]));

    let steps = planned["steps"]
        .as_array()
        .unwrap_or_else(|| panic!("a plan has steps: {planned}"));

    assert!(
        steps
            .iter()
            .all(|step| step["action"]["package"] != "caddy"),
        "an apply nobody asked for a web server plans none: {planned}"
    );
}

/// **`--autostart` reaches the service the apply creates, and nothing it found** — roadmap task T116.
///
/// The whole of what the flag claims, from the end a person is at. The fixture declares one
/// `fakeservice` instance and no site, so this stays offline: the package row is already there from
/// `home.declare`, the install step plans `Satisfied`, and the instance is the one thing created.
#[tokio::test(flavor = "multi_thread")]
async fn an_apply_hands_the_autostart_setting_to_what_it_creates() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    // Writes the `fakeservice` package row, which is what keeps the install step offline — and a
    // service of its own, which is the one this apply must *not* re-decide. The async form, because
    // `Home::declare` builds a runtime of its own and this test already is one.
    mixengine_testkit::create(
        home.endpoint_ref(),
        &home.database_file(),
        &[Service::new("fakeservice@untouched")],
    )
    .await;

    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/with-a-service.toml");
    home.mix(&["blueprint", "import", &fixture.display().to_string()]);

    let directory = repository();
    let into = directory.path().join("shop").display().to_string();

    let applied = stdout(&home.mix(&[
        "blueprint",
        "apply",
        "with-a-service",
        "--project",
        "shop",
        "--path",
        &into,
        "--autostart",
        "--json",
    ]));
    assert!(!applied.contains("\"failed\""), "a step failed: {applied}");

    let listed = json(&home.mix(&["service", "list", "--json"]));
    let services = listed["services"].as_array().expect("a list of services");

    let made = services
        .iter()
        .find(|service| service["id"] == "fakeservice@shop")
        .unwrap_or_else(|| panic!("the apply made an instance: {listed}"));
    assert_eq!(
        made["autostart"],
        Value::Bool(true),
        "what the apply made carries the setting: {listed}"
    );

    let found = services
        .iter()
        .find(|service| service["id"] == "fakeservice@untouched")
        .unwrap_or_else(|| panic!("the service that was already here: {listed}"));
    assert_eq!(
        found["autostart"],
        Value::Bool(false),
        "a service this apply did not make is not re-decided: {listed}"
    );
}

/// **And without the flag, nothing carries it** — roadmap task T116.
///
/// The default is a constraint rather than a taste: `warm_start.rs` is the `bench` job and times a
/// single `mix service start`, so an apply that quietly set this would put a boot walk beside that
/// measurement.
#[tokio::test(flavor = "multi_thread")]
async fn an_apply_sets_no_autostart_unless_it_is_asked_to() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    mixengine_testkit::create(
        home.endpoint_ref(),
        &home.database_file(),
        &[Service::new("fakeservice@untouched")],
    )
    .await;

    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/with-a-service.toml");
    home.mix(&["blueprint", "import", &fixture.display().to_string()]);

    let directory = repository();
    let into = directory.path().join("shop").display().to_string();

    home.mix(&[
        "blueprint",
        "apply",
        "with-a-service",
        "--project",
        "shop",
        "--path",
        &into,
        "--json",
    ]);

    let listed = json(&home.mix(&["service", "list", "--json"]));

    assert!(
        listed["services"]
            .as_array()
            .expect("a list of services")
            .iter()
            .all(|service| service["autostart"] == Value::Bool(false)),
        "an apply nobody asked set nothing: {listed}"
    );
}

/// **`--start` leaves this project's services running, and only those** — roadmap tasks T117 and
/// **T125**.
///
/// The last step of *get me a working site*, and the one the apply itself may not take: an apply
/// never raises an elevation prompt, so a front end started inside the job would serve the new site
/// at a name this machine does not resolve.
///
/// **T117 sent no target at all**, which means *every service this home declares*, because a client
/// may not derive the apply's own set and the daemon had no narrower question to ask. T125 gave it
/// one, and the home here is the case that made it matter: two instances of the same service, one
/// this project's and one somebody else's. A start that leaves the second running is a machine
/// where installing a second stack turns on every database its owner has.
#[tokio::test(flavor = "multi_thread")]
async fn an_apply_starts_what_the_project_needs_and_leaves_the_rest_alone() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    mixengine_testkit::create(
        home.endpoint_ref(),
        &home.database_file(),
        &[Service::new("fakeservice@already")],
    )
    .await;

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/with-a-site-and-a-service.toml");
    home.mix(&["blueprint", "import", &fixture.display().to_string()]);

    let directory = repository();
    let into = directory.path().join("shop").display().to_string();

    let applied = home.mix(&[
        "blueprint",
        "apply",
        "with-a-site-and-a-service",
        "--project",
        "shop",
        "--path",
        &into,
        "--start",
        "--json",
    ]);
    assert!(
        applied.status.success(),
        "the apply and the start: {}",
        stdout(&applied)
    );

    let listed = json(&home.mix(&["service", "list", "--json"]));
    let services = listed["services"].as_array().expect("a list of services");
    let state = |id: &str| {
        services
            .iter()
            .find(|service| service["id"] == id)
            .unwrap_or_else(|| panic!("`{id}` is declared: {listed}"))["state"]
            .clone()
    };

    assert_eq!(
        state("fakeservice@shop"),
        "running",
        "the site this apply made declares it: {listed}"
    );
    assert_eq!(
        state("fakeservice@already"),
        "stopped",
        "no site of this project declares it, and `--start` is not a machine-wide switch: {listed}"
    );
}

/// **The steps a blueprint declares reach the person, and its projects keep them** — roadmap task
/// **T205**, D5 and D6. The apply's answer carries them expanded, `project show` reads them back
/// through `blueprint_id`, and a capture of the applied project writes them back with the token,
/// not the slug.
#[tokio::test(flavor = "multi_thread")]
async fn an_applied_blueprints_next_steps_are_reported_kept_and_captured() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/with-next-steps.toml");
    home.mix(&["blueprint", "import", &fixture.display().to_string()]);

    let directory = repository();
    let into = directory.path().join("app").display().to_string();

    // `--json` streams the plan, the job's lines and the job; the last line is what was applied.
    let streamed = stdout(&home.mix(&[
        "blueprint",
        "apply",
        "with-next-steps",
        "--project",
        "My App",
        "--path",
        &into,
        "--json",
    ]));
    let applied: Value = serde_json::from_str(streamed.lines().last().expect("a last line"))
        .expect("the applied report");
    let steps = &applied["next_steps"];
    assert_eq!(steps["trusted"], false, "an unsigned import: {applied}");
    assert_eq!(steps["steps"][0]["run"], "npm install my-app", "{applied}");
    assert_eq!(steps["steps"][1]["kind"], "serve", "{applied}");

    let shown = json(&home.mix(&["project", "show", "My App", "--json"]));
    assert_eq!(shown["next_steps"], *steps, "{shown}");

    let printed = stdout(&home.mix(&["project", "show", "My App"]));
    assert!(printed.contains("npm install my-app"), "{printed}");

    let captured = json(&home.mix(&[
        "blueprint",
        "capture",
        "again",
        "--project",
        "My App",
        "--json",
    ]));
    let rendered = std::fs::read_to_string(
        captured["file"]
            .as_str()
            .expect("the capture names its file"),
    )
    .expect("the rendering");
    assert!(
        rendered.contains("run = \"npm install {project}\""),
        "{rendered}"
    );
}

/// A fixture blueprint, imported into `home`.
fn imported(home: &Home, file: &str) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(file);
    let output = home.mix(&["blueprint", "import", &fixture.display().to_string()]);
    assert!(
        output.status.success(),
        "import {file}: {}",
        stdout(&output)
    );
}

/// What `mix site show` says about one site: its names, and the services it links.
fn site_shown(home: &Home, domain: &str) -> (Vec<String>, Vec<String>) {
    let shown = json(&home.mix(&["site", "show", domain, "--json"]));
    let strings = |values: &Value, key: Option<&str>| -> Vec<String> {
        values
            .as_array()
            .unwrap_or_else(|| panic!("a list in {shown}"))
            .iter()
            .map(|value| {
                key.map_or(value, |key| &value[key])
                    .as_str()
                    .unwrap_or_else(|| panic!("a string in {shown}"))
                    .to_owned()
            })
            .collect()
    };

    (
        strings(&shown["domains"], None),
        strings(&shown["services"], Some("service")),
    )
}

/// **T204a.** A blueprint with two sites makes both, each with its own names and only its own
/// links — and applying it again finds nothing left to do.
#[tokio::test(flavor = "multi_thread")]
async fn a_blueprint_with_two_sites_makes_both_with_their_own_links() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    // `fakeservice`'s `packages` row, so its install step plans `Satisfied` and stays offline.
    mixengine_testkit::declare::package(&home.database_file()).await;
    imported(&home, "with-two-sites.toml");

    let directory = repository();
    let into = directory.path().join("shop").display().to_string();
    let apply = [
        "blueprint",
        "apply",
        "with-two-sites",
        "--project",
        "shop",
        "--path",
        &into,
        "--json",
    ];
    let applied = home.mix(&apply);
    assert!(applied.status.success(), "{}", stdout(&applied));

    let (names, links) = site_shown(&home, "shop.test");
    assert_eq!(names, ["shop.test", "www.shop.test"]);
    assert_eq!(links, ["fakeservice@shop"]);

    let (names, links) = site_shown(&home, "docs.shop.test");
    assert_eq!(names, ["docs.shop.test"]);
    assert!(links.is_empty(), "{links:?}");

    let again = stdout(&home.mix(&apply));
    assert!(
        !again.contains("\"result\":\"done\""),
        "a second apply did work the first one should have done: {again}"
    );
}

/// **T204a, D4 — a resumed apply.** Since the first run, the second site was deleted; running the
/// apply again makes only the missing site, under its own names.
#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_apply_makes_only_the_missing_site() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    // `fakeservice`'s `packages` row, so its install step plans `Satisfied` and stays offline.
    mixengine_testkit::declare::package(&home.database_file()).await;
    imported(&home, "with-two-sites.toml");

    let directory = repository();
    let into = directory.path().join("shop").display().to_string();
    let apply = [
        "blueprint",
        "apply",
        "with-two-sites",
        "--project",
        "shop",
        "--path",
        &into,
        "--json",
    ];
    home.mix(&apply);
    let deleted = home.mix(&["site", "delete", "docs.shop.test"]);
    assert!(deleted.status.success(), "{}", stdout(&deleted));

    let again = home.mix(&apply);
    assert!(again.status.success(), "{}", stdout(&again));

    let (names, _) = site_shown(&home, "docs.shop.test");
    assert_eq!(names, ["docs.shop.test"]);
    let (names, _) = site_shown(&home, "shop.test");
    assert_eq!(names, ["shop.test", "www.shop.test"]);
}

/// **T204a, D4 — a name goes back to its own site.** Both sites are there and the first lost its
/// alias; running the apply again gives the alias back to that site — not to whichever site the
/// project's root finds, which with two sites at one root is a guess.
#[tokio::test(flavor = "multi_thread")]
async fn a_missing_name_goes_back_to_its_own_site() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    mixengine_testkit::declare::package(&home.database_file()).await;
    imported(&home, "with-two-sites.toml");

    let directory = repository();
    let into = directory.path().join("shop").display().to_string();
    let apply = [
        "blueprint",
        "apply",
        "with-two-sites",
        "--project",
        "shop",
        "--path",
        &into,
        "--json",
    ];
    home.mix(&apply);
    let removed = home.mix(&["domain", "remove", "www.shop.test"]);
    assert!(removed.status.success(), "{}", stdout(&removed));

    let again = home.mix(&apply);
    assert!(again.status.success(), "{}", stdout(&again));

    let (names, _) = site_shown(&home, "shop.test");
    assert_eq!(names, ["shop.test", "www.shop.test"]);
    let (names, _) = site_shown(&home, "docs.shop.test");
    assert_eq!(names, ["docs.shop.test"]);
}

/// **T204a, D3 — and the fix it carries.** A shared instance that was already here plans
/// `Satisfied`, and the site is still linked to it.
#[tokio::test(flavor = "multi_thread")]
async fn a_service_that_was_already_here_is_still_linked() {
    let home = Home::new();
    let _daemon = home.start_daemon();
    mixengine_testkit::create(
        home.endpoint_ref(),
        &home.database_file(),
        &[Service::new("fakeservice@already")],
    )
    .await;
    imported(&home, "with-a-shared-service.toml");

    let directory = repository();
    let into = directory.path().join("shop").display().to_string();
    let applied = home.mix(&[
        "blueprint",
        "apply",
        "with-a-shared-service",
        "--project",
        "shop",
        "--path",
        &into,
        "--json",
    ]);
    assert!(applied.status.success(), "{}", stdout(&applied));

    let (_, links) = site_shown(&home, "shop.test");
    assert_eq!(links, ["fakeservice@already"]);
}
