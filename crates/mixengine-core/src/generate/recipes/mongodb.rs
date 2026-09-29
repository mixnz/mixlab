//! MongoDB — roadmap task **T154**.
//!
//! The contract is `mixengine-packages`' P18, taken as published: `mongod` and `mongos` under
//! `bin/`, no shell, `requires.cpu = "avx"` on every cell. What this recipe decides is what that
//! contract left to this repository — a rendered `mongod.conf`, a port, a readiness check — and the
//! design is `docs/specs/2026-09-17-t153-mongodb-is-a-service-design.md`.
//!
//! # Nothing to create before it runs
//!
//! `mongod` initialises an empty data directory itself on its first start, and
//! [`Generator`](crate::generate::Generator) makes that directory for every service. So there is no
//! [`Recipe::ritual`], and no credential either — see the next section.
//!
//! # No accounts, and two locks in their place
//!
//! Access control is off, which is MongoDB's own default and Redis's arrangement here. Turning it on
//! means authenticating every readiness check, health probe and handoff with SCRAM, through a client
//! the artifact does not ship. What a password would protect is protected twice instead: this recipe
//! refuses a bind address that is not loopback, and `mixengine-elevate` never opens
//! 27017 to a network.
//!
//! # What the Windows build refuses, measured
//!
//! `net.unixDomainSocket.enabled` in the file is an *unrecognised option* to the Windows `mongod`,
//! which exits before it starts. Without it a Unix `mongod` creates `/tmp/mongodb-<port>.sock`, an
//! address nothing in MixEngine diagnoses, holds while idle or tells a client about. So the socket is
//! turned off on the command line, `--nounixsocket`, and only where that flag exists.
//!
//! # Stopped by a signal, which on Windows is a kill
//!
//! `SIGTERM` is a clean shutdown on Unix. On Windows ADR 0008 makes the same spec a kill, and
//! WiredTiger recovers from it at the next start — measured, `Detected unclean shutdown` followed by
//! `Waiting for connections` half a second later. What a kill can lose is a write acknowledged
//! without `j: true` inside the last journal commit interval, 100 ms by default. A clean Windows stop
//! needs something that can send `shutdown` over the wire, which is a follow-up.

use std::net::{IpAddr, SocketAddr};

use mixengine_proto::{
    HealthCheck, HealthProbe, Millis, ReadyCheck, ServiceSpec, ServiceSpecBuilder, StopBehaviour,
};

use crate::generate::document::Validator;
use crate::generate::recipe::{Context, Instancing, Recipe, TemplateFile, Upstream};
use crate::generate::settings::{Preset, Setting};
use crate::install::SmokeTest;
use crate::{Error, Result};

/// The `packages.name` this recipe is for.
const PACKAGE: &str = "mongodb";

/// The server, as the package publishes it — `bin/mongod`, `bin/mongod.exe`.
const SERVER: &str = "mongod";

/// The rendered configuration, under `etc/<service-id>/`.
const CONFIG_FILE: &str = "mongod.conf";

/// How much WiredTiger may cache, in megabytes.
const WIREDTIGER_CACHE_MB: &str = "wiredtiger_cache_mb";

/// How long the server is given to announce itself before the start is a failure, in milliseconds.
const READY_TIMEOUT: &str = "ready_timeout_ms";

/// How long the shutdown is given before the process group is killed, in milliseconds.
const STOP_GRACE: &str = "stop_grace_ms";

/// What `mongod` prints once its listener is up, after WiredTiger has opened and recovered.
///
/// Log id 23016 on every line this recipe runs. **The line, rather than an accept**, because a
/// `mongod` that lost its port to another server exits on Unix — and an accept in the moment before
/// it does is an answer from the other server.
const READY_LINE: &str = "Waiting for connections";

/// How often the running server is asked whether it is still there.
const HEALTH_INTERVAL: Millis = Millis(10_000);

/// How long one of those may take. Well inside the interval.
const HEALTH_TIMEOUT: Millis = Millis(2_000);

/// MongoDB, as MixEngine runs it.
#[derive(Debug)]
pub struct Mongodb;

