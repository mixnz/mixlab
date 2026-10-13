//! Caddy: the default front end — roadmap task **T31**.
//!
//! The first real [`Recipe`], and the shape the four after it are meant to follow. What it renders
//! is one file, and what makes that file interesting is the three mechanisms hanging off it:
//!
//! - **`caddy validate` judges it before it is installed.** A [`Validator`] is all that takes, and
//!   the staging directory T30 built is what makes it honest — the whole rendering is checked where
//!   it is staged, `import sites/*.caddy` and all, so a configuration that is refused is one nothing
//!   was installed from and the server goes on reading the last one that worked.
//! - **The admin endpoint is both the readiness check and the health check.** `GET /config/` on it
//!   answers `200` with the running configuration, which is a stronger statement than a TCP accept:
//!   a Caddy whose listener is up and whose config failed to apply is not a Caddy anything should be
//!   routed to.
//! - **A changed rendering is reloaded rather than restarted**, through
//!   [`ReloadBehaviour::Command`] and that same endpoint. This is the service the whole idea is for:
//!   every site on the machine is reached through this process, and dropping every connection
//!   because one of them was edited is a cost nobody asked for.
//!
//! # Judged against the real server
//!
//! Every one of those was measured against Caddy 2.11.4 rather than read about, which is what
//! `docs/roadmap/phase-3-services.md` means by a recipe being judged against the real server.
//! Two of the findings are in the template beside the lines they explain — backtick-quoted paths,
//! `persist_config off`. The third is here because it is about the *spec* and not the file:
//! **`caddy run`, not `caddy start`.** `start` spawns a child, hands it the parent's stdout and
//! returns, so anything capturing that output waits for the server rather than for the launcher.
//! `run` is the process that serves, which is the only kind of process a supervisor can supervise.
//!
//! **The fourth was found by a rotation and is `--force` on the reload.** Caddy adapts the Caddyfile
//! to JSON and the adapter throws comments away — so the line `site.caddy` carries a certificate
//! fingerprint on, which is the only thing a reissue changes, is invisible to the server. The
//! configuration it is handed is identical to the one it is running, its admin endpoint skips the
//! load, and it goes on presenting the certificate it holds in memory. See the test that names the
//! flag.
//!
//! # It renders every site, and into its own set
//!
//! Roadmap task **T43**. `sites/*.caddy` is imported by the Caddyfile above and rendered by
//! [`Recipe::sites`] below, into the *same* `Vec<Document>` — never by a second path that runs
//! afterwards. The import resolves against the directory holding the file it is written in, which is
//! the staging directory while `caddy validate` is looking and `etc/caddy/` afterwards, so a site
//! file written anywhere else would be invisible to the checker and present at run time: the one
//! arrangement whose correctness cannot be checked. T31 put the import here rather than in Phase 4
//! for exactly that reason, and this is the half that was waiting.
//!
//! [`swept`](Recipe::swept) is the other half. A site that stops being declared has to lose its file
//! on the same walk, or it goes on being served — so `sites/` holds exactly what was rendered into
//! it and nothing else.
//!
//! # What this recipe deliberately does not do
//!
//! **It issues no certificate.** `auto_https` is `off`, which is what a machine with no CA and no
//! sites should say: on, Caddy would try to obtain a public certificate for a name it will never be
//! reachable at. Phase 5 owns the answer — MixEngine's own CA in the OS trust store — and it is a
//! setting rather than a constant so that the day it changes is a default moving, not a template
//! being edited.
//!
//! **And it installs no certificate authority**, which is a stronger statement and takes two more
//! lines of the template: `skip_install_trust`, and a `pki` block naming the local CA for it to
//! apply to. `auto_https off` stops Caddy *obtaining* certificates and does nothing about its own
//! local CA, whose root Caddy installs into the user's trust store the first time it provisions one.
//! Found by T76's port-scan suite: five `Caddy Local Authority` roots in `CurrentUser\Root` on a
//! development machine, none of them asked for, and a CI runner that hung for the whole readiness
//! budget inside that install. MixEngine reaches a trust store exactly once, through
//! `mixengine-elevate`, for its own authority and with the user's consent — a front end doing it by
//! default is that design undone.
//!
//! **The option on its own was measured to do nothing**, which is why the block is there. The
//! Caddyfile adapter applies `skip_install_trust` to the certificate authorities it emits and emits
//! none unless asked, so on 2.11.4 the option alone adapts to a configuration with no `pki` app in
//! it, and the CA provisioned at run time is the implicit one with the installing default — the
//! first fix for this shipped exactly that and the runner hung again. `caddy adapt` is what tells
//! the two apart, and `mixengine-cli`'s `caddy.rs` asks it against the real program.
//!
//! [`Recipe`]: crate::generate::Recipe

