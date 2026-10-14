//! MariaDB: the database MixEngine runs for a site — roadmap task **T33**.
//!
//! **The first recipe with something to do before it can ever start.** Caddy and php-fpm are a
//! rendered file and a command line; a database is a rendered file, a command line, *and* a data
//! directory that has to be created once by a different program, with a root password that must
//! exist nowhere on disk. The second half is [`Recipe::ritual`] and
//! [`first_run`](crate::generate::first_run); this module's own share of it is the steps.
//!
//! # What was measured rather than assumed
//!
//! Every platform difference below comes out of T33a, which ran a real server on all three systems
//! rather than reading about one:
//!
//! - **`mariadb-install-db` is two different programs.** A shell script on Unix, a C++ program of
//!   the same name on Windows, sharing almost no options —
//!   `--auth-root-authentication-method=normal` is Unix-only and the Windows build answers `unknown
//!   variable` and exits 7.
//! - **Upstream's script does not quote `$basedir`**, so a path with a space in it is split into two
//!   arguments and the script stops with "Could not find my_print_defaults". A user called
//!   `Nguyen Hai Quang` is not an edge case.
//! - **`sockaddr_un` caps a socket path at 103 characters** and the server aborts *after* InnoDB has
//!   started, which reads like a storage failure and is not one.
//! - **Windows `mariadbd` writes only to `<datadir>/<hostname>.err`** and sends nothing to stdout,
//!   so a supervisor reading the process's own output finds an empty file.
//! - **MariaDB's option parser treats `\` as an escape** and everything after an unquoted `#` as a
//!   comment, which is why every path in the rendered file is quoted and forward-slashed.
//! - **From 11.4 a server with no certificate configured generates one at every start** — a
//!   4096-bit RSA key, four to thirteen seconds on a CI runner — and an 11.4 client with a password
//!   on its command line refuses a server that has turned TLS off. So the recipe asks for a leaf of
//!   this home's authority (T99) rather than for `skip-ssl`, which was tried and withdrawn.
//!
//! # What this recipe deliberately does not do
//!
//! **No reload.** MariaDB reads its configuration once, at startup. A changed override takes effect
//! when somebody restarts the service, and the daemon does not restart what nobody asked it to.
//!
//! **No `unix_socket` authentication.** Root is created with a password, which is the one
//! arrangement that means the same thing on all three systems — and the credential lives in the OS
//! keyring, never in the rendered file (ADR 0006).
//!
//! **No `mariadb-upgrade`.** Running a data directory bootstrapped by one version against a later
//! one is a task of its own; what is recorded here is the version that performed the bootstrap, in
//! [`READY_MARKER`](crate::generate::first_run::READY_MARKER), so that task has something to read.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use mixengine_platform::KEYRING_SERVICE;
use mixengine_proto::{
    HealthCheck, HealthProbe, Millis, ReadyCheck, ServiceSpec, ServiceSpecBuilder, StopBehaviour,
};

use crate::ServiceError;
use crate::generate::context::Context;
use crate::generate::first_run::{Ritual, SecretSpec, Step};
use crate::generate::recipe::{
    Claim, ClientCommand, Endpoints, Instancing, Recipe, TemplateFile, Upstream,
};
use crate::generate::settings::{Preset, Setting};
use crate::{Error, Result};

/// The `packages.name` this recipe is found under.
pub const PACKAGE: &str = "mariadb";

/// The server. Asked for by name rather than by path: the archive puts it at `bin/mariadbd`, and
/// upstream renamed it from `mysqld` between 10.4 and 10.6 — so the index's own `provides` map is
/// what knows.
const SERVER: &str = "mariadbd";

/// The client the readiness check, the health check and the shutdown all run.
const ADMIN: &str = "mariadb-admin";

/// The client that runs SQL — roadmap task **T77a**.
///
/// Not [`ADMIN`], which speaks the administrative sub-commands (`ping`, `shutdown`) and takes no
/// statements: making a database is SQL, and this is what reads it.
const CLIENT: &str = "mariadb";

/// The one-shot that bootstraps a data directory — a shell script on Unix and a different C++
/// program of the same name on Windows. See the module note.
const INSTALL_DB: &str = "mariadb-install-db";

/// The dump tool, fronted in `bin/` under this name and under `mysqldump` — roadmap task **T130**.
const DUMP: &str = "mariadb-dump";

