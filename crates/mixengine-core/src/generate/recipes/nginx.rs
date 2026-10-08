//! Nginx: the alternative front end — roadmap task **T37**.
//!
//! The second of the two programs `docs/features/services.md` will let a site be reached through,
//! and the one that makes "exactly one active front end" a rule somebody can break — which is why
//! [`Role`] arrived with it. Everything else here is [`caddy`](super::caddy)'s shape answered by a
//! server that has none of Caddy's mechanisms:
//!
//! - **There is no admin endpoint, so the recipe renders one.** A loopback `server` block that
//!   answers `200` on `/mixengine/health` and `404` on everything else is both the readiness check
//!   and the health probe. The obvious alternative — a TCP connect on the port — is not a weaker
//!   version of that, it is a different question: the master process holds the listening socket, so
//!   a connection is accepted in exactly the same way when every worker has died. A request that
//!   comes back is one a worker reading this configuration served.
//! - **`nginx -t` judges the rendering before it is installed**, over the staging directory T30
//!   builds, exactly as `caddy validate` does. What makes the two of them work differently is where
//!   an `include` resolves: Caddy's `import` is relative to the file, nginx's is relative to the
//!   **prefix** — so `-p` is passed in both places, pointing at the staging directory while the
//!   configuration is being judged and at `etc/<service-id>/` once it is installed.
//! - **A changed rendering is reloaded rather than restarted**, through `-s reload` against the
//!   running master. That is a signal in nginx's own spelling rather than an OS one, so it works on
//!   Windows, where the supervisor's own `signal` does not — and it is why the pid file goes where
//!   this configuration says rather than where nginx would compile it: `-s` finds a master through
//!   the configuration it is given.
//!
//! # Judged against the real server
//!
//! `crates/mixengine-cli/tests/nginx.rs` runs the whole of that against a real nginx, and it runs it
//! through the same harness [`caddy`](super::caddy)'s suite does — which is the parity half of T37.
//! Two findings are in the template beside the lines they explain: forward-slashed quoted paths, and
//! five temp directories that are children of one that exists.
//!
//! # What this recipe deliberately does not do
//!
//! **It renders no site, and listens on nothing a site would be reached on.** `include sites/*.conf`
//! matches nothing until Phase 4 (T39, T43), and the row's own port is written into no `listen` at
//! all: binding 80 needs the port grant T42 has not built on macOS and Linux, and a front end
//! holding a port it serves nothing on is worse than one that has not taken it yet. Caddy says the
//! same thing by writing `http_port` into a global block and binding neither.
//!
//! **It terminates no TLS.** Phase 5 owns the certificate, and a `listen ... ssl` with no
//! certificate is a configuration nginx refuses outright rather than one it starts without.
//!
//! [`Role`]: crate::generate::recipe::Role

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};

use mixengine_proto::{
    HealthCheck, HealthProbe, Millis, ReadyCheck, ReloadBehaviour, ServiceSpec, ServiceSpecBuilder,
    StopBehaviour,
};

use crate::generate::document::{CONFIG, Document, Reason, Validator};
use crate::generate::recipe::{
    Context, Endpoints, Instancing, Recipe, Role, TemplateFile, Upstream,
};
use crate::generate::served::{Served, ServedKind};
use crate::generate::settings::{Preset, Setting};
use crate::install::SmokeTest;
use crate::{Error, Result};

/// The `packages.name` this recipe is for, which is also the name the binary is published under.
const PACKAGE: &str = "nginx";

/// The rendered configuration, under `etc/<service-id>/`.
const CONFIG_FILE: &str = "nginx.conf";

/// The data file out of the archive that a generated configuration cannot do without.
///
/// Every `Content-Type` nginx serves comes out of it, and a generated file has no `conf/` of its own
/// to reach it through — see [`Endpoints::includes`].
const MIME_TYPES: &str = "mime.types";

/// The other file out of the archive a generated configuration includes: what a `fastcgi_pass` to
/// PHP-FPM needs, and the reason MixEngine renders an nginx configuration at all — D6.
///
/// `mixengine-packages`' `tools/nginx.py` publishes it under `CONF_FILES` in as many words.
const FASTCGI_PARAMS: &str = "fastcgi_params";

/// One rendered site, under `etc/<service-id>/sites/`.
const SITE: &str = include_str!("nginx/site.conf");

/// The directory the sites go in, which is also the one this recipe sweeps. The name is in
/// `nginx.conf`'s `include sites/*.conf;` as well.
const SITES: &str = "sites";

/// The directory an installed extension's `[[recipe.front_end]]` goes in — roadmap task **T81c**.
///
/// Swept for [`SITES`]' reason: an uninstalled extension's fragment has to leave with it. The name
/// is in `nginx.conf`'s `include extensions/*.conf;` as well.
const EXTENSIONS: &str = "extensions";

/// Where this home's public certificate authority is rendered, relative to this service's
/// configuration — roadmap task **T75**.
///
/// **A directory holding exactly one file.** `certs/ca/` holds the signing key beside the
/// certificate, so it is never a directory a front end is pointed at; a copy of the public half is
/// what a phone downloads. The T75 design, D9.
const AUTHORITY: &str = "public/ca.crt";

/// The directory of [`AUTHORITY`], which is what a front end is pointed at.
///
/// Two constants rather than one and a `parent()`: what the template writes is a directory and what
/// the document writes is a file, and a path derived from the other at render time would be a third
/// place this layout is decided.
const AUTHORITY_DIR: &str = "public";

/// Where the page a site with nothing behind it answers with is rendered — roadmap task **T124**.
///
/// **A directory of its own, and never [`AUTHORITY_DIR`].** That one holds this home's authority and
/// is asserted to hold exactly one file, because the front end is pointed at the *directory* and so
/// what else is in it is what else is published. The assertion is about the authority and should
/// stay about the authority.
///
/// Outside `nginx.conf`'s own `include sites/*.conf;` as well, which is the second reason it is not
/// `sites/`: a rendered HTML page is not a configuration fragment and must never be read as one.
const WELCOME_DIR: &str = "welcome";

/// The port a front end answers on when its row names none.
///
/// nginx's own configuration carries no listen for sites, so unlike Caddy there is no server default
/// to fall through to — this is that default, written down, and it is the same 80 Caddy would use.
const DEFAULT_HTTP_PORT: u16 = 80;

/// The port a site with a certificate is served on over TLS — roadmap task **T51**.
///
/// **A setting and not a constant**, which was not the first design: nginx has no global listen, so
/// a constant looked like enough. It is not. This is the first port a front end binds that no test
/// could move, and `tests/nginx.rs` runs a real server as an unprivileged user — where 443 is
/// refused, and nginx rejects the whole configuration rather than the one listener. The same is
/// true of any machine that has not been granted the ports, so a setting is what the *product*
/// needs as well as the suite.
///
/// `Context::bound` still turns whatever this is into its bound half on macOS.
const HTTPS_PORT: &str = "https_port";

/// Where the status endpoint listens. Loopback always — see the template.
const STATUS_HOST: &str = "127.0.0.1";

/// What the status endpoint answers on, and the one path in this configuration that is MixEngine's
/// rather than a user's.
const HEALTH_PATH: &str = "/mixengine/health";

/// The port that endpoint listens on.
///
/// **One above Caddy's 2019**, which is the whole of the reasoning: it is the same thing for the
/// other front end, the two can never both be running ([`Role::FrontEnd`]), and a person who
/// remembers one number has remembered both. nginx publishes no default of its own to borrow.
const STATUS_PORT: &str = "status_port";

/// How many worker processes to start.
const WORKER_PROCESSES: &str = "worker_processes";

/// How many connections each of them may hold at once.
const WORKER_CONNECTIONS: &str = "worker_connections";

/// The largest request body nginx will accept, in nginx's own units: `64m`, `1g`.
const CLIENT_MAX_BODY_SIZE: &str = "client_max_body_size";

/// The level nginx logs at, in its own spelling: `debug`, `info`, `notice`, `warn`, `error`,
/// `crit`, `alert`, `emerg`.
const LOG_LEVEL: &str = "log_level";

/// How long the status endpoint is given to answer before the start is a failure, in milliseconds.
const READY_TIMEOUT: &str = "ready_timeout_ms";

/// How long `-s quit` is given before the process group is killed, in milliseconds.
const STOP_GRACE: &str = "stop_grace_ms";

/// How often the status endpoint is asked whether the server is still there.
const HEALTH_INTERVAL: Millis = Millis(10_000);

/// How long one of those may take. Well inside the interval, which [`ServiceSpec::validate`]
/// insists on.
const HEALTH_TIMEOUT: Millis = Millis(2_000);

/// How long a reload is waited for.
///
/// `-s reload` returns as soon as the master has accepted the new configuration, while the old
/// workers finish what they are serving behind it — so this covers a master under load rather than
/// a graceful shutdown. Nothing is killed when it expires; see [`ReloadBehaviour::Command`].
const RELOAD_PATIENCE: Millis = Millis(30_000);

/// Nginx, as MixEngine runs it.
#[derive(Debug)]
pub struct Nginx;

