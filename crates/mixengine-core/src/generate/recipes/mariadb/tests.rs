use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mixengine_proto::ServiceId;

use super::*;
use crate::ServiceError;
use crate::generate::Upstream;
use crate::generate::first_run::FirstRun;
use crate::generate::recipe::{self, ServiceCertificate};
use crate::generate::settings::Settings;

/// An absolute path on whichever system this is compiled for.
const fn root() -> &'static str {
    if cfg!(windows) {
        r"C:\MixEngine"
    } else {
        "/opt/mixengine"
    }
}

/// What the artifact publishes, as `mixengine-packages` publishes it.
fn provides() -> BTreeMap<String, String> {
    [
        ("mariadbd", "bin/mariadbd"),
        ("mariadb", "bin/mariadb"),
        ("mariadb-admin", "bin/mariadb-admin"),
        ("mariadb-install-db", "scripts/mariadb-install-db"),
    ]
    .into_iter()
    .map(|(name, path)| {
        (
            name.to_owned(),
            format!("{path}{}", std::env::consts::EXE_SUFFIX),
        )
    })
    .collect()
}

/// A `mariadb@main` in a home at [`root`], with `overrides` applied.
fn context(overrides: &str) -> Context {
    with_provides(provides(), overrides)
}

/// The same, for an install that publishes something else — or nothing.
fn with_provides(provides: BTreeMap<String, String>, overrides: &str) -> Context {
    let service = ServiceId::parse("mariadb@main").expect("an id");
    let settings =
        Settings::merge(Mariadb.settings(), overrides, &service).expect("usable overrides");

    let context = Context::for_test(
        service,
        PACKAGE,
        Path::new(root()),
        provides,
        Some(3306),
        settings,
    );
    let endpoints = Mariadb
        .endpoints(&context)
        .expect("a home this short has a usable socket path");

    context.with_endpoints(endpoints)
}

/// The rendered `my.cnf` for `overrides`.
fn rendered(overrides: &str) -> String {
    recipe::render(&Mariadb, &context(overrides))
        .expect("a rendering")
        .first()
        .expect("one file")
        .contents()
        .to_owned()
}

/// **A database is woken at the addresses it listens on itself.**
///
/// There is no front end in front of MariaDB to name a fallback in, so the activator binds the
/// service's own address while it is stopped. On a system with Unix sockets that is *two*
/// addresses and not one, and the difference is a client's habit rather than a configuration:
/// a generated `.env` names `127.0.0.1`, and `mariadb` typed with no host at all names the
/// socket. Waking on only one of them leaves the other client hanging against an address
/// nothing holds.
/// **Health is asked in the server's own protocol.** Readiness, which runs once per start, still
/// asks the client program.
#[test]
fn health_is_asked_in_the_protocol_and_readiness_is_still_the_client() {
    let spec = Mariadb
        .spec(&context("{}"))
        .expect("a spec")
        .build()
        .expect("a valid spec");
    let health = spec.health().expect("a health check");

    assert!(
        matches!(health.probe, HealthProbe::MysqlGreeting { .. }),
        "{health:?}"
    );
    assert_eq!(health.interval, HEALTH_INTERVAL);
    assert!(
        matches!(spec.ready(), ReadyCheck::Command { .. }),
        "{:?}",
        spec.ready()
    );
}
#[test]
fn a_stopped_server_is_woken_at_its_port_and_at_its_socket() {
    let context = context("{}");

    let held = Mariadb
        .held_while_stopped(&context)
        .expect("the addresses it is woken at");

    assert!(
        held.contains(&Upstream::Tcp(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            3306
        ))),
        "a client dialling 127.0.0.1:3306 would not wake it: {held:?}"
    );

    assert_eq!(
        held.len(),
        if cfg!(windows) { 1 } else { 2 },
        "the socket is an address on every system that has one: {held:?}"
    );
}

/// Two instances of one server, so its id carries an `@`.
#[test]
fn mariadb_is_named_because_a_home_may_have_two() {
    assert_eq!(Mariadb.instancing(), Instancing::Named);
}