use mixengine_proto::{
    HealthCheck, HealthProbe, Millis, ReadyCheck, ReloadBehaviour, ServiceSpec, ServiceSpecBuilder,
    StopBehaviour,
};

use crate::generate::document::{CONFIG, Document, Validator};
use crate::generate::recipe::{Context, Instancing, Recipe, Role, TemplateFile, Upstream};
use crate::generate::served::{Served, ServedKind};
use crate::generate::settings::{Preset, Setting};
use crate::install::SmokeTest;
use crate::{Error, Result};

/// The `packages.name` this recipe is for, which is also the name of the binary inside the package.
///
/// One value and not two: `mixengine-packages` publishes Caddy as a single executable at the root of
/// the archive, and [`Context::program`] is what spells it `caddy.exe` on Windows.
const PACKAGE: &str = "caddy";

/// The rendered configuration, under `etc/<service-id>/`.
const CADDYFILE: &str = "Caddyfile";

/// One rendered site, under `etc/<service-id>/sites/`.
const SITE: &str = include_str!("caddy/site.caddy");

/// The directory the sites go in, which is also the one this recipe sweeps.
///
/// The name is in the Caddyfile's `import sites/*.caddy` as well, and the two have to agree — but
/// they are a template and a directory listing rather than two constants, so this is the one place
/// in Rust that spells it.
const SITES: &str = "sites";

/// The directory an installed extension's `[[recipe.front_end]]` goes in — roadmap task **T81c**.
///
/// Swept for [`SITES`]' reason: an uninstalled extension's fragment has to leave with it, and the
/// pass that removes what the recipe stopped rendering is the one that does it. The name is in the
/// Caddyfile's `import extensions/*.caddy` as well.
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
/// stay about the authority; a second feature moving in weakens it to nothing, and the next person
/// reading it cannot tell which file the rule was for.
const WELCOME_DIR: &str = "welcome";

/// Where the admin endpoint listens. Loopback always — see the template.
const ADMIN_HOST: &str = "127.0.0.1";

/// The addresses every site answers on before anything is shared — roadmap task **T74**.
///
/// **Written on every site block, and that is the fix rather than the decoration.** Caddy's own
/// default is every interface, so before T74 a site was already reachable from the network on any
/// machine whose firewall allowed the port — it simply answered nothing, because no site block
/// matched an address-shaped `Host`. T74 opens that port deliberately, which turns "answers
/// nothing" into a promise this code has to keep: without these two lines a site nobody shared is
/// listening on the LAN, and the only thing between it and a stranger is which `Host` header they
/// send.
///
/// Both loopback families, because `blog.test` resolves to 127.0.0.1 through the hosts file while
/// `localhost` resolves to ::1 first on Windows — binding one would break whichever the developer
/// typed.
const LOOPBACK: [&str; 2] = ["127.0.0.1", "::1"];

/// The port the admin endpoint listens on. Caddy's own default, so a `caddy` command typed by hand
/// with no `--address` reaches the server MixEngine is running.
const ADMIN_PORT: &str = "admin_port";

/// The port a site written without one is served on over TLS. The row's own `port` is the other
/// half of the pair.
const HTTPS_PORT: &str = "https_port";

