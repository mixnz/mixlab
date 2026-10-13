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
        // In any case, on [`holds`]' reasoning: MixLab sends the lowercase name the daemon lists.
        let answers = site
            .domain
            .as_deref()
            .unwrap_or(default_domain)
            .eq_ignore_ascii_case(name)
            || site
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name));

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

    // **No pins, no table** — an empty `[runtimes]` says nothing and reads as if something were
    // meant to be there. A table the file already has is left as it is either way.
    if !export.pins.is_empty() {
        set(&mut document, "runtimes", |table| {
            for (kind, constraint) in &export.pins {
                table[kind.as_str()] = toml_edit::value(constraint.as_str());
            }
        });
    }

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

/// Whether `domain` is one of this site's names, in any case: the daemon stores names lowercased,
/// and a person writing the file may not have.
fn holds(site: &ExportSite, domain: &str) -> bool {
    site.domain.eq_ignore_ascii_case(domain)
        || site
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(domain))
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
    // old one's key behind — roadmap task **T204**, spec D2. Only the keys the current kind does
    // not use are removed: the one it does use is overwritten where it stands, so an export with
    // nothing new to say moves no line in somebody's diff.
    if !matches!(site.kind, SiteKind::ReverseProxy { .. }) {
        table.remove("upstream");
    }
    if !matches!(site.kind, SiteKind::NodeApp { .. }) {
        table.remove("port");
    }

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
mod tests;
