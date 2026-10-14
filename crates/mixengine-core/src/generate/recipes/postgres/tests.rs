use std::path::Path;

use mixengine_proto::ServiceId;

use super::*;
use crate::ServiceError;
use crate::generate::Upstream;
use crate::generate::first_run::FirstRun;
use crate::generate::recipe;
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
///
/// One spelling per command on every route — the Debian cell buys that with a symlink — so this
/// is a layout rather than a search. The five required ones only: the artifact publishes
/// fifteen, and a recipe may rely on these.
fn provides() -> BTreeMap<String, String> {
    ["postgres", "initdb", "pg_ctl", "psql", "pg_isready"]
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                format!("bin/{name}{}", std::env::consts::EXE_SUFFIX),
            )
        })
        .collect()
}

/// What a failed start is diagnosed against — roadmap task **T38**.
#[test]
fn the_spec_declares_the_port_the_cluster_will_bind() {
    let context = context("{}");
    let spec = Postgres
        .spec(&context)
        .expect("a spec")
        .build()
        .expect("a valid spec");

    assert_eq!(spec.ports(), [5432]);
}

/// A `postgres@main` in a home at [`root`], with `overrides` applied.
fn context(overrides: &str) -> Context {
    with_provides(provides(), overrides)
}

/// The same, for an install that publishes something else — or nothing.
fn with_provides(provides: BTreeMap<String, String>, overrides: &str) -> Context {
    let service = ServiceId::parse("postgres@main").expect("an id");
    let settings =
        Settings::merge(Postgres.settings(), overrides, &service).expect("usable overrides");

    let context = Context::for_test(
        service,
        PACKAGE,
        Path::new(root()),
        provides,
        Some(5432),
        settings,
    );
    let endpoints = Postgres
        .endpoints(&context)
        .expect("a home this short has a usable socket path");

    context.with_endpoints(endpoints)
}

/// The rendered `name` for `overrides`.
fn rendered(name: &str, overrides: &str) -> String {
    recipe::render(&Postgres, &context(overrides))
        .expect("a rendering")
        .into_iter()
        .find(|document| document.relative() == Path::new(name))
        .unwrap_or_else(|| panic!("this recipe renders no {name}"))
        .contents()
        .to_owned()
}

/// The built spec for `overrides`.
fn built(overrides: &str) -> ServiceSpec {
    Postgres
        .spec(&context(overrides))
        .expect("a spec")
        .build()
        .expect("a valid spec")
}

/// Two clusters in one home, so the id carries an `@`.
/// **A database is woken at the addresses it listens on itself** — T70a's D4.
///
/// On a system with Unix sockets that is *two* addresses and not one, and the difference is a
/// client's habit rather than a configuration: a generated `.env` names `127.0.0.1`, and the
/// client typed with no host at all names the socket. Waking on only one of them leaves the
/// other hanging against an address nothing holds.
#[test]
fn a_stopped_server_is_woken_at_its_port_and_at_its_socket() {
    let held = Postgres
        .held_while_stopped(&context("{}"))
        .expect("the addresses it is woken at");

    assert!(
        held.contains(&Upstream::Tcp(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            5432
        ))),
        "a client dialling 127.0.0.1:5432 would not wake it: {held:?}"
    );

    assert_eq!(
        held.len(),
        if cfg!(windows) { 1 } else { 2 },
        "the socket is an address on every system that has one: {held:?}"
    );
}

/// **The socket file PostgreSQL would have created, named by the recipe that knows the
/// convention** — T70a's D4.
///
/// `unix_socket_directories` takes a directory and the server creates `.s.PGSQL.<port>` inside
/// it, so [`Endpoints::socket`] holds a directory here and a *file* for MariaDB — which is why
/// its own doc says nothing outside each pair may read it either way, and why this name is
/// derived in this file and nowhere above.
#[test]
#[cfg(not(windows))]
fn a_stopped_server_is_woken_at_the_socket_file_the_server_would_have_created() {
    let held = Postgres
        .held_while_stopped(&context("{}"))
        .expect("the addresses it is woken at");

    assert!(
        held.iter().any(|address| matches!(
            address,
            Upstream::Socket(path) if path.file_name() == Some(".s.PGSQL.5432".as_ref())
        )),
        "nothing here is the file a client finds in unix_socket_directories: {held:?}"
    );
}

