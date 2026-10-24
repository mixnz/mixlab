//! The `extension.toml` manifest.
//!
//! **Its own type rather than a [`ServiceSpec`]**, and not as a matter of taste: `ServiceSpec` has
//! sixteen fields where `[service]` has four, it has no `id` — the manifest's author is not naming
//! a service when they write `program` — and every path and address in the file is a *template*.
//! `{install_dir}/mailpit` is not a `PathBuf` any check would accept and `{listen}:{ui_port}` is
//! not a `SocketAddr` at all, so a `Deserialize` that succeeded on this file would be one that had
//! stopped checking. What the first planner found for `mixengine.toml`, arriving a second time.
//!
//! What *is* shared is the vocabulary — [`EnvValue`], [`StopBehaviour`], [`RestartPolicy`],
//! [`Millis`], [`VersionConstraint`] — which is what [ADR 0006] asked for, and which is why the rule
//! that a spec cannot express a secret by value costs this module nothing to obey: writing a
//! `value` beside `from = "keyring"` is refused by the type, here as everywhere else.
//!
//! Rendering the templates into the spec is [`super::render`].
//!
//! [`ServiceSpec`]: mixengine_proto::ServiceSpec
//! [ADR 0006]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0006-servicespec-in-proto-and-secret-free.md

use std::collections::BTreeMap;
use std::path::Path;

use mixengine_proto::{
    EnvValue, ExtensionId, ExtensionKind, ExtensionPermissions, FrontEndServer, Millis,
    NetworkReach, PackageVersion, ReloadBehaviour, RestartPolicy, RuntimeKind, StopBehaviour,
    VersionConstraint,
};

use crate::{Error, ExtensionError, Result};

/// The only schema this build writes, and the highest one it reads.
pub const SCHEMA: u32 = 1;

/// What the manifest is called, inside the directory that holds an extension.
pub const FILE_NAME: &str = "extension.toml";

/// The placeholders that are not ports.
///
/// The first three are the installer's. The next four arrive with the one document that has a
/// database and a secret behind it — a `[web-app.config]` — and the last is the one placeholder
/// that renders the *name* of an environment variable rather than a value. They are reserved here
/// and not only in the renderer, because a `[ports]` key becomes a placeholder: a port called
/// `db_host` would be a manifest whose own port shadowed the address it was pointed at.
const FIXED_PLACEHOLDERS: [&str; 8] = [
    "install_dir",
    "data_dir",
    "listen",
    "db_host",
    "db_port",
    "db_user",
    "db_password_env",
    "secret",
];

/// An extension, as its manifest says it.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionManifest {
    /// The format version.
    pub schema: u32,

    /// `[extension]`.
    pub extension: Header,

    /// `[artifact.<target>]`, keyed by the target word — `windows-x86_64`, or `any` for something
    /// that runs anywhere.
    pub artifacts: BTreeMap<String, Artifact>,

    /// `[ports]` — the ports it would like, each key its own placeholder.
    ///
    /// **Its own table rather than a field inside `[service]`**, because a port is an installer's
    /// concern: by the time a spec exists it has already been told which number to use. Nothing
    /// here reserves anything — allocation is the installer's.
    pub ports: BTreeMap<String, u16>,

    /// `[permissions]`.
    pub permissions: ExtensionPermissions,

    /// Whichever of the three bodies `kind` names.
    pub body: Body,

    /// `[recipe]`, which may accompany any kind.
    pub recipe: Option<RecipeTable>,

    /// `[ui]`, the page a person opens. A `service`'s only.
    pub ui: Option<UiTable>,
}

/// `[ui]` — the port a person opens in a browser.
///
/// **A port's name and a path, never an address.** The address renders as
/// `http://127.0.0.1:<allocated port><path>`: `127.0.0.1` is where the person's browser is, and a
/// host written in a manifest is refused everywhere else. Only a `service` may carry one; a
/// web-app's page is its site.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct UiTable {
    /// A key of `[ports]`.
    pub port: String,

    /// Where on that port, starting with `/`.
    #[serde(default = "root_path")]
    pub path: String,
}

/// `[ui].path` when a manifest leaves it out.
fn root_path() -> String {
    "/".to_owned()
}