/// The physical backup tool. Fronted under its own name only: MySQL's equivalent is a different
/// product with different arguments, so there is no spelling to stand in for.
const BACKUP: &str = "mariadb-backup";

/// The post-upgrade table check, which a person runs by hand after moving a data directory between
/// branches. Fronted for that, and not run by anything here.
const UPGRADE: &str = "mariadb-upgrade";

/// The rendered configuration, under `etc/<service-id>/`.
const CONFIG_FILE: &str = "my.cnf";

/// How much memory InnoDB is given, allocated at startup and held with nobody connected.
///
/// **64M against the server's own 128M** — roadmap task **T73**, and the comment above this line
/// claimed to be dev-tuned for two phases while rendering upstream's number. A development database
/// is small enough that its working set fits either way; what the difference buys is memory a
/// laptop is not holding at every moment it is not being used.
///
/// A user with a large dump raises it in one override, and `mix service list` shows what they set.
/// The `bench` job's `tuned_footprint` suite is what keeps this honest in the other direction.
const BUFFER_POOL: &str = "innodb_buffer_pool_size";

/// How many connections it accepts at once. MariaDB's own default, which is ample for one machine.
const MAX_CONNECTIONS: &str = "max_connections";

/// The server's character set.
const CHARACTER_SET: &str = "character_set";

/// The collation that goes with it.
///
/// `utf8mb4_general_ci` and **not** 11.4's own `utf8mb4_uca1400_ai_ci`, deliberately: this recipe is
/// found by package name and runs whichever series a user installed, and the 1400 collations do not
/// exist before 11.4. A default that fails to start on 10.11 is a default nobody can use.
const COLLATION: &str = "collation";

/// How long the server is given to answer a ping before the start is a failure, in milliseconds.
///
/// Two minutes. Long for what it usually is — a warm start answers in a second or two — and it is
/// not the warm start this is sized for: a first InnoDB recovery after an unclean stop reads the
/// whole redo log before it accepts a single query.
const READY_TIMEOUT: &str = "ready_timeout_ms";

/// How long `mariadb-admin shutdown` is given before the process group is killed, in milliseconds.
///
/// A minute, and for the same reason: flushing a dirty buffer pool is what this waits for, and a
/// database killed part-way through it is a database that recovers on the next start.
const STOP_GRACE: &str = "stop_grace_ms";

/// How often the server is asked whether it is still answering.
const HEALTH_INTERVAL: Millis = Millis(10_000);

/// How long one of those may take. Well inside the interval, which [`ServiceSpec::validate`] insists
/// on: two probes that could overlap are two probes that can queue.
const HEALTH_TIMEOUT: Millis = Millis(5_000);

/// The environment variable MariaDB's own clients read a password from.
///
/// **The reason there is no password on any command line this recipe builds.** An argument list is
/// visible to every process on the machine through `ps` and Task Manager; an environment is not, and
/// this is the variable the client library was given for exactly that.
pub(super) const PASSWORD_VARIABLE: &str = "MYSQL_PWD";

/// The account the ritual creates and everything here authenticates as.
pub(super) const ROOT: &str = "root";

/// What this recipe's ritual needs the daemon to generate.
///
/// Thirty-two characters of `[A-Za-z0-9]` — 190 bits — and the alphabet is what makes the SQL
/// interpolation in [`bootstrap`] safe without an escaper. See
/// [`generate_secret`](mixengine_platform::generate_secret).
const SECRETS: &[SecretSpec] = &[SecretSpec {
    key: ROOT,
    length: 32,
}];

/// How long each half of the bootstrap is given.
///
/// Fifteen minutes: `mariadb-install-db` starts a server of its own and loads the whole system
/// schema, and on a cold Windows machine Defender reads every one of those files on the way past.
const BOOTSTRAP_PATIENCE: Millis = Millis(900_000);

/// MariaDB, as MixEngine runs it.
#[derive(Debug)]
pub struct Mariadb;

