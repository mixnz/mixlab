//! MySQL: the other database with these programs' names — roadmap task **T34c**.
//!
//! **Not a version of [`mariadb`](super::mariadb)**, and anybody maintaining an application against
//! one of them can say which. It is its own package, its own recipe and its own rows — and the two
//! of them name the same port, which is the whole reason a port is allocated when a row is written
//! rather than taken from a recipe at start ([`crate::services::ports`]).
//!
//! # What was measured rather than assumed
//!
//! Every route below was run against a real server — 8.4.10 and 5.6.51 while this was written, and
//! every published cell by `mixengine-packages`' own `mysql_smoke.py` before the artifact was
//! allowed out:
//!
//! - **`--bootstrap` was deprecated at 5.7.6 and removed at 8.0.0, not removed at 5.7.6.** Measured
//!   against 5.7.44's own `--verbose --help`, which still lists it, and against a run: 1.7 s to set
//!   the password and exit. So 5.7 sets its root password the way 5.6 does, on standard input to a
//!   server that runs the statements and stops; only 8.0 and newer, which really have lost the mode,
//!   put the statement in a file the daemon writes and removes around one step — see
//!   [`SecretFile`].
//! - **`--initialize-insecure` creates exactly one account, `root@localhost`.** MariaDB's installer
//!   creates `root@127.0.0.1` as well, which is what makes its `skip-name-resolve` safe; copying
//!   that line into this template would leave every client refused by a server whose own log says it
//!   is ready for connections. So this configuration does not set it, and the template says why.
//! - **A modern MySQL opens a second listener nobody asked for**: the X Protocol, on 33060, which no
//!   allocation ever handed out and no `services` row records. `loose-mysqlx = OFF` switches it off
//!   on 8.0 and newer and is a warning rather than a refusal on 5.6 and 5.7, where the option does
//!   not exist — one line instead of a version branch inside a template.
//! - **Bootstrapping is a table of three routes and not a version test** — see `Route`. 5.6
//!   answers differently on Windows than on Unix, and its Unix installer is *Perl* in a tree
//!   compiled from source, so what runs it is read off its own first line.
//! - **5.7 cannot be stopped from an `--init-file`, so it is never started with one.** Its Windows
//!   binary first refuses `--skip-networking` alone, aborting with `TCP/IP, --shared-memory, or
//!   --named-pipe should be configured on NT OS`; handed a transport that satisfies that check it
//!   runs the file, logs `ready for connections` and then stays up, the `SHUTDOWN` it was given
//!   having reached a server with nothing yet able to receive it. What that costs is not a failure
//!   but a silence: the whole of `BOOTSTRAP_PATIENCE` spent on a server nobody asked for, and a
//!   first run that never finished. Measured against 5.7.44 — and the reason the fix is a route for
//!   the whole line on every system rather than one more flag for one of them.
//!
//! # What this recipe deliberately does not do
//!
//! **No reload**, for MariaDB's reason: the configuration is read once, at startup.
//!
//! **No authentication-plugin default.** 8.0 wants `default_authentication_plugin`, 8.4 removed that
//! variable and disabled `mysql_native_password` altogether, and 5.6 knows neither. A line that
//! makes an old client work on one release refuses to start on another, and this recipe is found by
//! package name and runs whichever of five lines a user installed. The day a client needs it, it is
//! an override on one instance rather than a default for all of them.
//!
//! **No `mysql_upgrade`.** Running a data directory bootstrapped by one version against a later one
//! is a task of its own; what is recorded is the version that performed the bootstrap, in
//! [`READY_MARKER`](crate::generate::first_run::READY_MARKER).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use mixengine_platform::KEYRING_SERVICE;
use mixengine_proto::{
    HealthCheck, HealthProbe, Millis, ReadyCheck, ServiceSpec, ServiceSpecBuilder, StopBehaviour,
};

use crate::generate::first_run::{Ritual, SecretFile, SecretSpec, Step};
use crate::generate::recipe::{
    Claim, ClientCommand, Context, Endpoints, Instancing, Recipe, TemplateFile, Upstream,
};
use crate::generate::settings::{Preset, Setting};
use crate::{Error, Result};

/// The `packages.name` this recipe is found under.
pub const PACKAGE: &str = "mysql";

/// The server.
const SERVER: &str = "mysqld";

/// The client the readiness check, the health check and the shutdown all run.
const ADMIN: &str = "mysqladmin";

/// The client that runs SQL — roadmap task **T77a**.
///
/// Not [`ADMIN`], which speaks the administrative sub-commands and takes no statements.
const CLIENT: &str = "mysql";

/// The installer 5.6 publishes on Unix and nowhere else — `scripts/mysql_install_db`.
const INSTALL_DB: &str = "mysql_install_db";

/// The rendered configuration, under `etc/<service-id>/`.
const CONFIG_FILE: &str = "my.cnf";