/// `[extension]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    /// What it is called everywhere, and the name of its directory.
    pub id: ExtensionId,

    /// The display name.
    pub name: String,

    /// Its own version, not MixEngine's.
    pub version: PackageVersion,

    /// What it is, which decides which tables are legal.
    pub kind: ExtensionKind,

    /// What it is for, in a sentence.
    #[serde(default)]
    pub description: String,

    /// Where to read about it.
    #[serde(default)]
    pub homepage: Option<String>,
}

/// One `[artifact.<target>]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    /// Where to fetch it. **Verified at install**, not here — reading a manifest downloads nothing.
    pub url: String,

    /// What it must hash to.
    pub sha256: String,

    /// How large it is, in bytes.
    ///
    /// **Optional here where the package index makes it mandatory.** The downloader uses it to stop
    /// a body that never ends before a disk is filled, which the checksum cannot do because it is
    /// only knowable once everything has arrived. A published extension should carry one; an author
    /// pointing `--path` at their own directory should not have to count bytes to try something, so
    /// an absent size falls back to a fixed ceiling
    /// (`extensions::install::UNDECLARED_SIZE_CEILING`) rather than to no bound at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// The three bodies, one per [`ExtensionKind`].
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// A supervised process. Boxed because it is three times the size of the next variant, and
    /// an enum is as big as its largest arm wherever one is held.
    Service(Box<ServiceTemplate>),

    /// Source served on an internal domain.
    WebApp(WebApp),

    /// Nothing but `[recipe]` — which is held beside this rather than inside it, because it may
    /// accompany any kind.
    Recipe,
}

/// `[service]` — a [`ServiceSpec`](mixengine_proto::ServiceSpec) with the templates still in it.
///
/// **What an extension may declare is what its program *is*, never policy about the machine**:
/// `limits` are the machine owner's, an `idle` policy on something nothing can wake is a service
/// that stops for good, `logs` are per-home, and `depends_on` is an edge into a service graph the
/// extension cannot see and a name it would have to guess. Each of those takes the builder's own
/// default — the same answer a compiled-in recipe gets when it says nothing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceTemplate {
    /// The binary. Must grow from `{install_dir}`.
    pub program: String,

    /// The working directory. Must grow from `{install_dir}` or `{data_dir}`.
    pub cwd: String,

    /// Arguments, already split — never a command line to be parsed.
    #[serde(default)]
    pub args: Vec<String>,

    /// The child's environment. [`EnvValue`] whole, so ADR 0006's secret rule is inherited rather
    /// than restated.
    #[serde(default)]
    pub env: BTreeMap<String, EnvValue>,

    /// When traffic may be routed to it.
    pub ready: ReadyTemplate,

    /// Whether it is still fine once it is ready.
    #[serde(default)]
    pub health: Option<HealthTemplate>,

    /// What to do when it exits.
    #[serde(default)]
    pub restart: RestartPolicy,

    /// How to ask it to stop. The `command` form is refused — see [`read`].
    #[serde(default)]
    pub stop: StopBehaviour,

    /// How to hand it a changed configuration. The `command` form is refused — see [`read`].
    #[serde(default)]
    pub reload: Option<ReloadBehaviour>,
}

/// `ready`, with every address and path still a template.
///
/// Mirrors [`ReadyCheck`](mixengine_proto::ReadyCheck) variant for variant. A second type rather
/// than a borrow over the first, because the two are read at different moments: this is what a
/// person wrote, and that is what will be connected to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReadyTemplate {
    /// Connect to an address.
    Tcp {
        /// `{listen}:{<port>}`.
        addr: String,
        /// How long to keep trying.
        timeout: Millis,
    },

    /// Connect to a socket, whose path must grow from a placeholder.
    UnixSocket {
        /// The socket path.
        path: String,
        /// How long to keep trying.
        timeout: Millis,
    },

    /// Fetch a URL, whose host must be `{listen}`.
    Http {
        /// `http://{listen}:{<port>}/…`.
        url: String,
        /// The status that means ready.
        expect_status: u16,
        /// How long to keep trying.
        timeout: Millis,
    },

    /// Wait for a line in its own output.
    LogPattern {
        /// The pattern.
        regex: String,
        /// How long to keep waiting.
        timeout: Millis,
    },

    /// It is ready once it has been running for a moment.
    PidAlive {
        /// How long it has to stay up before it counts.
        settle: Millis,
    },
}

