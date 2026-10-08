//! What this build knows about running one kind of service.
//!
//! A `services` row says *that* MariaDB 11.4 is installed as `mariadb@main` on port 3306 with these
//! overrides. It cannot say what MariaDB **is**: which binary starts it, which file it reads, how to
//! tell whether it is up, what stops it cleanly. That knowledge is a [`Recipe`], and it is compiled
//! into this build rather than carried in the package index — the index describes a *download*
//! (`provides`, `requires`, a checksum), and a template that has to change with a MixEngine release
//! cannot be published by a pipeline that runs on a different schedule.
//!
//! **A recipe is looked up by `packages.name`**, which is the one thing a row and a recipe agree on.
//! `mariadb@main` and `mariadb@legacy` are two rows, two data directories and two ports; they are
//! one recipe, and the difference between them is entirely in the [`Context`] it is handed.
//!
//! **The set this build ships is [`Catalogue::builtin`], and each entry in it is a roadmap task of
//! its own** — Caddy is T31 and is in ([`recipes::caddy`](super::recipes::caddy)), php-fpm is T32,
//! MariaDB T33, PostgreSQL T34, Redis and Memcached T35 — because each is a template, a set of
//! overrides worth having and a first-start ritual, judged against the real server. What T30 owns is
//! everything around them: the merge, the render, the diff, the staging and the [`ServiceSpec`] that
//! comes out. A catalogue is a value, so a test — and a debug build with a fixture to supervise —
//! composes its own.
//!
//! [`ServiceSpec`]: mixengine_proto::ServiceSpec

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mixengine_platform::PortBinding;
use mixengine_proto::{ServiceId, ServiceSpecBuilder};
use serde::Serialize;

use super::document::{Document, Validator};
use super::served::Served;
use super::settings::{Setting, Settings};
use crate::{Error, Result};

/// One file a recipe renders, and where it goes under `etc/<service-id>/`.
///
/// The source is a `&'static str` — `include_str!` for anything longer than a few lines — because a
/// template is part of the build and not part of the home: a user who could edit one would be
/// editing generated configuration by a slower route, and the file would then be a second place for
/// the truth to live. What they edit is an override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateFile {
    /// Where the rendering goes, relative to the service's configuration directory.
    pub path: &'static str,

    /// The template itself, in Jinja syntax.
    pub source: &'static str,
}

/// One extension's `[[recipe.front_end]]`, rendered — roadmap task **T81c**.
///
/// **The substitution has already happened**, and it happened where the extension's row is, because
/// `{install_dir}` means the directory *that extension* was installed into and only its row knows
/// where that is. What reaches a front-end recipe is text and the name to file it under, which is
/// the same shape [`IniAddition`](crate::runtimes::extensions::IniAddition) hands to a PHP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontEndAddition {
    /// Which extension asked, which is also what its file is named after.
    pub extension: String,

    /// The directives, with their placeholders substituted and their paths spelled the way this
    /// front end spells one.
    pub fragment: String,
}

/// Everything a template and a [`Recipe`] are told about one service instance.
///
/// Assembled by [`Generator`](super::Generator) from the `services` row, the `packages` row it
/// points at and the home's layout, so that neither a template nor a recipe reads the database or
/// joins a path of its own.
///
/// The fields are `pub(super)` and there is no constructor: the only thing that may build one is
/// [`Generator`](super::Generator), which is the only thing that has read the row. A recipe reads it
/// through the accessors below.
#[derive(Debug, Clone)]
pub struct Context {
    /// Which service this is.
    pub(super) service: ServiceId,

    /// Which home this is — roadmap task **T126**.
    ///
    /// Read once per walk by [`Generator`](super::Generator), like
    /// [`bindings`](Self::bindings): a recipe is a function of its context, and one that went
    /// looking in `settings` would be a second place this home's identity is decided. Its one
    /// reader is [`Context::secret_address`], which is every credential address this build
    /// composes.
    pub(super) home: crate::home::HomeId,

    /// `packages.name` — the name this context's recipe was found under.
    pub(super) package: String,

    /// `packages.version`, as upstream writes it.
    pub(super) version: String,

    /// Where the package is unpacked: `packages/<name>/<version>/`.
    pub(super) install_path: PathBuf,

    /// What that install calls its executables, and where each one is inside the directory.
    ///
    /// `runtime_installs.provides_json`, and **empty for a service that came from a `packages`
    /// row** — see [`Context::provided`], which is the only thing that reads it.
    pub(super) provides: BTreeMap<String, String>,

    /// `etc/<service-id>/`, where everything rendered goes.
    pub(super) etc: PathBuf,

    /// `etc/` itself, for the one thing a recipe needs that is not its own directory.
    ///
    /// A pool has to name the ini set of the *runtime* it runs, which is generated per version and
    /// is not a file this recipe renders — see [`crate::runtimes::extensions`].
    pub(super) etc_root: PathBuf,

    /// This instance's data directory, which is the user's and is never regenerated.
    pub(super) data: PathBuf,

    /// `run/`, for a socket or a pid file.
    pub(super) run: PathBuf,

    /// `logs/services/<service-id>/`.
    pub(super) logs: PathBuf,

    /// The port from the row, or [`None`] for a service that listens on a socket.
    pub(super) port: Option<u16>,

    /// The port the *activator* listens on for this service — roadmap task **T70**.
    ///
    /// [`None`] for a service that listens on a socket, whose activator derives its address from
    /// the service's own, and for a row written before the column existed.
    pub(super) activation_port: Option<u16>,

    /// The address it binds, `127.0.0.1` unless the row says otherwise.
    pub(super) bind: String,

    /// The recipe's defaults with the user's overrides applied.
    pub(super) settings: Settings,

    /// The paths this recipe computes that its own template also has to name.
    ///
    /// Filled by [`Generator`](super::Generator) from [`Recipe::endpoints`] before anything is
    /// rendered, so the file and the check the daemon makes read one value. See [`Endpoints`].
    pub(super) endpoints: Endpoints,

    /// What this system makes a program bind to answer on each port a front end serves.
    ///
    /// **Data and not a `#[cfg]`**, which is the whole reason it arrives here: on macOS a front end
    /// binds 8080 to answer on 80, and `mixengine-core` may not know what system it is on. Filled by
    /// [`Generator`](super::Generator) from what the platform layer says, and read by a template
    /// through the `bound` filter.
    pub(super) bindings: Vec<PortBinding>,

    /// This home's certificate authority, as the public PEM — roadmap task **T75**.
    ///
    /// **Filled by [`Generator`](super::Generator), like [`bindings`](Self::bindings)**, and for the
    /// same reason: a recipe may not go looking for it. A front end renders it into a directory of
    /// its own so that a phone can install it and trust a shared site's certificate — the T75
    /// design, D9.
    ///
    /// **The bytes and not a path**, which is the whole of that decision. `certs/ca/root.key` sits
    /// beside `certs/ca/root.crt`, so a front end pointed at the certificates directory would serve
    /// this home's signing key to the local network. Rendering a copy means the directory a front
    /// end is pointed at holds exactly one file, and that is true by construction rather than by
    /// two directives agreeing.
    ///
    /// [`None`] on a home whose authority has not been generated.
    pub(super) authority: Option<String>,

    /// Whether a site with nothing behind it answers with MixEngine's page — roadmap task **T124**.
    ///
    /// **Filled by [`Generator`](super::Generator), like [`authority`](Self::authority)**, and for
    /// the same reason: a recipe is a function of its context, and one that went reading
    /// `config.toml` would be a second place this home's preferences are decided.
    ///
    /// `config.sites.welcome_page`, and therefore `true` unless somebody turned it off.
    pub(super) welcome: bool,

    /// What the extensions installed here add to this front end's configuration — roadmap task
    /// **T81c**.
    ///
    /// **Filled by [`Generator`](super::Generator), like [`bindings`](Self::bindings) and
    /// [`authority`](Self::authority)**, and for the same reason: a recipe is a function of its
    /// context, and one that went reading the `extensions` table would be a second place the
    /// placeholders are substituted.
    ///
    /// **Empty on every service that is not this home's front end**, and already filtered to the
    /// fragments written for *this* front end — see [`Recipe::fragments`]. A Caddyfile fragment is
    /// not something the nginx recipe should have to know it must skip.
    pub(super) fragments: Vec<FrontEndAddition>,

    /// The credentials this service's first-run ritual was given, by the key its recipe declared.
    ///
    /// **Empty everywhere except inside [`FirstRun::steps`]**, and never part of
    /// [`Context::rendering`]: a `my.cnf` with a root password in it would be a plaintext credential
    /// on disk, written by the very design that refuses one. There is a test.
    ///
    /// [`FirstRun::steps`]: super::first_run::FirstRun::steps
    pub(super) secrets: BTreeMap<String, String>,

    /// The credential this service's processes are handed at spawn, when it has one — roadmap task
    /// **T82a**, that design's D4.
    ///
    /// **Filled by [`Generator`](super::Generator), like [`bindings`](Self::bindings) and
    /// [`fragments`](Self::fragments)**, and for the same reason: what it takes to answer is three
    /// tables — the extension sites, the database each is linked to, and what that database's recipe
    /// calls its administrator — and a recipe that went reading them would be a second place this
    /// home's shape is decided.
    ///
    /// **An address and never a value**, which is what separates this from
    /// [`secrets`](Self::secrets) above: that map holds what a first run generated and is why it
    /// never reaches a rendering, while this holds only what the supervisor will look up. It is
    /// [`None`] on every service but the php-fpm pool of a `web-app` extension that declared
    /// `signs_in`.
    pub(super) credential: Option<crate::extensions::pools::Credential>,

