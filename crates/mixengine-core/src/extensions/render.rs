//! Turning a manifest's templates into the thing that would run — roadmap task **T80**.
//!
//! **The manifest never writes an address** (the T80 design, D2). [`text`] substitutes `{listen}`
//! from `permissions.network` and from nothing else, and a host written out anywhere in the file is
//! refused before substitution — including `127.0.0.1`, which is the one an author would write in
//! good faith. So an extension that declared `loopback` has no way to spell any other address:
//! there is no check here that a later feature could forget to consult, because there is nothing to
//! check.
//!
//! **And every path grows from a placeholder** (D4), which is what `filesystem = ["own-data"]`
//! *is*. An extension cannot reach a path it was not handed, because it cannot write one down.
//!
//! The refusals are checked against what the *author* wrote rather than against the rendering,
//! because after substitution `{listen}` has become an address and the two would be
//! indistinguishable.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::{Component, PathBuf};

use mixengine_proto::{
    EnvValue, ExtensionId, HealthCheck, HealthProbe, ReadyCheck, ServiceSpec, ServiceSpecBuilder,
};

use super::manifest::{
    ExtensionManifest, HealthProbeTemplate, HealthTemplate, ReadyTemplate, ServiceTemplate,
};
use crate::{Error, ExtensionError, Paths, Result};

/// The three placeholders that are not ports, spelled as they appear in a file.
pub const INSTALL_DIR: &str = "{install_dir}";

/// See [`INSTALL_DIR`].
pub const DATA_DIR: &str = "{data_dir}";

/// `{db_password_env}`, spelled as it appears in a file — roadmap task **T82a**, the design's D2.
///
/// **The one placeholder that renders the name of an environment variable rather than a value.**
/// The password itself never reaches this crate: a pool's spec carries an `EnvValue::Keyring` and
/// the supervisor reads it at spawn, so what a generated `config.inc.php` holds is the address of a
/// credential in exactly the sense that spec does.
pub const PASSWORD_ENV: &str = "{db_password_env}";

/// What [`PASSWORD_ENV`] renders to.
///
/// Declared in [`pools`](super::pools), where the pool that carries it is, and re-exported here
/// because the placeholder and the name it produces belong together for a reader.
pub use super::pools::CREDENTIAL_ENV;

/// The scheme prefixes a readiness or health url may start with, each followed by `{listen}`.
const URL_PREFIXES: [&str; 2] = ["http://{listen}", "https://{listen}"];

/// Where one extension's placeholders point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// `{install_dir}` — where the extension's own files are.
    pub install_dir: PathBuf,

    /// `{data_dir}` — where what it writes goes.
    pub data_dir: PathBuf,

    /// Each port placeholder, by the name `[ports]` gave it.
    pub ports: BTreeMap<String, u16>,

    /// `{listen}`.
    pub listen: IpAddr,

    /// Where this `web-app`'s database is, for `{db_host}`, `{db_port}` and `{db_user}` — roadmap
    /// task **T82**, the design's D5.
    ///
    /// [`None`] everywhere a configuration is not being written: a plan has resolved nothing yet,
    /// and a `service` extension has no database at all.
    pub database: Option<DatabaseEndpoint>,

    /// `{secret}` — roadmap task **T82**, the design's D7.
    ///
    /// **Supplied by the daemon and never read here.** This crate has no business reaching a
    /// credential store (`generate::databases`' D1), so the value arrives already read out of the
    /// keyring, which is the same shape a recipe's declared secrets take.
    pub secret: Option<String>,

    /// `{db_password_env}` — the variable a `web-app` declaring `signs_in` reads its password from,
    /// roadmap task **T82a**, the design's D2.
    ///
    /// **A name and never a value.** The password itself is never in this crate: the pool's spec
    /// carries an `EnvValue::Keyring` and the supervisor reads it at spawn. [`None`] wherever the
    /// manifest did not ask, which is what makes the placeholder refuse rather than render nothing.
    pub password_env: Option<String>,
}

