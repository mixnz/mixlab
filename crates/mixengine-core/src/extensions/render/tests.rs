use std::net::Ipv4Addr;
use std::path::Path;

use super::*;
use crate::config::PathOverrides;
use crate::extensions::manifest::{self, Body};

/// A context with a database and a secret behind it, as `extensions::config` builds one.
fn with_a_database() -> Context {
    Context {
        install_dir: Path::new(root()).join("extensions").join("pma"),
        data_dir: Path::new(root()).join("data").join("pma"),
        ports: BTreeMap::new(),
        listen: IpAddr::V4(Ipv4Addr::LOCALHOST),
        database: Some(DatabaseEndpoint {
            service: mixengine_proto::ServiceId::parse("mariadb@main").expect("a service id"),
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 3306,
            user: "root".to_owned(),
        }),
        secret: Some("s".repeat(32)),
        password_env: None,
    }
}

/// **`{db_password_env}` renders the one name this system uses.** A name and never a password: what
/// the pool carries is an `EnvValue::Keyring`, and what the generated file carries is the variable
/// to read it out of.
#[test]
fn the_password_variable_renders_its_name_and_never_a_password() {
    let mut context = with_a_database();
    context.password_env = Some(CREDENTIAL_ENV.to_owned());

    let rendered = php_source(
        &an_id(),
        "web-app.config.text",
        "$cfg['p'] = getenv('{db_password_env}');",
        &context,
    )
    .expect("it renders");

    assert_eq!(rendered, format!("$cfg['p'] = getenv('{CREDENTIAL_ENV}');"));
}

/// And a context that was never given one refuses it, naming the field that supplies it — which
/// is a *manifest* field one line up the same file, where the other four are a property of this
/// home.
#[test]
fn the_password_variable_needs_the_manifest_to_have_asked_for_it() {
    let refusal = php_source(
        &an_id(),
        "web-app.config.text",
        "$cfg['p'] = getenv('{db_password_env}');",
        &with_a_database(),
    )
    .expect_err("nothing answers it");

    let said = refusal.to_string();
    assert!(said.contains("db_password_env"), "{said}");
    assert!(said.contains("signs_in"), "{said}");
}

/// An absolute root on whichever system this is compiled for.
fn root() -> &'static str {
    if cfg!(windows) { r"C:\home" } else { "/home" }
}

/// An id for a rendering that is not about which extension it is.
fn an_id() -> ExtensionId {
    ExtensionId::parse("pma").expect("an extension id")
}

/// **PHP is made of braces.**
///
/// [`Destination::Field`] refuses a `{…}` it does not recognise, and is right to: a `{home_dir}`
/// surviving into an argument is a literal brace handed to a program. A `config.inc.php` is not
/// that document — `function f() {` is a brace before the first placeholder — so this is a
/// fragment's answer arriving a second time, including the part two unit tests missed and the real
/// server caught: the scan continues **one character on**, never past the `}` it was looking for.
#[test]
fn php_source_keeps_its_own_braces_and_substitutes_ours() {
    let rendered = php_source(
        &an_id(),
        "web-app.config.text",
        "<?php\nfunction f() {\n  $c['host'] = '{db_host}';\n  $c['port'] = {db_port};\n}\n",
        &with_a_database(),
    )
    .expect("it renders");

    assert!(rendered.contains("function f() {"), "{rendered}");
    assert!(rendered.contains("$c['host'] = '127.0.0.1';"), "{rendered}");
    assert!(rendered.contains("$c['port'] = 3306;"), "{rendered}");
    assert!(rendered.contains("\n}\n"), "{rendered}");
}

/// A path substituted into PHP is forward-slashed: PHP accepts `/` on Windows, and a backslash
/// inside a single-quoted PHP string is a character the reader has to reason about — `'\n'` is
/// two characters there and one in the next quote style along.
#[test]
fn a_path_substituted_into_php_source_is_forward_slashed() {
    let rendered = php_source(
        &an_id(),
        "web-app.config.text",
        "@include '{data_dir}/config.user.php';",
        &with_a_database(),
    )
    .expect("it renders");

    assert!(!rendered.contains('\\'), "{rendered}");
    assert!(rendered.ends_with("/config.user.php';"), "{rendered}");
}