    /// The certificate this service presents, when it has one on disk — roadmap task **T99**.
    ///
    /// **Filled by [`Generator`](super::Generator), like [`authority`](Self::authority)**, and for
    /// the same reason: a recipe may not go looking on a disk. The generator reads the pair while
    /// preparing a row and issues one just before the render for a recipe whose
    /// [`Recipe::certificate`] asks; either way what arrives here is what `certs::service::read`
    /// said about the files, never a description of what was just written.
    ///
    /// [`None`] on every service whose recipe asks for none, and on one whose issuance failed — the
    /// template then writes nothing about TLS and the server does what it did before T99.
    pub(super) certificate: Option<ServiceCertificate>,
}

impl Context {
    /// Which service this is.
    #[must_use]
    pub fn service(&self) -> &ServiceId {
        &self.service
    }

    /// The name this context's recipe was found under.
    #[must_use]
    pub fn package(&self) -> &str {
        &self.package
    }

    /// The installed version, as upstream writes it.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Where the package is unpacked.
    #[must_use]
    pub fn install_path(&self) -> &Path {
        &self.install_path
    }

    /// This instance's data directory.
    #[must_use]
    pub fn data(&self) -> &Path {
        &self.data
    }

    /// Where everything this recipe renders goes: `etc/<service-id>/`.
    ///
    /// It exists by the time a spec is built — [`Generator`](super::Generator) installs before it
    /// asks — which is what makes it the working directory a recipe reaches for when the service has
    /// no better one. A data directory is often the better one and is often not there yet: creating
    /// it is a first-start ritual (`mariadb-install-db`, `initdb`) and not a side effect of
    /// rendering a config file.
    #[must_use]
    pub fn etc(&self) -> &Path {
        &self.etc
    }

    /// `etc/`, the root of everything generated.
    #[must_use]
    pub fn etc_root(&self) -> &Path {
        &self.etc_root
    }

    /// This home's certificate authority as a public PEM, when it has one — roadmap task **T75**.
    #[must_use]
    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// Whether this home answers a site with nothing behind it — roadmap task **T124**.
    #[must_use]
    pub fn welcome(&self) -> bool {
        self.welcome
    }

    /// `run/`, for a socket or a pid file.
    #[must_use]
    pub fn run(&self) -> &Path {
        &self.run
    }

    /// Where this service's output is written.
    #[must_use]
    pub fn logs(&self) -> &Path {
        &self.logs
    }

    /// The port from the row, or [`None`] for a service that listens on a socket.
    #[must_use]
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// The port the activator listens on for this service, where it needs one of its own.
    ///
    /// **A second column and not `port + 1`** — roadmap task **T70**, design D3. With pools on 9000
    /// and 9001 an arithmetic rule gives the first pool's activator the second pool's own port, and
    /// what a user sees is one service refusing to bind and a conflict reported about a number
    /// nobody chose. Allocated once by [`crate::services::ports`] and never computed again.
    #[must_use]
    pub fn activation_port(&self) -> Option<u16> {
        self.activation_port
    }

    /// The address it binds.
    #[must_use]
    pub fn bind(&self) -> &str {
        &self.bind
    }

    /// The port a program must bind to answer on `answering`.
    ///
    /// Itself on every system but macOS, and on macOS itself for everything but 80 and 443. A port
    /// nothing was asked about maps to itself, which is correct rather than a fallback: the mapping
    /// is only ever about the two the operating system reserves.
    #[must_use]
    pub fn bound(&self, answering: u16) -> u16 {
        bound(&self.bindings, answering)
    }

    /// The recipe's defaults with the user's overrides applied.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Where a file this recipe renders ends up.
    ///
    /// What a [`Recipe::spec`] passes on a command line: the program is told to read the file the
    /// template produced, and neither half joins the path itself.
    #[must_use]
    pub fn config(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.etc.join(relative)
    }

    /// An executable inside the installed package, spelled the way this OS spells one.
    ///
    /// `caddy` here is `caddy.exe` on Windows, and a spec's `program` has to be the second on that
    /// machine or the supervisor is handed a path that does not exist. Recipes therefore never write
    /// the suffix, and no recipe carries a `#[cfg]`.
    #[must_use]
    pub fn program(&self, name: &str) -> PathBuf {
        program(&self.install_path, name)
    }

    /// The executable this install publishes under `name`, wherever the publisher put it.
    ///
    /// [`program`](Self::program) is the other half of the pair and the right one for a package: it
    /// joins a name to the install path and lets this OS spell the suffix, which works because
    /// `mixengine-packages` publishes a server as one executable named after its package. **A
    /// runtime is the case where that is not true.** `php-fpm` is `sbin/php-fpm` inside a Unix
    /// build and does not exist at all inside a Windows one, where the same job is done by
    /// `php-cgi.exe` at the root — so a recipe that wrote either path down would be right on one
    /// system and wrong on the other. This looks the name up in the index's own answer, and the
    /// recorded value already carries whatever suffix it needs.
    ///
    /// # Errors
    ///
    /// [`Error::ServiceProvidesNothing`], naming the service and listing what the install does
    /// publish — which is the whole of what somebody looking at a PHP packed without a SAPI needs.
    pub fn provided(&self, name: &str) -> Result<PathBuf> {
        self.provides
            .get(name)
            .map(|relative| crate::paths::join_stored(&self.install_path, relative))
            .ok_or_else(|| Error::ServiceProvidesNothing {
                service: self.service.as_str().to_owned(),
                executable: name.to_owned(),
                known: self.provides.keys().cloned().collect(),
            })
    }

    /// Where this service listens on a Unix socket, if it does.
    ///
    /// Computed once by [`Recipe::endpoints`] and read back here, rather than joined again: the file
    /// and the daemon's own check have to name one path, and two places that build it are two places
    /// for it to drift.
    #[must_use]
    pub fn socket(&self) -> Option<&Path> {
        self.endpoints.socket.as_deref()
    }

    /// Where this package keeps its loadable plugins, for the one system that does not derive it.
    #[must_use]
    pub fn plugins(&self) -> Option<&Path> {
        self.endpoints.plugins.as_deref()
    }

    /// This instance's own directory for temporary files, when its recipe asked for one —
    /// [`Endpoints::scratch`].
    #[must_use]
    pub fn scratch(&self) -> Option<&Path> {
        self.endpoints.scratch.as_deref()
    }

    /// Which home this is — roadmap task **T126**.
    ///
    /// What tells two homes on one machine apart wherever they would otherwise share a name: the
    /// keyring address, and the bootstrap's space-free view (T33b).
    #[must_use]
    pub fn home(&self) -> &crate::home::HomeId {
        &self.home
    }

    /// Where a credential of this service's lives inside the keyring's `mixengine` namespace.
    ///
    /// `<service-id>/<key>` — `mariadb@main/root`. The service id rather than the package name,
    /// because two instances of one server are two databases with two different passwords.
    ///
    /// **One composition, and that is the whole reason it is here.** A recipe names this entry in
    /// the [`EnvValue::Keyring`](mixengine_proto::EnvValue) its spec carries, and the daemon writes
    /// the generated value to it before the first step of the ritual runs; the failure when the two
    /// disagree is a server that starts and a client that cannot authenticate against it, reported
    /// as a service that never became ready.
    ///
    /// **The composition itself is [`services::handoff::secret_key`](crate::services::handoff::secret_key)'s**
    /// — roadmap task **T84**. It moved there when the address stopped being ours alone: MixLab reads
    /// these entries, so the rule is published, and a published rule spelled out in two places is
    /// one that drifts.
    #[must_use]
    pub fn secret_address(&self, key: &str) -> String {
        crate::services::handoff::secret_key(&self.home, &self.service, key)
    }

    /// What this home's extensions add to this front end's configuration — roadmap task **T81c**.
    ///
    /// Already rendered and already filtered to this front end. Empty for everything that is not
    /// one.
    #[must_use]
    pub fn fragments(&self) -> &[FrontEndAddition] {
        &self.fragments
    }

    /// The credential this service's processes are handed at spawn, when it has one — roadmap task
    /// **T82a**.
    ///
    /// **An address rather than a password**, so a recipe reading this puts an
    /// [`EnvValue::Keyring`](mixengine_proto::EnvValue) on its spec and the supervisor does the
    /// reading. Nothing in this crate ever holds the value.
    #[must_use]
    pub fn credential(&self) -> Option<&crate::extensions::pools::Credential> {
        self.credential.as_ref()
    }

    /// The certificate this service presents, when a usable pair is on disk — roadmap task
    /// **T99**. See [`Context::certificate`](Self#structfield.certificate).
    #[must_use]
    pub fn certificate(&self) -> Option<&ServiceCertificate> {
        self.certificate.as_ref()
    }

    /// The credential this recipe declared under `key`, or an empty string when there is none.
    ///
    /// Empty rather than [`None`], because the only caller is a ritual's step builder and the only
    /// way to reach it with no secret is a recipe that declared none — which is a bug in the recipe,
    /// and shows up as a bootstrap that sets an empty password in the suite that drives a real
    /// server. A `Result` here would be a third failure path for a case a test already covers.
    #[must_use]
    pub fn secret(&self, key: &str) -> &str {
        self.secrets.get(key).map_or("", String::as_str)
    }

    /// Put the generated credentials in, which is [`FirstRun::steps`]'s doing and nobody else's.
    ///
    /// [`FirstRun::steps`]: super::first_run::FirstRun::steps
    pub(super) fn set_secrets(&mut self, secrets: BTreeMap<String, String>) {
        self.secrets = secrets;
    }

