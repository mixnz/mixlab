//! php-fpm: the FastCGI pool behind every PHP site.
//!
//! **The first recipe whose binary does not come from a package.** A PHP is installed with
//! `runtime.install` into `runtime_installs`, and the process that serves its sites lives inside
//! that directory — so this recipe's service row points there, `service.create` refuses to write one
//! by hand, and `runtime.uninstall` is the thing that takes it away.
//!
//! # Two mechanisms, one vocabulary
//!
//! There is **no php-fpm on Windows** and this is upstream's shape rather than an omission of ours:
//! every PHP in `mixengine-packages`' index from 7.0 to 8.5 publishes `php` and `php-fpm` on Linux
//! and macOS, and `php` and `php-cgi` on Windows. What was not obvious, and was measured against the
//! artifact this project publishes rather than read about, is that this costs almost nothing:
//! `php-cgi.exe` given `PHP_FCGI_CHILDREN` **is** a process manager — a master, N children, a child
//! respawned within a second of being killed, recycling at `PHP_FCGI_MAX_REQUESTS`, and every child
//! going with the master when it is terminated. That is php-fpm with `pm = static`, configured
//! through the environment instead of through a file.
//!
//! So the two systems differ only in the mechanism, and a user meets one vocabulary:
//!
//! | | Unix | Windows |
//! | --- | --- | --- |
//! | program | `provides["php-fpm"]` | `provides["php-cgi"]` |
//! | workers | `pm.max_children` in the pool file | `PHP_FCGI_CHILDREN` |
//! | recycling | `pm.max_requests` | `PHP_FCGI_MAX_REQUESTS` |
//! | listen | `run/php-fpm-<instance>.sock` | `127.0.0.1:<services.port>` |
//! | reload | `SIGUSR2` | none |
//!
//! Which binary it is comes out of the artifact's own `provides` map rather than being written down
//! here, which is what keeps a `#[cfg]` out of this file: the index says where the executable is,
//! and the recipe asks for it by the name we gave it.
//!
//! # What this recipe deliberately does not do
//!
//! **No `pm = dynamic` and no `pm = ondemand`.** Windows can express neither, and an override that
//! works on two systems out of three is exactly the divide this recipe is written to avoid.
//!
//! **No `request_terminate_timeout` on Windows.** A hung script holds a worker there for as long as
//! it hangs, and with five of them that is a dead PHP. The fix needs no process manager — the master
//! respawns a killed child, so the daemon would only have to kill a worker that has run too long —
//! but doing it right needs its own measurement of how a hung script behaves on that system, and
//! that is a task of its own.
//!
//! **No `php.ini` and no `conf.d` of its own.** What a *pool* renders and what a *runtime's* ini set
//! contains are different files with different owners, and this recipe owns the first. What it does
//! do is name the second: `PHP_INI_SCAN_DIR` is set on both arms, so the pool and the `php` on
//! somebody's terminal load one set — see [`crate::runtimes::extensions`].
//!
//! **No site, and no `pool.d/` either.** Phase 4 renders the first per-site file and brings both the
//! directory and the `include` that finds it. Naming them here ahead of time was tried and reverted:
//! php-fpm treats a glob whose directory is missing as a hard error rather than as a pattern that
//! matched nothing, and the directory cannot be there for the first `--test` — `include` names the
//! *installed* path while validation runs over the *staged* one, before anything is installed. The
//! file says so where the line used to be.
//!
//! # A pool of an extension's own
//!
//! A `web-app` extension is served on `php-fpm@<extension-id>`, a second pool on the same installed
//! PHP as the shared `php-fpm@<version>`. Three things in this file follow from it: the socket is
//! named after the *instance* rather than the version, so two pools of one PHP do not collide; a
//! pool may carry an `EnvValue::Keyring` its site signs in with, and an edge to the database that
//! credential opens; and the pool file renders `clear_env = no` for that pool alone, because
//! php-fpm hands a worker nothing otherwise and the only alternative would write a password into a
//! generated file. `with_credential` in this file is where the second and third meet.
//!
//! **A `pm.status_path`, and still no slowlog.** The status page is what tells the daemon whether
//! anybody is using a pool it cannot count connections to — see `STATUS_PATH` and
//! [`Recipe::idle_probe`]. It is rendered on both systems and read on one: `php-cgi.exe` ignores
//! the directive because it never reads this file at all, and Windows counts a real port instead.
//! The slowlog stays absent, because nothing reads it.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use mixengine_proto::{
    HealthCheck, HealthProbe, Millis, ReadyCheck, ReloadBehaviour, ReloadSignal, RuntimeKind,
    ServiceSpec, ServiceSpecBuilder, StopBehaviour,
};