impl Recipe for Mongodb {
    fn package(&self) -> &'static str {
        PACKAGE
    }

    /// `mongodb@main` beside `mongodb@legacy`: two ports, two data directories.
    fn instancing(&self) -> Instancing {
        Instancing::Named
    }

    /// MongoDB's wire protocol, which MixLab opens although this server names no administrator — a
    /// handoff to it carries no credential, as a Redis one does not.
    fn protocol(&self) -> Option<mixengine_proto::DatabaseProtocol> {
        Some(mixengine_proto::DatabaseProtocol::Mongodb)
    }

    /// 27017, the port every driver and every tutorial names.
    fn preferred_port(&self) -> Option<u16> {
        Some(27017)
    }

    fn smoke_test(&self) -> Option<SmokeTest> {
        Some(SmokeTest {
            executable: SERVER.to_owned(),
            args: vec!["--version".to_owned()],
            unset: &[],
        })
    }

    fn settings(&self) -> &'static [Setting] {
        &[
            Setting {
                key: WIREDTIGER_CACHE_MB,
                default: Preset::Number(256),
            },
            Setting {
                // A first start on a slow disk creates a dozen WiredTiger files; a start after a
                // kill replays the journal first.
                key: READY_TIMEOUT,
                default: Preset::Number(60_000),
            },
            Setting {
                key: STOP_GRACE,
                default: Preset::Number(10_000),
            },
        ]
    }

    fn files(&self) -> &'static [TemplateFile] {
        &[TemplateFile {
            path: CONFIG_FILE,
            source: include_str!("mongodb/mongod.conf"),
        }]
    }

    /// There is none: `mongod` has no mode that reads a configuration without becoming a server, so
    /// a file it refuses is a start that fails with its own complaint on the log stream.
    fn validator(&self, _context: &Context) -> Option<Validator> {
        None
    }

    /// Connected clients, as for the other databases. A driver keeps its monitoring connection open,
    /// so a MixLab tab or an application pool holding one keeps the server warm.
    fn idle_probe(&self, context: &Context) -> Option<mixengine_proto::IdleProbe> {
        context
            .port()
            .map(|port| mixengine_proto::IdleProbe::Connections { port })
    }

    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder> {
        let settings = context.settings();
        let server = context.provided(SERVER)?;
        let address = address(context)?;

        let mut args = vec![
            "--config".to_owned(),
            context.config(CONFIG_FILE).display().to_string(),
        ];

        // **Off Windows only** — see the module note: the Windows build refuses the option in either
        // spelling, and a Unix build left alone makes a socket in `/tmp` nothing here knows about.
        if !cfg!(windows) {
            args.push("--nounixsocket".to_owned());
        }

        Ok(ServiceSpec::builder(context.service().clone(), server)
            .args(args)
            .cwd(context.data())
            // What a failed start is diagnosed against (T38).
            .ports([address.port()])
            .ready(ReadyCheck::LogPattern {
                regex: READY_LINE.to_owned(),
                timeout: millis(settings.number(READY_TIMEOUT)),
            })
            .health(HealthCheck {
                // An accept, and that is all there is to ask: the artifact ships no client.
                probe: HealthProbe::Tcp { addr: address },
                interval: HEALTH_INTERVAL,
                timeout: HEALTH_TIMEOUT,
                // Three rather than one: a checkpoint flush is a busy server, not a sick one.
                failures_before_degraded: 3,
                successes_before_running: 1,
            })
            .stop(StopBehaviour::Signal {
                grace: millis(settings.number(STOP_GRACE)),
            }))
    }

    /// Its port, and nothing else — it listens on no socket.
    fn held_while_stopped(&self, context: &Context) -> Result<Vec<Upstream>> {
        Ok(vec![Upstream::Tcp(address(context)?)])
    }

    /// An hour, T70a's number for every database.
    ///
    /// **Only while the home saves resources** — roadmap task **T167b**, ADR 0041. Until then this
    /// number was the default for every home; now a service nobody set is never idle-stopped.
    fn idle_when_saving(&self) -> Option<Millis> {
        Some(Millis::from_secs(60 * 60))
    }
}