    /// This context as a template sees it.
    ///
    /// Four groups rather than one flat object, deliberately: a setting called `port` and the row's
    /// own port are different values that a template has to be able to tell apart, and flattening
    /// them would let a recipe shadow the row by declaring a setting with the wrong name.
    fn rendering(&self) -> Rendering<'_> {
        Rendering {
            service: Instance {
                id: self.service.as_str(),
                name: self.service.name(),
                instance: self.service.instance(),
                instance_or_name: self.service.instance().unwrap_or(self.service.name()),
                inherits_environment: self.credential.is_some(),
                port: self.port,
                bind: &self.bind,
            },
            package: Origin {
                name: &self.package,
                version: &self.version,
                path: &self.install_path,
            },
            paths: Layout {
                etc: &self.etc,
                data: &self.data,
                run: &self.run,
                logs: &self.logs,
                socket: self.endpoints.socket.as_deref(),
                plugins: self.endpoints.plugins.as_deref(),
                scratch: self.endpoints.scratch.as_deref(),
                includes: &self.endpoints.includes,
            },
            settings: &self.settings,
            extra: self.settings.extra(),
            certificate: self
                .certificate
                .as_ref()
                .map(|certificate| CertificateView {
                    path: &certificate.certificate,
                    key: &certificate.key,
                    fingerprint: &certificate.fingerprint,
                }),
        }
    }
}

/// The home id every context built by [`Context::for_test`] carries — roadmap task **T126**.
///
/// Named rather than inline so a test asserting a whole address writes it once: the addresses a
/// recipe composes begin with this.
#[cfg(test)]
pub(crate) const TEST_HOME: &str = "0123456789ab";

#[cfg(test)]
impl Context {
    /// A context for `service`, laid out under `root` as a home would lay it out.
    ///
    /// **The only thing besides [`Generator`](super::Generator) that may build one, and only in this
    /// crate's own tests.** It exists because of what a real recipe's [`Recipe::validator`] is: the
    /// service's own binary. Rendering through a generator runs `caddy validate`, so a test of the
    /// *template* would need fifty megabytes of Caddy installed to find out whether a variable name
    /// is misspelled — and would then be measuring Caddy. The real server judges the real thing in
    /// `crates/mixengine-daemon/tests/caddy.rs`; this is what keeps the cheap half cheap.
    pub(super) fn for_test(
        service: ServiceId,
        package: &str,
        root: &Path,
        provides: BTreeMap<String, String>,
        port: Option<u16>,
        settings: Settings,
    ) -> Self {
        Self {
            etc: root.join("etc").join(service.as_str()),
            etc_root: root.join("etc"),
            data: root.join("data").join(package),
            run: root.join("run"),
            logs: root.join("logs").join("services").join(service.as_str()),
            install_path: root.join("packages").join(package),
            provides,
            package: package.to_owned(),
            version: "0.0.0".to_owned(),
            port,
            activation_port: None,
            bind: "127.0.0.1".to_owned(),
            settings,
            endpoints: Endpoints::default(),
            bindings: Vec::new(),
            authority: None,
            // A test renders what a home renders: the switch is on unless a case is about it being
            // off, and that case says so with `with_welcome`.
            welcome: true,
            fragments: Vec::new(),
            secrets: BTreeMap::new(),
            credential: None,

            // A fixed id, so an address a test asserts is one it can spell. A real home's is
            // random and six bytes wide — `migrations/0001_initial.sql`.
            home: crate::home::HomeId::parse(TEST_HOME).expect("a valid id"),
            certificate: None,
            service,
        }
    }

    /// The credential a real render would have resolved off this home's extension sites.
    ///
    /// A setter rather than an argument to [`for_test`](Self::for_test), on
    /// [`with_activation_port`](Self::with_activation_port)'s reasoning: one recipe in this crate
    /// can carry one, and a parameter every other call site passed [`None`] to would be ten edits
    /// that say nothing.
    pub(super) fn with_credential(
        mut self,
        credential: Option<crate::extensions::pools::Credential>,
    ) -> Self {
        self.credential = credential;
        self
    }

    /// The certificate a real render would have read off this home's `certs/services/` — roadmap
    /// task **T99**. A setter for [`with_credential`](Self::with_credential)'s reason.
    pub(super) fn with_certificate(mut self, certificate: ServiceCertificate) -> Self {
        self.certificate = Some(certificate);
        self
    }

    /// The version of the package this instance runs, which a recipe may branch on.
    ///
    /// MySQL is why: which program bootstraps a data directory is a fact about the *line*, and a
    /// test that could not vary it could only ever exercise one of three routes.
    pub(super) fn with_version(mut self, version: &str) -> Self {
        self.version = version.to_owned();
        self
    }

    /// The activator port a real render would have read off the row.
    ///
    /// A setter rather than a seventh argument to [`for_test`](Self::for_test): all but two of this
    /// crate's recipes have no activator, and a parameter every one of them passed [`None`] to
    /// would be ten call sites edited to say nothing.
    pub(super) fn with_activation_port(mut self, port: Option<u16>) -> Self {
        self.activation_port = port;
        self
    }

    /// The address a row asked this service to bind — roadmap task **T154**, whose recipe refuses
    /// every one that is not loopback.
    pub(super) fn with_bind(mut self, bind: &str) -> Self {
        bind.clone_into(&mut self.bind);
        self
    }

    /// The endpoints a real render would have asked the recipe for.
    pub(super) fn with_endpoints(mut self, endpoints: Endpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    /// The mapping a real render would have been given.
    pub(super) fn with_bindings(mut self, bindings: Vec<PortBinding>) -> Self {
        self.bindings = bindings;
        self
    }

    /// The authority a real render would have read off this home's certificates directory.
    pub(super) fn with_authority(mut self, authority: Option<String>) -> Self {
        self.authority = authority;
        self
    }

    /// A home that turned the welcome page off — roadmap task **T124**.
    #[cfg(test)]
    pub(super) fn with_welcome(mut self, welcome: bool) -> Self {
        self.welcome = welcome;
        self
    }

    /// A home whose id is not [`TEST_HOME`] — roadmap task **T33b**, whose test needs two.
    #[cfg(test)]
    pub(super) fn with_home(mut self, home: crate::home::HomeId) -> Self {
        self.home = home;
        self
    }

    /// The fragments a real render would have read out of the  table.
    pub(super) fn with_fragments(mut self, fragments: Vec<FrontEndAddition>) -> Self {
        self.fragments = fragments;
        self
    }

    /// One credential, for the test that proves a template cannot see one.
    pub(super) fn put_secret(&mut self, key: &str, secret: &str) {
        self.secrets.insert(key.to_owned(), secret.to_owned());
    }
}

/// [`Context`] in the shape a template reads it.
#[derive(Debug, Serialize)]
struct Rendering<'a> {
    service: Instance<'a>,
    package: Origin<'a>,
    paths: Layout<'a>,
    settings: &'a Settings,
    /// Also `settings.extra`, and repeated at the top level because every template ends with it.
    extra: &'a str,

    /// The certificate this service presents, or nothing — roadmap task **T99**.
    ///
    /// **A fifth group rather than three loose fields on `paths`**, so a template branches on one
    /// value: `{% if certificate %}`. This is the one `Option` a template *can* branch on safely,
    /// unlike [`Instance::instance`]: minijinja renders a `None` as a falsy `none`, and nothing
    /// interpolates this group outside the branch that tests it.
    certificate: Option<CertificateView<'a>>,
}

/// The `certificate` group of a [`Rendering`] — roadmap task **T99**.
#[derive(Debug, Serialize)]
struct CertificateView<'a> {
    /// The certificate, absolute.
    path: &'a Path,

    /// Its private key, absolute.
    key: &'a Path,

    /// SHA-256 of the certificate's DER, lowercase hex, for the file's header.
    fingerprint: &'a str,
}

/// The `service` half of a [`Rendering`].
#[derive(Debug, Serialize)]
struct Instance<'a> {
    id: &'a str,
    name: &'a str,
    instance: Option<&'a str>,

    /// The instance half of the id, falling back to the package name for a service that has none —
    /// roadmap task **T82a**.
    ///
    /// **Not [`instance`](Self::instance), which is an [`Option`] a template cannot branch on
    /// safely**: minijinja renders a `none` as the word rather than as an undefined value, so a
    /// template writing `{{ service.instance }}` for a single-instance service would quietly produce
    /// a path called `none`. This is what a template wants whenever it needs the string that
    /// separates one instance of a package from another, and it is the same expression
    /// `recipes::php_fpm::socket_path` uses in Rust — which is what keeps that recipe's file and its
    /// readiness check naming one socket.
    instance_or_name: &'a str,

    /// Whether this service's own process manager should pass its environment to what it spawns —
    /// roadmap task **T82a**, that design's D3.
    ///
    /// True exactly when this service carries a [`Credential`](crate::extensions::pools::Credential):
    /// php-fpm clears a worker's environment unless told otherwise, and the only alternative to
    /// telling it otherwise is `env[NAME] = <literal>`, which performs no expansion and would put a
    /// database superuser's password on disk.
    ///
    /// **A flag rather than the credential**, deliberately: a template that could name a keyring
    /// entry is a template that could print one, and nothing a `Rendering` carries should be able to.
    inherits_environment: bool,

    port: Option<u16>,
    bind: &'a str,
}

/// The `package` half.
#[derive(Debug, Serialize)]
struct Origin<'a> {
    name: &'a str,
    version: &'a str,
    path: &'a Path,
}

