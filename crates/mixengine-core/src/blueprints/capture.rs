//! Turning a project that already works into a manifest.
//!
//! Roadmap task **T77**. The whole difficulty is in one sentence of the feature doc: capture *what
//! is actually in use*, **not the global defaults**, and never data, credentials or absolute paths.
//! Each of the rules below is that sentence applied to one thing this home knows.
//!
//! # Where each key comes from
//!
//! | Key | Read from |
//! |---|---|
//! | `[runtimes]` | [`crate::resolve`], keeping only what the project or its manifest decided (D4a) |
//! | `[site]` / `[[sites]]` | the project's `sites` rows in primary-domain order, their `site_domains`, and nothing about ports (T204a) |
//! | `[[sites]] services` | each site's `site_service_links`, minus the front end and the pools (T204a) |
//! | `[[services]]` | every site's links, each once, minus the front end and minus the pools |
//! | `[[services]] database`, `user` | the project's own `mixengine.toml` (D3) |
//! | `[php] extensions` | the *choices* on the PHP the pool runs, which are already deviations (D2) |
//!
//! # What never comes out of here
//!
//! `root_path` and every other absolute path; passwords, keyring entries and database contents;
//! `http_port` and `https_port`, which are properties of this machine's front end; LAN sharing,
//! which is something a person turns on for a machine and a moment; and `[scaffold]`, because
//! capture does not invent a command to run on somebody else's computer.
//!
//! The test that holds this is written against the **rendered string** rather than against the
//! struct: asserting on fields would only prove that the ones we remembered are empty.

use std::collections::BTreeMap;

use mixengine_proto::{RuntimeKind, RuntimeSource, ServiceId, SiteKind, VersionConstraint};

use crate::blueprints::manifest::{
    BlueprintManifest, BlueprintService, BlueprintSite, Header, PER_PROJECT, Php, Provenance,
};
use crate::projects::ProjectRecord;
use crate::{Error, Result, Store, manifest, resolve, services, sites};

/// The token a project's own name is replaced by.
const TOKEN: &str = "{project}";

/// Packages that serve every site on the machine rather than belonging to one project.
///
/// Left out of `[[services]]` because they belong to whoever receives the blueprint: a machine has
/// one front end, and a blueprint that asked for a second would be describing this home rather than
/// the project captured from it.
const FRONT_ENDS: [&str; 2] = ["caddy", "nginx"];

/// The package a php-fpm pool is an instance of.
///
/// Also left out, and for a different reason: the pool is already said by `[runtimes] php` together
/// with `kind = "php-fpm"`, so a `[[services]]` entry for it would be the same fact written twice —
/// and the second copy would carry this machine's instance name.
const POOL: &str = "php-fpm";

/// Everything a capture needs that is not in the database.
#[derive(Debug, Clone, Copy)]
pub struct Asked<'a> {
    /// The project to read.
    pub project: &'a ProjectRecord,

    /// The blueprint's name.
    pub name: &'a str,

    /// What it is for.
    pub description: &'a str,

    /// `windows`, `macos` or `linux` — from the platform layer, never from a `cfg!` in this crate.
    pub os: &'a str,

    /// This build's version.
    pub version: &'a str,

    /// The moment, as the caller reads the clock. Passed in so a test can have one.
    pub created_at: &'a str,
}