/// Every path in the rendered file is quoted and forward-slashed.
///
/// **Both halves are one measurement, made against a real server**: MariaDB's option parser treats
/// `\` as an escape and everything after an unquoted `#` as a comment, so
/// `C:\Users\Nguyen Hai Quang` breaks a naive rendering in two different ways at once — and a
/// user whose home has a space in it is a real user on all three systems.
#[test]
fn every_path_in_the_configuration_is_quoted_and_forward_slashed() {
    let rendered = rendered("{}");
    let named = [
        "basedir",
        "datadir",
        "tmpdir",
        "log_error",
        "socket",
        "plugin-dir",
    ];

    let mut seen = 0;
    for line in rendered
        .lines()
        .filter(|line| named.iter().any(|key| line.starts_with(key)))
    {
        let value = line.split_once('=').expect("a setting").1.trim();

        assert!(
            value.starts_with('"') && value.ends_with('"'),
            "an unquoted path ends at the first `#`: {line}"
        );
        assert!(
            !value.contains('\\'),
            "a backslash is an escape to this parser: {line}"
        );
        seen += 1;
    }

    assert!(seen >= 3, "no paths were checked at all:\n{rendered}");
}

/// The lines that are stated rather than defaulted, each for something measured.
#[test]
fn the_configuration_states_what_a_supervisor_cannot_guess() {
    let rendered = rendered("{}");

    assert!(rendered.contains("log_error"), "{rendered}");
    assert!(rendered.contains("tmpdir = \""), "{rendered}");
    assert!(rendered.contains("skip-name-resolve"), "{rendered}");
    assert!(rendered.contains("bind-address = 127.0.0.1"), "{rendered}");
    assert!(rendered.contains("port = 3306"), "{rendered}");
}