/// The `paths` half.
#[derive(Debug, Serialize)]
struct Layout<'a> {
    etc: &'a Path,
    data: &'a Path,
    run: &'a Path,
    logs: &'a Path,

    /// [`Endpoints::socket`], for a template that has to write it into a configuration file.
    socket: Option<&'a Path>,

    /// [`Endpoints::plugins`], likewise.
    plugins: Option<&'a Path>,

    /// [`Endpoints::scratch`], for a template whose server takes a `tmpdir`.
    scratch: Option<&'a Path>,

    /// [`Endpoints::includes`], which a template reads by name: `paths.includes['mime.types']`.
    includes: &'a BTreeMap<String, PathBuf>,
}

/// The pair a service presents, as a template and a spec have to name it — roadmap task **T99**.
///
/// The shape of `served::SiteCertificate`, for a leaf that belongs to a server rather than to a
/// site. **Read off the disk and never described from a write**: the generator fills it from
/// `certs::service::read`, so the fingerprint here is the one on disk now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCertificate {
    /// Absolute path to the certificate.
    pub certificate: PathBuf,

    /// Absolute path to the private key.
    pub key: PathBuf,

    /// SHA-256 of the certificate's DER, lowercase hex.
    ///
    /// **Rendered into the generated file's header, and read back by nothing** — T51's reason,
    /// restated for a server that does not reload: a reissue to the same path must still change the
    /// file, so that a rewrite finds a difference and the file says which certificate it was
    /// rendered for.
    pub fingerprint: String,
}

/// Paths a recipe computes that its own template also has to name.
///
/// Four so far — two MariaDB's, one nginx's, and one the MySQL family shares — and all of them here
/// for one reason: the alternative is a template joining a path itself, and the failure when the
/// file and the daemon's own check disagree is a service that starts perfectly and is reported as
/// never having come up.
#[derive(Debug, Clone, Default)]
pub struct Endpoints {
    /// A directory of this instance's own for its temporary files, for a server that would otherwise
    /// use the machine's — and *clean* the machine's, at every start, of every file that looks like
    /// its own, whoever made it. [`None`] for a service that keeps no temporary files or keeps them
    /// where nobody else looks. The generator creates it beside the log and data directories, so a
    /// first-run step and the service itself can both count on it being there.
    pub scratch: Option<PathBuf>,

    /// Where this service listens on a Unix socket — [`None`] on a system without them, and for
    /// every service that listens on a port alone.
    ///
    /// **Two recipes read this two ways, and the field promises neither.** MariaDB puts the socket
    /// *file* here, because `socket = ` in `my.cnf` names a file. PostgreSQL puts the *directory*
    /// here, because `unix_socket_directories` takes a directory and the server creates
    /// `.s.PGSQL.<port>` inside it. Each is the convention of one recipe and its own template, which
    /// is why nothing outside that pair may assume either: a caller measuring this against
    /// [`within_socket_limit`](super::recipes) would be seventeen characters optimistic about
    /// PostgreSQL, and the recipe measures the file rather than the directory for exactly that
    /// reason.
    pub socket: Option<PathBuf>,

    /// Where this package keeps its loadable plugins, for the one system that does not derive it.
    pub plugins: Option<PathBuf>,

    /// Data files out of the package's own archive that the template `include`s by absolute path,
    /// keyed by the `provides` name the index publishes them under — roadmap task **T37**.
    ///
    /// **nginx is why, and it will not be the only one.** A generated `nginx.conf` sits in
    /// `etc/nginx/` with no `conf/` beside it, so `mime.types` has to be reached where the artifact
    /// keeps it — and Phase 4's sites need `fastcgi_params` from the same place. Resolved through
    /// [`Context::provided`], so a package that publishes neither fails while the recipe is being
    /// rendered, naming what the install does provide, rather than as an `include` of a file that
    /// is not there.
    pub includes: BTreeMap<String, PathBuf>,
}

/// How many instances of this package a home may have, which is what an id may look like.
///
/// **A recipe must answer**, which is why [`Recipe::instancing`] has no default body: the question
/// has a different answer for every server in `docs/features/services.md`'s catalogue, and a
/// default here would be a decision made by whoever wrote this enum on behalf of a recipe nobody had
/// written yet. It is also the half of T36 that `service.create` cannot avoid — what a *second*
/// instance of one package means — while running two of them side by side stays T36's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instancing {
    /// Exactly one, and its id carries no `@`: there is one Caddy, and one active front end.
    Single,

    /// As many as are named, and every id carries one: `mariadb@main`, `mariadb@legacy`.
    Named,
}

/// Which table supplies the binary a recipe runs.
///
/// **A property of the recipe, not a rule in the daemon**, for [`Instancing`]'s reason: where
/// php-fpm's process comes from is a fact about php-fpm, and spelling it here is what lets both the
/// refusal in `service.create` and the hook that creates the pool derive from one answer instead of
/// from a string compared in two places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A `packages` row, put there by `package.install`, named by `service.create`.
    Package,

    /// A `runtime_installs` row of this kind, put there by `runtime.install` — which also creates
    /// the service, because a pool without a PHP is nothing and a PHP without a pool is a language
    /// no site can be served by. `service.create` refuses such a recipe and says which command to
    /// use instead.
    Runtime(mixengine_proto::RuntimeKind),
}

/// What a service is *for*, where two packages can be for the same thing — roadmap task **T37**.
///
/// **[`Instancing`] cannot say this**, which is why there are two enums rather than one. Instancing
/// is about a package: how many rows may name `nginx`. This is about a *job*: `docs/features/services.md`
/// says exactly one of Caddy and Nginx is the active front end, and both of them answering
/// [`Instancing::Single`] leaves a home with one of each — two programs that both own 80 and 443 the
/// moment sites arrive.
///
/// Only the one distinction, because only one exists: every other recipe in this catalogue is a
/// server that a home may run beside any of the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The program every site on the machine is reached through. Caddy and Nginx.
    ///
    /// **It carries which configuration language it reads** — roadmap task **T81c**. A
    /// `[[recipe.front_end]]` fragment is written for one of the two syntaxes and is a syntax error
    /// in the other, so something has to say which recipe a fragment belongs to; saying it here
    /// means a recipe cannot be the front end without answering, which a second method beside this
    /// one would have allowed.
    FrontEnd(mixengine_proto::FrontEndServer),

    /// Everything else: a database, a cache, a pool. As many as the home wants.
    Other,
}

/// An executable inside an installed package, spelled the way this OS spells one.
///
/// **The one spelling of that join, and the reason it is a free function** — roadmap task **T97**.
/// [`Context::program`] is how a recipe asks, and it is the only way to ask while a service exists;
/// `service.set_front_end` has to name the binary of a front end whose row *does not exist yet*, in
/// order to find out whether this machine will let it answer on 80 and 443 before anything is
/// stopped or deleted. Both callers therefore ask the same function, so the path a grant is written
/// against and the path a spec runs cannot come to differ by a suffix.
#[must_use]
pub fn program(install_path: &Path, name: &str) -> PathBuf {
    install_path.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// Where one service listens, for whoever else has to point at it.
///
/// **A value and not a string**, because the two shapes are spelled differently by every program
/// that consumes one: Caddy writes a socket as `unix//run/php-fpm-8.3.sock` and nginx writes the
/// same socket as `unix:/run/php-fpm-8.3.sock`. Each front end converts this in its own recipe,
/// which is the only place that spelling is knowledge about anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upstream {
    /// A Unix domain socket, by absolute path.
    Socket(PathBuf),

    /// A TCP address, which on Windows is what a pool has instead.
    Tcp(SocketAddr),
}

/// Both addresses a site may point at for one service — roadmap task **T70**.
///
/// **One value rather than two maps**, because a site file names them together and in one order:
/// the service first, the activator second, so that a request the service refuses is retried
/// against whatever can start it. Two maps built beside each other could disagree about which
/// service an activator belongs to; this cannot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstreams {
    /// Where the service itself listens.
    pub listen: Upstream,

    /// Where the activator waits on its behalf, or [`None`] for a service nothing can start by
    /// connecting to it — which is every recipe but php-fpm today.
    pub activator: Option<Upstream>,
}

/// One command `<root>/bin` fronts on an installed package's behalf — roadmap task **T130**.
///
/// **Declared rather than derived from `provides`**, and nginx is the reason: its map holds
/// `mime.types` and `fastcgi_params` beside `nginx.exe`, so a `bin/` filled from the map wholesale
/// would hold a `mime.types` that is a copy of the shim binary. The same rule keeps the supervised
/// servers out — [`crate::shims::COMMANDS`] states it for `php-fpm` and it holds here for
/// `mariadbd`, `postgres` and `redis-server`: a shim in front of one would be a second way to start
/// a process nothing is watching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientCommand {
    /// What the user types, and what the file in `bin/` is named.
    pub name: &'static str,

    /// Which of the artifact's executables it runs, by the key `provides` publishes it under.
    pub executable: &'static str,

    /// Whether this name is the package's own or a spelling it stands in for.
    pub claim: Claim,
}

/// How strongly a package claims a command name.
///
/// Two installed packages can want one name — MariaDB is documented under `mysql` and a MariaDB
/// 10.x archive still ships a file by that name — and something has to decide without asking. The
/// order is `crate::shims::resolve_claims`; this is the first and largest term of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Claim {
    /// A name from a product this package stands in for. MariaDB's `mysql`, and nothing else today.
    Alias,

    /// The name the package publishes itself. `mariadb`, `psql`, `redis-cli`.
    Own,
}

