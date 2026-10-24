//! The blueprint manifest: `schema`, `[blueprint]`, `[runtimes]`, `[site]` or `[[sites]]`,
//! `[[services]]`, `[php]`, `[scaffold]` and `[[next_steps]]`.
//!
//! **Its own type rather than `mixengine.toml`'s.** The two files overlap but are not one: a
//! blueprint carries `domain_pattern` where a project manifest carries `domain` and `aliases`, and
//! it carries `database` and `user`, which the project manifest does not interpret. They also have
//! two lifetimes. `mixengine.toml` is written by a person, lives in their repository under their
//! comments and is edited byte-preservingly by [`crate::manifest::write`]; a blueprint is
//! generated, read once and thrown away. One struct serving both would make every key an `Option`
//! and hand the comment-preserving writer a second file shape to preserve.
//!
//! **There is no `[php] ini`** (D2). Every ini value MixEngine writes is a constant in
//! [`crate::runtimes::extensions`] — the same `memory_limit = 512M` on every machine this product
//! runs on — so there is no deviation to capture, and capturing it would be capturing a global
//! default, which is the one thing this task is defined against. The key arrives with the task that
//! gives a project an ini of its own.
//!
//! The D-numbers below are those of `docs/specs/2026-09-01-t77-blueprint-capture-design.md`.

use std::collections::BTreeMap;

use mixengine_proto::{RuntimeKind, SiteKind, VersionConstraint};

use crate::{BlueprintError, Error, Result};

/// The highest schema this build reads. What it writes is [`schema_of`]'s answer (ADR 0061).
pub const SCHEMA: u32 = 3;

/// The schema a blueprint with several sites is written at.
pub const SEVERAL_SITES: u32 = 3;

/// The instance name that means "one of this project's own".
///
/// A word rather than the captured instance's literal name, and this is the trap D4 exists for: a
/// project `blog` using `mariadb@blog` has a *dedicated* server, and copying that name into the
/// blueprint would make applying it as `shop` plug the new project into the old one's database.
pub const PER_PROJECT: &str = "per-project";

/// A blueprint, as its file says it.
///
/// Read through `RawManifest`, which is where the rules a TOML parser cannot state are checked:
/// `[scaffold]` names a command or an archive and not both, and every `[[next_steps]]` entry is one
/// a person can run unchanged in any shell.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "RawManifest")]
pub struct BlueprintManifest {
    /// The format version. Refused on the way in when it is higher than [`SCHEMA`].
    pub schema: u32,

    /// Who this blueprint is, and what wrote it.
    pub blueprint: Header,

    /// The languages it needs, by kind.
    pub runtimes: BTreeMap<RuntimeKind, VersionConstraint>,

    /// What is served, in the order the file lists it: `[site]` as one entry, `[[sites]]` as its
    /// entries.
    pub sites: Vec<BlueprintSite>,

    /// The services it needs, in the order the file lists them.
    pub services: Vec<BlueprintService>,

    /// What PHP has to be able to load.
    pub php: Option<Php>,

    /// A command to run in the new project's directory.
    ///
    /// **Never written by [`crate::blueprints::capture`]**: capture does not invent a command to
    /// execute on somebody else's machine. A hand-written or gallery blueprint may carry one, and
    /// an apply runs it — in the new project's directory, and only after somebody has agreed to the
    /// exact command, per apply, never on import.
    pub scaffold: Option<Scaffold>,

    /// A release archive to unpack into the new project's directory.
    /// Read from the same `[scaffold]` table as [`Self::scaffold`]; at most one of the two is set.
    pub archive: Option<Archive>,

    /// What is left for a person to do after the apply. Read with `{project}` unexpanded, and kept
    /// that way: the steps are expanded where they are read.
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

    /// `[[site.routes]]`, in the order they were captured.
    ///
    /// **A php-fpm route's pool is dropped on capture**, for the reason the site's own is: which
    /// pool answers is a fact about the machine a blueprint was taken from, and the receiving
    /// machine decides its own.
    pub routes: Vec<mixengine_proto::SiteRoute>,

    /// The `[[services]]` entries this site links, by `name` or `name@instance`. [`None`] links
    /// every one, which is what `[site]` always means.
    pub services: Option<Vec<String>>,
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

    /// The `.env` key an apply offers to write this database's URL under. Written only when the
    /// person agrees, and only on a service with an account.
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

/// `[scaffold] archive`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    /// `https://` only.
    pub url: String,

    /// The single top-level folder whose contents become the project root.
    pub strip: Option<String>,