/// Where a `web-app`'s database is, and what to call its superuser — roadmap task **T82**, the
/// design's D5.
///
/// **Three fields and no fourth.** There is no socket, because every database recipe binds a TCP
/// port on all three systems — T34c's *"a pool gets its 9000 the same way `mariadb@main` gets its
/// 3306"* — so one address form is enough, and a placeholder rendering to nothing on Windows is a
/// template nobody could write once. There is no password, because the design's D6 says a generated
/// file does not carry one: where the credential lives stays
/// [`Context::secret_address`](crate::generate::context::Context::secret_address)'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseEndpoint {
    /// Which service answers — the row `site_service_links` froze at install.
    pub service: mixengine_proto::ServiceId,

    /// `{db_host}`.
    pub host: IpAddr,

    /// `{db_port}`.
    pub port: u16,

    /// `{db_user}` — the recipe's
    /// [`administrator`](crate::generate::recipe::Recipe::administrator), never the manifest's
    /// guess.
    pub user: String,
}

impl Context {
    /// The context an install on this machine *would* build.
    ///
    /// **The ports here are what the extension asked for, not what it has.** Allocation is T81's,
    /// through [`Port::Allocate`](crate::services::Port), and nothing is reserved by rendering —
    /// a line that read like a reservation is how somebody concludes a port is held.
    #[must_use]
    pub fn planned(paths: &Paths, manifest: &ExtensionManifest) -> Self {
        let id = manifest.extension.id.as_str();

        Self {
            install_dir: paths.extensions().join(id),
            // **Not under the install directory** — roadmap task **T81**, its design's D13. An
            // uninstall removes `install_dir` whole and keeps this unless somebody asks otherwise,
            // which is a promise a `data` nested inside it could not keep; the alternative, deleting
            // everything under the install directory except one child, is a rule to remember at
            // every future site that removes an extension. Two directories, two lifetimes: one
            // belongs to the version installed and goes with it, the other belongs to the person and
            // outlives every upgrade.
            data_dir: paths.data().join("extensions").join(id),
            ports: manifest.ports.clone(),
            listen: manifest.permissions.network.listen_address(),
            // A plan resolves no database and creates no secret: both are decided when the rows are
            // written, which is after this, and a plan that answered them would be describing an
            // install that has not happened. `password_env` is the manifest's own answer and is
            // still left here, because only `config::rendered` holds both halves of it — the
            // declaration and the document it is substituted into.
            database: None,
            secret: None,
            password_env: None,
        }
    }

    /// The context an extension that **is** installed renders under.
    ///
    /// **The row's ports, not the manifest's.** `[ports]` is what an extension asked for; what it
    /// holds is in `extension_ports`, and rendering the wish would produce a spec that binds a port
    /// somebody else was given.
    ///
    /// The database, the secret and the password variable are left for the caller to fill: a
    /// `ServiceSpec` needs none of them, and only [`config`](super::config) has read the link row,
    /// the keyring, or the `[web-app.database]` that asks for the third.
    #[must_use]
    pub fn installed(installed: &super::store::Installed) -> Self {
        Self {
            install_dir: installed.install_dir.clone(),
            data_dir: installed.data_dir.clone(),
            ports: installed.ports.clone(),
            listen: installed.manifest.permissions.network.listen_address(),
            database: None,
            secret: None,
            password_env: None,
        }
    }

    /// What one placeholder stands for, and whether what it stands for is a path.
    ///
    /// The second half is why this returns a pair: a manifest writes `{install_dir}/mailpit` with
    /// the slash every author types, and on a system that spells a path with backslashes the raw
    /// substitution is `C:\\…\\mailpit/mailpit` — which works, and reads like a bug in every
    /// rendering, log line and generated file it lands in. See [`text`].
    fn placeholder(&self, name: &str) -> Option<(String, bool)> {
        match name {
            "install_dir" => Some((self.install_dir.display().to_string(), true)),
            "data_dir" => Some((self.data_dir.display().to_string(), true)),
            "listen" => Some((self.listen.to_string(), false)),
            // **Before the ports, so a `[ports]` key can never shadow one.** The parse already
            // refuses such a key (`manifest::FIXED_PLACEHOLDERS`); this is the renderer agreeing
            // rather than depending on it.
            "db_host" => self
                .database
                .as_ref()
                .map(|db| (db.host.to_string(), false)),
            "db_port" => self
                .database
                .as_ref()
                .map(|db| (db.port.to_string(), false)),
            "db_user" => self.database.as_ref().map(|db| (db.user.clone(), false)),
            "secret" => self.secret.clone().map(|secret| (secret, false)),
            // **A name, and the only placeholder that renders one** — roadmap task **T82a**, D2.
            // Answered from the manifest's `signs_in` rather than from anything this home holds,
            // which is why it is [`None`] on a context nobody asked to configure.
            "db_password_env" => self.password_env.clone().map(|name| (name, false)),
            port => self
                .ports
                .get(port)
                .map(|number| (number.to_string(), false)),
        }
    }
}

