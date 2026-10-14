use super::*;

/// **A registry entry is a manifest in JSON, and it is read by the same code that reads the
/// file** — roadmap task **T81**, its design's D2.
///
/// The document the registry publishes holds manifests rather than pointers to them, so a
/// `--path` install and a registry install are proved by one parse. That only holds if what is
/// written is the *file's* shape: an enum serialised by its Rust variant would round-trip
/// perfectly and produce a document nobody could write by hand or check against a manifest.
#[test]
fn a_manifest_round_trips_through_json_in_the_file_s_own_shape() {
    let file = parse(mixengine_testkit::extension::MAILPIT).expect("mailpit parses");

    let json = to_value(&file);
    let again = read_value(json.clone()).expect("the entry reads back");

    assert_eq!(file, again);

    let object = json.as_object().expect("an object");
    assert!(
        object.contains_key("service"),
        "the table `kind` names: {json}"
    );
    assert!(object.contains_key("ports"), "{json}");
    assert!(
        object.contains_key("artifact"),
        "`[artifact.<target>]`, spelled as the file spells it: {json}"
    );
    assert!(
        !object.contains_key("body"),
        "a Rust enum leaked into the document: {json}"
    );
}

/// An entry from a schema this build does not read is refused by *version*, exactly as a file
/// from one is — before anything decides which unknown key to complain about.
#[test]
fn an_entry_from_a_newer_schema_is_refused_by_version() {
    let mut json = to_value(&parse(mixengine_testkit::extension::MAILPIT).expect("parses"));
    json["schema"] = serde_json::Value::from(SCHEMA + 1);

    let refusal = read_value(json).expect_err("refused");

    assert!(
        matches!(refusal, Error::Extension(ExtensionError::UnknownExtensionSchema { ref id, schema })
            if id == "mailpit" && schema == SCHEMA + 1),
        "{refusal}"
    );
}

/// Every kind has a fixture, and each one parses into the body its `kind` names.
#[test]
fn every_kind_reads() {
    let mailpit = parse(mixengine_testkit::extension::MAILPIT).expect("mailpit parses");
    assert_eq!(mailpit.extension.kind, ExtensionKind::Service);
    assert!(matches!(mailpit.body, Body::Service(_)));

    let phpmyadmin = parse(mixengine_testkit::extension::PHPMYADMIN).expect("phpmyadmin parses");
    assert!(matches!(phpmyadmin.body, Body::WebApp(_)));

    let sendmail = parse(mixengine_testkit::extension::SENDMAIL).expect("sendmail parses");
    assert!(matches!(sendmail.body, Body::Recipe));
}

/// **D7.** Mailpit is a supervised service *and* a php.ini change, in one extension, because
/// two extensions for one product would be two things to install and uninstall in step.
#[test]
fn a_service_may_also_carry_a_recipe() {
    let mailpit = parse(mixengine_testkit::extension::MAILPIT).expect("parses");

    let recipe = mailpit.recipe.expect("mailpit carries a recipe");

    assert_eq!(recipe.php_ini.len(), 1);
    assert_eq!(recipe.php_ini[0].key, "sendmail_path");
}

/// A table belonging to another kind is somebody who believes their extension will be
/// supervised. It is refused, not ignored.
#[test]
fn a_table_from_another_kind_is_refused() {
    let text = with_body(
        "web-app",
        "[web-app]\nroot = \"{install_dir}/app\"\ndomain = \"pma\"\n\n[web-app.runtime]\nkind = \"php\"\nrequires = \"^8.1\"\n\n[service]\nprogram = \"{install_dir}/x\"\ncwd = \"{data_dir}\"\nready = { type = \"pid_alive\", settle = \"1s\" }\n",
    );

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(
            ExtensionError::ExtensionTableUnexpected { .. }
        ))
    ));
}

/// **`desktop-app` is not a kind** — roadmap task **T165**, ADR 0038. A manifest that still
/// says so is refused by the reader like any other word it does not know.
#[test]
fn desktop_app_is_not_a_kind() {
    let text = with_body("desktop-app", "[desktop-app]\nscheme = \"otherapp\"\n");

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(ExtensionError::ExtensionManifest { .. }))
    ));
}