/// How to configure and run one kind of service.
///
/// Implemented once per `packages.name`. Everything except [`spec`](Self::spec) has a default,
/// because a service with no configuration file of its own — Redis very nearly, Memcached entirely —
/// is a recipe that is only a command line.
pub trait Recipe: std::fmt::Debug + Send + Sync {
    /// The `packages.name` this recipe is for.
    ///
    /// **`&str` rather than `&'static str` since T81.** Every compiled-in recipe returns a literal
    /// and reads the same; what the borrow makes room for is a recipe built at run time out of an
    /// installed extension's manifest, whose name is a `String` in a row
    /// ([`ExtensionRecipe`](crate::extensions::recipe::ExtensionRecipe)).
    fn package(&self) -> &str;

    /// How many instances of this package a home may have. See [`Instancing`].
    fn instancing(&self) -> Instancing;

    /// What this service is for, where two packages can be for the same thing. See [`Role`].
    ///
    /// Defaulted to [`Role::Other`] because that is what a server *is* unless it is one of the two
    /// front ends: a recipe added later has to opt into the exclusivity rather than remember to opt
    /// out of it.
    fn role(&self) -> Role {
        Role::Other
    }

    /// Which table supplies the binary. See [`Source`].
    ///
    /// Defaulted, unlike [`instancing`](Self::instancing), because the answer *is* the same for
    /// every server the index publishes and only differs for the one recipe that runs out of a
    /// language.
    fn source(&self) -> Source {
        Source::Package
    }

    /// The port this service would like, and [`None`] for one the daemon hands no port to.
    ///
    /// **A wish, not a reservation** — roadmap task **T34c**. It is declared here, beside the binary
    /// and the template, because which number a product is documented under is a fact about the
    /// product: 3306 for either database, 6379 for Redis. A `service.create` that had to know them
    /// would be a caller that has to know the whole catalogue, and two recipes naming 3306 would be
    /// a special case rather than the ordinary one it is —
    /// [`Port::Allocate`](crate::services::Port::Allocate) gives the first row to ask its wish and
    /// the next the first free port above.
    ///
    /// [`None`] means the daemon allocates nothing: a pool on a Unix socket, and Caddy, whose 80 and
    /// 443 are its own settings — a front end moved to 81 because something else answered on 80 is
    /// not a front end anybody asked for.
    fn preferred_port(&self) -> Option<u16> {
        None
    }

