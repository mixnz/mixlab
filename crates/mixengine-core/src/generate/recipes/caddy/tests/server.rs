//! The Caddy process: its spec, its admin endpoint and the Caddyfile.

use super::*;

/// What a failed start is diagnosed against.
///
/// **The admin endpoint alone, and that is not an omission.** `http_port` and `https_port` are
/// written into the global block, but Caddy binds neither until a site tells it to — so a Caddy
/// with no site that failed to start never wanted 80. Declaring one anyway would put another
/// program's IIS into the reason for a failure that was not about it.
#[test]
fn the_spec_declares_the_admin_endpoint_it_will_bind() {
    let context = context("{}");
    let spec = Caddy
        .spec(&context)
        .expect("a spec")
        .build()
        .expect("a valid spec");

    assert_eq!(spec.ports(), [2019]);
}

/// There is one Caddy, which is what stops `service.create` being asked for a second Caddy.
#[test]
fn caddy_exists_once() {
    assert_eq!(Caddy.instancing(), Instancing::Single);
}

/// And what stops it being asked for an nginx beside this one is the other answer. Two recipes,
/// one job; [`Instancing`] is about a package and cannot say it.
#[test]
fn caddy_is_a_front_end() {
    assert_eq!(
        Caddy.role(),
        Role::FrontEnd(mixengine_proto::FrontEndServer::Caddy)
    );
}

/// An artifact that unpacks and will not run is one the user meets against their own site, which is
/// what the first real package showed and the reason `Installer::install` takes a smoke test at
/// all.
///
/// `caddy version` and not `caddy --version`: Caddy's is a subcommand, and the flag that is not
/// one exits non-zero — which would fail every install of a perfectly good archive.
#[test]
fn caddy_proves_itself_by_running() {
    let smoke = Caddy.smoke_test().expect("a server proves that it runs");

    assert_eq!(smoke.executable, PACKAGE);
    assert_eq!(smoke.args, ["version"]);
}

/// What the file renders to, for `overrides`.
fn caddyfile(overrides: &str) -> String {
    let documents = recipe::render(&Caddy, &context(overrides)).expect("a rendering");

    assert_eq!(documents.len(), 1, "Caddy renders one file");
    assert_eq!(documents[0].relative(), Path::new(CADDYFILE));

    documents[0].contents().to_owned()
}

#[test]
fn the_rendering_says_what_the_row_and_the_defaults_say() {
    let rendered = caddyfile("{}");

    assert!(rendered.contains("admin 127.0.0.1:2019"), "{rendered}");
    assert!(rendered.contains("http_port 80"), "{rendered}");
    assert!(rendered.contains("https_port 443"), "{rendered}");
    assert!(rendered.contains("auto_https off"), "{rendered}");
    assert!(rendered.contains("import sites/*.caddy"), "{rendered}");

    // Not a preference. Without it Caddy writes the configuration it last loaded to the user's
    // own config directory and reads it back on the next start, which is both a write outside
    // MIXENGINE_HOME and a second source of truth for a file rendered from the database.
    assert!(rendered.contains("persist_config off"), "{rendered}");

    // **The two lines that keep a front end out of the user's trust store.** `auto_https off`
    // above stops Caddy obtaining certificates and says nothing about its own local CA, whose
    // root it installs on first provisioning — silently, and on Windows blocking on a consent
    // nobody is there to give. MixEngine reaches a trust store once, through
    // `mixengine-elevate`, for its own authority; a second one arriving by default is that
    // design undone.
    //
    // **The `pki` block is not decoration.** The adapter applies `skip_install_trust` only to
    // authorities the configuration names, and names none on its own — so the option without
    // the block adapts to a file with no `pki` app in it and changes nothing. Both are asserted
    // here because shipping only the first one is the mistake that was actually made.
    assert!(rendered.contains("skip_install_trust"), "{rendered}");
    assert!(rendered.contains("pki {"), "{rendered}");
    assert!(rendered.contains("ca local"), "{rendered}");
}

/// **Every path this file writes has to survive being a Windows path**, which is the finding
/// that decided how they are quoted: a Caddyfile token in double quotes treats `\"` and `\\` as
/// escapes, so `C:\srv\caddy\` ends its string one character early and the parse error names the
/// line after it. Inside backticks nothing is an escape.
///
/// Asserted on the rendering rather than trusted from the template, because the two ways to lose
/// this are a quote character edited in the template and a path interpolated somewhere new.
#[test]
fn every_path_in_the_rendering_is_quoted_the_way_a_windows_path_survives() {
    let rendered = caddyfile("{}");

    for line in rendered.lines().filter(|line| line.contains(root())) {
        let quoted = line.trim();

        assert!(
            quoted.ends_with('`') && quoted.matches('`').count() == 2,
            "a path reached the Caddyfile outside backticks: {line}"
        );
    }

    assert!(
        rendered.contains(&format!("`{}`", context("{}").data().display())),
        "the storage directory is not the one the row names: {rendered}"
    );
}

