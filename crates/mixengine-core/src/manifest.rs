//! `mixengine.toml` — the file a project pins its runtimes in, read and written in one place.
//!
//! **One reader** (spec D9). [`crate::resolve`] used to deserialise a deliberately narrow struct of
//! its own: `[runtimes]` and nothing else. T39 needs `[project] name` on import and a writer for
//! export, and two structs describing one file would be two answers to one question — so the narrow
//! one is gone and `resolve` is a caller.
//!
//! **Unknown sections are still allowed through.** `[site]` and `[[services]]` have types as of
//! T39a, and `[[sites]]` as of T204, but the file also has to hold what T43 and Phase 8 will add, and a `deny_unknown_fields`
//! here would make this build refuse the manifests those tasks write. What is still closed is the
//! map inside `[runtimes]` — a key naming a language MixEngine does not manage is a pin that would
//! silently do nothing — and the two typed sections' own required keys.
//!
//! # The writer edits; it does not rewrite
//!
//! This file lives in the user's repository, under version control, with their comments and their
//! key order in it — and, after T39a, a `[site]` or `[[sites]]` block they wrote by hand. Serialising a fresh
//! document over it would destroy all of that, and would do it to the one file whose entire purpose
//! is to be read by a person. So [`write()`] edits a `toml_edit` document: it sets `[project] name`
//! and the `[runtimes]` keys it owns, and leaves every other byte alone.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mixengine_proto::{PackageVersion, RuntimeKind, ServiceId, SiteKind, VersionConstraint};

use crate::{Error, Result};

/// The file a project pins its runtimes in, checked into the user's repository.
pub const FILE_NAME: &str = "mixengine.toml";

/// `mixengine.toml`, as this build understands it.
///
/// Four sections and no catch-all: what the writer preserves beyond them it preserves through the
/// document it edits rather than through a field nothing reads, so a section T43 adds survives an
/// export without this type having to hold it.
#[derive(Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "Sections")]
pub struct Manifest {
    /// `[project]`, when the file has one.
    pub project: Option<Project>,

    /// The versions this project wants, by language.
    pub runtimes: BTreeMap<RuntimeKind, VersionConstraint>,

    /// `[site]` or `[[sites]]`, in the order the file writes them — roadmap task **T204**.
    ///
    /// **One list for both spellings**, so no caller asks which one a file used: `[site]` is a
    /// list of one, and is what every file written before T204 holds. The form only matters to the
    /// writer, which reads it off the document it edits.
    pub sites: Vec<ManifestSite>,

    /// `[[services]]`, in the order the file lists them.
    pub services: Vec<ManifestService>,
}

/// The sections exactly as the file spells them, before `[site]` and `[[sites]]` become one list.
#[derive(serde::Deserialize)]
struct Sections {
    #[serde(default)]
    project: Option<Project>,
    #[serde(default)]
    runtimes: BTreeMap<RuntimeKind, VersionConstraint>,
    #[serde(default)]
    site: Option<ManifestSite>,
    #[serde(default)]
    sites: Option<Vec<ManifestSite>>,
    #[serde(default)]
    services: Vec<ManifestService>,
}

impl TryFrom<Sections> for Manifest {
    type Error = String;

    /// **Both spellings at once is refused** (T204, D1): a file holding both was meant one way, and
    /// picking either would apply something its author did not write.
    fn try_from(sections: Sections) -> std::result::Result<Self, String> {
        let sites = match (sections.site, sections.sites) {
            (Some(_), Some(_)) => {
                return Err(
                    "the file declares both `[site]` and `[[sites]]`; keep one of them".to_owned(),
                );
            }
            (Some(site), None) => vec![site],
            (None, Some(sites)) => sites,
            (None, None) => Vec::new(),
        };

        Ok(Self {
            project: sections.project,
            runtimes: sections.runtimes,
            sites,
            services: sections.services,
        })
    }
}

/// Which of a manifest's sites a `site.create` falls through to — roadmap task **T204**, spec D5.
#[derive(Debug, PartialEq, Eq)]
pub struct Choice<'a> {
    /// The entry, when one was chosen.
    pub site: Option<&'a ManifestSite>,

    /// Where the new site's links come from when the request names none.
    pub services: ServicesFrom<'a>,
}

/// Where a new site's links fall through to.
#[derive(Debug, PartialEq, Eq)]
pub enum ServicesFrom<'a> {
    /// The chosen entry's own `services`.
    Listed(&'a [ServiceId]),

    /// Every `[[services]]` entry: a chosen entry with no `services` key, or a file declaring no
    /// site — which is everything a manifest meant before T204.
    Every,

    /// None. The request describes a site a several-site file does not, and guessing that it wants
    /// every service the others use is the invented default T39a's D10 refused.
    Nothing,
}

/// Why no entry could be chosen.
#[derive(Debug, PartialEq, Eq)]
pub enum Unchosen {
    /// `from` names no entry.
    NotDeclared {
        /// What `from` said.
        name: String,
        /// Every entry's name, in file order.
        declared: Vec<String>,
    },

    /// Several entries and nothing to choose between them by.
    Ambiguous {
        /// Every entry's name, in file order.
        declared: Vec<String>,
    },

    /// Two entries answer to one name.
    NamedTwice {
        /// The name both answer to.
        name: String,
        /// The first entry's position, counted from 1 as a person reads the file.
        first: usize,
        /// The second's.
        second: usize,
    },
}

impl Manifest {
    /// Choose the entry a `site.create` falls through to: `from`, then `domains[0]`, then the only
    /// one — spec D5.
    ///
    /// `default_domain` is what an entry with no `domain` is called, which is the name
    /// `site.create` would give it (`<slug>.test`).
    ///
    /// # Errors
    ///
    /// [`Unchosen`], each variant one refusal of D5.
    pub fn choose<'a>(
        &'a self,
        from: Option<&str>,
        domains: Option<&[String]>,
        default_domain: &str,
    ) -> std::result::Result<Choice<'a>, Unchosen> {
        let declared = || {
            self.sites
                .iter()
                .map(|site| {
                    site.domain
                        .clone()
                        .unwrap_or_else(|| default_domain.to_owned())
                })
                .collect::<Vec<_>>()
        };

        let site = match (from, domains.and_then(<[String]>::first)) {
            (Some(name), _) => {
                Some(named(&self.sites, name, default_domain)?.ok_or_else(|| {
                    Unchosen::NotDeclared {
                        name: name.to_owned(),
                        declared: declared(),
                    }
                })?)
            }
            (None, Some(primary)) => match named(&self.sites, primary, default_domain)? {
                Some(site) => Some(site),
                None if self.sites.len() == 1 => self.sites.first(),
                None => None,
            },
            (None, None) => match self.sites.len() {
                0 => None,
                1 => self.sites.first(),
                _ => {
                    return Err(Unchosen::Ambiguous {
                        declared: declared(),
                    });
                }
            },
        };

        let services = match site {
            Some(site) => site
                .services
                .as_deref()
                .map_or(ServicesFrom::Every, ServicesFrom::Listed),
            None if self.sites.len() > 1 => ServicesFrom::Nothing,
            None => ServicesFrom::Every,
        };

        Ok(Choice { site, services })
    }
}