/// And a kind with none of its own table is a manifest that says nothing about what it is.
#[test]
fn a_kind_without_its_table_is_refused() {
    let text = with_body("service", "");

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(
            ExtensionError::ExtensionTableMissing { .. }
        ))
    ));
}

/// Refused rather than half-read, for `UnknownBlueprintSchema`'s reason: a manifest whose
/// unknown sections were skipped would install as something other than what its author wrote.
#[test]
fn a_newer_schema_is_refused() {
    let text = with_body("recipe", "[[recipe.php_ini]]\nkey = \"a\"\nvalue = \"b\"\n")
        .replace("schema = 1", "schema = 2");

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(ExtensionError::UnknownExtensionSchema {
            schema: 2,
            ..
        }))
    ));
}

/// **D9.** A stop command is a second program to render, and a second place to repeat the
/// path rule, for something no planned extension needs.
#[test]
fn a_stop_command_is_refused() {
    let text = with_body(
        "service",
        "[service]\nprogram = \"{install_dir}/x\"\ncwd = \"{data_dir}\"\nready = { type = \"pid_alive\", settle = \"1s\" }\nstop = { type = \"command\", program = \"{install_dir}/stop\", args = [], grace = \"5s\" }\n",
    );

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(ExtensionError::ExtensionField { .. }))
    ));
}

/// A key in `[ports]` becomes a placeholder, so it has to be spellable as one.
#[test]
fn a_port_name_must_be_a_placeholder_name() {
    let text = with_body(
        "service",
        "[ports]\n\"ui port\" = 8025\n\n[service]\nprogram = \"{install_dir}/x\"\ncwd = \"{data_dir}\"\nready = { type = \"pid_alive\", settle = \"1s\" }\n",
    );

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(ExtensionError::ExtensionField { .. }))
    ));
}

/// **D8.** These are administrative interfaces onto the machine's own databases. The
/// difference between them and a site somebody chose to share is that nobody chose.
#[test]
fn a_web_app_may_not_ask_for_the_lan() {
    let text = mixengine_testkit::extension::PHPMYADMIN
        .replace("network = \"loopback\"", "network = \"lan\"");

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(ExtensionError::ExtensionField { .. }))
    ));
}

/// An id a compiled-in recipe already claims would be two definitions of one service.
#[test]
fn an_id_a_recipe_claims_is_refused() {
    let text = with_body("recipe", "[[recipe.php_ini]]\nkey = \"a\"\nvalue = \"b\"\n")
        .replace("id = \"probe\"", "id = \"mariadb\"");

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(ExtensionError::ExtensionIdTaken { .. }))
    ));
}

/// **T81c, D1.** A fragment says which configuration language it is written in, because a
/// Caddyfile fragment in an `nginx.conf` is a syntax error and there is no answer to *"which of
/// the two did the author mean"* that is not a guess.
#[test]
fn a_front_end_fragment_names_its_server() {
    let text = with_body(
        "recipe",
        "[[recipe.front_end]]\nserver = \"nginx\"\nfragment = \"map $a $b { default 0; }\"\n\n[[recipe.front_end]]\nserver = \"caddy\"\nfragment = \"(probe) { respond 204 }\"\n",
    );

    let manifest = parse(&text).expect("a manifest naming its servers");
    let recipe = manifest.recipe.expect("a recipe");

    assert_eq!(recipe.front_end.len(), 2);
    assert_eq!(recipe.front_end[0].server, FrontEndServer::Nginx);
    assert_eq!(recipe.front_end[1].server, FrontEndServer::Caddy);
}

/// A fragment with no server is a fragment that is a syntax error in one of the two places it
/// could go.
#[test]
fn a_fragment_with_no_server_is_refused() {
    let text = with_body("recipe", "[[recipe.front_end]]\nfragment = \"a\"\n");

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(ExtensionError::ExtensionManifest { .. }))
    ));
}

/// And a third front end is a recipe, a package and a variant, in the task that adds one —
/// never a string a manifest may write.
#[test]
fn a_server_this_build_does_not_ship_is_refused() {
    let text = with_body(
        "recipe",
        "[[recipe.front_end]]\nserver = \"apache\"\nfragment = \"a\"\n",
    );

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(ExtensionError::ExtensionManifest { .. }))
    ));
}

