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

use crate::generate::first_run::{Ritual, SecretSpec, Step};
use crate::generate::recipe::{
    Claim, ClientCommand, Context, Endpoints, Instancing, Recipe, TemplateFile, Upstream,
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
/// [`Error::ServiceProvidesNothing`] for an install missing one of the commands this needs, and
/// [`Error::SettingValue`] for a credential this recipe will not put in a SQL literal.
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
/// [`Error::SettingValue`] when the row carries no port. A database that listens on nothing a client
/// can be pointed at is not a database anybody can use, and the rendered file would say `port =
/// none` — so this is refused here rather than discovered by a server that will not start.
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
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use mixengine_proto::ServiceId;

    use super::*;
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

    /// **A database is woken at the addresses it listens on itself** — T70a's D4.
    ///
    /// There is no front end in front of MariaDB to name a fallback in, so the activator binds the
    /// service's own address while it is stopped. On a system with Unix sockets that is *two*
    /// addresses and not one, and the difference is a client's habit rather than a configuration:
    /// a generated `.env` names `127.0.0.1`, and `mariadb` typed with no host at all names the
    /// socket. Waking on only one of them leaves the other client hanging against an address
    /// nothing holds.
    /// **Health is asked in the server's own protocol** — roadmap task T190b, D4. Readiness, which
    /// runs once per start, still asks the client program.
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
    /// **Both halves are one measurement, made in T33a against a real server**: MariaDB's option
    /// parser treats `\` as an escape and everything after an unquoted `#` as a comment, so
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

    /// **A development machine's defaults, which are not the server's** — roadmap task **T73**.
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

    /// **With a certificate on the context, the file names both halves and the fingerprint** —
    /// roadmap task **T99**, the design's D5. Backslashes in, forward slashes out, for T33a's
    /// reason: MariaDB's option parser treats `\` as an escape.
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

    /// **Without one, the file says nothing about TLS** — the design's D4: the server then does
    /// what it did before T99, and a start is never refused for want of a certificate.
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

    /// What a failed start is diagnosed against — roadmap task **T38**.
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
                            crate::generate::recipe::TEST_HOME
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

    /// **A reset builds steps for a directory that already exists** — roadmap task **T127**.
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
    /// own** — roadmap task **T33c**, measured in [`crate::generate::recipes::scratch_dir`]'s note.
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
        // **The home is in front of the service** — roadmap task **T126**. Without it every
        // MIXENGINE_HOME on one machine writes to this one entry, and the last to bootstrap takes
        // the root password of every server that was already running.
        assert_eq!(
            context.secret_address(ROOT),
            format!("{}/mariadb@main/root", crate::generate::recipe::TEST_HOME)
        );

        // **And it is the shared composition, not a second one that agrees today** — roadmap task
        // **T84**. This address is published to MixLab, which reads the entry MixEngine wrote.
        assert_eq!(
            context.secret_address(ROOT),
            crate::services::handoff::secret_key(
                &crate::home::HomeId::parse(crate::generate::recipe::TEST_HOME).expect("an id"),
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
            matches!(&error, Error::SettingValue { value, .. } if !value.contains("a'b")),
            "the refusal quoted the credential: {error:?}"
        );
    }

    /// And so is no credential at all, which is a recipe that declared one and never got it.
    #[test]
    fn no_credential_at_all_is_refused_rather_than_setting_an_empty_password() {
        let error = steps(&context("{}")).expect_err("an empty password is not a password");

        assert!(matches!(error, Error::SettingValue { .. }), "{error:?}");
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
            matches!(error, Error::ServiceProvidesNothing { .. }),
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
                    !argument.contains(&credentials.root)
                        && !argument.contains(&credentials.account),
                    "{} put a password in an argument: {argument}",
                    step.label
                );
            }
        }
    }

    /// **The validator's alphabet is exactly what every accepted character survives being embedded
    /// as** — roadmap task **T77b**. A chosen password containing every printable-ASCII character
    /// the validator accepts must still produce SQL whose `IDENTIFIED BY '...'` literal closes
    /// where it should: an unescaped literal that swallowed a later character would shift where
    /// the string appears to end, and the assertion below would then fail to find the exact
    /// literal.
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

    /// **No `CHARACTER SET`, on purpose** — the T77a design, D7. The instance is already configured
    /// with one, and a second place deciding it is one that silently wins.
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
    /// `localhost` — T33's finding about the root account, just as true of this one.
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
}