/// The one entry answering to `name`, by its `domain` (or the default, when it has none) or any
/// alias.
fn named<'a>(
    sites: &'a [ManifestSite],
    name: &str,
    default_domain: &str,
) -> std::result::Result<Option<&'a ManifestSite>, Unchosen> {
    let mut hit: Option<(usize, &'a ManifestSite)> = None;

    for (position, site) in sites.iter().enumerate() {
        let answers = site.domain.as_deref().unwrap_or(default_domain) == name
            || site.aliases.iter().any(|alias| alias == name);

        if !answers {
            continue;
        }

        if let Some((first, _)) = hit {
            return Err(Unchosen::NamedTwice {
                name: name.to_owned(),
                first: first + 1,
                second: position + 1,
            });
        }

        hit = Some((position, site));
    }

    Ok(hit.map(|(_, site)| site))
}

/// `[site]` — what is served out of this directory, and at what name.
///
/// Every field is optional because every one of them falls through to a default the daemon knows
/// (spec D7): a manifest saying only `domain = "blog.test"` is a whole declaration.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ManifestSite {
    /// The primary domain.
    pub domain: Option<String>,

    /// Every other name it answers to.
    pub aliases: Vec<String>,

    /// Relative to the project's root.
    pub doc_root: Option<String>,

    /// Whether HTTPS is wanted.
    pub https: Option<bool>,

    /// What it serves, when the file says.
    ///
    /// Read from the **whole** `[site]` table rather than from a nested one, because
    /// [`SiteKind`] is internally tagged and its TOML representation is `kind = "reverse-proxy"`
    /// sitting flat beside `upstream = "…"`. One type reads the file and the wire, with nothing in
    /// between to drift.
    pub kind: Option<SiteKind>,

    /// `[[site.routes]]`, in the order the file writes them — roadmap task **T135**.
    ///
    /// Empty where the file names none, which is every manifest written before T135. The daemon is
    /// what sorts them into match order; a file is what somebody typed.
    pub routes: Vec<mixengine_proto::SiteRoute>,

    /// `services = [...]`: the ids this site links — roadmap task **T204**, spec D3.
    ///
    /// **Absent is not empty.** [`None`] falls through to every `[[services]]` entry, which is
    /// what every file written before T204 means. `Some(vec![])` links none. An id without an
    /// instance is looked up the way `[[services]]` is: the bare name, then `name@main`.
    pub services: Option<Vec<ServiceId>>,
}

impl<'de> serde::Deserialize<'de> for ManifestSite {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::Error as _;

        let table = toml::Table::deserialize(deserializer)?;

        // Read before the kind, because deserialising the kind consumes a clone of the whole table
        // and these four keys are not its business.
        let text = |key: &str| {
            table
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        };

        let aliases = table
            .get("aliases")
            .and_then(|value| value.as_array())
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

        // **Read before the kind, and harmless to it either way** — roadmap task **T135**.
        // `SiteKind` names no `routes`, and nothing here denies unknown fields, so the array below
        // is invisible to the line after it. Reading it first is what keeps that a fact about this
        // function rather than about `SiteKind`'s attributes.
        let routes = table
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
            .unwrap_or_default();

        // **Read before the kind, and harmless to it either way**, on `routes`' reasoning —
        // roadmap task **T204**.
        let services = table
            .get("services")
            .map(|value| {
                value
                    .as_array()
                    .ok_or_else(|| D::Error::custom("`services` is a list of service ids"))?
                    .iter()
                    .map(|value| {
                        let text = value
                            .as_str()
                            .ok_or_else(|| D::Error::custom("a service id is a string"))?;
                        ServiceId::parse(text).map_err(D::Error::custom)
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .transpose()?;

        // Absent is `None`; present and wrong is the file being wrong, which the enum decides.
        let kind = match table.contains_key("kind") {
            true => Some(SiteKind::deserialize(table.clone()).map_err(D::Error::custom)?),
            false => None,
        };

        Ok(Self {
            domain: text("domain"),
            aliases,
            doc_root: text("doc_root"),
            https: table.get("https").and_then(toml::Value::as_bool),
            kind,
            routes,
            services,
        })
    }
}

/// One `[[services]]` entry.
///
/// **`database` and `user` are read as of T77**, and by exactly one caller:
/// [`crate::blueprints::capture`]. They used to be absent from this type on the rule that a key read
/// and then quietly ignored is a promise not kept (spec D8) — which was right while nothing could
/// act on them. What changed is that something can: a captured blueprint carries the database name
/// so that the apply T78 brings can create it under the new project's name. This build still
/// creates no databases, and the writer still preserves both keys through the document it edits.
#[derive(Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct ManifestService {
    /// The package, which is the first half of a [`mixengine_proto::ServiceId`].
    pub name: String,

    /// The instance, when the file names one.
    ///
    /// Absent is **not** the same as `"main"`: what an absent instance means is decided by the
    /// lookup, which tries the bare name — what a single-instance package such as `caddy` is
    /// actually called — before `name@main`.
    #[serde(default)]
    pub instance: Option<String>,

    /// The version wanted. Its *syntax* is refused here; whether anything satisfies it is the
    /// daemon's question and is reported rather than refused.
    #[serde(default)]
    pub version: Option<VersionConstraint>,

    /// The database this service is expected to hold for the project, when the file names one.
    ///
    /// Preserved, not interpreted: nothing in this build creates it. What reads it is
    /// [`crate::blueprints::capture`], so that a blueprint captured from a project that names its
    /// database carries the name rather than losing it.
    #[serde(default)]
    pub database: Option<String>,

    /// The account that reaches [`Self::database`], by the same rule and with the same reader.
    ///
    /// **Never a password.** There is no key for one here and this type will not grow one: a
    /// credential in a file whose whole purpose is to be committed to somebody's repository is the
    /// accident this product must not have.
    #[serde(default)]
    pub user: Option<String>,
}

/// `[project]`.
#[derive(Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct Project {
    /// What the project is called, when the file says.
    #[serde(default)]
    pub name: Option<String>,
}

/// Where a directory's manifest is.
#[must_use]
pub fn at(directory: &Path) -> PathBuf {
    directory.join(FILE_NAME)
}

/// Read one, or [`None`] where there is none to read.
///
/// **A file that cannot be opened is treated as one that is not there**, which is the rule
/// [`crate::resolve`] has always followed and the reason it can walk to the root: the ancestor walk
/// passes through other people's directories on the way up, and a permission error three levels
/// above somebody's project is not a fact about their project.
///
/// # Errors
///
/// [`Error::Manifest`] for a file that does not parse — including a `[runtimes]` key naming a
/// language this build does not manage — and [`Error::Io`] for a read that failed some other way.
pub fn read(path: &Path) -> Result<Option<Manifest>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            return Ok(None);
        }
        Err(source) => {
            return Err(Error::Io {
                action: "read",
                path: path.to_path_buf(),
                source,
            });
        }
    };

    toml::from_str(&text)
        .map(Some)
        .map_err(|source| Error::Manifest {
            path: path.to_path_buf(),
            source,
        })
}

/// What `project.export` puts into somebody's repository.
///
/// A struct rather than a widening argument list: this is one thing — *the project, as a colleague
/// should receive it* — and a signature whose arguments have to be counted is one a caller gets
/// wrong silently.
#[derive(Debug, Clone)]
pub struct Export {
    /// `[project] name`.
    pub name: String,