    /// Whether the directory must be empty; the same meaning as [`Scaffold::needs_empty_dir`].
    pub needs_empty_dir: bool,

    /// Whether the archive is unpacked only into an empty directory and skipped over a full one —
    /// a starter, which a folder somebody cloned has no use for (the gallery's `php-mysql` and
    /// `static`).
    ///
    /// Where [`Archive::needs_empty_dir`] blocks the apply, this plans the step `Satisfied`: the
    /// code already there is the code the site serves, and nothing of it is overwritten.
    pub when_empty: bool,
}

/// The file as TOML says it, before the `[scaffold]` and `[[next_steps]]` rules are checked.
#[derive(serde::Deserialize)]
struct RawManifest {
    schema: u32,
    blueprint: Header,
    #[serde(default)]
    runtimes: BTreeMap<RuntimeKind, VersionConstraint>,
    #[serde(default)]
    site: Option<BlueprintSite>,
    #[serde(default)]
    sites: Option<Vec<BlueprintSite>>,
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

/// The longest `run` a step may carry.
const RUN_LIMIT: usize = 512;

/// What a `run` may not contain, so it means one thing in `cmd.exe`, PowerShell and `sh`.
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

        let sites = match (raw.site, raw.sites) {
            (Some(_), Some(_)) => {
                return Err(
                    "the file has both [site] and [[sites]]; a blueprint uses one".to_owned(),
                );
            }
            (Some(site), None) => {
                if site.services.is_some() {
                    return Err(
                        "[site] links every [[services]] entry; `services` belongs on a \
                                [[sites]] entry"
                            .to_owned(),
                    );
                }
                vec![site]
            }
            (None, Some(sites)) => {
                // **An older build would apply this as a project with no site**, so the file has to
                // say which builds can read it.
                if raw.schema < SEVERAL_SITES {
                    return Err(format!(
                        "[[sites]] needs schema = {SEVERAL_SITES}, and this file says {}",
                        raw.schema
                    ));
                }
                sites
            }
            (None, None) => Vec::new(),
        };

        checked_names(&sites)?;
        for (index, site) in sites.iter().enumerate() {
            for item in site.services.iter().flatten() {
                linked_service(item, &raw.services)
                    .map_err(|reason| format!("sites[{}]: {reason}", index + 1))?;
            }
        }

        for (index, step) in raw.next_steps.iter().enumerate() {
            checked_step(step, &sites)
                .map_err(|reason| format!("next_steps[{}]: {reason}", index + 1))?;
        }

        checked_dotenv(&raw.services)?;

        Ok(Self {
            schema: raw.schema,
            blueprint: raw.blueprint,
            runtimes: raw.runtimes,
            sites,
            services: raw.services,
            php: raw.php,
            scaffold,
            archive,
            next_steps: raw.next_steps,
        })
    }
}

/// `[[services]] dotenv`'s rules: on a service with an account, a variable name, and on one service
/// only, since an apply remembers one database.
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

/// No name answers for two entries, or twice in one.
fn checked_names(sites: &[BlueprintSite]) -> std::result::Result<(), String> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();

    for (index, site) in sites.iter().enumerate() {
        for name in std::iter::once(&site.domain_pattern).chain(&site.aliases) {
            if let Some(first) = seen.insert(name.as_str(), index + 1) {
                return Err(format!(
                    "sites[{first}] and sites[{}] both answer to {name}",
                    index + 1
                ));
            }
        }
    }

    Ok(())
}

/// The `[[services]]` entry one `services` item names: the entry whose `name` it is, or, written
/// `name@instance`, the one whose `instance` is that as written.
///
/// # Errors
///
/// The reason, for an item that names no entry or names two.
pub fn linked_service<'a>(
    item: &str,
    services: &'a [BlueprintService],
) -> std::result::Result<&'a BlueprintService, String> {
    let (name, instance) = match item.split_once('@') {
        Some((name, instance)) => (name, Some(instance)),
        None => (item, None),
    };
    let mut found = services.iter().filter(|service| {
        service.name == name
            && instance.is_none_or(|instance| service.instance.as_deref() == Some(instance))
    });

    match (found.next(), found.next()) {
        (Some(one), None) => Ok(one),
        (None, _) => Err(format!(
            "services names {item}, and no [[services]] entry is {item}"
        )),
        (Some(_), Some(_)) => Err(format!(
            "services names {item}, and two [[services]] entries are {name}; name one as \
             {name}@<instance>"
        )),
    }
}