use crate::generate::context::Context;
use crate::generate::document::{CONFIG, Validator};
use crate::generate::recipe::{Instancing, Recipe, Source, TemplateFile, Upstream};
use crate::generate::settings::{Preset, Setting};
use crate::{Error, Result, ServiceError};

/// The `packages.name` this recipe is found under, which for a pool is the id's own half: a service
/// is `php-fpm@8.3.33` and the row beneath it names a `php`.
pub const PACKAGE: &str = "php-fpm";

/// The executable that serves a pool on a system that has php-fpm, as the index names it.
const FPM: &str = "php-fpm";

/// And on Windows, where it does not. See the module note.
const CGI: &str = "php-cgi";

/// The rendered pool configuration, under `etc/<service-id>/`.
const POOL_FILE: &str = "php-fpm.conf";

/// What php-fpm's own status page is asked for, and what the pool file offers it at.
///
/// **It must not end in `.php`, and that is the whole of what keeps it private.** The status page
/// shares the socket a site's traffic arrives on, and what separates them is that both front ends
/// hand FastCGI only what matches `.php` — Caddy's `php_fastcgi` after its `try_files` rewrite,
/// nginx's `location ~ \.php$`. So no URL from outside can produce a `SCRIPT_NAME` equal to this.
/// `crates/mixengine-cli/tests/caddy.rs` proves that against a real front end rather than trusting
/// the argument.
///
/// **Written here and in the template both**, which the test below holds to one string: a template
/// cannot call a function, and the probe has to ask for exactly what the pool was told to answer.
const STATUS_PATH: &str = "/mixengine-status";

/// How many workers the pool holds. Five is php-fpm's own `pm.max_children` for a `www` pool, and
/// is a number a laptop can serve a development site with while running everything else.
const MAX_CHILDREN: &str = "max_children";

/// How many requests a worker serves before it is retired and replaced. Bounds what a leaking
/// extension costs; zero turns it off.
const MAX_REQUESTS: &str = "max_requests";

/// How long one request may run before its worker is killed, in seconds. **Unix only** — see the
/// module note. `0` is php-fpm's own "no limit".
const REQUEST_TIMEOUT: &str = "request_timeout";

/// How long the pool is given to be listening before the start is a failure, in milliseconds.
const READY_TIMEOUT: &str = "ready_timeout_ms";

/// How long a stop is given before the process group is killed, in milliseconds.
const STOP_GRACE: &str = "stop_grace_ms";

/// How often the socket is asked whether the master is still accepting.
const HEALTH_INTERVAL: Millis = Millis(10_000);

/// How long one of those may take. Well inside the interval, which [`ServiceSpec::validate`]
/// insists on: two probes that could overlap are two probes that can queue.
const HEALTH_TIMEOUT: Millis = Millis(2_000);

/// How long a `SIGUSR2` is treated as in progress.
///
/// What it covers is a graceful pool restart: every worker finishes the request it is serving before
/// its replacement takes over, so the wait is really the longest request a site has in flight.
/// Nothing is killed when it expires.
const RELOAD_PATIENCE: Millis = Millis(10_000);

/// php-fpm, as MixEngine runs it.
#[derive(Debug)]
pub struct PhpFpm;