/// **The one thing this destination still refuses**: a placeholder of ours with nothing behind
/// it. Left standing it would reach PHP as a literal `{db_host}` and connect to a host called
/// that, which is reported as a database that is down.
#[test]
fn a_database_placeholder_with_no_database_is_refused() {
    let mut context = with_a_database();
    context.database = None;
    context.secret = None;

    for template in ["'{db_host}'", "'{db_port}'", "'{db_user}'", "'{secret}'"] {
        let refusal = php_source(&an_id(), "web-app.config.text", template, &context)
            .expect_err("nothing answers it");

        assert!(
            matches!(&refusal, Error::Extension(ExtensionError::ExtensionField { field, .. }) if field == "web-app.config.text"),
            "{template}: {refusal}"
        );
    }
}

/// The whole of the address rule, proved by reading what came out rather than by trusting what went
/// in — the first planner's method for "never data, credentials or absolute paths".
#[test]
fn a_loopback_extension_renders_no_other_address() {
    let spec = render(mixengine_testkit::extension::MAILPIT).expect("renders");

    let mut written = spec.args().join(" ");
    if let ReadyCheck::Tcp { addr, .. } = spec.ready() {
        written.push(' ');
        written.push_str(&addr.to_string());
    }

    for token in written.split_whitespace() {
        let host = token.rsplit_once(':').map_or(token, |(host, _)| host);
        if let Ok(address) = host.parse::<IpAddr>() {
            assert_eq!(
                address,
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                "a loopback extension rendered {address}"
            );
        }
    }
}

/// Ports come out of `[ports]` and paths out of the context, so the rendered spec is what
/// would run rather than a description of it.
#[test]
fn the_rendered_spec_is_what_would_run() {
    let spec = render(mixengine_testkit::extension::MAILPIT).expect("renders");

    assert_eq!(spec.id().as_str(), "mailpit");
    assert!(spec.program().is_absolute());
    assert!(spec.args().contains(&"127.0.0.1:8025".to_owned()));
    assert!(spec.args().contains(&"127.0.0.1:1025".to_owned()));
    assert!(spec.cwd().ends_with("mailpit"));
}

/// A manifest writes `{install_dir}/mailpit` with the slash every author types, and the
/// placeholder renders with this system's separator. The rendered *path* uses one of them, not
/// both — an argument is left as it will be passed, but a path is MixEngine's to spell.
#[test]
fn a_rendered_path_uses_one_separator() {
    let spec = render(mixengine_testkit::extension::MAILPIT).expect("renders");

    let mut written = vec![spec.program().display().to_string()];
    written.extend(spec.args().iter().cloned());

    for shown in written {
        // `--listen` and the address beside it carry no separator at all, so they pass this by
        // having nothing to say; what it is really asserting is the two arguments that begin at
        // a placeholder.
        assert!(
            shown
                .chars()
                .filter(|character| *character == '/' || *character == '\\')
                .all(|character| character == std::path::MAIN_SEPARATOR),
            "{shown} mixes separators"
        );
    }
}

/// **The data directory is not inside the install directory.**
///
/// An uninstall removes `install_dir` whole and keeps what a person accumulated unless they ask
/// otherwise, and a `data` nested inside it would make that promise unkeepable. The alternative
/// — deleting everything under the install directory except one child — is a rule somebody has
/// to remember at every future site that removes an extension.
#[test]
fn data_outlives_the_install_it_came_with() {
    let home = tempfile::tempdir().expect("a directory");
    let paths = Paths::new(home.path().to_path_buf(), &PathOverrides::default());
    let file = manifest::read(
        Path::new("probe").join(manifest::FILE_NAME).as_path(),
        mixengine_testkit::extension::MAILPIT,
    )
    .expect("a manifest");

    let context = Context::planned(&paths, &file);

    assert!(
        !context.data_dir.starts_with(&context.install_dir),
        "data at {} sits inside the install at {}",
        context.data_dir.display(),
        context.install_dir.display()
    );
    assert_eq!(
        context.data_dir,
        paths.data().join("extensions").join("mailpit")
    );
}

/// `lan` is the other rendering, and there is no third.
#[test]
fn lan_renders_every_interface() {
    let text = mixengine_testkit::extension::MAILPIT
        .replace("network = \"loopback\"", "network = \"lan\"");

    let spec = render(&text).expect("renders");

    assert!(spec.args().contains(&"0.0.0.0:8025".to_owned()));
}