/// Every entry's `domain_pattern`, for a refusal to list.
fn patterns(sites: &[BlueprintSite]) -> String {
    sites
        .iter()
        .map(|site| site.domain_pattern.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The rules for one `[[next_steps]]` entry.
fn checked_step(
    step: &mixengine_proto::NextStep,
    sites: &[BlueprintSite],
) -> std::result::Result<(), String> {
    use mixengine_proto::NextStepKind;

    // **With several sites every step belongs to one**: a `serve` runs one site's program and an
    // `open` opens one site's address, so a client is never left to guess. Named by
    // `domain_pattern`, the identity an entry already has.
    match (&step.site, sites.len() > 1) {
        (Some(_), false) => {
            return Err("`site` names one of several sites, and this blueprint has one".to_owned());
        }
        (None, true) => {
            return Err(format!(
                "`site` is required when a blueprint has several sites: one of {}",
                patterns(sites)
            ));
        }
        (Some(named), true) if !sites.iter().any(|site| site.domain_pattern == *named) => {
            return Err(format!(
                "`site` {named} is no site's domain_pattern; the sites are {}",
                patterns(sites)
            ));
        }
        _ => {}
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

/// One line a person can run unchanged in any shell this product meets.
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

/// Whether the sites are written as `[[sites]]`: several of them, or one whose links `[site]`
/// cannot say.
#[must_use]
pub fn writes_sites_table(manifest: &BlueprintManifest) -> bool {
    manifest.sites.len() > 1 || manifest.sites.iter().any(|site| site.services.is_some())
}

/// The schema a manifest is written at: the lowest that holds it (ADR 0061).
///
/// **A key that changes what an apply does raises it; a key that only informs does not.** An
/// archive and a `dotenv` change the apply, so either is schema 2; `[[next_steps]]` informs, so it
/// is invisible here. Several sites change what an apply makes — an older build would make a
/// project with none — so `[[sites]]` is schema 3.
#[must_use]
pub fn schema_of(manifest: &BlueprintManifest) -> u32 {
    if writes_sites_table(manifest) {
        return SEVERAL_SITES;
    }

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

        // **Absent and empty are different**: absent links every `[[services]]` entry, `[]` links
        // none.
        let services = table
            .get("services")
            .map(|value| {
                value
                    .as_array()
                    .ok_or_else(|| D::Error::custom("services is a list of service names"))?
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| D::Error::custom("a service is named by a string"))
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .transpose()?;

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
            // **Invisible to the kind above**, which names no `routes` and denies no unknown field.
            // Absent is none, which is every manifest the gallery ships.
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
            services,
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
/// [`BlueprintError::UnknownBlueprintSchema`] for a newer format, and
/// [`BlueprintError::BlueprintManifest`] for a file that does not parse.
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

    let versioned: Versioned = toml::from_str(text)
        .map_err(|source| Error::Blueprint(BlueprintError::BlueprintManifest { source }))?;

    if versioned.schema > SCHEMA {
        return Err(Error::Blueprint(BlueprintError::UnknownBlueprintSchema {
            name: versioned
                .blueprint
                .map(|header| header.name)
                .unwrap_or_default(),
            schema: versioned.schema,
        }));
    }

    toml::from_str(text)
        .map_err(|source| Error::Blueprint(BlueprintError::BlueprintManifest { source }))
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

    if writes_sites_table(manifest) {
        // **Several sites, or one naming its links.**
        let mut sites = toml_edit::ArrayOfTables::new();
        for site in &manifest.sites {
            sites.push(site_table(site));
        }
        document["sites"] = Item::ArrayOfTables(sites);
    } else if let Some(site) = manifest.sites.first() {
        document["site"] = Item::Table(site_table(site));
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
    // sit in.
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
            if let Some(site) = &step.site {
                entry["site"] = value(site);
            }
            steps.push(entry);
        }

        document["next_steps"] = Item::ArrayOfTables(steps);
    }

    document.to_string()
}

/// One site as its table: `[site]`, or one `[[sites]]` entry.
fn site_table(site: &BlueprintSite) -> toml_edit::Table {
    use toml_edit::{Array, Item, Table, value};

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

    // **The links, when the site names them.** A scalar, so before the routes for the reason the
    // next comment gives.
    if let Some(services) = &site.services {
        let mut links = Array::new();
        for item in services {
            links.push(item.as_str());
        }
        table["services"] = value(links);
    }

    // **After the scalars**, on [`render`]'s own reason for being hand-built: TOML puts
    // tables after the values of the table they sit in, and an array of tables written before
    // `doc_root` would move every key after it into the wrong table.
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

    table
}

#[cfg(test)]
mod tests;
