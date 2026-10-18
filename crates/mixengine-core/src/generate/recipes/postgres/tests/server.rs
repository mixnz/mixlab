//! The server: its configuration, socket, readiness, stop and reload.

use super::*;

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