impl Recipe for PhpFpm {
    fn package(&self) -> &'static str {
        PACKAGE
    }

    /// One pool per installed PHP, named by the version it runs.
    ///
    /// The **full** version — `php-fpm@8.3.33` — because `runtime_installs` is
    /// `UNIQUE (kind, version)` over the full version, so 8.3.33 and 8.3.34 can both be installed
    /// and `php-fpm@8.3` would then name neither.
    fn instancing(&self) -> Instancing {
        Instancing::Named
    }

    /// 9000 on the systems where a pool listens on TCP, and asked for on the ones where it does
    /// not: a recipe declares the wish, and a declaration whose pool is a socket carries
    /// [`Port::None`](crate::services::Port::None) instead of consulting this.
    fn preferred_port(&self) -> Option<u16> {
        Some(9000)
    }

    fn source(&self) -> Source {
        Source::Runtime(RuntimeKind::Php)
    }

    /// One set of overrides on every system, rendered into a file or an environment as the platform
    /// requires. See the module note for what is deliberately absent from it.
    fn settings(&self) -> &'static [Setting] {
        &[
            Setting {
                key: MAX_CHILDREN,
                default: Preset::Number(5),
            },
            Setting {
                key: MAX_REQUESTS,
                default: Preset::Number(500),
            },
            Setting {
                key: REQUEST_TIMEOUT,
                default: Preset::Number(120),
            },
            Setting {
                // Thirty seconds. A pool is up in tens of milliseconds; what this is really waiting
                // for is a first run on Windows, where the runtime is read off the disk before the
                // process starts — Defender where it is on, and a slow or busy disk where it is
                // not. Fifteen was the first number and it was measured too short twice on CI's own
                // Windows runner, with Defender off (runs 35456409469 and 35470533602): the pool
                // was still alive and still not listening when the check gave up.
                //
                // **One number on every system, like every other setting here.** A default that
                // differed by operating system would make a home structurally different from a
                // colleague's, which is the thing this recipe's module note refuses. What the
                // longer wait costs is on the failing path only: a pool that will never start is
                // reported after thirty seconds instead of fifteen, and it is reported with its own
                // last lines beside it (the runner logs them since the same task).
                key: READY_TIMEOUT,
                default: Preset::Number(30_000),
            },
            Setting {
                key: STOP_GRACE,
                default: Preset::Number(10_000),
            },
        ]
    }

    /// One file, rendered on every system — and read by php-fpm on the two that have one.
    ///
    /// **Windows renders it and runs none of it**, which is deliberate and is the cheaper of the two
    /// mistakes available. A `#[cfg]` here would break this crate's rule about platform conditionals
    /// for a file that costs a few hundred bytes; it would also make a home on one system
    /// structurally different from a home on another, so that a user comparing theirs with a
    /// colleague's finds a directory missing rather than a value differing.
    fn files(&self) -> &'static [TemplateFile] {
        &[TemplateFile {
            path: POOL_FILE,
            source: include_str!("php_fpm/php-fpm.conf"),
        }]
    }

    /// `php-fpm --test`, pointed at the staged file — and nothing on Windows, where there is no file
    /// to test and the SAPI has no such flag.
    ///
    /// [`None`] falls out of the lookup rather than being decided: a Windows PHP publishes no
    /// `php-fpm`, so [`Context::provided`] fails and there is nothing to run. That is the same
    /// answer a `#[cfg]` would give, arrived at from the index instead of from this file.
    fn validator(&self, context: &Context) -> Option<Validator> {
        let program = context.provided(FPM).ok()?;

        Some(Validator::new(program, POOL_FILE).args(["--test", "--fpm-config", CONFIG]))
    }

    /// The pool, in whichever of the two shapes this system runs it.
    ///
    /// Both arms come off `listen`, which is the same expression
    /// [`upstream`](Recipe::upstream) answers with — so the socket in this pool's own
    /// `php-fpm.conf`, the one its readiness check asks and the one every site's `fastcgi_pass`
    /// names are one value computed once.
    /// The pool is busy when something is connected to it — and on a socket, when php-fpm says so.
    ///
    /// **Two mechanisms for one question, because the two systems run two different programs.**
    /// Where a pool is php-fpm on a socket there is no port to count, and no cross-platform way to
    /// count a socket's connections, so the pool is asked about itself over FastCGI. Where it is
    /// `php-cgi.exe -b` on a port, the port is counted as it always has been: that program is not
    /// php-fpm and publishes no status page to ask.
    ///
    /// That is `LimitSupport`'s shape rather than a split — ask what this system can answer, instead
    /// of insisting all three answer alike.
    ///
    /// **Off `listen` and not off the row's port**, which is the same rule the rest of this recipe
    /// keeps: the pool file, the readiness check, the upstream a site names and now the probe are
    /// one expression computed once. A probe derived from a column could count a port the pool is
    /// not listening on.
    ///
    /// **This once answered [`None`] on a socket**, and that one line is what the cold path cost:
    /// no probe means `generate` attaches no `IdlePolicy`, so the pool was never idle-stopped, so
    /// no request to it was ever cold. The activator that would have woken it already existed.
    fn idle_probe(&self, context: &Context) -> Option<mixengine_proto::IdleProbe> {
        match listen(context).ok()? {
            Upstream::Tcp(address) => Some(mixengine_proto::IdleProbe::Connections {
                port: address.port(),
            }),

            Upstream::Socket(socket) => Some(mixengine_proto::IdleProbe::FastCgiStatus {
                socket,
                path: STATUS_PATH.to_owned(),
            }),
        }
    }

    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder> {
        match listen(context)? {
            Upstream::Socket(socket) => Self::unix(context, &socket),
            Upstream::Tcp(address) => Self::windows(context, address),
        }
    }

    /// Where this pool listens, for the site configuration that has to point at it.
    fn upstream(&self, context: &Context) -> Result<Option<Upstream>> {
        listen(context).map(Some)
    }

    /// Where the activator listens for this pool.
    ///
    /// **The two shapes are not symmetrical, and that asymmetry is deliberate.** A socket pool's
    /// activator is derived from the pool's own path and costs nothing; a TCP pool's cannot be
    /// derived at all, because `port + 1` hands the first pool's activator the second pool's own
    /// port — so it is allocated onto the row, and a row that has none has no activator rather than
    /// an invented one.
    /// **Half an hour, and this is the first recipe in the build to name a number at all.**
    ///
    /// Idle detection shipped switched off because stopping a pool is only safe once something
    /// starts it again on the next request. That something is now here: the site file names the
    /// pool's activator after the pool, and the request that finds the pool down is what wakes it.
    ///
    /// **A pool first.** The databases and the caches answered `None` until a connection could
    /// start *them* again; a number there would have idle-stopped a database that nothing could
    /// bring back. The two front ends keep answering it for ever.
    ///
    /// The row still outranks this in both directions — `0` is a person saying never, and a number
    /// is a person saying how long — so a home whose owner switched idle-stopping off does not have
    /// it switched back on by this default arriving.
    ///
    /// **Only while the home saves resources** (ADR 0041). Before that this number was the default
    /// for every home; now a service nobody set is never idle-stopped.
    fn idle_when_saving(&self) -> Option<Millis> {
        Some(Millis::from_secs(30 * 60))
    }

    /// **True, and this is the service the watchdog was built for.**
    ///
    /// A pool that has grown past its ceiling is a pool leaking through its workers, and the fix a
    /// person would apply by hand is this exact restart. What it costs is the requests in flight —
    /// which is what `pm.max_requests` already spends, on a schedule nobody watches, to bound the
    /// same leak.
    ///
    /// The databases, the caches and the two front ends all keep the trait's `false`. Nothing about
    /// this reaches a service whose owner set no `memory_mb`, or a machine whose kernel holds the
    /// ceiling itself.
    fn restart_over_memory_default(&self) -> bool {
        true
    }

    fn activation_port_needed(&self) -> bool {
        listens_on_tcp()
    }

    fn activator(&self, context: &Context) -> Result<Option<Upstream>> {
        if listens_on_tcp() {
            Ok(context
                .activation_port()
                .map(|port| Upstream::Tcp(SocketAddr::from(([127, 0, 0, 1], port)))))
        } else {
            let pool = socket_path(context)?;

            super::activator_socket(context.service().as_str(), "listen", &pool)
                .map(|socket| Some(Upstream::Socket(socket)))
        }
    }
}

