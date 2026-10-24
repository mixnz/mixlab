//! The nginx process itself: its spec, the rendering of `nginx.conf`, and the settings it is
//! rendered from.

use super::*;

/// What the file renders to, for `overrides`.
fn conf(overrides: &str) -> String {
    let documents = recipe::render(&Nginx, &context(overrides)).expect("a rendering");

    assert_eq!(documents.len(), 1, "nginx renders one file");
    assert_eq!(documents[0].relative(), Path::new(CONFIG_FILE));

    documents[0].contents().to_owned()
}

/// The spec this recipe builds for `overrides`.
fn spec(overrides: &str) -> ServiceSpec {
    Nginx
        .spec(&context(overrides))
        .expect("a builder")
        .build()
        .expect("a usable spec")
}

/// There is one nginx, which is what stops `service.create` being asked for a second one.
///
/// The same answer as Caddy's and for the same sentence in `docs/features/services.md`:
/// exactly one active front end. What stops a *Caddy* being created beside this one is
/// [`Recipe::role`], which is a different rule about a different mistake.
#[test]
fn nginx_exists_once() {
    assert_eq!(Nginx.instancing(), Instancing::Single);
}

/// And it is a front end, which is the half `service.create` reads.
#[test]
fn nginx_is_a_front_end() {
    assert_eq!(
        Nginx.role(),
        Role::FrontEnd(mixengine_proto::FrontEndServer::Nginx)
    );
}

/// What a failed start is diagnosed against.
///
/// **The status endpoint alone.** The row's own port is what sites are served on, and
/// `nginx.conf` writes no listener for it — a site's own file does — so declaring it would put
/// another program's IIS into the reason for a failure that was not about it.
#[test]
fn the_spec_declares_the_status_endpoint_it_will_bind() {
    assert_eq!(spec("{}").ports(), [2020]);
}

/// An artifact that unpacks and will not run is one the user meets against their own site.
///
/// `-v` and not `-t`: the second reads a configuration, and there is none to read at the moment
/// an archive is being installed.
#[test]
fn nginx_proves_itself_by_running() {
    let smoke = Nginx.smoke_test().expect("a server proves that it runs");

    assert_eq!(smoke.executable, PACKAGE);
    assert_eq!(smoke.args, ["-v"]);
}

#[test]
fn the_rendering_says_what_the_row_and_the_defaults_say() {
    let rendered = conf("{}");

    assert!(rendered.contains("worker_processes 1;"), "{rendered}");
    assert!(rendered.contains("worker_connections 1024;"), "{rendered}");
    assert!(
        rendered.contains("listen 127.0.0.1:2020;"),
        "the status endpoint is not in the file: {rendered}"
    );
    assert!(rendered.contains(HEALTH_PATH), "{rendered}");
    assert!(rendered.contains("client_max_body_size 64m;"), "{rendered}");
    assert!(rendered.contains("include sites/*.conf;"), "{rendered}");

    // The one data file nothing works without, reached by the absolute path the index publishes
    // it at rather than through nginx's own `conf/` — which a generated configuration has none
    // of. Asserted as the whole path, because "contains mime.types" would also be true of a
    // template that joined one itself and got the layout wrong.
    let published = context("{}")
        .provided(MIME_TYPES)
        .expect("a published mime.types")
        .to_string_lossy()
        .replace('\\', "/");
    assert!(
        rendered.contains(&format!("include \"{published}\";")),
        "{rendered}"
    );
}

/// **In the foreground, with its errors on the stream.**
///
/// `daemon off;` is nginx's spelling of the decision `caddy run` is Caddy's: the default forks a
/// master and returns, so what a supervisor would be watching is a launcher that has already
/// exited. `-e stderr` is the other half — an error *before* the configuration has been read
/// goes to the compiled-in `logs/error.log` under the prefix otherwise, which is a file nobody
/// is reading.
#[test]
fn the_program_stays_in_the_foreground_and_says_so_on_the_stream() {
    let rendered = conf("{}");
    assert!(rendered.contains("daemon off;"), "{rendered}");

    let spec = spec("{}");
    let args = spec.args().join(" ");

    assert!(args.contains("-e stderr"), "{args}");
    assert!(args.contains("-c "), "{args}");
    assert!(args.contains("-p "), "{args}");
}