    /// `[runtimes]`, the keys this export owns.
    pub pins: BTreeMap<RuntimeKind, VersionConstraint>,

    /// Every site of the project, in `sites::records` order — roadmap task **T204**.
    ///
    /// Written as `[site]` or `[[sites]]` by spec D1's table; which entry is whose is D2's.
    pub sites: Vec<ExportSite>,
}

/// What [`write()`] did — roadmap task **T204**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Whether the file had to be created.
    pub created: bool,

    /// The `domain` of each site entry no site of the project holds, left exactly as written.
    ///
    /// An export never deletes (T39a's D9), so it says what it left in the same answer that left
    /// it — spec D4.
    pub sites_kept: Vec<String>,
}

/// `[site]`, as an export writes it.
#[derive(Debug, Clone)]
pub struct ExportSite {
    /// The primary.
    pub domain: String,
    /// Every other name.
    pub aliases: Vec<String>,
    /// Relative, as stored.
    pub doc_root: String,
    /// Whether HTTPS is declared.
    pub https: bool,
    /// What it serves, written from an exhaustive match so a fifth kind cannot be forgotten.
    pub kind: SiteKind,
    /// The routes it declares, written from an exhaustive match for the same reason — roadmap task
    /// **T135**.
    pub routes: Vec<mixengine_proto::SiteRoute>,
    /// The services it declares.
    pub services: Vec<ExportService>,
}

/// One `[[services]]` entry, as an export writes it.
#[derive(Debug, Clone)]
pub struct ExportService {
    /// The id exactly as the site links it, which is what the site's `services = [...]` names —
    /// roadmap task **T204**. `name` and `instance` are its halves spelled out for `[[services]]`;
    /// this is kept whole because a single-instance package (`caddy`) has no `@main` to add.
    pub link: ServiceId,
    /// The package.
    pub name: String,
    /// The instance, spelled out even when it is `main`, because the file is read by a person.
    pub instance: String,
    /// What is installed here, so a colleague knows what to install. Omitted when it cannot be read.
    pub version: Option<PackageVersion>,
}

/// Set `[project] name`, these `[runtimes]` keys, and every site in `<directory>/mixengine.toml`.
///
/// Answers whether the file had to be created and which site entries it left. Keys this call does
/// not name are left as they are — a pin the user wrote and MixEngine does not know about is still
/// theirs.
///
/// # Errors
///
/// [`Error::Manifest`] for an existing file that does not parse — refused before a byte is written,
/// so a broken manifest is never made worse — [`Error::ManifestEdit`] for one that parses as TOML
/// but not as a document this can edit, including a `sites` key that is not a list of `[[sites]]`
/// tables, and [`Error::Io`] when the file cannot be read or written.
pub fn write(directory: &Path, export: &Export) -> Result<Written> {
    let path = at(directory);

    // Validated through the reader first, so the failure a caller sees for a broken file is the
    // same `Error::Manifest` every other door gives it, naming the same path.
    let created = read(&path)?.is_none() && !path.exists();

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(Error::Io {
                action: "read",
                path,
                source,
            });
        }
    };

    let mut document: toml_edit::DocumentMut =
        text.parse()
            .map_err(|error: toml_edit::TomlError| Error::ManifestEdit {
                path: path.clone(),
                reason: error.to_string(),
            })?;

    set(&mut document, "project", |table| {
        table["name"] = toml_edit::value(export.name.as_str());
    });

    set(&mut document, "runtimes", |table| {
        for (kind, constraint) in &export.pins {
            table[kind.as_str()] = toml_edit::value(constraint.as_str());
        }
    });

    let sites_kept =
        write_sites(&mut document, &export.sites).map_err(|reason| Error::ManifestEdit {
            path: path.clone(),
            reason,
        })?;

    // **The union of every site's links** — roadmap task **T204**, spec D3. It used to be the one
    // site's, and only when there was exactly one, so a project with two exported no services.
    let services = every_service(&export.sites);
    if !services.is_empty() {
        merge_services(&mut document, &services);
    }

    std::fs::write(&path, document.to_string()).map_err(|source| Error::Io {
        action: "write",
        path: path.clone(),
        source,
    })?;

    tracing::info!(path = %path.display(), created, "a project manifest was written");

    Ok(Written {
        created,
        sites_kept,
    })
}

/// `[site]` or `[[sites]]`, in the form spec D1's table picks; answers the entries it kept.
///
/// The file has already been read through [`read()`], so it never holds both forms here.
fn write_sites(
    document: &mut toml_edit::DocumentMut,
    sites: &[ExportSite],
) -> std::result::Result<Vec<String>, String> {
    let many = document.contains_key("sites");

    if sites.is_empty() {
        return Ok(declared_domains(document));
    }

    if !many && sites.len() == 1 {
        set(document, "site", |table| fill(table, &sites[0]));
        return Ok(Vec::new());
    }

    // From here the file is, or becomes, `[[sites]]`. A `[site]` a person wrote is carried over
    // as the first entry — its keys, their order and the comment above its header with it.
    let single = document.remove("site");
    let item = document
        .entry("sites")
        .or_insert_with(|| toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));

    if item.as_array().is_some_and(toml_edit::Array::is_empty) {
        *item = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }

    let Some(entries) = item.as_array_of_tables_mut() else {
        return Err(
            "`sites` is not a list of `[[sites]]` tables, so no site can be written into it"
                .to_owned(),
        );
    };

    if let Some(toml_edit::Item::Table(table)) = single {
        let prefix = table.decor().prefix().cloned();
        entries.push(table);

        if let (Some(prefix), Some(first)) = (prefix, entries.iter_mut().last()) {
            first.decor_mut().set_prefix(prefix);
        }
    }

    // **D2: an entry belongs to the site holding its `domain` among any of its names.** A domain
    // belongs to one site in a home, so at most one site matches; the first entry to claim a site
    // has it, and a later one for the same site is kept and reported.
    let mut owner = vec![None; entries.len()];
    let mut taken = vec![false; sites.len()];

    for (position, entry) in entries.iter().enumerate() {
        let Some(domain) = entry.get("domain").and_then(toml_edit::Item::as_str) else {
            continue;
        };

        if let Some(index) =
            (0..sites.len()).find(|&index| !taken[index] && holds(&sites[index], domain))
        {
            taken[index] = true;
            owner[position] = Some(index);
        }
    }

    let mut kept = Vec::new();

    for (position, entry) in entries.iter_mut().enumerate() {
        match owner[position] {
            Some(index) => fill(entry, &sites[index]),
            None => kept.push(entry_name(entry)),
        }
    }

    for (index, site) in sites.iter().enumerate() {
        if !taken[index] {
            let mut fresh = toml_edit::Table::new();
            fill(&mut fresh, site);
            entries.push(fresh);
        }
    }

    Ok(kept)
}

/// Whether `domain` is one of this site's names.
fn holds(site: &ExportSite, domain: &str) -> bool {
    site.domain == domain || site.aliases.iter().any(|alias| alias == domain)
}

/// What an entry is called in a report: its `domain`, or `(no domain)` for one that names none.
fn entry_name(entry: &toml_edit::Table) -> String {
    entry
        .get("domain")
        .and_then(toml_edit::Item::as_str)
        .unwrap_or("(no domain)")
        .to_owned()
}