impl PhpFpm {
    /// The pool as a system with php-fpm runs it.
    fn unix(context: &Context, socket: &Path) -> Result<ServiceSpecBuilder> {
        let settings = context.settings();
        let program = context.provided(FPM)?;

        Ok(with_credential(
            ServiceSpec::builder(context.service().clone(), &program)
                // `--nodaemonize`, so the process the supervisor holds is the master itself. Without it
                // php-fpm forks and the parent exits successfully, which looks from out here exactly
                // like a service that started and immediately stopped.
                .args([
                    "--nodaemonize".to_owned(),
                    "--fpm-config".to_owned(),
                    context.config(POOL_FILE).to_string_lossy().into_owned(),
                ])
                .cwd(context.etc())
                // The runtime's own ini set, owned by `runtimes::extensions` and not by this
                // recipe. Set identically on both systems, which is why it is written twice rather
                // than in one arm: `php-cgi.exe` reads it exactly as php-fpm does, and a pool that
                // did not would disagree with `php -m`.
                .env(
                    crate::runtimes::extensions::SCAN_DIR_ENV,
                    crate::runtimes::extensions::conf_d(
                        context.etc_root(),
                        RuntimeKind::Php,
                        context.version(),
                    )
                    .to_string_lossy()
                    .into_owned(),
                )
                .ready(ReadyCheck::UnixSocket {
                    path: socket.to_path_buf(),
                    timeout: millis(settings.number(READY_TIMEOUT)),
                })
                .health(HealthCheck {
                    probe: HealthProbe::UnixSocket {
                        path: socket.to_path_buf(),
                    },
                    interval: HEALTH_INTERVAL,
                    timeout: HEALTH_TIMEOUT,
                    // Three intervals rather than one: a reload cycles every worker, and a pool serving
                    // a slow request can miss a probe doing it. That is a busy PHP, not a sick one.
                    failures_before_degraded: 3,
                    successes_before_running: 1,
                })
                // The master finishes what its workers are serving and replaces them with workers that
                // read the new file. This is the service the whole idea is for after Caddy: restarting
                // would drop every request in flight for a change to one site's settings.
                .reload(ReloadBehaviour::Signal {
                    signal: ReloadSignal::Usr2,
                    patience: RELOAD_PATIENCE,
                })
                // `SIGTERM` to the group, which php-fpm reads as an immediate shutdown; the workers are
                // in that group and go with it.
                .stop(StopBehaviour::Signal {
                    grace: millis(settings.number(STOP_GRACE)),
                }),
            context,
        ))
    }