/// **The refusal table.** Every row is a way of writing something the manifest may not say,
/// and every one is refused with the field named.
///
/// Each row is a whole `[service]` table rather than one line added to a default, because half
/// of them replace `program` or `cwd` — appended, those would be a duplicate TOML key and the
/// test would pass on the wrong refusal.
#[test]
fn the_manifest_may_not_say_these() {
    let rows: [(&str, &str); 9] = [
        (
            "a written-out loopback address",
            "args = [\"--listen\", \"127.0.0.1:{ui_port}\"]",
        ),
        (
            "every interface, written out",
            "args = [\"--listen\", \"0.0.0.0:{ui_port}\"]",
        ),
        (
            "a host name",
            "args = [\"--listen\", \"localhost:{ui_port}\"]",
        ),
        (
            "an IPv6 address",
            "args = [\"--listen\", \"[::1]:{ui_port}\"]",
        ),
        (
            "an address hidden in an environment value",
            "[service.env]\nBIND = \"127.0.0.1\"",
        ),
        ("an absolute program", "program = \"/usr/local/bin/x\""),
        ("a relative program", "program = \"x\""),
        ("a path climbing out", "cwd = \"{data_dir}/../../etc\""),
        (
            "an unknown placeholder",
            "args = [\"--home\", \"{home_dir}\"]",
        ),
    ];

    for (what, extra) in rows {
        let outcome = render(&probe(extra));

        assert!(
            matches!(
                outcome,
                Err(Error::Extension(ExtensionError::ExtensionField { .. }))
            ),
            "{what} should be refused, got {outcome:?}"
        );
    }
}

/// A url is the same escape by another door.
#[test]
fn a_ready_url_may_not_name_a_host() {
    let outcome = render(&probe(
        "ready = { type = \"http\", url = \"http://127.0.0.1:{ui_port}/health\", expect_status = 200, timeout = \"10s\" }",
    ));

    assert!(matches!(
        outcome,
        Err(Error::Extension(ExtensionError::ExtensionField { .. }))
    ));
}

/// And `{listen}` in a url is how the same check is written correctly.
#[test]
fn a_ready_url_may_carry_the_placeholder() {
    let outcome = render(&probe(
        "ready = { type = \"http\", url = \"http://{listen}:{ui_port}/health\", expect_status = 200, timeout = \"10s\" }",
    ));

    assert!(outcome.is_ok(), "{outcome:?}");
}

/// A manifest whose rendering is not a usable spec is reported against the extension, not
/// against a spec nobody wrote. `restart` is the one field an author may state that the
/// supervisor will refuse — every other rule here is stricter than `validate`'s.
#[test]
fn an_unusable_rendering_names_the_extension() {
    let outcome = render(&probe(
        "restart = { type = \"on_failure\", max_retries = 0, window = \"5m\", backoff = { initial = \"1s\", max = \"30s\", multiplier_percent = 200, jitter_percent = 10 } }",
    ));

    assert!(
        matches!(
            outcome,
            Err(Error::Extension(ExtensionError::ExtensionSpec { .. }))
        ),
        "{outcome:?}"
    );
}

/// A whole manifest for a `service` extension, whose `[service]` table is `extra` plus
/// whichever of `program`, `cwd` and `ready` `extra` did not itself supply.
fn probe(extra: &str) -> String {
    let mut body = String::new();

    for (key, line) in [
        ("program", "program = \"{install_dir}/x\""),
        ("cwd", "cwd = \"{data_dir}\""),
        ("ready", "ready = { type = \"pid_alive\", settle = \"1s\" }"),
        ("restart", ""),
    ] {
        if !line.is_empty() && !extra.starts_with(key) {
            body.push_str(line);
            body.push('\n');
        }
    }

    format!(
        "schema = 1\n\n[extension]\nid = \"probe\"\nname = \"Probe\"\nversion = \"1.0.0\"\nkind = \"service\"\n\n[ports]\nui_port = 8025\n\n[service]\n{body}{extra}\n"
    )
}

/// A context pointing at a home under `root`, for the fragment tests.
fn probe_context(root: &Path) -> (ExtensionId, Context) {
    let id = ExtensionId::parse("probe").expect("an id");
    let context = Context {
        install_dir: root.join("extensions").join("probe"),
        data_dir: root.join("data"),
        ports: BTreeMap::from([("ui_port".to_owned(), 8025)]),
        listen: IpAddr::V4(Ipv4Addr::LOCALHOST),
        database: None,
        secret: None,
        password_env: None,
    };

    (id, context)
}