/// What the file holding the statement that sets the root password is called.
///
/// In `run/`, which [`Paths::bootstrap`](crate::paths::Paths::bootstrap) restricts to this account
/// on every start, and named after the service so two instances bootstrapping at once cannot meet.
const INIT_FILE_SUFFIX: &str = "-init.sql";

/// How much memory InnoDB is given, allocated at startup and held with nobody connected.
///
/// 64M against the server's own 128M, for [`mariadb`](super::mariadb)'s reason and measured by the
/// same `bench` suite — roadmap task **T73**.
const BUFFER_POOL: &str = "innodb_buffer_pool_size";

/// How many connections it accepts at once. MySQL's own default.
const MAX_CONNECTIONS: &str = "max_connections";

/// The server's character set.
const CHARACTER_SET: &str = "character_set";

/// The collation that goes with it.
///
/// `utf8mb4_general_ci` and **not** 8.0's own `utf8mb4_0900_ai_ci`, deliberately: this recipe runs
/// whichever of five lines a user installed, and the 0900 collations do not exist before 8.0. A
/// default that refuses to start on 5.7 is a default nobody can use.
const COLLATION: &str = "collation";

/// How long the server is given to answer a ping before the start is a failure, in milliseconds.
const READY_TIMEOUT: &str = "ready_timeout_ms";

/// How long `mysqladmin shutdown` is given before the process group is killed, in milliseconds.
const STOP_GRACE: &str = "stop_grace_ms";

/// How often the server is asked whether it is still answering.
const HEALTH_INTERVAL: Millis = Millis(10_000);

/// How long one of those may take, well inside the interval.
const HEALTH_TIMEOUT: Millis = Millis(5_000);

/// The environment variable MySQL's own clients read a password from.
///
/// The reason no command line this recipe builds carries one.
pub(super) const PASSWORD_VARIABLE: &str = "MYSQL_PWD";

/// The account the ritual gives a password, and everything here authenticates as.
pub(super) const ROOT: &str = "root";

/// What this recipe's ritual needs the daemon to generate.
const SECRETS: &[SecretSpec] = &[SecretSpec {
    key: ROOT,
    length: 32,
}];

/// How long each half of the bootstrap is given.
const BOOTSTRAP_PATIENCE: Millis = Millis(900_000);

/// Which program can make a data directory for this artifact.
///
/// **A table rather than a version test**, because the split is not only between lines: 5.6 answers
/// differently on Windows than it does on Unix, and what decides is what upstream put in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    /// 5.7 and newer: `mysqld --initialize-insecure`, which writes into an empty directory and
    /// leaves root without a password — what a development machine wants, and what `--initialize`
    /// (a random password buried in the error log) does not give.
    Initialize,

    /// 5.6 on Unix: `scripts/mysql_install_db`, reached through a space-free view because upstream's
    /// script does not quote `$basedir`, and run by the interpreter its own first line names.
    Script,

    /// 5.6 on Windows: neither of those exists, and upstream's zip ships a `data/` directory whose
    /// system tables are already built. The documented first run is to copy it.
    ShippedData,
}

/// MySQL, as MixEngine runs it.
#[derive(Debug)]
pub struct Mysql;

