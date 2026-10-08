//! The blueprint manifest: `schema`, `[blueprint]`, `[runtimes]`, `[site]`, `[[services]]`,
//! `[php]`, `[scaffold]` and `[[next_steps]]`.
//!
//! **Its own type rather than `mixengine.toml`'s** — the T77 design, D1. The two files overlap but
//! are not one: a blueprint carries `domain_pattern` where a project manifest carries `domain` and
//! `aliases`, and it carries `database` and `user`, which the project manifest does not interpret.
//! They also have two lifetimes. `mixengine.toml` is written by a person, lives in their repository
//! under their comments and is edited byte-preservingly by [`crate::manifest::write`]; a blueprint
//! is generated, read once and thrown away. One struct serving both would make every key an
//! `Option` and hand the comment-preserving writer a second file shape to preserve.
//!
//! **There is no `[php] ini`** (D2). Every ini value MixEngine writes is a constant in
//! [`crate::runtimes::extensions`] — the same `memory_limit = 512M` on every machine this product
//! runs on — so there is no deviation to capture, and capturing it would be capturing a global
//! default, which is the one thing this task is defined against. The key arrives with the task that
//! gives a project an ini of its own.

use std::collections::BTreeMap;

use mixengine_proto::{RuntimeKind, SiteKind, VersionConstraint};

use crate::{Error, Result};

/// The highest schema this build reads. What it writes is [`schema_of`]'s answer (ADR 0061).
pub const SCHEMA: u32 = 2;

/// The instance name that means "one of this project's own".
///
/// A word rather than the captured instance's literal name, and this is the trap D4 exists for: a
/// project `blog` using `mariadb@blog` has a *dedicated* server, and copying that name into the
/// blueprint would make applying it as `shop` plug the new project into the old one's database.
pub const PER_PROJECT: &str = "per-project";

/// A blueprint, as its file says it.
///
/// Read through `RawManifest`, which is where the rules a TOML parser cannot state are checked:
/// `[scaffold]` names a command or an archive and not both (D2), and every `[[next_steps]]` entry
/// is one a person can run unchanged in any shell (D4) — roadmap task **T205**.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "RawManifest")]
pub struct BlueprintManifest {
    /// The format version. Refused on the way in when it is higher than [`SCHEMA`].
    pub schema: u32,

    /// Who this blueprint is, and what wrote it.
    pub blueprint: Header,

    /// The languages it needs, by kind.
    pub runtimes: BTreeMap<RuntimeKind, VersionConstraint>,

    /// What is served, when the blueprint describes a site at all.
    pub site: Option<BlueprintSite>,

    /// The services it needs, in the order the file lists them.
    pub services: Vec<BlueprintService>,

    /// What PHP has to be able to load.
    pub php: Option<Php>,

    /// A command to run in the new project's directory.
    ///
    /// **Never written by [`crate::blueprints::capture`]**: capture does not invent a command to
    /// execute on somebody else's machine. A hand-written or gallery blueprint may carry one, and
    /// since roadmap task **T78a** an apply runs it — in the new project's directory, and only
    /// after somebody has agreed to the exact command, per apply, never on import.
    pub scaffold: Option<Scaffold>,

    /// A release archive to unpack into the new project's directory — roadmap task **T205**, D2.
    /// Read from the same `[scaffold]` table as [`Self::scaffold`]; at most one of the two is set.
    pub archive: Option<Archive>,

    /// What is left for a person to do after the apply — roadmap task **T205**, D4. Read with
    /// `{project}` unexpanded, and kept that way: the steps are expanded where they are read.
    pub next_steps: Vec<mixengine_proto::NextStep>,
}

/// `[blueprint]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Header {
    /// The display name.
    pub name: String,

    /// What it is for.
    #[serde(default)]
    pub description: String,

    /// When it was captured, ISO-8601 UTC.
    pub created_at: String,

    /// What made it.
    pub created_on: Provenance,
}

/// `[blueprint.created_on]` — provenance a person reads.
///
/// **Deliberately not a machine identity**: no host name, no account, nothing that would make a
/// blueprint say where it has been.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Provenance {
    /// `windows`, `macos` or `linux`.
    pub os: String,

    /// The MixEngine that wrote it.
    pub version: String,
}

/// `[site]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlueprintSite {
    /// What it serves.
    ///
    /// Read from the **whole** table rather than from a nested one, because [`SiteKind`] is
    /// internally tagged and its TOML spelling is `kind = "reverse-proxy"` sitting flat beside
    /// `upstream = "…"`. The same shape [`crate::manifest::ManifestSite`] reads, read the same way.
    pub kind: SiteKind,

    /// Relative to the project root; `""` is the root itself.
    pub doc_root: String,

    /// Whether HTTPS is declared.
    pub https: bool,

    /// The primary domain, with `{project}` where the captured project's name was.
    pub domain_pattern: String,

    /// Every other name, by the same rule.
    pub aliases: Vec<String>,

    /// `[[site.routes]]`, in the order they were captured — roadmap task **T135**.
    ///
    /// **A php-fpm route's pool is dropped on capture**, for the reason the site's own is: which
    /// pool answers is a fact about the machine a blueprint was taken from, and the receiving
    /// machine decides its own.
    pub routes: Vec<mixengine_proto::SiteRoute>,
}

/// One `[[services]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct BlueprintService {
    /// The package: `mariadb`, `redis`.
    pub name: String,

    /// The version wanted — exact when captured, a range when somebody wrote it by hand.
    #[serde(default)]
    pub version: Option<VersionConstraint>,

    /// [`PER_PROJECT`] for one of this project's own, or the name of a shared instance to reuse.
    #[serde(default)]
    pub instance: Option<String>,

    /// The database to create, `{project}` allowed.
    #[serde(default)]
    pub database: Option<String>,

    /// The account to create, `{project}` allowed. **Never a password.**
    #[serde(default)]
    pub user: Option<String>,

    /// The `.env` key an apply offers to write this database's URL under — roadmap task
    /// **T205a**. Written only when the person agrees, and only on a service with an account.
    #[serde(default)]
    pub dotenv: Option<String>,
}

