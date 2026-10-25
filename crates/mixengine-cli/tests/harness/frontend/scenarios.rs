//! The nginx and Caddy scenarios every front-end suite runs.

use super::*;

/// Caddy, as the suites that need a real front end have to know it.
///
/// **Here rather than in `tests/caddy.rs`**: the idle-footprint budget needs a home
/// with a real Caddy running in it and nothing else, which is this const and `declared` and
/// nothing more. A second copy of it would be a second answer to what version the index
/// publishes and what file the recipe renders.
/// The other program a site can be reached through.
///
/// **Beside `CADDY` rather than in `tests/nginx.rs`**, because it stopped being that suite's alone:
/// `welcome.rs` drives the same arc through both front ends, and a second copy of this table is a
/// second answer to which files an nginx artifact publishes — one forgotten `fastcgi_params` is an
/// nginx that renders and will not start.
pub(crate) const NGINX: FrontEnd = FrontEnd {
    package: "nginx",
    // Where an unpacked nginx is, as the CI step and a developer both set it: the directory holding
    // the binary, which for this package is also the root of the tree `conf/` sits in.
    variable: "MIXENGINE_NGINX_PACKAGE",
    version: "1.x",
    config: "nginx.conf",
    archive: Archive::WholeTree,
    // The data files a generated configuration reaches into the archive for, under the names the
    // recipe asks for them by.
    data_files: &[
        ("mime.types", "conf/mime.types"),
        ("fastcgi_params", "conf/fastcgi_params"),
    ],
    alone: |status| nginx_overrides(status, None),
    serving: |status, port, says| {
        nginx_overrides(
            status,
            // Inside `http { }`, which is where this template renders `extra` — a `server` block at
            // the top level of an nginx configuration is a parse error, and that difference from
            // Caddy is the reason the free-form override is rendered where each format wants it.
            Some(format!(
                "server {{\n        listen 127.0.0.1:{port};\n        \
                 location / {{\n            return 200 \"{says}\";\n        }}\n    }}\n"
            )),
        )
    },
    broken: |status| nginx_overrides(status, Some("this is not nginx {".to_owned())),
    // Inside `http { }`, which is where `include extensions/*.conf;` puts it — the same difference
    // from Caddy the free-form override above is written around.
    fragment: "server {\n    listen {listen}:{fragment_port};\n    location / {\n        \
               return 200 \"from the fragment\";\n    }\n}\n",
    broken_fragment: "notadirective {\n",
    control_line: |status| format!("listen 127.0.0.1:{status};"),
    // The endpoint this recipe renders *because* nginx has no admin one. See the module note on
    // `mixengine_core::generate::recipes::nginx`: a TCP accept cannot tell a serving nginx from one
    // whose workers have all died, because the master holds the listening socket either way.
    control_path: "/mixengine/health",
};

/// The whole overrides document for an nginx on `status`, with `extra` pasted in if there is any.
///
/// **The whole document and not a patch**, which is what `config_overrides_json` is: a setting that
/// is not in it is not set. So every override this suite writes repeats the status port, and one
/// that forgot would move the endpoint back to the recipe's default under a server listening on the
/// one this home chose — a readiness check and a health probe pointed at a port nothing answers on.
fn nginx_overrides(status: u16, extra: Option<String>) -> String {
    serde_json::json!({
        "status_port": status,
        "https_port": free_tls_port(),
        "extra": extra.unwrap_or_default(),
    })
    .to_string()
}

pub(crate) const CADDY: FrontEnd = FrontEnd {
    package: "caddy",
    // Where an unpacked Caddy is, as the CI step and a developer both set it: the directory holding
    // the binary, since `mixengine-packages` publishes Caddy as one executable with nothing around
    // it. That is also what a `packages` row's `install_path` is.
    variable: "MIXENGINE_CADDY_PACKAGE",
    version: "2.x",
    config: "Caddyfile",
    archive: Archive::OneProgram,
    // A Caddyfile includes nothing out of its own archive.
    data_files: &[],
    alone: |admin| overrides(admin, None),
    serving: |admin, port, says| {
        overrides(
            admin,
            Some(format!(
                "http://127.0.0.1:{port} {{\n\trespond \"{says}\"\n}}\n"
            )),
        )
    },
    broken: |admin| overrides(admin, Some("this is not a Caddyfile {".to_owned())),
    // At the top level, which is where `import extensions/*.caddy` puts it.
    fragment: "http://{listen}:{fragment_port} {\n\trespond \"from the fragment\"\n}\n",
    broken_fragment: "notadirective {\n",
    control_line: |admin| format!("admin 127.0.0.1:{admin}"),
    // Caddy's own admin endpoint: `GET /config/` answers `200` with the running configuration, which
    // is a stronger statement than a TCP accept and is what the recipe's readiness check asks.
    control_path: "/config/",
};