impl Recipe for Mysql {
    fn package(&self) -> &'static str {
        PACKAGE
    }

    fn instancing(&self) -> Instancing {
        Instancing::Named
    }

    /// 3306, which MariaDB names too. Whichever of the two is created first is given it.
    fn preferred_port(&self) -> Option<u16> {
        Some(3306)
    }

    /// `root`, the same name `ROOT` addresses the credential under — roadmap task **T82**.
    fn administrator(&self) -> Option<&'static str> {
        Some(ROOT)
    }

    /// MySQL's protocol — T83.
    fn protocol(&self) -> Option<mixengine_proto::DatabaseProtocol> {
        Some(mixengine_proto::DatabaseProtocol::Mysql)
    }

    /// Every client Oracle's archive publishes, under its own name — roadmap task **T130**.
    ///
    /// **All nine are `Own`**, which is what takes `mysql`, `mysqladmin` and `mysqldump` away from
    /// MariaDB the moment this package is installed: a person with both products on one machine and
    /// a `mysql` on their PATH means the one that answers the MySQL wire, and the alias is the half
    /// of that pair that can be spelled another way.
    ///
    /// `mysqlpump` is absent although 5.7 publishes one: it was deprecated in 8.0 and removed in
    /// 8.4, so a name in `bin/` for it would appear and vanish with the version somebody installed.
    /// `mysqld`, `mysql_install_db` and `my_print_defaults` are the supervisor's.
    fn clients(&self) -> &'static [ClientCommand] {
        &[
            ClientCommand {
                name: CLIENT,
                executable: CLIENT,
                claim: Claim::Own,
            },
            ClientCommand {
                name: ADMIN,
                executable: ADMIN,
                claim: Claim::Own,
            },
            ClientCommand {
                name: "mysqldump",
                executable: "mysqldump",
                claim: Claim::Own,
            },
            ClientCommand {
                name: "mysqlcheck",
                executable: "mysqlcheck",
                claim: Claim::Own,
            },
            ClientCommand {
                name: "mysqlshow",
                executable: "mysqlshow",
                claim: Claim::Own,
            },
            ClientCommand {
                name: "mysqlimport",
                executable: "mysqlimport",
                claim: Claim::Own,
            },
            ClientCommand {
                name: "mysqlbinlog",
                executable: "mysqlbinlog",
                claim: Claim::Own,
            },
            ClientCommand {
                name: "mysqlslap",
                executable: "mysqlslap",
                claim: Claim::Own,
            },
            ClientCommand {
                name: "mysql_upgrade",
                executable: "mysql_upgrade",
                claim: Claim::Own,
            },
        ]
    }

    /// Where this instance listens, in the two variables every MySQL-family client reads.
    fn client_env(&self, listen: &Upstream) -> std::collections::BTreeMap<&'static str, String> {
        super::mysql_family::client_env(listen)
    }

    /// `mysqld --version`, which is cheap and touches the server's own machinery.
    fn smoke_test(&self) -> Option<crate::install::SmokeTest> {
        Some(crate::install::SmokeTest {
            executable: SERVER.to_owned(),
            args: vec!["--version".to_owned()],
            unset: &[],
        })
    }

    fn settings(&self) -> &'static [Setting] {
        &[
            Setting {
                key: BUFFER_POOL,
                default: Preset::Text("64M"),
            },
            Setting {
                key: MAX_CONNECTIONS,
                default: Preset::Number(151),
            },
            Setting {
                key: CHARACTER_SET,
                default: Preset::Text("utf8mb4"),
            },
            Setting {
                key: COLLATION,
                default: Preset::Text("utf8mb4_general_ci"),
            },
            Setting {
                key: READY_TIMEOUT,
                default: Preset::Number(120_000),
            },
            Setting {
                key: STOP_GRACE,
                default: Preset::Number(60_000),
            },
        ]
    }

    fn files(&self) -> &'static [TemplateFile] {
        &[TemplateFile {
            path: CONFIG_FILE,
            source: include_str!("mysql/my.cnf"),
        }]
    }

    /// The socket on a system that has them, and nothing else.
    ///
    /// No plugin directory: MySQL derives one from `basedir` on every cell `mixengine-packages`
    /// publishes, which is where this differs from MariaDB on Windows and was measured rather than
    /// assumed.
    fn endpoints(&self, context: &Context) -> Result<Endpoints> {
        if cfg!(windows) {
            return Ok(Endpoints {
                scratch: Some(super::scratch_dir(context)),
                ..Endpoints::default()
            });
        }

        Ok(Endpoints {
            socket: Some(socket_path(context)?),
            plugins: None,
            scratch: Some(super::scratch_dir(context)),
            ..Endpoints::default()
        })
    }

    /// Its port, and its socket where the system has one — T70a's D4.
    ///
    /// **Two addresses and not one**, because which of them a client uses is that client's habit
    /// rather than a setting: a generated `.env` names the port, and the client typed with no host
    /// at all names the socket. [`cfg!`] is a value and not an attribute, so both arms compile
    /// everywhere and a test exercises the branch this machine is not.
    fn held_while_stopped(&self, context: &Context) -> Result<Vec<Upstream>> {
        let mut held = vec![Upstream::Tcp(address(context)?)];

        if !cfg!(windows) {
            held.push(Upstream::Socket(socket_path(context)?));
        }

        Ok(held)
    }

    /// An hour — T70a, design D9, and the number `resource-isolation.md` already publishes.
    ///
    /// **Longer than php-fpm's half hour on purpose.** A pool starts in tens of milliseconds; a
    /// server replays its log first, so a developer coming back to a project after fifty minutes
    /// would pay for the stop rather than benefit from it. What the extra half hour costs is one
    /// idle server's memory, which is the thing being traded and is worth naming.
    ///
    /// **Answerable only now.** Until T70a the daemon could stop this and nothing could start it
    /// again, and a default that idled it would have been a default that broke a home which changed
    /// nothing — which is why the number arrives in the last commit of that task rather than the
    /// first.
    ///
    /// **Only while the home saves resources** — roadmap task **T167b**, ADR 0041. Until then this
    /// number was the default for every home; now a service nobody set is never idle-stopped.
    fn idle_when_saving(&self) -> Option<mixengine_proto::Millis> {
        Some(mixengine_proto::Millis::from_secs(60 * 60))
    }

    /// The server, and the three things that are one client run with one credential.
    /// As MariaDB's, and for its reasons — the two speak one protocol and hide one counter behind
    /// one authentication.
    fn idle_probe(&self, context: &Context) -> Option<mixengine_proto::IdleProbe> {
        context
            .port()
            .map(|port| mixengine_proto::IdleProbe::Connections { port })
    }

    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder> {
        let settings = context.settings();
        let server = context.provided(SERVER)?;
        let admin = context.provided(ADMIN)?;
        let addr = address(context)?;

        Ok(ServiceSpec::builder(context.service().clone(), &server)
            // What a failed start is diagnosed against (T38).
            .ports([addr.port()])
            // **First, and on its own**: `--defaults-file` means *read this file and no other*,
            // which is what keeps a machine's own `/etc/my.cnf` — naming a datadir, a socket and a
            // port — from being inherited into somebody else's database.
            .args([format!(
                "--defaults-file={}",
                context.config(CONFIG_FILE).display()
            )])
            .cwd(context.etc())
            // Named rather than carried: a spec is data and cannot hold a password (ADR 0006).
            .env_from_keyring(
                PASSWORD_VARIABLE,
                KEYRING_SERVICE,
                context.secret_address(ROOT),
            )
            .ready(ReadyCheck::Command {
                program: admin.clone(),
                args: ping(addr),
                timeout: millis(settings.number(READY_TIMEOUT)),
            })
            .health(HealthCheck {
                // The server's greeting, not `mysqladmin ping`: the same answer without starting a
                // process every ten seconds — roadmap task T190b, D4.
                probe: HealthProbe::MysqlGreeting { addr },
                interval: HEALTH_INTERVAL,
                timeout: HEALTH_TIMEOUT,
                failures_before_degraded: 3,
                successes_before_running: 1,
            })
            // Asked rather than signalled: a terminated `mysqld` leaves a dirty buffer pool, and the
            // next start pays for it with a recovery that reads the whole redo log.
            .stop(StopBehaviour::Command {
                program: admin,
                args: {
                    let mut args = connection(addr);
                    args.push("shutdown".to_owned());
                    args
                },
                grace: millis(settings.number(STOP_GRACE)),
            }))
        // **No reload.** MySQL reads its configuration once, at startup.
    }

    /// The data directory, created once, with a root password that exists only in the OS keyring.
    fn ritual(&self) -> Option<Ritual> {
        Some(Ritual {
            secrets: SECRETS,
            steps,
            reset: Some(reset_steps),
        })
    }

    /// The same statements MariaDB gets, through the same builder — roadmap task **T77a**.
    ///
    /// The two servers' *bootstraps* differ, which is why they have two rituals; making a database
    /// is one syntax, and `mysql_provisions_with_the_same_statements_mariadb_does` below is what
    /// would notice if that stopped being true.
    fn databases(&self) -> Option<crate::generate::databases::DatabaseAdmin> {
        Some(crate::generate::databases::DatabaseAdmin {
            root: ROOT,
            probe: |context, ask, root| {
                super::mysql_family::probe(context, ask, root, &client(context, ask)?)
            },
            steps: |context, ask, found, credentials| {
                super::mysql_family::steps(context, ask, found, credentials, &client(context, ask)?)
            },
        })
    }
}