/// Where this instance listens, or the refusal that names what is wrong with the row.
///
/// # Errors
///
/// [`Error::SettingValue`] for a bind address that is not a loopback address — including one that
/// does not parse, which is refused rather than guessed at — and for a row with no port.
fn address(context: &Context) -> Result<SocketAddr> {
    let service = context.service().as_str().to_owned();

    let bind = context
        .bind()
        .parse::<IpAddr>()
        .ok()
        .filter(IpAddr::is_loopback)
        .ok_or_else(|| Error::SettingValue {
            service: service.clone(),
            key: "bind_addr",
            value: context.bind().to_owned(),
            reason: "MongoDB runs here with no accounts, so on an address a network can reach it \
                     is a database anyone on that network can read and delete; bind it to \
                     127.0.0.1",
        })?;

    let port = context.port().ok_or_else(|| Error::SettingValue {
        service,
        key: "port",
        value: "none".to_owned(),
        reason: "a database listens on a TCP port and this service's row carries none; \
                 `service.create` allocates one",
    })?;

    Ok(SocketAddr::new(bind, port))
}

/// A setting as a length of time, with a negative one read as none at all.
fn millis(number: i64) -> Millis {
    Millis(u64::try_from(number).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::Ipv4Addr;
    use std::path::Path;

    use mixengine_proto::ServiceId;

    use super::*;
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

    /// What the package publishes.
    fn provides() -> BTreeMap<String, String> {
        [
            (SERVER.to_owned(), format!("bin/{SERVER}")),
            ("mongos".to_owned(), "bin/mongos".to_owned()),
        ]
        .into_iter()
        .collect()
    }

    /// A `mongodb@main` on 27017 in a home at `root`, with `overrides` applied.
    fn context_at(root: &str, provides: BTreeMap<String, String>, overrides: &str) -> Context {
        let service = ServiceId::parse("mongodb@main").expect("an id");
        let settings =
            Settings::merge(Mongodb.settings(), overrides, &service).expect("usable overrides");

        Context::for_test(
            service,
            PACKAGE,
            Path::new(root),
            provides,
            Some(27017),
            settings,
        )
    }

    fn context(overrides: &str) -> Context {
        context_at(root(), provides(), overrides)
    }

    fn built(context: &Context) -> ServiceSpec {
        Mongodb
            .spec(context)
            .expect("a spec")
            .build()
            .expect("a valid spec")
    }

    fn rendered(context: &Context) -> String {
        let documents = recipe::render(&Mongodb, context).expect("a rendering");

        assert_eq!(documents.len(), 1, "MongoDB renders one file");
        assert_eq!(documents[0].relative(), Path::new(CONFIG_FILE));

        documents[0].contents().to_owned()
    }

    #[test]
    fn mongodb_exists_by_name() {
        assert_eq!(Mongodb.instancing(), Instancing::Named);
    }

    #[test]
    fn mongodb_proves_itself_by_running() {
        let smoke = Mongodb.smoke_test().expect("a server proves that it runs");

        assert_eq!(smoke.executable, SERVER);
        assert_eq!(smoke.args, ["--version"]);
    }

    /// **The socket is refused on the command line, and only where the flag exists** — measured:
    /// the Windows build calls either spelling an unrecognised option and exits.
    #[test]
    fn the_configuration_is_named_absolutely_and_the_socket_refused_off_windows() {
        let context = context("{}");
        let spec = built(&context);

        assert_eq!(spec.args()[0], "--config");
        assert_eq!(
            spec.args()[1],
            context.config(CONFIG_FILE).display().to_string()
        );
        assert_eq!(
            spec.args().contains(&"--nounixsocket".to_owned()),
            !cfg!(windows),
            "{:?}",
            spec.args()
        );
        assert_eq!(spec.cwd(), context.data());
    }

    /// **A path is single-quoted, and a quote inside it doubled**, which is the only escape YAML's
    /// single-quoted scalar has — so a Windows path's backslashes survive and a home under
    /// `O'Brien` does not end the scalar early.
    #[test]
    fn the_data_directory_is_single_quoted_with_a_quote_doubled() {
        let home = if cfg!(windows) {
            r"C:\Users\O'Brien\.mixengine"
        } else {
            "/home/O'Brien/.mixengine"
        };
        let context = context_at(home, provides(), "{}");
        let rendered = rendered(&context);
        let data = context.data().display().to_string().replace('\'', "''");

        assert!(
            rendered.contains(&format!("dbPath: '{data}'")),
            "{rendered}"
        );
    }

    #[test]
    fn the_rendering_says_what_the_row_and_the_defaults_say() {
        let rendered = rendered(&context("{}"));

        assert!(rendered.contains("bindIp: 127.0.0.1"), "{rendered}");
        assert!(rendered.contains("port: 27017"), "{rendered}");
        assert!(rendered.contains("cacheSizeGB: 0.25"), "{rendered}");
        assert!(
            rendered.contains("diagnosticDataCollectionEnabled: false"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("unixDomainSocket"),
            "the Windows build refuses the option: {rendered}"
        );
        assert!(
            !rendered.contains("systemLog:"),
            "a log file would be output nothing captures: {rendered}"
        );
        assert!(!rendered.contains("fork:"), "{rendered}");
    }

    #[test]
    fn an_override_is_what_the_file_says() {
        let rendered = rendered(&context(r#"{"wiredtiger_cache_mb": 1024}"#));

        assert!(rendered.contains("cacheSizeGB: 1.0"), "{rendered}");
    }

    /// **The server's own announcement**, which an accept is not: on Unix a `mongod` that lost its
    /// port exits, and a connection made in the moment before it does reaches the other server.
    #[test]
    fn readiness_is_the_servers_own_announcement() {
        let spec = built(&context("{}"));

        assert_eq!(
            spec.ready(),
            &ReadyCheck::LogPattern {
                regex: READY_LINE.to_owned(),
                timeout: Millis(60_000),
            }
        );
    }

    #[test]
    fn health_is_an_accept_and_a_stop_is_a_signal() {
        let spec = built(&context("{}"));
        let expected = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 27017);

        assert_eq!(
            spec.health().expect("a health check").probe,
            HealthProbe::Tcp { addr: expected }
        );
        assert_eq!(
            spec.stop(),
            &StopBehaviour::Signal {
                grace: Millis(10_000)
            }
        );
        assert!(spec.reload().is_none(), "mongod re-reads nothing");
        assert_eq!(spec.ports(), [27017]);
    }

    /// **The first of the two locks that stand in for a password** — design D2.
    #[test]
    fn a_bind_address_a_network_can_reach_is_refused() {
        for bind in ["0.0.0.0", "192.168.1.20", "not-an-address"] {
            let refused = Mongodb
                .spec(&context("{}").with_bind(bind))
                .expect_err("no accounts, so no address but loopback");

            assert!(
                matches!(
                    refused,
                    Error::SettingValue {
                        key: "bind_addr",
                        ..
                    }
                ),
                "{bind}: {refused:?}"
            );
        }

        assert!(Mongodb.spec(&context("{}").with_bind("::1")).is_ok());
    }

    #[test]
    fn a_row_with_no_port_is_refused_by_name() {
        let service = ServiceId::parse("mongodb@main").expect("an id");
        let settings =
            Settings::merge(Mongodb.settings(), "{}", &service).expect("usable overrides");
        let context = Context::for_test(
            service,
            PACKAGE,
            Path::new(root()),
            provides(),
            None,
            settings,
        );

        let refused = Mongodb.spec(&context).expect_err("a port is not optional");

        assert!(
            matches!(refused, Error::SettingValue { key: "port", .. }),
            "{refused:?}"
        );
    }

    #[test]
    fn an_install_without_mongod_names_the_program_it_wanted() {
        let context = context_at(
            root(),
            [("mongos".to_owned(), "bin/mongos".to_owned())]
                .into_iter()
                .collect(),
            "{}",
        );

        let refused = Mongodb.spec(&context).expect_err("no server, no service");

        assert!(format!("{refused}").contains(SERVER), "{refused}");
    }

    #[test]
    fn a_stopped_server_is_woken_at_its_port_alone() {
        assert_eq!(
            Mongodb
                .held_while_stopped(&context("{}"))
                .expect("the addresses it is woken at"),
            vec![Upstream::Tcp(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                27017
            ))]
        );
    }
}