impl Recipe for Nginx {
    fn package(&self) -> &'static str {
        PACKAGE
    }

    /// There is one nginx, for the reason there is one Caddy: a second is two processes contending
    /// for the ports every site on the machine is reached through.
    fn instancing(&self) -> Instancing {
        Instancing::Single
    }

    /// And it is the other answer to the same question, which [`Instancing`] cannot express.
    fn role(&self) -> Role {
        Role::FrontEnd(mixengine_proto::FrontEndServer::Nginx)
    }

    fn smoke_test(&self) -> Option<SmokeTest> {
        Some(SmokeTest {
            executable: PACKAGE.to_owned(),
            // `-v` and not `-t`: the second reads a configuration, and at the moment an archive is
            // being installed there is no service and therefore nothing rendered to read.
            args: vec!["-v".to_owned()],
            unset: &[],
        })
    }

    fn settings(&self) -> &'static [Setting] {
        &[
            Setting {
                key: CLIENT_MAX_BODY_SIZE,
                default: Preset::Text("64m"),
            },
            Setting {
                key: LOG_LEVEL,
                default: Preset::Text("error"),
            },
            Setting {
                // Thirty seconds, for Caddy's reason rather than nginx's: the server itself is up in
                // milliseconds, and what this is really waiting for is a first start on Windows with
                // Defender reading the binary.
                key: READY_TIMEOUT,
                default: Preset::Number(30_000),
            },
            Setting {
                key: STATUS_PORT,
                default: Preset::Number(2020),
            },
            Setting {
                key: HTTPS_PORT,
                default: Preset::Number(443),
            },
            Setting {
                key: STOP_GRACE,
                default: Preset::Number(10_000),
            },
            Setting {
                key: WORKER_CONNECTIONS,
                default: Preset::Number(1024),
            },
            Setting {
                // One, not `auto`. See the template: on the borrowed Windows build the extra workers
                // do nothing, and one developer's machine has nothing for them to do anywhere else.
                key: WORKER_PROCESSES,
                default: Preset::Number(1),
            },
        ]
    }

    fn files(&self) -> &'static [TemplateFile] {
        &[TemplateFile {
            path: CONFIG_FILE,
            source: include_str!("nginx/nginx.conf"),
        }]
    }

    /// Exactly `sites/`, and only because this recipe is a front end — D4.
    fn swept(&self) -> &'static [&'static str] {
        &[SITES, EXTENSIONS, WELCOME_DIR]
    }

    /// One file per site, named after its primary domain — D12.
    ///
    /// Rendered into the set `nginx.conf`'s own `include sites/*.conf;` picks up, which resolves
    /// against the prefix — the staging directory while `nginx -t` is judging it, and `etc/nginx/`
    /// afterwards. That is why the validator passes `-p .`.
    ///
    /// # Errors
    ///
    /// [`Error::TemplateBroken`] naming the site template, and
    /// [`Error::ServiceProvidesNothing`] for an install that
    /// publishes no `fastcgi_params` — a package problem reported while rendering rather than an
    /// `include` of a file that is not there.
    fn sites(&self, context: &Context, served: &[Served]) -> Result<Vec<Document>> {
        let fastcgi_params = forward_slashed(&context.provided(FASTCGI_PARAMS)?);

        // The row's port is what a browser asks for; this is what the process must listen on. A row
        // with no port answers on 80, exactly as Caddy's own default does.
        let listen = listening(
            context.bind(),
            context.bound(context.port().unwrap_or(DEFAULT_HTTP_PORT)),
        );

        // The same function and the same mapping, a second time: `mixengine-platform` stays the only
        // thing that knows which system moves a port.
        let https_port = port(context, HTTPS_PORT)?;
        let listen_tls = listening(context.bind(), context.bound(https_port));

        let mut documents = served
            .iter()
            .map(|site| {
                let rendering = SiteRendering {
                    primary: site.primary(),
                    domains: &site.domains,
                    doc_root: forward_slashed(&site.doc_root),
                    kind: kind(&site.kind),
                    upstream: upstream(&site.kind),
                    activator: activator(&site.kind),
                    rewrite: site_rewrite(&site.kind).cloned(),
                    routes: site
                        .routes
                        .iter()
                        .enumerate()
                        .map(|(position, route)| {
                            RouteRendering::new(site.primary(), position, route)
                        })
                        .collect(),
                    group: group(site.primary()),
                    fastcgi_params: &fastcgi_params,
                    listen: &listen,
                    listen_tls: &listen_tls,
                    certificate: site.certificate.as_ref().map(Certificate::from),
                    https_redirect: site.https_redirect,
                    // The LAN listener binds the *bound* port, exactly as loopback's does: a
                    // machine that redirects 80 to 8080 redirects it for every address, and a
                    // listener on the number a browser types would answer nothing at all.
                    lan: site.shared.as_ref().map(|shared| {
                        listening(
                            &shared.address.to_string(),
                            context.bound(context.port().unwrap_or(DEFAULT_HTTP_PORT)),
                        )
                    }),
                    lan_tls: site.shared.as_ref().map(|shared| {
                        listening(&shared.address.to_string(), context.bound(https_port))
                    }),
                    mdns: site.shared.as_ref().and_then(|shared| shared.name.clone()),
                    // **Only for a shared site, which is where "served only while sharing is on"
                    // is actually enforced** - roadmap task T75. The rendered copy's directory,
                    // absolute, and [`None`] on a home that has no authority to serve.
                    authority: site
                        .shared
                        .as_ref()
                        .and(context.authority())
                        .map(|_| forward_slashed(&context.config(AUTHORITY_DIR))),

                    // **Every site gets one** — roadmap task T124. Unlike `authority` above this is
                    // never conditional on the site: what a page is *for* is a site nobody has put
                    // anything into yet, which is every site at the moment it is made.
                    welcome: context
                        .welcome()
                        .then(|| forward_slashed(&context.config(WELCOME_DIR))),
                };

                let contents = crate::generate::served::render(
                    SITE,
                    "nginx/site.conf",
                    context.service(),
                    &rendering,
                )?;

                Ok(Document::new(
                    format!("{SITES}/{}.conf", site.primary()),
                    contents,
                ))
            })
            .collect::<Result<Vec<Document>>>()?;

        // **Every site's welcome page, after every site's configuration** — roadmap task T124.
        // Appended in a second pass rather than returned two at a time from the map above, so that
        // `documents[n]` goes on meaning the nth site: the authority below already depends on that
        // and says so, and interleaving would have made it depend on the stride instead.
        //
        // **Nothing at all on a home that turned it off** (D6), which is the same answer the
        // rendering gives: a page with no route is a file nothing reads, and a route with no page
        // would be a 404 on top of the 404 this feature exists to replace.
        if context.welcome() {
            documents.reserve(served.len());

            for site in served {
                let page = crate::generate::welcome::page(
                    site.primary(),
                    &site.kind,
                    &site.doc_root_relative,
                    &site.steps,
                );

                documents.push(Document::new(
                    format!("{WELCOME_DIR}/{}.html", site.primary()),
                    crate::generate::welcome::render(context.service(), &page)?,
                ));

                // And, for a site whose pool MixEngine starts, the page its 502 is answered with
                // while that pool is not answering — roadmap task T167f.
                if let Some(starting) = crate::generate::welcome::render_starting(
                    context.service(),
                    site.primary(),
                    &site.kind,
                )? {
                    documents.push(Document::new(
                        format!(
                            "{WELCOME_DIR}/{}",
                            crate::generate::welcome::starting_file(site.primary())
                        ),
                        starting,
                    ));
                }
            }
        }

        // **This home's authority, appended last** — roadmap task T75. Last so that a caller
        // naming `documents[0]` still means the first site, and unconditional so that the file's
        // presence never has to track sharing state: what is conditional is the *route*, which the
        // template renders only for a shared site.
        if let Some(authority) = context.authority() {
            documents.push(Document::new(AUTHORITY, authority));
        }

        Ok(documents)
    }

    /// One file per extension, in the set `include extensions/*.conf;` picks up — roadmap task
    /// **T81c**.
    ///
    /// The fragment arrives rendered, with its paths already forward-slashed: which separator an
    /// `nginx.conf` takes is a property of this file format and is decided where the substitution
    /// happens, in `extensions::render`.
    fn fragments(&self, context: &Context) -> Result<Vec<Document>> {
        Ok(context
            .fragments()
            .iter()
            .map(|addition| {
                Document::new(
                    format!("{EXTENSIONS}/{}.conf", addition.extension),
                    format!(
                        "# Generated by MixEngine from the {} extension. Every edit here is \
                         overwritten;\n# what you change is the extension's own manifest.\n{}\n",
                        addition.extension,
                        addition.fragment.trim_end()
                    ),
                )
            })
            .collect())
    }

    /// The archive's own `mime.types`, by the absolute path the index publishes it at.
    ///
    /// Resolved here rather than joined in the template, which is what [`Endpoints`] is for: a
    /// package that publishes no `mime.types` fails while this recipe is being rendered, naming what
    /// the install does provide, instead of producing an `include` of a file that is not there.
    fn endpoints(&self, context: &Context) -> Result<Endpoints> {
        Ok(Endpoints {
            includes: BTreeMap::from([
                (MIME_TYPES.to_owned(), context.provided(MIME_TYPES)?),
                (FASTCGI_PARAMS.to_owned(), context.provided(FASTCGI_PARAMS)?),
            ]),
            ..Endpoints::default()
        })
    }

    /// `nginx -t`, pointed at the staged configuration with the staging directory as its prefix.
    ///
    /// **`-p .` and not the installed directory**, which is the whole reason this is not a copy of
    /// Caddy's: an `include` inside an nginx configuration resolves against the prefix, so a checker
    /// given the installed one would judge a staged file against the sites that are already live.
    /// The validator runs with the staging directory as its working directory, which is what `.`
    /// is — and what makes the rendering judged as a whole, includes and all.
    ///
    /// `-e stderr` so that a complaint arrives on the pipe the error is read from rather than in a
    /// `logs/error.log` under a prefix that is about to be thrown away.
    ///
    /// **[`Reason::First`], because nginx says why and then says that it failed.** A refusal is two
    /// lines — `nginx: [emerg] <what is wrong> in <file>:<line>` and then
    /// `nginx: configuration file <path> test failed` — and the second names the file the message
    /// around it already names. Reported by its last line, every nginx configuration error a person
    /// ever meets would read as *something is wrong somewhere*.
    fn validator(&self, context: &Context) -> Option<Validator> {
        Some(
            Validator::new(context.program(PACKAGE), CONFIG_FILE)
                .args(["-t", "-p", ".", "-c", CONFIG, "-e", "stderr"])
                .reason(Reason::First),
        )
    }

    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder> {
        let settings = context.settings();

        let nginx = context.program(PACKAGE);
        let prefix = context.etc().to_string_lossy().into_owned();
        let config = context.config(CONFIG_FILE).to_string_lossy().into_owned();
        let status_port = port(context, STATUS_PORT)?;
        let health = format!("http://{STATUS_HOST}:{status_port}{HEALTH_PATH}");

        // What every invocation of this binary says, whether it is the server or a signal sent to
        // one: which prefix, which configuration, and where the errors go. A signal that named a
        // different configuration would look for a pid file this instance never wrote.
        let invocation = |trailing: &[&str]| {
            let mut args = vec![
                "-p".to_owned(),
                prefix.clone(),
                "-c".to_owned(),
                config.clone(),
                "-e".to_owned(),
                "stderr".to_owned(),
            ];
            args.extend(trailing.iter().map(|arg| (*arg).to_owned()));
            args
        };

        Ok(ServiceSpec::builder(context.service().clone(), &nginx)
            .args(invocation(&[]))
            // The configuration directory, which is also the prefix: a relative path inside a site
            // — a document root somebody wrote by hand — resolves against it, and so does the
            // `include` that will bring that site in.
            .cwd(context.etc())
            // What a failed start is diagnosed against (T38), and it is the status endpoint alone:
            // nothing here listens on the port sites are served on until T43 renders one.
            .ports([status_port])
            .ready(ReadyCheck::Http {
                url: health.clone(),
                expect_status: 200,
                timeout: millis(settings.number(READY_TIMEOUT)),
            })
            .health(HealthCheck {
                probe: HealthProbe::Http {
                    url: health,
                    expect_status: 200,
                },
                interval: HEALTH_INTERVAL,
                timeout: HEALTH_TIMEOUT,
                // Three intervals rather than one, as Caddy has: a master that is reloading is
                // finishing requests on the old workers, which is a busy front end and not a sick
                // one.
                failures_before_degraded: 3,
                successes_before_running: 1,
            })
            .reload(ReloadBehaviour::Command {
                program: nginx.clone(),
                args: invocation(&["-s", "reload"]),
                patience: RELOAD_PATIENCE,
            })
            // `-s quit` and not `-s stop`: the first lets the workers finish what they are serving,
            // the second cuts every connection where it stands. A front end being stopped is a
            // developer restarting their own machine's web server, not an emergency.
            .stop(StopBehaviour::Command {
                program: nginx,
                args: invocation(&["-s", "quit"]),
                grace: millis(settings.number(STOP_GRACE)),
            }))
    }
}