/// Which of the three bootstraps this artifact has.
///
/// `windows` is an argument rather than a [`cfg!`] so that the whole table is exercised wherever the
/// tests run: two of its three answers would otherwise be unreachable on any one machine.
pub(super) fn route(version: &str, windows: bool) -> Route {
    let (major, minor) = version_parts(version);

    // A version this cannot read is treated as a modern one: every line published since 5.7 has
    // `--initialize-insecure`, and the two that do not are the two whose numbers are unmistakable.
    if (major, minor) > (5, 6) {
        return Route::Initialize;
    }

    if windows {
        Route::ShippedData
    } else {
        Route::Script
    }
}

/// The `(major, minor)` a version string starts with. Unparsable is `(0, 0)`, which reads as old.
fn version_parts(version: &str) -> (u32, u32) {
    let mut parts = version
        .split(['.', '-'])
        .filter_map(|part| part.parse::<u32>().ok());

    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// Whether this version sets its root password in `--bootstrap` rather than through `--init-file`.
///
/// **Not the same split as [`route`]**, which is about what builds a data directory. 5.7 builds one
/// the modern way, with `--initialize-insecure`, and then sets the password the old way, because it
/// still has the old way and cannot be made to finish the new one — see the module header. 5.6
/// answers the same and reaches [`bootstrap`] through its own routes regardless.
///
/// Major version alone, because `--bootstrap` went at 8.0.0 and nowhere in between. A string
/// [`version_parts`] cannot read answers `0` and so reads as old, which costs nothing: [`route`]
/// sends that version down 5.6's own routes and it never reaches this question.
fn bootstraps_the_password(version: &str) -> bool {
    version_parts(version).0 < 8
}

/// The things that have to happen before this database is ever started.
///
/// # Errors
///
/// [`Error::ServiceProvidesNothing`] for an install missing one of the commands this needs, and
/// [`Error::SettingValue`] for a credential this recipe will not put in a SQL literal.
fn steps(context: &Context) -> Result<Vec<Step>> {
    steps_for(
        context,
        route(context.version(), cfg!(windows)),
        cfg!(windows),
    )
}

/// The steps for one route, which is what a test can ask for on any system.
///
/// `windows` is an argument for [`route`]'s reason: the 5.7-on-Windows quirk `set_the_password`
/// works around would otherwise be exercised on only one of the two systems the CI runs on.
///
/// # Errors
///
/// As [`steps`].
pub(super) fn steps_for(context: &Context, route: Route, windows: bool) -> Result<Vec<Step>> {
    let password = context.secret(ROOT);

    refuse_a_password_that_needs_escaping(context, password)?;

    match route {
        Route::Initialize => Ok(vec![
            initialize(context)?,
            the_password_step(context, password, windows)?,
        ]),

        Route::Script => {
            let view = super::space_free_view(context);

            Ok(vec![
                super::link_a_space_free_view(context, &view),
                install_db(context, &view)?,
                bootstrap(context, password)?,
                super::remove_the_space_free_view(context, &view),
            ])
        }

        Route::ShippedData => Ok(vec![
            copy_the_shipped_data(context)?,
            bootstrap(context, password)?,
        ]),
    }
}

/// Set the root password in a data directory that already exists — roadmap task **T127**.
///
/// # Errors
///
/// As [`steps`].
fn reset_steps(context: &Context) -> Result<Vec<Step>> {
    reset_steps_for(
        context,
        route(context.version(), cfg!(windows)),
        cfg!(windows),
    )
}

/// The reset for one route, which is what a test can ask for on any system.
///
/// **The route's own password step, and the routes differ** — [`the_password_step`] for
/// `Route::Initialize`, which is `ALTER USER` through `--init-file` on 8.0 and newer and a
/// `--bootstrap` on 5.7; the two older routes write the grant tables directly in `--bootstrap` too.
/// What is dropped from each is the step that *creates* the directory — `initialize`, `install_db`,
/// `copy_the_shipped_data` — because a repair has one already, full of somebody's databases.
///
/// **`windows` is an argument for [`route`]'s reason**, and here it buys more than it does there:
/// 5.6 is the one published line that repairs itself through `--bootstrap`, and no machine anybody
/// runs these tests on is running 5.6.
///
/// **Measured**, against 8.0.44 on Windows: `--skip-networking --init-file` against a populated data
/// directory that had been killed ran XA crash recovery and set the password in 2.3 s, keeping every
/// database. It also failed the first time, with `The innodb_system data file 'ibdata1' must be
/// writable` — because `mysqld.exe` forks a child and the child was still holding the directory.
/// That is a sentence about a live process wearing the words of a file permission, and it is why the
/// daemon proves the directory is writable before it runs any of this.
///
/// # Errors
///
/// As [`steps_for`].
pub(super) fn reset_steps_for(context: &Context, route: Route, windows: bool) -> Result<Vec<Step>> {
    let password = context.secret(ROOT);

    refuse_a_password_that_needs_escaping(context, password)?;

    Ok(match route {
        Route::Initialize => vec![the_password_step(context, password, windows)?],
        Route::Script | Route::ShippedData => vec![bootstrap(context, password)?],
    })
}

/// The step that gives `root` its password on a directory `--initialize-insecure` built.
///
/// One function so that the ritual and the repair cannot drift apart — the T127 reason
/// [`refuse_a_password_that_needs_escaping`] is one function — and the branch inside it is
/// [`bootstraps_the_password`]: 5.7 through a server that reads its statements and exits, 8.0 and
/// newer through one that has to be told to stop.
///
/// # Errors
///
/// As [`steps_for`].
fn the_password_step(context: &Context, password: &str, windows: bool) -> Result<Step> {
    if bootstraps_the_password(context.version()) {
        bootstrap_the_password(context, password)
    } else {
        set_the_password(context, password, windows)
    }
}

/// Refuse a credential that would have to be escaped to be interpolated.
///
/// **Refused rather than escaped**, for MariaDB's reason: the only producer of this value is
/// `mixengine_platform::generate_secret`, whose alphabet is what makes the interpolation below safe
/// without an escaper nobody could prove right.
///
/// One function and not two copies since **T127**, so that the repair cannot end up with a weaker
/// rule than the ritual it repairs.
///
/// # Errors
///
/// [`Error::SettingValue`], naming no value.
fn refuse_a_password_that_needs_escaping(context: &Context, password: &str) -> Result<()> {
    if password.is_empty() || !password.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::SettingValue {
            service: context.service().as_str().to_owned(),
            key: "root password",
            value: "<redacted>".to_owned(),
            reason: "a generated credential is alphanumeric so that it needs no escaping in the \
                     statement that sets it; this one is not, which is a bug in whatever made it",
        });
    }

    Ok(())
}