/// **Every path this file writes is forward-slashed and quoted**, which is MariaDB's finding in
/// nginx's spelling: `ngx_conf_read_token` treats `\` inside a quoted string as an escape, so a
/// home under `C:\Users\Nguyen Hai Quang` loses every separator — and unquoted, the directive
/// stops at the space instead. nginx accepts `/` on Windows, so one spelling works on all three
/// systems.
#[test]
fn every_path_in_the_rendering_is_written_the_way_a_windows_path_survives() {
    let rendered = conf("{}");
    let home = root().replace('\\', "/");

    assert!(
        !rendered.contains(root()) || !cfg!(windows),
        "a path reached nginx.conf with backslashes in it: {rendered}"
    );

    for line in rendered.lines().filter(|line| line.contains(&home)) {
        assert!(
            line.matches('"').count() == 2,
            "a path reached nginx.conf outside quotes: {line}"
        );
    }
}

/// The five temp directories nginx makes for itself are children of one that already exists.
///
/// The finding is `mixengine-packages`' and it is the reason this is asserted rather than assumed:
/// nginx creates `client_body_temp` and the four beside it with a **single** `mkdir`, so a missing
/// parent is `[emerg] CreateDirectory() failed (3)` on a configuration that passed `nginx -t` one
/// line earlier. The data directory is made by `Generator::render`; putting the five leaves
/// directly inside it is what makes a `temp/` nobody creates unnecessary.
#[test]
fn every_temp_directory_nginx_makes_has_a_parent_that_already_exists() {
    let context = context("{}");
    let data = context.data().to_string_lossy().replace('\\', "/");
    let rendered = conf("{}");

    for directive in [
        "client_body_temp_path",
        "proxy_temp_path",
        "fastcgi_temp_path",
        "scgi_temp_path",
        "uwsgi_temp_path",
    ] {
        let line = rendered
            .lines()
            .find(|line| line.trim_start().starts_with(directive))
            .unwrap_or_else(|| panic!("{directive} is not in the rendering: {rendered}"));

        let path = line
            .split('"')
            .nth(1)
            .unwrap_or_else(|| panic!("{directive} names no quoted path: {line}"));

        assert_eq!(
            path.rsplit_once('/').map(|(parent, _)| parent),
            Some(data.as_str()),
            "{directive} is not a child of the data directory, which is the one that exists"
        );
    }
}

/// The status endpoint is one value read by four things — the file, the readiness check, the
/// health probe and what a failed start is diagnosed against — so an override that moved it and
/// left one behind would be a service that starts and is never reported up.
#[test]
fn an_override_moves_the_status_endpoint_everywhere_it_is_named() {
    let moved = r#"{"status_port": 2121}"#;
    let rendered = conf(moved);

    assert!(rendered.contains("listen 127.0.0.1:2121;"), "{rendered}");

    let spec = spec(moved);

    assert!(
        matches!(spec.ready(), ReadyCheck::Http { url, .. } if url.contains("127.0.0.1:2121")),
        "{:?}",
        spec.ready()
    );
    assert!(matches!(
        spec.health().map(|health| &health.probe),
        Some(HealthProbe::Http { url, .. }) if url.contains("127.0.0.1:2121")
    ));
    assert_eq!(spec.ports(), [2121]);
}

/// A reload is `-s reload` and a stop is `-s quit`, both through the same configuration the
/// server was started with — which is how either one finds the pid file this instance wrote.
#[test]
fn a_reload_and_a_stop_are_sent_through_this_instances_own_configuration() {
    let spec = spec("{}");
    let config = context("{}").config(CONFIG_FILE).display().to_string();

    let Some(ReloadBehaviour::Command { args, .. }) = spec.reload() else {
        panic!("a reload that is not a command: {:?}", spec.reload());
    };
    assert!(args.contains(&"reload".to_owned()), "{args:?}");
    assert!(args.contains(&config), "{args:?}");

    let StopBehaviour::Command { args, .. } = spec.stop() else {
        panic!("a stop that is not a command: {:?}", spec.stop());
    };
    assert!(args.contains(&"quit".to_owned()), "{args:?}");
    assert!(args.contains(&config), "{args:?}");
}

/// **`nginx.conf` serves nothing on the row's own port**, because a site's own file does.
///
/// A front end that bound 80 here would be serving nothing on it. Caddy says the same thing by
/// writing `http_port` into a global block and binding nothing until a site asks it to.
#[test]
fn the_rendering_listens_on_nothing_a_site_would_be_reached_on() {
    let rendered = conf("{}");

    assert!(
        !rendered.contains("listen 80"),
        "a front end with no sites is listening on the port sites are served on: {rendered}"
    );
}

/// A whole number is what the merge guarantees and a port is what the recipe needs.
#[test]
fn a_number_that_is_not_a_port_is_refused_against_the_setting_that_holds_it() {
    for offered in ["70000", "0", "-1"] {
        let error = Nginx
            .spec(&context(&format!(r#"{{"status_port": {offered}}}"#)))
            .expect_err("a number that is not a port");

        let message = error.to_string();
        assert!(message.contains("status_port"), "{message}");
        assert!(message.contains(offered), "{message}");
    }
}