/// `health`, with its probe still a template.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct HealthTemplate {
    /// What to ask.
    pub probe: HealthProbeTemplate,

    /// How often.
    pub interval: Millis,

    /// How long each ask may take.
    pub timeout: Millis,

    /// How many failures make it degraded.
    pub failures_before_degraded: u32,

    /// How many successes bring it back.
    pub successes_before_running: u32,
}

/// What a [`HealthTemplate`] asks, with the address or path still a template.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HealthProbeTemplate {
    /// Connect to an address.
    Tcp {
        /// `{listen}:{<port>}`.
        addr: String,
    },

    /// Connect to a socket.
    UnixSocket {
        /// The socket path.
        path: String,
    },

    /// Fetch a URL.
    Http {
        /// `http://{listen}:{<port>}/…`.
        url: String,
        /// The status that means healthy.
        expect_status: u16,
    },
}

/// `[web-app]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebApp {
    /// The document root. Must grow from `{install_dir}`.
    pub root: String,

    /// One label, placed under the internal domain by whoever generates the site.
    pub domain: String,

    /// The one file MixEngine generates for this application, and the text it is generated from.
    ///
    /// **The text lives here rather than inside the artifact**, where the format's first version
    /// had it as `template = "config.inc.php.tmpl"`. That field said *a file inside the extension*,
    /// and for a registry install the extension's files **are** upstream's archive, verified
    /// against a hash upstream published: there is no step between the download and the rename
    /// where a file of ours could be added without making that hash a hash of something else.
    /// Measured on the real one — phpMyAdmin's `libraries/vendor_config.php` fixes
    /// `'configFile' => ROOT_PATH . 'config.inc.php'`, with no environment override — so the
    /// destination is inside the served root and the source has nowhere else to come from.
    ///
    /// **So an upgrade does not clobber what a person changed**: the generated file is ours and the
    /// settings inside it are theirs, which is the split every other generated file here takes. A
    /// manifest carries the `@include` of the user's own file, under `{data_dir}`, which outlives an
    /// uninstall; nothing here enforces that it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<WebAppConfig>,

    /// The database this application administers, where it needs one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<WebAppDatabase>,

    /// Which language, and which versions of it will do.
    pub runtime: WebAppRuntime,
}

/// `[web-app.config]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebAppConfig {
    /// Where it goes, **relative to [`WebApp::root`]**. Never absolute, and never climbing out —
    /// checked at parse, where every other refusal about this format lives.
    pub path: String,

    /// What is written there, with the placeholders still in it. Rendered through
    /// [`render::php_source`](super::render::php_source), whose destination knows that a `{` it does
    /// not recognise is the language's own punctuation rather than a mistake.
    pub text: String,
}

/// `[web-app.database]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebAppDatabase {
    /// The engines this application can administer, **in order of preference**.
    ///
    /// The first one with a declared service on this machine is the one it is pointed at, and a
    /// machine with none of them is told so before anything is fetched — an extension site's shape
    /// for the PHP, arriving a second time for the database.
    pub engines: Vec<String>,

    /// Whether this application is handed the server's **superuser password**, in its own php-fpm
    /// pool's environment.
    ///
    /// **A boolean, because the variable's name is not an author's to write.** It arrives in
    /// [`CREDENTIAL_ENV`](super::render::CREDENTIAL_ENV), read out of the OS keyring by the
    /// supervisor at the moment the pool is spawned, and the manifest reaches it through
    /// `{db_password_env}` rather than by spelling it — so there is no name to collide with `PATH`
    /// or with `PHP_INI_SCAN_DIR`, and therefore no check anybody could forget. That is the rule an
    /// address already follows (an extension cannot write one, so nothing has to refuse one)
    /// applied to a second field.
    ///
    /// **Off by default**, because a missing value is silence and silence is not consent. Every
    /// surface that shows a plan names the account this would sign in as.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signs_in: bool,
}