    /// The pool as Windows runs it: `php-cgi.exe` on a port, with the pool in the environment.
    fn windows(context: &Context, addr: SocketAddr) -> Result<ServiceSpecBuilder> {
        let settings = context.settings();
        let program = context.provided(CGI)?;

        Ok(with_credential(
            ServiceSpec::builder(context.service().clone(), &program)
                .args(["-b".to_owned(), addr.to_string()])
                // What a failed start is diagnosed against. Only this arm declares one: the Unix
                // pool listens on a socket, which nothing else on the machine can be holding.
                .ports([addr.port()])
                .cwd(context.etc())
                // The runtime's own ini set, owned by `runtimes::extensions` and not by this
                // recipe. Set identically on both systems, which is why it is written twice rather
                // than in one arm: `php-cgi.exe` reads it exactly as php-fpm does, and a pool that
                // did not would disagree with `php -m`.
                .env(
                    crate::runtimes::extensions::SCAN_DIR_ENV,
                    crate::runtimes::extensions::conf_d(
                        context.etc_root(),
                        RuntimeKind::Php,
                        context.version(),
                    )
                    .to_string_lossy()
                    .into_owned(),
                )
                // The two variables that make `php-cgi.exe` a process manager rather than a queue of
                // one. Measured, not assumed — see the module note. They are the same two numbers the
                // pool file carries on Unix, which is what makes the override set one set.
                .env(
                    "PHP_FCGI_CHILDREN",
                    settings.number(MAX_CHILDREN).to_string(),
                )
                .env(
                    "PHP_FCGI_MAX_REQUESTS",
                    settings.number(MAX_REQUESTS).to_string(),
                )
                .ready(ReadyCheck::Tcp {
                    addr,
                    timeout: millis(settings.number(READY_TIMEOUT)),
                })
                .health(HealthCheck {
                    probe: HealthProbe::Tcp { addr },
                    interval: HEALTH_INTERVAL,
                    timeout: HEALTH_TIMEOUT,
                    failures_before_degraded: 3,
                    successes_before_running: 1,
                })
                // **No reload.** There is no signal to send here, so a changed override leaves the
                // running pool on its old configuration until somebody restarts it — and the daemon does
                // not restart a thing nobody asked it to restart. The supervisor says so once, in
                // `daemon.log`, and `mix doctor` owes the sentence.
                //
                // `StopBehaviour::Signal` degrades to a kill here (ADR 0008), which is safe for this
                // service and for a measured reason: terminating the master was observed to take every
                // child with it, so nothing is left holding the port.
                .stop(StopBehaviour::Signal {
                    grace: millis(settings.number(STOP_GRACE)),
                }),
            context,
        ))
    }
}