/// Every site entry the file holds, by name — what an export with no site leaves.
fn declared_domains(document: &toml_edit::DocumentMut) -> Vec<String> {
    if let Some(table) = document.get("site").and_then(toml_edit::Item::as_table) {
        return vec![entry_name(table)];
    }

    document
        .get("sites")
        .and_then(toml_edit::Item::as_array_of_tables)
        .map(|entries| entries.iter().map(entry_name).collect())
        .unwrap_or_default()
}

/// Every service any site links, once each, by `name` + `instance`.
fn every_service(sites: &[ExportSite]) -> Vec<ExportService> {
    let mut every: Vec<ExportService> = Vec::new();

    for service in sites.iter().flat_map(|site| &site.services) {
        if !every
            .iter()
            .any(|known| known.name == service.name && known.instance == service.instance)
        {
            every.push(service.clone());
        }
    }

    every
}

/// Write one site's owned keys into its table, leaving every other key as it is.
fn fill(table: &mut toml_edit::Table, site: &ExportSite) {
    table.set_implicit(false);
    table["domain"] = toml_edit::value(site.domain.as_str());

    // Written even when empty, because an alias removed in the database and left in the file
    // would be a file that disagrees with the home it came from — and `aliases` is a key this
    // export owns outright, unlike an entry of `[[services]]`.
    let mut aliases = toml_edit::Array::new();
    for alias in &site.aliases {
        aliases.push(alias.as_str());
    }
    table["aliases"] = toml_edit::value(aliases);

    table["doc_root"] = toml_edit::value(site.doc_root.as_str());
    table["https"] = toml_edit::value(site.https);

    // **Every kind's payload is this export's**, so a site that changed kind does not leave the
    // old one's key behind — roadmap task **T204**, spec D2. Removed before the match writes the
    // current kind's.
    table.remove("upstream");
    table.remove("port");

    // Exhaustive, so a fifth kind is a compile error here rather than a key silently missing from
    // somebody's manifest.
    match &site.kind {
        SiteKind::PhpFpm { .. } => {
            table["kind"] = toml_edit::value("php-fpm");
        }
        SiteKind::Static => {
            table["kind"] = toml_edit::value("static");
        }
        SiteKind::ReverseProxy { upstream } => {
            table["kind"] = toml_edit::value("reverse-proxy");
            table["upstream"] = toml_edit::value(upstream.as_str());
        }
        SiteKind::NodeApp { port } => {
            table["kind"] = toml_edit::value("node-app");
            table["port"] = toml_edit::value(i64::from(*port));
        }
    }

    // **Written whole, and written even when empty** — roadmap task **T135**, on `aliases`' rule
    // and for its reason: a route removed in the database and left in the file would be a file
    // that disagrees with the home it came from, and this is a key the export owns outright.
    let mut routes = toml_edit::ArrayOfTables::new();

    for route in &site.routes {
        let mut entry = toml_edit::Table::new();
        entry["path"] = toml_edit::value(route.path.as_str());

        // Exhaustive, so a fourth target is a compile error here rather than a key silently
        // missing from somebody's manifest.
        match &route.target {
            mixengine_proto::RouteTarget::Proxy { upstream } => {
                entry["target"] = toml_edit::value("proxy");
                entry["upstream"] = toml_edit::value(upstream.as_str());
            }
            mixengine_proto::RouteTarget::PhpFpm { pool } => {
                entry["target"] = toml_edit::value("php-fpm");

                if let Some(pool) = pool {
                    entry["pool"] = toml_edit::value(pool.as_str());
                }
            }
            mixengine_proto::RouteTarget::Static { root } => {
                entry["target"] = toml_edit::value("static");
                entry["root"] = toml_edit::value(root.as_str());
            }
        }

        routes.push(entry);
    }

    table["routes"] = toml_edit::Item::ArrayOfTables(routes);

    // **Written whole, and written even when empty** — roadmap task **T204**, spec D3, on
    // `aliases`' rule: a link removed in the database and left here would be a file that disagrees
    // with the home it came from.
    let mut links = toml_edit::Array::new();
    for service in &site.services {
        links.push(service.link.to_string());
    }
    table["services"] = toml_edit::value(links);
}

/// Add and update `[[services]]`; never delete.
///
/// **The honest consequence, stated rather than discovered: an export is a merge, not a mirror.**
/// Removing a link in the database does not remove its line from the file, and there is no
/// `--prune`. The alternative is an export that deletes a hand-written `database = "blog"` from a
/// file under version control, which is not a trade this makes.
///
/// Identity is `name` plus `instance`, with an absent `instance` in the file matching `main` —
/// the same rule the reader's lookup follows, so an export does not create a second entry for a
/// service the file already names.
fn merge_services(document: &mut toml_edit::DocumentMut, services: &[ExportService]) {
    let array = document
        .entry("services")
        .or_insert_with(|| toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));

    let Some(array) = array.as_array_of_tables_mut() else {
        // The file calls `services` something other than an array of tables. Left alone: this
        // export does not own the key badly enough to overwrite whatever a person meant by it.
        return;
    };

    for service in services {
        let existing = array.iter_mut().find(|table| {
            table.get("name").and_then(|value| value.as_str()) == Some(service.name.as_str())
                && table
                    .get("instance")
                    .and_then(|value| value.as_str())
                    .unwrap_or("main")
                    == service.instance
        });

        let table = match existing {
            Some(table) => table,
            None => {
                let mut fresh = toml_edit::Table::new();
                fresh["name"] = toml_edit::value(service.name.as_str());
                fresh["instance"] = toml_edit::value(service.instance.as_str());
                array.push(fresh);
                array.iter_mut().last().expect("the table just pushed")
            }
        };

        if let Some(version) = &service.version {
            table["version"] = toml_edit::value(version.as_str());
        }
    }
}