/// `[web-app.runtime]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebAppRuntime {
    /// The language.
    pub kind: RuntimeKind,

    /// Which versions of it will do.
    ///
    /// **A constraint and not a pin**: the version a web-app runs on is MixEngine's to choose, and
    /// deliberately not the user's project's — an administrative interface that broke because
    /// somebody pinned their project to an older PHP would be a tool that fails exactly when it is
    /// needed.
    pub requires: VersionConstraint,
}

/// `[recipe]` — what an extension adds to what MixEngine generates.
///
/// **Two forms, and both have a consumer named in the roadmap**: `php_ini` is Mailpit's
/// `sendmail_path`, and `front_end` is the "extra Caddy directives" the feature document names. No
/// third form until something reads one.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeTable {
    /// Settings applied to every managed PHP.
    #[serde(default)]
    pub php_ini: Vec<PhpIniEntry>,

    /// Fragments added to the front end's configuration.
    #[serde(default)]
    pub front_end: Vec<FrontEndFragment>,
}

/// One `[[recipe.php_ini]]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhpIniEntry {
    /// The ini key.
    pub key: String,

    /// Its value, which may carry placeholders.
    pub value: String,
}

/// One `[[recipe.front_end]]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct FrontEndFragment {
    /// Which front end it is written for.
    ///
    /// **Required, and not defaulted to either one.** The two configuration languages are not
    /// interchangeable, so a fragment with no server would be accepted by the parser and be a
    /// syntax error on half the machines that installed it.
    pub server: FrontEndServer,

    /// The directives, which may carry placeholders.
    pub fragment: String,
}

/// Enough of a manifest to refuse a schema this build does not read.
///
/// Read on its own first, the shape `blueprints::manifest::read` takes, so a manifest from a newer
/// build is refused by *version* rather than by whichever unknown key it happens to contain first.
#[derive(serde::Deserialize)]
struct Versioned {
    schema: u32,
    #[serde(default)]
    extension: Option<IdOnly>,
}

impl Versioned {
    /// Whether this build reads that schema.
    ///
    /// # Errors
    ///
    /// [`ExtensionError::UnknownExtensionSchema`], naming the extension where the header got far
    /// enough to say which one it is.
    fn readable(self) -> Result<()> {
        if self.schema > SCHEMA {
            return Err(Error::Extension(ExtensionError::UnknownExtensionSchema {
                id: self.extension.map(|header| header.id).unwrap_or_default(),
                schema: self.schema,
            }));
        }

        Ok(())
    }
}

/// The id, where the header got that far. Not [`Header`], because a newer schema may have changed
/// everything else about it.
#[derive(serde::Deserialize)]
struct IdOnly {
    #[serde(default)]
    id: String,
}

/// The whole file, before `kind` has been checked against the tables beside it.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    schema: u32,
    extension: Header,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    artifact: BTreeMap<String, Artifact>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    ports: BTreeMap<String, u16>,
    #[serde(default)]
    permissions: ExtensionPermissions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    service: Option<ServiceTemplate>,
    #[serde(default, rename = "web-app", skip_serializing_if = "Option::is_none")]
    web_app: Option<WebApp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recipe: Option<RecipeTable>,
    // **Through `Raw` as well as the checked type**: an installed row and a published
    // entry are both this shape, and a `[ui]` dropped here would vanish from both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui: Option<UiTable>,
}

impl From<&ExtensionManifest> for Raw {
    /// **The file's shape, back out of the checked type.**
    ///
    /// A registry entry is a manifest written as JSON, and the document is worth having only if it
    /// is the shape somebody could have written: `[service]` under `service`, `[web-app]` under
    /// `web-app`. Serialising [`Body`] itself would produce a key named after a Rust variant, which
    /// round-trips perfectly and is not a manifest.
    fn from(manifest: &ExtensionManifest) -> Self {
        let (service, web_app) = match &manifest.body {
            Body::Service(template) => (Some((**template).clone()), None),
            Body::WebApp(app) => (None, Some(app.clone())),
            Body::Recipe => (None, None),
        };

        Self {
            schema: manifest.schema,
            extension: manifest.extension.clone(),
            artifact: manifest.artifacts.clone(),
            ports: manifest.ports.clone(),
            permissions: manifest.permissions.clone(),
            service,
            web_app,
            recipe: manifest.recipe.clone(),
            ui: manifest.ui.clone(),
        }
    }
}