#[test]
fn postgres_is_named_because_a_home_may_have_two() {
    assert_eq!(Postgres.instancing(), Instancing::Named);
}

/// Every path in the rendered configuration is quoted and forward-slashed.
///
/// PostgreSQL's parser reads `\` inside a quoted string as an escape, so a home under
/// `C:\Users\Nguyen Hai Quang` breaks an unrendered path — the same measurement `my.cnf` was
/// written from, made against a different parser.
#[test]
fn every_path_in_the_configuration_is_quoted_and_forward_slashed() {
    let rendered = rendered(CONFIG_FILE, "{}");
    let named = [
        "data_directory",
        "hba_file",
        "ident_file",
        "unix_socket_directories",
    ];

    let mut seen = 0;
    for line in rendered
        .lines()
        .filter(|line| named.iter().any(|key| line.starts_with(key)))
    {
        let value = line.split_once('=').expect("a setting").1.trim();

        assert!(
            value.starts_with('\'') && value.ends_with('\''),
            "an unquoted path is not a path to this parser: {line}"
        );
        assert!(
            !value.contains('\\'),
            "a backslash is an escape inside a quoted string: {line}"
        );
        seen += 1;
    }

    assert!(seen >= 3, "no paths were checked at all:\n{rendered}");
}

/// **The wall between this data and everybody else on the machine.**
///
/// Three lines, and not one of them `trust`. `initdb`'s own default would have written `trust`
/// for local connections; this is the file the server actually reads, and it is generated
/// rather than inherited precisely so that default never applies.
#[test]
fn nothing_in_pg_hba_trusts_anybody() {
    let rendered = rendered(HBA_FILE, "{}");

    assert!(
        !rendered
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .any(|line| line.split_whitespace().any(|word| word == "trust")),
        "a `trust` line lets anybody on this machine in as the superuser:\n{rendered}"
    );

    for expected in [
        "local   all   all                scram-sha-256",
        "host    all   all   127.0.0.1/32 scram-sha-256",
        "host    all   all   ::1/128      scram-sha-256",
    ] {
        assert!(
            rendered.contains(expected),
            "{expected} is missing:\n{rendered}"
        );
    }
}

/// `ident_file` has to name a file that exists, and it may not name one inside the cluster.
///
/// `etc/` reaching into the data directory would be generated configuration touching the one
/// place that must never be regenerated — so the file is generated, empty, beside the other two.
#[test]
fn an_empty_pg_ident_is_generated_because_it_has_to_exist() {
    let rendered = rendered(IDENT_FILE, "{}");

    assert!(
        rendered
            .lines()
            .all(|line| line.trim().is_empty() || line.trim_start().starts_with('#')),
        "pg_ident maps an OS account to a database role and this recipe maps none:\n{rendered}"
    );
}