/// `mysqld --initialize-insecure`: the server builds its own data directory.
///
/// `--no-defaults` first, or the server reads whatever `my.cnf` this machine already has and
/// initialises somewhere else entirely. The error log is named because a failure here has nowhere
/// else to be: the process writes almost nothing to its own output.
fn initialize(context: &Context) -> Result<Step> {
    Ok(Step {
        label: "create the data directory".to_owned(),
        program: context.provided(SERVER)?,
        args: vec![
            "--no-defaults".to_owned(),
            format!("--basedir={}", context.install_path().display()),
            format!("--datadir={}", context.data().display()),
            format!("--log-error={}", bootstrap_log(context).display()),
            "--initialize-insecure".to_owned(),
        ],
        stdin: None,
        secret_file: None,
        env: super::scratch_environment(context),
        cwd: context.etc().to_path_buf(),
        timeout: BOOTSTRAP_PATIENCE,
    })
}

/// Give `root` its password, through a server nothing off this machine can connect to.
///
/// **`--skip-networking` is the whole point.** `--initialize-insecure` leaves an account with no
/// password, and every other way of setting one has a window in which that account is reachable: on
/// 3306, on a temporary port, or through a socket. This server binds no *network* port, runs the two
/// statements it was given and stops itself with the second of them.
///
/// **8.0 and newer only**, which is what makes the paragraph above true: 5.7 runs the file and then
/// stays up regardless, so it is sent to [`bootstrap_the_password`] instead and no version reaching
/// here needs a transport flag to get past a check 8.0 dropped.
///
/// **The Unix socket it still opens is this home's own**, because a server that binds no port binds
/// a socket instead and the built-in name for it is `/tmp/mysql.sock` — one path for every MySQL on
/// the machine. Measured against 8.4.10 on Linux: two of these started together and the second one
/// logged `Another process with pid N is using unix socket file` and aborted, which is a first run
/// failing for a reason that has nothing to do with the home it was failing in.
///
/// The statement is in a file rather than on the command line because an argument list is readable
/// by every process on this machine. The daemon writes that file inside `run/`, which is owner-only,
/// and removes it whatever the step does — see [`SecretFile`].
fn set_the_password(context: &Context, password: &str, windows: bool) -> Result<Step> {
    let init = context
        .run()
        .join(format!("{}{INIT_FILE_SUFFIX}", context.service().as_str()));

    let mut args = vec![
        "--no-defaults".to_owned(),
        format!("--basedir={}", context.install_path().display()),
        format!("--datadir={}", context.data().display()),
        format!("--log-error={}", bootstrap_log(context).display()),
        "--skip-networking".to_owned(),
        // The same two lines the generated `my.cnf` carries, for the same reason and argued there:
        // a default-built server names `/tmp/mysql.sock` and the X plugin names `/tmp/mysqlx.sock`,
        // both of them machine-wide, so this server would collide with any other MySQL on the
        // machine — including a second MixEngine home bootstrapping its own. `loose-` because 5.6
        // and 5.7 have no `mysqlx` and `mysqld` refuses a whole option it does not know.
        "--loose-mysqlx=OFF".to_owned(),
    ];

    if !windows {
        args.push(format!("--socket={}", socket_path(context)?.display()));
    }

    args.push(format!("--init-file={}", init.display()));

    Ok(Step {
        label: "set the root password".to_owned(),
        program: context.provided(SERVER)?,
        args,
        stdin: None,
        secret_file: Some(SecretFile {
            path: init,
            // `--initialize-insecure` creates `root@localhost` and nothing else: no anonymous
            // accounts and no `test` database, so there is nothing here to clean up after it.
            content: format!(
                "ALTER USER '{ROOT}'@'localhost' IDENTIFIED BY '{password}';\nSHUTDOWN;\n"
            ),
        }),
        env: super::scratch_environment(context),
        cwd: context.etc().to_path_buf(),
        timeout: BOOTSTRAP_PATIENCE,
    })
}