/// An unknown key is a setting somebody believes they made.
#[test]
fn an_unknown_key_is_refused() {
    let text = with_body(
        "recipe",
        "[[recipe.php_ini]]\nkey = \"a\"\nvalue = \"b\"\nnote = \"c\"\n",
    );

    assert!(matches!(
        parse(&text),
        Err(Error::Extension(ExtensionError::ExtensionManifest { .. }))
    ));
}

/// **T81b, D4.** `[web-app].domain` is one label placed under `mixengine.test`; a dotted one
/// would have become `pma.tools.mixengine.test`, and an uppercase one is not a label this
/// system's domain syntax accepts.
#[test]
fn a_web_app_domain_is_one_label() {
    for (domain, expected) in [
        ("pma.tools", "may not contain `.`"),
        ("PMA", "lowercase ASCII"),
    ] {
        let text = with_body(
            "web-app",
            &format!(
                "[web-app]\nroot = \"{{install_dir}}/app\"\ndomain = \"{domain}\"\n\n[web-app.runtime]\nkind = \"php\"\nrequires = \"^8.1\"\n"
            ),
        );

        let refusal = parse(&text).expect_err(domain);
        assert!(
            matches!(refusal, Error::Extension(ExtensionError::ExtensionField { ref field, ref reason, .. })
                if field == "web-app.domain" && reason.contains(expected)),
            "{domain}: {refusal}"
        );
    }

    let fine = with_body(
        "web-app",
        "[web-app]\nroot = \"{install_dir}/app\"\ndomain = \"pma\"\n\n[web-app.runtime]\nkind = \"php\"\nrequires = \"^8.1\"\n",
    );
    assert!(parse(&fine).is_ok());
}

/// **Every fixture is a manifest the roster publishes** — roadmap task **T82**, the design's
/// D10.
///
/// The claim `extension.rs` opens with is only worth having if something checks it, and what a
/// published entry has to survive is this parse: `mixnz/mixengine-packages` renders each of
/// these through [`read`] and refuses the run on anything this refuses.
#[test]
fn every_shipped_manifest_parses() {
    use mixengine_testkit::extension;

    for (name, text) in [
        ("mailpit", extension::MAILPIT),
        ("phpmyadmin", extension::PHPMYADMIN),
        ("adminer", extension::ADMINER),
        ("sendmail", extension::SENDMAIL),
    ] {
        let manifest = parse(text).unwrap_or_else(|error| panic!("{name}: {error}"));

        // And it survives the round trip a registry entry takes, which is how it is published.
        let again = read_value(to_value(&manifest))
            .unwrap_or_else(|error| panic!("{name} as an entry: {error}"));

        assert_eq!(manifest, again, "{name}");
    }
}

/// **Adminer is the one whose artifact is not an archive** — the design's D3 — and the one whose
/// generated file is the application's entry point rather than a configuration beside it.
#[test]
fn adminer_is_one_file_and_a_generated_entry_point() {
    let manifest = parse(mixengine_testkit::extension::ADMINER).expect("adminer parses");

    let artifact = manifest.artifacts.get("any").expect("published anywhere");
    assert!(
        artifact.url.ends_with(".php"),
        "the distribution is one file: {}",
        artifact.url
    );

    let Body::WebApp(app) = &manifest.body else {
        panic!("a web-app");
    };
    let config = app.config.as_ref().expect("a generated file");

    assert_eq!(config.path, "index.php");
    assert!(
        config.text.contains("include __DIR__"),
        "a relative include is a bet on the working directory: {}",
        config.text
    );
}

/// A `web-app` with whatever tables the caller wants beside `[web-app]`.
fn a_web_app(extra: &str) -> String {
    with_body(
        "web-app",
        &format!(
            "[web-app]\nroot = \"{{install_dir}}/pma\"\ndomain = \"pma\"\n\n\
             [web-app.runtime]\nkind = \"php\"\nrequires = \"^8.0\"\n\n{extra}"
        ),
    )
}