/// Reach one top-level table, creating it if the file has none, and edit it.
///
/// `set_implicit(false)` is what makes a created table render its own `[header]`: a table
/// `toml_edit` believes is implicit is one it prints only through its children, and a `[project]`
/// that never appears is a file the reader is right about and a person is confused by.
fn set(
    document: &mut toml_edit::DocumentMut,
    section: &str,
    edit: impl FnOnce(&mut toml_edit::Table),
) {
    let item = document
        .entry(section)
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));

    if let Some(table) = item.as_table_mut() {
        table.set_implicit(false);
        edit(table);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere() -> tempfile::TempDir {
        tempfile::tempdir().expect("a temporary directory")
    }

    fn export(sites: Vec<ExportSite>) -> Export {
        Export {
            name: "blog".to_owned(),
            pins: pins(&[(RuntimeKind::Php, "^8.3")]),
            sites,
        }
    }

    fn site(domain: &str, aliases: &[&str]) -> ExportSite {
        ExportSite {
            domain: domain.to_owned(),
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
            doc_root: String::new(),
            https: false,
            kind: mixengine_proto::SiteKind::Static,
            routes: Vec::new(),
            services: Vec::new(),
        }
    }

    fn linked(id: &str, version: &str) -> ExportService {
        let link = ServiceId::parse(id).expect("an id");
        ExportService {
            name: link.name().to_owned(),
            instance: link.instance().unwrap_or("main").to_owned(),
            link,
            version: Some(PackageVersion::parse(version).expect("a version")),
        }
    }

    /// **T135, D11.** A round trip that silently dropped half of what a site is would be worse than
    /// one that refused, so `[[site.routes]]` is written by an export and read by an import.
    #[test]
    fn routes_survive_an_export_and_come_back() {
        let home = somewhere();

        let routes = vec![
            mixengine_proto::SiteRoute {
                path: "/api".to_owned(),
                target: mixengine_proto::RouteTarget::Proxy {
                    upstream: "http://127.0.0.1:3003/xyz".to_owned(),
                },
            },
            mixengine_proto::SiteRoute {
                path: "/admin".to_owned(),
                target: mixengine_proto::RouteTarget::PhpFpm {
                    pool: Some(mixengine_proto::ServiceId::parse("php-fpm@8.3.33").expect("an id")),
                },
            },
            mixengine_proto::SiteRoute {
                path: "/assets".to_owned(),
                target: mixengine_proto::RouteTarget::Static {
                    root: "dist".to_owned(),
                },
            },
        ];

        write(
            home.path(),
            &export(vec![ExportSite {
                domain: "blog.test".to_owned(),
                aliases: Vec::new(),
                doc_root: String::new(),
                https: false,
                kind: mixengine_proto::SiteKind::NodeApp { port: 3000 },
                routes: routes.clone(),
                services: Vec::new(),
            }]),
        )
        .expect("a manifest");

        let manifest = read(&at(home.path())).expect("a read").expect("a manifest");
        let site = manifest.sites.into_iter().next().expect("a site");

        assert_eq!(site.routes, routes);
        assert_eq!(
            site.kind,
            Some(mixengine_proto::SiteKind::NodeApp { port: 3000 }),
            "the routes beside the kind do not confuse the kind"
        );
    }

    /// **D9.** The export writes the site, and everything the daemon does not own survives it —
    /// including a hand-written key inside an entry it *does* update.
    #[test]
    fn an_export_writes_the_site_and_leaves_every_hand_written_key_alone() {
        let home = somewhere();
        let original = "# the blog\n\
                        [runtimes]\n\
                        php = \"8.2\"\n\n\
                        [[services]]\n\
                        name = \"mariadb\"\n\
                        instance = \"main\"\n\
                        version = \"11.3\"\n\
                        database = \"blog\"      # provisioned by hand, for now\n\n\
                        [[services]]\n\
                        name = \"meilisearch\"   # nothing in this build knows what this is\n";
        std::fs::write(at(home.path()), original).expect("a manifest");

        write(
            home.path(),
            &export(vec![ExportSite {
                domain: "blog.test".to_owned(),
                aliases: vec!["api.blog.test".to_owned()],
                doc_root: "public".to_owned(),
                https: true,
                kind: mixengine_proto::SiteKind::PhpFpm { pool: None },
                routes: Vec::new(),
                services: vec![ExportService {
                    link: ServiceId::parse("mariadb@main").expect("an id"),
                    name: "mariadb".to_owned(),
                    instance: "main".to_owned(),
                    version: Some(PackageVersion::parse("11.4.2").expect("a version")),
                }],
            }]),
        )
        .expect("it is written");

        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        assert!(after.contains("# the blog"), "{after}");
        assert!(after.contains("domain = \"blog.test\""), "{after}");
        assert!(after.contains("aliases = [\"api.blog.test\"]"), "{after}");
        assert!(after.contains("doc_root = \"public\""), "{after}");
        assert!(after.contains("kind = \"php-fpm\""), "{after}");
        assert!(after.contains("https = true"), "{after}");

        // Add and update; never delete.
        assert!(
            after.contains("version = \"11.4.2\""),
            "the link was updated: {after}"
        );
        assert!(
            after.contains("database = \"blog\""),
            "a hand-written key inside an updated entry survives: {after}"
        );
        assert!(
            after.contains("meilisearch"),
            "an entry the daemon knows nothing about is left exactly as it is: {after}"
        );
        assert!(after.contains("# provisioned by hand"), "{after}");
    }

    /// A kind's payload is written from an exhaustive match, so a fifth kind cannot be forgotten.
    #[test]
    fn a_proxy_is_written_with_the_address_it_forwards_to() {
        let home = somewhere();

        write(
            home.path(),
            &export(vec![ExportSite {
                domain: "app.test".to_owned(),
                aliases: Vec::new(),
                doc_root: String::new(),
                https: false,
                kind: mixengine_proto::SiteKind::ReverseProxy {
                    upstream: "http://127.0.0.1:5173".to_owned(),
                },
                routes: Vec::new(),
                services: Vec::new(),
            }]),
        )
        .expect("it is written");

        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        assert!(after.contains("kind = \"reverse-proxy\""), "{after}");
        assert!(
            after.contains("upstream = \"http://127.0.0.1:5173\""),
            "{after}"
        );

        // And it reads back as the kind it was written from.
        let read_back = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there")
            .sites
            .into_iter()
            .next()
            .expect("a [site]");
        assert_eq!(
            read_back.kind,
            Some(mixengine_proto::SiteKind::ReverseProxy {
                upstream: "http://127.0.0.1:5173".to_owned()
            })
        );
    }

    /// A project with no site writes no `[site]`, and does not delete one somebody wrote by hand.
    #[test]
    fn an_export_with_no_site_leaves_a_hand_written_one_alone() {
        let home = somewhere();
        std::fs::write(at(home.path()), "[site]\ndomain = \"typed.test\"\n").expect("a manifest");

        write(home.path(), &export(Vec::new())).expect("it is written");

        let after = std::fs::read_to_string(at(home.path())).expect("the file");
        assert!(after.contains("typed.test"), "{after}");
    }

    /// **D7.** `[site]` with no `kind` is a manifest this build has always accepted, and the type
    /// has to keep accepting it.
    #[test]
    fn a_site_with_no_kind_reads_as_one_that_named_none() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[site]\ndomain = \"blog.test\"\naliases = [\"api.blog.test\"]\ndoc_root = \"public\"\n",
        )
        .expect("a manifest");

        let site = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there")
            .sites
            .into_iter()
            .next()
            .expect("a [site]");

        assert_eq!(site.domain.as_deref(), Some("blog.test"));
        assert_eq!(site.aliases, ["api.blog.test"]);
        assert_eq!(site.doc_root.as_deref(), Some("public"));
        assert_eq!(site.kind, None, "no kind is not the same as php-fpm");
        assert_eq!(site.https, None);
    }

    /// A kind reads out of the flat table beside the keys it has no use for.
    #[test]
    fn a_kind_reads_from_the_table_it_shares_with_the_rest_of_the_site() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[site]\ndomain = \"blog.test\"\nkind = \"reverse-proxy\"\n\
             upstream = \"http://127.0.0.1:5173\"\nhttps = true\n",
        )
        .expect("a manifest");

        let site = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there")
            .sites
            .into_iter()
            .next()
            .expect("a [site]");

        assert_eq!(
            site.kind,
            Some(mixengine_proto::SiteKind::ReverseProxy {
                upstream: "http://127.0.0.1:5173".to_owned()
            })
        );
        assert_eq!(site.https, Some(true));
    }

    /// And a kind that cannot be one is refused by the enum, naming the file.
    #[test]
    fn a_proxy_with_no_upstream_is_refused_by_the_definition() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[site]\ndomain = \"blog.test\"\nkind = \"reverse-proxy\"\n",
        )
        .expect("a manifest");

        let error = read(&at(home.path())).expect_err("a proxy with nowhere to go");

        assert!(
            matches!(&error, Error::Manifest { path, .. } if path.ends_with(FILE_NAME)),
            "{error:?}"
        );
    }

    /// **D8.** `[[services]]` is read as a name, an instance and a constraint — and the keys this
    /// build does not interpret survive being read past.
    #[test]
    fn services_are_read_as_names_instances_and_constraints() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[[services]]\nname = \"mariadb\"\ninstance = \"main\"\nversion = \"11.4\"\n\
             database = \"blog\"\n\n[[services]]\nname = \"redis\"\n",
        )
        .expect("a manifest");

        let services = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there")
            .services;

        assert_eq!(services.len(), 2);
        assert_eq!(services[0].name, "mariadb");
        assert_eq!(services[0].instance.as_deref(), Some("main"));
        assert_eq!(
            services[0].version.as_ref().map(VersionConstraint::as_str),
            Some("11.4")
        );
        assert_eq!(
            services[1].instance, None,
            "absent is not the same as \"main\""
        );
        assert_eq!(services[1].version, None);
    }

    /// **T77, and the note this overturns.** `database` and `user` used to pass through unread, on
    /// the rule that a key nothing acts on is a promise not kept. `blueprint.capture` is what acts
    /// on them: without this, a blueprint captured from a Laravel project carries no database name
    /// and the site it creates elsewhere connects to nothing.
    #[test]
    fn a_service_entry_carries_the_database_and_the_account_it_names() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[[services]]