/// An override reaches the file rather than being ignored.
#[test]
fn the_configuration_says_what_the_overrides_said() {
    let rendered = rendered(CONFIG_FILE, r#"{"shared_buffers": "512MB"}"#);

    assert!(rendered.contains("shared_buffers = 512MB"), "{rendered}");
}

/// **A development machine's durability, which is not the server's** — roadmap task **T73**.
///
/// The commit barrier and nothing else: `synchronous_commit = off` costs the last second of
/// committed transactions and cannot leave a cluster that will not open. `fsync = off` would,
/// which is why the second half of this test is the more important one.
#[test]
fn the_commit_barrier_is_relaxed_and_the_recovery_barriers_are_not() {
    let rendered = rendered(CONFIG_FILE, "{}");

    assert!(rendered.contains("synchronous_commit = off"), "{rendered}");

    for line in rendered.lines().filter(|line| !line.starts_with('#')) {
        assert!(!line.contains("fsync"), "{line}\n{rendered}");
        assert!(!line.contains("full_page_writes"), "{line}\n{rendered}");
    }
}

/// The escape hatch is real: `extra` renders after every directive above it, and PostgreSQL
/// reads a file in order, so a later line wins.
#[test]
fn a_user_can_put_the_servers_own_durability_back() {
    let rendered = rendered(CONFIG_FILE, r#"{"extra": "synchronous_commit = on"}"#);

    let relaxed = rendered
        .find("synchronous_commit = off")
        .expect("the recipe states its own value");
    let restored = rendered
        .rfind("synchronous_commit = on")
        .expect("the override reaches the file");

    assert!(restored > relaxed, "{rendered}");
}

/// The server's own output is the supervisor's, which is a line worth stating rather than
/// defaulting: `logging_collector = on` starts a background process that writes into `log/`
/// *inside the data directory*, and a supervisor reading the process's streams would find an
/// empty file and report that the server said nothing.
#[test]
fn the_server_writes_to_the_stream_the_supervisor_is_reading() {
    let rendered = rendered(CONFIG_FILE, "{}");

    assert!(rendered.contains("logging_collector = off"), "{rendered}");
    assert!(
        rendered.contains("log_destination = 'stderr'"),
        "{rendered}"
    );
}

/// The socket is a *directory* here, and the limit is checked against the file inside it.
///
/// `unix_socket_directories` takes a directory and PostgreSQL puts `.s.PGSQL.<port>` in it — so
/// a recipe that measured the directory against 103 characters would be measuring the wrong
/// string by seventeen, and the failure it let through arrives after the server has started.
///
/// **And the directory is `run/` itself, which is the half that was measured the hard way.**
/// A directory named after the service reads better and does not exist: nothing creates one,
/// and the server answers *could not create lock file … No such file or directory* and
/// crash-loops. `run/` is there because the daemon's own socket is in it.
#[cfg(unix)]
#[test]
fn the_socket_endpoint_is_a_directory_that_exists() {
    let context = context("{}");
    let endpoints = Postgres.endpoints(&context).expect("a short home");
    let directory = endpoints.socket.expect("this system has sockets");

    assert_eq!(
        directory,
        context.run(),
        "a socket directory nothing creates is a server that will not start"
    );
    assert!(
        !directory.to_string_lossy().contains(".s.PGSQL"),
        "the endpoint names the file rather than the directory: {}",
        directory.display()
    );
}

/// And a home too deep for one is refused by name, rather than by a server that has started.
#[cfg(unix)]
#[test]
fn a_home_that_cannot_hold_a_socket_is_refused_by_name() {
    let deep = format!("/{}", "d".repeat(120));
    let service = ServiceId::parse("postgres@main").expect("an id");
    let settings = Settings::merge(Postgres.settings(), "{}", &service).expect("usable overrides");
    let context = Context::for_test(
        service,
        PACKAGE,
        Path::new(&deep),
        provides(),
        Some(5432),
        settings,
    );

    let error = Postgres
        .endpoints(&context)
        .expect_err("103 characters is 103 characters");

    assert!(
        matches!(&error, Error::Service(ServiceError::SettingValue { key, .. }) if *key == "socket"),
        "{error:?}"
    );
}

/// There are no Unix sockets on Windows, and the template states that rather than leaving it
/// out.
#[cfg(windows)]
#[test]
fn windows_has_no_socket_and_the_file_says_so() {
    let endpoints = Postgres.endpoints(&context("{}")).expect("endpoints");

    assert!(endpoints.socket.is_none(), "{endpoints:?}");

    let rendered = rendered(CONFIG_FILE, "{}");
    assert!(
        rendered.contains("unix_socket_directories = ''"),
        "{rendered}"
    );
}

/// A row with no port is refused here rather than by a server that will not start.
#[test]
fn a_service_row_with_no_port_is_refused() {
    let service = ServiceId::parse("postgres@main").expect("an id");
    let settings = Settings::merge(Postgres.settings(), "{}", &service).expect("usable overrides");
    let context = Context::for_test(
        service,
        PACKAGE,
        Path::new(root()),
        provides(),
        None,
        settings,
    );

    let error = address(&context).expect_err("a database listens on a port");

    assert!(
        matches!(&error, Error::Service(ServiceError::SettingValue { key, .. }) if *key == "port"),
        "{error:?}"
    );
}

/// The server is pointed at its own file, and at nothing else — and at the cluster only once.
///
/// **`--config-file` alone, with no `-D`.** `data_directory` is stated *in* that file, and
/// naming the cluster twice is two places for it to drift.
#[test]
fn the_server_reads_one_file_and_names_the_cluster_once() {
    let spec = built("{}");

    assert!(
        spec.args()
            .iter()
            .any(|arg| arg.starts_with("--config-file=") && arg.ends_with(CONFIG_FILE)),
        "{:?}",
        spec.args()
    );
    assert!(
        !spec
            .args()
            .iter()
            .any(|arg| arg == "-D" || arg.starts_with("--pgdata")),
        "the cluster is named in the file, not on the command line: {:?}",
        spec.args()
    );
}

/// **Ready and health are two different questions**, and this is where the difference lives.
///
/// `pg_isready` is a good liveness probe and a weak readiness one: it sends a startup packet and
/// distinguishes *accepting* from *rejecting*, which a TCP accept cannot — and it never
/// authenticates. A cluster whose superuser password did not get set would pass it every time.
/// So readiness is a query run as the superuser with the generated password, and health is the
/// cheap probe that needs no credential.
#[test]
fn ready_authenticates_and_health_does_not() {
    let spec = built("{}");

    let ReadyCheck::Command { program, args, .. } = spec.ready() else {
        panic!("a database is proved up by a query: {:?}", spec.ready());
    };
    assert!(
        program.ends_with(format!("psql{}", std::env::consts::EXE_SUFFIX)),
        "{program:?}"
    );
    assert!(args.iter().any(|arg| arg == "SELECT 1"), "{args:?}");

    let HealthProbe::Command { program, .. } = &spec.health().expect("a health check").probe else {
        panic!("a database is watched by asking it, not by connecting to it");
    };
    assert!(
        program.ends_with(format!("pg_isready{}", std::env::consts::EXE_SUFFIX)),
        "{program:?}"
    );
}

/// **The file and the readiness check must name one server.**
///
/// Computed twice — once in Jinja for the file, once in Rust for the check — and the failure
/// when they disagree is a database that starts perfectly and is reported as never having come
/// up.
#[test]
fn the_file_and_the_readiness_check_name_one_port() {
    let spec = built("{}");
    let rendered = rendered(CONFIG_FILE, "{}");

    let ReadyCheck::Command { args, .. } = spec.ready() else {
        panic!("{:?}", spec.ready());
    };

    assert!(args.iter().any(|arg| arg == "--port=5432"), "{args:?}");
    assert!(rendered.contains("port = 5432"), "{rendered}");
}

/// No password reaches an argument list, which every process on the machine can read.
#[test]
fn the_credential_is_named_rather_than_carried() {
    let spec = built("{}");

    assert!(
        matches!(
            spec.env().get(PASSWORD_VARIABLE),
            Some(mixengine_proto::EnvValue::Keyring { service, key })
                if service == KEYRING_SERVICE
                    && key == &format!(
                        "{}/postgres@main/postgres",
                        crate::generate::context::TEST_HOME
                    )
        ),
        "{:?}",
        spec.env()
    );
}

/// **Asked to shut down, not terminated.** A killed postmaster leaves an unclean shutdown and
/// the next start pays for it by replaying the write-ahead log; `--mode fast` disconnects
/// clients and shuts down cleanly. `pg_ctl` finds the postmaster through `postmaster.pid`, so it
/// does not need to have started it.
#[test]
fn postgres_is_stopped_by_asking_it_to_shut_down() {
    let spec = built("{}");

    let StopBehaviour::Command { program, args, .. } = spec.stop() else {
        panic!("a signal leaves a cluster to recover: {:?}", spec.stop());
    };

    assert!(
        program.ends_with(format!("pg_ctl{}", std::env::consts::EXE_SUFFIX)),
        "{program:?}"
    );
    assert!(args.iter().any(|arg| arg == "stop"), "{args:?}");
    assert!(args.iter().any(|arg| arg == "--mode=fast"), "{args:?}");
}

/// **The first service in this catalogue with a real reload on all three systems.**
///
/// MariaDB has none and cannot have one; php-fpm's is a signal, which Windows answers
/// `unsupported`. `pg_ctl reload` is one shape everywhere.
#[test]
fn postgres_reloads_the_same_way_on_every_system() {
    let spec = built("{}");

    let Some(ReloadBehaviour::Command { program, args, .. }) = spec.reload() else {
        panic!("this is the recipe that has one: {:?}", spec.reload());
    };

    assert!(
        program.ends_with(format!("pg_ctl{}", std::env::consts::EXE_SUFFIX)),
        "{program:?}"
    );
    assert!(args.iter().any(|arg| arg == "reload"), "{args:?}");
}

/// A package that publishes none of what this recipe needs is named as such.
#[test]
fn a_postgres_without_its_own_binaries_is_named() {
    let context = with_provides(BTreeMap::new(), "{}");

    let error = Postgres.spec(&context).expect_err("nothing to run");

    assert!(
        matches!(
            error,
            Error::Service(ServiceError::ServiceProvidesNothing { .. })
        ),
        "{error:?}"
    );
}

/// A context carrying the credential the daemon would have generated.
fn initialised(secret: &str) -> Context {
    let mut context = context("{}");
    context.put_secret(SUPERUSER, secret);

    context
}

/// A recipe that declares a credential also declares what to do with it.
#[test]
fn postgres_declares_the_one_credential_its_ritual_needs() {
    let ritual = Postgres.ritual().expect("a database has a first run");

    assert_eq!(ritual.secrets.len(), 1, "{:?}", ritual.secrets);
    assert_eq!(ritual.secrets[0].key, SUPERUSER);
    assert!(ritual.secrets[0].length >= 32, "{:?}", ritual.secrets);
}

/// **The superuser is readable from a provisioning, because a refusal has to be attributed** —
/// roadmap task **T127a**. A server's log says who was refused, and this is what tells a line
/// about *this home's* superuser apart from one about somebody's application account.
#[test]
fn a_provisioning_says_which_account_its_superuser_is() {
    let context = context("{}");
    let admin = Postgres.databases().expect("a database vocabulary");

    let provisioning = crate::generate::databases::Provisioning::new(&context, admin);

    assert_eq!(provisioning.root_user(), SUPERUSER);
    // The two answers are about one account, which is what makes a log line attributable to the
    // credential this home holds rather than merely to somebody.
    assert!(
        provisioning.root_address().ends_with(SUPERUSER),
        "{}",
        provisioning.root_address()
    );
}

/// The keyring entry the spec names and the one the ritual is stored under are one address.
///
/// Composed twice, and the failure when they disagree is a server that starts and a client that
/// cannot authenticate against it — reported as a service that never became ready.
#[test]
fn the_spec_and_the_ritual_name_one_credential() {
    let context = context("{}");
    let spec = built("{}");

    let named = spec
        .env()
        .get(PASSWORD_VARIABLE)
        .expect("the spec names a credential");

    assert!(
        matches!(named, mixengine_proto::EnvValue::Keyring { key, .. }
            if key == &context.secret_address(SUPERUSER)),
        "{named:?}"
    );
    // The home is in front of the service — roadmap task **T126**, and MariaDB's own test
    // says why.
    assert_eq!(
        context.secret_address(SUPERUSER),
        format!(
            "{}/postgres@main/postgres",
            crate::generate::context::TEST_HOME
        )
    );
}

/// **`initdb` is told its locale and its encoding, because otherwise it reads the machine's.**
///
/// Measured in `mixengine-packages`: on a machine whose system locale it does not recognise it
/// reports *could not find suitable text search configuration*, sets the default to `simple`,
/// and **exits zero**. Two developers, two databases that answer differently.
///
/// And `reject` rather than `scram-sha-256`, which `initdb` refuses without a password to go
/// with it — see [`create_the_cluster`].
#[test]
fn the_cluster_is_created_with_a_locale_this_recipe_chose() {
    let steps = steps(&initialised("hunter2")).expect("steps");
    let initdb = &steps[0];

    assert!(
        initdb.args.iter().any(|arg| arg == "--locale=C"),
        "{:?}",
        initdb.args
    );
    assert!(
        initdb.args.iter().any(|arg| arg == "--encoding=UTF8"),
        "{:?}",
        initdb.args
    );
    assert!(
        initdb.args.iter().any(|arg| arg == "--auth-local=reject"),
        "{:?}",
        initdb.args
    );
    assert!(
        initdb.args.iter().any(|arg| arg == "--auth-host=reject"),
        "{:?}",
        initdb.args
    );
}

/// And an override of either reaches it, because a user who wants their production collation
/// says so — and owns the per-OS spelling when they do.
#[test]
fn a_stated_locale_reaches_initdb() {
    let mut context = context(r#"{"locale": "en_US.UTF-8"}"#);
    context.put_secret(SUPERUSER, "hunter2");

    let steps = steps(&context).expect("steps");

    assert!(
        steps[0]
            .args
            .iter()
            .any(|arg| arg == "--locale=en_US.UTF-8"),
        "{:?}",
        steps[0].args
    );
}

/// **No `--pwfile`, and the reason is the whole shape of this ritual.**
///
/// `initdb` will only take a password from a file, and a file is a plaintext superuser
/// credential on disk for the whole of a bootstrap that can take minutes — and one a half-failed
/// ritual leaves behind. So the role is created with **no** password and the password is set
/// through single-user mode, which opens no port and no socket: there is no instant at which a
/// password-less superuser is reachable by anybody.
#[test]
fn the_password_is_set_through_a_server_listening_on_nothing() {
    let steps = steps(&initialised("hunter2")).expect("steps");

    assert!(
        !steps
            .iter()
            .any(|step| step.args.iter().any(|arg| arg.starts_with("--pwfile"))),
        "a plaintext credential on disk: {steps:?}"
    );

    let single = steps
        .iter()
        .find(|step| step.stdin.is_some())
        .expect("one step speaks SQL");

    assert!(
        single.args.iter().any(|arg| arg == "--single"),
        "{:?}",
        single.args
    );
    assert!(
        single
            .program
            .ends_with(format!("postgres{}", std::env::consts::EXE_SUFFIX)),
        "{:?}",
        single.program
    );

    let sql = single.stdin.as_deref().expect("the SQL is on stdin");
    assert!(
        sql.contains("ALTER ROLE postgres PASSWORD 'hunter2';"),
        "{sql}"
    );

    // Single-user mode reads one statement per line, so a statement wrapped over two would be
    // two statements and neither of them valid.
    for line in sql.lines().filter(|line| !line.trim().is_empty()) {
        assert!(
            line.trim_end().ends_with(';'),
            "a wrapped statement: {line}"
        );
    }
}

/// A credential that would need escaping is refused rather than escaped.
///
/// `mixengine_platform::generate_secret` restricts its alphabet for exactly the interpolation
/// above; this is the other end of that arrangement.
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

/// Two steps, the same two, on every system — which is the difference from MariaDB, whose
/// bootstrap is two different programs and four steps on Unix.
#[test]
fn the_ritual_is_two_steps_everywhere() {
    let steps = steps(&initialised("abc123")).expect("steps");

    assert_eq!(
        steps.len(),
        2,
        "{:?}",
        steps.iter().map(|step| &step.label).collect::<Vec<_>>()
    );
    assert!(
        steps.iter().all(|step| step.program.is_absolute()),
        "a relative program is whatever the PATH says at the moment it runs: {steps:?}"
    );
}

/// An install missing `initdb` is named, rather than failing at the moment of the first run.
#[test]
fn a_postgres_that_cannot_bootstrap_is_named() {
    let mut without = provides();
    without.remove("initdb");

    let mut context = with_provides(without, "{}");
    context.put_secret(SUPERUSER, "abc123");

    let error = steps(&context).expect_err("there is nothing to create a cluster with");

    assert!(
        matches!(
            error,
            Error::Service(ServiceError::ServiceProvidesNothing { .. })
        ),
        "{error:?}"
    );
}
/// **A first run is measured before its credentials exist** — the same guarantee, for this
/// recipe's own validation of a stand-in credential. See the MariaDB test of this name for the
/// failure it exists against.
#[test]
fn a_first_run_is_measured_before_its_credentials_exist() {
    let plan = FirstRun::new(
        &context("{}"),
        Postgres.ritual().expect("postgres bootstraps"),
    );

    assert!(
        plan.budget() >= BOOTSTRAP_PATIENCE,
        "one bootstrap step alone asks for {BOOTSTRAP_PATIENCE:?}, and the plan measured {:?}",
        plan.budget()
    );
}

/// **A reset sets the password and never creates a cluster** — roadmap task **T127**.
///
/// `--single` is the assertion beside the negative one: the repair has to reach the cluster
/// through a backend that opens no port and no socket, for the reason
/// [`set_the_password`] exists at all. A reset that started the real server to run an `ALTER
/// ROLE` would put a cluster whose password nobody knows on a TCP port first.
#[test]
fn the_reset_sets_the_password_and_never_creates_a_cluster() {
    let plan = FirstRun::new(
        &context("{}"),
        Postgres.ritual().expect("postgres bootstraps"),
    );

    let steps = plan
        .reset_steps(BTreeMap::from([(SUPERUSER.to_owned(), "a".repeat(32))]))
        .expect("a reset builds")
        .expect("postgres declares one");

    assert_eq!(steps.len(), 1, "{steps:?}");
    assert!(
        steps[0].args.iter().any(|arg| arg == "--single"),
        "the reset must reach the cluster through a backend that opens no port: {:?}",
        steps[0].args
    );
    assert!(
        !steps[0].program.display().to_string().contains(INITDB),
        "the reset runs initdb: {:?}",
        steps[0].program
    );
    assert!(
        steps[0]
            .stdin
            .as_deref()
            .is_some_and(|sql| sql.contains("ALTER ROLE")),
        "{:?}",
        steps[0].stdin
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
    let admin = Postgres
        .databases()
        .expect("postgres administers databases");
    let (ask, credentials) = asked();

    (admin.steps)(&context, &ask, found, &credentials)
        .expect("statements")
        .iter()
        .filter_map(|step| step.stdin.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// **The validator's alphabet is exactly what every accepted character survives being embedded
/// as** — roadmap task **T77b**. A chosen password containing every printable-ASCII character
/// the validator accepts must still produce SQL whose `LOGIN PASSWORD '...'` literal (on the
/// `ALTER ROLE` line, the plain one and not the `\gexec`-wrapped `CREATE ROLE`) closes where it
/// should.
#[test]
fn every_accepted_password_character_survives_the_login_password_literal() {
    let chosen =
        crate::generate::databases::validated_password("aZ09!\"#$%&()*+,-./:;<=>?@[]^_`{|}~")
            .expect("every one of these is accepted");

    let context = context("{}");
    let admin = Postgres
        .databases()
        .expect("postgres administers databases");
    let (ask, _) = asked();
    let credentials = crate::generate::Credentials {
        root: "a".repeat(32),
        account: chosen.clone(),
    };

    let sql = (admin.steps)(
        &context,
        &ask,
        crate::generate::Found::default(),
        &credentials,
    )
    .expect("statements")
    .iter()
    .filter_map(|step| step.stdin.clone())
    .collect::<Vec<_>>()
    .join("\n");

    assert!(
        sql.contains(&format!(
            "ALTER ROLE \"blog\" WITH LOGIN PASSWORD '{chosen}';"
        )),
        "the chosen password must appear as one intact literal: {sql}"
    );
}

/// **Design D6.** The role is created before the database, because the database is created
/// *owned by* it — and from PostgreSQL 15 `GRANT ALL ON DATABASE` no longer carries `CREATE` on
/// the `public` schema, so a role granted everything still cannot make a table.
#[test]
fn the_role_is_made_before_the_database_that_it_owns() {
    let sql = provisioning_sql(crate::generate::Found::default());

    let role = sql.find("CREATE ROLE").expect("a role is created");
    let database = sql.find("CREATE DATABASE").expect("a database is created");

    assert!(role < database, "the database is created first:\n{sql}");
    assert!(
        sql.contains(r#"CREATE DATABASE "blog" OWNER "blog""#),
        "{sql}"
    );
}

/// PostgreSQL has no `IF NOT EXISTS` for either object, so the conditional is psql's `\gexec` —
/// on a line of its own, where a meta-command is unambiguous.
#[test]
fn each_creation_is_guarded_because_neither_has_an_if_not_exists() {
    let sql = provisioning_sql(crate::generate::Found::default());

    assert!(sql.contains("pg_roles"), "{sql}");
    assert!(sql.contains("pg_database"), "{sql}");
    assert_eq!(sql.matches("\n\\gexec\n").count(), 2, "{sql}");
}

/// **A database that was already there does not change owner.** D3's deed covers the account,
/// not the database, and `ALTER DATABASE … OWNER TO` would hand somebody else's database over.
#[test]
fn an_existing_database_is_granted_and_never_taken_over() {
    let sql = provisioning_sql(crate::generate::Found {
        database: true,
        user: false,
    });

    assert!(!sql.contains("ALTER DATABASE"), "{sql}");
    assert!(
        sql.contains(r#"GRANT ALL PRIVILEGES ON DATABASE "blog" TO "blog""#),
        "{sql}"
    );
}

/// **Design D13.** The last step logs in as the new role and creates a *real* table in `public`
/// — a temporary one lives in a schema of its own and would prove nothing about the ownership
/// this recipe exists to get right.
#[test]
fn the_last_step_writes_a_real_table_as_the_new_role() {
    let context = context("{}");
    let admin = Postgres
        .databases()
        .expect("postgres administers databases");
    let (ask, credentials) = asked();

    let steps = (admin.steps)(
        &context,
        &ask,
        crate::generate::Found::default(),
        &credentials,
    )
    .expect("statements");
    let last = steps.last().expect("there is a last step");
    let sql = last.stdin.as_deref().unwrap_or_default();

    assert!(
        last.args.contains(&"--username=blog".to_owned()),
        "{:?}",
        last.args
    );
    assert!(
        last.args.contains(&"--dbname=blog".to_owned()),
        "{:?}",
        last.args
    );
    assert_eq!(
        last.env.get(PASSWORD_VARIABLE),
        Some(&credentials.account),
        "the check must authenticate as the new role and not as the superuser"
    );
    assert!(sql.contains("CREATE TABLE public."), "{sql}");
    assert!(!sql.to_ascii_uppercase().contains("TEMPORARY"), "{sql}");
    assert!(!sql.to_ascii_uppercase().contains("TEMP TABLE"), "{sql}");
}

/// No password in any argument, on the MariaDB recipe's reasoning: every process on this machine
/// can read an argument list.
#[test]
fn provisioning_puts_no_credential_in_an_argument() {
    let context = context("{}");
    let admin = Postgres
        .databases()
        .expect("postgres administers databases");
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