/// **The text is the manifest's, because the artifact is upstream's** — roadmap task **T82**,
/// the design's D1.
///
/// For a registry install the extension's files *are* the published archive, verified against a
/// hash its publisher wrote down. There is no step between the download and the rename where a
/// file of ours could be added without making that hash a hash of something else — so the text
/// of the one file MixEngine generates travels in the manifest.
#[test]
fn a_web_app_carries_the_text_of_its_configuration_and_the_database_it_administers() {
    let manifest = parse(&a_web_app(
        "[web-app.config]\npath = \"config.inc.php\"\n\
         text = \"<?php $cfg['t'] = '{data_dir}';\"\n\n\
         [web-app.database]\nengines = [\"mariadb\", \"mysql\"]\n",
    ))
    .expect("it parses");

    let Body::WebApp(app) = &manifest.body else {
        panic!("a web-app");
    };

    let config = app.config.as_ref().expect("a configuration");
    assert_eq!(config.path, "config.inc.php");
    assert!(config.text.contains("{data_dir}"), "{}", config.text);
    assert_eq!(
        app.database.as_ref().expect("a database").engines,
        ["mariadb", "mysql"]
    );
}

/// A generated file goes under the root the manifest declared, and `..` is how one would reach
/// outside it — [`rooted`](super::super::render::rooted)'s rule, checked here because this path
/// is joined rather than rendered.
#[test]
fn a_configuration_path_may_not_climb_out_of_the_root() {
    for path in ["../evil.php", "/etc/passwd", "a/../../b.php", ""] {
        let text = a_web_app(&format!(
            "[web-app.config]\npath = \"{path}\"\ntext = \"<?php\"\n"
        ));

        let refusal = parse(&text).expect_err("it is refused");

        assert!(
            matches!(&refusal, Error::Extension(ExtensionError::ExtensionField { field, .. }) if field == "web-app.config.path"),
            "{path}: {refusal}"
        );
    }
}

/// A `web-app` that declares a database says which engines it can administer, and an empty list
/// says nothing at all.
#[test]
fn a_declared_database_names_at_least_one_engine() {
    let refusal = parse(&a_web_app("[web-app.database]\nengines = []\n"))
        .expect_err("an empty list is refused");

    assert!(
        matches!(&refusal, Error::Extension(ExtensionError::ExtensionField { field, .. }) if field == "web-app.database.engines"),
        "{refusal}"
    );
}

/// **The four placeholders T82 adds join the three that were already reserved.**
///
/// A `[ports]` key becomes a placeholder, so a port called `db_host` would be a manifest whose
/// own port shadowed the address it was pointed at.
#[test]
fn a_port_may_not_be_named_after_a_placeholder_that_already_means_something() {
    for name in FIXED_PLACEHOLDERS {
        let text = with_body("recipe", &format!("[ports]\n{name} = 1234\n"));

        assert!(parse(&text).is_err(), "{name} is already taken");
    }
}

/// [`read`] against a file name a message can name.
fn parse(text: &str) -> Result<ExtensionManifest> {
    read(Path::new("probe").join(FILE_NAME).as_path(), text)
}

/// A manifest with a header of the given kind and whatever body the caller wants.
fn with_body(kind: &str, body: &str) -> String {
    format!(
        "schema = 1\n\n[extension]\nid = \"probe\"\nname = \"Probe\"\nversion = \"1.0.0\"\nkind = \"{kind}\"\n\n{body}"
    )
}

/// **A manifest that asks for a superuser password has to read it** — roadmap task **T82a**,
/// the design's D2. Consent that buys nothing is consent nobody should be asked for, and this
/// is the one rule in this format that spans two tables.
#[test]
fn signing_in_without_reading_the_password_is_refused() {
    let refusal = parse(&signing_in("signs_in = true", "$cfg['h'] = '{db_host}';"))
        .expect_err("a manifest that never reads the password");

    let said = refusal.to_string();
    assert!(said.contains("web-app.database.signs_in"), "{said}");
    assert!(said.contains("db_password_env"), "{said}");
}

/// And one that does is accepted, with the flag on the parsed body.
#[test]
fn signing_in_is_read_off_the_database_table() {
    let manifest = parse(&signing_in(
        "signs_in = true",
        "$cfg['p'] = getenv('{db_password_env}');",
    ))
    .expect("it parses");

    let Body::WebApp(app) = &manifest.body else {
        panic!("a web-app");
    };
    assert!(app.database.as_ref().expect("a database").signs_in);
}