/// Read an `extension.toml`.
///
/// Two passes, the shape `blueprints::manifest::read` takes: the schema is read on its own first,
/// so a manifest from a newer build is refused by *version* rather than by whichever unknown key it
/// happens to contain first.
///
/// `path` names the file for the message; nothing is read off disk here.
///
/// # Errors
///
/// [`ExtensionError::ExtensionManifest`] for a parse failure;
/// [`ExtensionError::UnknownExtensionSchema`] for a newer format;
/// [`ExtensionError::ExtensionTableUnexpected`] and [`ExtensionError::ExtensionTableMissing`] where
/// the tables do not match `kind`; [`ExtensionError::ExtensionField`] for a field the format
/// refuses; [`ExtensionError::ExtensionIdTaken`] for an id a compiled-in recipe already claims.
pub fn read(path: &Path, text: &str) -> Result<ExtensionManifest> {
    let failed = |source: toml::de::Error| {
        Error::Extension(ExtensionError::ExtensionManifest {
            path: path.display().to_string(),
            source,
        })
    };

    let versioned: Versioned = toml::from_str(text).map_err(failed)?;
    versioned.readable()?;

    let raw: Raw = toml::from_str(text).map_err(failed)?;
    checked(raw)
}

/// Read one entry of the published registry.
///
/// The same manifest in the same shape, arriving as JSON instead of TOML, and checked by the same
/// code [`read`] checks a file with rather than by a second set of rules: what a `--path` install
/// refuses is what an entry refuses.
///
/// # Errors
///
/// [`ExtensionError::ExtensionEntry`] when the entry is not a manifest at all, and everything
/// [`read`] reports about one that is.
pub fn read_value(entry: serde_json::Value) -> Result<ExtensionManifest> {
    let failed = |source: serde_json::Error| {
        Error::Extension(ExtensionError::ExtensionEntry {
            source: Box::new(source),
        })
    };

    let versioned: Versioned = serde_json::from_value(entry.clone()).map_err(failed)?;
    versioned.readable()?;

    let raw: Raw = serde_json::from_value(entry).map_err(failed)?;
    checked(raw)
}

/// Write a manifest as the registry publishes it, in the shape its file has.
///
/// What the `extensions.manifest_json` column holds and what the packaging repository publishes,
/// out of one rendering rather than two.
#[must_use]
pub fn to_value(manifest: &ExtensionManifest) -> serde_json::Value {
    serde_json::to_value(Raw::from(manifest))
        .expect("a manifest is made of strings, numbers and maps")
}

/// Everything a manifest is checked for once it has been read, whichever format it arrived in.
fn checked(raw: Raw) -> Result<ExtensionManifest> {
    let id = raw.extension.id.clone();
    let kind = raw.extension.kind;

    if crate::generate::Catalogue::builtin()
        .packages()
        .any(|package| package == id.as_str())
    {
        return Err(Error::Extension(ExtensionError::ExtensionIdTaken {
            id: id.as_str().to_owned(),
        }));
    }

    let body = body(&id, kind, raw.service, raw.web_app)?;

    for name in raw.ports.keys() {
        placeholder_name(&id, name)?;
    }

    check_service(&id, &body)?;
    check_reach(&id, kind, raw.permissions.network)?;
    check_label(&id, &body)?;
    check_config(&id, &body)?;
    check_ui(&id, kind, &raw.ports, raw.ui.as_ref())?;

    Ok(ExtensionManifest {
        schema: raw.schema,
        extension: raw.extension,
        artifacts: raw.artifact,
        ports: raw.ports,
        permissions: raw.permissions,
        body,
        recipe: raw.recipe,
        ui: raw.ui,
    })
}

