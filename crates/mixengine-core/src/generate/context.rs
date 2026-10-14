//! What a recipe is handed: one service's row, settings, paths and certificate, and the
//! view of them a template renders from. Everything a recipe knows about *this* instance
//! rather than about the kind of service it is — see [`super::recipe`] for that half.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mixengine_platform::PortBinding;
use mixengine_proto::ServiceId;
use serde::Serialize;

use super::recipe::{Endpoints, FrontEndAddition, ServiceCertificate, program};
use super::settings::Settings;
use crate::ServiceError;
use crate::{Error, Result};

/// Everything a template and a [`Recipe`] are told about one service instance.
///
/// Assembled by [`Generator`](super::Generator) from the `services` row, the `packages` row it
/// points at and the home's layout, so that neither a template nor a recipe reads the database or
/// joins a path of its own.
///
/// The fields are `pub(super)` and there is no constructor: the only thing that may build one is
/// [`Generator`](super::Generator), which is the only thing that has read the row. A recipe reads it
/// through the accessors below.
///
/// [`Recipe`]: super::Recipe
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
    ///
    /// [`Recipe::endpoints`]: super::Recipe::endpoints
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
    ///
    /// [`Recipe::fragments`]: super::Recipe::fragments
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
    ///
    /// [`Recipe::certificate`]: super::Recipe::certificate
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
    ///
    /// [`Recipe::spec`]: super::Recipe::spec
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
    /// [`ServiceError::ServiceProvidesNothing`], naming the service and listing what the install
    /// does publish — which is the whole of what somebody looking at a PHP packed without a SAPI
    /// needs.
    pub fn provided(&self, name: &str) -> Result<PathBuf> {
        self.provides
            .get(name)
            .map(|relative| crate::paths::join_stored(&self.install_path, relative))
            .ok_or_else(|| {
                Error::Service(ServiceError::ServiceProvidesNothing {
                    service: self.service.as_str().to_owned(),
                    executable: name.to_owned(),
                    known: self.provides.keys().cloned().collect(),
                })
            })
    }

    /// Where this service listens on a Unix socket, if it does.
    ///
    /// Computed once by [`Recipe::endpoints`] and read back here, rather than joined again: the file
    /// and the daemon's own check have to name one path, and two places that build it are two places
    /// for it to drift.
    ///
    /// [`Recipe::endpoints`]: super::Recipe::endpoints
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
    pub(super) fn rendering(&self) -> Rendering<'_> {
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
pub(super) struct Rendering<'a> {
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

/// What a program must bind to answer on `answering`, given this system's table.
pub(super) fn bound(bindings: &[PortBinding], answering: u16) -> u16 {
    bindings
        .iter()
        .find(|binding| binding.answer == answering)
        .map_or(answering, |binding| binding.bind)
}