/// Caddy's `auto_https` global option, verbatim: `off`, `disable_redirects`, `disable_certs`,
/// `ignore_loaded_certs`.
///
/// Free text rather than a flag, and it costs nothing to be wrong about: an override Caddy does not
/// recognise is refused by `caddy validate` in that program's own words, before anything is
/// installed. A closed list here would be a second copy of Caddy's vocabulary to keep in step with
/// it across releases.
const AUTO_HTTPS: &str = "auto_https";

/// The level Caddy logs at, in its own spelling: `DEBUG`, `INFO`, `WARN`, `ERROR`.
const LOG_LEVEL: &str = "log_level";

/// How long the admin endpoint is given to answer before the start is a failure, in milliseconds.
const READY_TIMEOUT: &str = "ready_timeout_ms";

/// How long `caddy stop` is given before the process group is killed, in milliseconds.
const STOP_GRACE: &str = "stop_grace_ms";

/// How often the admin endpoint is asked whether the server is still there.
const HEALTH_INTERVAL: Millis = Millis(10_000);

/// How long one of those may take. Well inside the interval, which [`ServiceSpec::validate`]
/// insists on: two probes that could overlap are two probes that can queue.
const HEALTH_TIMEOUT: Millis = Millis(2_000);

/// How long a reload is waited for.
///
/// Generous because of what it covers: `caddy reload` adapts the whole Caddyfile, sends it to the
/// running server and waits for the new configuration to be *provisioned*, which on a machine with
/// forty sites is real work. Nothing is killed when it expires — see [`ReloadBehaviour::Command`].
const RELOAD_PATIENCE: Millis = Millis(30_000);

/// Caddy, as MixEngine runs it.
#[derive(Debug)]
pub struct Caddy;