    /// The account a client connects to this server as, for a server that has one — roadmap task
    /// **T82**, the design's D5.
    ///
    /// **Defaulted to [`None`], because that is what a server *is* unless it is a database.** A
    /// front end, a cache and a pool have no such account, so a recipe added later opts in rather
    /// than remembering to opt out.
    ///
    /// A **name and never a credential**: where the password lives is
    /// [`Context::secret_address`]'s answer, and it stays in the keyring. Read by
    /// [`extensions::database`](crate::extensions::database) for `{db_user}`, and by
    /// [`services::handoff`](crate::services::handoff) for the connection handoff — one answer, so
    /// a manifest never has to guess it.
    fn administrator(&self) -> Option<&'static str> {
        None
    }

    /// What a database client speaks to this server, for a server one opens — roadmap task
    /// **T83**, the design's D5.
    ///
    /// **Defaulted to [`None`] for [`administrator`](Self::administrator)'s reason**: a front end, a
    /// cache with no client protocol and a pool are not something a database client opens. Redis
    /// answers although it names no administrator — MixLab opens a Redis, and a handoff to it simply
    /// carries no credential.
    fn protocol(&self) -> Option<mixengine_proto::DatabaseProtocol> {
        None
    }

    /// The commands a person runs out of this package — roadmap task **T130**.
    ///
    /// **Defaulted to nothing**, because that is what a package *is* unless somebody types one of
    /// its programs: the two front ends and php-fpm publish binaries a person never invokes by
    /// hand, and a recipe added later opts in rather than remembering to opt out.
    ///
    /// A row whose [`ClientCommand::executable`] the installed artifact does not publish is silently
    /// skipped when `bin/` is filled — the Windows MariaDB packs no `mariadb-backup` on every
    /// branch, and a name in `bin/` that resolves to nothing is worse than a missing one.
    fn clients(&self) -> &'static [ClientCommand] {
        &[]
    }

    /// What one of those commands is told about the instance it belongs to — roadmap task **T130**.
    ///
    /// **A client's own variable and nothing invented**: `mysql` reads `MYSQL_TCP_PORT` and `psql`
    /// reads `PGPORT`, and a family with no such variable — Redis, Memcached — answers an empty map
    /// rather than one nothing will read. What makes it necessary is the port allocator:
    /// `services.md` gives 3306 to whichever of MariaDB and MySQL asks first and the next free port
    /// above to the other, so on a home with both, a bare `mysql` would otherwise open a session on
    /// the other product's server and report success.
    ///
    /// Handed the [`Upstream`] rather than the whole [`Context`] because a shim has no service to
    /// build one from — it is resolving a client in somebody's terminal, with the daemon possibly
    /// stopped. A socket is told to nobody: no client family has a variable for one.
    fn client_env(&self, listen: &Upstream) -> BTreeMap<&'static str, String> {
        let _ = listen;
        BTreeMap::new()
    }

    /// How to tell that this service has nothing to do — roadmap task **T69**.
    ///
    /// **The recipe's half of an [`IdlePolicy`], and `services.idle_minutes` holds the other.** Only
    /// the recipe knows which port its pool listens on or whether it renders a status endpoint, and
    /// a user has no way to check such a value and no reason to want it different: a probe that
    /// disagrees with the program it measures is a bug here, not a preference there. How *long* a
    /// machine's owner will keep something warm is theirs, and is the column.
    ///
    /// [`None`] means never idle-stopped whatever the row says, which is both front ends' answer —
    /// the thing that starts everything else back up cannot be the thing that gets stopped.
    ///
    /// [`IdlePolicy`]: mixengine_proto::IdlePolicy
    fn idle_probe(&self, context: &Context) -> Option<mixengine_proto::IdleProbe> {
        let _ = context;
        None
    }

    /// How long this service should look idle before it is stopped, when nobody has said and the
    /// home is not saving resources.
    ///
    /// **[`None`] for every recipe, and that is the decision** — [ADR 0041]: MixEngine stops nothing
    /// a person did not ask it to stop. A site that was up stays up. The numbers T70 and T70a chose
    /// are [`idle_when_saving`](Self::idle_when_saving)'s, and reach a service only while the home's
    /// *Save battery* switch is on.
    ///
    /// Kept as a method rather than deleted so that `idle_minutes` can still tell *nobody said*
    /// from *somebody said no*, and so a recipe for a program that should idle regardless has a
    /// place to say so. None does.
    ///
    /// [ADR 0041]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0041-mixengine-stops-nothing-a-person-did-not-ask-it-to.md
    fn idle_default(&self) -> Option<mixengine_proto::Millis> {
        None
    }

    /// How long this service should look idle before it is stopped, when nobody has said and the
    /// home **is** saving resources — roadmap task **T167b**.
    ///
    /// **A recipe answers a number only once something can start its service again.** Stopping a
    /// service nothing can wake is a site that answers 502 for ever, so php-fpm names half an hour
    /// (**T70** — the request that finds the pool down is what wakes it), the databases and the
    /// caches an hour (**T70a** — the connection that finds the server down is what wakes it), and
    /// the two front ends [`None`] for ever, because the thing that starts everything else back up
    /// cannot be the thing that gets stopped.
    fn idle_when_saving(&self) -> Option<mixengine_proto::Millis> {
        None
    }

    /// Whether a memory watchdog may restart this service — roadmap task **T71a**.
    ///
    /// **`false`, and a recipe opts in**, because whether a program survives being restarted under
    /// memory pressure is a property of the program and not a preference about it: a php-fpm pool
    /// loses the requests in flight, which `pm.max_requests` already recycles workers underneath; a
    /// database loses a transaction; a cache loses everything somebody believes is still there.
    ///
    /// **Unlike [`idle_default`](Self::idle_default), this is not overruled by a row**, and needs no
    /// three-state column to leave room for one: nothing about it is per-home. A person's control
    /// over the watchdog is `memory_mb` itself — nothing watches a service that declared no ceiling.
    /// The day somebody wants an override, it arrives as a column whose `NULL` means *what the
    /// recipe says*, and nothing stored has to be guessed at.
    fn restart_over_memory_default(&self) -> bool {
        false
    }

    /// What proves an installed copy of this package actually runs here.
    ///
    /// Handed to [`Installer::install`](crate::install::Installer::install) after the archive is
    /// unpacked and before the staging directory is renamed into place, so a build that will not
    /// start on this machine leaves nothing behind. [`None`] for a package with nothing cheap to
    /// run — but a server almost always has one, and T20a's whole finding is that unpacking is not
    /// evidence that anything runs.
    ///
    /// The executable is named by its key in `Artifact::provides` rather than by a path: the path
    /// inside the archive belongs to whoever published it, and the name belongs to us.
    fn smoke_test(&self) -> Option<crate::install::SmokeTest> {
        None
    }

    /// Every override this recipe understands, and what each is when nobody has said.
    ///
    /// An override naming anything else is refused — see [`settings`](super::settings).
    fn settings(&self) -> &'static [Setting] {
        &[]
    }

    /// The files it renders into `etc/<service-id>/`.
    fn files(&self) -> &'static [TemplateFile] {
        &[]
    }

    /// The command that judges a rendering before it is installed, if there is one.
    ///
    /// Handed the [`Context`] because the checker is usually the service's own binary — `caddy
    /// validate`, `nginx -t` — which lives inside the package this instance was installed from.
    fn validator(&self, context: &Context) -> Option<Validator> {
        let _ = context;
        None
    }

    /// The service, as something the supervisor can run.
    ///
    /// A **builder** rather than a finished [`ServiceSpec`](mixengine_proto::ServiceSpec), because
    /// the parts of a spec that come from the row rather than from the recipe — the resource limits,
    /// today — are applied by [`Generator`](super::Generator) afterwards. A recipe that returned a
    /// finished spec could forget one, and the failure would be a limit silently not applied.
    ///
    /// # Errors
    ///
    /// Whatever this particular service cannot answer: a setting whose value is impossible, a
    /// dependency that is not a service id. A spec that does not *build* is not this method's error
    /// to report — the generator builds it.
    fn spec(&self, context: &Context) -> Result<ServiceSpecBuilder>;

    /// The paths above, for the recipes that have any.
    ///
    /// Asked once by [`Generator`](super::Generator) and stored on the [`Context`], so a template
    /// and a [`spec`](Self::spec) read one answer instead of computing two.
    ///
    /// # Errors
    ///
    /// Whatever computing one costs — a socket path this kernel will not accept.
    fn endpoints(&self, context: &Context) -> Result<Endpoints> {
        let _ = context;

        Ok(Endpoints::default())
    }

    /// Where this service listens, for a *different* service's configuration to point at.
    ///
    /// [`None`] for everything that nothing points at, which is every recipe but php-fpm. The pool
    /// is why this exists: `fastcgi_pass` needs `run/php-fpm-<version>.sock` on Unix and
    /// `127.0.0.1:<row port>` on Windows, and both are already computed inside that recipe's own
    /// spec. A site template that worked either of them out again would be a second copy of a rule
    /// whose whole point is that it differs per system.
    ///
    /// # Errors
    ///
    /// Whatever computing one costs — a socket path this kernel will not accept, a Windows pool
    /// whose row carries no port.
    fn upstream(&self, context: &Context) -> Result<Option<Upstream>> {
        let _ = context;

        Ok(None)
    }

    /// Where the *activator* listens for this service, for a site file to name after
    /// [`upstream`](Self::upstream) — roadmap task **T70**.
    ///
    /// [`None`] for every recipe nothing can start by connecting to it, which is the default and is
    /// most of them. A recipe that answers [`Some`] is promising two things: the address differs
    /// from its own, and it is the same address on every render — a site file that moved when a pool
    /// stopped would make each idle stop reload the front end, which is a reload storm driven by the
    /// thing that exists to save work.
    ///
    /// **[`Some`] is not a promise that the daemon is listening there.** Whether it binds is the
    /// daemon's, and depends on why the service is stopped: a service a person stopped is not one a
    /// request may start again (design D8). What this answers is only *where*.
    ///
    /// # Errors
    ///
    /// Whatever computing one costs — for a socket, a home too deeply nested for the derived path,
    /// which is nine characters longer than the service's own and can cross `sockaddr_un`'s limit on
    /// a home that was just inside it.
    fn activator(&self, context: &Context) -> Result<Option<Upstream>> {
        let _ = context;

        Ok(None)
    }

    /// Whether this recipe's activator needs a port allocated onto the row — roadmap task **T70**.
    ///
    /// **A question about the recipe and this system, never about one instance**, which is why it
    /// takes no [`Context`]: the port has to be allocated before a context exists to render with.
    /// [`activator`](Self::activator) is what says *where*; this says only *whether a number is
    /// owed*, and answers `false` for a recipe whose activator derives its address from a socket
    /// path — there is nothing to allocate and nothing to take out of circulation.
    fn activation_port_needed(&self) -> bool {
        false
    }

    /// The addresses of this service's *own* that a connection may start it at — roadmap task
    /// **T70a**, design D4.
    ///
    /// Empty by default, and empty for php-fpm on purpose. A pool has a front end in front of it,
    /// so its activator gets a permanent address of its own ([`activator`](Self::activator)) and
    /// the site file names both. A database has nothing in front of it — a client dials
    /// `127.0.0.1:3306` and nothing else will do — so the daemon binds what the service itself
    /// listens on while the service is idle-stopped, and gives it back on the start.
    ///
    /// **More than one address, because a database has more than one.** On a system with Unix
    /// sockets MariaDB answers on a port *and* on a socket in `run/`, and which of the two a
    /// client uses is that client's habit rather than a setting: a generated `.env` names the
    /// port, `mariadb` typed with no host at all names the socket. A recipe answering only the
    /// port leaves the second client hanging against an address nothing holds.
    ///
    /// **This is the one place [`activator`](Self::activator)'s permanent address does not hold**,
    /// and it cannot: the address belongs to the service, so it is bound only while nothing is
    /// serving it. What that costs is the window between the release and the service's own bind,
    /// which is the service's start time — stated in `docs/features/resource-isolation.md`
    /// rather than hidden.
    ///
    /// # Errors
    ///
    /// Whatever computing one costs — a socket path this kernel will not accept, or a row carrying
    /// no port for a service that has nothing else to be addressed by.
    fn held_while_stopped(&self, context: &Context) -> Result<Vec<Upstream>> {
        let _ = context;

        Ok(Vec::new())
    }

    /// Directories under `etc/<service-id>/` whose contents must be exactly what
    /// [`sites`](Self::sites) and [`files`](Self::files) render into them.
    ///
    /// Anything else in one is removed by [`install`](super::document::install), in the same
    /// operation and before the same reload. Only the two front ends declare one, and each declares
    /// `sites/`: without it a deleted site keeps the file it had, and a file in that directory is a
    /// site that goes on being served.
    ///
    /// **Nothing sweeps `etc/<service-id>/` itself.** A directory belonging to a service that was
    /// deleted is `service.delete`'s problem and is not made this one's by proximity.
    fn swept(&self) -> &'static [&'static str] {
        &[]
    }

    /// The site files this service serves, if it is the one every site is reached through.
    ///
    /// Asked only of the recipe holding [`Role::FrontEnd`], and appended to the set
    /// [`files`](Self::files) rendered — **not installed by a path of its own**. That is the whole
    /// arrangement: the checker judges a staging directory, so a site file written anywhere else
    /// would be invisible to `caddy validate` and present at run time, which is the one arrangement
    /// whose correctness cannot be checked before it is live.
    ///
    /// `context` is the *front end's* own, which is where a site block gets the port to listen on
    /// and the paths its includes resolve against.
    ///
    /// # Errors
    ///
    /// [`Error::TemplateBroken`] naming the site template: a template is this build's, so a refusal
    /// here is a bug of ours rather than a configuration a user can fix.
    fn sites(&self, context: &Context, served: &[Served]) -> Result<Vec<Document>> {
        let _ = (context, served);

        Ok(Vec::new())
    }

    /// What this home's extensions add to this service's configuration — roadmap task **T81c**.
    ///
    /// Asked only of the recipe holding [`Role::FrontEnd`], appended to the set
    /// [`files`](Self::files) rendered, and for [`sites`](Self::sites)' reason exactly: the checker
    /// judges a staging directory, so a fragment written anywhere else would be invisible to
    /// `caddy validate` and present at run time.
    ///
    /// A second method rather than more return values from `sites`, because they are two questions —
    /// what this home serves, and what its extensions added — and a recipe that answers one and not
    /// the other should not have to say so with an empty vector.
    ///
    /// `context.fragments()` is already filtered to the fragments written for this front end.
    ///
    /// # Errors
    ///
    /// Whatever building a document out of already-rendered text costs, which today is nothing —
    /// the signature is a `Result` because a recipe that wanted to refuse a fragment it could not
    /// place would have nowhere else to say so.
    fn fragments(&self, context: &Context) -> Result<Vec<Document>> {
        let _ = context;

        Ok(Vec::new())
    }

    /// What must be done once, before this service is ever started — [`None`] for most.
    ///
    /// See [`first_run`](super::first_run) for the shape, and for why the credentials a ritual needs
    /// are declared here and generated by the daemon.
    fn ritual(&self) -> Option<super::first_run::Ritual> {
        None
    }

    /// The names a certificate for this service must cover, when it wants one — [`None`] for most.
    /// Roadmap task **T99**.
    ///
    /// Opt-in like [`ritual`](Self::ritual). A recipe that answers gets a leaf this home's authority
    /// signed, under `certs/services/`, issued by the generator just before the render and reachable
    /// through [`Context::certificate`]; one that answers [`None`] is never asked again. See
    /// `certs::service` for what the names are and why they are IPv4 only.
    fn certificate(&self, context: &Context) -> Option<Vec<String>> {
        let _ = context;

        None
    }

    /// How this package makes a database and an account for one — [`None`] for most.
    ///
    /// See [`databases`](super::databases) for the shape, and for why the daemon rather than the
    /// recipe holds the credentials. Opt-in like [`ritual`](Self::ritual): a recipe with no
    /// databases says nothing, rather than having to remember to say no.
    fn databases(&self) -> Option<super::databases::DatabaseAdmin> {
        None
    }
}

/// The recipes a running daemon can find.
///
/// A value rather than a global, which is what makes the generator testable at all: a test composes
/// a catalogue holding one recipe of its own and never touches what this build ships.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    recipes: BTreeMap<String, Arc<dyn Recipe>>,

    /// Packages this build installs, lists and removes but never runs — roadmap task **T206a**.
    toolchains: std::collections::BTreeSet<String>,
}