/// **A free TLS port, and not the 443 the preset carries.**
///
/// A front end actually binds `https_port`, because a site with a certificate renders a TLS
/// listener. These suites run a real server as an unprivileged user, where 443 is refused — and both
/// servers reject the *whole* configuration over one listener they cannot bind, so the failure is
/// not "no HTTPS" but "the reload was refused and the old configuration is still running". The HTTP
/// port was already a free one for the same reason; this is its other half.
fn free_tls_port() -> u16 {
    free_port()
}
/// The whole overrides document for a Caddy on `admin`, with `extra` pasted in if there is any.
///
/// **The whole document and not a patch**, which is what `config_overrides_json` is: a setting that
/// is not in it is not set. So every override this suite writes repeats the admin port, and one that
/// forgot would move the endpoint back to Caddy's default under a server listening on the one this
/// home chose — a reload and a stop sent to an address nothing answers on.
fn overrides(admin: u16, extra: Option<String>) -> String {
    serde_json::json!({
        "admin_port": admin,
        "https_port": free_tls_port(),
        "extra": extra.unwrap_or_default(),
    })
    .to_string()
}

/// A home with this server **installed** in it, on ports nothing else is using, and a daemon over it.
///
/// The archive is packed out of what the `_services.yml` step fetched, served by a registry that
/// signs its own index, and installed through `package.install` — so this covers the whole package
/// path against a real artifact on all three systems at no extra cost, and the service it then
/// creates is one `service.create` wrote rather than one a fixture inserted.
///
/// The control port is moved off the recipe's default for the reason the site port is chosen rather
/// than fixed: a developer running this suite may well have a Caddy of their own on 2019, and a test
/// that took it over would be a test that stops somebody's work.
pub(crate) async fn declared(
    front: &FrontEnd,
) -> (
    Home,
    super::super::Daemon,
    mixengine_testkit::MockRegistry,
    u16,
    u16,
) {
    let (site, control) = (free_port(), free_port());

    let packed = front.pack();
    let registry = mixengine_testkit::MockRegistry::start(&serde_json::json!({
        "schema": 1, "generated_at": "2026-08-21T06:55:12Z", "packages": []
    }))
    .await;
    let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
    registry.publish(&front.index(&packed, &url));

    let home = Home::new();
    let daemon = home.start_daemon_reading_index(&registry.url(), registry.public_key());

    let installed =
        json(&home.mix(&["package", "install", front.package, front.version, "--json"]));
    assert_eq!(
        installed["state"],
        "succeeded",
        "{installed}
{}",
        home.daemon_log()
    );

    // **No `@`**, which is the instancing rule seen from a recipe that has it: there is one of these,
    // and an id carrying an instance name would be refused here.
    let created = json(&home.mix(&[
        "service",
        "create",
        front.package,
        front.version,
        "--port",
        &site.to_string(),
        "--json",
    ]));
    assert_eq!(
        created["service"]["id"],
        front.package,
        "{created}
{}",
        home.daemon_log()
    );

    mixengine_testkit::declare::reconfigure(
        &home.database_file(),
        front.package,
        &(front.alone)(control),
    )
    .await;

    (home, daemon, registry, site, control)
}

/// What a `[[recipe.front_end]]` fragment says when it is being served.
pub(crate) const FRAGMENT_SAYS: &str = "from the fragment";