/// `[ui]`'s rules.
///
/// # Errors
///
/// [`ExtensionError::ExtensionTableUnexpected`] for a `[ui]` on anything but a `service`, and
/// [`ExtensionError::ExtensionField`] naming `ui.port` or `ui.path`.
fn check_ui(
    id: &ExtensionId,
    kind: ExtensionKind,
    ports: &BTreeMap<String, u16>,
    ui: Option<&UiTable>,
) -> Result<()> {
    let Some(ui) = ui else {
        return Ok(());
    };

    if kind != ExtensionKind::Service {
        return Err(Error::Extension(ExtensionError::ExtensionTableUnexpected {
            id: id.as_str().to_owned(),
            kind: kind.as_str(),
            table: "ui",
        }));
    }

    if !ports.contains_key(&ui.port) {
        return Err(Error::Extension(ExtensionError::ExtensionField {
            id: id.as_str().to_owned(),
            field: "ui.port".to_owned(),
            reason: format!("names `{}`, which `[ports]` does not declare", ui.port),
        }));
    }

    if !ui.path.starts_with('/') {
        return Err(Error::Extension(ExtensionError::ExtensionField {
            id: id.as_str().to_owned(),
            field: "ui.path".to_owned(),
            reason: "has to start with `/`".to_owned(),
        }));
    }

    Ok(())
}

/// Pair `kind` with the one table it is allowed, refusing the other.
fn body(
    id: &ExtensionId,
    kind: ExtensionKind,
    service: Option<ServiceTemplate>,
    web_app: Option<WebApp>,
) -> Result<Body> {
    let present: [(&'static str, bool); 2] = [
        ("service", service.is_some()),
        ("web-app", web_app.is_some()),
    ];

    let own = match kind {
        ExtensionKind::Service => Some("service"),
        ExtensionKind::WebApp => Some("web-app"),
        ExtensionKind::Recipe => None,
    };

    for (table, is_present) in present {
        if is_present && own != Some(table) {
            return Err(Error::Extension(ExtensionError::ExtensionTableUnexpected {
                id: id.as_str().to_owned(),
                kind: kind.as_str(),
                table,
            }));
        }
    }

    let missing = |table: &'static str| {
        Error::Extension(ExtensionError::ExtensionTableMissing {
            id: id.as_str().to_owned(),
            kind: kind.as_str(),
            table,
        })
    };

    match kind {
        ExtensionKind::Service => service
            .map(|template| Body::Service(Box::new(template)))
            .ok_or_else(|| missing("service")),
        ExtensionKind::WebApp => web_app.map(Body::WebApp).ok_or_else(|| missing("web-app")),
        ExtensionKind::Recipe => Ok(Body::Recipe),
    }
}

/// The rules that are about `[service]` alone.
fn check_service(id: &ExtensionId, body: &Body) -> Result<()> {
    let Body::Service(template) = body else {
        return Ok(());
    };

    let refuse = |field: &str| {
        Err(Error::Extension(ExtensionError::ExtensionField {
            id: id.as_str().to_owned(),
            field: field.to_owned(),
            reason: "may not be a command: a second program is a second path to render, and \
                     nothing yet needs one"
                .to_owned(),
        }))
    };

    if matches!(template.stop, StopBehaviour::Command { .. }) {
        return refuse("stop");
    }

    if matches!(template.reload, Some(ReloadBehaviour::Command { .. })) {
        return refuse("reload");
    }

    Ok(())
}

/// `web-app` may not ask for the LAN.
fn check_reach(id: &ExtensionId, kind: ExtensionKind, network: NetworkReach) -> Result<()> {
    if kind == ExtensionKind::WebApp && network == NetworkReach::Lan {
        return Err(Error::Extension(ExtensionError::ExtensionField {
            id: id.as_str().to_owned(),
            field: "permissions.network".to_owned(),
            reason: "may not be `lan` for a web-app: these are administrative interfaces onto \
                     this machine's own databases, and the difference between one of them and a \
                     site somebody chose to share is that nobody chose"
                .to_owned(),
        }));
    }

    Ok(())
}