/// Set the root password on 5.7, through the same bootstrap mode 5.6 uses.
///
/// **The mode 5.7 was said not to have.** It was deprecated at 5.7.6 and removed at 8.0.0, and
/// reading the first date as the second is what put this line on `--init-file`, where it hangs —
/// the module header has the measurement. Here the server reads two statements on standard input,
/// writes them and exits: 1.7 s against 5.7.44 on Windows, and no listener of any kind to reason
/// about, which is what `--skip-networking`, `--shared-memory` and a socket path were all for.
///
/// **`authentication_string` and not `Password`**, the column 5.7.6 really did remove; and an
/// `ALTER USER` in its place would be refused, because bootstrap mode implies `--skip-grant-tables`.
/// `PASSWORD()` is still there to hash with, deprecated beside `--bootstrap` and removed with it.
///
/// **Nothing is deleted here, unlike [`bootstrap`].** What this route starts from is a directory
/// `--initialize-insecure` built, which holds `root@localhost` and nothing else — no anonymous
/// accounts and no `test` database — and the repair of T127 runs the same step against a directory
/// full of somebody's own work, where removing rows would be the last thing to do.
///
/// **No `--log-error`, for [`bootstrap`]'s reason**: a statement that fails is quoted back by the
/// server that refused it, and this one carries the credential. The error log would be a file on
/// disk holding it. What a failure here can say is therefore what the process itself says, which is
/// the trade ADR 0006 already makes for every step fed on standard input.
fn bootstrap_the_password(context: &Context, password: &str) -> Result<Step> {
    Ok(Step {
        label: "set the root password".to_owned(),
        program: context.provided(SERVER)?,
        args: vec![
            "--no-defaults".to_owned(),
            "--bootstrap".to_owned(),
            format!("--basedir={}", context.install_path().display()),
            format!("--datadir={}", context.data().display()),
        ],
        stdin: Some(format!(
            "USE mysql;\n\
             UPDATE user SET authentication_string = PASSWORD('{password}'), \
             plugin = 'mysql_native_password' WHERE User = '{ROOT}';\n\
             FLUSH PRIVILEGES;\n"
        )),
        secret_file: None,
        env: super::scratch_environment(context),
        cwd: context.etc().to_path_buf(),
        timeout: BOOTSTRAP_PATIENCE,
    })
}