/// The default is off: a missing value is silence, and silence is not consent — which is the
/// rule [`NetworkReach`](mixengine_proto::NetworkReach) already states for the other field a
/// person is shown before an install.
#[test]
fn a_database_that_says_nothing_does_not_sign_in() {
    let manifest = parse(&signing_in("", "$cfg['h'] = '{db_host}';")).expect("it parses");

    let Body::WebApp(app) = &manifest.body else {
        panic!("a web-app");
    };
    assert!(!app.database.as_ref().expect("a database").signs_in);
}

/// A `[ports]` key may not shadow the placeholder that renders the variable's name.
#[test]
fn a_port_cannot_be_called_after_the_password_variable() {
    let refusal = parse(&with_body(
        "service",
        "[ports]\ndb_password_env = 8025\n\n[service]\nprogram = \"{install_dir}/p\"\n\
         cwd = \"{data_dir}\"\nready = { type = \"pid_alive\", settle = \"1s\" }\n",
    ))
    .expect_err("a port shadowing a fixed placeholder");

    assert!(refusal.to_string().contains("db_password_env"));
}

/// A `web-app` with one line of `[web-app.database]` and one of configuration text.
fn signing_in(database_line: &str, config_line: &str) -> String {
    with_body(
        "web-app",
        &format!(
            "[web-app]\nroot = \"{{install_dir}}/pma\"\ndomain = \"pma\"\n\n\
             [web-app.database]\nengines = [\"mariadb\"]\n{database_line}\n\n\
             [web-app.runtime]\nkind = \"php\"\nrequires = \"^8.0\"\n\n\
             [web-app.config]\npath = \"config.inc.php\"\ntext = \"\"\"\n\
             <?php\n{config_line}\n\"\"\"\n"
        ),
    )
}

/// A `service` with one port and no `[ui]`, for the `[ui]` tests to add one to.
const SMALL_SERVICE: &str = r#"
schema = 1

[extension]
id = "catcher"
name = "Catcher"
version = "1.0.0"
kind = "service"

[ports]
ui_port = 18025

[service]
program = "{install_dir}/catcher"
cwd = "{data_dir}"
args = ["--listen", "{listen}:{ui_port}"]
ready = { type = "tcp", addr = "{listen}:{ui_port}", timeout = "10s" }

[permissions]
network = "loopback"
filesystem = ["own-data"]
"#;

/// **T200a, D1.** `[ui]` names a port the manifest declares, and survives `to_value`.
#[test]
fn a_service_names_the_port_a_person_opens() {
    let text = format!("{SMALL_SERVICE}\n[ui]\nport = \"ui_port\"\n");
    let read = parse(&text).expect("a manifest with [ui]");
    assert_eq!(
        read.ui,
        Some(UiTable {
            port: "ui_port".to_owned(),
            path: "/".to_owned()
        })
    );

    let stored = to_value(&read);
    assert_eq!(stored["ui"]["port"], "ui_port", "{stored}");
    let back = read_value(stored).expect("the stored shape reads back");
    assert_eq!(back.ui, read.ui);
}

/// **T200a, D1.** A manifest with no `[ui]` — every one installed before T200a — reads as none,
/// and is stored without one.
#[test]
fn a_manifest_without_ui_has_none() {
    let read = parse(SMALL_SERVICE).expect("a manifest");
    assert_eq!(read.ui, None);
    assert!(to_value(&read).get("ui").is_none());
}

/// **T200a, D1.** The port has to be one `[ports]` declares, and the path has to be a path.
#[test]
fn a_ui_naming_no_port_or_no_path_is_refused() {
    for (table, field) in [
        ("[ui]\nport = \"web\"\n", "ui.port"),
        ("[ui]\nport = \"ui_port\"\npath = \"inbox\"\n", "ui.path"),
    ] {
        let text = format!("{SMALL_SERVICE}\n{table}");
        let refused = parse(&text).expect_err("refused");
        assert!(
            matches!(&refused, Error::Extension(ExtensionError::ExtensionField { field: f, .. }) if f == field),
            "{refused}"
        );
    }
}

/// **T200a, D1.** A web-app's page is its site.
#[test]
fn ui_is_refused_on_a_web_app() {
    let text = format!(
        "{}\n[ui]\nport = \"x\"\n",
        mixengine_testkit::extension::ADMINER
    );
    let refused = parse(&text).expect_err("refused");
    assert!(
        matches!(
            refused,
            Error::Extension(ExtensionError::ExtensionTableUnexpected { table: "ui", .. })
        ),
        "{refused}"
    );
}