/// Substitute every placeholder in one field, refusing any the vocabulary does not contain.
///
/// # Errors
///
/// [`ExtensionError::ExtensionField`] naming the field and the placeholder. An unknown one is
/// refused rather than left standing, because a `{home_dir}` that survived into an argument would
/// be a literal brace handed to a program, which is a bug reported by whatever that program does
/// with it.
pub fn text(id: &ExtensionId, field: &str, template: &str, context: &Context) -> Result<String> {
    spelled(id, field, template, context, Destination::Field)
}

/// The placeholders a `[web-app.config]` may use that only a written configuration can answer.
const ANSWERED_ONLY_WHEN_CONFIGURING: [&str; 5] =
    ["db_host", "db_port", "db_user", "secret", "db_password_env"];

/// What a reader is told about one of [`ANSWERED_ONLY_WHEN_CONFIGURING`] that nothing answers.
///
/// **Named per placeholder where the field that would supply it is not obvious** — roadmap task
/// **T82a**. Four of the five are answered by a home that has a database and a secret, so "this
/// home has nothing to answer it with" is the whole story; `{db_password_env}` is answered by a
/// *manifest* field one line further up the same file, and saying which field is the difference
/// between a refusal an author can act on and one they have to read this crate to understand.
fn unanswered(name: &str) -> String {
    match name {
        "db_password_env" => {
            format!("uses `{{{name}}}`, and `[web-app.database].signs_in` is what supplies it")
        }
        other => format!("uses `{{{other}}}`, and this home has nothing to answer it with"),
    }
}

/// Substitute a `[web-app.config]` text — roadmap task **T82**, the design's D1 and D8.
///
/// [`Destination::PhpSource`] copies a `{…}` it does not recognise, because PHP's own braces
/// outnumber ours in any real file. **What is still refused is a placeholder of ours with nothing
/// behind it**: left standing, `{db_host}` reaches PHP as a literal and the application connects to
/// a host called that — reported as a database that is down, five files from anything that looks
/// wrong.
///
/// # Errors
///
/// [`ExtensionError::ExtensionField`] naming the field, for a `{db_*}` or `{secret}` this context
/// cannot answer. For four of the five that is a context built without them rather than a manifest
/// that is wrong, which is why the message says what this home has rather than what the author
/// should have written; `{db_password_env}` is the exception and names the field that supplies it,
/// because that field is a manifest's and not a home's.
pub fn php_source(
    id: &ExtensionId,
    field: &str,
    template: &str,
    context: &Context,
) -> Result<String> {
    for name in ANSWERED_ONLY_WHEN_CONFIGURING {
        if template.contains(&format!("{{{name}}}")) && context.placeholder(name).is_none() {
            return Err(refuse(id, field, &unanswered(name)));
        }
    }

    spelled(id, field, template, context, Destination::PhpSource)
}