/// `[php]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Php {
    /// Extensions this project needs loaded, in name order.
    ///
    /// **Enabling only** (D2). A blueprint says what a project needs loaded; turning something
    /// *off* on the receiving machine would change the PHP every other project there runs, which is
    /// harm it was never asked to do.
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// `[scaffold]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Scaffold {
    /// The command, run in the project directory and nowhere else.
    pub command: String,

    /// Whether this command refuses a directory that already holds anything.
    ///
    /// **Declared, never inferred from the command.** `composer create-project . ` stops at the
    /// first entry a directory holds — a `.git` included — while `composer install` on a tree
    /// somebody cloned is a scaffold that *needs* one. Nothing about the two strings says which is
    /// which, so the author says it, and [`crate::blueprints::plan`] is what asks the directory.
    ///
    /// Default `false`, which is what keeps every `[scaffold]` written before this key existed
    /// running exactly where it used to.
    #[serde(default)]
    pub needs_empty_dir: bool,

    /// Whether this command takes its own name from the directory it runs in, and so refuses one
    /// whose name is not a legal npm package name.
    ///
    /// **Declared, never inferred from the command.** `npx create-next-app .` derives the package
    /// name from the directory's basename and npm allows no capitals and no spaces; `composer
    /// create-project laravel/laravel .` takes its name from its argument and does not care what
    /// the folder is called. The two strings say nothing about which is which, so the author says
    /// it — [`Scaffold::needs_empty_dir`]'s own reasoning, one property along.
    ///
    /// **Named for npm because the rule is npm's.** This format already says `php`, `composer`,
    /// `mariadb`; a Ruby or Python scaffold that one day needs the same protection gets its own key
    /// carrying its own true rule, rather than crowding into one vague key that means something
    /// slightly different in each ecosystem.
    ///
    /// Default `false`, which is what keeps every `[scaffold]` written before this key existed
    /// planning exactly as it does today.
    #[serde(default)]
    pub needs_npm_safe_dir: bool,
}

/// `[scaffold] archive` — roadmap task **T205**, D2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    /// `https://` only.
    pub url: String,

    /// The single top-level folder whose contents become the project root.
    pub strip: Option<String>,

    /// Whether the directory must be empty; the same meaning as [`Scaffold::needs_empty_dir`].
    pub needs_empty_dir: bool,

    /// Whether the archive is unpacked only into an empty directory and skipped over a full one —
    /// a starter, which a folder somebody cloned has no use for (T205, `php-mysql` and `static`).
    ///
    /// Where [`Archive::needs_empty_dir`] blocks the apply, this plans the step `Satisfied`: the
    /// code already there is the code the site serves, and nothing of it is overwritten.
    pub when_empty: bool,
}

/// The file as TOML says it, before the rules of D2 and D4 are checked.
#[derive(serde::Deserialize)]
struct RawManifest {
    schema: u32,
    blueprint: Header,
    #[serde(default)]
    runtimes: BTreeMap<RuntimeKind, VersionConstraint>,
    #[serde(default)]
    site: Option<BlueprintSite>,
    #[serde(default)]
    services: Vec<BlueprintService>,
    #[serde(default)]
    php: Option<Php>,
    #[serde(default)]
    scaffold: Option<RawScaffold>,
    #[serde(default)]
    next_steps: Vec<mixengine_proto::NextStep>,
}

/// `[scaffold]` before it is known to be a command or an archive.
#[derive(serde::Deserialize)]
struct RawScaffold {
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    archive: Option<String>,
    #[serde(default)]
    strip: Option<String>,
    #[serde(default)]
    needs_empty_dir: bool,
    #[serde(default)]
    needs_npm_safe_dir: bool,
    #[serde(default)]
    when_empty: bool,
}

/// The longest `run` a step may carry (D4).
const RUN_LIMIT: usize = 512;

/// What a `run` may not contain, so it means one thing in `cmd.exe`, PowerShell and `sh` (D4).
const FORBIDDEN: &[&str] = &["&&", "||", ";", "|", ">", "<", "`", "$", "%", "\\"];

impl TryFrom<RawManifest> for BlueprintManifest {
    type Error = String;

    fn try_from(raw: RawManifest) -> std::result::Result<Self, String> {
        let (scaffold, archive) = match raw.scaffold {
            None => (None, None),
            Some(table) => match (table.command, table.archive) {
                (Some(_), Some(_)) => {
                    return Err(
                        "[scaffold] names both a command and an archive; it takes one".to_owned(),
                    );
                }
                (None, None) => {
                    return Err("[scaffold] names neither a command nor an archive".to_owned());
                }
                (Some(_), None) if table.when_empty => {
                    return Err(
                        "[scaffold] when_empty describes an archive, and this scaffold \
                                is a command"
                            .to_owned(),
                    );
                }
                (Some(command), None) => (
                    Some(Scaffold {
                        command,
                        needs_empty_dir: table.needs_empty_dir,
                        needs_npm_safe_dir: table.needs_npm_safe_dir,
                    }),
                    None,
                ),
                (None, Some(url)) => {
                    if !url.starts_with("https://") {
                        return Err(format!(
                            "[scaffold] archive {url} is not an https:// address"
                        ));
                    }
                    if table.needs_npm_safe_dir {
                        return Err(
                            "[scaffold] needs_npm_safe_dir describes a command, and this \
                                    scaffold is an archive"
                                .to_owned(),
                        );
                    }
                    if table.when_empty && table.needs_empty_dir {
                        return Err("[scaffold] when_empty skips the archive over a full \
                                    directory and needs_empty_dir blocks the apply there; it \
                                    takes one"
                            .to_owned());
                    }
                    if let Some(strip) = &table.strip
                        && !super::archive::is_folder_name(strip)
                    {
                        return Err(format!(
                            "[scaffold] strip {strip:?} is not one folder name at the archive's \
                             top level"
                        ));
                    }
                    (
                        None,
                        Some(Archive {
                            url,
                            strip: table.strip,
                            needs_empty_dir: table.needs_empty_dir,
                            when_empty: table.when_empty,
                        }),
                    )
                }
            },
        };

        for (index, step) in raw.next_steps.iter().enumerate() {
            checked_step(step).map_err(|reason| format!("next_steps[{}]: {reason}", index + 1))?;
        }

        checked_dotenv(&raw.services)?;

        Ok(Self {
            schema: raw.schema,
            blueprint: raw.blueprint,
            runtimes: raw.runtimes,
            site: raw.site,
            services: raw.services,
            php: raw.php,
            scaffold,
            archive,
            next_steps: raw.next_steps,
        })
    }
}