/// Write down what a project is made of.
///
/// # Errors
///
/// [`Error::ProjectRunsSeveralPhps`] for a project whose sites a single `[runtimes] php` cannot
/// describe; [`Error::Database`] when a table cannot be read, and [`Error::Manifest`] when the
/// project's own `mixengine.toml` does not parse.
pub async fn capture(store: &Store, asked: &Asked<'_>) -> Result<BlueprintManifest> {
    let project = asked.project;
    // **Every site, in primary-domain order** — roadmap task **T204a**, D6: `sites::records`'s
    // own, so capturing twice still writes one file twice.
    let records = sites::records(store, Some(project.id)).await?;
    let declared = manifest::read(&manifest::at(&project.root))?;

    let (pool, php_version) = one_php(store, project, &records).await?;

    // Every site's links, each once, in order of first appearance: what `[[services]]` holds.
    let mut union: Vec<ServiceId> = Vec::new();
    for record in &records {
        for service in record.services.iter().filter(|service| kept(service)) {
            if !union.contains(service) {
                union.push(service.clone());
            }
        }
    }

    let services = linked(store, project, &union, pool.as_ref(), declared.as_ref()).await?;

    // **How a site names one of those** (D3): by `name`, or `name@instance` as the file writes the
    // instance when two entries share a name. `linked` keeps `union`'s order one for one.
    let item_of = |service: &ServiceId| -> Option<String> {
        let position = union.iter().position(|one| one == service)?;
        let entry = services.get(position)?;
        let shared = services
            .iter()
            .filter(|other| other.name == entry.name)
            .count()
            > 1;
        Some(match (&entry.instance, shared) {
            (Some(instance), true) => format!("{}@{instance}", entry.name),
            _ => entry.name.clone(),
        })
    };

    let several = records.len() > 1;
    let sites: Vec<BlueprintSite> = records
        .iter()
        .map(|site| BlueprintSite {
            // The pool is dropped: which pool a site uses is a fact about the machine it was
            // created on, and the receiving machine decides its own.
            kind: match &site.kind {
                SiteKind::PhpFpm { .. } => SiteKind::PhpFpm { pool: None },
                other => other.clone(),
            },
            doc_root: site.doc_root.clone(),
            https: site.https_enabled,
            domain_pattern: site
                .domains
                .first()
                .map(|domain| tokenised_domain(domain, &project.name))
                .unwrap_or_default(),
            aliases: site
                .domains
                .iter()
                .skip(1)
                .map(|domain| tokenised_domain(domain, &project.name))
                .collect(),
            // **The same rule one table down** — roadmap task **T135**. A route's pool is a fact
            // about this machine; everything else about the route travels.
            routes: site
                .routes
                .iter()
                .map(|route| mixengine_proto::SiteRoute {
                    path: route.path.clone(),
                    target: match &route.target {
                        mixengine_proto::RouteTarget::PhpFpm { .. } => {
                            mixengine_proto::RouteTarget::PhpFpm { pool: None }
                        }
                        other => other.clone(),
                    },
                })
                .collect(),
            // **Said for every site when there are several** (D6), `[]` included, so the file
            // says what each one links rather than falling back to *every service*. One site
            // links them all, which `[site]` already means.
            services: several.then(|| {
                site.services
                    .iter()
                    .filter(|service| kept(service))
                    .filter_map(item_of)
                    .collect()
            }),
        })
        .collect();

    // **As the blueprint has them** — roadmap task **T205**, D5. Unexpanded in the row, so nothing
    // here turns a slug back into `{project}`.
    let next_steps = match crate::blueprints::steps::declared(store, project.id).await? {
        None => Vec::new(),
        Some((origin, _)) => {
            let mut steps = origin.next_steps;

            // **A step with no site, now that there are several** (T204a, D6): it belonged to the
            // one site its blueprint described, so it goes to the entry with that pattern, or to
            // the first. Without this, a project that gained a second site by hand would capture
            // into a file this build refuses to read.
            if several {
                let home = origin
                    .sites
                    .first()
                    .map(|site| site.domain_pattern.as_str())
                    .filter(|pattern| sites.iter().any(|site| site.domain_pattern == *pattern))
                    .or_else(|| sites.first().map(|site| site.domain_pattern.as_str()))
                    .map(str::to_owned);

                for step in &mut steps {
                    if step.site.is_none() {
                        step.site.clone_from(&home);
                    }
                }
            }

            steps
        }
    };

    Ok(BlueprintManifest {
        // **The lowest schema, because capture never writes an archive** — ADR 0061. `SCHEMA` is
        // what this build reads, not what a capture holds, and `render` writes several sites at
        // 3 whatever this says (T204a).
        schema: 1,
        blueprint: Header {
            name: asked.name.to_owned(),
            description: asked.description.to_owned(),
            created_at: asked.created_at.to_owned(),
            created_on: Provenance {
                os: asked.os.to_owned(),
                version: asked.version.to_owned(),
            },
        },
        runtimes: runtimes(store, project, php_version.as_ref()).await?,
        sites,
        services,
        php: match php_version.as_ref() {
            Some(version) => extensions(store, version).await?,
            None => None,
        },
        // **Never.** Capture does not invent a command to execute on somebody else's machine.
        scaffold: None,
        archive: None,
        next_steps,
    })
}