/// Where a substitution is landing — roadmap task **T81c**, the design's D4.
///
/// Two things a placeholder cannot decide for itself, and both of them are properties of the
/// document rather than of the extension.
///
/// **How a path is spelled.** The same `{install_dir}` is a command-line argument in one field and a
/// directive in a generated `nginx.conf` in another, and nginx does not read a Windows path:
/// `ngx_conf_read_token` treats `\` inside a quoted string as an escape, so
/// `C:\Users\Nguyen Hai Quang` arrives with every separator eaten. nginx accepts `/` on all three
/// systems, which is why `nginx.conf` is rendered through a `replace` filter and why a fragment
/// substituted into it takes the same spelling.
///
/// **Whether a `{…}` that is not ours is a mistake.** In a field whose whole content is MixEngine's
/// vocabulary it is: `{home_dir}` surviving into an argument is a literal brace handed to a program,
/// reported as whatever that program does with it. In a *fragment* it is not, and this is the one
/// place the T80 rule does not hold — a brace is the destination language's own punctuation.
/// `location / { return 404; }` is nginx, `handle { file_server }` is Caddy, and `{host}` is a Caddy
/// placeholder spelled exactly the way ours are. Refusing what we do not recognise would refuse
/// every real fragment; so an unrecognised `{…}` is copied verbatim, and what takes over the job of
/// catching a misspelling is the front end's own parser, run over the fragment before the install is
/// allowed to proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// A field whose whole content is this vocabulary: an argument, an environment value, a
    /// `php.ini` value. This system's separator, and an unknown placeholder is refused.
    Field,

    /// A `[[recipe.front_end]]` fragment written for a Caddyfile. This system's separator — a
    /// backtick-quoted Caddyfile token takes a Windows path as it is written.
    Caddyfile,

    /// A `[[recipe.front_end]]` fragment written for an `nginx.conf`. Forward slashes.
    NginxConf,

    /// A `[web-app.config]` text, which is PHP source — roadmap task **T82**, the design's D8.
    ///
    /// Forward slashes, because PHP accepts them on Windows and a backslash inside a single-quoted
    /// PHP string is a character the reader has to reason about. An unrecognised `{…}` is copied
    /// verbatim for T81c's reason arriving a second time: `function f() {` opens a brace before the
    /// first placeholder, and refusing what we do not recognise would refuse every real file.
    ///
    /// What takes over from the refusal is the same thing that took it over for a fragment — the
    /// real parser. Here that is the PHP runtime, which is why this feature's testing rests on a
    /// real run rather than on a renderer that agrees with itself. The one refusal
    /// [`php_source`] keeps is a placeholder of *ours* with nothing behind it.
    PhpSource,
}

impl Destination {
    /// The character a separator is written as here.
    const fn separator(self) -> char {
        match self {
            Self::Field | Self::Caddyfile => std::path::MAIN_SEPARATOR,
            Self::NginxConf | Self::PhpSource => '/',
        }
    }

    /// Whether a `{…}` this vocabulary does not contain is a mistake or the destination's own.
    const fn unknown_is_a_mistake(self) -> bool {
        matches!(self, Self::Field)
    }

    /// One substituted path value, respelled.
    ///
    /// Applied to the *value* rather than to the whole rendering, and that is the point: a fragment
    /// is arbitrary configuration, and an nginx `location ~ \.php$` is a backslash that means
    /// something. Only the text a placeholder produced is a path.
    fn spell(self, value: String) -> String {
        match self {
            Self::Field | Self::Caddyfile => value,
            Self::NginxConf | Self::PhpSource => value.replace('\\', "/"),
        }
    }
}

impl From<mixengine_proto::FrontEndServer> for Destination {
    fn from(server: mixengine_proto::FrontEndServer) -> Self {
        match server {
            mixengine_proto::FrontEndServer::Caddy => Self::Caddyfile,
            mixengine_proto::FrontEndServer::Nginx => Self::NginxConf,
        }
    }
}