name = \"mariadb\"
instance = \"main\"
database = \"blog\"
             user = \"blog\"

[[services]]
name = \"redis\"
",
        )
        .expect("a manifest");

        let services = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there")
            .services;

        assert_eq!(services[0].database.as_deref(), Some("blog"));
        assert_eq!(services[0].user.as_deref(), Some("blog"));
        assert_eq!(
            services[1].database, None,
            "a service that names no database has none"
        );
    }

    /// A `version` whose *syntax* is wrong is the file being wrong, and is refused. Whether
    /// anything installed satisfies it is a different question, asked by the daemon and reported.
    #[test]
    fn a_version_that_is_not_a_constraint_is_refused() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[[services]]\nname = \"mariadb\"\nversion = \"~11.4\"\n",
        )
        .expect("a manifest");

        assert!(matches!(
            read(&at(home.path())),
            Err(Error::Manifest { .. })
        ));
    }

    fn pins(entries: &[(RuntimeKind, &str)]) -> BTreeMap<RuntimeKind, VersionConstraint> {
        entries
            .iter()
            .map(|(kind, text)| {
                (
                    *kind,
                    VersionConstraint::parse((*text).to_owned()).expect("a constraint"),
                )
            })
            .collect()
    }

    /// The whole file, where `resolve` used to read a third of it — and the sections this build has
    /// no types for still must not make it refuse the file.
    #[test]
    fn a_manifest_declaring_more_than_runtimes_is_read_whole() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[project]\nname = \"blog\"\n\n[runtimes]\nphp = \"^8.3\"\n\n\
             [site]\ndomain = \"blog.test\"\n\n[[services]]\nname = \"redis\"\n",
        )
        .expect("a manifest");

        let manifest = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there");

        assert_eq!(
            manifest.project.and_then(|project| project.name).as_deref(),
            Some("blog")
        );
        assert_eq!(
            manifest
                .runtimes
                .get(&RuntimeKind::Php)
                .map(VersionConstraint::as_str),
            Some("^8.3")
        );
    }

    /// A directory with no manifest is not a failure — it is the ordinary case.
    #[test]
    fn a_directory_with_no_manifest_answers_nothing_rather_than_failing() {
        let home = somewhere();

        assert_eq!(read(&at(home.path())).expect("no manifest is fine"), None);
    }

    /// A pin that does nothing looks exactly like a pin that does not work, so the file is refused
    /// by name — `Error::Manifest`'s own reasoning, kept when the reader moved here.
    #[test]
    fn a_manifest_that_does_not_parse_names_itself() {
        let home = somewhere();

        for body in [
            "[runtimes]\nphp = \"~8.3\"\n",
            "[runtimes]\nphhp = \"8.3\"\n",
            "[runtimes\n",
        ] {
            std::fs::write(at(home.path()), body).expect("a manifest");

            let error = read(&at(home.path())).expect_err("the manifest is wrong");

            assert!(
                matches!(&error, Error::Manifest { path, .. } if path.ends_with(FILE_NAME)),
                "{error:?} for {body:?}"
            );
        }
    }

    /// **What D10 is for.** An export is written into somebody's version-controlled file, so
    /// everything it does not own survives it byte for byte.
    #[test]
    fn writing_a_manifest_keeps_the_comments_the_order_and_the_sections_it_does_not_own() {
        let home = somewhere();
        let original = "# the blog\n\
                        [runtimes]\n\
                        node = \"22\"      # the front end build\n\
                        php = \"8.2\"\n\n\
                        [site]\n\
                        domain = \"blog.test\"\n\n\
                        [[services]]\n\
                        name = \"redis\"\n";
        std::fs::write(at(home.path()), original).expect("a manifest");

        let created = write(home.path(), &export(Vec::new()))
            .expect("it is written")
            .created;

        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        assert!(!created, "the file was already there");
        assert!(after.contains("# the blog"), "{after}");
        assert!(after.contains("# the front end build"), "{after}");
        assert!(
            after.contains("[site]") && after.contains("blog.test"),
            "{after}"
        );
        assert!(after.contains("[[services]]"), "{after}");
        assert!(
            after.find("node =").expect("node") < after.find("php =").expect("php"),
            "the key order the user chose is theirs: {after}"
        );
        assert!(
            after.contains("php = \"^8.3\""),
            "the owned key changed: {after}"
        );
        assert!(
            after.contains("name = \"blog\""),
            "the name was written: {after}"
        );

        // And what it wrote is what the reader reads back.
        let manifest = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there");
        assert_eq!(
            manifest
                .runtimes
                .get(&RuntimeKind::Php)
                .map(VersionConstraint::as_str),
            Some("^8.3")
        );
    }

    /// A directory with no manifest gets one, and says that it did.
    #[test]
    fn a_directory_with_no_manifest_gets_one_written() {
        let home = somewhere();

        let created = write(home.path(), &export(Vec::new()))
            .expect("it is written")
            .created;

        assert!(created);
        assert_eq!(
            read(&at(home.path()))
                .expect("it parses")
                .expect("it is there")
                .project
                .and_then(|project| project.name)
                .as_deref(),
            Some("blog")
        );
    }

    /// **T204, D1.** `[[sites]]` reads as its entries, in file order.
    #[test]
    fn several_sites_read_in_the_order_the_file_writes_them() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[[sites]]\ndomain = \"web.test\"\nkind = \"static\"\n\n\
             [[sites]]\ndomain = \"api.test\"\nkind = \"reverse-proxy\"\n\
             upstream = \"http://127.0.0.1:3000\"\n",
        )
        .expect("a manifest");

        let sites = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there")
            .sites;

        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].domain.as_deref(), Some("web.test"));
        assert_eq!(sites[1].domain.as_deref(), Some("api.test"));
    }

    /// **T204, D1.** One `[site]` is a list of one, which every file written before T204 is.
    #[test]
    fn a_single_site_reads_as_a_list_of_one() {
        let home = somewhere();
        std::fs::write(at(home.path()), "[site]\ndomain = \"blog.test\"\n").expect("a manifest");

        let sites = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there")
            .sites;

        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].domain.as_deref(), Some("blog.test"));
    }

    /// **T204, D1.** Both forms at once is a file whose author meant one of them, and choosing would
    /// apply something they did not write.
    #[test]
    fn a_file_with_both_forms_is_refused() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[site]\ndomain = \"a.test\"\n\n[[sites]]\ndomain = \"b.test\"\n",
        )
        .expect("a manifest");

        let error = read(&at(home.path())).expect_err("two answers to one question");

        assert!(
            matches!(&error, Error::Manifest { path, .. } if path.ends_with(FILE_NAME)),
            "{error:?}"
        );
        assert!(format!("{error:?}").contains("[[sites]]"), "{error:?}");
    }

    /// **T204, D3.** An absent `services` is not an empty one: absent falls through to every
    /// `[[services]]` entry, and `[]` links none.
    #[test]
    fn an_absent_services_list_is_told_apart_from_an_empty_one() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[[sites]]\ndomain = \"a.test\"\n\n\
             [[sites]]\ndomain = \"b.test\"\nservices = []\n\n\
             [[sites]]\ndomain = \"c.test\"\nservices = [\"mariadb@main\", \"redis\"]\n",
        )
        .expect("a manifest");

        let sites = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there")
            .sites;

        assert_eq!(sites[0].services, None);
        assert_eq!(sites[1].services, Some(Vec::new()));
        assert_eq!(
            sites[2].services,
            Some(vec![
                ServiceId::parse("mariadb@main").expect("an id"),
                ServiceId::parse("redis").expect("an id"),
            ])
        );
    }

    /// **T204, D3.** A `services` entry that cannot be an id is the file being wrong.
    #[test]
    fn a_services_entry_that_is_not_an_id_is_refused() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[site]\ndomain = \"a.test\"\nservices = [\"Not An Id\"]\n",
        )
        .expect("a manifest");

        assert!(matches!(
            read(&at(home.path())),
            Err(Error::Manifest { .. })
        ));
    }

    /// **T204, D5.** A duplicate domain is refused where a site is chosen, never here, so the shim's
    /// `resolve` keeps working in that directory.
    #[test]
    fn a_duplicate_domain_still_reads() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[runtimes]\nphp = \"8.3\"\n\n[[sites]]\ndomain = \"a.test\"\n\n\
             [[sites]]\ndomain = \"a.test\"\n",
        )
        .expect("a manifest");

        let manifest = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there");

        assert_eq!(manifest.sites.len(), 2);
        assert_eq!(manifest.runtimes.len(), 1);
    }

    /// **T204, D1.** Two sites into a file with none is `[[sites]]`, each with its own links, and
    /// `[[services]]` is the union.
    #[test]
    fn two_sites_are_written_as_many_with_their_own_services() {
        let home = somewhere();
        let mut web = site("web.test", &[]);
        web.services = vec![linked("mariadb@main", "11.4.2")];
        let mut api = site("api.test", &[]);
        api.services = vec![
            linked("mariadb@main", "11.4.2"),
            linked("redis@main", "7.4.1"),
        ];

        let written = write(home.path(), &export(vec![web, api])).expect("it is written");
        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        assert!(written.sites_kept.is_empty(), "{written:?}");
        assert!(!after.contains("[site]"), "{after}");
        assert_eq!(after.matches("[[sites]]").count(), 2, "{after}");
        assert!(after.contains("services = [\"mariadb@main\"]"), "{after}");
        assert!(
            after.contains("services = [\"mariadb@main\", \"redis@main\"]"),
            "{after}"
        );
        assert_eq!(
            after.matches("[[services]]").count(),
            2,
            "one entry per service: {after}"
        );

        let sites = read(&at(home.path()))
            .expect("it parses")
            .expect("it is there")
            .sites;
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[1].services.as_ref().map(Vec::len), Some(2));
    }

    /// **T204, D1.** One site stays `[site]`, the form every released build reads.
    #[test]
    fn one_site_is_still_written_as_one() {
        let home = somewhere();

        write(home.path(), &export(vec![site("blog.test", &[])])).expect("it is written");
        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        assert!(after.contains("[site]"), "{after}");
        assert!(!after.contains("[[sites]]"), "{after}");
        assert!(
            after.contains("services = []"),
            "D3: written in both forms: {after}"
        );
    }

    /// **T204, D1.** A file already in `[[sites]]` stays there when only one site is left, so the
    /// diff shows the sites changing rather than the form.
    #[test]
    fn a_file_in_many_stays_in_many_with_one_site() {
        let home = somewhere();
        std::fs::write(at(home.path()), "[[sites]]\ndomain = \"blog.test\"\n").expect("a manifest");

        write(home.path(), &export(vec![site("blog.test", &[])])).expect("it is written");
        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        assert_eq!(after.matches("[[sites]]").count(), 1, "{after}");
        assert!(!after.contains("[site]"), "{after}");
    }

    /// **T204, D1.** A `[site]` a person wrote becomes the first `[[sites]]`, with its comment, its
    /// unknown key and their order.
    #[test]
    fn a_single_site_becomes_the_first_of_many_with_its_comment_and_keys() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[project]\nname = \"blog\"\n\n# the main site\n[site]\ndomain = \"blog.test\"\n\
             # nothing in this build reads this\nlegacy = 1\n",
        )
        .expect("a manifest");

        write(
            home.path(),
            &export(vec![site("blog.test", &[]), site("shop.test", &[])]),
        )
        .expect("it is written");
        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        assert!(!after.contains("[site]"), "{after}");
        let first = after.find("[[sites]]").expect("an entry");
        let comment = after.find("# the main site").expect("the comment survives");
        assert!(
            comment < first,
            "the comment stays above its header: {after}"
        );
        assert!(
            after.contains("# nothing in this build reads this\nlegacy = 1"),
            "{after}"
        );
        let blog = after.find("domain = \"blog.test\"").expect("blog");
        let shop = after.find("domain = \"shop.test\"").expect("shop");
        assert!(blog < shop, "the converted entry is first: {after}");
    }

    /// **T204, D2.** An entry belongs to the site holding its `domain` among any of its names, so a
    /// renamed primary updates the entry rather than adding a second one.
    #[test]
    fn an_entry_is_found_by_an_alias_after_the_primary_moved() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[[sites]]\ndomain = \"blog.test\"\nnote = \"kept\"\n\n\
             [[sites]]\ndomain = \"shop.test\"\n",
        )
        .expect("a manifest");

        let written = write(
            home.path(),
            &export(vec![
                site("www.blog.test", &["blog.test"]),
                site("shop.test", &[]),
            ]),
        )
        .expect("it is written");
        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        assert!(written.sites_kept.is_empty(), "{written:?}");
        assert_eq!(after.matches("[[sites]]").count(), 2, "{after}");
        assert!(after.contains("domain = \"www.blog.test\""), "{after}");
        assert!(after.contains("aliases = [\"blog.test\"]"), "{after}");
        assert!(
            after.contains("note = \"kept\""),
            "an unowned key survives: {after}"
        );
    }

    /// **T204, D2 and D4.** An entry no site holds is left byte for byte and named; so is a second
    /// entry for a site an earlier one already took.
    #[test]
    fn an_entry_no_site_holds_is_kept_and_named() {
        let home = somewhere();
        std::fs::write(
            at(home.path()),
            "[[sites]]\ndomain = \"old.test\"   # deleted here, still in git\n\n\
             [[sites]]\ndomain = \"blog.test\"\n\n[[sites]]\ndomain = \"www.blog.test\"\n",
        )
        .expect("a manifest");

        let written = write(
            home.path(),
            &export(vec![
                site("blog.test", &["www.blog.test"]),
                site("new.test", &[]),
            ]),
        )
        .expect("it is written");
        let after = std::fs::read_to_string(at(home.path())).expect("the file");

        assert_eq!(written.sites_kept, ["old.test", "www.blog.test"]);
        assert!(
            after.contains("domain = \"old.test\"   # deleted here, still in git"),
            "{after}"
        );
        assert!(after.contains("domain = \"new.test\""), "{after}");
    }

    /// **T204, D4.** A project with no site leaves a hand-written one and names it.
    #[test]
    fn no_site_names_every_entry_it_left() {
        let home = somewhere();
        std::fs::write(at(home.path()), "[site]\ndomain = \"typed.test\"\n").expect("a manifest");

        let written = write(home.path(), &export(Vec::new())).expect("it is written");

        assert_eq!(written.sites_kept, ["typed.test"]);
    }

    /// **T204, D2.** The kind's payload keys are owned, so a proxy that became static loses its
    /// `upstream` in either form.
    #[test]
    fn a_kind_that_changed_takes_its_old_payload_with_it() {
        for original in [
            "[site]\ndomain = \"app.test\"\nkind = \"reverse-proxy\"\n\
             upstream = \"http://127.0.0.1:1\"\n",
            "[[sites]]\ndomain = \"app.test\"\nkind = \"node-app\"\nport = 3000\n",
        ] {
            let home = somewhere();
            std::fs::write(at(home.path()), original).expect("a manifest");

            write(home.path(), &export(vec![site("app.test", &[])])).expect("it is written");
            let after = std::fs::read_to_string(at(home.path())).expect("the file");

            assert!(after.contains("kind = \"static\""), "{after}");
            assert!(!after.contains("upstream"), "{after}");
            assert!(!after.contains("port"), "{after}");
        }
    }

    fn declaring(text: &str) -> Manifest {
        toml::from_str(text).expect("a manifest")
    }

    const THREE: &str = "[[sites]]\ndomain = \"web.test\"\naliases = [\"www.web.test\"]\n\n\
                         [[sites]]\ndomain = \"api.test\"\nservices = [\"redis\"]\n\n\
                         [[sites]]\ndomain = \"admin.test\"\nservices = []\n";

    /// **T204, D5, step 1.** `from` picks by any of an entry's names.
    #[test]
    fn from_picks_an_entry_by_any_of_its_names() {
        let manifest = declaring(THREE);

        let chosen = manifest
            .choose(Some("www.web.test"), None, "blog.test")
            .expect("chosen");

        assert_eq!(
            chosen.site.and_then(|site| site.domain.as_deref()),
            Some("web.test")
        );
        assert_eq!(
            chosen.services,
            ServicesFrom::Every,
            "no `services` key: every one"
        );
    }

    /// **T204, D5, step 1.** A `from` naming nothing the file declares is `NotDeclared`, carrying
    /// what it does declare.
    #[test]
    fn from_naming_nothing_is_not_declared() {
        let manifest = declaring(THREE);

        assert_eq!(
            manifest.choose(Some("nope.test"), None, "blog.test"),
            Err(Unchosen::NotDeclared {
                name: "nope.test".to_owned(),
                declared: vec!["web.test".into(), "api.test".into(), "admin.test".into()],
            })
        );
    }

    /// **T204, D5, step 2.** A named domain picks its entry; one the file does not describe picks
    /// none, and no services fall through.
    #[test]
    fn a_named_domain_picks_its_entry_or_none() {
        let manifest = declaring(THREE);
        let redis = [ServiceId::parse("redis").expect("an id")];

        let api = manifest
            .choose(None, Some(&["api.test".to_owned()]), "blog.test")
            .expect("chosen");
        assert_eq!(api.services, ServicesFrom::Listed(&redis));

        let other = manifest
            .choose(None, Some(&["other.test".to_owned()]), "blog.test")
            .expect("chosen");
        assert_eq!(other.site, None);
        assert_eq!(other.services, ServicesFrom::Nothing);
    }

    /// **T204, D5, steps 2 and 3.** One entry is today's behaviour: chosen whatever the domain.
    #[test]
    fn one_entry_is_chosen_as_it_always_was() {
        let manifest = declaring("[site]\ndomain = \"blog.test\"\n");

        for domains in [None, Some(vec!["other.test".to_owned()])] {
            let chosen = manifest
                .choose(None, domains.as_deref(), "blog.test")
                .expect("chosen");
            assert!(chosen.site.is_some());
            assert_eq!(chosen.services, ServicesFrom::Every);
        }
    }

    /// **T204, D5, step 3.** Several and nothing named is refused rather than guessed.
    #[test]
    fn several_and_nothing_named_is_ambiguous() {
        assert!(matches!(
            declaring(THREE).choose(None, None, "blog.test"),
            Err(Unchosen::Ambiguous { declared }) if declared.len() == 3
        ));
    }

    /// **T204, D5.** No site at all: the defaults, with `[[services]]` as before.
    #[test]
    fn no_entry_falls_through_to_every_service() {
        let manifest = Manifest::default();

        let chosen = manifest.choose(None, None, "blog.test").expect("chosen");

        assert_eq!(chosen.site, None);
        assert_eq!(chosen.services, ServicesFrom::Every);
    }

    /// **T204, D5.** Two entries answering to one name is refused at the point of choosing,
    /// numbered the way a person counts them.
    #[test]
    fn named_twice() {
        let manifest = declaring(
            "[[sites]]\ndomain = \"a.test\"\n\n\
             [[sites]]\ndomain = \"b.test\"\naliases = [\"a.test\"]\n",
        );

        assert_eq!(
            manifest.choose(Some("a.test"), None, "blog.test"),
            Err(Unchosen::NamedTwice {
                name: "a.test".into(),
                first: 1,
                second: 2
            })
        );
    }

    /// **T204, D7.** An entry with no `domain` answers to the name `site.create` would give it.
    #[test]
    fn an_entry_with_no_domain_answers_to_the_default() {
        let manifest =
            declaring("[[sites]]\ndoc_root = \"public\"\n\n[[sites]]\ndomain = \"b.test\"\n");

        let chosen = manifest
            .choose(Some("blog.test"), None, "blog.test")
            .expect("chosen");

        assert_eq!(
            chosen.site.and_then(|site| site.doc_root.as_deref()),
            Some("public")
        );
    }
}