/// nginx eats a backslash, so a path substituted into an nginx fragment is forward-slashed whatever
/// this system spells one with; the same path bound for a Caddyfile is left as written, because a
/// backtick-quoted token takes it as it is.
#[test]
fn a_fragment_spells_a_path_the_way_its_server_does() {
    let home = tempfile::tempdir().expect("a directory");
    let (id, context) = probe_context(home.path());

    let nginx = fragment(
        &id,
        "recipe.front_end[0]",
        "root \"{install_dir}/public\";",
        &context,
        Destination::NginxConf,
    )
    .expect("an nginx fragment renders");

    assert!(!nginx.contains('\\'), "{nginx}");

    let caddy = fragment(
        &id,
        "recipe.front_end[1]",
        "root * `{install_dir}/public`",
        &context,
        Destination::Caddyfile,
    )
    .expect("a caddy fragment renders");

    assert!(
        caddy.contains(&format!("probe{}public", std::path::MAIN_SEPARATOR)),
        "{caddy}"
    );
}

/// **A brace is the destination language's punctuation.** A block, a Caddy placeholder and an nginx
/// variable all pass through untouched, and a backslash that is not part of a substituted path
/// stays the author's: `location ~ \.php$` means something.
#[test]
fn a_fragment_leaves_the_destination_s_own_syntax_alone() {
    let home = tempfile::tempdir().expect("a directory");
    let (id, context) = probe_context(home.path());

    let nginx = fragment(
        &id,
        "recipe.front_end[0]",
        "location ~ \\.php$ { return 404; }",
        &context,
        Destination::NginxConf,
    )
    .expect("an nginx fragment renders");

    assert_eq!(nginx, "location ~ \\.php$ { return 404; }");

    let caddy = fragment(
        &id,
        "recipe.front_end[1]",
        "handle { header_up Host {host} }",
        &context,
        Destination::Caddyfile,
    )
    .expect("a caddy fragment renders");

    assert_eq!(caddy, "handle { header_up Host {host} }");
}

/// **A placeholder inside a block is still a placeholder** — found by a real nginx.
///
/// The nearest `}` after a block's `{` is nowhere near it: in `server {\n    listen {listen}…`
/// it is the closing brace of `{listen}`. A renderer that consumed through it swallowed the
/// whole span, re-emitted it verbatim, and left `{listen}` in the file — reported by nginx as a
/// `listen` directive with no `;`, four lines from anything that looks wrong.
///
/// **Two unit tests missed this**, and the reason is worth keeping: re-emitting the swallowed
/// text is lossless whenever nothing inside it needed substituting, so a fragment that opens its
/// block *after* its placeholders — which Caddy's does — renders correctly by luck.
#[test]
fn a_placeholder_inside_a_block_is_substituted() {
    let home = tempfile::tempdir().expect("a directory");
    let (id, context) = probe_context(home.path());

    let rendered = fragment(
        &id,
        "recipe.front_end[0]",
        "server {\n    listen {listen}:{ui_port};\n}\n",
        &context,
        Destination::NginxConf,
    )
    .expect("an nginx fragment renders");

    assert_eq!(rendered, "server {\n    listen 127.0.0.1:8025;\n}\n");
}

/// And a field whose whole content *is* this vocabulary keeps the refusal it always had: an
/// unknown placeholder there is a literal brace handed to a program.
#[test]
fn a_field_still_refuses_a_placeholder_it_does_not_know() {
    let home = tempfile::tempdir().expect("a directory");
    let (id, context) = probe_context(home.path());

    assert!(matches!(
        value(&id, "service.args[0]", "{home_dir}/x", &context),
        Err(Error::Extension(ExtensionError::ExtensionField { .. }))
    ));
}

/// A fragment lands in the configuration of the process every site is reached through, so it is
/// held to the address rule an argument is held to.
#[test]
fn a_fragment_may_not_write_out_an_address() {
    let home = tempfile::tempdir().expect("a directory");
    let (id, context) = probe_context(home.path());

    assert!(matches!(
        fragment(
            &id,
            "recipe.front_end[0]",
            "proxy_pass http://127.0.0.1:8025;",
            &context,
            Destination::NginxConf,
        ),
        Err(Error::Extension(ExtensionError::ExtensionField { .. }))
    ));
}

/// Read a manifest and render its service against a throwaway home.
fn render(text: &str) -> Result<ServiceSpec> {
    let home = tempfile::tempdir().expect("a directory");
    let paths = Paths::new(home.path().to_path_buf(), &PathOverrides::default());

    let file = manifest::read(Path::new("probe").join(manifest::FILE_NAME).as_path(), text)?;
    let Body::Service(template) = &file.body else {
        panic!("a service");
    };
    let context = Context::planned(&paths, &file);

    service_spec(&file, template, &context)
}