impl Recipe for Mariadb {
    fn package(&self) -> &'static str {
        PACKAGE
    }

    /// As many as are named: `mariadb@main`, `mariadb@legacy`. A machine that serves two projects
    /// with incompatible schemas needs two databases, not one with two schemas in it.
    fn instancing(&self) -> Instancing {
        Instancing::Named
    }

    /// 3306, which MySQL names too — see [`Recipe::preferred_port`]. Whichever of the two is
    /// created first gets it.
    fn preferred_port(&self) -> Option<u16> {
        Some(3306)
    }

    /// `root`, the same name `ROOT` addresses the credential under — roadmap task **T82**.
    fn administrator(&self) -> Option<&'static str> {
        Some(ROOT)
    }

    /// MySQL's protocol: a fact about the server, whichever client is on the other end — T83.
    fn protocol(&self) -> Option<mixengine_proto::DatabaseProtocol> {
        Some(mixengine_proto::DatabaseProtocol::Mysql)
    }

    /// The five programs a person runs, and the three spellings they may already have in their
    /// fingers — roadmap task **T130**.
    ///
    /// **MariaDB is the only recipe with an alias**, because it is the only product here that
    /// renamed its own client: the Windows archive at 12.3 publishes `mariadb.exe` and no
    /// `mysql.exe`, while every tutorial, script and habit in the world says `mysqldump`. Each
    /// alias stands for a program this package does publish, and all three disappear the moment the
    /// `mysql` package is installed beside it — a real name always beats a spelling.
    ///
    /// `mariadb-install-db` is deliberately absent although the artifact publishes it. It writes a
    /// data directory the daemon owns, and a person running it by hand against a live instance is a
    /// corrupted one; it stays reachable at its full path.
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
                name: DUMP,
                executable: DUMP,
                claim: Claim::Own,
            },
            ClientCommand {
                name: BACKUP,
                executable: BACKUP,
                claim: Claim::Own,
            },
            ClientCommand {
                name: UPGRADE,
                executable: UPGRADE,
                claim: Claim::Own,
            },
            ClientCommand {
                name: "mysql",
                executable: CLIENT,
                claim: Claim::Alias,
            },
            ClientCommand {
                name: "mysqladmin",
                executable: ADMIN,
                claim: Claim::Alias,
            },
            ClientCommand {
                name: "mysqldump",
                executable: DUMP,
                claim: Claim::Alias,
            },
        ]
    }

    /// Where this instance listens, in the two variables every MySQL-family client reads.
    fn client_env(&self, listen: &Upstream) -> std::collections::BTreeMap<&'static str, String> {
        super::mysql_family::client_env(listen)
    }

    /// `mariadbd --version`, which is cheap and touches the server's own machinery.
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
            source: include_str!("mariadb/my.cnf"),
        }]
    }

    /// The socket on a system that has them, and the plugin directory on the one that needs saying.
    ///
    /// [`cfg!`] is a *value* and not an attribute, so both arms compile everywhere and a test can
    /// exercise the branch the machine it runs on is not.
    fn endpoints(&self, context: &Context) -> Result<Endpoints> {
        if cfg!(windows) {
            return Ok(Endpoints {
                socket: None,
                plugins: Some(context.install_path().join("lib").join("plugin")),
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

    /// The server, and the three things that are all one client run with one credential.
    ///
    /// **Readiness, health and shutdown all speak SQL over TCP**, and every one of them is the same
    /// decision: a TCP accept proves a listener, which stays true for the whole of InnoDB's crash
    /// recovery while the server refuses every query. `--protocol=TCP` even on Unix, where a socket
    /// exists, because the socket is the thing the *configuration* names and the port is the thing
    /// the row does — and a probe should ask the question a client would.
    /// A database with an open session is in use, whether or not a query is running.
    ///
    /// **Connections and not the query counter**, which is what the roadmap line for T69 asked for
    /// and what cannot be had: `Queries` comes out of `SHOW GLOBAL STATUS`, which means speaking
    /// MySQL's protocol as an authenticated user, and this probe lives on a `ServiceSpec`, which
    /// never carries a secret (ADR 0006).
    /// The error a connection count makes is in the safe direction: a client connected and idle
    /// reads as busy, so a database is kept running that could have been stopped, and one somebody
    /// is holding open is never stopped.
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
            // What a failed start is diagnosed against (T38): the port this server will bind, and
            // not the socket beside it — a path cannot be held by a program with no `services` row.
            .ports([addr.port()])
            // **One option, and it has to be the first one.** `--defaults-file` means *read this
            // file and no other*, which is the difference between running this instance and running
            // whatever the machine already has: a user with a MariaDB of their own has an
            // `/etc/mysql/my.cnf` naming a datadir, a socket and a port, and silently inheriting any
            // of them would be writing into somebody else's database.
            //
            // **Not preceded by `--no-defaults`**, which is the shape this was first written in and
            // which does not work: MariaDB honours whichever of the two comes first, so the pair
            // means "read nothing at all" — and a server that read nothing looks for its data
            // directory beside its own binary. Measured rather than reasoned about: it started,
            // failed to `chdir`, and crash-looped six times before the supervisor gave up.
            .args([format!(
                "--defaults-file={}",
                context.config(CONFIG_FILE).display()
            )])
            .cwd(context.etc())
            // The credential the three client runs below need, named rather than carried: a spec is
            // data and cannot hold a password (ADR 0006). The supervisor reads it out of the OS
            // keyring at spawn time and keeps the resolved environment for the life of the process,
            // which is what lets a health probe and a shutdown authenticate too.
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
                // The server's greeting, not `mariadb-admin ping`: the same answer without starting
                // a process every ten seconds — roadmap task T190b, D4.
                probe: HealthProbe::MysqlGreeting { addr },
                interval: HEALTH_INTERVAL,
                timeout: HEALTH_TIMEOUT,
                // Three rather than one: a probe can miss its window behind a checkpoint flush on a
                // busy database, and that is not a sick server.
                failures_before_degraded: 3,
                successes_before_running: 1,
            })
            // **Asked rather than signalled**, and this is the whole reason `StopBehaviour::Command`
            // exists: a terminated `mariadbd` leaves a dirty buffer pool, and the next start pays for
            // it with a crash recovery that reads the entire redo log. `shutdown` flushes first.
            .stop(StopBehaviour::Command {
                program: admin,
                args: {
                    let mut args = connection(addr);
                    args.push("shutdown".to_owned());
                    args
                },
                grace: millis(settings.number(STOP_GRACE)),
            }))
        // **No reload.** MariaDB reads its configuration once, at startup — there is no signal and
        // no command that would make it read this file again, so a changed override waits for a
        // restart somebody asked for.
    }

    /// `localhost` and the address this instance binds — roadmap task **T99**.
    ///
    /// The one recipe here that asks: see the module note's last bullet for the measurement, and
    /// `certs::service::names` for why the list is what it is.
    fn certificate(&self, context: &Context) -> Option<Vec<String>> {
        Some(crate::certs::service::names(context.bind()))
    }

    /// The data directory, created once, with a root password that exists only in the OS keyring.
    fn ritual(&self) -> Option<Ritual> {
        Some(Ritual {
            secrets: SECRETS,
            steps,
            reset: Some(reset_steps),
        })
    }

    /// How a database and an account for it are made on this server — roadmap task **T77a**.
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