/// One of this recipe's port settings, as a port.
///
/// [`caddy`](super::caddy)'s reasoning, and deliberately its own copy rather than a shared helper:
/// what makes the message useful is that it names the setting, and the two recipes have different
/// settings.
fn port(context: &Context, key: &'static str) -> Result<u16> {
    let number = context.settings().number(key);

    u16::try_from(number)
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| Error::SettingValue {
            service: context.service().as_str().to_owned(),
            key,
            value: number.to_string(),
            reason: "a port is a number from 1 to 65535",
        })
}

/// A setting as a length of time, with a negative one read as none at all.
fn millis(number: i64) -> Millis {
    Millis(u64::try_from(number).unwrap_or_default())
}

/// One site, as `nginx/site.conf` reads it.
#[derive(Debug, serde::Serialize)]
struct SiteRendering<'a> {
    primary: &'a str,
    domains: &'a [String],
    doc_root: String,
    kind: &'static str,

    /// Empty for the kinds whose branch does not read it — `Strict` undefined behaviour means the
    /// key has to be there whichever branch is taken.
    upstream: String,

    /// The activator to fall back to, or [`None`] for a pool nothing can wake — T70.
    ///
    /// Present whichever branch is taken, for `upstream`'s reason. [`None`] renders a
    /// `fastcgi_pass` straight at the pool, which is what this file rendered before T70.
    activator: Option<String>,

    /// What `/` becomes on the way out, for a proxy site whose upstream carried a path — roadmap
    /// task **T135**. [`None`] renders exactly what this file rendered before T135.
    rewrite: Option<crate::generate::served::Rewrite>,

    /// What answers before the kind does, longest prefix first — roadmap task **T135**.
    routes: Vec<RouteRendering>,

    /// What the `upstream` group holding the two is called, when there is one.
    group: String,
    fastcgi_params: &'a str,
    listen: &'a str,

    /// What the TLS listener binds. **Always present**, for `upstream`'s reason: `Strict` undefined
    /// behaviour makes a missing key an error whichever branch the template takes.
    listen_tls: &'a str,

    /// [`None`] renders no TLS at all — the T51 design, D4.
    certificate: Option<Certificate>,

    /// Whether this site renders two `server` blocks — one redirecting, one serving — rather than
    /// the one T51 shipped — roadmap task **T98**.
    ///
    /// **Read beside `certificate`, never alone.** A site can carry this as `true` with no usable
    /// certificate, and redirecting to a TLS listener nothing is bound to would be worse than the
    /// plaintext page such a site has always been able to serve.
    https_redirect: bool,

    /// What a shared site's second listener binds, or [`None`] for a site that is not shared —
    /// roadmap task **T74**.
    ///
    /// A second `listen` line rather than a changed one: loopback keeps working, which is what the
    /// browser on this machine is using while a phone looks at the same site.
    lan: Option<String>,

    /// The same for TLS. Present whichever branch is taken, for `upstream`'s reason.
    lan_tls: Option<String>,

    /// The mDNS name a shared site also answers to, or [`None`] — roadmap task **T75**.
    ///
    /// It joins `server_name` rather than adding a listener: a name is matched after nginx has
    /// picked a listener group, so this is the half that decides which site replies.
    mdns: Option<String>,

    /// The directory this home's public authority was rendered into, or [`None`] — roadmap task
    /// **T75**.
    ///
    /// **[`None`] for a site that is not shared**, which is how "served only while sharing is on"
    /// becomes a property of the rendering rather than a promise made about it. It is also [`None`]
    /// on a home with no authority to serve.
    authority: Option<String>,

    /// The directory this site's welcome page was rendered into — roadmap task **T124**.
    ///
    /// **[`None`] on a home that turned the page off** — the T124 design, D6. The key is always
    /// present, for [`upstream`](Self::upstream)'s reason: `Strict` makes a missing key an error
    /// rather than a falsy value, and `None` serialises to `null`, which `{% if %}` reads as false.
    ///
    /// Forward-slashed like every other path this template writes: nginx reads a backslash in a
    /// quoted string as an escape, so a Windows path spelled natively ends the string early.
    welcome: Option<String>,
}

/// A certificate as the template writes it — roadmap task **T51**.
///
/// **Strings and not `Path`s**, because a template writes text: `Path`'s `Serialize` is lossy on a
/// path that is not UTF-8, and this module already forward-slashes and stringifies every path it
/// renders.
#[derive(Debug, serde::Serialize)]
struct Certificate {
    certificate: String,
    key: String,
    fingerprint: String,
}

impl From<&crate::generate::served::SiteCertificate> for Certificate {
    fn from(certificate: &crate::generate::served::SiteCertificate) -> Self {
        Self {
            certificate: forward_slashed(&certificate.certificate),
            key: forward_slashed(&certificate.key),
            fingerprint: certificate.fingerprint.clone(),
        }
    }
}

/// What a site's `listen` says: the address the row asked for, and the port the process must bind.
///
/// **The address and not the port alone**, which is nginx's own dispatch rule showing through: it
/// groups servers by listen *address* first and consults `server_name` only inside a group, so a
/// site left on the wildcard `*:8080` is unreachable beside anything that took `127.0.0.1:8080`
/// — the name is never looked at. Writing the address the row carries is also what makes LAN
/// sharing (T74) a change to one column rather than to this template.
///
/// [`SocketAddr`] does the spelling, so an IPv6 `bind_addr` arrives bracketed the way nginx needs.
/// A `bind_addr` that is not an address at all renders as the bare port — nginx's "any" — rather
/// than as an invented loopback: a front end that had quietly stopped answering on the LAN is the
/// worse of the two failures.
fn listening(bind: &str, port: u16) -> String {
    bind.parse::<IpAddr>().map_or_else(
        |_| port.to_string(),
        |address| SocketAddr::new(address, port).to_string(),
    )
}

/// A path as nginx has to read it: forward slashes, on every system.
///
/// nginx accepts `/` on Windows and its own tokeniser eats `\` inside the quotes these paths are
/// written in, so one spelling works on all three. `nginx.conf` does the same thing with a Jinja
/// filter; this is the Rust half, for the values this recipe computes.
fn forward_slashed(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Which branch of the template this kind takes.
///
/// Three and not four: a `node-app` renders as a reverse proxy to loopback and that is all it is —
/// D7.
const fn kind(kind: &ServedKind) -> &'static str {
    match kind {
        ServedKind::PhpFpm { .. } => "php-fpm",
        ServedKind::Static => "static",
        ServedKind::ReverseProxy { .. } | ServedKind::NodeApp { .. } => "proxy",
    }
}

/// The address this kind is proxied or passed to, as **nginx** spells one.
///
/// `unix:` and then the path, which is not Caddy's `unix/` — the difference is the reason
/// [`Upstream`] is a value rather than a string.
fn upstream(kind: &ServedKind) -> String {
    match kind {
        ServedKind::PhpFpm { upstream, .. } => address(upstream),
        ServedKind::ReverseProxy { upstream, .. } => upstream.clone(),
        ServedKind::NodeApp { port } => format!("http://127.0.0.1:{port}"),
        ServedKind::Static => String::new(),
    }
}

/// What `/` becomes on the way out for a site whose own upstream carried a path — roadmap task
/// **T135**.
///
/// [`None`] for every other kind, and for the ordinary proxy site whose upstream is an address and
/// nothing more — which is what this file rendered before T135. nginx would have read the path as a
/// prefix replacement of its own and Caddy would have refused it, so the two front ends disagreed
/// about what that URL meant; `served::split_upstream` is where they stopped.
fn site_rewrite(kind: &ServedKind) -> Option<&crate::generate::served::Rewrite> {
    match kind {
        ServedKind::ReverseProxy { rewrite, .. } => rewrite.as_ref(),
        _ => None,
    }
}

/// One route, as `nginx/site.conf` reads it.
///
/// **Every key present whichever branch the template takes**, for [`SiteRendering::upstream`]'s
/// reason: `UndefinedBehavior::Strict` makes a missing one an error rather than a falsy value.
#[derive(Debug, serde::Serialize)]
struct RouteRendering {
    /// The prefix, as the row holds it.
    path: String,

    /// The anchored expression a proxy route's `location ~` is given.
    ///
    /// **A regex and not a prefix**, because nginx's prefix locations match as text: `location /api`
    /// takes `/apidocs` as well, which is the trap this whole shape exists to avoid.
    regex: String,

    /// Which branch of the template this target takes.
    kind: &'static str,

    /// Where a proxy route forwards — scheme, host and port, never a path: nginx refuses a URI part
    /// in a `proxy_pass` inside a regex location, and the rewrite below has already done the work.
    address: String,

    /// What the prefix becomes on the way out, or [`None`] to pass the path through.
    rewrite: Option<crate::generate::served::Rewrite>,

    /// Where a static route's files are, forward-slashed.
    root: String,

    /// Where a php-fpm route's pool listens, in nginx's spelling.
    upstream: String,

    /// Where the activator waits for that pool — roadmap task **T70**, the site's own rule.
    activator: Option<String>,

    /// What the `upstream` group holding the two is called, when there is one.
    ///
    /// Named after the site **and the route's position**: nginx refuses a configuration that
    /// declares one upstream name twice, which takes the whole front end down rather than one site,
    /// and two routes of one site naming one pool is ordinary.
    group: String,
}

impl RouteRendering {
    /// One [`ServedRoute`](crate::generate::served::ServedRoute) in nginx's spelling.
    fn new(primary: &str, position: usize, route: &crate::generate::served::ServedRoute) -> Self {
        use crate::generate::served::ServedRouteTarget as Target;

        let mut rendering = Self {
            path: route.path.clone(),
            group: format!("{}_route_{position}", group(primary)),
            activator: None,
            // The same escaping `served` does for the rewrite, for the same reason: a `.` in a path
            // segment is a regex metacharacter, and `/v1.0` would otherwise take `/v1X0`.
            regex: format!("^{}(/.*)?$", route.path.replace('.', r"\.")),
            kind: "proxy",
            address: String::new(),
            rewrite: None,
            root: String::new(),
            upstream: String::new(),
        };

        match &route.target {
            Target::Proxy { address, rewrite } => {
                rendering.address.clone_from(address);
                rendering.rewrite.clone_from(rewrite);
            }
            Target::PhpFpm {
                upstream,
                activator,
            } => {
                rendering.kind = "php-fpm";
                rendering.upstream = address(upstream);
                rendering.activator = activator.as_ref().map(address);
            }
            Target::Static { root } => {
                rendering.kind = "static";
                rendering.root = forward_slashed(root);
            }
        }

        rendering
    }
}