/// [`text`], told where what it renders is going.
fn spelled(
    id: &ExtensionId,
    field: &str,
    template: &str,
    context: &Context,
    destination: Destination,
) -> Result<String> {
    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;

    // Whether what is being copied is the tail of a path that began at `{install_dir}` or
    // `{data_dir}`. While it is, a `/` the author typed is written the way the destination writes a
    // separator; whitespace ends the path, so `{install_dir}/mailpit sendmail --addr a/b` converts
    // the first slash and leaves the argument after it alone. On a system whose separator already
    // *is* `/` every branch of this is a copy.
    let mut in_path = false;

    while let Some(open) = rest.find('{') {
        copy(&mut rendered, &rest[..open], &mut in_path, destination);
        let after = &rest[open + 1..];

        let Some(close) = after.find('}') else {
            if destination.unknown_is_a_mistake() {
                return Err(refuse(id, field, "has a `{` with no `}` after it"));
            }

            rendered.push('{');
            in_path = false;
            rest = after;
            continue;
        };

        let name = &after[..close];

        match context.placeholder(name) {
            Some((value, is_path)) => {
                match is_path {
                    true => {
                        rendered.push_str(&destination.spell(value));
                        in_path = true;
                    }
                    false => rendered.push_str(&value),
                }

                rest = &after[close + 1..];
            }
            None if destination.unknown_is_a_mistake() => {
                return Err(refuse(
                    id,
                    field,
                    &format!(
                        "uses `{{{name}}}`, which is not a placeholder this manifest declares"
                    ),
                ));
            }

            // **The brace alone, and the scan continues one character on** — not past the `}` that
            // was looked for. A block's `{` is the common case here (`server {`, `handle {`), and
            // the nearest `}` after one is nowhere near it: in
            // `server {\n    listen {listen}:{port};` it is the closing brace of `{listen}`, so
            // consuming through it swallows a real placeholder and leaves `{listen}` in an
            // `nginx.conf` — which nginx reports as a `listen` directive with no `;`, five lines
            // from anything that looks wrong. Found by the real server in T81c, after two unit
            // tests missed it: re-emitting the swallowed text is *lossless* whenever nothing
            // inside it needed substituting, so a fragment that opens a block after its
            // placeholders renders correctly by luck.
            //
            // A brace also ends whatever path was being copied — nothing MixEngine handed out
            // continues through one.
            None => {
                rendered.push('{');
                in_path = false;
                rest = after;
            }
        }
    }

    copy(&mut rendered, rest, &mut in_path, destination);

    Ok(rendered)
}

/// Copy literal text, spelling a separator the destination's way while inside a path.
fn copy(rendered: &mut String, literal: &str, in_path: &mut bool, destination: Destination) {
    for character in literal.chars() {
        if character.is_whitespace() {
            *in_path = false;
        }

        match *in_path && character == '/' {
            true => rendered.push(destination.separator()),
            false => rendered.push(character),
        }
    }
}

/// Substitute a path, insisting it grew from one of `allowed` and climbs out of nothing.
///
/// This function is what `filesystem = ["own-data"]` *is* (the T80 design, D4): an extension
/// reaches the paths it was handed because it has no way to write down any other.
///
/// # Errors
///
/// [`ExtensionError::ExtensionField`] naming the field, for a path that starts somewhere else or
/// climbs out with `..`.
pub fn rooted(
    id: &ExtensionId,
    field: &str,
    template: &str,
    allowed: &[&str],
    context: &Context,
) -> Result<PathBuf> {
    if !allowed.iter().any(|root| template.starts_with(root)) {
        return Err(refuse(
            id,
            field,
            &format!(
                "must start with {}: an extension reaches the paths it was handed and no others",
                allowed.join(" or ")
            ),
        ));
    }

    let path = PathBuf::from(text(id, field, template, context)?);

    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(refuse(
            id,
            field,
            "climbs out of the directory it starts in with `..`",
        ));
    }

    // **Rebuilt from its components, which normalises the separator.** A manifest writes
    // `{install_dir}/mailpit` with the slash every author types, and on Windows the placeholder
    // renders with backslashes — so the raw substitution is `C:\…\mailpit/mailpit`, which works and
    // reads like a bug in every rendering, log line and generated file it appears in. A *path* is
    // MixEngine's to spell; an argument is not, and `args` is left exactly as it will be passed.
    Ok(path.components().collect())
}

/// Whether a template writes out a host itself.
///
/// Run on what the author wrote, never on the rendering. Two shapes are looked for: anything that
/// parses as an IP address — on its own, or as the host half of a `host:port` — and the word
/// `localhost`. Both are found inside a longer string, because `http://127.0.0.1:8025/health` is
/// the same address written where a scanner splitting on whitespace would miss it.
fn names_an_address(template: &str) -> bool {
    let hostish =
        |character: char| character.is_ascii_hexdigit() || character == ':' || character == '.';

    let runs = template.split(|character: char| !hostish(character));

    for run in runs {
        if run.parse::<IpAddr>().is_ok() {
            return true;
        }

        if let Some((host, _)) = run.rsplit_once(':')
            && host.parse::<IpAddr>().is_ok()
        {
            return true;
        }
    }

    template
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '-'))
        .any(|word| word.eq_ignore_ascii_case("localhost"))
}