/// `mysql_install_db` as 5.6 on Unix publishes it: a Perl program under a name that says nothing.
///
/// Three things stated that nothing would think to state, each of them measured:
///
/// - **The interpreter comes off the script's own first line.** A tree compiled by
///   `mixengine-packages` configures `mysql_install_db.pl.in` on every platform, so handing this to
///   `/bin/sh` answers `use: command not found` and reads like a corrupt archive.
/// - **`--no-defaults`**, or the script and the server it starts read this machine's own `my.cnf`.
/// - **`/usr/sbin` and `/sbin` on the PATH**: `chown` is in `/usr/sbin` on macOS and `/usr/bin` on
///   Linux, and a cut-down path fails on exactly one of the two.
fn install_db(context: &Context, view: &Path) -> Result<Step> {
    let relative = context
        .provided(INSTALL_DB)?
        .strip_prefix(context.install_path())
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| PathBuf::from(INSTALL_DB));
    let basedir = view.join("basedir");

    Ok(Step {
        label: "create the data directory".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![
            "-c".to_owned(),
            // The shebang is a path off the machine that built the tree, so it is used when it is
            // still there and looked up by name when it is not — and `sh` is the last resort, for a
            // script that has no first line to read.
            "shebang=$(sed -n '1s|^#! *||p' \"$1\" | cut -d' ' -f1); \
             if [ ! -x \"$shebang\" ]; then \
               shebang=$(command -v \"$(basename \"${shebang:-sh}\")\" 2>/dev/null || echo /bin/sh); \
             fi; \
             exec \"$shebang\" \"$1\" --no-defaults --basedir=\"$2\" --datadir=\"$3\" --user=\"$(id -un)\""
                .to_owned(),
            "sh".to_owned(),
            basedir.join(&relative).display().to_string(),
            basedir.display().to_string(),
            view.join("datadir").display().to_string(),
        ],
        stdin: None,
        secret_file: None,
        env: {
            let mut env = super::scratch_environment(context);
            env.insert(
                "PATH".to_owned(),
                format!(
                    "{}:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                    basedir.join("bin").display()
                ),
            );
            env
        },
        cwd: PathBuf::from("/tmp"),
        timeout: BOOTSTRAP_PATIENCE,
    })
}

/// 5.6 on Windows: copy the `data/` directory upstream's zip ships with its system tables built.
///
/// `xcopy` rather than a shell, and directly rather than through `cmd.exe`: a copy needs no command
/// interpreter, and one handed a program's standard input reads it as commands. `/I` says the
/// destination is a directory, `/E` includes the empty ones, `/Q` keeps a hundred and thirty-six
/// file names out of the job log, and `/Y` overwrites without asking a question nobody is there to
/// answer.
fn copy_the_shipped_data(context: &Context) -> Result<Step> {
    Ok(Step {
        label: "create the data directory".to_owned(),
        program: PathBuf::from(r"C:\Windows\System32\xcopy.exe"),
        args: vec![
            context.install_path().join("data").display().to_string(),
            context.data().display().to_string(),
            "/E".to_owned(),
            "/I".to_owned(),
            "/Q".to_owned(),
            "/Y".to_owned(),
        ],
        stdin: None,
        secret_file: None,
        // A copy needs no scratch directory; it carries the variable so that every step of every
        // route does, which is the one shape the recipe's test holds them all to.
        env: super::scratch_environment(context),
        cwd: context.etc().to_path_buf(),
        timeout: BOOTSTRAP_PATIENCE,
    })
}