/// Whether a link is the project's own: not a front end, which belongs to the machine, and not a
/// php-fpm pool, which `[runtimes] php` already says.
fn kept(service: &ServiceId) -> bool {
    !FRONT_ENDS.contains(&service.name()) && service.name() != POOL
}

/// The PHP the project's php-fpm sites run, and the pool the first of them names — roadmap task
/// **T204a**, D6.
///
/// # Errors
///
/// [`Error::ProjectRunsSeveralPhps`] when they run more than one version.
async fn one_php(
    store: &Store,
    project: &ProjectRecord,
    records: &[sites::SiteRecord],
) -> Result<(Option<ServiceId>, Option<mixengine_proto::PackageVersion>)> {
    let mut first: Option<(ServiceId, Option<mixengine_proto::PackageVersion>)> = None;
    let mut seen: Vec<(String, String)> = Vec::new();

    for record in records {
        let SiteKind::PhpFpm { pool: Some(pool) } = &record.kind else {
            continue;
        };
        let version = services::version(store, pool).await?;

        if let Some(version) = &version {
            seen.push((
                record.domains.first().cloned().unwrap_or_default(),
                version.as_str().to_owned(),
            ));
        }
        if first.is_none() {
            first = Some((pool.clone(), version));
        }
    }

    let distinct = seen
        .iter()
        .map(|(_, version)| version.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if distinct.len() > 1 {
        return Err(Error::ProjectRunsSeveralPhps {
            project: project.name.clone(),
            sites: seen,
        });
    }

    Ok(match first {
        Some((pool, version)) => (Some(pool), version),
        None => (None, None),
    })
}

/// The languages this project asked for, resolved to the exact versions it is running on.
///
/// **D4a**: a kind whose answer came from [`RuntimeSource::Default`] was decided by this machine
/// rather than by the project, and writing it down would put this home's default into a file meant
/// for somebody else's. PHP is the exception and not by special-casing — it comes from the
/// `runtime_installs` row behind the pool the site names, which is a fact about the site.
async fn runtimes(
    store: &Store,
    project: &ProjectRecord,
    php: Option<&mixengine_proto::PackageVersion>,
) -> Result<BTreeMap<RuntimeKind, VersionConstraint>> {
    let mut captured = BTreeMap::new();

    for kind in RuntimeKind::ALL {
        let question = resolve::Question {
            kind,
            cwd: Some(&project.root),
            explicit: None,
        };

        match resolve::runtime(store, &question).await {
            Ok(resolved) if resolved.source != RuntimeSource::Default => {
                if let Ok(constraint) = VersionConstraint::parse(resolved.runtime.version.as_str())
                {
                    captured.insert(kind, constraint);
                }
            }
            Ok(_) => {}
            // A language nothing declares and nothing installs is not a language this project uses.
            // Every other failure is a database that cannot be read, and is worth raising.
            Err(Error::RuntimeUnresolved { .. } | Error::NoDefaultRuntime { .. }) => {}
            Err(other) => return Err(other),
        }
    }

    if let Some(version) = php
        && let Ok(constraint) = VersionConstraint::parse(version.as_str())
    {
        captured.insert(RuntimeKind::Php, constraint);
    }

    Ok(captured)
}

/// The services the site declares, as a blueprint states them.
async fn linked(
    store: &Store,
    project: &ProjectRecord,
    declared: &[ServiceId],
    pool: Option<&ServiceId>,
    manifest: Option<&manifest::Manifest>,
) -> Result<Vec<BlueprintService>> {
    let mut captured = Vec::new();

    for service in declared {
        if FRONT_ENDS.contains(&service.name()) || service.name() == POOL {
            continue;
        }
        if Some(service) == pool {
            continue;
        }

        let named = manifest.and_then(|manifest| {
            manifest
                .services
                .iter()
                .find(|entry| entry.name == service.name())
        });

        captured.push(BlueprintService {
            name: service.name().to_owned(),
            version: services::version(store, service)
                .await?
                .and_then(|version| VersionConstraint::parse(version.as_str()).ok()),
            instance: service
                .instance()
                .map(|instance| match instance == project.name {
                    // **The trap D4 exists for.** A dedicated instance copied by name would make the
                    // next project plug into this one's database server.
                    true => PER_PROJECT.to_owned(),
                    false => instance.to_owned(),
                }),
            database: named
                .and_then(|entry| entry.database.as_deref())
                .map(|database| tokenised_value(database, &project.name)),
            user: named
                .and_then(|entry| entry.user.as_deref())
                .map(|user| tokenised_value(user, &project.name)),
            // Never captured (T205a): a project's `.env` says nothing about which key a blueprint
            // should offer, and a manifest that named one would be a guess.
            dotenv: None,
        });
    }

    Ok(captured)
}

/// What somebody turned on for the PHP this project's pool runs.
///
/// **Only the deviations, and only the ones turned on** (D2). The set a build enables by itself is
/// the receiving machine's business, and turning something *off* there is not this project's
/// requirement.
async fn extensions(
    store: &Store,
    version: &mixengine_proto::PackageVersion,
) -> Result<Option<Php>> {
    let state = crate::runtimes::extensions::state(store, RuntimeKind::Php, version).await?;

    let mut enabled: Vec<String> = state
        .choices
        .iter()
        .filter(|(_, wanted)| **wanted)
        .map(|(name, _)| name.clone())
        .collect();

    enabled.sort();

    Ok((!enabled.is_empty()).then_some(Php {
        extensions: enabled,
    }))
}

/// `blog.test` for project `blog` becomes `{project}.test`; `shop-staging.test` stays as it is.
///
/// **Substitution, never invention** (D4). The comparison is per label rather than per substring,
/// so a project called `e` does not turn every `e` in a domain into a token — and a domain that
/// does not contain the project's name keeps its literal spelling, which the plan then reports as a
/// domain conflict rather than guessing at a pattern that would break on the second machine.
fn tokenised_domain(domain: &str, project: &str) -> String {
    domain
        .split('.')
        .map(|label| match label == project {
            true => TOKEN,
            false => label,
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// The whole value, or nothing: a database called `blog` for project `blog` is `{project}`.
fn tokenised_value(value: &str, project: &str) -> String {
    match value == project {
        true => TOKEN.to_owned(),
        false => value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use mixengine_proto::Timestamp;

    use crate::blueprints::manifest::render;
    use crate::projects::Registration;

    /// A home with one project, whose root exists so a manifest can be written into it.
    async fn home(name: &str) -> (tempfile::TempDir, Store, ProjectRecord) {
        let temp = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(&temp.path().join("mixengine.db"))
            .await
            .expect("a database");

        let root = temp.path().join(name);
        std::fs::create_dir_all(root.join("public")).expect("a project directory");

        let project = crate::projects::create(
            &store,
            &Registration {
                name: name.to_owned(),
                root,
                pins: BTreeMap::new(),
            },
            Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
        )
        .await
        .expect("a project");

        (temp, store, project)
    }

    /// An installed PHP, a pool on it, and the row that says the pool loads `xdebug`.
    async fn a_php_pool(store: &Store, version: &str, choices: &str) {
        sqlx::query(
            r#"INSERT INTO runtime_installs
                   (id, kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256, extension_choices_json, extensions_json)
               VALUES (1, 'php', ?1, 'stable', '/runtimes/php', '2026-09-01T00:00:00Z', 1,
                       'https://example.invalid/php', 'ab', ?2,
                       '{"shared":["redis","xdebug","mongodb"],"enabled":["redis"],"compiled_in":[]}')"#,
        )
        .bind(version)
        .bind(choices)
        .execute(store.pool())
        .await
        .expect("a runtime install");

        sqlx::query(
            "INSERT INTO services (id, runtime_install_id, instance_name, state, port)
             VALUES (?1, 1, ?2, 'stopped', 9000)",
        )
        .bind(format!("php-fpm@{version}"))
        .bind(version)
        .execute(store.pool())
        .await
        .expect("a pool row");
    }

    /// A package instance, so a site has something to link to.
    async fn a_package(store: &Store, id: i64, name: &str, version: &str, instance: &str) {
        sqlx::query(
            "INSERT INTO packages (id, name, version, install_path, installed_at, source_url, sha256)
             VALUES (?1, ?2, ?3, '/packages/x', '2026-09-01T00:00:00Z', 'https://example.invalid/p', 'ab')",
        )
        .bind(id)
        .bind(name)
        .bind(version)
        .execute(store.pool())
        .await
        .expect("a package");

        sqlx::query(
            "INSERT INTO services (id, package_id, instance_name, state, port, config_overrides_json)
             VALUES (?1, ?2, ?3, 'stopped', 3306, '{\"password\":\"hunter2\"}')",
        )
        .bind(format!("{name}@{instance}"))
        .bind(id)
        .bind(instance)
        .execute(store.pool())
        .await
        .expect("a service row");
    }

    fn asked<'a>(project: &'a ProjectRecord, name: &'a str) -> Asked<'a> {
        Asked {
            project,
            name,
            description: "",
            os: "linux",
            version: "0.1.0",
            created_at: "2026-09-01T09:00:00Z",
        }
    }

    async fn a_site(store: &Store, project: &ProjectRecord, domains: &[&str], links: &[&str]) {
        let pool = ServiceId::parse("php-fpm@8.2.23").expect("an id");

        crate::sites::create(
            store,
            &crate::sites::NewSite {
                owner: crate::sites::SiteOwner::Project(project.id),
                doc_root: "public".to_owned(),
                kind: SiteKind::PhpFpm {
                    pool: Some(pool.clone()),
                },
                https_enabled: true,
                https_redirect: false,
                domains: domains.iter().map(|domain| (*domain).to_owned()).collect(),
                services: links
                    .iter()
                    .map(|id| ServiceId::parse(*id).expect("an id"))
                    .collect(),
                routes: Vec::new(),
            },
        )
        .await
        .expect("a site");
    }

    /// The shape of a capture: what the project uses, with `{project}` where its own name was.
    #[tokio::test]
    async fn a_capture_says_what_the_project_uses_and_nothing_about_this_machine() {
        let (_temp, store, project) = home("blog").await;
        a_php_pool(&store, "8.2.23", r#"{"xdebug":true,"mongodb":false}"#).await;
        a_package(&store, 1, "mariadb", "11.4.3", "main").await;
        std::fs::write(
            crate::manifest::at(&project.root),
            "[[services]]\nname = \"mariadb\"\ndatabase = \"blog\"\nuser = \"blog\"\n",
        )
        .expect("a project manifest");
        a_site(
            &store,
            &project,
            &["blog.test", "api.blog.test"],
            &["mariadb@main"],
        )
        .await;

        let manifest = capture(&store, &asked(&project, "blog-stack"))
            .await
            .expect("a capture");

        let site = manifest.sites.first().expect("a site");
        assert_eq!(site.domain_pattern, "{project}.test");
        assert_eq!(site.aliases, vec!["api.{project}.test".to_owned()]);
        assert_eq!(site.doc_root, "public");
        assert_eq!(site.kind, SiteKind::PhpFpm { pool: None });

        assert_eq!(manifest.services.len(), 1);
        assert_eq!(manifest.services[0].name, "mariadb");
        assert_eq!(manifest.services[0].instance.as_deref(), Some("main"));
        assert_eq!(manifest.services[0].database.as_deref(), Some("{project}"));
        assert_eq!(manifest.services[0].user.as_deref(), Some("{project}"));

        assert_eq!(
            manifest
                .runtimes
                .get(&RuntimeKind::Php)
                .map(VersionConstraint::as_str),
            Some("8.2.23"),
            "php comes from the pool the site names"
        );

        assert_eq!(
            manifest.php.as_ref().map(|php| php.extensions.clone()),
            Some(vec!["xdebug".to_owned()]),
            "only the deviations, and only the ones turned on"
        );

        assert!(
            manifest.scaffold.is_none(),
            "capture never writes a scaffold"
        );
    }

    /// **D4, and the trap it exists for.** A dedicated instance copied by name would make the next
    /// project plug into this one's database server.
    #[tokio::test]
    async fn a_dedicated_instance_is_captured_as_per_project_rather_than_by_name() {
        let (_temp, store, project) = home("blog").await;
        a_php_pool(&store, "8.2.23", "{}").await;
        a_package(&store, 1, "redis", "7.2.5", "blog").await;
        a_site(&store, &project, &["blog.test"], &["redis@blog"]).await;

        let manifest = capture(&store, &asked(&project, "blog-stack"))
            .await
            .expect("a capture");

        assert_eq!(manifest.services[0].instance.as_deref(), Some(PER_PROJECT));
    }

    /// **T204a, D6.** Every site, each with its own links; `[[services]]` the union.
    #[tokio::test]
    async fn a_project_with_several_sites_captures_them_all() {
        let (_temp, store, project) = home("blog").await;
        a_php_pool(&store, "8.2.23", "{}").await;
        a_package(&store, 1, "mariadb", "11.4.3", "main").await;
        a_package(&store, 2, "redis", "7.2.5", "main").await;
        a_site(
            &store,
            &project,
            &["blog.test"],
            &["mariadb@main", "redis@main"],
        )
        .await;
        a_site(&store, &project, &["api.blog.test"], &["redis@main"]).await;
        a_site(&store, &project, &["docs.blog.test"], &[]).await;

        let manifest = capture(&store, &asked(&project, "blog-stack"))
            .await
            .expect("a capture");

        // In primary-domain order, `sites::records`' own, which makes two captures one file.
        let patterns: Vec<_> = manifest
            .sites
            .iter()
            .map(|site| site.domain_pattern.as_str())
            .collect();
        assert_eq!(
            patterns,
            [
                "api.{project}.test",
                "{project}.test",
                "docs.{project}.test"
            ]
        );
        assert_eq!(manifest.sites[0].services, Some(vec!["redis".to_owned()]));
        assert_eq!(
            manifest.sites[1].services,
            Some(vec!["mariadb".to_owned(), "redis".to_owned()])
        );
        assert_eq!(manifest.sites[2].services, Some(Vec::new()));

        let names: Vec<_> = manifest
            .services
            .iter()
            .map(|service| service.name.as_str())
            .collect();
        assert_eq!(names, ["redis", "mariadb"], "in order of first appearance");

        let rendered = render(&manifest);
        assert!(rendered.starts_with("schema = 3"), "{rendered}");
        let read_back = crate::blueprints::manifest::read(&rendered).expect("it reads back");
        assert_eq!(read_back.sites, manifest.sites);
        assert_eq!(read_back.services, manifest.services);
    }

    /// **T204a, D3 and D6.** Two links to one package name the instance, as the file writes it.
    #[tokio::test]
    async fn two_instances_of_one_package_are_linked_by_instance() {
        let (_temp, store, project) = home("blog").await;
        a_php_pool(&store, "8.2.23", "{}").await;
        a_package(&store, 1, "mariadb", "11.4.3", "main").await;
        sqlx::query(
            "INSERT INTO services (id, package_id, instance_name, state, port)
             VALUES ('mariadb@blog', 1, 'blog', 'stopped', 3307)",
        )
        .execute(store.pool())
        .await
        .expect("a dedicated instance");
        a_site(&store, &project, &["blog.test"], &["mariadb@main"]).await;
        a_site(&store, &project, &["api.blog.test"], &["mariadb@blog"]).await;

        let manifest = capture(&store, &asked(&project, "blog-stack"))
            .await
            .expect("a capture");

        let links = |pattern: &str| {
            manifest
                .sites
                .iter()
                .find(|site| site.domain_pattern == pattern)
                .and_then(|site| site.services.clone())
        };
        assert_eq!(
            links("{project}.test"),
            Some(vec!["mariadb@main".to_owned()])
        );
        assert_eq!(
            links("api.{project}.test"),
            Some(vec![format!("mariadb@{PER_PROJECT}")])
        );
        crate::blueprints::manifest::read(&render(&manifest)).expect("a file this build reads");
    }

    /// **T204a, D6.** A blueprint has one PHP, so two are refused and both are named.
    #[tokio::test]
    async fn sites_on_two_phps_are_refused_naming_both() {
        let (_temp, store, project) = home("blog").await;
        a_php_pool(&store, "8.2.23", "{}").await;
        sqlx::query(
            r#"INSERT INTO runtime_installs
                   (id, kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256)
               VALUES (2, 'php', '8.3.12', 'stable', '/runtimes/php83', '2026-09-01T00:00:00Z', 1,
                       'https://example.invalid/php', 'ab')"#,
        )
        .execute(store.pool())
        .await
        .expect("a second runtime");
        sqlx::query(
            "INSERT INTO services (id, runtime_install_id, instance_name, state, port)
             VALUES ('php-fpm@8.3.12', 2, '8.3.12', 'stopped', 9001)",
        )
        .execute(store.pool())
        .await
        .expect("a second pool");

        a_site(&store, &project, &["blog.test"], &[]).await;
        crate::sites::create(
            &store,
            &crate::sites::NewSite {
                owner: crate::sites::SiteOwner::Project(project.id),
                doc_root: "public".to_owned(),
                kind: SiteKind::PhpFpm {
                    pool: Some(ServiceId::parse("php-fpm@8.3.12").expect("an id")),
                },
                https_enabled: true,
                https_redirect: false,
                domains: vec!["api.blog.test".to_owned()],
                services: Vec::new(),
                routes: Vec::new(),
            },
        )
        .await
        .expect("a site on the other PHP");

        let error = capture(&store, &asked(&project, "blog-stack"))
            .await
            .expect_err("refused");

        assert!(matches!(error, Error::ProjectRunsSeveralPhps { .. }));
        let message = error.to_string();
        for part in ["blog.test", "8.2.23", "api.blog.test", "8.3.12"] {
            assert!(message.contains(part), "{part}: {message}");
        }
    }

    /// **T204a, D6.** Steps from a one-site blueprint go to the site that blueprint described, so
    /// a project that later gained a site still captures into a file this build reads.
    #[tokio::test]
    async fn steps_from_a_one_site_blueprint_are_given_its_site() {
        let (_temp, store, project) = home("blog").await;
        a_php_pool(&store, "8.2.23", "{}").await;
        a_site(&store, &project, &["admin.blog.test"], &[]).await;
        a_site(&store, &project, &["blog.test"], &[]).await;

        let origin = "schema = 1\n\n[blueprint]\nname = \"one\"\ncreated_at = \"x\"\n\n\
            [blueprint.created_on]\nos = \"any\"\nversion = \"0\"\n\n\
            [site]\nkind = \"php-fpm\"\ndomain_pattern = \"{project}.test\"\n\n\
            [[next_steps]]\nkind = \"once\"\nrun = \"php artisan migrate\"\n";
        sqlx::query(
            "INSERT INTO blueprints (id, name, manifest_toml, created_at, source, trusted)
             VALUES ('one', 'one', ?, 'x', 'captured', 1)",
        )
        .bind(origin)
        .execute(store.pool())
        .await
        .expect("a blueprint row");
        sqlx::query("UPDATE projects SET blueprint_id = 'one' WHERE id = ?")
            .bind(project.id)
            .execute(store.pool())
            .await
            .expect("the project points at it");

        let manifest = capture(&store, &asked(&project, "again"))
            .await
            .expect("a capture");

        assert_eq!(
            manifest.next_steps[0].site.as_deref(),
            Some("{project}.test"),
            "the origin's site, not the first one listed"
        );
        crate::blueprints::manifest::read(&render(&manifest)).expect("a file this build reads");
    }

    /// **D4a.** A version this machine's default decided is this machine's, not the project's.
    #[tokio::test]
    async fn a_runtime_that_only_the_global_default_named_is_not_captured() {
        let (_temp, store, project) = home("blog").await;

        sqlx::query(
            r#"INSERT INTO runtime_installs
                   (id, kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256, is_default)
               VALUES (1, 'node', '22.8.0', 'stable', '/runtimes/node', '2026-09-01T00:00:00Z', 1,
                       'https://example.invalid/node', 'ab', 1)"#,
        )
        .execute(store.pool())
        .await
        .expect("a default node");

        let manifest = capture(&store, &asked(&project, "blog-stack"))
            .await
            .expect("a capture");

        assert!(
            manifest.runtimes.is_empty(),
            "{:?} came from this machine, not from the project",
            manifest.runtimes
        );
    }

    /// A domain that does not contain the project's name keeps its literal spelling: guessing a
    /// pattern is what would break on the second machine.
    #[tokio::test]
    async fn a_domain_that_does_not_carry_the_project_name_is_left_alone() {
        let (_temp, store, project) = home("blog").await;
        a_php_pool(&store, "8.2.23", "{}").await;
        a_site(&store, &project, &["shop-staging.test"], &[]).await;

        let manifest = capture(&store, &asked(&project, "blog-stack"))
            .await
            .expect("a capture");

        assert_eq!(manifest.sites[0].domain_pattern, "shop-staging.test");
    }

    /// **D6, and the test the whole task is measured by.** Written against the rendered string,
    /// because asserting on the struct would only prove that the fields we remembered are empty.
    #[tokio::test]
    async fn nothing_forbidden_reaches_the_rendered_manifest() {
        let (temp, store, project) = home("blog").await;
        a_php_pool(&store, "8.2.23", r#"{"xdebug":true}"#).await;
        a_package(&store, 1, "mariadb", "11.4.3", "main").await;
        a_site(&store, &project, &["blog.test"], &["mariadb@main"]).await;
        // **Two sites** — roadmap task **T204a**: `[[sites]]` and its `services` are a second
        // shape of the file, and they are held to the same rule.
        a_site(&store, &project, &["api.blog.test"], &["mariadb@main"]).await;

        let manifest = capture(&store, &asked(&project, "blog-stack"))
            .await
            .expect("a capture");
        let rendered = render(&manifest);
        assert!(rendered.contains("[[sites]]"), "{rendered}");

        let home_directory = temp.path().display().to_string();
        assert!(
            !rendered.contains(&home_directory),
            "the home directory is in it:\n{rendered}"
        );

        for forbidden in [
            "hunter2",
            "password",
            "secret",
            "token",
            "3306",
            "9000",
            "MIXENGINE_HOME",
        ] {
            assert!(
                !rendered.contains(forbidden),
                "{forbidden} reached the manifest:\n{rendered}"
            );
        }

        // An absolute path in either of this machine's spellings.
        assert!(!rendered.contains(":\\"), "{rendered}");
        assert!(
            !rendered.lines().any(|line| line.contains(" = \"/")),
            "{rendered}"
        );
    }
}