/// Substitute a field that may not name a host: an argument, an environment value, a recipe's own
/// value.
///
/// # Errors
///
/// [`ExtensionError::ExtensionField`] naming the field, for a written-out address or an unknown
/// placeholder.
pub fn value(id: &ExtensionId, field: &str, template: &str, context: &Context) -> Result<String> {
    no_address(id, field, template)?;

    text(id, field, template, context)
}

/// Substitute a `[[recipe.front_end]]` fragment — roadmap task **T81c**.
///
/// [`value`] pointed at a configuration language rather than at a field of ours — see
/// [`Destination`] for the two things that changes — and held to the same address rule for the same
/// reason: a fragment lands in the configuration of the one process every site on this machine is
/// reached through, so an author who could write a host there could reach further from the front end
/// than the permission they declared reaches from their own program.
///
/// # Errors
///
/// [`ExtensionError::ExtensionField`] naming the field, for a written-out address.
pub fn fragment(
    id: &ExtensionId,
    field: &str,
    template: &str,
    context: &Context,
    destination: Destination,
) -> Result<String> {
    no_address(id, field, template)?;

    spelled(id, field, template, context, destination)
}

/// Refuse a template that writes out a host.
fn no_address(id: &ExtensionId, field: &str, template: &str) -> Result<()> {
    if names_an_address(template) {
        return Err(refuse(
            id,
            field,
            "writes out an address: an extension declares how far it reaches with \
             `permissions.network`, and `{listen}` is where the address comes from",
        ));
    }

    Ok(())
}

/// Substitute an address, insisting the host half is `{listen}`.
fn address(id: &ExtensionId, field: &str, template: &str, context: &Context) -> Result<String> {
    if !template.starts_with("{listen}:") {
        return Err(refuse(
            id,
            field,
            "must be `{listen}:<port>`: the host is not the manifest's to choose",
        ));
    }

    text(id, field, template, context)
}

/// Substitute a url, insisting its host is `{listen}`.
fn url(id: &ExtensionId, field: &str, template: &str, context: &Context) -> Result<String> {
    if !URL_PREFIXES
        .iter()
        .any(|prefix| template.starts_with(prefix))
    {
        return Err(refuse(
            id,
            field,
            "must begin `http://{listen}` or `https://{listen}`: the host is not the manifest's \
             to choose",
        ));
    }

    text(id, field, template, context)
}

/// Render a `[service]` into the spec that would run.
///
/// # Errors
///
/// [`ExtensionError::ExtensionField`] for anything the format refuses — an address written out, a
/// path that does not grow from a placeholder, an unknown placeholder — and
/// [`ExtensionError::ExtensionSpec`] where the rendering is a spec [`ServiceSpec`] will not accept,
/// reported against the extension rather than against a spec nobody wrote. The rules above are
/// stricter than most of what `validate` checks, so what reaches that second error is a `restart`
/// policy the manifest is free to state and the supervisor is not free to run.
pub fn service_spec(
    manifest: &ExtensionManifest,
    template: &ServiceTemplate,
    context: &Context,
) -> Result<ServiceSpec> {
    let id = &manifest.extension.id;

    service_builder(manifest, template, context)?
        .build()
        .map_err(|source| {
            Error::Extension(ExtensionError::ExtensionSpec {
                id: id.as_str().to_owned(),
                source,
            })
        })
}