/// `[[services]] dotenv`'s rules — roadmap task **T205a**: on a service with an account, a
/// variable name, and on one service only, since an apply remembers one database.
fn checked_dotenv(services: &[BlueprintService]) -> std::result::Result<(), String> {
    let mut offered = services
        .iter()
        .filter_map(|service| service.dotenv.as_deref().map(|key| (service, key)));

    let Some((service, key)) = offered.next() else {
        return Ok(());
    };
    if service.database.is_none() || service.user.is_none() {
        return Err(format!(
            "[[services]] {} names dotenv {key} and no database and user to write it for",
            service.name
        ));
    }
    if !super::dotenv::is_key(key) {
        return Err(format!(
            "[[services]] {} dotenv {key:?} is not a variable name: letters, digits and _, not \
             starting with a digit",
            service.name
        ));
    }
    if offered.next().is_some() {
        return Err(
            "dotenv is on more than one service; an apply writes one database's URL, so it takes \
             one service"
                .to_owned(),
        );
    }
    Ok(())
}

/// D4's rules for one step.
fn checked_step(step: &mixengine_proto::NextStep) -> std::result::Result<(), String> {
    use mixengine_proto::NextStepKind;

    // **Refused until a blueprint can have several sites** — T204a defines what it names.
    if step.site.is_some() {
        return Err("`site` names one of several sites, and this blueprint has one".to_owned());
    }

    match step.kind {
        NextStepKind::Open => {
            if step.run.is_some() {
                return Err("an `open` step has no `run`".to_owned());
            }
            if let Some(path) = &step.path
                && !path.starts_with('/')
            {
                return Err(format!("`path` {path} has to start with /"));
            }
            Ok(())
        }
        NextStepKind::Once | NextStepKind::Serve => {
            if step.path.is_some() {
                return Err("only an `open` step has a `path`".to_owned());
            }
            let Some(run) = &step.run else {
                return Err("a `once` or `serve` step needs `run`".to_owned());
            };
            checked_run(run)
        }
    }
}

/// D4: one line a person can run unchanged in any shell this product meets.
fn checked_run(run: &str) -> std::result::Result<(), String> {
    if run.trim().is_empty() {
        return Err("`run` is empty".to_owned());
    }
    if run.contains('\n') || run.contains('\r') {
        return Err("`run` is one line".to_owned());
    }
    if run.len() > RUN_LIMIT {
        return Err(format!("`run` is longer than {RUN_LIMIT} characters"));
    }
    if let Some(found) = FORBIDDEN.iter().find(|token| run.contains(**token)) {
        return Err(format!(
            "`run` contains `{found}`, which does not mean the same in every shell"
        ));
    }

    // `{project}` is the only token; any other brace pair is a token from somewhere else.
    let without = run.replace("{project}", "");
    if without.contains('{') || without.contains('}') {
        return Err("`run` may use {project} and no other token".to_owned());
    }

    Ok(())
}

/// The schema a manifest is written at: the lowest that holds it (ADR 0061).
///
/// **A key that changes what an apply does raises it; a key that only informs does not.** An
/// archive and a `dotenv` change the apply, so either is schema 2 (T205, T205a); `[[next_steps]]`
/// informs, so it is invisible here.
#[must_use]
pub fn schema_of(manifest: &BlueprintManifest) -> u32 {
    let offers_dotenv = manifest
        .services
        .iter()
        .any(|service| service.dotenv.is_some());
    match manifest.archive.is_some() || offers_dotenv {
        true => 2,
        false => 1,
    }
}

impl<'de> serde::Deserialize<'de> for BlueprintSite {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::Error as _;

        let table = toml::Table::deserialize(deserializer)?;

        // Read before the kind, because deserialising the kind consumes a clone of the whole table
        // and these keys are not its business.
        let text = |key: &str| {
            table
                .get(key)
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        };

        let aliases = table
            .get("aliases")
            .and_then(toml::Value::as_array)
            .map(|array| {
                array
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| D::Error::custom("an alias is a string"))
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            kind: SiteKind::deserialize(table.clone()).map_err(D::Error::custom)?,
            doc_root: text("doc_root").unwrap_or_default(),
            // Absent is HTTPS, which is what a site created through `site.create` gets.
            https: table
                .get("https")
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
            domain_pattern: text("domain_pattern")
                .ok_or_else(|| D::Error::custom("a [site] needs a domain_pattern"))?,
            aliases,
            // **Invisible to the kind above**, which names no `routes` and denies no unknown field
            // — roadmap task **T135**. Absent is none, which is every manifest the gallery ships.
            routes: table
                .get("routes")
                .and_then(|value| value.as_array())
                .map(|array| {
                    array
                        .iter()
                        .map(|value| {
                            mixengine_proto::SiteRoute::deserialize(value.clone())
                                .map_err(D::Error::custom)
                        })
                        .collect::<std::result::Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default(),
        })
    }
}