/// The credential this pool was given, put on the builder.
///
/// **Both arms and one function**, which is the rule this recipe already keeps for
/// `PHP_INI_SCAN_DIR`: a pool that carried a credential on one system and not on the other would be
/// a phpMyAdmin that signs itself in on a laptop and not on a colleague's.
///
/// **The value is never here.** What goes on the spec is an
/// [`EnvValue::Keyring`](mixengine_proto::EnvValue), which the supervisor resolves at the moment it
/// builds the child's `Command` — so the password exists nowhere that is persisted, serialised or
/// logged, which is what [ADR 0006] exists for.
///
/// **The edge is the daemon's rather than the manifest's.** An `extension.toml` may not carry
/// `depends_on`, because it is an edge into a graph the extension cannot see; this one is derived
/// from a link this home resolved, and it is here because a pool started before its database has
/// ever run finds no keyring entry at all — the entry is written by that database's first run. Both
/// halves come out of the same [`Credential`], so a database that goes away takes the edge with it,
/// which is what keeps `ServiceGraph::new` able to build.
///
/// [`Credential`]: crate::extensions::pools::Credential
/// [ADR 0006]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0006-servicespec-in-proto-and-secret-free.md
fn with_credential(builder: ServiceSpecBuilder, context: &Context) -> ServiceSpecBuilder {
    match context.credential() {
        Some(credential) => builder
            .env_from_keyring(
                credential.env.clone(),
                credential.keyring_service.clone(),
                credential.keyring_key.clone(),
            )
            .depends_on(credential.database.clone()),
        None => builder,
    }
}

/// Where this pool listens, in whichever of the two shapes this system runs it.
///
/// [`cfg!`] is a *value* and not an attribute, so both arms compile everywhere — which is what keeps
/// this file cross-platform and lets a test exercise the branch the machine it runs on is not.
fn listen(context: &Context) -> Result<Upstream> {
    if listens_on_tcp() {
        Ok(Upstream::Tcp(address(context)?))
    } else {
        Ok(Upstream::Socket(socket_path(context)?))
    }
}

/// Whether a pool on this system listens on a port rather than on a socket.
///
/// **One statement of the rule, read from three places** — the address the pool listens on, the
/// address its activator listens on, and whether that activator is owed a port at all. Three
/// `cfg!(windows)`s would be three chances for two of them to disagree, and what a disagreement
/// looks like is a site pointing at an address nothing binds.
const fn listens_on_tcp() -> bool {
    cfg!(windows)
}

/// Where this pool listens on a system with Unix sockets.
///
/// `run/` and not the data directory, and short on purpose: the kernel's cap on a socket path is the
/// whole reason — see [`within_socket_limit`](super::within_socket_limit) — and `run/` is near the
/// top of the home while a data directory is two levels down inside one whose name the user chose.
///
/// **Named after the instance and not after the version.** For every pool
/// [`pools::ensure`](crate::services::pools::ensure) makes the two are one string, so no existing
/// home's socket moves; what makes them differ is the pool a `web-app` extension owns, which runs
/// out of the same PHP as the shared one and must not answer on the same socket. The template
/// spells the same expression through `service.instance_or_name`, and
/// `the_file_and_the_readiness_check_name_one_socket` is what holds the two together.
///
/// # Errors
///
/// [`ServiceError::SettingValue`] when the path this home would need is longer than the kernel
/// accepts.
fn socket_path(context: &Context) -> Result<PathBuf> {
    let name = context
        .service()
        .instance()
        .unwrap_or_else(|| context.service().name());
    let socket = context.run().join(format!("php-fpm-{name}.sock"));

    super::within_socket_limit(context.service().as_str(), "listen", &socket)?;

    Ok(socket)
}

/// Where this pool listens on Windows: the port its row was given, on loopback.
///
/// The port is the row's rather than a number derived here, because it is allocated once when the
/// pool is created and has to be the same on every start — see [`crate::services::pools`].
///
/// # Errors
///
/// [`ServiceError::SettingValue`] when the row carries no port, which is a pool created on a system
/// that does not need one and then run on a system that does.
fn address(context: &Context) -> Result<SocketAddr> {
    let port = context.port().ok_or_else(|| {
        Error::Service(ServiceError::SettingValue {
            service: context.service().as_str().to_owned(),
            key: "port",
            value: "none".to_owned(),
            reason: "a pool on this system listens on a TCP port and its row carries none; \
                 `runtime.install` allocates one when it creates the pool",
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
