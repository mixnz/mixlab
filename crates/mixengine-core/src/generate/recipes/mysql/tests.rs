use std::collections::BTreeMap;
use std::path::Path;

use mixengine_proto::{ReadyCheck, ServiceId, StopBehaviour};

use super::*;
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

/// What an 8.x artifact publishes, as `mixengine-packages` publishes it.
fn provides() -> BTreeMap<String, String> {
    [
        ("mysqld", "bin/mysqld"),
        ("mysql", "bin/mysql"),
        ("mysqladmin", "bin/mysqladmin"),
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

/// The same for a 5.6 tree compiled here, which publishes the installer script as well.
fn provides_5_6() -> BTreeMap<String, String> {
    let mut provides = provides();
    provides.insert(INSTALL_DB.to_owned(), "scripts/mysql_install_db".to_owned());
    provides
}

/// A `mysql@main` of `version` in a home at [`root`], with `overrides` applied.
fn context_of(version: &str, provides: BTreeMap<String, String>, overrides: &str) -> Context {
    let service = ServiceId::parse("mysql@main").expect("an id");
    let settings =
        Settings::merge(Mysql.settings(), overrides, &service).expect("usable overrides");

    let context = Context::for_test(
        service,
        PACKAGE,
        Path::new(root()),
        provides,
        Some(3306),
        settings,
    )
    .with_version(version);

    let endpoints = Mysql
        .endpoints(&context)
        .expect("a home this short has a usable socket path");

    context.with_endpoints(endpoints)
}

/// The version this recipe is written against first: the line `services.md` names.
fn context(overrides: &str) -> Context {
    context_of("8.4.10", provides(), overrides)
}

/// The rendered `my.cnf` for `overrides`.
fn rendered(overrides: &str) -> String {
    recipe::render(&Mysql, &context(overrides))
        .expect("a rendering")
        .first()
        .expect("one file")
        .contents()
        .to_owned()
}

/// The same context with the credential a ritual would have been handed.
fn initialised(version: &str, provides: BTreeMap<String, String>) -> Context {
    let mut context = context_of(version, provides, "{}");
    context.put_secret(ROOT, "abcd1234abcd1234abcd1234abcd1234");
    context
}

/// **A reset follows the same route its ritual does, and creates no data directory** — roadmap
/// task **T127**.
///
/// All four rows of the route table, for [`route`]'s own reason: 5.6 is the one published line
/// that repairs itself through `--bootstrap` rather than through `--init-file`, and no machine
/// anybody runs these tests on is running 5.6.
/// **Health is asked in the server's own protocol** — roadmap task T190b, D4. Readiness, which
/// runs once per start, still asks the client program.
#[test]
fn health_is_asked_in_the_protocol_and_readiness_is_still_the_client() {
    let spec = Mysql
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
fn the_reset_follows_the_route_and_creates_no_data_directory() {
    for (version, provides, route, windows) in [
        ("8.4.10", provides(), Route::Initialize, false),
        ("5.7.44", provides(), Route::Initialize, true),
        ("5.6.51", provides_5_6(), Route::Script, false),
        ("5.6.51", provides(), Route::ShippedData, true),
    ] {
        let context = initialised(version, provides);
        let steps = reset_steps_for(&context, route, windows).expect("a reset builds");

        assert!(!steps.is_empty(), "{version} {route:?}");

        for step in &steps {
            let program = step.program.display().to_string();
            assert!(
                !program.contains(INSTALL_DB),
                "{version} {route:?}: a reset step creates a data directory: {program}"
            );
        }
    }
}

/// **Every step of every route, and the file, name one scratch directory of this instance's
/// own** — roadmap task **T33c**, on MariaDB's measurement: the deletion is the same code in
/// this server, and 5.6's installer loads the same system tables through the same temporary
/// ones.
#[test]
fn every_first_run_step_and_the_file_name_this_instances_own_scratch_directory() {
    use crate::generate::recipes::{TMPDIR, scratch_dir};

    for (version, provides, route, windows) in [
        ("8.4.10", provides(), Route::Initialize, false),
        ("5.7.44", provides(), Route::Initialize, true),
        ("5.6.51", provides_5_6(), Route::Script, false),
        ("5.6.51", provides(), Route::ShippedData, true),
    ] {
        let context = initialised(version, provides);
        let scratch = context
            .scratch()
            .expect("this recipe asks for a scratch directory");
        assert_eq!(scratch, scratch_dir(&context));
        assert_eq!(scratch.parent(), context.data().parent(), "{scratch:?}");

        let expected = scratch.display().to_string();
        for step in steps_for(&context, route, windows).expect("the steps") {
            assert_eq!(
                step.env.get(TMPDIR),
                Some(&expected),
                "{version} {route:?}: {step:?}"
            );
        }
    }

    let context = context("{}");
    let expected = context
        .scratch()
        .expect("a scratch directory")
        .display()
        .to_string()
        .replace('\\', "/");
    let rendered = rendered("{}");
    let line = format!("tmpdir = \"{expected}\"");
    assert!(rendered.contains(&line), "no `{line}` in:\n{rendered}");
}

/// Two instances of one server, so its id carries an `@`.
/// **A database is woken at the addresses it listens on itself** — T70a's D4.
///
/// On a system with Unix sockets that is *two* addresses and not one, and the difference is a
/// client's habit rather than a configuration: a generated `.env` names `127.0.0.1`, and the
/// client typed with no host at all names the socket. Waking on only one of them leaves the
/// other hanging against an address nothing holds.
#[test]
fn a_stopped_server_is_woken_at_its_port_and_at_its_socket() {
    let held = Mysql
        .held_while_stopped(&context("{}"))
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

#[test]
fn mysql_is_named_because_a_home_may_have_two() {
    assert_eq!(Mysql.instancing(), Instancing::Named);
}

/// Every path in the rendered file is quoted and forward-slashed.
///
/// MySQL reads its option file with the same parser MariaDB does: `\` escapes and an unquoted
/// `#` starts a comment, so a home under `C:\Users\Nguyen Hai Quang` breaks an unrendered path
/// in two ways at once.
#[test]
fn every_path_in_the_configuration_is_quoted_and_forward_slashed() {
    let rendered = rendered("{}");

    for line in rendered.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        if !key.trim_end().ends_with("dir") && !key.trim_end().contains("log_error") {
            continue;
        }

        let value = value.trim();

        assert!(value.starts_with('"'), "{line}");
        assert!(!value.contains('\\'), "{line}");
    }
}

/// **No `skip-name-resolve`, and it is a measurement rather than a preference.**
///
/// MariaDB's installer creates `root@127.0.0.1`; MySQL's `--initialize-insecure` creates only
/// `root@localhost`. With name resolution off, a client connecting over TCP to 127.0.0.1 is
/// matched as a string and finds no account — so every client, this recipe's own readiness
/// check included, is refused by a server whose log says it is ready.
#[test]
fn the_configuration_does_not_switch_off_the_lookup_that_makes_root_reachable() {
    let rendered = rendered("{}");

    // The comment explaining the absence names it, which is the point of the comment — what
    // must not exist is a line the server reads.
    for line in rendered.lines().filter(|line| !line.starts_with('#')) {
        assert!(!line.contains("skip-name-resolve"), "{rendered}");
    }
}

/// The X Protocol is switched off, in the one spelling every line here can read.
///
/// A second listener on 33060 is a port the allocator never handed out and cannot see, so two
/// instances of MySQL 8 would collide on it however carefully their 3306s were chosen. The
/// `loose-` prefix is what makes one line safe on 5.6 and 5.7, where the option does not exist
/// and an unprefixed one is a server that refuses to start.
#[test]
fn the_second_listener_a_modern_mysql_would_open_is_switched_off() {
    let rendered = rendered("{}");

    assert!(rendered.contains("loose-mysqlx"), "{rendered}");
}

/// **A development machine's defaults, which are not the server's** — roadmap task **T73**.
///
/// Three lines, each holding memory or a disk wait that a laptop pays for and never reads:
/// MyISAM's key cache, the instrumentation MySQL ships on where MariaDB ships it off, and the
/// flush every commit otherwise waits for. Every one of them exists in 5.6, which is the oldest
/// series `docs/features/services.md` offers — an option file with an unknown directive is
/// refused whole, so a younger line would stop the server rather than be ignored.
#[test]
fn the_configuration_is_tuned_for_a_laptop_rather_than_for_a_server() {
    let rendered = rendered("{}");

    assert!(rendered.contains("key_buffer_size = 16M"), "{rendered}");
    assert!(rendered.contains("performance_schema = OFF"), "{rendered}");
    assert!(
        rendered.contains("innodb_flush_log_at_trx_commit = 2"),
        "{rendered}"
    );
}

/// **The relaxed flush is the log's, and the page barriers are untouched.**
///
/// What T73 spends is the last second of committed transactions; what it must never spend is a
/// data directory that will not open, and `innodb_doublewrite` is what keeps a torn page
/// recoverable.
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

/// The file and the readiness check name one port, whatever the row says.
#[test]
fn the_file_and_the_readiness_check_name_one_port() {
    let rendered = rendered("{}");
    let spec = Mysql
        .spec(&context("{}"))
        .expect("a spec")
        .build()
        .expect("a valid spec");

    assert!(rendered.contains("port = 3306"), "{rendered}");
    assert_eq!(spec.ports(), [3306]);

    let ReadyCheck::Command { args, .. } = spec.ready() else {
        panic!("readiness is a client run: {:?}", spec.ready());
    };

    assert!(args.iter().any(|arg| arg == "--port=3306"), "{args:?}");
}

/// The password is named for the supervisor to resolve, and never carried.
#[test]
fn the_credential_is_named_rather_than_carried() {
    let spec = Mysql
        .spec(&context("{}"))
        .expect("a spec")
        .build()
        .expect("a valid spec");

    let ReadyCheck::Command { args, .. } = spec.ready() else {
        panic!("readiness is a client run");
    };

    assert!(
        !args.iter().any(|arg| arg.contains("password")),
        "a password on a command line is readable by every process here: {args:?}"
    );
    assert!(
        spec.env()
            .iter()
            .any(|(key, _)| key == super::PASSWORD_VARIABLE),
        "the client reads its password out of the environment: {:?}",
        spec.env()
    );
}

/// The server reads the file this recipe rendered, and no other.
#[test]
fn the_server_reads_its_own_file_and_no_other() {
    let spec = Mysql
        .spec(&context("{}"))
        .expect("a spec")
        .build()
        .expect("a valid spec");

    let first = spec.args().first().expect("an argument");

    assert!(first.starts_with("--defaults-file="), "{first}");
    assert!(first.ends_with(CONFIG_FILE), "{first}");
}

/// Stopped by asking it to, so the next start is not a crash recovery.
#[test]
fn mysql_is_stopped_by_asking_it_to_shut_down() {
    let spec = Mysql
        .spec(&context("{}"))
        .expect("a spec")
        .build()
        .expect("a valid spec");

    let StopBehaviour::Command { args, .. } = spec.stop() else {
        panic!("a database is asked to stop: {:?}", spec.stop());
    };

    assert!(args.iter().any(|arg| arg == "shutdown"), "{args:?}");
}

/// And has no reload, because MySQL reads its configuration once.
#[test]
fn mysql_has_no_reload_because_it_cannot() {
    let spec = Mysql
        .spec(&context("{}"))
        .expect("a spec")
        .build()
        .expect("a valid spec");

    assert!(spec.reload().is_none(), "{:?}", spec.reload());
}

/// **The route is a table, and it is asked on every system rather than only on this one.**
///
/// Which program makes a data directory is not one question with a version answer: 5.6 answers
/// differently on Windows than on Unix, and a `cfg!` inside the step builder would leave two of
/// the three routes unexercised wherever the tests happen to run.
#[test]
fn the_bootstrap_route_is_a_table_of_three_and_not_a_version_test() {
    assert_eq!(route("8.4.10", true), Route::Initialize);
    assert_eq!(route("8.4.10", false), Route::Initialize);
    assert_eq!(route("5.7.44", false), Route::Initialize);
    assert_eq!(
        route("5.6.51", false),
        Route::Script,
        "5.6 predates --initialize-insecure and has an installer script"
    );
    assert_eq!(
        route("5.6.51", true),
        Route::ShippedData,
        "upstream's 5.6 zip has no installer and ships a built data/ instead"
    );
}

/// A modern line is bootstrapped by the server itself, and then told its password.
///
/// The second step is a server that listens on nothing — `--skip-networking` — which is what
/// keeps a password-less root off 3306 during the one window it would otherwise exist in. It
/// stops itself: the statement after the password is `SHUTDOWN`.
#[test]
fn a_modern_line_initialises_itself_and_sets_the_password_on_a_server_nobody_can_reach() {
    let steps =
        steps_for(&initialised("8.4.10", provides()), Route::Initialize, false).expect("two steps");

    assert_eq!(steps.len(), 2, "{steps:?}");
    assert!(
        steps[0]
            .args
            .iter()
            .any(|arg| arg == "--initialize-insecure"),
        "{:?}",
        steps[0]
    );

    let setting = &steps[1];
    let file = setting
        .secret_file
        .as_ref()
        .expect("the statement goes into a file this step is given");

    assert!(
        setting.args.iter().any(|arg| arg == "--skip-networking"),
        "{setting:?}"
    );
    assert!(
        setting
            .args
            .iter()
            .any(|arg| arg == &format!("--init-file={}", file.path.display())),
        "{setting:?}"
    );
    assert!(
        file.content.contains("ALTER USER 'root'@'localhost'"),
        "the account `--initialize-insecure` creates is the only one there is"
    );
    assert!(
        file.content.contains("SHUTDOWN"),
        "nothing else can stop a server listening on nothing"
    );
    assert!(
        !setting.args.iter().any(|arg| arg == "--shared-memory"),
        "8.4 dropped the Windows transport check this flag works around: {setting:?}"
    );
}

/// **5.7 sets its password in `--bootstrap`, on every system** — the `--init-file` it is given
/// instead runs, sets the password and leaves a server that will not stop.
///
/// Measured against 5.7.44 on Windows: `ready for connections`, then the whole of
/// [`BOOTSTRAP_PATIENCE`] and a first run reported as never finished. `windows` is passed
/// explicitly — see [`steps_for`] — so both halves of the old split are exercised wherever the
/// tests run, and both of them answer the same now.
#[test]
fn five_seven_sets_the_password_in_bootstrap_mode_on_every_system() {
    for windows in [true, false] {
        let steps = steps_for(
            &initialised("5.7.44", provides()),
            Route::Initialize,
            windows,
        )
        .expect("two steps");

        let setting = &steps[1];

        assert!(
            setting.args.iter().any(|arg| arg == "--bootstrap"),
            "windows={windows}: {setting:?}"
        );
        assert!(
            !setting
                .args
                .iter()
                .any(|arg| arg.starts_with("--init-file")),
            "windows={windows}: a server told to stop from a file is one that does not: \
                 {setting:?}"
        );
        assert!(
            setting.secret_file.is_none(),
            "windows={windows}: bootstrap mode reads its statements on stdin: {setting:?}"
        );
        assert!(
            setting
                .stdin
                .as_deref()
                .is_some_and(|sql| sql.contains("authentication_string")),
            "windows={windows}: 5.7.6 removed the `Password` column this would otherwise \
                 write: {setting:?}"
        );
    }
}

/// Neither transport flag survives, because nothing left on this route needs one.
///
/// `--shared-memory` was 5.7-on-Windows' way past `TCP/IP, --shared-memory, or --named-pipe
/// should be configured on NT OS`, and 8.0 dropped that check; a `--bootstrap` opens no listener
/// at all, so 5.7 has nothing to get past either.
#[test]
fn no_route_asks_for_shared_memory_any_more() {
    for (version, route, windows) in [
        ("8.4.10", Route::Initialize, false),
        ("8.4.10", Route::Initialize, true),
        ("5.7.44", Route::Initialize, false),
        ("5.7.44", Route::Initialize, true),
    ] {
        for step in steps_for(&initialised(version, provides()), route, windows).expect("the steps")
        {
            assert!(
                !step.args.iter().any(|arg| arg == "--shared-memory"),
                "{version} windows={windows}: {step:?}"
            );
        }
    }
}

/// The server that sets the password names a socket of its own, and no machine-wide one.
///
/// **Two of these run at once on one machine** — two homes, or the acceptance suite's own two
/// tests — and the built-in `/tmp/mysql.sock` is one name for both of them. Measured against
/// 8.4.10 on Linux: the second server aborts with `Unable to setup unix socket lock file`, and
/// what the user is told is that their first run failed.
#[test]
fn the_bootstrap_server_binds_this_homes_socket_and_not_the_machines() {
    let steps =
        steps_for(&initialised("8.4.10", provides()), Route::Initialize, false).expect("two steps");

    let setting = &steps[1];
    assert!(
        setting
            .args
            .iter()
            .any(|arg| arg.starts_with("--socket=") && arg.ends_with("mysql@main.sock")),
        "{setting:?}"
    );
}

/// On Windows `--socket` names a pipe rather than a path, and this server offers no transport
/// at all — so the flag is left off, exactly as the generated `my.cnf` leaves it off.
#[test]
fn the_bootstrap_server_names_no_socket_on_windows() {
    let steps =
        steps_for(&initialised("8.4.10", provides()), Route::Initialize, true).expect("two steps");

    let setting = &steps[1];
    assert!(
        !setting.args.iter().any(|arg| arg.starts_with("--socket=")),
        "{setting:?}"
    );
}

/// And the X plugin's own listener is off, on every route that starts a server to set a password.
///
/// `/tmp/mysqlx.sock` is a second machine-wide name, and the second half of the same collision:
/// in the measurement above it was the X plugin that complained first. `loose-` because 5.6 and
/// 5.7 have no such option and `mysqld` refuses a whole option it does not know — the argument
/// the generated `my.cnf` already makes for the same line.
#[test]
fn the_bootstrap_server_turns_the_x_protocol_off() {
    for windows in [false, true] {
        let steps = steps_for(
            &initialised("8.4.10", provides()),
            Route::Initialize,
            windows,
        )
        .expect("two steps");

        let setting = &steps[1];
        assert!(
            setting.args.iter().any(|arg| arg == "--loose-mysqlx=OFF"),
            "{setting:?}"
        );
    }
}

/// 8.0 and newer dropped the check 5.7 has, on Windows as much as anywhere else.
#[test]
fn windows_eight_oh_needs_no_shared_memory_because_upstream_dropped_the_check() {
    let steps =
        steps_for(&initialised("8.0.44", provides()), Route::Initialize, true).expect("two steps");

    let setting = &steps[1];
    assert!(
        !setting.args.iter().any(|arg| arg == "--shared-memory"),
        "{setting:?}"
    );
}

/// 5.6 on Unix runs its installer through the interpreter the script's own first line names.
///
/// In a tree compiled by `mixengine-packages` that script is Perl rather than shell, because
/// 5.6's `scripts/CMakeLists.txt` configures `mysql_install_db.pl.in` on every platform and
/// only appends `.pl` to the name on Windows. Handed to `/bin/sh` it answers `use: command not
/// found` and reads like a corrupt archive.
#[test]
fn five_six_on_unix_runs_its_installer_through_the_interpreter_the_script_names() {
    let steps =
        steps_for(&initialised("5.6.51", provides_5_6()), Route::Script, false).expect("the steps");

    let installing = steps
        .iter()
        .find(|step| step.label.contains("data directory"))
        .expect("a step that creates the data directory");

    let script = installing.args.join(" ");

    assert!(script.contains("mysql_install_db"), "{installing:?}");
    assert!(
        script.contains("#!"),
        "the interpreter is read off the script's own first line: {installing:?}"
    );
}

/// 5.6 on Windows has no installer at all, and copies the directory upstream ships built.
#[test]
fn five_six_on_windows_copies_the_data_directory_upstream_ships() {
    let steps =
        steps_for(&initialised("5.6.51", provides()), Route::ShippedData, true).expect("the steps");

    let copying = steps
        .iter()
        .find(|step| step.label.contains("data directory"))
        .expect("a step that creates the data directory");

    assert!(
        copying.args.iter().any(|arg| arg.ends_with("data")),
        "{copying:?}"
    );
}

/// Wherever the password goes, it is never an argument.
///
/// An argument list is readable by every process on the machine through `ps` and Task Manager.
/// The two places it may be are a file this step is given and removed with, and the standard
/// input of a server in bootstrap mode.
#[test]
fn the_password_is_never_in_an_argument_list() {
    const PASSWORD: &str = "abcd1234abcd1234abcd1234abcd1234";

    for (version, provides, route, windows) in [
        ("8.4.10", provides(), Route::Initialize, false),
        ("5.7.44", provides(), Route::Initialize, false),
        ("5.7.44", provides(), Route::Initialize, true),
        ("5.6.51", provides_5_6(), Route::Script, false),
        ("5.6.51", provides(), Route::ShippedData, true),
    ] {
        let steps = steps_for(&initialised(version, provides), route, windows).expect("the steps");

        for step in &steps {
            assert!(
                !step.args.iter().any(|arg| arg.contains(PASSWORD)),
                "{version} {route:?}: {step:?}"
            );
        }

        assert!(
            steps.iter().any(|step| {
                step.stdin
                    .as_ref()
                    .is_some_and(|input| input.contains(PASSWORD))
                    || step
                        .secret_file
                        .as_ref()
                        .is_some_and(|file| file.content.contains(PASSWORD))
            }),
            "{version} {route:?} never sets the password at all"
        );
    }
}

/// A credential that would need escaping in a SQL literal is refused rather than escaped.
#[test]
fn a_credential_that_would_need_escaping_is_refused() {
    let mut context = context_of("8.4.10", provides(), "{}");
    context.put_secret(ROOT, "hunter2'; DROP DATABASE mysql; --");

    steps(&context).expect_err("a password that is not alphanumeric is a bug upstream of here");
}

/// The ritual declares the one credential it needs, and its steps are the ones above.
#[test]
fn mysql_declares_the_one_credential_its_ritual_needs() {
    let ritual = Mysql.ritual().expect("a database has a first run");

    assert_eq!(ritual.secrets.len(), 1);
    assert_eq!(ritual.secrets[0].key, ROOT);
}
/// **A first run is measured before its credentials exist** — the same guarantee, for this
/// recipe's own validation of a stand-in credential. See the MariaDB test of this name for the
/// failure it exists against.
#[test]
fn a_first_run_is_measured_before_its_credentials_exist() {
    let plan = FirstRun::new(&context("{}"), Mysql.ritual().expect("mysql bootstraps"));

    assert!(
        plan.budget() >= BOOTSTRAP_PATIENCE,
        "one bootstrap step alone asks for {BOOTSTRAP_PATIENCE:?}, and the plan measured {:?}",
        plan.budget()
    );
}

/// MySQL and MariaDB provision through one statement builder, and this is what would notice if
/// somebody gave one of them a syntax of its own.
#[test]
fn mysql_provisions_with_the_same_statements_mariadb_does() {
    let admin = Mysql.databases().expect("mysql administers databases");

    assert_eq!(admin.root, ROOT);

    let context = context("{}");
    let ask = crate::generate::Ask {
        database: "shop".to_owned(),
        user: "shop".to_owned(),
    };
    let credentials = crate::generate::Credentials {
        root: "a".repeat(32),
        account: "b".repeat(32),
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
        sql.contains("CREATE DATABASE IF NOT EXISTS `shop`"),
        "{sql}"
    );
    assert!(
        sql.contains("GRANT ALL PRIVILEGES ON `shop`.* TO 'shop'@'localhost'"),
        "{sql}"
    );
}

/// Every client this recipe runs reads none of the machine's option files — `--no-defaults`,
/// first. A `~/.my.cnf` with `[client] password=` otherwise beats `MYSQL_PWD`, and the provisioning
/// probe and the shutdown were refused with `using password: NO` on a machine that had one.
#[test]
fn every_client_it_runs_ignores_the_machines_option_files() {
    let context = context("{}");
    let spec = Mysql
        .spec(&context)
        .expect("a spec")
        .build()
        .expect("a valid spec");
    let admin = Mysql.databases().expect("mysql administers databases");
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