/// Read one.
///
/// **The version is read before the rest.** A file from a build that knew more than this one is
/// refused rather than half-understood: a manifest whose unknown sections were skipped would apply
/// as something other than what its author wrote down.
///
/// # Errors
///
/// [`Error::UnknownBlueprintSchema`] for a newer format, and [`Error::BlueprintManifest`] for a
/// file that does not parse.
pub fn read(text: &str) -> Result<BlueprintManifest> {
    /// Just enough to learn the version, and the name to say it with.
    #[derive(serde::Deserialize)]
    struct Versioned {
        schema: u32,
        #[serde(default)]
        blueprint: Option<NamedOnly>,
    }

    /// The one field of `[blueprint]` a refusal needs.
    #[derive(serde::Deserialize)]
    struct NamedOnly {
        #[serde(default)]
        name: String,
    }

    let versioned: Versioned =
        toml::from_str(text).map_err(|source| Error::BlueprintManifest { source })?;

    if versioned.schema > SCHEMA {
        return Err(Error::UnknownBlueprintSchema {
            name: versioned
                .blueprint
                .map(|header| header.name)
                .unwrap_or_default(),
            schema: versioned.schema,
        });
    }

    toml::from_str(text).map_err(|source| Error::BlueprintManifest { source })
}

/// Write one, in one fixed order.
///
/// **Deterministic by construction** (D7), which is what makes "capturing the same project twice
/// produces two identical files" true rather than lucky: the section order is this function's, the
/// map inside `[runtimes]` is a [`BTreeMap`], and the two lists are sorted by whoever built them.
///
/// A hand-built document rather than a derived `Serialize`, for the reason the module note gives:
/// [`SiteKind`] is internally tagged, and leaving both the key order and TOML's
/// scalars-before-tables rule to a derive would leave the one property this function exists for to
/// chance.
#[must_use]
pub fn render(manifest: &BlueprintManifest) -> String {
    use toml_edit::{Array, DocumentMut, Item, Table, value};

    let mut document = DocumentMut::new();
    document["schema"] = value(i64::from(schema_of(manifest)));

    let mut header = Table::new();
    header["name"] = value(&manifest.blueprint.name);
    if !manifest.blueprint.description.is_empty() {
        header["description"] = value(&manifest.blueprint.description);
    }
    header["created_at"] = value(&manifest.blueprint.created_at);

    let mut created_on = Table::new();
    created_on["os"] = value(&manifest.blueprint.created_on.os);
    created_on["version"] = value(&manifest.blueprint.created_on.version);
    header["created_on"] = Item::Table(created_on);
    document["blueprint"] = Item::Table(header);

    if !manifest.runtimes.is_empty() {
        let mut runtimes = Table::new();
        for (kind, constraint) in &manifest.runtimes {
            runtimes[kind.as_str()] = value(constraint.as_str());
        }
        document["runtimes"] = Item::Table(runtimes);
    }

    if let Some(site) = &manifest.site {
        let mut table = Table::new();

        // The kind renders flat — `kind = "php-fpm"` beside whatever that kind carries — and it is
        // serialised through `toml::Value` so that one spelling of that shape exists, the same one
        // the reader above accepts.
        if let Ok(toml::Value::Table(flat)) = toml::Value::try_from(&site.kind) {
            for (key, item) in flat {
                match item {
                    toml::Value::String(text) => table[key.as_str()] = value(text),
                    toml::Value::Integer(number) => table[key.as_str()] = value(number),
                    toml::Value::Boolean(flag) => table[key.as_str()] = value(flag),
                    // Nothing else is a `SiteKind` payload today, and a variant that grew one would
                    // rather be missing here — and caught by the round-trip test — than rendered as
                    // something the reader cannot take back.
                    _ => {}
                }
            }
        }

        table["doc_root"] = value(&site.doc_root);
        table["https"] = value(site.https);
        table["domain_pattern"] = value(&site.domain_pattern);

        if !site.aliases.is_empty() {
            let mut aliases = Array::new();
            for alias in &site.aliases {
                aliases.push(alias.as_str());
            }
            table["aliases"] = value(aliases);
        }

        // **After the scalars**, on this function's own reason for being hand-built: TOML puts
        // tables after the values of the table they sit in, and an array of tables written before
        // `doc_root` would move every key after it into the wrong table — roadmap task **T135**.
        if !site.routes.is_empty() {
            let mut routes = toml_edit::ArrayOfTables::new();

            for route in &site.routes {
                let mut entry = Table::new();
                entry["path"] = value(route.path.as_str());

                // Exhaustive, so a fourth target is a compile error here rather than a key silently
                // missing from a published manifest.
                match &route.target {
                    mixengine_proto::RouteTarget::Proxy { upstream } => {
                        entry["target"] = value("proxy");
                        entry["upstream"] = value(upstream.as_str());
                    }
                    mixengine_proto::RouteTarget::PhpFpm { pool } => {
                        entry["target"] = value("php-fpm");

                        if let Some(pool) = pool {
                            entry["pool"] = value(pool.as_str());
                        }
                    }
                    mixengine_proto::RouteTarget::Static { root } => {
                        entry["target"] = value("static");
                        entry["root"] = value(root.as_str());
                    }
                }

                routes.push(entry);
            }

            table["routes"] = Item::ArrayOfTables(routes);
        }

        document["site"] = Item::Table(table);
    }

    if !manifest.services.is_empty() {
        let mut services = toml_edit::ArrayOfTables::new();

        for service in &manifest.services {
            let mut table = Table::new();
            table["name"] = value(&service.name);

            if let Some(version) = &service.version {
                table["version"] = value(version.as_str());
            }
            if let Some(instance) = &service.instance {
                table["instance"] = value(instance);
            }
            if let Some(database) = &service.database {
                table["database"] = value(database);
            }
            if let Some(user) = &service.user {
                table["user"] = value(user);
            }
            if let Some(dotenv) = &service.dotenv {
                table["dotenv"] = value(dotenv);
            }

            services.push(table);
        }

        document["services"] = Item::ArrayOfTables(services);
    }

    if let Some(php) = &manifest.php
        && !php.extensions.is_empty()
    {
        let mut table = Table::new();
        let mut extensions = Array::new();
        for name in &php.extensions {
            extensions.push(name.as_str());
        }
        table["extensions"] = value(extensions);
        document["php"] = Item::Table(table);
    }

    if let Some(scaffold) = &manifest.scaffold {
        let mut table = Table::new();
        table["command"] = value(&scaffold.command);

        // **Written only when it is true**, which is what keeps a manifest captured or imported
        // before this key existed rendering back the bytes it arrived as.
        if scaffold.needs_empty_dir {
            table["needs_empty_dir"] = value(true);
        }
        if scaffold.needs_npm_safe_dir {
            table["needs_npm_safe_dir"] = value(true);
        }

        document["scaffold"] = Item::Table(table);
    } else if let Some(archive) = &manifest.archive {
        let mut table = Table::new();
        table["archive"] = value(&archive.url);
        if let Some(strip) = &archive.strip {
            table["strip"] = value(strip);
        }
        if archive.needs_empty_dir {
            table["needs_empty_dir"] = value(true);
        }
        if archive.when_empty {
            table["when_empty"] = value(true);
        }
        document["scaffold"] = Item::Table(table);
    }

    // **Last**, as an array of tables must be: TOML puts tables after the values of the table they
    // sit in — roadmap task **T205**, D4.
    if !manifest.next_steps.is_empty() {
        let mut steps = toml_edit::ArrayOfTables::new();

        for step in &manifest.next_steps {
            let mut entry = Table::new();
            entry["kind"] = value(match step.kind {
                mixengine_proto::NextStepKind::Once => "once",
                mixengine_proto::NextStepKind::Serve => "serve",
                mixengine_proto::NextStepKind::Open => "open",
            });
            if let Some(run) = &step.run {
                entry["run"] = value(run);
            }
            if let Some(path) = &step.path {
                entry["path"] = value(path);
            }
            if let Some(note) = &step.note {
                entry["note"] = value(note);
            }
            if step.optional {
                entry["optional"] = value(true);
            }
            if step.credentials {
                entry["credentials"] = value(true);
            }
            steps.push(entry);
        }

        document["next_steps"] = Item::ArrayOfTables(steps);
    }

    document.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_manifest() -> BlueprintManifest {
        BlueprintManifest {
            schema: 1,
            blueprint: Header {
                name: "laravel-php82".to_owned(),
                description: "Laravel + MariaDB".to_owned(),
                created_at: "2026-09-01T09:00:00Z".to_owned(),
                created_on: Provenance {
                    os: "windows".to_owned(),
                    version: "0.1.0".to_owned(),
                },
            },
            runtimes: [(
                RuntimeKind::Php,
                VersionConstraint::parse("8.2.23").expect("a constraint"),
            )]
            .into_iter()
            .collect(),
            site: Some(BlueprintSite {
                kind: SiteKind::PhpFpm { pool: None },
                doc_root: "public".to_owned(),
                https: true,
                domain_pattern: "{project}.test".to_owned(),
                aliases: vec!["api.{project}.test".to_owned()],
                routes: Vec::new(),
            }),
            services: vec![BlueprintService {
                name: "mariadb".to_owned(),
                version: Some(VersionConstraint::parse("11.4.3").expect("a constraint")),
                instance: Some("main".to_owned()),
                database: Some("{project}".to_owned()),
                user: Some("{project}".to_owned()),
                dotenv: None,
            }],
            php: Some(Php {
                extensions: vec!["redis".to_owned(), "xdebug".to_owned()],
            }),
            scaffold: None,
            archive: None,
            next_steps: Vec::new(),
        }
    }

    /// A manifest with every section this feature has, in the order the renderer writes them.
    ///
    /// Taken from the renderer's own output rather than written by hand, which is what makes the
    /// assertion below about the *format* rather than about somebody's typing.
    const GALLERY_SHAPED: &str = r#"schema = 1

[blueprint]
name = "laravel-php82"
description = "Laravel + MariaDB"
created_at = "2026-09-01T09:00:00Z"

[blueprint.created_on]
os = "windows"
version = "0.1.0"

[runtimes]
php = "8.2.23"

[site]
kind = "php-fpm"
doc_root = "public"
https = true
domain_pattern = "{project}.test"
aliases = ["api.{project}.test"]

[[services]]
name = "mariadb"
version = "11.4.3"
instance = "main"
database = "{project}"
user = "{project}"

[php]
extensions = ["redis", "xdebug"]

[scaffold]
command = "composer create-project laravel/laravel {project}"
"#;

    /// **The rendering is not the signed artifact, and in practice it is the same bytes** — roadmap
    /// task **T78a**, its design's D16. Nothing depends on this: trust is decided over the bytes
    /// that were handed in, once, at import. But a gallery file that came back differently would
    /// mean anybody checking a `.minisig` against `blueprints/<slug>.toml` finds a failure with no
    /// tampering behind it, and this is what T77's byte-identical renderer was for.
    #[test]
    fn a_gallery_shaped_manifest_renders_back_byte_for_byte() {
        let read_back = read(GALLERY_SHAPED).expect("it parses");

        assert_eq!(render(&read_back), GALLERY_SHAPED);
    }

    /// **A command asks for an empty directory by saying so, and otherwise asks for nothing.**
    ///
    /// The default has to be `false` or every `[scaffold]` written before this key existed would
    /// start refusing directories its author was happy to run in — `composer install` on a cloned
    /// tree is a scaffold, and a blueprint that carried one is not this key's business.
    #[test]
    fn a_scaffold_asks_for_nothing_about_the_directory_unless_it_says_so() {
        let read_back = read(GALLERY_SHAPED).expect("it parses");

        assert!(!read_back.scaffold.expect("a scaffold").needs_empty_dir);
    }

    /// **A site's routes survive a render and a read** — roadmap task **T135**, that design's D11.
    ///
    /// The one property this test is really about is the ordering the renderer's own note explains:
    /// TOML puts a table after the values of the table it sits in, so an array of tables written
    /// before `doc_root` would take every key after it into the wrong table. Reading the render back
    /// is what would catch that.
    #[test]
    fn a_sites_routes_survive_the_round_trip() {
        let routes = vec![
            mixengine_proto::SiteRoute {
                path: "/api".to_owned(),
                target: mixengine_proto::RouteTarget::Proxy {
                    upstream: "http://127.0.0.1:3003/xyz".to_owned(),
                },
            },
            mixengine_proto::SiteRoute {
                path: "/admin".to_owned(),
                target: mixengine_proto::RouteTarget::PhpFpm { pool: None },
            },
            mixengine_proto::SiteRoute {
                path: "/assets".to_owned(),
                target: mixengine_proto::RouteTarget::Static {
                    root: "dist".to_owned(),
                },
            },
        ];

        let mut manifest = a_manifest();
        manifest.site.as_mut().expect("a site").routes = routes.clone();

        let rendered = render(&manifest);
        let read_back = read(&rendered).expect("it parses");
        let site = read_back.site.as_ref().expect("a site");

        assert_eq!(site.routes, routes);
        assert_eq!(
            site.doc_root, "public",
            "a key written before the array of tables is still in the site table:\n{rendered}"
        );
        assert_eq!(site.kind, SiteKind::PhpFpm { pool: None });
        assert_eq!(render(&read_back), rendered, "and it renders back the same");
    }

    /// And a manifest that does say so renders it back, which is what lets one travel.
    #[test]
    fn a_scaffold_that_asks_for_an_empty_directory_survives_the_round_trip() {
        let asking = GALLERY_SHAPED.replace(
            "command = \"composer create-project laravel/laravel {project}\"\n",
            "command = \"composer create-project laravel/laravel {project}\"\n\
             needs_empty_dir = true\n",
        );

        let read_back = read(&asking).expect("it parses");

        assert!(
            read_back
                .scaffold
                .as_ref()
                .expect("a scaffold")
                .needs_empty_dir
        );
        assert_eq!(render(&read_back), asking);
    }

    /// **A command that names itself after its directory says so too** — roadmap task **T120c**.
    /// Written only when true, like its neighbour, so a manifest captured or imported before this
    /// key existed renders back exactly the bytes it arrived as.
    #[test]
    fn a_scaffold_that_needs_an_npm_safe_directory_survives_the_round_trip() {
        let asking = GALLERY_SHAPED.replace(
            "command = \"composer create-project laravel/laravel {project}\"\n",
            "command = \"composer create-project laravel/laravel {project}\"\n\
             needs_empty_dir = true\n\
             needs_npm_safe_dir = true\n",
        );

        let read_back = read(&asking).expect("it parses");

        assert!(
            read_back
                .scaffold
                .as_ref()
                .expect("a scaffold")
                .needs_npm_safe_dir
        );
        assert_eq!(render(&read_back), asking);
    }

    /// And a scaffold that says nothing about its directory's name is judged on nothing.
    #[test]
    fn a_scaffold_says_nothing_about_its_directorys_name_unless_it_says_so() {
        let read_back = read(GALLERY_SHAPED).expect("it parses");

        assert!(!read_back.scaffold.expect("a scaffold").needs_npm_safe_dir);
    }

    /// What is written can be read, and reading it back gives the same value — the property every
    /// later task leans on.
    #[test]
    fn a_rendered_manifest_reads_back_as_itself() {
        let manifest = a_manifest();
        let rendered = render(&manifest);

        assert_eq!(read(&rendered).expect("it parses"), manifest, "{rendered}");
    }

    /// Every kind survives the round trip, including the two that carry a payload beside the tag.
    #[test]
    fn every_site_kind_survives_being_written_and_read() {
        for kind in [
            SiteKind::PhpFpm { pool: None },
            SiteKind::Static,
            SiteKind::ReverseProxy {
                upstream: "http://127.0.0.1:3000".to_owned(),
            },
            SiteKind::NodeApp { port: 3000 },
        ] {
            let mut manifest = a_manifest();
            manifest.site.as_mut().expect("a site").kind = kind.clone();

            let rendered = render(&manifest);
            let read_back = read(&rendered).expect("it parses");

            assert_eq!(
                read_back.site.expect("a site").kind,
                kind,
                "{kind:?} did not survive:\n{rendered}"
            );
        }
    }

    /// **D7.** Two captures of one project must produce two identical files, or a golden test says
    /// nothing and a re-capture's diff is noise.
    #[test]
    fn rendering_is_deterministic_and_puts_the_schema_first() {
        let rendered = render(&a_manifest());

        assert_eq!(rendered, render(&a_manifest()));
        assert!(rendered.starts_with("schema = 1\n"), "{rendered}");
        assert!(
            rendered.find("[blueprint]") < rendered.find("[runtimes]"),
            "{rendered}"
        );
        assert!(
            rendered.find("[[services]]") < rendered.find("[php]"),
            "{rendered}"
        );
    }

    /// The pool is a fact about the machine that was captured, and `SiteKind::PhpFpm` can carry
    /// one. It never goes in.
    #[test]
    fn a_site_kind_is_written_flat_and_carries_no_pool() {
        let rendered = render(&a_manifest());

        assert!(rendered.contains("kind = \"php-fpm\""), "{rendered}");
        assert!(!rendered.contains("pool"), "{rendered}");
    }

    /// A file from a build that knew more than this one is refused by name rather than half-read.
    #[test]
    fn a_newer_schema_is_refused_by_name() {
        let text = render(&a_manifest()).replace("schema = 1", "schema = 3");

        assert!(
            matches!(
                read(&text),
                Err(Error::UnknownBlueprintSchema { schema: 3, ref name }) if name == "laravel-php82"
            ),
            "{:?}",
            read(&text)
        );
    }

    /// A `[site]` without a name to answer to is not a site, and saying so beats defaulting to
    /// something the author did not write.
    #[test]
    fn a_site_without_a_domain_pattern_does_not_parse() {
        let text = r#"
schema = 1

[blueprint]
name = "x"
created_at = "2026-09-01T09:00:00Z"

[blueprint.created_on]
os = "linux"
version = "0.1.0"

[site]
kind = "static"
doc_root = "public"
"#;

        assert!(matches!(read(text), Err(Error::BlueprintManifest { .. })));
    }
    fn with_scaffold(table: &str) -> String {
        format!(
            "schema = 2\n\n[blueprint]\nname = \"wp\"\ncreated_at = \"2026-10-08T00:00:00Z\"\n\n\
             [blueprint.created_on]\nos = \"any\"\nversion = \"0.0.1\"\n\n[scaffold]\n{table}\n"
        )
    }

    fn with_steps(steps: &str) -> String {
        format!(
            "schema = 1\n\n[blueprint]\nname = \"x\"\ncreated_at = \"2026-10-08T00:00:00Z\"\n\n\
             [blueprint.created_on]\nos = \"any\"\nversion = \"0.0.1\"\n\n{steps}\n"
        )
    }

    fn refusal(text: &str) -> String {
        match read(text) {
            Err(error) => format!("{error:?}"),
            Ok(_) => panic!("read accepted:\n{text}"),
        }
    }

    #[test]
    fn an_archive_scaffold_reads_and_renders_at_schema_2() {
        let text = with_scaffold(
            "archive = \"https://wordpress.org/latest.zip\"\nstrip = \"wordpress\"\nneeds_empty_dir = true",
        );
        let manifest = read(&text).expect("reads");

        assert!(manifest.scaffold.is_none());
        assert_eq!(
            manifest.archive,
            Some(Archive {
                url: "https://wordpress.org/latest.zip".to_owned(),
                strip: Some("wordpress".to_owned()),
                needs_empty_dir: true,
                when_empty: false,
            })
        );
        assert_eq!(schema_of(&manifest), 2);
        assert!(render(&manifest).starts_with("schema = 2\n"));
        assert_eq!(read(&render(&manifest)).expect("round trip"), manifest);
    }

    #[test]
    fn a_manifest_without_an_archive_renders_at_schema_1_whatever_it_said() {
        let mut manifest = a_manifest();
        manifest.schema = 2;

        assert_eq!(schema_of(&manifest), 1);
        assert!(render(&manifest).starts_with("schema = 1\n"));
    }

    #[test]
    fn an_archive_and_a_command_together_are_refused() {
        let text =
            with_scaffold("command = \"composer install\"\narchive = \"https://x.org/a.zip\"");
        assert!(refusal(&text).contains("both"), "{}", refusal(&text));
    }

    #[test]
    fn a_scaffold_with_neither_is_refused() {
        let text = with_scaffold("needs_empty_dir = true");
        assert!(refusal(&text).contains("neither"), "{}", refusal(&text));
    }

    /// **`when_empty` reads, renders and round-trips** — a starter that is unpacked only into an
    /// empty folder and skipped over a clone (T205, `php-mysql` and `static`).
    #[test]
    fn an_archive_unpacked_only_when_empty_round_trips() {
        let text = with_scaffold(
            "archive = \"https://x.org/a-starter.zip\"\nstrip = \"a\"\nwhen_empty = true",
        );
        let manifest = read(&text).expect("reads");

        let archive = manifest.archive.as_ref().expect("an archive");
        assert!(archive.when_empty);
        assert!(!archive.needs_empty_dir);
        assert!(render(&manifest).contains("when_empty = true"));
        assert_eq!(read(&render(&manifest)).expect("round trip"), manifest);
    }

    /// **`when_empty` and `needs_empty_dir` contradict each other**: one skips the archive over a
    /// full folder, the other blocks the apply there.
    #[test]
    fn when_empty_beside_needs_empty_dir_is_refused() {
        let text = with_scaffold(
            "archive = \"https://x.org/a.zip\"\nneeds_empty_dir = true\nwhen_empty = true",
        );
        assert!(refusal(&text).contains("when_empty"), "{}", refusal(&text));
    }

    /// **`when_empty` describes an archive.** A command that skipped itself over a full folder
    /// would be a scaffold that silently did nothing where it is most often run.
    #[test]
    fn when_empty_on_a_command_is_refused() {
        let text = with_scaffold("command = \"composer install\"\nwhen_empty = true");
        assert!(refusal(&text).contains("when_empty"), "{}", refusal(&text));
    }

    fn with_services(services: &str) -> String {
        format!(
            "schema = 2\n\n[blueprint]\nname = \"x\"\ncreated_at = \"2026-10-09T00:00:00Z\"\n\n\
             [blueprint.created_on]\nos = \"any\"\nversion = \"0.0.1\"\n\n{services}\n"
        )
    }

    /// **`dotenv` reads, renders and round-trips, at schema 2** — roadmap task **T205a**.
    #[test]
    fn a_dotenv_key_round_trips_at_schema_2() {
        let text = with_services(
            "[[services]]\nname = \"postgres\"\ndatabase = \"{project}\"\nuser = \"{project}\"\n\
             dotenv = \"DATABASE_URL\"",
        );
        let manifest = read(&text).expect("reads");

        assert_eq!(manifest.services[0].dotenv.as_deref(), Some("DATABASE_URL"));
        assert_eq!(schema_of(&manifest), 2);
        assert!(render(&manifest).contains("dotenv = \"DATABASE_URL\""));
        assert_eq!(read(&render(&manifest)).expect("round trip"), manifest);
    }

    #[test]
    fn dotenv_without_an_account_is_refused() {
        let text = with_services("[[services]]\nname = \"redis\"\ndotenv = \"REDIS_URL\"");
        assert!(refusal(&text).contains("dotenv"), "{}", refusal(&text));
    }

    #[test]
    fn dotenv_that_is_not_a_variable_name_is_refused() {
        let text = with_services(
            "[[services]]\nname = \"postgres\"\ndatabase = \"d\"\nuser = \"u\"\n\
             dotenv = \"DATABASE-URL\"",
        );
        assert!(
            refusal(&text).contains("DATABASE-URL"),
            "{}",
            refusal(&text)
        );
    }

    #[test]
    fn dotenv_on_a_second_service_is_refused() {
        let text = with_services(
            "[[services]]\nname = \"postgres\"\ndatabase = \"d\"\nuser = \"u\"\ndotenv = \"A\"\n\n\
             [[services]]\nname = \"mysql\"\ndatabase = \"d\"\nuser = \"u\"\ndotenv = \"B\"",
        );
        assert!(refusal(&text).contains("one service"), "{}", refusal(&text));
    }

    #[test]
    fn a_plain_http_archive_is_refused() {
        let text = with_scaffold("archive = \"http://wordpress.org/latest.zip\"");
        assert!(refusal(&text).contains("https"), "{}", refusal(&text));
    }

    /// **`strip` names one folder at the archive's top level, and nothing else.** A path in it
    /// would move whatever it reaches outside the unpacked tree into the project, and the consent
    /// a person gives names the URL, not this.
    #[test]
    fn a_strip_that_is_not_one_folder_name_is_refused() {
        for strip in [
            "..",
            "../outside",
            "a/b",
            "a\\\\b",
            "C:\\\\Users",
            "/etc",
            ".",
            "",
        ] {
            let text = with_scaffold(&format!(
                "archive = \"https://x.org/a.zip\"\nstrip = \"{strip}\""
            ));
            assert!(
                refusal(&text).contains("strip"),
                "{strip}: {}",
                refusal(&text)
            );
        }
    }

    #[test]
    fn an_npm_name_check_beside_an_archive_is_refused() {
        let text = with_scaffold("archive = \"https://x.org/a.zip\"\nneeds_npm_safe_dir = true");
        assert!(
            refusal(&text).contains("needs_npm_safe_dir"),
            "{}",
            refusal(&text)
        );
    }

    #[test]
    fn next_steps_read_render_and_do_not_raise_the_schema() {
        let text = with_steps(
            "[[next_steps]]\nkind = \"once\"\nrun = \"npm install\"\n\n\
             [[next_steps]]\nkind = \"serve\"\nrun = \"npm run dev\"\nnote = \"port 3000\"\n\n\
             [[next_steps]]\nkind = \"open\"\npath = \"/wp-admin/install.php\"\ncredentials = true\noptional = true",
        );
        let manifest = read(&text).expect("reads");

        assert_eq!(manifest.next_steps.len(), 3);
        assert_eq!(manifest.next_steps[1].note.as_deref(), Some("port 3000"));
        assert!(manifest.next_steps[2].credentials);
        assert_eq!(schema_of(&manifest), 1);
        assert_eq!(read(&render(&manifest)).expect("round trip"), manifest);
    }

    #[test]
    fn an_unknown_key_inside_a_step_is_ignored() {
        let text = with_steps(
            "[[next_steps]]\nkind = \"once\"\nrun = \"npm install\"\ndone_when = \"node_modules\"",
        );
        assert_eq!(read(&text).expect("reads").next_steps.len(), 1);
    }

    #[test]
    fn every_run_rule_is_refused_naming_the_step() {
        for bad in [
            "npm install && npm run dev",
            "npm install || true",
            "cd x; npm i",
            "ls | wc",
            "echo > x",
            "sort < x",
            "echo `id`",
            "echo $HOME",
            "echo %PATH%",
            r"vendor\bin\pest",
            "",
        ] {
            let text = with_steps(&format!(
                "[[next_steps]]\nkind = \"once\"\nrun = \"npm install\"\n\n\
                 [[next_steps]]\nkind = \"once\"\nrun = {bad:?}"
            ));
            let said = refusal(&text);
            assert!(said.contains("next_steps[2]"), "{bad:?}: {said}");
        }

        let long = "a".repeat(513);
        let text = with_steps(&format!(
            "[[next_steps]]\nkind = \"once\"\nrun = \"{long}\""
        ));
        assert!(refusal(&text).contains("512"));
    }

    #[test]
    fn a_token_other_than_project_is_refused() {
        let text = with_steps("[[next_steps]]\nkind = \"once\"\nrun = \"echo {domain}\"");
        assert!(refusal(&text).contains("{project}"));
    }

    #[test]
    fn run_on_open_and_path_elsewhere_are_refused() {
        assert!(
            refusal(&with_steps(
                "[[next_steps]]\nkind = \"open\"\nrun = \"npm i\""
            ))
            .contains("open")
        );
        assert!(
            refusal(&with_steps(
                "[[next_steps]]\nkind = \"once\"\nrun = \"npm i\"\npath = \"/x\""
            ))
            .contains("path")
        );
        assert!(refusal(&with_steps("[[next_steps]]\nkind = \"serve\"")).contains("run"));
        assert!(
            refusal(&with_steps("[[next_steps]]\nkind = \"open\"\npath = \"x\"")).contains("/")
        );
    }

    #[test]
    fn a_site_on_a_step_is_refused_until_a_blueprint_has_several() {
        let text = with_steps("[[next_steps]]\nkind = \"once\"\nrun = \"npm i\"\nsite = \"web\"");
        assert!(refusal(&text).contains("site"));
    }
}