/// **An extension's front-end fragment, judged and served by the real server.**
///
/// The claim no unit test can make. A fragment is arbitrary text in the server's own language, so
/// the only thing that can say whether one is a configuration is the program that reads
/// configurations — and the three properties this suite exists for are all about that program:
///
/// 1. a fragment it accepts is **installed and served**, by the process that was already running;
/// 2. a fragment it refuses **stops the install**, before anything is downloaded or written, and
///    leaves the home exactly as it was;
/// 3. an uninstall **takes the fragment with it**, which is the sweep — and it does so by removing
///    the row before anything renders, which is the only way out of a home whose front end has
///    stopped accepting a fragment it once took.
pub(crate) async fn serves_what_an_extension_s_fragment_adds(front: &FrontEnd) {
    let (home, _daemon, _registry, _site_port, control) = declared(front).await;
    let id = front.package;

    let started = json(&home.mix(&["service", "start", id, "--json"]));
    assert_eq!(
        started["complete"],
        true,
        "{started}\n{}",
        home.daemon_log()
    );

    let directory = home.path().join("etc").join(id).join("extensions");

    // --- refused, before anything is fetched ------------------------------------------------------
    //
    // The fragment goes in first while there is still nothing to compare against, so what the
    // assertions below read is a home that never heard of this extension rather than one that
    // recovered.
    let broken = manifest_directory("broken-fragment", front, front.broken_fragment);
    let refused = home.mix(&[
        "extension",
        "install",
        "--path",
        &broken.path().display().to_string(),
        "--yes",
    ]);

    assert!(
        !refused.status.success(),
        "a fragment {id} cannot parse was installed: {}\n{}",
        String::from_utf8_lossy(&refused.stdout),
        home.daemon_log()
    );
    assert!(
        !stdout(&home.mix(&["extension", "list"])).contains("broken-fragment"),
        "a refused install left a row behind\n{}",
        home.daemon_log()
    );
    assert!(
        !directory.join("broken-fragment.conf").exists()
            && !directory.join("broken-fragment.caddy").exists(),
        "a refused install wrote its fragment anyway"
    );

    // --- accepted, and served by the process that was already running -----------------------------
    let good = manifest_directory("fragment", front, front.fragment);
    let installed = home.mix(&[
        "extension",
        "install",
        "--path",
        &good.path().display().to_string(),
        "--yes",
    ]);
    // **Both streams**, because a failed job says why on one of them and which one is `mix`'s to
    // decide: a message printed against an empty `stderr` is a reader left with the daemon log and
    // a guess.
    assert!(
        installed.status.success(),
        "a fragment {id} accepts was refused: {}{}\n{}",
        String::from_utf8_lossy(&installed.stdout),
        String::from_utf8_lossy(&installed.stderr),
        home.daemon_log()
    );

    // **The port the allocator gave it, not the one the manifest asked for.** A wish that was taken
    // is answered with the next free number, and a test reading the wish would be asking a port
    // nothing listens on — see `mixengine_core::services::Port::Allocate`.
    let listed = json(&home.mix(&["extension", "list", "--json"]));
    let port = listed["extensions"][0]["ports"]
        .as_array()
        .and_then(|ports| ports.first())
        .and_then(|port| port["wanted"].as_u64())
        .unwrap_or_else(|| panic!("the extension holds a port: {listed}"));
    let port = u16::try_from(port).expect("a port number");

    let deadline = Instant::now() + EVENTUALLY;
    loop {
        if get(port).is_some_and(|answer| answer.contains(FRAGMENT_SAYS)) {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "{id} never served what the extension's fragment added\n--- rendered ---\n{}\n\
             --- daemon.log ---\n{}",
            std::fs::read_to_string(directory.join(format!(
                "fragment.{}",
                match id == "caddy" {
                    true => "caddy",
                    false => "conf",
                }
            )))
            .unwrap_or_else(|error| format!("no fragment file: {error}")),
            home.daemon_log()
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // --- uninstalled, which is the sweep --------------------------------------------------------
    let removed = home.mix(&["extension", "uninstall", "fragment"]);
    assert!(
        removed.status.success(),
        "an extension whose fragment is live could not be uninstalled, which is the only way out \
         of a home whose front end has stopped accepting one: {}\n{}",
        String::from_utf8_lossy(&removed.stderr),
        home.daemon_log()
    );

    let deadline = Instant::now() + EVENTUALLY;
    loop {
        if !get(port).is_some_and(|answer| answer.contains(FRAGMENT_SAYS)) {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "{id} went on serving a fragment nothing declares any more\n{}",
            home.daemon_log()
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    assert_eq!(
        std::fs::read_dir(&directory)
            .map(|entries| entries.count())
            .unwrap_or_default(),
        0,
        "the uninstalled extension's fragment is still in the front end's configuration"
    );

    // The control endpoint still answers, which says the whole arc happened to a server that was
    // never replaced: what moved was its configuration.
    assert!(
        frontend_control(front, control).is_some_and(|answer| answer.contains(" 200 ")),
        "{id} did not survive the extension it was given\n{}",
        home.daemon_log()
    );
}

/// A directory holding an `extension.toml` for a `recipe` extension carrying one fragment.
///
/// Written out rather than taken from `mixengine-testkit`, on `extension_lifecycle.rs`' rule:
/// `mixengine-core` is not a dependency of `mix` and is not made one for a test.
fn manifest_directory(id: &str, front: &FrontEnd, fragment: &str) -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix("mixengine-fragment")
        .tempdir()
        .expect("a directory for the manifest");

    std::fs::write(
        directory.path().join("extension.toml"),
        format!(
            "schema = 1\n\n\
             [extension]\n\
             id = \"{id}\"\n\
             name = \"A front-end fragment\"\n\
             version = \"1.0.0\"\n\
             kind = \"recipe\"\n\
             description = \"Directives an extension adds to the front end\"\n\n\
             [ports]\n\
             fragment_port = 18081\n\n\
             [[recipe.front_end]]\n\
             server = \"{}\"\n\
             fragment = \"\"\"\n{fragment}\"\"\"\n\n\
             [permissions]\n\
             network = \"loopback\"\n\
             filesystem = [\"own-data\"]\n",
            front.package
        ),
    )
    .expect("the manifest is written");

    directory
}

/// **The whole of what a front end is, in the order a user meets it.**
///
/// One test rather than five, deliberately: each step is the previous one's precondition, and five
/// tests would be five real servers started to re-reach the state this one is already in. What each
/// assertion proves is written beside it.
pub(crate) async fn is_generated_validated_started_reloaded_and_stopped(front: &FrontEnd) {
    let (home, _daemon, _registry, site_port, control) = declared(front).await;
    let id = front.package;

    // --- generated, and judged by the server itself ----------------------------------------------
    //
    // `service start` renders the configuration and runs the recipe's validator over the staged copy
    // before installing it, so a start that completes is a configuration the real program accepted.
    let started = json(&home.mix(&["service", "start", id, "--json"]));
    assert_eq!(
        started["complete"],
        true,
        "{started}\n{}",
        home.daemon_log()
    );

    let config = home.path().join("etc").join(id).join(front.config);
    let rendered = std::fs::read_to_string(&config).expect("the generated configuration");
    assert!(
        rendered.contains(&(front.control_line)(control)),
        "{rendered}"
    );

    // --- started, and proved up by whatever this server answers ----------------------------------
    //
    // The readiness check in the spec is an HTTP request the recipe chose, so a service the daemon
    // reports as running is one that answered it.
    let up = status(&home, id);
    assert_eq!(up["state"], "running", "{up}\n{}", home.daemon_log());
    let pid = up["pid"].as_u64().expect("a running service has a pid");

    assert!(
        get(site_port).is_none(),
        "a front end with no sites answered on the port sites are served on"
    );

    // The one mechanism the two front ends do not share, asked in each one's own spelling: Caddy's
    // admin endpoint, and the `server` block MixEngine renders for nginx because nginx has none.
    // This is what the recipe's readiness check asks, so a `running` service has already answered it
    // — what is added here is that it is answering on the port a *person* was told about.
    assert!(
        frontend_control(front, control).is_some_and(|answer| answer.contains(" 200 ")),
        "{id}'s control endpoint on {control} did not answer 200 on {}:\n{}",
        front.control_path,
        home.daemon_log()
    );

    // --- reloaded ---------------------------------------------------------------------------------
    //
    // A site pasted into the free-form override, and then nothing but a listing: the configuration is
    // rendered at the top of every `service.*` call, and a rendering that moved under a running
    // service is handed to it. Nothing here restarts anything.
    mixengine_testkit::declare::reconfigure(
        &home.database_file(),
        id,
        &(front.serving)(control, site_port, "mixengine reloaded me"),
    )
    .await;

    let listed = json(&home.mix(&["service", "list", "--json"]));
    assert_eq!(listed["services"][0]["state"], "running", "{listed}");

    let deadline = Instant::now() + EVENTUALLY;
    loop {
        if get(site_port).is_some_and(|answer| answer.contains("mixengine reloaded me")) {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "the running {id} never served the site the reload gave it\n--- {} ---\n{}\n\
             --- daemon.log ---\n{}",
            front.config,
            std::fs::read_to_string(&config).unwrap_or_default(),
            home.daemon_log()
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    let reloaded = status(&home, id);
    assert_eq!(
        reloaded["pid"].as_u64(),
        Some(pid),
        "the server was replaced rather than reloaded, which is the cost the whole task avoids: \
         {reloaded}"
    );

    // **No front end MixEngine runs may install a certificate authority** — found while building
    // LAN sharing.
    //
    // Caddy provisions a local CA of its own and installs its root into the user's trust store the
    // first time it starts, unless told not to: `auto_https off` stops it *obtaining* certificates
    // and says nothing about this. Five `Caddy Local Authority` roots were found in
    // `CurrentUser\Root` on the machine this was written on, none of them asked for — and on a CI
    // runner the install blocked for the whole readiness budget with the server half provisioned,
    // because adding to that store wants a consent nobody is there to give.
    //
    // MixEngine reaches a trust store exactly once, through `mixengine-elevate`, for its own
    // authority and with the user's agreement. A second one arriving because a front end's default
    // said so is that design undone, so it is asserted against the running server rather than
    // trusted from the template.
    let said = service_log(&home, id);
    assert!(
        !said.contains("installing root certificate"),
        "{id} installed a certificate authority on this machine, which only \
         `mixengine-elevate` may do\n--- current.log ---\n{said}"
    );

    // --- a site, declared the way a person declares one ------------------------------------------
    //
    // Everything above went through a free-form override, which proves the *reload* and says
    // nothing about sites. This is the claim itself: a `sites` row becomes a file in the front
    // end's own document set, judged by the server's own checker as part of that set, and served by
    // the process that was already running.
    let project = tempfile::Builder::new()
        .prefix("mixengine-site")
        .tempdir()
        .expect("a directory to serve");
    std::fs::write(
        project.path().join("index.html"),
        "<h1>mixengine serves blog.test</h1>\n",
    )
    .expect("something to serve");

    let root = project.path().display().to_string();
    let registered = json(&home.mix(&["project", "create", &root, "--name", "blog", "--json"]));
    assert_eq!(registered["project"]["name"], "blog", "{registered}");

    let declared_site = json(&home.mix(&[
        "site",
        "create",
        "--project",
        "blog",
        "--domain",
        "blog.test",
        "--kind",
        "static",
        "--json",
    ]));
    assert_eq!(
        declared_site["site"]["site"]["domain"],
        "blog.test",
        "{declared_site}\n{}",
        home.daemon_log()
    );

    let sites = home.path().join("etc").join(id).join("sites");
    assert!(
        std::fs::read_dir(&sites)
            .expect("the sites directory")
            .filter_map(std::result::Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with("blog.test")),
        "no file was rendered for the site\n{}",
        home.daemon_log()
    );

    // The `Host` header and not the name: CI has no elevation, so no hosts entry exists — and what is
    // under test is the rendering and the server reading it, not resolution.
    let deadline = Instant::now() + EVENTUALLY;
    loop {
        let answer = request_as(site_port, "/", "blog.test");

        if answer
            .as_deref()
            .is_some_and(|body| body.contains("mixengine serves blog.test"))
        {
            break;
        }

        // The rendering and the answer, both, because either one alone leaves the reader guessing: a
        // file that was never written and a file the server would not match look identical from out
        // here.
        assert!(
            Instant::now() < deadline,
            "{id} never served the site it was given\n--- answered ---\n{}\n--- rendered ---\n{}\n\
             --- daemon.log ---\n{}",
            answer.unwrap_or_else(|| "nothing at all".to_owned()),
            rendered_sites(&home, id),
            home.daemon_log()
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // --- stopped, which is the sweep and the reload together --------------------------------------
    //
    // The sweep and the reload in one step: the flag goes down, the file goes with it because
    // `sites/` holds exactly what was rendered into it, and the removal is what makes the walk
    // count as changed — without that the site would go on being served by a server nobody told.
    let stopped_site = json(&home.mix(&["site", "stop", "blog.test", "--json"]));
    assert_eq!(stopped_site["site"]["state"], "disabled", "{stopped_site}");

    let deadline = Instant::now() + EVENTUALLY;
    loop {
        let answer = request_as(site_port, "/", "blog.test");

        if !answer.is_some_and(|body| body.contains("mixengine serves blog.test")) {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "{id} went on serving a site nothing declares any more\n{}",
            home.daemon_log()
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // And the *other* site — the one pasted into the free-form override — is untouched, which is what
    // says the sweep took the site file and nothing around it.
    //
    // **Waited for rather than asked once**, and the loop above is why. `request_as` reports a
    // connection it could not make and a site that is gone identically, so the wait for `blog.test`
    // to stop being served can be satisfied by the reload itself rather than by its result — and an
    // assertion made in that instant reads a server mid-reload as a server that swept both sites.
    //
    // **Everything the server was given and everything it said, in the failure.** Run 35665122912
    // (services, windows-latest, web) failed here thirty seconds after the reload had been logged,
    // with only the daemon's log to read — which cannot tell a `404`, a closed connection and a
    // configuration missing the override apart.
    let deadline = Instant::now() + EVENTUALLY;
    loop {
        let answer = get(site_port);
        if answer
            .as_deref()
            .is_some_and(|body| body.contains("mixengine reloaded me"))
        {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "the sweep took more than the site it was about\n--- answered ---\n{}\n--- {} ---\n{}\n\
             --- rendered ---\n{}\n--- current.log ---\n{}\n--- daemon.log ---\n{}",
            answer.unwrap_or_else(|| "nothing at all".to_owned()),
            front.config,
            std::fs::read_to_string(&config).unwrap_or_default(),
            rendered_sites(&home, id),
            service_log(&home, id),
            home.daemon_log()
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // --- refused, with the last good configuration still live -------------------------------------
    //
    // The half of validation that matters. The server's own checker refuses the staged rendering, so
    // nothing is installed — and the process goes on serving what it was serving, which is what a
    // user whose typo would otherwise have taken every site on the machine down needs to be true.
    mixengine_testkit::declare::reconfigure(&home.database_file(), id, &(front.broken)(control))
        .await;

    let refused = home.mix(&["service", "list", "--json"]);
    assert!(
        !refused.status.success(),
        "a configuration {id} cannot parse was accepted: {}",
        String::from_utf8_lossy(&refused.stdout)
    );

    assert!(
        std::fs::read_to_string(&config)
            .expect("the configuration is still there")
            .contains("mixengine reloaded me"),
        "the refused rendering was installed anyway"
    );

    // **Waited for, and this was the one read in the whole arc that was not.** A refused rendering
    // installs nothing and reloads nothing, so a site it did not take down is answering already and
    // the first attempt returns — while a one-shot connect on a loaded runner reports "not there"
    // for reasons that have nothing to do with any configuration, which is what failed a macOS leg
    // on 2026-08-28. **The wait cannot hide a real failure of this property**: nothing is coming to
    // restore a site the refusal did take down — no reload was ordered and none will be — so a site
    // that is gone is gone for the whole of `EVENTUALLY` and the assertion still fails.
    let deadline = Instant::now() + EVENTUALLY;
    loop {
        if get(site_port).is_some_and(|answer| answer.contains("mixengine reloaded me")) {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "a configuration that was never installed stopped the site that was being served\n{}",
            home.daemon_log()
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // --- stopped ----------------------------------------------------------------------------------
    //
    // The spec's `StopBehaviour::Command`, which is each server's own way of being asked. The port
    // going quiet is what says the process really went, rather than the row having been written.
    mixengine_testkit::declare::reconfigure(
        &home.database_file(),
        id,
        &(front.serving)(control, site_port, "mixengine reloaded me"),
    )
    .await;

    let stopped = json(&home.mix(&["service", "stop", id, "--json"]));
    assert_eq!(
        stopped["complete"],
        true,
        "{stopped}\n{}",
        home.daemon_log()
    );
    assert_eq!(status(&home, id)["state"], "stopped");

    let deadline = Instant::now() + EVENTUALLY;
    while get(site_port).is_some() {
        assert!(
            Instant::now() < deadline,
            "something is still serving the site port after {id} was stopped\n--- daemon.log ---\n{}",
            home.daemon_log()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Every file in this front end's `sites/` directory, named and quoted, or why there are none.
///
/// What a "the site was not served" failure needs beside the daemon's own log: the file the server
/// was given, which is the difference between a rendering that never happened and one the server
/// would not match.
fn rendered_sites(home: &Home, id: &str) -> String {
    let directory = home.path().join("etc").join(id).join("sites");

    let Ok(entries) = std::fs::read_dir(&directory) else {
        return format!("{} does not exist", directory.display());
    };

    entries
        .filter_map(std::result::Result::ok)
        .map(|entry| {
            format!(
                "--- {} ---\n{}",
                entry.path().display(),
                std::fs::read_to_string(entry.path())
                    .unwrap_or_else(|error| format!("unreadable: {error}"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// This front end's control endpoint, asked whatever it answers on.
fn frontend_control(front: &FrontEnd, control: u16) -> Option<String> {
    request(control, front.control_path)
}

/// What `mix service status <id>` says.
pub(crate) fn status(home: &Home, id: &str) -> Value {
    json(&home.mix(&["service", "status", id, "--json"]))
}