/// The activator's address for this kind, as nginx spells one — roadmap task **T70**.
///
/// [`None`] for everything nothing can start by connecting to it, which renders the site exactly as
/// it rendered before T70.
fn activator(kind: &ServedKind) -> Option<String> {
    match kind {
        ServedKind::PhpFpm { activator, .. } => activator.as_ref().map(address),
        _ => None,
    }
}

/// One [`Upstream`] in nginx's spelling.
///
/// **A socket is in double quotes, like every other path this recipe writes.** The pool's socket
/// lives under the home, and macOS's default home has a space in it, which nginx's parser reads as
/// the end of the argument — the same fault Caddy's recipe measured as a 502 on every PHP request.
/// `forward_slashed` has already removed the one character a double-quoted nginx string would
/// process. A TCP address has no space to protect and is left bare.
fn address(upstream: &Upstream) -> String {
    match upstream {
        Upstream::Socket(path) => format!("\"unix:{}\"", forward_slashed(path)),
        Upstream::Tcp(address) => address.to_string(),
    }
}

/// What to call the `upstream` group holding a site's pool and its activator — roadmap task **T70**.
///
/// **Named after the site and never after the pool.** Two sites sharing one pool are the ordinary
/// case, and nginx refuses a configuration that declares one upstream name twice — which takes the
/// whole front end down rather than the one site, since a refused configuration is a server that
/// does not start. A site's primary domain is already unique across this home, which is what
/// `sites/<primary>.conf` relies on to be one file per site.
///
/// Everything but letters, digits and `_` becomes `_`: a group name is a bare token to nginx's
/// parser, and a domain carries dots and hyphens.
fn group(primary: &str) -> String {
    let sanitised: String = primary
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();

    format!("mixengine_{sanitised}")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use mixengine_platform::PortBinding;
    use mixengine_proto::{HealthProbe, ReadyCheck, ReloadBehaviour, ServiceId, StopBehaviour};

    use super::*;
    use crate::generate::recipe;
    use crate::generate::recipe::FrontEndAddition;
    use crate::generate::served::Shared;
    use crate::generate::settings::Settings;

    /// D6: nginx's `fastcgi_params` comes out of the package rather than being written into this
    /// template by hand. `mixengine-packages`' `tools/nginx.py` publishes it under `CONF_FILES` for
    /// exactly this reason, and copying seventeen `fastcgi_param` lines in here would be this
    /// repository maintaining a second copy of a file the server already reads.
    #[test]
    fn the_package_supplies_the_fastcgi_parameters_a_php_site_needs() {
        let endpoints = Nginx
            .endpoints(&context("{}"))
            .expect("a package publishing what a generated configuration includes");

        assert!(
            endpoints.includes.contains_key("fastcgi_params"),
            "a generated nginx configuration has no conf/ beside it, so the file has to be reached \
             where the artifact keeps it"
        );
    }

    /// D8, nginx's half. Caddy takes the port from its global block; nginx has none, so each site's
    /// `server` block declares its own `listen` — and it is the port the process **binds**, which on
    /// macOS is 8080 for a front end answering on 80.
    #[test]
    fn a_site_listens_on_the_port_the_process_binds() {
        let context = context_on("{}", Some(80)).with_bindings(vec![PortBinding {
            answer: 80,
            bind: 8080,
        }]);

        let served = vec![Served {
            shared: None,
            domains: vec!["blog.test".to_owned(), "www.blog.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::Static,
            https: true,
            https_redirect: false,
            certificate: None,
            steps: Vec::new(),
        }];

        let rendered = Nginx.sites(&context, &served).expect("one site")[0]
            .contents()
            .to_owned();

        assert!(rendered.contains("listen 127.0.0.1:8080;"), "{rendered}");
        assert!(
            rendered.contains("server_name blog.test www.blog.test;"),
            "{rendered}"
        );
    }

    /// A document root on whichever system this is compiled for.
    fn doc_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\src\blog\public")
        } else {
            PathBuf::from("/src/blog/public")
        }
    }

    /// Each kind renders the directive that kind needs, and a `node-app` renders exactly what a
    /// reverse proxy to loopback renders — D7 asserted rather than described.
    #[test]
    fn each_kind_renders_a_server_block_naming_what_it_was_given() {
        let served = vec![
            Served {
                shared: None,
                domains: vec!["php.test".to_owned()],
                routes: Vec::new(),
                doc_root: doc_root(),
                doc_root_relative: "public".to_owned(),
                kind: ServedKind::PhpFpm {
                    upstream: Upstream::Socket(PathBuf::from("/home/me/run/php-fpm-8.3.sock")),
                    activator: None,
                },
                https: true,
                https_redirect: false,
                certificate: None,
                steps: Vec::new(),
            },
            Served {
                shared: None,
                domains: vec!["proxy.test".to_owned()],
                routes: Vec::new(),
                doc_root: doc_root(),
                doc_root_relative: "public".to_owned(),
                kind: ServedKind::ReverseProxy {
                    upstream: "http://127.0.0.1:4000".to_owned(),
                    rewrite: None,
                },
                https: true,
                https_redirect: false,
                certificate: None,
                steps: Vec::new(),
            },
            Served {
                shared: None,
                domains: vec!["node.test".to_owned()],
                routes: Vec::new(),
                doc_root: doc_root(),
                doc_root_relative: "public".to_owned(),
                kind: ServedKind::NodeApp { port: 3000 },
                https: true,
                https_redirect: false,
                certificate: None,
                steps: Vec::new(),
            },
        ];

        let documents = Nginx
            .sites(&context("{}"), &served)
            .expect("three site files");

        assert_eq!(
            documents[0].relative(),
            Path::new("sites").join("php.test.conf")
        );

        let php = documents[0].contents();
        assert!(
            php.contains("fastcgi_pass \"unix:/home/me/run/php-fpm-8.3.sock\";"),
            "a socket is spelled nginx's way, which is not Caddy's: {php}"
        );
        assert!(php.contains("include \""), "{php}");

        assert!(
            documents[1]
                .contents()
                .contains("proxy_pass http://127.0.0.1:4000;")
        );
        assert!(
            documents[2]
                .contents()
                .contains("proxy_pass http://127.0.0.1:3000;")
        );
    }

    /// **One file per extension, named after it** — roadmap task **T81c**. The fragment arrives
    /// already substituted and already forward-slashed; what this adds is the header.
    #[test]
    fn an_extension_s_fragment_is_a_file_of_its_own() {
        let context = context("{}").with_fragments(vec![FrontEndAddition {
            extension: "probe".to_owned(),
            fragment: "map $a $b { default 0; }".to_owned(),
        }]);

        let documents = Nginx.fragments(&context).expect("the fragments render");

        assert_eq!(documents.len(), 1);
        assert_eq!(
            documents[0].relative(),
            Path::new("extensions").join("probe.conf")
        );
        assert!(
            documents[0]
                .contents()
                .ends_with("map $a $b { default 0; }\n"),
            "{}",
            documents[0].contents()
        );
    }

    /// One site per file and one fragment per extension, and all three directories are swept —
    /// `welcome/` since roadmap task **T124**, because a deleted site that kept its page would be a
    /// page served for a site that no longer exists.
    #[test]
    fn the_front_end_sweeps_the_directories_that_follow_a_table() {
        assert_eq!(Nginx.swept(), &["sites", "extensions", "welcome"]);
    }

    /// **Two documents per site now** — roadmap task T124: the site's configuration, and the page
    /// it answers with when it has nothing to serve.
    #[test]
    fn a_site_renders_a_welcome_page_beside_its_configuration() {
        let documents = Nginx
            .sites(&context("{}"), &[a_static_site()])
            .expect("a rendering");

        let names: Vec<_> = documents
            .iter()
            .map(|document| {
                document
                    .relative()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect();

        assert!(
            names.contains(&"sites/blog.test.conf".to_owned()),
            "{names:?}"
        );
        assert!(
            names.contains(&"welcome/blog.test.html".to_owned()),
            "{names:?}"
        );
    }

    /// **The root path only, and after the site's own index files** — the T124 design, D3. A
    /// catch-all `error_page 404` would replace an application's own 404, which is MixEngine lying
    /// about somebody else's program.
    #[test]
    fn a_static_site_falls_back_to_the_welcome_page_only_at_the_root() {
        let rendered = render_site(&a_static_site());

        assert!(
            rendered.contains("location = / {"),
            "the welcome route must be nginx's exact match:
{rendered}"
        );
        assert!(
            rendered.contains("try_files /index.html /index.htm @mixengine_welcome;"),
            "the site's own index files must be named before the fallback:
{rendered}"
        );
        assert!(
            !rendered.contains("error_page 404 ="),
            "a file-serving site never replaces an application's own 404:
{rendered}"
        );
        assert!(
            rendered.contains("add_header Cache-Control \"no-store\""),
            "{rendered}"
        );
    }

    /// **Both `server` blocks carry it.** `site.conf` renders the serving locations twice — once in
    /// the block `https_redirect` splits out and once in the combined one — and the copy that is
    /// missed is the one a user with the redirect turned on meets, with both suites still green.
    #[test]
    fn both_server_blocks_fall_back_to_the_welcome_page() {
        let combined = render_site(&a_static_site());
        let redirecting = render_site(&Served {
            https_redirect: true,
            ..a_static_site()
        });

        assert_eq!(
            combined.matches("location @mixengine_welcome").count(),
            1,
            "one serving block, one fallback:
{combined}"
        );
        assert_eq!(
            redirecting.matches("location @mixengine_welcome").count(),
            1,
            "the redirecting block serves nothing, so only the TLS block gets it:
{redirecting}"
        );
    }

    /// **A dead upstream is answered, at every path** — the T124 design, D4. Wide is safe here in
    /// a way it is not for the file kinds: a 502 nginx generated means nothing was listening, so
    /// there is no application whose answer is being overwritten.
    #[test]
    fn a_proxy_site_answers_a_dead_upstream_with_the_welcome_page() {
        let rendered = render_site(&Served {
            kind: ServedKind::NodeApp { port: 3000 },
            ..a_static_site()
        });

        assert!(
            rendered.contains("error_page 502 504 = @mixengine_welcome;"),
            "{rendered}"
        );
        assert!(
            rendered.contains("location @mixengine_welcome"),
            "{rendered}"
        );
    }

    /// **And an upstream's own 502 passes through untouched** — D4's negative half, asserted
    /// because the failure it guards against is a directive somebody adds later believing it
    /// belongs. With `proxy_intercept_errors on;` a user's own gateway reporting its own upstream
    /// would be replaced by MixEngine's page, which is this feature overwriting an application's
    /// answer — the exact thing D3 refuses to do to a 404.
    ///
    /// **The directive and not the word**, on `two_sites_on_one_pool_declare_two_differently_named_groups`'
    /// rule: the comment above it in `site.conf` names it too, and a search for the word would be
    /// green forever whatever the configuration said.
    #[test]
    fn an_upstreams_own_gateway_error_is_not_intercepted() {
        let rendered = render_site(&Served {
            kind: ServedKind::NodeApp { port: 3000 },
            ..a_static_site()
        });

        let directive = rendered
            .lines()
            .find(|line| line.trim_start().starts_with("proxy_intercept_errors"));

        assert!(
            directive.is_none(),
            "this directive would capture a 502 the application sent: {directive:?}"
        );
    }

    /// **The root-only rule belongs to the kinds that serve files.** A proxy site with a working
    /// upstream serves `/` from that upstream, and an exact-match location in front of it would
    /// take the site's own home page away.
    #[test]
    fn a_proxy_site_has_no_root_only_welcome_location() {
        let rendered = render_site(&Served {
            kind: ServedKind::ReverseProxy {
                upstream: "http://127.0.0.1:8000".to_owned(),
                rewrite: None,
            },
            ..a_static_site()
        });

        assert!(
            !rendered.contains("location = / {"),
            "a proxy site's `/` belongs to its upstream:
{rendered}"
        );
    }

    /// **Off renders neither the page nor the route** — the T124 design, D6. Either half alone is
    /// worse than neither: a page nothing routes to is a file nobody reads, and a route with no
    /// page behind it is a 404 on top of the 404 this feature exists to replace.
    #[test]
    fn the_switch_turned_off_renders_no_welcome_at_all() {
        let documents = Nginx
            .sites(&context("{}").with_welcome(false), &[a_static_site()])
            .expect("a rendering");

        assert_eq!(documents.len(), 1, "only the site's own configuration");
        assert!(
            !documents[0].contents().contains("location = / {"),
            "{}",
            documents[0].contents()
        );
    }

    /// **A php-fpm site is answered through the index module, never through `try_files`** — roadmap
    /// task **T124a**, and the assertion that keeps T124's source leak out.
    ///
    /// `try_files` serves the first file it finds *in the current context*, and this location has no
    /// `fastcgi_pass`, so a `location = /` naming `/index.php` would answer the site's home page
    /// with the site's own source, as text. The index module makes an **internal redirect** instead,
    /// so the same file is re-matched by `location ~ \.php$` and runs as PHP.
    #[test]
    fn a_php_site_is_answered_through_the_index_module_and_never_through_try_files() {
        let rendered = render_site(&a_php_site());

        assert!(
            rendered.contains("location = / {"),
            "the welcome route is nginx's exact match on the root:
{rendered}"
        );
        assert!(
            rendered.contains("error_page 403 404 = @mixengine_welcome;"),
            "both statuses the index module reports when it finds nothing:
{rendered}"
        );
        assert!(
            rendered.contains("location @mixengine_welcome"),
            "{rendered}"
        );
    }

    /// **The leak, asserted as an absence** — roadmap task **T124a**. This is the rendering T124
    /// shipped and had to take back, and the failure it causes is a disclosure rather than a wrong
    /// page, so it is worth a test of its own rather than a clause in the one above.
    #[test]
    fn no_try_files_in_this_rendering_ever_names_a_php_file() {
        let rendered = render_site(&a_php_site());

        let offending = rendered.lines().map(str::trim_start).find(|line| {
            line.starts_with("try_files") && line.contains(".php") && {
                // The front controller is the *last* element, which try_files reaches by internal
                // redirect — that one is correct and is what `location /` has always rendered.
                let items: Vec<_> = line
                    .trim_end_matches(';')
                    .split_whitespace()
                    .skip(1)
                    .collect();
                items
                    .iter()
                    .take(items.len().saturating_sub(1))
                    .any(|item| item.contains(".php"))
            }
        });

        assert!(
            offending.is_none(),
            "a .php file before the last element of try_files is served in place, as text: {offending:?}"
        );
    }

    /// **An application's own error is still its own** — the T124 design, D3, and T167f.
    ///
    /// The `error_page` for a 404 lives in an exact-match location on `/`, so no other path reaches
    /// it. The php handler carries exactly one, for the 502 and 504 nginx produces when the pool
    /// cannot be reached — the starting page — and never `fastcgi_intercept_errors`, which is what
    /// would make an application's own 502 reach it too.
    #[test]
    fn the_error_page_is_scoped_to_the_root_and_not_to_the_php_handler() {
        let rendered = render_site(&a_php_site());

        let php_block = rendered
            .split(r"location ~ \.php$ {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }").next())
            .expect("a php location");

        assert_eq!(
            php_block
                .lines()
                .filter(|line| line.trim_start().starts_with("error_page"))
                .count(),
            1,
            "the php handler answers with whatever the application said, except nginx's own 502:
{php_block}"
        );
        assert!(
            php_block.contains("error_page 502 504 = @mixengine_starting;"),
            "{php_block}"
        );
        assert!(
            rendered.contains("location @mixengine_starting {"),
            "the named location the error page names: {rendered}"
        );
        assert!(
            !rendered.contains("fastcgi_intercept_errors"),
            "this directive would capture an error the application sent:
{rendered}"
        );
    }

    /// **`alias` is refused inside a named location, and nginx refuses the whole file over it** —
    /// roadmap task **T124a**, measured against nginx 1.31.3: *the "alias" directive cannot be used
    /// inside the named location*. One page's mistake would take every site on the machine down
    /// with it, which is why this is asserted here rather than left to the serving suite.
    #[test]
    fn the_welcome_location_is_served_with_root_and_never_with_alias() {
        for site in [a_static_site(), a_php_site()] {
            let rendered = render_site(&site);
            let named = rendered
                .split("location @mixengine_welcome {")
                .nth(1)
                .expect("the welcome location");
            let body = named
                .split(
                    "
    }",
                )
                .next()
                .expect("its body");

            assert!(
                !body.contains("alias "),
                "nginx refuses this configuration outright:
{body}"
            );
            assert!(body.contains("root \""), "{body}");
            assert!(body.contains("try_files /blog.test.html =404;"), "{body}");
        }
    }

    /// A php-fpm site with three routes, as `served` hands them over — roadmap task **T135**.
    fn a_site_with_routes() -> Served {
        use crate::generate::served::{Rewrite, ServedRoute, ServedRouteTarget};

        Served {
            routes: vec![
                ServedRoute {
                    path: "/v1.0".to_owned(),
                    target: ServedRouteTarget::Proxy {
                        address: "http://127.0.0.1:4000".to_owned(),
                        rewrite: None,
                    },
                },
                ServedRoute {
                    path: "/abc".to_owned(),
                    target: ServedRouteTarget::Proxy {
                        address: "http://127.0.0.1:3003".to_owned(),
                        rewrite: Some(Rewrite {
                            regex: "^/abc(/.*)?$".to_owned(),
                            replacement: "/xyz$1".to_owned(),
                        }),
                    },
                },
                ServedRoute {
                    path: "/admin".to_owned(),
                    target: ServedRouteTarget::PhpFpm {
                        upstream: Upstream::Tcp("127.0.0.1:9001".parse().expect("an address")),
                        activator: None,
                    },
                },
            ],
            ..a_php_site()
        }
    }

    /// **Every route is rendered before the site's own `location ~ \.php$`** — roadmap task
    /// **T135**.
    ///
    /// nginx takes regex locations in the order it read them, so a route written after that
    /// handler would never be reached for a `.php` under its prefix.
    #[test]
    fn a_route_is_rendered_before_the_php_handler() {
        let rendered = render_site(&a_site_with_routes());

        let route = rendered
            .find("location ~ ^/abc(/.*)?$")
            .expect("the route location");
        // The site's own handler is at the server block's own indentation; a route's nested one is
        // four spaces deeper, which is what tells the two apart in the text.
        let php = rendered
            .find("\n    location ~ \\.php$ {")
            .expect("the site's own php handler");

        assert!(
            route < php,
            "a route rendered after the php handler is a route nginx never reaches:\n{rendered}"
        );

        // **Where the site renders two blocks, both carry the routes.** A site whose `/api`
        // answered on HTTP and not on HTTPS would be a padlock that works everywhere except where
        // the application is — and the redirecting shape is the one that has two serving blocks to
        // get wrong.
        let redirecting = render_site(&Served {
            https_redirect: true,
            ..a_site_with_routes()
        });

        assert_eq!(
            redirecting.matches("location ~ ^/abc(/.*)?$").count(),
            1,
            "the block that only redirects carries no routes; the one that serves carries them all:
{redirecting}"
        );
        assert!(
            redirecting.contains("return 307 https://$host$request_uri;"),
            "{redirecting}"
        );
    }

    /// **A file-serving route is a pair of prefix locations with its PHP handler nested inside** —
    /// roadmap task **T135**, that design's D4.
    ///
    /// `^~` stops the sibling regex search, so a handler written beside this location would never
    /// be reached and `try_files $uri` would answer `/admin/x.php` with its own source, as text —
    /// T124a's leak, in a new place.
    #[test]
    fn a_php_route_nests_its_handler_inside_the_prefix_location() {
        let rendered = render_site(&a_site_with_routes());

        assert!(
            rendered.contains("location = /admin { return 301 /admin/; }"),
            "the exact match is the segment boundary:\n{rendered}"
        );
        assert!(rendered.contains("location ^~ /admin/ {"), "{rendered}");

        let prefix = rendered
            .find("location ^~ /admin/ {")
            .expect("the prefix location");
        let nested = rendered[prefix..]
            .find("fastcgi_pass 127.0.0.1:9001;")
            .expect("the route's own pool");
        let closed = rendered[prefix..]
            .find("\n    }")
            .expect("its closing brace");

        assert!(
            nested < closed,
            "the php handler is inside the prefix location, not beside it:\n{rendered}"
        );
        assert!(
            rendered.contains("try_files $uri $uri/ /admin/index.php?$query_string;"),
            "{rendered}"
        );
    }

    /// **A `.` in a path is escaped where it reaches a regex** — roadmap task **T135**, that
    /// design's D5. Unescaped, `/v1.0` would also take `/v1X0`.
    #[test]
    fn a_dot_in_a_route_path_is_escaped() {
        let rendered = render_site(&a_site_with_routes());

        assert!(
            rendered.contains(r"location ~ ^/v1\.0(/.*)?$"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("location ~ ^/v1.0(/.*)?$"),
            "an unescaped dot matches any character:\n{rendered}"
        );
        assert!(
            rendered.contains("proxy_pass http://127.0.0.1:4000;"),
            "a route whose upstream has no path forwards it unchanged:\n{rendered}"
        );
        assert!(
            rendered.contains("rewrite ^/abc(/.*)?$ /xyz$1 break;"),
            "{rendered}"
        );
    }

    /// A php-fpm site at `blog.test`, on the static fixture's domain and certificate.
    fn a_php_site() -> Served {
        Served {
            kind: ServedKind::PhpFpm {
                upstream: Upstream::Tcp("127.0.0.1:9000".parse().expect("an address")),
                activator: None,
            },
            ..a_static_site()
        }
    }

    /// A static site at `blog.test` with a certificate, so both shapes of the template are
    /// reachable from one fixture.
    fn a_static_site() -> Served {
        Served {
            shared: None,
            domains: vec!["blog.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::Static,
            https: true,
            https_redirect: false,
            certificate: Some(crate::generate::served::SiteCertificate {
                certificate: std::path::PathBuf::from("/certs/blog.test.crt"),
                key: std::path::PathBuf::from("/certs/blog.test.key"),
                fingerprint: "ab".repeat(32),
            }),
            steps: Vec::new(),
        }
    }

    /// An absolute path on whichever system this is compiled for.
    const fn root() -> &'static str {
        if cfg!(windows) {
            r"C:\MixEngine"
        } else {
            "/opt/mixengine"
        }
    }

    /// Where the binary sits inside the archive, as the index publishes it.
    fn nginx_binary() -> String {
        format!("nginx{}", std::env::consts::EXE_SUFFIX)
    }

    /// A static site at `blog.test` with a certificate — roadmap task **T51**.
    ///
    /// Paths that do not exist, deliberately: this module renders text and never reads a disk.
    /// Whether the pair is there was decided in `generate::served`.
    fn a_site_with_a_certificate() -> Served {
        Served {
            shared: None,
            domains: vec!["blog.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::Static,
            https: true,
            https_redirect: false,
            certificate: Some(crate::generate::served::SiteCertificate {
                certificate: PathBuf::from("/home/someone/.mixengine/certs/sites/blog.test.crt"),
                key: PathBuf::from("/home/someone/.mixengine/certs/sites/blog.test.key"),
                fingerprint: "ab".repeat(32),
            }),
            steps: Vec::new(),
        }
    }

    /// One site through the real recipe.
    fn render_site(site: &Served) -> String {
        Nginx
            .sites(&context("{}"), std::slice::from_ref(site))
            .expect("one site file")[0]
            .contents()
            .to_owned()
    }

    /// **nginx says in one directive what Caddy needs three for** — roadmap task **T70**, D2.
    ///
    /// `backup` in an `upstream` group *is* "only when the others have refused", so there is no
    /// policy to state and no load balancing to switch off. Measured against a real nginx 1.24.0
    /// with the pool's address dead: 200 on the first request, in 7.9 ms. What has to be right here
    /// is the shape — a group, the pool plain, the activator marked `backup`, and `fastcgi_pass`
    /// pointing at the group rather than at either address.
    #[test]
    fn a_pool_that_can_be_woken_renders_a_group_whose_second_server_is_a_backup() {
        let rendered = render_site(&Served {
            shared: None,
            domains: vec!["php.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::PhpFpm {
                upstream: Upstream::Tcp("127.0.0.1:9000".parse().expect("an address")),
                activator: Some(Upstream::Tcp("127.0.0.1:9500".parse().expect("an address"))),
            },
            https: false,
            https_redirect: false,
            certificate: None,
            steps: Vec::new(),
        });

        assert!(
            rendered.contains("server 127.0.0.1:9000;"),
            "the pool is the ordinary member of the group:\n{rendered}"
        );
        assert!(
            rendered.contains("server 127.0.0.1:9500 backup;"),
            "the activator is reached only once the pool has refused:\n{rendered}"
        );
        assert!(
            !rendered.contains("fastcgi_pass 127.0.0.1:9000;"),
            "passing straight to the pool bypasses the group that makes the fallback work:\n\
             {rendered}"
        );
    }

    /// **Two sites sharing one pool must not declare one `upstream` name twice.**
    ///
    /// nginx refuses a configuration with a duplicate upstream name outright, so the group is named
    /// after the *site* and not after the pool — and this is what would catch a name derived from
    /// the pool instead. The whole front end fails to start when it is wrong, not the one site.
    #[test]
    fn two_sites_on_one_pool_declare_two_differently_named_groups() {
        let pool = Upstream::Tcp("127.0.0.1:9000".parse().expect("an address"));
        let activator = Some(Upstream::Tcp("127.0.0.1:9500".parse().expect("an address")));

        let served: Vec<Served> = ["one.test", "two.test"]
            .into_iter()
            .map(|domain| Served {
                shared: None,
                domains: vec![domain.to_owned()],
                routes: Vec::new(),
                doc_root: doc_root(),
                doc_root_relative: "public".to_owned(),
                kind: ServedKind::PhpFpm {
                    upstream: pool.clone(),
                    activator: activator.clone(),
                },
                https: false,
                https_redirect: false,
                certificate: None,
                steps: Vec::new(),
            })
            .collect();

        let documents = Nginx
            .sites(&context("{}"), &served)
            .expect("two site files");

        let names: Vec<String> = documents
            .iter()
            // Site configurations only: since T124 a welcome page is rendered beside each of them,
            // and an HTML file declares no `upstream` group.
            .filter(|document| document.relative().starts_with(SITES))
            .map(|document| {
                // The *directive* and not the word: this file explains itself in prose that says
                // "upstream" too, and a search that found the comment would compare two identical
                // sentences and pass whatever the names were.
                document
                    .contents()
                    .lines()
                    .find(|line| line.starts_with("upstream "))
                    .expect("a group at the top level")
                    .trim_end_matches(" {")
                    .to_owned()
            })
            .collect();

        assert_ne!(
            names[0], names[1],
            "nginx refuses a duplicate upstream name and the whole front end fails to start"
        );
    }

    /// A pool with no activator renders what it rendered before T70: a `fastcgi_pass` straight at
    /// the pool, and no group at all.
    #[test]
    fn a_pool_with_no_activator_is_passed_to_directly() {
        let rendered = render_site(&Served {
            shared: None,
            domains: vec!["php.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::PhpFpm {
                upstream: Upstream::Tcp("127.0.0.1:9000".parse().expect("an address")),
                activator: None,
            },
            https: false,
            https_redirect: false,
            certificate: None,
            steps: Vec::new(),
        });

        assert!(
            rendered.contains("fastcgi_pass 127.0.0.1:9000;"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("upstream "),
            "a group of one is a group for nothing:\n{rendered}"
        );
    }

    /// **The authority is rendered into a directory that holds only it** — the T75 design, D9.
    ///
    /// `certs/ca/root.key` sits beside `certs/ca/root.crt`, so a front end pointed at the
    /// certificates directory would serve this home's signing key to the local network. The copy is
    /// what makes the served directory provably harmless, and this is where that is checked.
    #[test]
    fn the_front_end_renders_the_public_authority_and_nothing_beside_it() {
        let context = context("{}").with_authority(Some(
            "-----BEGIN CERTIFICATE-----
zz
"
            .to_owned(),
        ));

        let documents = Nginx.sites(&context, &[]).expect("the authority");

        let authority: Vec<_> = documents
            .iter()
            .filter(|document| document.relative().starts_with("public"))
            .collect();

        assert_eq!(authority.len(), 1, "{documents:?}");
        assert_eq!(
            authority[0].relative(),
            std::path::Path::new("public/ca.crt")
        );
        assert!(authority[0].contents().contains("BEGIN CERTIFICATE"));
        assert!(!authority[0].contents().contains("PRIVATE KEY"));
    }

    /// A home with no authority renders no file, rather than an empty one a phone would download
    /// and fail to install.
    #[test]
    fn a_home_with_no_authority_renders_no_authority_file() {
        let documents = Nginx.sites(&context("{}"), &[]).expect("no sites");

        assert!(
            !documents
                .iter()
                .any(|document| document.relative().starts_with("public")),
            "{documents:?}"
        );
    }

    /// **The CA is downloadable from a shared site and from no other** — the T75 design, D9.
    ///
    /// "Served only while sharing is on" is a property of the rendering, which is what makes it
    /// something a test can hold rather than something a reviewer has to believe.
    #[test]
    fn a_shared_site_serves_the_authority() {
        let context = context("{}").with_authority(Some("-----BEGIN CERTIFICATE-----".to_owned()));

        let rendered = Nginx
            .sites(&context, &[a_shared_site([192, 168, 1, 10])])
            .expect("one site")[0]
            .contents()
            .to_owned();

        assert!(rendered.contains("/__mixengine/ca.crt"), "{rendered}");
        assert!(
            rendered.contains("application/x-x509-ca-cert"),
            "{rendered}"
        );
    }

    /// **The route never names the certificates directory**, which is where the signing key is.
    ///
    /// A test rather than a comment, because the difference between safe and catastrophic here is
    /// one word in a path.
    #[test]
    fn the_authority_route_never_names_the_certificates_directory() {
        let context = context("{}").with_authority(Some("-----BEGIN CERTIFICATE-----".to_owned()));

        let rendered = Nginx
            .sites(&context, &[a_shared_site([192, 168, 1, 10])])
            .expect("one site")[0]
            .contents()
            .to_owned();

        // **Directives only.** The comment above the route names `certs/ca/` in order to say why
        // it is not what the route points at, and a whole-file search would read that as the
        // opposite of what it says.
        let directives: String = rendered
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join(
                "
",
            );

        assert!(!directives.contains("root.key"), "{directives}");
        assert!(!directives.contains("certs"), "{directives}");
    }

    /// An unshared site beside a shared one serves nothing of the sort.
    #[test]
    fn an_unshared_site_serves_no_authority() {
        let context = context("{}").with_authority(Some("-----BEGIN CERTIFICATE-----".to_owned()));

        let rendered = Nginx
            .sites(
                &context,
                &[Served {
                    shared: None,
                    domains: vec!["shop.test".to_owned()],
                    routes: Vec::new(),
                    doc_root: doc_root(),
                    doc_root_relative: "public".to_owned(),
                    kind: ServedKind::Static,
                    https: false,
                    https_redirect: false,
                    certificate: None,
                    steps: Vec::new(),
                }],
            )
            .expect("one site")[0]
            .contents()
            .to_owned();

        assert!(!rendered.contains("__mixengine"), "{rendered}");
    }

    /// A site shared on the LAN, without a certificate — roadmap task **T74**.
    fn a_shared_site(address: [u8; 4]) -> Served {
        Served {
            shared: Some(Shared {
                address: address.into(),
                name: Some("blog-mixengine.local".to_owned()),
            }),
            domains: vec!["blog.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::Static,
            https: false,
            https_redirect: false,
            certificate: None,
            steps: Vec::new(),
        }
    }

    /// **A second `listen`, not a changed one** — the T74 design, D2. The browser on this machine
    /// and the phone are looking at the same site at the same time, so loopback stays.
    #[test]
    fn a_shared_site_listens_on_loopback_and_the_lan_address() {
        let rendered = render_site(&a_shared_site([192, 168, 1, 10]));

        assert!(rendered.contains("listen 127.0.0.1:80;"), "{rendered}");
        assert!(rendered.contains("listen 192.168.1.10:80;"), "{rendered}");
        assert_eq!(
            rendered
                .matches(
                    "
    listen "
                )
                .count(),
            2,
            "{rendered}"
        );
    }

    /// **The name joins `server_name`** — the T75 design, D3. nginx picks a listener group by
    /// address and only then consults the names, so this is the half that decides which site
    /// replies to `blog-mixengine.local` once the responder has said where it resolves.
    #[test]
    fn a_shared_site_names_itself_in_server_name() {
        let rendered = render_site(&a_shared_site([192, 168, 1, 10]));

        assert!(
            rendered.contains("server_name blog.test blog-mixengine.local;"),
            "{rendered}"
        );
    }

    /// **An unshared site carries no name at all**, which is what makes "opt-in per site" a
    /// property of the rendering rather than a promise made about it.
    #[test]
    fn an_unshared_site_carries_no_mdns_name() {
        let rendered = render_site(&Served {
            shared: None,
            domains: vec!["shop.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::Static,
            https: false,
            https_redirect: false,
            certificate: None,
            steps: Vec::new(),
        });

        // The directive rather than the whole file: the comment above it names the shape a
        // shared site's name takes, and a comment is not a name this site answers to.
        assert!(rendered.contains("server_name shop.test;"), "{rendered}");
    }

    /// **Two sites shared on one address** — the assumption T74 recorded and could not yet reach.
    ///
    /// nginx groups servers by listen address before it consults `server_name`, so a request
    /// carrying `Host: <ip>` is answered by that group's *default* — the first server block on the
    /// address — while each site's own mDNS name is matched by name. Asserted so that a later
    /// change cannot quietly move which site an address-shaped `Host` reaches.
    #[test]
    fn two_sites_shared_on_one_address_are_told_apart_by_name() {
        let shop = Served {
            shared: Some(Shared {
                address: [192, 168, 1, 10].into(),
                name: Some("shop-mixengine.local".to_owned()),
            }),
            domains: vec!["shop.test".to_owned()],
            routes: Vec::new(),
            doc_root: doc_root(),
            doc_root_relative: "public".to_owned(),
            kind: ServedKind::Static,
            https: false,
            https_redirect: false,
            certificate: None,
            steps: Vec::new(),
        };

        let rendered = Nginx
            .sites(&context("{}"), &[a_shared_site([192, 168, 1, 10]), shop])
            .expect("two site files");

        assert!(
            rendered[0].contents().contains("blog-mixengine.local"),
            "{}",
            rendered[0].contents()
        );
        assert!(
            rendered[1].contents().contains("shop-mixengine.local"),
            "{}",
            rendered[1].contents()
        );

        // Both on the one address, which is what makes the group's default the question it is.
        assert_eq!(
            rendered
                .iter()
                .filter(|document| document.contents().contains("listen 192.168.1.10:80;"))
                .count(),
            2
        );
    }

    /// Sharing is opt-in per site: the site beside it is untouched.
    #[test]
    fn an_unshared_site_listens_once_per_scheme() {
        let rendered = render_site(&Served {
            shared: None,
            ..a_shared_site([192, 168, 1, 10])
        });

        assert_eq!(
            rendered
                .matches(
                    "
    listen "
                )
                .count(),
            1,
            "{rendered}"
        );
        assert!(!rendered.contains("192.168.1.10"), "{rendered}");
    }

    /// A shared HTTPS site listens on the LAN address over TLS as well, because the certificate
    /// covers that address as an IP SAN — T74, D9.
    #[test]
    fn a_shared_https_site_offers_tls_on_the_lan_address_too() {
        let rendered = render_site(&Served {
            shared: Some(Shared {
                address: [192, 168, 1, 10].into(),
                name: Some("blog-mixengine.local".to_owned()),
            }),
            ..a_site_with_a_certificate()
        });

        assert_eq!(
            rendered
                .matches(
                    "
    listen "
                )
                .count(),
            4,
            "{rendered}"
        );
        assert_eq!(
            rendered.matches("192.168.1.10").count(),
            2,
            "one plaintext listener and one TLS one:
{rendered}"
        );
    }

    /// **The consequence of D2 and D3, asserted rather than discovered.**
    ///
    /// nginx groups servers by listen address before it consults `server_name`, so the LAN address
    /// has exactly one server block in its group: a request arriving from the network with another
    /// site's `Host` is answered by the shared site as that group's default, not by the site it
    /// named. That is the intended outcome — no unshared site is served over the LAN — and this
    /// test exists so a later change cannot quietly turn it into the other one.
    #[test]
    fn the_lan_address_belongs_to_the_shared_site_alone() {
        let shared = a_shared_site([192, 168, 1, 10]);
        let other = Served {
            shared: None,
            domains: vec!["shop.test".to_owned()],
            ..a_shared_site([192, 168, 1, 10])
        };

        let documents = Nginx
            .sites(&context("{}"), &[shared, other])
            .expect("two site files");

        // One file per site, so the question is which files carry the address rather than which
        // blocks do — and the answer has to be exactly the shared one.
        let carrying: Vec<&str> = documents
            .iter()
            .map(|document| document.contents())
            .filter(|contents| contents.contains("192.168.1.10"))
            .collect();

        assert_eq!(carrying.len(), 1, "{documents:?}");
        assert!(carrying[0].contains("blog.test"), "{}", carrying[0]);
        assert!(!carrying[0].contains("shop.test"), "{}", carrying[0]);
    }

    /// An HTTPS site listens twice and names its certificate — roadmap task **T51**.
    ///
    /// **One `server` block and not two, unlike Caddy**, and that was measured rather than assumed:
    /// nginx 1.24 answers `syntax is ok` to a block carrying a plaintext listener and a TLS one,
    /// where Caddy refuses the equivalent outright. TLS attaches to a listener here and to a site
    /// block there.
    #[test]
    fn an_https_site_listens_on_tls_and_names_its_certificate() {
        let rendered = render_site(&a_site_with_a_certificate());

        assert_eq!(rendered.matches("\n    listen ").count(), 2, "{rendered}");
        assert!(rendered.contains(" ssl;"), "{rendered}");
        assert!(
            rendered.contains(
                "ssl_certificate \"/home/someone/.mixengine/certs/sites/blog.test.crt\";"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "ssl_certificate_key \"/home/someone/.mixengine/certs/sites/blog.test.key\";"
            ),
            "{rendered}"
        );
        assert_eq!(rendered.matches("server {").count(), 1, "{rendered}");
    }

    /// **Two `server` blocks, where T51's own D6 needed only one** — roadmap task **T98**. The
    /// block that answers on the plaintext listener returns 307 and carries none of the site's own
    /// content; everything `an_https_site_listens_on_tls_and_names_its_certificate` asserts about a
    /// single-block HTTPS site is still true of the *second* block here.
    #[test]
    fn a_site_with_redirect_on_renders_a_redirecting_block_and_a_serving_block() {
        let rendered = render_site(&Served {
            https_redirect: true,
            ..a_site_with_a_certificate()
        });

        assert_eq!(rendered.matches("server {").count(), 2, "{rendered}");
        assert!(
            rendered.contains("return 307 https://$host$request_uri;"),
            "{rendered}"
        );
        assert_eq!(
            rendered.matches("try_files $uri $uri/ =404;").count(),
            1,
            "only the serving block has the site's own content: {rendered}"
        );
        assert!(
            rendered.contains(
                "ssl_certificate \"/home/someone/.mixengine/certs/sites/blog.test.crt\";"
            ),
            "{rendered}"
        );
    }

    /// **Off by default, and a site that never asked still renders the one block T51 shipped** —
    /// the regression this whole feature must not be.
    #[test]
    fn a_site_with_redirect_off_renders_one_server_block_as_before() {
        let rendered = render_site(&a_site_with_a_certificate());

        assert_eq!(rendered.matches("server {").count(), 1, "{rendered}");
        assert!(!rendered.contains("return 307"), "{rendered}");
    }

    /// **A redirect needs a usable certificate, not only the flag** — the T51 design's D4 applied a
    /// second time, on nginx's own shape: a site that asked for HTTPS but has nothing on disk to
    /// serve it with already renders one plaintext listener; asking for a redirect too must not
    /// turn that into a 307 toward a TLS listener nothing is bound to.
    #[test]
    fn a_site_with_redirect_on_but_no_certificate_renders_one_block_as_before() {
        let mut site = a_site_with_a_certificate();
        site.https_redirect = true;
        site.certificate = None;

        let rendered = render_site(&site);

        assert_eq!(rendered.matches("server {").count(), 1, "{rendered}");
        assert!(!rendered.contains("return 307"), "{rendered}");
        assert!(
            rendered.contains("try_files $uri $uri/ =404;"),
            "{rendered}"
        );
    }

    /// **The CA route is reachable over plaintext on a redirecting shared site** — roadmap task
    /// **T98**, the design's D3, on nginx's own shape: the route lives in the redirecting block
    /// beside the `location /` that returns 307, and nowhere in the serving block behind it — a
    /// phone that has not yet trusted this home's authority cannot reach that block at all.
    #[test]
    fn a_redirecting_shared_site_still_serves_its_ca_route_over_plaintext() {
        let context = context("{}").with_authority(Some("-----BEGIN CERTIFICATE-----".to_owned()));
        let site = Served {
            https_redirect: true,
            certificate: Some(crate::generate::served::SiteCertificate {
                certificate: PathBuf::from("/home/someone/.mixengine/certs/sites/blog.test.crt"),
                key: PathBuf::from("/home/someone/.mixengine/certs/sites/blog.test.key"),
                fingerprint: "ab".repeat(32),
            }),
            ..a_shared_site([192, 168, 1, 10])
        };

        let rendered = Nginx.sites(&context, &[site]).expect("one site")[0]
            .contents()
            .to_owned();

        assert!(
            rendered.contains("location = /__mixengine/ca.crt {"),
            "{rendered}"
        );
        assert!(
            rendered.contains("return 307 https://$host$request_uri;"),
            "{rendered}"
        );

        // The CA route has to sit in the *redirecting* block, which a plaintext request actually
        // reaches — not in the serving block behind the TLS listener a phone cannot get to yet.
        let ca_route = rendered.find("/__mixengine/ca.crt").expect("the route");
        let second_server = rendered.rfind("server {").expect("the second block");
        assert!(ca_route < second_server, "{rendered}");
    }

    /// A site with no certificate listens once and names none — the T51 design, D4.
    #[test]
    fn a_site_with_no_certificate_has_one_listener() {
        let mut site = a_site_with_a_certificate();
        site.certificate = None;

        let rendered = render_site(&site);

        assert_eq!(rendered.matches("\n    listen ").count(), 1, "{rendered}");
        assert!(!rendered.contains("ssl"), "{rendered}");
    }

    /// **The fingerprint is what makes a reissue reload** — the T51 design, D5.
    #[test]
    fn a_different_certificate_renders_a_different_file() {
        let site = a_site_with_a_certificate();
        let before = render_site(&site);

        let mut reissued = site.clone();
        reissued
            .certificate
            .as_mut()
            .expect("a certificate")
            .fingerprint = "cd".repeat(32);

        assert_ne!(
            before,
            render_site(&reissued),
            "a reissued certificate rendered the same bytes, so nothing would reload"
        );
    }

    /// And nothing changing renders identical bytes — a rendering that had become unstable would
    /// reload the front end on every unrelated `service.*` call.
    #[test]
    fn rendering_twice_with_nothing_changed_is_identical() {
        let site = a_site_with_a_certificate();

        assert_eq!(render_site(&site), render_site(&site));
    }
    /// An nginx on port 80 in a home at [`root`], with `overrides` applied.
    ///
    /// The root is a plain string rather than a temporary directory, for [`super::super::caddy`]'s
    /// reason: nothing here writes a file, and what the assertions are about is the *text* a path
    /// becomes. On Windows that text contains backslashes, which is the subject of one of these.
    fn context(overrides: &str) -> Context {
        context_on(overrides, Some(80))
    }

    /// The same, with the port the row carries spelled out — what D8's half of this recipe is about.
    fn context_on(overrides: &str, port: Option<u16>) -> Context {
        let service = ServiceId::parse("nginx").expect("an id");
        let settings =
            Settings::merge(Nginx.settings(), overrides, &service).expect("usable overrides");

        let context = Context::for_test(
            service,
            PACKAGE,
            Path::new(root()),
            // What `mixengine-packages` publishes: the server, and the data files a generated
            // configuration includes. `mime.types` is the one nothing works without.
            [
                ("nginx".to_owned(), nginx_binary()),
                (MIME_TYPES.to_owned(), "conf/mime.types".to_owned()),
                (FASTCGI_PARAMS.to_owned(), "conf/fastcgi_params".to_owned()),
            ]
            .into_iter()
            .collect(),
            port,
            settings,
        );

        // What `Generator::render` does before it renders anything, and this template needs it: the
        // `include` of the archive's own `mime.types` is resolved here rather than joined in the
        // file.
        let endpoints = Nginx
            .endpoints(&context)
            .expect("a package publishing what a generated configuration includes");

        context.with_endpoints(endpoints)
    }

    /// What the file renders to, for `overrides`.
    fn conf(overrides: &str) -> String {
        let documents = recipe::render(&Nginx, &context(overrides)).expect("a rendering");

        assert_eq!(documents.len(), 1, "nginx renders one file");
        assert_eq!(documents[0].relative(), Path::new(CONFIG_FILE));

        documents[0].contents().to_owned()
    }

    /// The spec this recipe builds for `overrides`.
    fn spec(overrides: &str) -> ServiceSpec {
        Nginx
            .spec(&context(overrides))
            .expect("a builder")
            .build()
            .expect("a usable spec")
    }

    /// There is one nginx, which is what stops `service.create` being asked for a second one.
    ///
    /// The same answer as Caddy's and for the same sentence in `docs/features/services.md`:
    /// exactly one active front end. What stops a *Caddy* being created beside this one is
    /// [`Recipe::role`], which is a different rule about a different mistake.
    #[test]
    fn nginx_exists_once() {
        assert_eq!(Nginx.instancing(), Instancing::Single);
    }

    /// And it is a front end, which is the half `service.create` reads.
    #[test]
    fn nginx_is_a_front_end() {
        assert_eq!(
            Nginx.role(),
            Role::FrontEnd(mixengine_proto::FrontEndServer::Nginx)
        );
    }

    /// What a failed start is diagnosed against — roadmap task **T38**.
    ///
    /// **The status endpoint alone.** The row's own port is what sites will be served on, and this
    /// recipe writes no listener for it until sites exist (T43) — so an nginx that failed to start
    /// never wanted 80, and declaring it would put another program's IIS into the reason for a
    /// failure that was not about it.
    #[test]
    fn the_spec_declares_the_status_endpoint_it_will_bind() {
        assert_eq!(spec("{}").ports(), [2020]);
    }

    /// An artifact that unpacks and will not run is one the user meets against their own site.
    ///
    /// `-v` and not `-t`: the second reads a configuration, and there is none to read at the moment
    /// an archive is being installed.
    #[test]
    fn nginx_proves_itself_by_running() {
        let smoke = Nginx.smoke_test().expect("a server proves that it runs");

        assert_eq!(smoke.executable, PACKAGE);
        assert_eq!(smoke.args, ["-v"]);
    }

    #[test]
    fn the_rendering_says_what_the_row_and_the_defaults_say() {
        let rendered = conf("{}");

        assert!(rendered.contains("worker_processes 1;"), "{rendered}");
        assert!(rendered.contains("worker_connections 1024;"), "{rendered}");
        assert!(
            rendered.contains("listen 127.0.0.1:2020;"),
            "the status endpoint is not in the file: {rendered}"
        );
        assert!(rendered.contains(HEALTH_PATH), "{rendered}");
        assert!(rendered.contains("client_max_body_size 64m;"), "{rendered}");
        assert!(rendered.contains("include sites/*.conf;"), "{rendered}");

        // The one data file nothing works without, reached by the absolute path the index publishes
        // it at rather than through nginx's own `conf/` — which a generated configuration has none
        // of. Asserted as the whole path, because "contains mime.types" would also be true of a
        // template that joined one itself and got the layout wrong.
        let published = context("{}")
            .provided(MIME_TYPES)
            .expect("a published mime.types")
            .to_string_lossy()
            .replace('\\', "/");
        assert!(
            rendered.contains(&format!("include \"{published}\";")),
            "{rendered}"
        );
    }

    /// **In the foreground, with its errors on the stream.**
    ///
    /// `daemon off;` is nginx's spelling of the decision `caddy run` is Caddy's: the default forks a
    /// master and returns, so what a supervisor would be watching is a launcher that has already
    /// exited. `-e stderr` is the other half — an error *before* the configuration has been read
    /// goes to the compiled-in `logs/error.log` under the prefix otherwise, which is a file nobody
    /// is reading.
    #[test]
    fn the_program_stays_in_the_foreground_and_says_so_on_the_stream() {
        let rendered = conf("{}");
        assert!(rendered.contains("daemon off;"), "{rendered}");

        let spec = spec("{}");
        let args = spec.args().join(" ");

        assert!(args.contains("-e stderr"), "{args}");
        assert!(args.contains("-c "), "{args}");
        assert!(args.contains("-p "), "{args}");
    }

    /// **Every path this file writes is forward-slashed and quoted**, which is MariaDB's finding in
    /// nginx's spelling: `ngx_conf_read_token` treats `\` inside a quoted string as an escape, so a
    /// home under `C:\Users\Nguyen Hai Quang` loses every separator — and unquoted, the directive
    /// stops at the space instead. nginx accepts `/` on Windows, so one spelling works on all three
    /// systems.
    #[test]
    fn every_path_in_the_rendering_is_written_the_way_a_windows_path_survives() {
        let rendered = conf("{}");
        let home = root().replace('\\', "/");

        assert!(
            !rendered.contains(root()) || !cfg!(windows),
            "a path reached nginx.conf with backslashes in it: {rendered}"
        );

        for line in rendered.lines().filter(|line| line.contains(&home)) {
            assert!(
                line.matches('"').count() == 2,
                "a path reached nginx.conf outside quotes: {line}"
            );
        }
    }

    /// The five temp directories nginx makes for itself are children of one that already exists.
    ///
    /// The finding is `mixengine-packages`' and it is the reason this is asserted rather than
    /// assumed: nginx creates `client_body_temp` and the four beside it with a **single** `mkdir`,
    /// so a missing parent is `[emerg] CreateDirectory() failed (3)` on a configuration that passed
    /// `nginx -t` one line earlier. The data directory is made by `Generator::render` (T35); putting
    /// the five leaves directly inside it is what makes a `temp/` nobody creates unnecessary.
    #[test]
    fn every_temp_directory_nginx_makes_has_a_parent_that_already_exists() {
        let context = context("{}");
        let data = context.data().to_string_lossy().replace('\\', "/");
        let rendered = conf("{}");

        for directive in [
            "client_body_temp_path",
            "proxy_temp_path",
            "fastcgi_temp_path",
            "scgi_temp_path",
            "uwsgi_temp_path",
        ] {
            let line = rendered
                .lines()
                .find(|line| line.trim_start().starts_with(directive))
                .unwrap_or_else(|| panic!("{directive} is not in the rendering: {rendered}"));

            let path = line
                .split('"')
                .nth(1)
                .unwrap_or_else(|| panic!("{directive} names no quoted path: {line}"));

            assert_eq!(
                path.rsplit_once('/').map(|(parent, _)| parent),
                Some(data.as_str()),
                "{directive} is not a child of the data directory, which is the one that exists"
            );
        }
    }

    /// The status endpoint is one value read by four things — the file, the readiness check, the
    /// health probe and what a failed start is diagnosed against — so an override that moved it and
    /// left one behind would be a service that starts and is never reported up.
    #[test]
    fn an_override_moves_the_status_endpoint_everywhere_it_is_named() {
        let moved = r#"{"status_port": 2121}"#;
        let rendered = conf(moved);

        assert!(rendered.contains("listen 127.0.0.1:2121;"), "{rendered}");

        let spec = spec(moved);

        assert!(
            matches!(spec.ready(), ReadyCheck::Http { url, .. } if url.contains("127.0.0.1:2121")),
            "{:?}",
            spec.ready()
        );
        assert!(matches!(
            spec.health().map(|health| &health.probe),
            Some(HealthProbe::Http { url, .. }) if url.contains("127.0.0.1:2121")
        ));
        assert_eq!(spec.ports(), [2121]);
    }

    /// A reload is `-s reload` and a stop is `-s quit`, both through the same configuration the
    /// server was started with — which is how either one finds the pid file this instance wrote.
    #[test]
    fn a_reload_and_a_stop_are_sent_through_this_instances_own_configuration() {
        let spec = spec("{}");
        let config = context("{}").config(CONFIG_FILE).display().to_string();

        let Some(ReloadBehaviour::Command { args, .. }) = spec.reload() else {
            panic!("a reload that is not a command: {:?}", spec.reload());
        };
        assert!(args.contains(&"reload".to_owned()), "{args:?}");
        assert!(args.contains(&config), "{args:?}");

        let StopBehaviour::Command { args, .. } = spec.stop() else {
            panic!("a stop that is not a command: {:?}", spec.stop());
        };
        assert!(args.contains(&"quit".to_owned()), "{args:?}");
        assert!(args.contains(&config), "{args:?}");
    }

    /// **Nothing is served on the row's own port yet**, and that is T43's to add rather than a gap.
    ///
    /// A front end that bound 80 here would need the port grant T42 has not built on macOS and
    /// Linux, and would be serving nothing on it. Caddy says the same thing by writing `http_port`
    /// into a global block and binding nothing until a site asks it to.
    #[test]
    fn the_rendering_listens_on_nothing_a_site_would_be_reached_on() {
        let rendered = conf("{}");

        assert!(
            !rendered.contains("listen 80"),
            "a front end with no sites is listening on the port sites are served on: {rendered}"
        );
    }

    /// A whole number is what the merge guarantees and a port is what the recipe needs.
    #[test]
    fn a_number_that_is_not_a_port_is_refused_against_the_setting_that_holds_it() {
        for offered in ["70000", "0", "-1"] {
            let error = Nginx
                .spec(&context(&format!(r#"{{"status_port": {offered}}}"#)))
                .expect_err("a number that is not a port");

            let message = error.to_string();
            assert!(message.contains("status_port"), "{message}");
            assert!(message.contains(offered), "{message}");
        }
    }
}