/// The admin endpoint is one value read by four things — the file, the readiness check, the
/// health probe and both commands — so an override that moved it and left one of them behind
/// would be a service that starts and can never be stopped.
#[test]
fn an_override_moves_the_admin_endpoint_everywhere_it_is_named() {
    let rendered = caddyfile(r#"{"admin_port": 2020}"#);
    assert!(rendered.contains("admin 127.0.0.1:2020"), "{rendered}");

    let spec = Caddy
        .spec(&context(r#"{"admin_port": 2020}"#))
        .expect("a builder")
        .build()
        .expect("a usable spec");

    assert!(matches!(spec.ready(), ReadyCheck::Http { url, .. } if url.contains("127.0.0.1:2020")));
    assert!(matches!(
        spec.health().map(|health| &health.probe),
        Some(HealthProbe::Http { url, .. }) if url.contains("127.0.0.1:2020")
    ));
    assert!(matches!(
        spec.stop(),
        StopBehaviour::Command { args, .. } if args.contains(&"127.0.0.1:2020".to_owned())
    ));
    assert!(matches!(
        spec.reload(),
        Some(ReloadBehaviour::Command { args, .. })
            if args.contains(&"127.0.0.1:2020".to_owned())
    ));
}

/// **`run`, and not `start`.** `caddy start` spawns a child, hands it the parent's stdout and
/// returns — so what the supervisor would be watching is a launcher that has already exited,
/// and what it would be capturing is a pipe the server holds open for as long as it serves.
#[test]
fn the_program_is_the_one_that_serves_rather_than_the_one_that_launches_it() {
    let spec = Caddy
        .spec(&context("{}"))
        .expect("a builder")
        .build()
        .expect("a usable spec");

    assert_eq!(spec.args().first().map(String::as_str), Some("run"));
    assert!(
        spec.program()
            .ends_with(format!("{PACKAGE}{}", std::env::consts::EXE_SUFFIX))
    );
    assert!(
        spec.args().contains(&CADDYFILE.to_owned())
            || spec.args().iter().any(|arg| arg.ends_with(CADDYFILE)),
        "{:?}",
        spec.args()
    );
}

/// **`--force`, and the reason is that the line a reissue changes is a comment.**
///
/// `site.caddy` carries `# Certificate sha256:…` so that a reissued certificate renders a file
/// that *differs* — without it `document::install` compares byte-identical bytes, finds no
/// change, and never asks anybody to re-read anything. That half works. The other half is that
/// `caddy reload` adapts the Caddyfile to JSON, **and the adapter throws comments away**: the
/// configuration Caddy is handed is identical to the one it is already running, so its admin
/// endpoint skips the load and the process goes on serving the certificate it holds in memory.
///
/// Measured, not read: CI's `system` job rotated the authority on Windows and macOS, and both
/// legs found the server still presenting the leaf signed by the authority that had just been
/// replaced — `problem: served_certificate_differs`, `trust: rejected`. `caddy reload --help`
/// on the pinned 2.11.4 names the flag for exactly this: *Force config reload, even if it is
/// the same*.
///
/// It costs nothing on the ordinary path. A reload is only ever asked for after the rendering
/// changed, so "even if it is the same" is a statement about Caddy's view of the file and never
/// about ours.
#[test]
fn a_reload_is_forced_because_the_line_a_reissued_certificate_changes_is_a_comment() {
    let spec = Caddy
        .spec(&context("{}"))
        .expect("a builder")
        .build()
        .expect("a usable spec");

    let Some(ReloadBehaviour::Command { args, .. }) = spec.reload() else {
        panic!("a front end that cannot be reloaded drops every connection to edit one site");
    };

    assert!(
        args.contains(&"--force".to_owned()),
        "a certificate is reissued to the same path, so the only thing that changes in this \
             file is a comment the adapter removes: {args:?}"
    );
}

/// A whole number is what the merge guarantees and a port is what the recipe needs, so this is
/// the recipe's own refusal rather than the merge's — and the message has to name the setting,
/// because the alternative is a URL that will not parse being reported hours later as a service
/// that never came up.
#[test]
fn a_number_that_is_not_a_port_is_refused_against_the_setting_that_holds_it() {
    for offered in ["70000", "0", "-1"] {
        let error = Caddy
            .spec(&context(&format!(r#"{{"admin_port": {offered}}}"#)))
            .expect_err("a number that is not a port");

        let message = error.to_string();
        assert!(message.contains("admin_port"), "{message}");
        assert!(message.contains(offered), "{message}");
    }
}

/// A row with no port is a front end answering on 80, which is what the `services` schema means
/// by a nullable `port` for a recipe that names no preferred one — and 80 is still written into
/// the global block, because leaving it out hands Caddy its own default *unmapped*. A machine on
/// which the front end binds 8080 to answer on 80 then had a Caddy refusing to start with
/// `bind: permission denied` on 127.0.0.1:80, for a row that said exactly what the design
/// intends. Measured on macOS, where `https_port 8443` was rendered beside no `http_port` at all.
#[test]
fn a_row_with_no_port_answers_on_eighty_and_binds_what_this_system_maps_it_to() {
    let portless = || {
        let service = ServiceId::parse("caddy").expect("an id");
        let settings = Settings::merge(Caddy.settings(), "{}", &service).expect("defaults");
        Context::for_test(
            service,
            PACKAGE,
            Path::new(root()),
            BTreeMap::new(),
            None,
            settings,
        )
    };

    let direct = recipe::render(&Caddy, &portless()).expect("a rendering");
    let plain = direct[0].contents();

    assert!(plain.contains("http_port 80"), "{plain}");
    assert!(plain.contains("https_port 443"), "{plain}");

    let redirected = portless().with_bindings(vec![
        PortBinding {
            answer: 80,
            bind: 8080,
        },
        PortBinding {
            answer: 443,
            bind: 8443,
        },
    ]);
    let mapped = recipe::render(&Caddy, &redirected).expect("a rendering");
    let rendered = mapped[0].contents();

    assert!(rendered.contains("http_port 8080"), "{rendered}");
    assert!(rendered.contains("https_port 8443"), "{rendered}");
}