/// The things that have to happen before this database is ever started.
///
/// # Errors
///
/// [`ServiceError::ServiceProvidesNothing`] for an install missing one of the commands this needs,
/// and [`ServiceError::SettingValue`] for a credential this recipe will not put in a SQL literal.
fn steps(context: &Context) -> Result<Vec<Step>> {
    let password = context.secret(ROOT);

    refuse_a_password_that_needs_escaping(context, password)?;

    let mut steps = Vec::with_capacity(4);

    if cfg!(windows) {
        steps.push(windows_install_db(context)?);
        steps.push(bootstrap(context, password)?);
    } else {
        let view = super::space_free_view(context);

        steps.push(super::link_a_space_free_view(context, &view));
        steps.push(unix_install_db(context, &view)?);
        steps.push(bootstrap(context, password)?);
        steps.push(super::remove_the_space_free_view(context, &view));
    }

    Ok(steps)
}

/// Set the root password in a data directory that already exists — roadmap task **T127**.
///
/// **[`bootstrap`] alone.** The ritual is `mariadb-install-db` and then that step; a repair is that
/// step by itself, because the directory is there and full of somebody's databases. Its statements
/// are true of a server in either state — an `UPDATE` and two conditional `DELETE`s — which is what
/// lets the ritual's own step be the repair unchanged rather than a second copy to keep in step.
///
/// **Measured rather than assumed**, against 11.4.12 on Windows: a data directory holding a
/// populated InnoDB tablespace that had been *killed* rather than shut down took the `UPDATE
/// global_priv` in 0.36 s, the databases in it were untouched, and the old password stopped working.
/// That mattered because `mariadb-install-db` refuses a non-empty directory — the finding
/// [`STARTED_MARKER`](crate::generate::first_run::STARTED_MARKER) exists for — and it was reasonable
/// to expect the same of the server's bootstrap mode.
///
/// # Errors
///
/// As [`steps`]: an install missing the server, and a credential this recipe will not put in a SQL
/// literal.
fn reset_steps(context: &Context) -> Result<Vec<Step>> {
    let password = context.secret(ROOT);

    refuse_a_password_that_needs_escaping(context, password)?;

    Ok(vec![bootstrap(context, password)?])
}