/// Set the root password on 5.6, through a server in bootstrap mode listening on nothing.
///
/// **5.6's grant tables, which are not 5.7's**: `Password` is the column here and 5.7.6 removed it,
/// so 5.7 gets a bootstrap of its own in [`bootstrap_the_password`] rather than this one. What the
/// two share is the mode — deprecated at 5.7.6, removed at 8.0.0 — and the reason for it.
///
/// The grant tables are written to directly because bootstrap mode implies `--skip-grant-tables`,
/// which refuses `SET PASSWORD` — the same refusal MariaDB's own bootstrap meets. `PASSWORD()` is a
/// function rather than a statement and is available there.
///
/// Every `root` row rather than `root@localhost`, and the anonymous accounts and the `test` database
/// go with them: what this route starts from is a directory an installer or upstream's own zip
/// built, and both of those leave all three.
fn bootstrap(context: &Context, password: &str) -> Result<Step> {
    Ok(Step {
        label: "set the root password and remove the accounts nobody should have".to_owned(),
        program: context.provided(SERVER)?,
        args: vec![
            "--no-defaults".to_owned(),
            "--bootstrap".to_owned(),
            format!("--basedir={}", context.install_path().display()),
            format!("--datadir={}", context.data().display()),
        ],
        stdin: Some(format!(
            "USE mysql;\n\
             UPDATE user SET Password = PASSWORD('{password}'), plugin = 'mysql_native_password' WHERE User = '{ROOT}';\n\
             DELETE FROM user WHERE User = '';\n\
             DELETE FROM user WHERE User = '{ROOT}' AND Host NOT IN ('localhost', '127.0.0.1', '::1');\n\
             DROP DATABASE IF EXISTS test;\n\
             DELETE FROM db WHERE Db LIKE 'test%';\n\
             FLUSH PRIVILEGES;\n"
        )),
        secret_file: None,
        env: super::scratch_environment(context),
        cwd: context.etc().to_path_buf(),
        timeout: BOOTSTRAP_PATIENCE,
    })
}

/// Where a bootstrap says what went wrong, which is not the process's own output.
fn bootstrap_log(context: &Context) -> PathBuf {
    context.logs().join("mysql-bootstrap.err")
}

/// How a client is told which server to ask and who to be.
fn connection(addr: SocketAddr) -> Vec<String> {
    vec![
        super::mysql_family::NO_DEFAULTS.to_owned(),
        "--protocol=TCP".to_owned(),
        format!("--host={}", addr.ip()),
        format!("--port={}", addr.port()),
        format!("--user={ROOT}"),
    ]
}

/// [`connection`], as the account that was just made rather than as root, against its own database.
///
/// The last step of a provisioning is the only thing here that authenticates as anybody but the
/// superuser — the T77a design, D13.
fn as_account(addr: SocketAddr, user: &str, database: &str) -> Vec<String> {
    vec![
        super::mysql_family::NO_DEFAULTS.to_owned(),
        "--protocol=TCP".to_owned(),
        format!("--host={}", addr.ip()),
        format!("--port={}", addr.port()),
        format!("--user={user}"),
        format!("--database={database}"),
    ]
}

/// Everything the provisioning statements need in order to reach this instance.
///
/// # Errors
///
/// A row carrying no port, or an install that publishes no SQL client.
fn client(context: &Context, ask: &crate::generate::Ask) -> Result<super::mysql_family::Client> {
    let addr = address(context)?;

    Ok(super::mysql_family::Client {
        program: context.provided(CLIENT)?,
        variable: PASSWORD_VARIABLE,
        as_root: connection(addr),
        as_account: as_account(addr, &ask.user, &ask.database),
    })
}

/// [`connection`], asking the one question that proves the server answers queries.
fn ping(addr: SocketAddr) -> Vec<String> {
    let mut args = connection(addr);
    args.push("ping".to_owned());
    args
}

/// Where this instance listens on a system with Unix sockets.
///
/// # Errors
///
/// [`Error::SettingValue`] when the path this home would need is longer than the kernel accepts.
fn socket_path(context: &Context) -> Result<PathBuf> {
    let socket = context
        .run()
        .join(format!("{}.sock", context.service().as_str()));

    super::within_socket_limit(context.service().as_str(), "socket", &socket)?;

    Ok(socket)
}

/// Where this instance listens on TCP: the port its row was given, on the address it names.
///
/// # Errors
///
/// [`Error::SettingValue`] when the row carries no port.
fn address(context: &Context) -> Result<SocketAddr> {
    let port = context.port().ok_or_else(|| Error::SettingValue {
        service: context.service().as_str().to_owned(),
        key: "port",
        value: "none".to_owned(),
        reason: "a database listens on a TCP port and this service's row carries none; \
                 `service.create` allocates one",
    })?;

    Ok(SocketAddr::new(
        context
            .bind()
            .parse::<IpAddr>()
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        port,
    ))
}

/// A setting as a length of time, with a negative one read as none at all.
fn millis(number: i64) -> Millis {
    Millis(u64::try_from(number).unwrap_or_default())
}

#[cfg(test)]
mod tests;