/// The same, stopping one step short of a finished spec — roadmap task **T81**.
///
/// **A builder is what a [`Recipe`](crate::generate::Recipe) owes**, because the parts of a spec
/// that come from the row rather than from the declaration — the resource limits a machine's owner
/// set — are applied by the generator afterwards. `inspect` wants the finished thing and the
/// supervisor's path wants the half-finished one; building it twice from two renderings would be
/// two answers to what one manifest says.
///
/// # Errors
///
/// As [`service_spec`], minus what building costs.
pub fn service_builder(
    manifest: &ExtensionManifest,
    template: &ServiceTemplate,
    context: &Context,
) -> Result<ServiceSpecBuilder> {
    let id = &manifest.extension.id;

    let program = rooted(
        id,
        "service.program",
        &template.program,
        &[INSTALL_DIR],
        context,
    )?;
    let cwd = rooted(
        id,
        "service.cwd",
        &template.cwd,
        &[INSTALL_DIR, DATA_DIR],
        context,
    )?;

    let mut arguments = Vec::with_capacity(template.args.len());
    for (index, argument) in template.args.iter().enumerate() {
        let field = format!("service.args[{index}]");
        arguments.push(value(id, &field, argument, context)?);
    }

    let mut builder = ServiceSpec::builder(id.service_id().clone(), program)
        .cwd(cwd)
        .args(arguments)
        .ports(manifest.ports.values().copied())
        .ready(ready(id, &template.ready, context)?)
        .restart(template.restart)
        .stop(template.stop.clone());

    for (key, declared) in &template.env {
        let field = format!("service.env.{key}");

        match declared {
            EnvValue::Literal { value: literal } => {
                builder = builder.env(key, value(id, &field, literal, context)?);
            }
            EnvValue::Keyring {
                service,
                key: entry,
            } => {
                builder = builder.env_from_keyring(key, service, entry);
            }
        }
    }

    if let Some(check) = &template.health {
        builder = builder.health(health(id, check, context)?);
    }

    if let Some(reload) = &template.reload {
        builder = builder.reload(reload.clone());
    }

    Ok(builder)
}

/// Render `ready`.
fn ready(id: &ExtensionId, template: &ReadyTemplate, context: &Context) -> Result<ReadyCheck> {
    Ok(match template {
        ReadyTemplate::Tcp { addr, timeout } => ReadyCheck::Tcp {
            addr: socket(id, "service.ready.addr", addr, context)?,
            timeout: *timeout,
        },
        ReadyTemplate::UnixSocket { path, timeout } => ReadyCheck::UnixSocket {
            path: rooted(
                id,
                "service.ready.path",
                path,
                &[INSTALL_DIR, DATA_DIR],
                context,
            )?,
            timeout: *timeout,
        },
        ReadyTemplate::Http {
            url: template,
            expect_status,
            timeout,
        } => ReadyCheck::Http {
            url: url(id, "service.ready.url", template, context)?,
            expect_status: *expect_status,
            timeout: *timeout,
        },
        ReadyTemplate::LogPattern { regex, timeout } => ReadyCheck::LogPattern {
            regex: regex.clone(),
            timeout: *timeout,
        },
        ReadyTemplate::PidAlive { settle } => ReadyCheck::PidAlive { settle: *settle },
    })
}

/// Render `health`.
fn health(id: &ExtensionId, template: &HealthTemplate, context: &Context) -> Result<HealthCheck> {
    let probe = match &template.probe {
        HealthProbeTemplate::Tcp { addr } => HealthProbe::Tcp {
            addr: socket(id, "service.health.probe.addr", addr, context)?,
        },
        HealthProbeTemplate::UnixSocket { path } => HealthProbe::UnixSocket {
            path: rooted(
                id,
                "service.health.probe.path",
                path,
                &[INSTALL_DIR, DATA_DIR],
                context,
            )?,
        },
        HealthProbeTemplate::Http {
            url: template,
            expect_status,
        } => HealthProbe::Http {
            url: url(id, "service.health.probe.url", template, context)?,
            expect_status: *expect_status,
        },
    };

    Ok(HealthCheck {
        probe,
        interval: template.interval,
        timeout: template.timeout,
        failures_before_degraded: template.failures_before_degraded,
        successes_before_running: template.successes_before_running,
    })
}

/// Render a `{listen}:<port>` into the address it becomes.
fn socket(
    id: &ExtensionId,
    field: &str,
    template: &str,
    context: &Context,
) -> Result<std::net::SocketAddr> {
    let rendered = address(id, field, template, context)?;

    rendered.parse().map_err(|_| {
        refuse(
            id,
            field,
            &format!("renders to `{rendered}`, which is not an address and a port"),
        )
    })
}

/// One refusal, phrased for whoever wrote the file.
fn refuse(id: &ExtensionId, field: &str, reason: &str) -> Error {
    Error::Extension(ExtensionError::ExtensionField {
        id: id.as_str().to_owned(),
        field: field.to_owned(),
        reason: reason.to_owned(),
    })
}

#[cfg(test)]
mod tests;
