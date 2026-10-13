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
mod tests;