impl Catalogue {
    /// What this build knows how to run.
    ///
    /// Nine recipes, which is `docs/features/services.md`'s catalogue — arrived one roadmap task
    /// at a time, because a template written before the server it configures is a guess nobody can
    /// check. A home whose `services` table names none of them is answered by this without a special
    /// case.
    #[must_use]
    pub fn builtin() -> Self {
        Self::default()
            .with(Arc::new(super::recipes::Caddy))
            .with(Arc::new(super::recipes::Memcached))
            .with(Arc::new(super::recipes::Mongodb))
            .with(Arc::new(super::recipes::Mariadb))
            .with(Arc::new(super::recipes::Mysql))
            .with(Arc::new(super::recipes::Nginx))
            .with(Arc::new(super::recipes::PhpFpm))
            .with(Arc::new(super::recipes::Postgres))
            .with(Arc::new(super::recipes::Redis))
            .with_toolchain("msys2")
    }

    /// The same catalogue, with `recipe` in it.
    ///
    /// A recipe for a `packages.name` that is already known **replaces** it, which is what lets a
    /// debug build put a fixture in front of a real service and a test put one in front of nothing.
    #[must_use]
    pub fn with(mut self, recipe: Arc<dyn Recipe>) -> Self {
        self.recipes.insert(recipe.package().to_owned(), recipe);
        self
    }

    /// The recipe for a package, if this build has one.
    #[must_use]
    pub fn recipe(&self, package: &str) -> Option<&Arc<dyn Recipe>> {
        self.recipes.get(package)
    }

    /// Every package this catalogue can run, in name order.
    ///
    /// For the message a service belonging to something else produces.
    pub fn packages(&self) -> impl Iterator<Item = &str> + '_ {
        self.recipes.keys().map(String::as_str)
    }

    /// A package this build installs, lists and removes but never runs — roadmap task **T206a**,
    /// D5. A toolchain has no recipe because there is nothing in it to supervise: `msys2` is the
    /// devkit RubyInstaller's Ruby needs to build gems with C extensions.
    #[must_use]
    pub fn with_toolchain(mut self, package: &str) -> Self {
        self.toolchains.insert(package.to_owned());
        self
    }

    /// Every toolchain this build knows.
    pub fn toolchains(&self) -> impl Iterator<Item = &str> + '_ {
        self.toolchains.iter().map(String::as_str)
    }

    /// Everything `package.install` may take: what this build runs, then what it only installs.
    pub fn installable(&self) -> impl Iterator<Item = &str> + '_ {
        self.packages().chain(self.toolchains())
    }

    /// Whether `package` is a toolchain rather than a server.
    #[must_use]
    pub fn is_toolchain(&self, package: &str) -> bool {
        self.toolchains.contains(package)
    }
}

/// What a program must bind to answer on `answering`, given this system's table.
fn bound(bindings: &[PortBinding], answering: u16) -> u16 {
    bindings
        .iter()
        .find(|binding| binding.answer == answering)
        .map_or(answering, |binding| binding.bind)
}