/// An override reaches the file rather than being ignored.
#[test]
fn the_configuration_says_what_the_overrides_said() {
    let rendered = rendered(r#"{"innodb_buffer_pool_size": "512M"}"#);

    assert!(
        rendered.contains("innodb_buffer_pool_size = 512M"),
        "{rendered}"
    );
}

/// **A development machine's defaults, which are not the server's.**
///
/// MyISAM's key cache is allocated at startup and held whether or not one MyISAM table exists,
/// and the log flush is what every commit otherwise waits for. Both exist in 10.6, the oldest
/// series the index publishes — an option file with an unknown directive is refused whole, so a
/// younger line would stop the server rather than be ignored.
///
/// `performance_schema` is deliberately absent here and present in MySQL's template: MariaDB
/// ships it off, and a line restating a default is a line to keep in step for no gain.
#[test]
fn the_configuration_is_tuned_for_a_laptop_rather_than_for_a_server() {
    let rendered = rendered("{}");

    assert!(rendered.contains("key_buffer_size = 16M"), "{rendered}");
    assert!(
        rendered.contains("innodb_flush_log_at_trx_commit = 2"),
        "{rendered}"
    );
}

/// **With a certificate on the context, the file names both halves and the fingerprint.**
/// Backslashes in, forward slashes out: MariaDB's option parser treats `\` as an escape.
#[test]
fn with_a_certificate_the_file_names_both_halves_and_the_fingerprint() {
    let rendered = recipe::render(
        &Mariadb,
        &context("{}").with_certificate(ServiceCertificate {
            certificate: PathBuf::from(r"C:\MixEngine\certs\services\mariadb@main.crt"),
            key: PathBuf::from(r"C:\MixEngine\certs\services\mariadb@main.key"),
            fingerprint: "abc123".to_owned(),
        }),
    )
    .expect("a rendering")
    .first()
    .expect("one file")
    .contents()
    .to_owned();

    assert!(
        rendered.contains(r#"ssl_cert = "C:/MixEngine/certs/services/mariadb@main.crt""#),
        "{rendered}"
    );
    assert!(
        rendered.contains(r#"ssl_key = "C:/MixEngine/certs/services/mariadb@main.key""#),
        "{rendered}"
    );
    assert!(
        rendered.contains("# Rendered for certificate abc123"),
        "{rendered}"
    );
}

/// **Without one, the file says nothing about TLS**: the server then does what it does with no
/// certificate named, and a start is never refused for want of a certificate.
#[test]
fn without_a_certificate_the_file_says_nothing_about_tls() {
    let rendered = rendered("{}");

    for line in rendered.lines().filter(|line| !line.starts_with('#')) {
        assert!(!line.contains("ssl"), "{line}\n{rendered}");
    }
}

/// **`skip-ssl` is written on no path.** It was, for one commit, and the real-server suite
/// found what it costs: an 11.4 client with a password on its command line is refused.
#[test]
fn skip_ssl_is_written_on_no_path() {
    let without = rendered("{}");
    let with = recipe::render(
        &Mariadb,
        &context("{}").with_certificate(ServiceCertificate {
            certificate: PathBuf::from("/certs/services/mariadb@main.crt"),
            key: PathBuf::from("/certs/services/mariadb@main.key"),
            fingerprint: "abc123".to_owned(),
        }),
    )
    .expect("a rendering")
    .first()
    .expect("one file")
    .contents()
    .to_owned();

    for rendered in [without, with] {
        assert!(
            !rendered.lines().any(|line| line.trim() == "skip-ssl"),
            "{rendered}"
        );
    }
}

/// The recipe asks for exactly `localhost` and its bind address, in that order.
#[test]
fn the_recipe_asks_for_localhost_and_its_bind_address() {
    assert_eq!(
        Mariadb.certificate(&context("{}")),
        Some(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])
    );
}

/// **The relaxed flush is the log's, and the page barriers are untouched.**
///
/// What the development defaults spend is the last second of committed transactions; what they must
/// never spend is a data directory that will not open, and `innodb_doublewrite` is what keeps a
/// torn page recoverable.
#[test]
fn the_barriers_that_make_a_page_recoverable_are_left_alone() {
    let rendered = rendered("{}");

    for line in rendered.lines().filter(|line| !line.starts_with('#')) {
        assert!(!line.contains("innodb_doublewrite"), "{line}\n{rendered}");
        assert!(!line.contains("innodb_flush_method"), "{line}\n{rendered}");
    }
}

/// The escape hatch is real: `extra` renders after every directive above it, and a later line
/// in an option file wins.
#[test]
fn a_user_can_put_the_servers_own_durability_back() {
    let rendered = rendered(r#"{"extra": "innodb_flush_log_at_trx_commit = 1"}"#);

    let relaxed = rendered
        .find("innodb_flush_log_at_trx_commit = 2")
        .expect("the recipe states its own value");
    let restored = rendered
        .rfind("innodb_flush_log_at_trx_commit = 1")
        .expect("the override reaches the file");

    assert!(restored > relaxed, "{rendered}");
}

/// **The file and the readiness check must name one server.**
///
/// They are computed twice — once in Jinja for the file the server reads, once in Rust for the
/// check the daemon makes — and the failure when they disagree is a database that starts
/// perfectly and is reported as never having come up.
#[test]
fn the_file_and_the_readiness_check_name_one_port() {
    let context = context("{}");
    let rendered = rendered("{}");
    let spec = Mariadb
        .spec(&context)
        .expect("a spec")
        .build()
        .expect("a valid spec");

    let ReadyCheck::Command { args, .. } = spec.ready() else {
        panic!(
            "a database is proved up by a query, not by an accept: {:?}",
            spec.ready()
        );
    };

    assert!(args.iter().any(|arg| arg == "--port=3306"), "{args:?}");
    assert!(rendered.contains("port = 3306"), "{rendered}");
}

/// What a failed start is diagnosed against.
///
/// The row's port and nothing else: the socket is a path and cannot be in conflict with a
/// program that has no `services` row, which is what this declaration is read for.
#[test]
fn the_spec_declares_the_port_the_server_will_bind() {
    let context = context("{}");
    let spec = Mariadb
        .spec(&context)
        .expect("a spec")
        .build()
        .expect("a valid spec");

    assert_eq!(spec.ports(), [3306]);
}

/// No password reaches an argument list, which every process on the machine can read.
#[test]
fn the_credential_is_named_rather_than_carried() {
    let spec = Mariadb
        .spec(&context("{}"))
        .expect("a spec")
        .build()
        .expect("a valid spec");

    assert!(
        matches!(
            spec.env().get(PASSWORD_VARIABLE),
            Some(mixengine_proto::EnvValue::Keyring { service, key })
                if service == KEYRING_SERVICE
                    && key == &format!(
                        "{}/mariadb@main/root",
                        crate::generate::context::TEST_HOME
                    )
        ),
        "{:?}",
        spec.env()
    );
}

/// The server is pointed at its own file, and at nothing else.
///
/// **`--no-defaults` must not be there.** MariaDB honours whichever of `--no-defaults` and
/// `--defaults-file` comes first, so the pair means "read nothing" — and a server that read
/// nothing looks for its data directory beside its own binary, fails to `chdir`, and
/// crash-loops. Which is what this recipe did until it was run against a real server.
#[test]
fn the_server_reads_its_own_file_and_no_other() {
    let spec = Mariadb
        .spec(&context("{}"))
        .expect("a spec")
        .build()
        .expect("a valid spec");

    assert!(
        spec.args()
            .first()
            .is_some_and(|first| first.starts_with("--defaults-file=")),
        "the configuration file has to be the first option: {:?}",
        spec.args()
    );
    assert!(
        !spec.args().iter().any(|arg| arg == "--no-defaults"),
        "`--no-defaults` before `--defaults-file` means the file is never read: {:?}",
        spec.args()
    );
}

/// A stop that is a command, because a signal leaves an unclean InnoDB.
#[test]
fn mariadb_is_stopped_by_asking_it_to_shut_down() {
    let spec = Mariadb
        .spec(&context("{}"))
        .expect("a spec")
        .build()
        .expect("a valid spec");

    assert!(
        matches!(spec.stop(), StopBehaviour::Command { program, args, .. }
            if program.ends_with(format!("mariadb-admin{}", std::env::consts::EXE_SUFFIX))
                && args.iter().any(|arg| arg == "shutdown")),
        "{:?}",
        spec.stop()
    );
}

/// And no reload at all: MariaDB reads its configuration once.
#[test]
fn mariadb_has_no_reload_because_it_cannot() {
    let spec = Mariadb
        .spec(&context("{}"))
        .expect("a spec")
        .build()
        .expect("a valid spec");

    assert!(spec.reload().is_none(), "{:?}", spec.reload());
}

/// **A first run is measured before its credentials exist.**
///
/// The daemon waits for the bootstrap job for as long as the steps ask plus a little, and it has
/// to decide that before it generates the credential those steps interpolate. Measuring a plan
/// by building its steps with no secrets does not work: this recipe refuses an empty password —
/// rightly — so the measurement comes back empty and thirty declared minutes collapse to the
/// slack alone. CI met it the first time two of these bootstrapped at once on Windows and the
/// first one was killed at sixty seconds.
#[test]
fn a_first_run_is_measured_before_its_credentials_exist() {
    let plan = FirstRun::new(
        &context("{}"),
        Mariadb.ritual().expect("mariadb bootstraps"),
    );

    assert!(
        plan.budget() >= BOOTSTRAP_PATIENCE,
        "one bootstrap step alone asks for {BOOTSTRAP_PATIENCE:?}, and the plan measured {:?}",
        plan.budget()
    );
}

/// **A reset builds steps for a directory that already exists.**
///
/// The assertion is the negative: not one of them creates a data directory. A reset that ran
/// `mariadb-install-db` over a live database is the failure this whole task exists to avoid,
/// and it is exactly the one a copy of [`steps`] would produce.
#[test]
fn the_reset_sets_a_password_and_creates_no_data_directory() {
    let plan = FirstRun::new(
        &context("{}"),
        Mariadb.ritual().expect("mariadb bootstraps"),
    );

    let steps = plan
        .reset_steps(BTreeMap::from([(ROOT.to_owned(), "a".repeat(32))]))
        .expect("a reset builds")
        .expect("mariadb declares one");

    assert!(!steps.is_empty(), "a declared reset built no steps");

    for step in &steps {
        let program = step.program.display().to_string();
        assert!(
            !program.contains(INSTALL_DB),
            "a reset step creates a data directory: {program}"
        );
    }

    assert!(
        steps.iter().any(|step| step
            .stdin
            .as_deref()
            .is_some_and(|sql| sql.contains("global_priv"))),
        "no reset step sets the root password: {steps:?}"
    );
}

/// A context carrying the credential the daemon would have generated.
fn initialised(secret: &str) -> Context {
    let mut context = context("{}");
    context.put_secret(ROOT, secret);

    context
}

/// **Every step of the ritual, and the file, name one scratch directory of this instance's
/// own**, measured in [`crate::generate::recipes::scratch_dir`]'s note.
///
/// In the environment rather than as an argument, and on every step rather than on the two
/// that start a server: `TMPDIR` reaches the server whichever of upstream's programs spawned
/// it, and one shape across all the steps is what a test can hold the recipe to. Beside the
/// data directory and named after the instance, so two instances of one package cannot share
/// it any more than they share their data.
#[test]
fn every_first_run_step_and_the_file_name_this_instances_own_scratch_directory() {
    use crate::generate::recipes::{TMPDIR, scratch_dir};

    let context = initialised("hunter2");
    let scratch = context
        .scratch()
        .expect("this recipe asks for a scratch directory");

    assert_eq!(scratch, scratch_dir(&context));
    assert_eq!(scratch.parent(), context.data().parent(), "{scratch:?}");
    // The fixture's data directory is flat; a real home's is `data/mariadb/main`, and what holds
    // in both is that the scratch directory is the data directory's name with `.tmp` after it.
    let expected_name = format!(
        "{}.tmp",
        context.data().file_name().unwrap().to_string_lossy()
    );
    assert_eq!(
        scratch.file_name().and_then(|name| name.to_str()),
        Some(expected_name.as_str()),
        "{scratch:?}"
    );

    let expected = scratch.display().to_string();
    for step in steps(&context).expect("steps") {
        assert_eq!(step.env.get(TMPDIR), Some(&expected), "{step:?}");
    }

    let rendered = rendered("{}");
    let line = format!("tmpdir = \"{}\"", expected.replace('\\', "/"));
    assert!(rendered.contains(&line), "no `{line}` in:\n{rendered}");
}

/// The keyring entry the spec names and the one the ritual is stored under are one address.
///
/// Composed twice — once for the spec's `EnvValue::Keyring`, once for the daemon writing the
/// generated value — and the failure when they disagree is a server that starts and a client
/// that cannot authenticate against it, reported as a service that never became ready.
#[test]
fn the_spec_and_the_ritual_name_one_credential() {
    let context = context("{}");
    let spec = Mariadb
        .spec(&context)
        .expect("a spec")
        .build()
        .expect("a valid spec");

    let named = spec
        .env()
        .get(PASSWORD_VARIABLE)
        .expect("the spec names a credential");

    assert!(
        matches!(named, mixengine_proto::EnvValue::Keyring { key, .. }
            if key == &context.secret_address(ROOT)),
        "{named:?}"
    );
    // **The home is in front of the service.** Without it every MIXENGINE_HOME on one machine
    // writes to this one entry, and the last to bootstrap takes the root password of every server
    // that was already running.
    assert_eq!(
        context.secret_address(ROOT),
        format!("{}/mariadb@main/root", crate::generate::context::TEST_HOME)
    );

    // **And it is the shared composition, not a second one that agrees today.** This address is
    // published to MixLab, which reads the entry MixEngine wrote.
    assert_eq!(
        context.secret_address(ROOT),
        crate::services::handoff::secret_key(
            &crate::home::HomeId::parse(crate::generate::context::TEST_HOME).expect("an id"),
            context.service(),
            ROOT
        )
    );
}

/// A recipe that declares a credential also declares what to do with it.
#[test]
fn mariadb_declares_the_one_credential_its_ritual_needs() {
    let ritual = Mariadb.ritual().expect("a database has a first run");

    assert_eq!(ritual.secrets.len(), 1, "{:?}", ritual.secrets);
    assert_eq!(ritual.secrets[0].key, ROOT);
    assert!(ritual.secrets[0].length >= 32, "{:?}", ritual.secrets);
}

/// The bootstrap sets the password it was given, and drops what should not survive.
///
/// Each statement named rather than left to "secure defaults", because the whole value of this
/// step is that nothing is left to be assumed.
#[test]
fn the_bootstrap_says_exactly_what_it_does_to_the_grant_tables() {
    let steps = steps(&initialised("hunter2")).expect("steps");
    let bootstrap = steps
        .iter()
        .find(|step| step.stdin.is_some())
        .expect("one step speaks SQL");
    let sql = bootstrap.stdin.as_deref().expect("the SQL is on stdin");

    // The row rather than `SET PASSWORD`, and every root rather than `root@localhost` — both
    // measured against 11.4.12, both explained at `bootstrap`.
    assert!(
        sql.contains("UPDATE global_priv SET priv = JSON_SET("),
        "{sql}"
    );
    assert!(sql.contains("WHERE User = 'root';"), "{sql}");
    assert!(sql.contains("PASSWORD('hunter2')"), "{sql}");
    assert!(
        sql.contains("DELETE FROM global_priv WHERE User = ''"),
        "{sql}"
    );
    assert!(sql.contains("DROP DATABASE IF EXISTS test"), "{sql}");
    assert!(
        !sql.contains("SET PASSWORD"),
        "bootstrap mode refuses that statement with error 1290: {sql}"
    );

    // Bootstrap mode reads one statement per line, so a statement wrapped over two would be two
    // statements and neither of them valid.
    for line in sql.lines().filter(|line| !line.trim().is_empty()) {
        assert!(
            line.trim_end().ends_with(';'),
            "a wrapped statement: {line}"
        );
    }

    assert!(
        bootstrap.args.iter().any(|arg| arg == "--bootstrap"),
        "the password is set by a server listening on nothing: {:?}",
        bootstrap.args
    );
    assert!(
        bootstrap.args.iter().any(|arg| arg == "--no-defaults"),
        "a machine own my.cnf must not reach this: {:?}",
        bootstrap.args
    );
}

/// A credential that would need escaping is refused rather than escaped.
///
/// `mixengine_platform::generate_secret` restricts its alphabet for exactly the interpolation
/// above; this is the other end of that arrangement, and it fails loudly rather than quietly
/// producing a statement whose second half is somebody password.
#[test]
fn a_credential_that_would_need_escaping_is_refused() {
    let error = steps(&initialised("a'b")).expect_err("that cannot go in a SQL literal");

    assert!(
        matches!(&error, Error::Service(ServiceError::SettingValue { value, .. }) if !value.contains("a'b")),
        "the refusal quoted the credential: {error:?}"
    );
}

/// And so is no credential at all, which is a recipe that declared one and never got it.
#[test]
fn no_credential_at_all_is_refused_rather_than_setting_an_empty_password() {
    let error = steps(&context("{}")).expect_err("an empty password is not a password");

    assert!(
        matches!(error, Error::Service(ServiceError::SettingValue { .. })),
        "{error:?}"
    );
}

/// The Unix ritual is four steps and the Windows one is two, and the difference is upstream.
#[test]
fn the_ritual_has_the_shape_this_system_needs() {
    let steps = steps(&initialised("abc123")).expect("steps");
    let labels: Vec<&str> = steps.iter().map(|step| step.label.as_str()).collect();

    if cfg!(windows) {
        assert_eq!(labels.len(), 2, "{labels:?}");
    } else {
        assert_eq!(labels.len(), 4, "{labels:?}");
        assert!(labels[0].contains("space"), "{labels:?}");
        assert!(labels[3].contains("space"), "{labels:?}");
    }

    assert!(
        steps.iter().all(|step| step.program.is_absolute()),
        "a relative program is whatever the PATH says at the moment it runs: {steps:?}"
    );
}

/// A package that publishes none of what this recipe needs is named as such.
#[test]
fn a_mariadb_without_its_own_binaries_is_named() {
    let context = with_provides(BTreeMap::new(), "{}");

    let error = Mariadb.spec(&context).expect_err("nothing to run");

    assert!(
        matches!(
            error,
            Error::Service(ServiceError::ServiceProvidesNothing { .. })
        ),
        "{error:?}"
    );
}

/// A provisioning of `blog` on this fixture, as [`Ask`] and [`Credentials`].
fn asked() -> (crate::generate::Ask, crate::generate::Credentials) {
    (
        crate::generate::Ask {
            database: "blog".to_owned(),
            user: "blog".to_owned(),
        },
        crate::generate::Credentials {
            root: "a".repeat(32),
            account: "b".repeat(32),
        },
    )
}

/// The SQL every step of a provisioning carries, joined.
fn provisioning_sql(found: crate::generate::Found) -> String {
    let context = context("{}");
    let admin = Mariadb.databases().expect("mariadb administers databases");
    let (ask, credentials) = asked();

    (admin.steps)(&context, &ask, found, &credentials)
        .expect("statements")
        .iter()
        .filter_map(|step| step.stdin.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// **No credential in any argument.** Statements travel on standard input and the password
/// travels in the environment, because every process on this machine can read an argument list.
#[test]
fn provisioning_puts_no_credential_in_an_argument() {
    let context = context("{}");
    let admin = Mariadb.databases().expect("mariadb administers databases");
    let (ask, credentials) = asked();

    let mut steps = vec![(admin.probe)(&context, &ask, &credentials.root).expect("a probe")];
    steps.extend(
        (admin.steps)(
            &context,
            &ask,
            crate::generate::Found::default(),
            &credentials,
        )
        .expect("statements"),
    );

    for step in &steps {
        for argument in &step.args {
            assert!(
                !argument.contains(&credentials.root) && !argument.contains(&credentials.account),
                "{} put a password in an argument: {argument}",
                step.label
            );
        }
    }
}

/// **The validator's alphabet is exactly what every accepted character survives being embedded
/// as.** A chosen password containing every printable-ASCII character the validator accepts must
/// still produce SQL whose `IDENTIFIED BY '...'` literal closes where it should: an unescaped
/// literal that swallowed a later character would shift where the string appears to end, and the
/// assertion below would then fail to find the exact literal.
#[test]
fn every_accepted_password_character_survives_the_identified_by_literal() {
    let chosen =
        crate::generate::databases::validated_password("aZ09!\"#$%&()*+,-./:;<=>?@[]^_`{|}~")
            .expect("every one of these is accepted");

    let context = context("{}");
    let admin = Mariadb.databases().expect("mariadb administers databases");
    let (ask, _) = asked();
    let credentials = crate::generate::Credentials {
        root: "a".repeat(32),
        account: chosen.clone(),
    };

    let steps = (admin.steps)(
        &context,
        &ask,
        crate::generate::Found::default(),
        &credentials,
    )
    .expect("statements");

    let mut found_identified_by = false;
    for step in &steps {
        if let Some(sql) = &step.stdin
            && sql.contains("IDENTIFIED BY")
        {
            found_identified_by = true;
            assert!(
                sql.contains(&format!("IDENTIFIED BY '{chosen}'")),
                "the chosen password must appear as one intact literal: {sql}"
            );
        }
    }
    assert!(
        found_identified_by,
        "no statement carried IDENTIFIED BY at all"
    );
}

/// The superuser's password reaches the client the way the health check's already does.
#[test]
fn the_superuser_password_travels_in_the_environment() {
    let context = context("{}");
    let admin = Mariadb.databases().expect("mariadb administers databases");
    let (ask, credentials) = asked();

    let probe = (admin.probe)(&context, &ask, &credentials.root).expect("a probe");

    assert_eq!(
        probe.env.get(PASSWORD_VARIABLE),
        Some(&credentials.root),
        "{:?}",
        probe.env.keys().collect::<Vec<_>>()
    );
}

/// **No `CHARACTER SET`, on purpose.** The instance is already configured with one, and a second
/// place deciding it is one that silently wins.
#[test]
fn creating_a_database_names_no_character_set() {
    let statements = provisioning_sql(crate::generate::Found::default());

    assert!(
        statements.contains("CREATE DATABASE IF NOT EXISTS `blog`"),
        "{statements}"
    );
    assert!(
        !statements.to_ascii_uppercase().contains("CHARACTER SET"),
        "{statements}"
    );
    assert!(
        !statements.to_ascii_uppercase().contains("COLLATE"),
        "{statements}"
    );
}

/// Three hosts, because `skip-name-resolve` means a TCP client is `127.0.0.1` and never
/// `localhost` — the recipe's finding about the root account, just as true of this one.
#[test]
fn the_account_is_made_for_every_host_a_local_client_arrives_as() {
    let statements = provisioning_sql(crate::generate::Found::default());

    for host in ["localhost", "127.0.0.1", "::1"] {
        assert!(
            statements.contains(&format!("CREATE USER IF NOT EXISTS 'blog'@'{host}'")),
            "no account for {host}:\n{statements}"
        );
        assert!(
            statements.contains(&format!(
                "GRANT ALL PRIVILEGES ON `blog`.* TO 'blog'@'{host}'"
            )),
            "no grant for {host}:\n{statements}"
        );
    }
}

/// **Design D13.** The last step logs in as the account that was just made and writes with it,
/// so a success the account cannot use cannot be reported.
#[test]
fn the_last_step_logs_in_as_the_new_account_and_writes() {
    let context = context("{}");
    let admin = Mariadb.databases().expect("mariadb administers databases");
    let (ask, credentials) = asked();

    let steps = (admin.steps)(
        &context,
        &ask,
        crate::generate::Found::default(),
        &credentials,
    )
    .expect("statements");
    let last = steps.last().expect("there is a last step");

    assert!(
        last.args.contains(&"--user=blog".to_owned()),
        "{:?}",
        last.args
    );
    assert_eq!(
        last.env.get(PASSWORD_VARIABLE),
        Some(&credentials.account),
        "the check must authenticate as the new account and not as root"
    );

    let sql = last.stdin.as_deref().unwrap_or_default();
    assert!(sql.contains("CREATE TABLE"), "{sql}");
    assert!(sql.contains("DROP TABLE"), "{sql}");
}

/// Every client this recipe runs reads none of the machine's option files — `--no-defaults`,
/// first. A `~/.my.cnf` with `[client] password=` otherwise beats `MYSQL_PWD`, and the provisioning
/// probe and the shutdown were refused with `using password: NO` on a machine that had one.
#[test]
fn every_client_it_runs_ignores_the_machines_option_files() {
    let context = context("{}");
    let spec = Mariadb
        .spec(&context)
        .expect("a spec")
        .build()
        .expect("a valid spec");
    let admin = Mariadb.databases().expect("mariadb administers databases");
    let ask = crate::generate::Ask {
        database: "blog".to_owned(),
        user: "blog".to_owned(),
    };
    let credentials = crate::generate::Credentials {
        root: "a".repeat(32),
        account: "b".repeat(32),
    };

    let mut clients: Vec<Vec<String>> = Vec::new();

    let mixengine_proto::ReadyCheck::Command { args, .. } = spec.ready() else {
        panic!("a database is proved up by a command: {:?}", spec.ready());
    };
    clients.push(args.clone());

    if let Some(mixengine_proto::HealthCheck {
        probe: mixengine_proto::HealthProbe::Command { args, .. },
        ..
    }) = spec.health()
    {
        clients.push(args.clone());
    }

    let mixengine_proto::StopBehaviour::Command { args, .. } = spec.stop() else {
        panic!("a database is asked to stop: {:?}", spec.stop());
    };
    clients.push(args.clone());

    clients.push(
        (admin.probe)(&context, &ask, &credentials.root)
            .expect("a probe")
            .args,
    );
    for step in (admin.steps)(
        &context,
        &ask,
        crate::generate::Found::default(),
        &credentials,
    )
    .expect("statements")
    {
        clients.push(step.args);
    }

    for args in clients {
        assert_eq!(
            args.first().map(String::as_str),
            Some(super::super::mysql_family::NO_DEFAULTS),
            "{args:?}"
        );
    }
}