impl Recipe for Caddy {
    fn package(&self) -> &'static str {
        PACKAGE
    }

    /// There is one Caddy.
    ///
    /// `caddy@main` would be a distinction without a difference, and a second one is two processes
    /// contending for port 80 — which is not a configuration anybody meant to ask for.
    fn instancing(&self) -> Instancing {
        Instancing::Single
    }

    /// And it is one of the two programs a site is reached through — roadmap task **T37**.
    ///
    /// What `docs/features/services.md` calls "exactly one active front end" needs both halves:
    /// this one is about the *job*, and it is what [`instancing`](Self::instancing) cannot say —
    /// one Caddy and one nginx are two rows that each obey their own recipe and still leave a home
    /// with two front ends. [`nginx`](super::nginx) is the other side of it.
    fn role(&self) -> Role {
        Role::FrontEnd(mixengine_proto::FrontEndServer::Caddy)
    }

    fn smoke_test(&self) -> Option<SmokeTest> {
        Some(SmokeTest {
            executable: PACKAGE.to_owned(),
            // A subcommand and not a flag: `caddy --version` exits non-zero, which would fail the
            // install of an archive that is perfectly good.
            args: vec!["version".to_owned()],
            unset: &[],
        })
    }

    fn settings(&self) -> &'static [Setting] {
        &[
            Setting {
                key: ADMIN_PORT,
                default: Preset::Number(2019),
            },
            Setting {
                key: AUTO_HTTPS,
                default: Preset::Text("off"),
            },
            Setting {
                key: HTTPS_PORT,
                default: Preset::Number(443),
            },
            Setting {
                key: LOG_LEVEL,
                default: Preset::Text("INFO"),
            },
            Setting {
                // Thirty seconds. Caddy itself is up in tens of milliseconds; what this is really
                // waiting for is a first run on Windows, where the binary is fifty megabytes and
                // Defender reads all of it before the process starts.
                key: READY_TIMEOUT,
                default: Preset::Number(30_000),
            },
            Setting {
                key: STOP_GRACE,
                default: Preset::Number(10_000),
            },
        ]
    }

    fn files(&self) -> &'static [TemplateFile] {
        &[TemplateFile {
            path: CADDYFILE,
            source: include_str!("caddy/Caddyfile"),
        }]
    }

    /// Exactly `sites/`, and only because this recipe is a front end — D4.
    ///
    /// Without it a deleted site keeps the file it had and goes on being served, which is what T31
    /// left open in as many words. Nothing sweeps `etc/caddy/` itself: a directory belonging to a
    /// *deleted service* is `service.delete`'s problem.
    fn swept(&self) -> &'static [&'static str] {
        &[SITES, EXTENSIONS, WELCOME_DIR]
    }

    /// One file per site, named after its primary domain — D12.
    ///
    /// Rendered into the set the Caddyfile's own `import sites/*.caddy` picks up, which is what makes
    /// the whole rendering judged by `caddy validate` where it is staged.
    fn sites(&self, context: &Context, served: &[Served]) -> Result<Vec<Document>> {
        let mut documents = served
            .iter()
            .map(|site| {
                let rendering = SiteRendering {
                    primary: site.primary(),
                    domains: &site.domains,
                    doc_root: &site.doc_root,
                    kind: kind(&site.kind),
                    upstream: upstream(&site.kind),
                    activator: activator(&site.kind),
                    rewrite: site_rewrite(&site.kind).cloned(),
                    routes: site
                        .routes
                        .iter()
                        .enumerate()
                        .map(|(position, route)| RouteRendering::new(position, route))
                        .collect(),
                    certificate: site.certificate.as_ref().map(Certificate::from),
                    https_redirect: site.https_redirect,
                    bind: bound(site.shared.as_ref().map(|shared| shared.address)),
                    lan: site
                        .shared
                        .as_ref()
                        .map(|shared| shared.address.to_string()),
                    mdns: site.shared.as_ref().and_then(|shared| shared.name.clone()),
                    // **Only for a shared site, which is where "served only while sharing is on"
                    // is actually enforced** - roadmap task T75. The rendered copy's directory,
                    // absolute, and [`None`] on a home that has no authority to serve.
                    authority: site
                        .shared
                        .as_ref()
                        .and(context.authority())
                        .map(|_| context.config(AUTHORITY_DIR).to_string_lossy().into_owned()),

                    // **Every site gets one** — roadmap task T124. Unlike `authority` above this is
                    // never conditional on the site: what a page is *for* is a site nobody has put
                    // anything into yet, which is every site at the moment it is made.
                    welcome: context
                        .welcome()
                        .then(|| context.config(WELCOME_DIR).to_string_lossy().into_owned()),
                };

                let contents = crate::generate::served::render(
                    SITE,
                    "caddy/site.caddy",
                    context.service(),
                    &rendering,
                )?;

                Ok(Document::new(
                    format!("{SITES}/{}.caddy", site.primary()),
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

    /// One file per extension, in the set `import extensions/*.caddy` picks up — roadmap task
    /// **T81c**.
    ///
    /// The fragment arrives rendered: its placeholders were substituted where the extension's row
    /// is, because `{install_dir}` means the directory *that* extension was installed into. What is
    /// added here is the header saying where the file came from, which is the same sentence every
    /// other generated file in this home opens with.
    fn fragments(&self, context: &Context) -> Result<Vec<Document>> {
        Ok(context
            .fragments()
            .iter()
            .map(|addition| {
                Document::new(
                    format!("{EXTENSIONS}/{}.caddy", addition.extension),
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

    /// `caddy validate`, pointed at the staged `Caddyfile`.
    ///
    /// The binary inside the package this instance was installed from, and not whichever `caddy` is
    /// on the `PATH`: a configuration is judged by the version that will read it, or the check is
    /// about a different program.
    fn validator(&self, context: &Context) -> Option<Validator> {
        Some(Validator::new(context.program(PACKAGE), CADDYFILE).args([
            "validate",
            "--adapter",
            "caddyfile",
            "--config",
            CONFIG,
        ]))
    }

    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder> {
        let settings = context.settings();

        let caddy = context.program(PACKAGE);
        let config = context.config(CADDYFILE).to_string_lossy().into_owned();
        let admin_port = port(context, ADMIN_PORT)?;
        let address = format!("{ADMIN_HOST}:{admin_port}");

        // Every one of these is what a person would type, which is the point of the admin endpoint
        // being on Caddy's own default port: the commands in this file are the commands in Caddy's
        // documentation, with `--address` added so that two instances cannot answer for each other.
        let admin = format!("http://{address}/config/");

        Ok(ServiceSpec::builder(context.service().clone(), &caddy)
            .args(["run", "--config", &config, "--adapter", "caddyfile"])
            // What a failed start is diagnosed against (T38), and it is the admin endpoint alone:
            // `http_port` and `https_port` are in the global block, but Caddy binds neither until a
            // site asks it to — and sites arrive with T43.
            .ports([admin_port])
            // The configuration directory, and not the data directory: `import sites/*.caddy`
            // resolves against the file rather than the process, but a relative path inside a site
            // — a document root somebody wrote by hand — resolves against this.
            .cwd(context.etc())
            .ready(ReadyCheck::Http {
                url: admin.clone(),
                expect_status: 200,
                timeout: millis(settings.number(READY_TIMEOUT)),
            })
            .health(HealthCheck {
                probe: HealthProbe::Http {
                    url: admin,
                    expect_status: 200,
                },
                interval: HEALTH_INTERVAL,
                timeout: HEALTH_TIMEOUT,
                // Three intervals rather than one: a reload provisions the new configuration before
                // it swaps it in, and a machine with many sites can miss a probe doing it. That is a
                // busy web server, not a sick one.
                failures_before_degraded: 3,
                successes_before_running: 1,
            })
            .reload(ReloadBehaviour::Command {
                program: caddy.clone(),
                args: vec![
                    "reload".to_owned(),
                    "--config".to_owned(),
                    config,
                    "--adapter".to_owned(),
                    "caddyfile".to_owned(),
                    "--address".to_owned(),
                    address.clone(),
                    // **The certificate line this reload exists for is a comment**, and the
                    // Caddyfile adapter removes comments — so the JSON Caddy is handed after a
                    // reissue is identical to the one it is running, and without this it answers by
                    // skipping the load and goes on serving the certificate it holds in memory. See
                    // the test that names this flag.
                    "--force".to_owned(),
                ],
                patience: RELOAD_PATIENCE,
            })
            // Through the admin endpoint rather than by signal, and the same on all three systems.
            // A signal would work on Unix and be a console control event on Windows; this is one
            // mechanism, it is the one Caddy documents, and it is the one T30a proved from a moved
            // directory on all six targets.
            .stop(StopBehaviour::Command {
                program: caddy,
                args: vec!["stop".to_owned(), "--address".to_owned(), address],
                grace: millis(settings.number(STOP_GRACE)),
            }))
    }
}

/// One site, as `caddy/site.caddy` reads it.
#[derive(Debug, serde::Serialize)]
struct SiteRendering<'a> {
    primary: &'a str,
    domains: &'a [String],
    doc_root: &'a std::path::Path,
    kind: &'static str,

    /// Empty for a static site, whose branch does not read it — but `Strict` undefined behaviour
    /// means the key has to be there whichever branch is taken.
    upstream: String,

    /// The activator to fall back to, or [`None`] for a pool nothing can wake — T70.
    ///
    /// Present whichever branch is taken, for `upstream`'s reason: `Strict` makes a missing key an
    /// error, and `None` serialises to `null`, which `{% if %}` reads as false.
    activator: Option<String>,

    /// What `/` becomes on the way out, for a proxy site whose upstream carried a path — roadmap
    /// task **T135**. [`None`] renders exactly what this file rendered before T135.
    rewrite: Option<crate::generate::served::Rewrite>,

    /// What answers before the kind does, longest prefix first — roadmap task **T135**.
    routes: Vec<RouteRendering>,

    /// [`None`] renders no TLS block at all — the T51 design, D4.
    ///
    /// **The key is always present**, for `upstream`'s reason: `UndefinedBehavior::Strict` makes a
    /// missing key an error rather than a falsy value. `None` serialises to `null`, which `{% if %}`
    /// reads as false.
    certificate: Option<Certificate>,

    /// Whether the plaintext block redirects rather than serves — roadmap task **T98**.
    ///
    /// **Read beside `certificate` in the template, never alone.** A site can carry this as `true`
    /// with no usable certificate — the same gap [`certificate`](Self::certificate) already has a
    /// name for — and redirecting to a TLS listener nothing is bound to would be worse than the
    /// plaintext page it has always been able to serve.
    https_redirect: bool,

    /// Every address this site's listeners bind, loopback first — roadmap task **T74**.
    ///
    /// **Never empty.** A site that is not shared binds the two loopback addresses and nothing
    /// else; a shared one adds its interface address. Caddy's `bind` *replaces* the default rather
    /// than adding to it, which is what makes both halves work: loopback has to be named or a
    /// shared site would go down in the browser on this machine, and the LAN address has to be
    /// absent or an unshared site would be listening on the network.
    bind: Vec<String>,

    /// The address a shared site also answers to by name, or [`None`] — roadmap task **T74**.
    ///
    /// **A site block matches on `Host`, and a phone sends the address it was given.** Binding the
    /// interface makes the connection arrive; without this line it arrives and matches no site, and
    /// Caddy answers 200 with an empty body — which is exactly what the first phone to try this saw.
    lan: Option<String>,

    /// The mDNS name a shared site also answers to, or [`None`] — roadmap task **T75**.
    ///
    /// **The same lesson as [`lan`](Self::lan), with a name.** The daemon advertises the name and
    /// this line is what makes the site *reply* to it; a name that resolves to a block which does
    /// not match it is the blank page T74 spent a phone finding.
    mdns: Option<String>,

    /// The directory this home's public authority was rendered into, or [`None`] — roadmap task
    /// **T75**.
    ///
    /// **[`None`] for a site that is not shared**, which is how "served only while sharing is on"
    /// becomes a property of the rendering rather than a promise made about it. It is also [`None`]
    /// on a home with no authority to serve.
    authority: Option<String>,

    /// The directory this site's welcome page was rendered into, absolute — roadmap task **T124**.
    ///
    /// **[`None`] on a home that turned the page off** — the T124 design, D6. The key is always
    /// present, for [`upstream`](Self::upstream)'s reason: `Strict` makes a missing key an error
    /// rather than a falsy value, and `None` serialises to `null`, which `{% if %}` reads as false.
    welcome: Option<String>,
}

/// What this site's listeners bind: loopback always, and the interface address when shared.
fn bound(shared: Option<std::net::Ipv4Addr>) -> Vec<String> {
    let mut bound: Vec<String> = LOOPBACK
        .iter()
        .map(|address| (*address).to_owned())
        .collect();
    bound.extend(shared.map(|address| address.to_string()));

    bound
}

/// A certificate as the template writes it — roadmap task **T51**.
///
/// **Strings and not `Path`s**, because a template writes text: `Path`'s `Serialize` is lossy on a
/// path that is not UTF-8, and `display()` is what the rest of this module already writes.
#[derive(Debug, serde::Serialize)]
struct Certificate {
    certificate: String,
    key: String,
    fingerprint: String,
}

impl From<&crate::generate::served::SiteCertificate> for Certificate {
    fn from(certificate: &crate::generate::served::SiteCertificate) -> Self {
        Self {
            certificate: certificate.certificate.display().to_string(),
            key: certificate.key.display().to_string(),
            fingerprint: certificate.fingerprint.clone(),
        }
    }
}

/// Which branch of the template this kind takes.
///
/// Three and not four: a `node-app` renders as a reverse proxy to loopback and that is all it is —
/// D7. Nothing in this build starts a node process, and a fourth branch would be a difference the
/// rendering does not have.
const fn kind(kind: &ServedKind) -> &'static str {
    match kind {
        ServedKind::PhpFpm { .. } => "php-fpm",
        ServedKind::Static => "static",
        ServedKind::ReverseProxy { .. } | ServedKind::NodeApp { .. } => "proxy",
    }
}

/// The address this kind is proxied or passed to, as **Caddy** spells one.
///
/// `unix/` and then the path, which is Caddy's own spelling of a socket and is not nginx's — the
/// same value renders as `unix:/run/…` there. That difference is the reason
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
/// nothing more — which is what this file rendered before T135.
fn site_rewrite(kind: &ServedKind) -> Option<&crate::generate::served::Rewrite> {
    match kind {
        ServedKind::ReverseProxy { rewrite, .. } => rewrite.as_ref(),
        _ => None,
    }
}

/// One route, as `caddy/site.caddy` reads it.
///
/// **Every key present whichever branch the template takes**, for [`SiteRendering::upstream`]'s
/// reason: `UndefinedBehavior::Strict` makes a missing one an error rather than a falsy value.
#[derive(Debug, serde::Serialize)]
struct RouteRendering {
    /// The prefix, as the row holds it.
    path: String,

    /// The name of this route's matcher, unique within the block.
    ///
    /// Derived from the position rather than from the path: a matcher name is a Caddyfile token,
    /// and a path is a string a person typed.
    matcher: String,

    /// Which branch of the macro this target takes.
    kind: &'static str,

    /// Where a proxy route forwards — scheme, host and port, never a path.
    address: String,

    /// What the prefix becomes on the way out, or [`None`] to pass the path through.
    rewrite: Option<crate::generate::served::Rewrite>,

    /// Where a static route's files are, absolute.
    root: String,

    /// Where a php-fpm route's pool listens, in Caddy's spelling.
    upstream: String,

    /// The activator to fall back to — roadmap task **T70**, the site's own rule.
    activator: Option<String>,
}

impl RouteRendering {
    /// One [`ServedRoute`](crate::generate::served::ServedRoute) in Caddy's spelling.
    fn new(position: usize, route: &crate::generate::served::ServedRoute) -> Self {
        use crate::generate::served::ServedRouteTarget as Target;

        let mut rendering = Self {
            path: route.path.clone(),
            matcher: format!("mixengine_route_{position}"),
            kind: "proxy",
            address: String::new(),
            rewrite: None,
            root: String::new(),
            upstream: String::new(),
            activator: None,
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
                rendering.root = root.display().to_string();
            }
        }

        rendering
    }
}

/// The activator's address for this kind, as Caddy spells one — roadmap task **T70**.
///
/// [`None`] for everything nothing can start by connecting to it, which renders the site exactly as
/// it rendered before T70.
fn activator(kind: &ServedKind) -> Option<String> {
    match kind {
        ServedKind::PhpFpm { activator, .. } => activator.as_ref().map(address),
        _ => None,
    }
}

/// One [`Upstream`] in Caddy's spelling.
///
/// **A socket is in backticks, like every other path this recipe writes.** The pool's socket lives
/// under the home, and on macOS the default home is `~/Library/Application Support/MixEngine` —
/// with a space, which the Caddyfile lexer reads as the end of one token and the start of the next.
/// Measured: `php_fastcgi unix//…/Application Support/…/php-fpm-8.3.33.sock` became two upstreams,
/// the second of them `Support/…`, and every PHP request was a 502 with *dial support: unknown
/// network support* in Caddy's log while `index.html` beside it served fine. A TCP address has no
/// space to protect and is left bare.
fn address(upstream: &Upstream) -> String {
    match upstream {
        Upstream::Socket(path) => format!("`unix/{}`", path.display()),
        Upstream::Tcp(address) => address.to_string(),
    }
}

/// One of this recipe's port settings, as a port.
///
/// A whole number is what the merge guarantees, and a *port* is what this recipe needs — so 70000
/// is refused here, by name, rather than reaching the supervisor as a URL that cannot be parsed and
/// being reported as a service that never came up.
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
///
/// Zero and below are refused by [`ServiceSpec::validate`] rather than here: a timeout of zero is a
/// statement about a spec, it is checked in one place for every recipe, and the message it produces
/// names the field.
fn millis(number: i64) -> Millis {
    Millis(u64::try_from(number).unwrap_or_default())
}

#[cfg(test)]
mod tests;