/// `[web-app].domain` is **one label**.
///
/// The site that serves the label composes `<label>.mixengine.<tld>`, so a dotted value would
/// quietly become a deeper name. Refused at parse, where every other refusal about this format
/// lives.
fn check_label(id: &ExtensionId, body: &Body) -> Result<()> {
    let Body::WebApp(app) = body else {
        return Ok(());
    };

    let refuse = |reason: String| {
        Err(Error::Extension(ExtensionError::ExtensionField {
            id: id.as_str().to_owned(),
            field: "web-app.domain".to_owned(),
            reason,
        }))
    };

    if app.domain.contains('.') {
        return refuse(
            "is one label, placed under the internal domain by the site that serves it, so it may \
             not contain `.`"
                .to_owned(),
        );
    }

    let composed = format!(
        "{}.mixengine.{}",
        app.domain,
        mixengine_proto::domains::DEFAULT_TLD
    );

    match mixengine_proto::domains::domain_syntax(&composed) {
        Some(because) => refuse(format!("does not make a domain: {because}")),
        None => Ok(()),
    }
}

/// `[web-app.config].path` stays under the root, and `[web-app.database].engines` names something.
///
/// **Joined rather than rendered**, which is why this check is here and not in
/// [`render::rooted`](super::render::rooted): the path carries no placeholders and is appended to a
/// root that has already been checked, so what is left to refuse is a value that would leave that
/// root — an absolute path, or one climbing out with `..`.
fn check_config(id: &ExtensionId, body: &Body) -> Result<()> {
    let Body::WebApp(app) = body else {
        return Ok(());
    };

    let refuse = |field: &str, reason: &str| {
        Err(Error::Extension(ExtensionError::ExtensionField {
            id: id.as_str().to_owned(),
            field: field.to_owned(),
            reason: reason.to_owned(),
        }))
    };

    if let Some(config) = &app.config {
        let path = Path::new(&config.path);

        if config.path.is_empty() {
            return refuse(
                "web-app.config.path",
                "is empty, and a generated file has to be called something",
            );
        }

        if path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        }) {
            return refuse(
                "web-app.config.path",
                "must stay under `web-app.root`: it is where MixEngine writes a file for this \
                 application, and an extension reaches the paths it was handed and no others",
            );
        }
    }

    if let Some(database) = &app.database
        && database.engines.is_empty()
    {
        return refuse(
            "web-app.database.engines",
            "is empty: a web-app that declares a database says which engines it can administer, \
             and a list naming none of them declares nothing",
        );
    }

    // **A manifest that asks for a database superuser's password has to read it.** The only
    // justification for putting that credential into a process is using it, so a `signs_in` nothing
    // reads is a person agreeing to something for nothing.
    //
    // The one rule in this format that spans two tables, and it is here rather than in the renderer
    // because a renderer only ever sees the half it was handed.
    if app
        .database
        .as_ref()
        .is_some_and(|database| database.signs_in)
        && !app
            .config
            .as_ref()
            .is_some_and(|config| config.text.contains(super::render::PASSWORD_ENV))
    {
        return Err(Error::Extension(ExtensionError::ExtensionField {
            id: id.as_str().to_owned(),
            field: "web-app.database.signs_in".to_owned(),
            reason: format!(
                "is set and no `[web-app.config].text` reads {}: a superuser password put into a \
                 process that never reads it is consent bought for nothing",
                super::render::PASSWORD_ENV
            ),
        }));
    }

    Ok(())
}

/// A `[ports]` key becomes a placeholder, so it has to be spellable as one — and it may not be one
/// of the three the renderer already answers.
fn placeholder_name(id: &ExtensionId, name: &str) -> Result<()> {
    let refuse = |reason: &str| {
        Err(Error::Extension(ExtensionError::ExtensionField {
            id: id.as_str().to_owned(),
            field: format!("ports.{name}"),
            reason: reason.to_owned(),
        }))
    };

    if name.is_empty() {
        return refuse("is empty, and a port's name is its placeholder");
    }

    if FIXED_PLACEHOLDERS.contains(&name) {
        return refuse("is already a placeholder that means something else");
    }

    if !name.starts_with(|character: char| character.is_ascii_lowercase()) {
        return refuse("must start with a lowercase letter, because it is a placeholder name");
    }

    if !name.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
    }) {
        return refuse("may only contain lowercase letters, digits and `_`");
    }

    Ok(())
}

#[cfg(test)]
mod tests;