/// Render every file `recipe` declares, for `context`.
///
/// # Errors
///
/// [`Error::TemplateBroken`], naming the file: a template is this build's, so this is a bug of ours
/// and not a configuration a user can fix — but which of a service's six files failed is the first
/// thing anybody needs to know.
pub(super) fn render(recipe: &dyn Recipe, context: &Context) -> Result<Vec<Document>> {
    let mut environment = minijinja::Environment::new();

    // A template that reads `{{ setings.port }}` renders an empty string by default, which is a
    // config file that is silently wrong — a port line with nothing after it. Strict makes it a
    // failure at the moment of rendering, with the name of the variable in it.
    environment.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);

    // Every one of these files is a config format where a trailing newline matters to somebody, and
    // Jinja's default is to eat the last one.
    environment.set_keep_trailing_newline(true);

    // What a program has to bind to answer on a port, as a filter rather than a variable: the two
    // numbers a front end maps come from two different places — `service.port` is the row's and
    // `settings.https_port` is an override — and a variable would have to be added per place.
    // Identity on every system but macOS, and identity there for everything but 80 and 443.
    let bindings = context.bindings.clone();
    environment.add_filter("bound", move |port: i64| -> i64 {
        u16::try_from(port).map_or(port, |answering| i64::from(bound(&bindings, answering)))
    });

    let rendering = minijinja::Value::from_serialize(context.rendering());

    recipe
        .files()
        .iter()
        .map(|file| {
            environment
                .render_str(file.source, &rendering)
                .map(|contents| Document::new(file.path, contents))
                .map_err(|source| Error::TemplateBroken {
                    service: context.service.as_str().to_owned(),
                    file: file.path,
                    source: Box::new(source),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **`msys2` is installable and never runnable** — roadmap task **T206a**, D5.
    #[test]
    fn msys2_is_installable_and_never_runnable() {
        let catalogue = Catalogue::builtin();
        assert!(catalogue.installable().any(|name| name == "msys2"));
        assert!(catalogue.is_toolchain("msys2"));
        assert!(!catalogue.packages().any(|name| name == "msys2"));
        assert!(catalogue.recipe("msys2").is_none());
        assert!(!catalogue.is_toolchain("caddy"));
    }

    /// **Every database names its superuser, and nothing else names anything** — roadmap task
    /// **T82**, the design's D5.
    ///
    /// The account name is the recipe's rather than the manifest's: a `web-app` that wrote `root`
    /// itself would be a manifest that is wrong the day a recipe changes its mind, and T83's
    /// connection handoff has to read the same answer out of the same place.
    #[test]
    fn only_the_databases_name_an_administrator() {
        let catalogue = super::super::Catalogue::builtin();

        for (package, expected) in [
            ("mariadb", Some("root")),
            ("mysql", Some("root")),
            ("postgres", Some("postgres")),
            ("redis", None),
            ("memcached", None),
            ("mongodb", None),
            ("caddy", None),
            ("nginx", None),
            ("php-fpm", None),
        ] {
            let recipe = catalogue
                .recipe(package)
                .unwrap_or_else(|| panic!("{package} is compiled in"));

            assert_eq!(recipe.administrator(), expected, "{package}");
        }
    }

    /// **Only MariaDB asks for a certificate** — roadmap task **T99**.
    ///
    /// MySQL writes its own pair once at `--initialize`, PostgreSQL ships with `ssl` off, and
    /// nothing else here speaks TLS to a client. A recipe that started asking would be a decision
    /// made in that recipe's own design, and this is the test that notices.
    #[test]
    fn only_mariadb_asks_for_a_certificate() {
        let catalogue = super::super::Catalogue::builtin();

        for (package, wants_one) in [
            ("mariadb", true),
            ("mysql", false),
            ("postgres", false),
            ("redis", false),
            ("memcached", false),
            ("mongodb", false),
            ("caddy", false),
            ("nginx", false),
            ("php-fpm", false),
        ] {
            let recipe = catalogue
                .recipe(package)
                .unwrap_or_else(|| panic!("{package} is compiled in"));
            let service = ServiceId::parse(format!("{package}@main")).expect("an id");
            let settings =
                Settings::merge(recipe.settings(), "{}", &service).expect("the defaults merge");
            let context = Context::for_test(
                service,
                package,
                Path::new(root()),
                BTreeMap::new(),
                Some(1),
                settings,
            );

            assert_eq!(
                recipe.certificate(&context).is_some(),
                wants_one,
                "{package}"
            );
        }
    }

    /// **A template can branch on `certificate` and on nothing else that is optional** — roadmap
    /// task **T99**. `service.instance` renders the word `none`; this group renders a falsy value.
    #[test]
    fn a_context_with_no_certificate_renders_a_falsy_group() {
        let service = ServiceId::parse("mariadb@main").expect("an id");
        let settings = Settings::merge(&[], "{}", &service).expect("nothing to merge");
        let context = Context::for_test(
            service,
            "mariadb",
            Path::new(root()),
            BTreeMap::new(),
            Some(3306),
            settings,
        );

        let render = |context: &Context| {
            minijinja::Environment::new()
                .render_str(
                    "{% if certificate %}yes {{ certificate.fingerprint }}{% else %}no{% endif %}",
                    minijinja::Value::from_serialize(context.rendering()),
                )
                .expect("the template renders")
        };

        assert_eq!(render(&context), "no");

        let context = context.with_certificate(ServiceCertificate {
            certificate: Path::new(root()).join("certs/services/mariadb@main.crt"),
            key: Path::new(root()).join("certs/services/mariadb@main.key"),
            fingerprint: "abc123".to_owned(),
        });

        assert_eq!(render(&context), "yes abc123");
    }

    /// An absolute path on whichever system this is compiled for.
    const fn root() -> &'static str {
        if cfg!(windows) {
            r"C:\MixEngine"
        } else {
            "/opt/mixengine"
        }
    }

    /// **While saving resources: an hour for the databases and the caches, half an hour for a pool,
    /// never for a front end** — T70, T70a, and T167b which moved them behind the switch.
    ///
    /// A number that idles a service nothing can start again is a home that broke, so each of these
    /// names only a service something can wake; the two front ends stay [`None`] for ever, because
    /// the thing that starts everything else back up cannot be the thing that gets stopped.
    /// **No recipe idle-stops a service nobody asked about** — ADR 0041. A site that was up stays
    /// up; the numbers above apply only while the home saves resources.
    #[test]
    fn no_recipe_idles_anything_by_default() {
        let catalogue = Catalogue::builtin();

        for package in [
            "php-fpm",
            "mariadb",
            "mysql",
            "postgres",
            "redis",
            "memcached",
            "mongodb",
            "caddy",
            "nginx",
        ] {
            let recipe = catalogue
                .recipe(package)
                .unwrap_or_else(|| panic!("{package} is a builtin recipe"));
            assert_eq!(recipe.idle_default(), None, "{package}");
        }
    }

    #[test]
    fn a_recipe_answers_an_idle_time_for_saving_once_something_can_start_it_again() {
        let catalogue = Catalogue::builtin();

        let default_of = |package: &str| {
            catalogue
                .recipe(package)
                .unwrap_or_else(|| panic!("{package} is a builtin recipe"))
                .idle_when_saving()
        };

        for package in [
            "mariadb",
            "mysql",
            "postgres",
            "redis",
            "memcached",
            "mongodb",
        ] {
            assert_eq!(
                default_of(package),
                Some(mixengine_proto::Millis::from_secs(60 * 60)),
                "{package} was not turned on by the task that made it wakeable"
            );
        }

        assert_eq!(
            default_of("php-fpm"),
            Some(mixengine_proto::Millis::from_secs(30 * 60)),
            "the pool's own default moved"
        );

        for package in ["caddy", "nginx"] {
            assert_eq!(
                default_of(package),
                None,
                "{package} starts everything else back up and must never be stopped"
            );
        }
    }

    /// Every service that listens on a port names the one its product is documented under.
    ///
    /// **A wish belongs to the recipe, not to `service.create`.** Which port MySQL would like is a
    /// fact about MySQL, and a caller that had to know 3306 would be a caller that has to know
    /// every number in the catalogue. Caddy is the deliberate exception: 80 and 443 are its own
    /// settings, and a web server renumbered to 81 because something else answered on 80 is not a
    /// web server anybody asked for.
    #[test]
    fn a_recipe_that_listens_on_a_port_says_which_one_it_would_like() {
        let catalogue = Catalogue::builtin();

        let preferred = |package: &str| {
            catalogue
                .recipe(package)
                .unwrap_or_else(|| panic!("{package} is in the catalogue"))
                .preferred_port()
        };

        assert_eq!(preferred("mariadb"), Some(3306));
        assert_eq!(
            preferred("mysql"),
            Some(3306),
            "the two databases name one number, which is what the allocation is for"
        );
        assert_eq!(preferred("postgres"), Some(5432));
        assert_eq!(preferred("redis"), Some(6379));
        assert_eq!(preferred("memcached"), Some(11211));
        assert_eq!(preferred("php-fpm"), Some(9000));
        assert_eq!(
            preferred("caddy"),
            None,
            "a front end's ports are its own settings"
        );
    }

    /// **T97.** The free function and the method are one join, and this is what keeps them one: a
    /// switch writes a port-80 grant against the first and the supervisor runs the second, so a
    /// suffix spelled in one place and not the other would be a capability on a file nothing runs.
    #[test]
    fn a_package_executable_is_spelled_once_however_it_is_asked_for() {
        let service = ServiceId::parse("nginx").expect("an id");
        let settings = Settings::merge(&[], "{}", &service).expect("no settings, no overrides");
        let context = Context::for_test(
            service,
            "nginx",
            Path::new(root()),
            BTreeMap::new(),
            None,
            settings,
        );

        let install_path = Path::new(root()).join("packages").join("nginx");

        assert_eq!(context.program("nginx"), program(&install_path, "nginx"));
        assert_eq!(
            program(&install_path, "nginx")
                .file_name()
                .and_then(std::ffi::OsStr::to_str),
            Some(if cfg!(windows) { "nginx.exe" } else { "nginx" }),
            "the suffix is this operating system's and never a recipe's"
        );
    }

    /// **A secret never reaches a template.**
    ///
    /// [`Context::rendering`] is what a Jinja template sees, and the secret map is not part of it. A
    /// `my.cnf` with a root password in it would be a plaintext credential on disk written by the
    /// very design that refuses one, so this is a test and not a comment.
    #[test]
    fn a_template_cannot_see_a_secret() {
        let service = ServiceId::parse("mariadb@main").expect("an id");
        let settings = Settings::merge(&[], "{}", &service).expect("no settings, no overrides");
        let mut context = Context::for_test(
            service,
            "mariadb",
            Path::new(root()),
            BTreeMap::new(),
            Some(3306),
            settings,
        );
        context.put_secret("root", "hunter2");

        // Serialised through the very value the renderer hands to minijinja, so what is asserted is
        // what a template can reach and not what this test remembered to look at.
        let rendering =
            serde_json::to_string(&context.rendering()).expect("a rendering serialises");

        assert!(!rendering.contains("hunter2"), "{rendering}");
        assert!(!rendering.contains("secret"), "{rendering}");
        assert_eq!(context.secret("root"), "hunter2", "and a recipe still can");
    }
}

#[cfg(test)]
mod client_tests {
    use super::*;

    /// **Nothing the supervisor runs is a command anybody can type** — roadmap task **T130**.
    ///
    /// [`crate::shims::COMMANDS`] states this for `php-fpm`, and a recipe is the other place it can
    /// be broken: a shim in front of `mariadbd` would be a second way to start a server nothing is
    /// watching, and one in front of `initdb` would be a way to write over a live cluster.
    #[test]
    fn no_recipe_fronts_a_server_or_a_bootstrapper() {
        let owned = [
            "mariadbd",
            "mysqld",
            "postgres",
            "redis-server",
            "caddy",
            "nginx",
            "memcached",
            "mongod",
            "mongos",
            "php-fpm",
            "initdb",
            "pg_ctl",
            "pg_upgrade",
            "mariadb-install-db",
            "mysql_install_db",
        ];

        let catalogue = super::super::Catalogue::builtin();

        for package in catalogue.packages() {
            let recipe = catalogue.recipe(package).expect("a listed recipe");

            for client in recipe.clients() {
                assert!(
                    !owned.contains(&client.executable),
                    "{package} fronts {}, which the supervisor owns",
                    client.executable
                );
            }
        }
    }

    /// **MariaDB is the only recipe with a compatibility spelling**, and each of its three stands
    /// for a program MariaDB itself publishes — an alias for an executable no artifact contains
    /// would be a name in `bin/` that resolves to nothing.
    #[test]
    fn mariadb_is_the_only_recipe_that_stands_in_for_another_product() {
        let catalogue = super::super::Catalogue::builtin();

        for package in catalogue.packages() {
            let recipe = catalogue.recipe(package).expect("a listed recipe");

            let aliases: Vec<(&str, &str)> = recipe
                .clients()
                .iter()
                .filter(|client| client.claim == Claim::Alias)
                .map(|client| (client.name, client.executable))
                .collect();

            if package != "mariadb" {
                assert!(
                    aliases.is_empty(),
                    "{package} claims a name it does not own"
                );
                continue;
            }

            assert_eq!(
                aliases,
                vec![
                    ("mysql", "mariadb"),
                    ("mysqladmin", "mariadb-admin"),
                    ("mysqldump", "mariadb-dump"),
                ]
            );

            let own: Vec<&str> = recipe
                .clients()
                .iter()
                .filter(|client| client.claim == Claim::Own)
                .map(|client| client.executable)
                .collect();

            for (name, executable) in aliases {
                assert!(
                    own.contains(&executable),
                    "{name} stands for {executable}, which mariadb does not publish"
                );
            }
        }
    }

    /// One package must not claim one name twice, and a client's name must be something a file can
    /// be called: both would be found by `bin/` rather than here, and much later.
    #[test]
    fn a_package_claims_each_name_once() {
        let catalogue = super::super::Catalogue::builtin();

        for package in catalogue.packages() {
            let recipe = catalogue.recipe(package).expect("a listed recipe");
            let mut seen = std::collections::BTreeSet::new();

            for client in recipe.clients() {
                assert!(
                    seen.insert(client.name),
                    "{package} claims {} twice",
                    client.name
                );
                assert!(
                    !client.name.contains(['/', '\\', '.']),
                    "{} is not a file name",
                    client.name
                );
            }
        }
    }

    /// **A client is told where its own instance listens** — roadmap task **T130**, the design's
    /// D4, which exists because the port allocator gives 3306 to whichever of the two MySQL-family
    /// products asks first and the next free port above to the other.
    #[test]
    fn a_client_is_told_where_its_instance_listens() {
        let catalogue = super::super::Catalogue::builtin();
        let listen = Upstream::Tcp("127.0.0.1:3307".parse().expect("a socket address"));

        let mariadb = catalogue
            .recipe("mariadb")
            .expect("a builtin recipe")
            .client_env(&listen);
        assert_eq!(
            mariadb.get("MYSQL_HOST").map(String::as_str),
            Some("127.0.0.1")
        );
        assert_eq!(
            mariadb.get("MYSQL_TCP_PORT").map(String::as_str),
            Some("3307")
        );

        let mysql = catalogue
            .recipe("mysql")
            .expect("a builtin recipe")
            .client_env(&listen);
        assert_eq!(
            mysql, mariadb,
            "two products on one wire protocol read one pair of variables"
        );

        let postgres = catalogue
            .recipe("postgres")
            .expect("a builtin recipe")
            .client_env(&listen);
        assert_eq!(postgres.get("PGPORT").map(String::as_str), Some("3307"));

        // Redis publishes a client and no environment for it to read. An empty map is the honest
        // answer; a variable of our own invention would be one `redis-cli` ignores.
        assert!(
            catalogue
                .recipe("redis")
                .expect("a builtin recipe")
                .client_env(&listen)
                .is_empty()
        );
    }

    /// A socket is told to nobody: no client family here has a variable for one, and a recipe that
    /// answered with a TCP port for a socket would be pointing a client at a listener that is not
    /// there.
    #[test]
    fn a_socket_is_told_to_nobody() {
        let catalogue = super::super::Catalogue::builtin();
        let listen = Upstream::Socket(std::path::PathBuf::from("/run/mariadb.sock"));

        for package in catalogue.packages() {
            let recipe = catalogue.recipe(package).expect("a listed recipe");
            assert!(
                recipe.client_env(&listen).is_empty(),
                "{package} named a variable for a socket"
            );
        }
    }
}