/// Refuse a credential that would have to be escaped to be interpolated.
///
/// **Refused rather than escaped.** The only producer of this value is
/// `mixengine_platform::generate_secret`, whose alphabet is chosen so that the interpolation in
/// [`bootstrap`] is safe; an escaper here would be a second thing to get right for a case that
/// cannot arise, and what it would hide is a credential in the wrong half of a SQL statement.
///
/// One function and not two copies since **T127**, so that the repair cannot end up with a weaker
/// rule than the ritual it repairs.
///
/// # Errors
///
/// [`ServiceError::SettingValue`], naming no value.
fn refuse_a_password_that_needs_escaping(context: &Context, password: &str) -> Result<()> {
    if password.is_empty() || !password.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::Service(ServiceError::SettingValue {
            service: context.service().as_str().to_owned(),
            key: "root password",
            value: "<redacted>".to_owned(),
            reason: "a generated credential is alphanumeric so that it needs no escaping in the \
                     statement that sets it; this one is not, which is a bug in whatever made it",
        }));
    }

    Ok(())
}

/// `mariadb-install-db` as a system with a shell runs it.
///
/// Three things stated that nothing would think to state, each measured in T33a:
///
/// - **`--no-defaults`**, or the script and the server it starts read the machine's own
///   `/etc/mysql/my.cnf`. A user with their own MariaDB installed has one naming a datadir, a socket
///   and a port, and an instance that inherited any of them would be writing into somebody else's
///   database.
/// - **`--user`**, or it decides the data directory should belong to a `mysql` account nobody
///   created and stops when it cannot hand it over. `id -un` inside the same `sh` rather than a
///   platform call, because a shell is already what runs this step.
/// - **`/usr/sbin` and `/sbin` on the PATH.** `chown` is in `/usr/sbin` on macOS and `/usr/bin` on
///   Linux, and a cut-down path produced `chown: command not found` on one and not the other.
///
/// `--auth-root-authentication-method=normal` is Unix-only and is why this is not one command with a
/// flag: without it root authenticates through `unix_socket` against an OS account of the same name
/// and cannot be reached by whoever MixEngine runs as. The Windows program answers `unknown
/// variable` and exits 7. **`--service` is never passed**: a first-run job that registered a system
/// service would have installed something the daemon cannot see.
///
/// And a fourth, in the environment rather than on the line: `TMPDIR`, so the server this script
/// starts keeps its temporary tables out of the `/tmp` every other server on the machine cleans at
/// startup — [`super::scratch_dir`], and roadmap task **T33c**.
fn unix_install_db(context: &Context, view: &Path) -> Result<Step> {
    // Named from the map so a series that spells it `mysql_install_db` is found too — and taken
    // relative to the view rather than to the install, because the script resolves its own helpers
    // against `$basedir`.
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
            "exec \"$1\" --no-defaults --basedir=\"$2\" --datadir=\"$3\" \
             --auth-root-authentication-method=normal --skip-test-db --user=\"$(id -un)\""
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

/// And as Windows runs it: a different program of the same name, sharing almost none of its options.
///
/// The data directory is **not** created first — this program writes it itself and refuses one that
/// is already there. Its parent is what has to exist, and the daemon's marker step made it.
fn windows_install_db(context: &Context) -> Result<Step> {
    Ok(Step {
        label: "create the data directory".to_owned(),
        program: context.provided(INSTALL_DB)?,
        args: vec![format!("--datadir={}", context.data().display())],
        stdin: None,
        secret_file: None,
        env: super::scratch_environment(context),
        cwd: context.etc().to_path_buf(),
        timeout: BOOTSTRAP_PATIENCE,
    })
}

/// Set the root password and drop what should not survive, through a server listening on nothing.
///
/// **`--bootstrap` listens on no port and no socket**, so there is no moment where a password-less
/// root sits on `127.0.0.1:3306` waiting for whoever is quickest. The two rejected alternatives are
/// in the design document: a temporary server with `--skip-networking`, which is Unix vocabulary and
/// would need a second shape on Windows; and starting the real server and setting the password
/// afterwards, which is a window measured in seconds during which anyone on the machine is root.
///
/// Each statement is named rather than left to "secure defaults", and each is one line because that
/// is what bootstrap mode reads.
///
/// # Why the grant tables are written to directly
///
/// **`SET PASSWORD` does not work here, and neither does `ALTER USER`.** Measured against 11.4.12:
/// bootstrap mode implies `--skip-grant-tables`, and both statements are refused with `ERROR 1290 —
/// The MariaDB server is running with the --skip-grant-tables option so it cannot execute this
/// statement`. What does work is the row itself, which is what upstream's own `mariadb-install-db`
/// does in the same mode. `PASSWORD()` is a function rather than a statement and is available.
///
/// # And why every `root` row, not `root@localhost`
///
/// The configuration says `skip-name-resolve`, so a client connecting over TCP to 127.0.0.1 is
/// matched as `root@127.0.0.1` and never as `root@localhost` — and the readiness check, the health
/// check and the shutdown are all exactly that client. `mariadb-install-db` creates four root rows
/// (`localhost`, this machine's name, `127.0.0.1`, `::1`); the password goes on all of them, and the
/// one named after the machine is then removed, because a machine's name is reachable from off it.
fn bootstrap(context: &Context, password: &str) -> Result<Step> {
    let mut args = vec![
        "--no-defaults".to_owned(),
        "--bootstrap".to_owned(),
        format!("--basedir={}", context.install_path().display()),
        format!("--datadir={}", context.data().display()),
    ];

    // The one line the Windows server is not able to derive — see the module note and the template.
    if let Some(plugins) = context.plugins() {
        args.push(format!("--plugin-dir={}", plugins.display()));
    }

    Ok(Step {
        label: "set the root password and remove the accounts nobody should have".to_owned(),
        program: context.provided(SERVER)?,
        args,
        stdin: Some(format!(
            "USE mysql;\n\
             UPDATE global_priv SET priv = JSON_SET(priv, '$.plugin', 'mysql_native_password', '$.authentication_string', PASSWORD('{password}')) WHERE User = 'root';\n\
             DELETE FROM global_priv WHERE User = '';\n\
             DELETE FROM global_priv WHERE User = 'root' AND Host NOT IN ('localhost', '127.0.0.1', '::1');\n\
             DROP DATABASE IF EXISTS test;\n"
        )),
        secret_file: None,
        env: super::scratch_environment(context),
        cwd: context.etc().to_path_buf(),
        timeout: BOOTSTRAP_PATIENCE,
    })
}

/// How a client is told which server to ask and who to be.
///
/// One function rather than three copies, because the readiness check, the health check and the
/// shutdown must reach the *same* server: a probe that reached a different one would report a
/// service that is not this one.
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
/// superuser, and it has to: proving the account works is the whole of what it is for — the T77a
/// design, D13.
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
/// `run/` and not the data directory, and short on purpose: the kernel's cap on a socket path is the
/// whole reason — see [`within_socket_limit`](super::within_socket_limit) — and `run/` is near the
/// top of the home while a data directory is two levels down inside one whose name the user chose.
///
/// # Errors
///
/// [`ServiceError::SettingValue`] when the path this home would need is longer than the kernel
/// accepts.
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
/// [`ServiceError::SettingValue`] when the row carries no port. A database that listens on nothing
/// a client can be pointed at is not a database anybody can use, and the rendered file would say
/// `port = none` — so this is refused here rather than discovered by a server that will not start.
fn address(context: &Context) -> Result<SocketAddr> {
    let port = context.port().ok_or_else(|| {
        Error::Service(ServiceError::SettingValue {
            service: context.service().as_str().to_owned(),
            key: "port",
            value: "none".to_owned(),
            reason: "a database listens on a TCP port and this service's row carries none; \
                 `service.create` allocates one",
        })
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
