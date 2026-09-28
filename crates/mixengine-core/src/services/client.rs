//! Which install a service's client commands run out of — roadmap task **T130**.
//!
//! A runtime resolves *per directory*: [`crate::resolve::runtime`] walks up from the working
//! directory to a project row and falls back to a default, which is what makes `php -v` differ
//! between two folders. A database cannot answer that way and never could. It has **instances** —
//! `mariadb@main` beside `mariadb@legacy`, each with its own package version, its own data
//! directory and its own port — and a working directory says nothing about which of them somebody
//! means.
//!
//! So a client belongs to an instance, and the instance decides two things at once: which binary
//! runs, and **where it connects by default**. The second is not a nicety.
//! `docs/features/services.md` gives 3306 to whichever of MariaDB and MySQL was created first
//! and the next free port above to the other, so on a home with both, a bare `mysql` that was told
//! nothing would open a session on the other product's server and report success.
//!
//! # The order, and why it ends where it does
//!
//! 1. `MIXENGINE_MARIADB=mariadb@legacy` — named outright, and refused when it names no row. A
//!    variable that quietly does nothing is the confusion this whole mechanism exists to end, which
//!    is [`crate::runtimes`]' argument for `MIXENGINE_PHP` one table across.
//! 2. One instance, and there is nothing to decide.
//! 3. More than one, and the instance holding the product's own port wins — which is what a person
//!    means by "the MariaDB". Then the lowest port, then the id ascending: arbitrary, but the same
//!    answer on every machine and on every run, because a client that ran against a different
//!    server depending on row order would be worse than one that refused.
//! 4. **No instance at all is not a failure.** `mysqldump -h db.example.com` is a real use of a
//!    client, so the newest installed version of the package supplies the binary and nothing is
//!    said about an endpoint.
//!
//! # Read without a daemon
//!
//! Every query here goes through a read-only [`Store`], because the caller is usually a shim
//! standing in somebody's terminal with the daemon stopped. That is the same promise
//! `crates/mixengine-shim/src/main.rs` already documents for runtimes, and the rows are on disk
//! whether or not anything is running.

use std::net::SocketAddr;
use std::path::PathBuf;

use mixengine_proto::{PackageVersion, ServiceId, VersionError};

use crate::generate::recipe::Upstream;
use crate::{Error, Result, Store};

/// The install a package's client commands run out of, and where its own server listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    /// The instance the client belongs to, and [`None`] on a home with no instance of this package.
    pub service: Option<ServiceId>,

    /// Which version of the package supplies the binary.
    pub version: PackageVersion,

    /// That version's directory, which is what a `provides` path is joined onto.
    pub install_path: PathBuf,

    /// Where [`service`](Self::service) listens, and [`None`] when there is no instance or its row
    /// holds no port.
    pub listen: Option<Upstream>,

    /// What that install publishes, by the key the index names it under.
    ///
    /// Carried rather than looked up afterwards because **both callers need it and the row is
    /// already open**: the daemon asks it to find out which client names `bin/` may front, and the
    /// shim asks it to turn the name somebody typed into a file. Two queries would be two chances
    /// for `bin/` to hold a name the run then cannot resolve.
    pub provides: std::collections::BTreeMap<String, String>,
}

impl Chosen {
    /// The file behind one of this install's published names.
    ///
    /// [`Context::provided`](crate::generate::recipe::Context::provided) for a client rather than
    /// for a recipe, and the same join: the path inside the archive belongs to whoever packed it,
    /// so it is read out of the recorded map rather than guessed at from the name.
    ///
    /// # Errors
    ///
    /// [`Error::PackageProvidesNothing`], naming the command and listing what this version does
    /// publish.
    pub fn program(&self, package: &str, executable: &str) -> Result<PathBuf> {
        self.provides
            .get(executable)
            .map(|relative| crate::paths::join_stored(&self.install_path, relative))
            .ok_or_else(|| Error::PackageProvidesNothing {
                package: package.to_owned(),
                version: self.version.clone(),
                executable: executable.to_owned(),
                known: self.provides.keys().cloned().collect(),
            })
    }
}

/// The variable that names an instance for one package: `MIXENGINE_MARIADB`.
///
/// Upper case with every character a package name may hold that an environment variable may not
/// folded to `_`, so `php-fpm` would be `MIXENGINE_PHP_FPM`. The same shape as
/// [`RuntimeKind::override_env`](mixengine_proto::RuntimeKind::override_env), because a person who
/// has learned one has learned the other.
#[must_use]
pub fn override_env(package: &str) -> String {
    let folded: String = package
        .chars()
        .map(|character| match character.is_ascii_alphanumeric() {
            true => character.to_ascii_uppercase(),
            false => '_',
        })
        .collect();

    format!("MIXENGINE_{folded}")
}

/// Every client command this home could front, as claims on a name — roadmap task **T130**.
///
/// **One implementation, two callers, and that is the point.** The daemon asks with `only` as
/// [`None`] to compose `bin/`; a shim asks with the name somebody just typed, to find out whose
/// program it is. Two walks would be a directory holding a command the shim then refused, or a
/// command the shim ran that `bin/` had given to the other package.
///
/// `only` is an optimisation and never a difference in the answer: a name is contested by at most
/// the packages whose recipes declare it, so a shim narrows the walk to those before it opens the
/// database and pays one query instead of eight. What comes out is handed to
/// [`shims::resolve_claims`](crate::shims::resolve_claims) either way.
///
/// A package with no installed version is skipped. `bin/` holding `node` on a machine with no
/// Node.js is [`crate::shims::COMMANDS`]' deliberate choice and is right for a *runtime*, whose
/// shim then says which command to type; there is no such sentence for `mysqldump` on a machine
/// that has never had a database.
///
/// # Errors
///
/// [`Error::Database`] when the rows cannot be read, and [`Error::UnreadablePackageRow`] for a row
/// this build cannot parse. A package that is not installed is not an error.
pub async fn claims(
    store: &Store,
    catalogue: &crate::generate::Catalogue,
    only: Option<&str>,
) -> Result<Vec<crate::shims::Claimed>> {
    let mut claims = Vec::new();

    for package in catalogue.packages() {
        let recipe = catalogue.recipe(package).expect("a listed recipe");

        let wanted: Vec<&crate::generate::recipe::ClientCommand> = recipe
            .clients()
            .iter()
            .filter(|client| only.is_none_or(|name| same_command(client.name, name)))
            .collect();

        if wanted.is_empty() {
            continue;
        }

        let preferred_port = recipe.preferred_port();

        let chosen = match chosen(store, package, preferred_port, None).await {
            Ok(chosen) => chosen,
            Err(Error::NotFound { .. }) => continue,
            Err(error) => return Err(error),
        };

        let on_preferred_port = matches!(
            (&chosen.listen, preferred_port),
            (Some(Upstream::Tcp(at)), Some(port)) if at.port() == port
        );

        for client in wanted {
            // **The chosen install and not any install.** A Windows MariaDB packs no
            // `mariadb-backup` on every branch, and a name in `bin/` that resolves to nothing is
            // worse than a missing one.
            if !chosen.provides.contains_key(client.executable) {
                continue;
            }

            claims.push(crate::shims::Claimed {
                name: client.name.to_owned(),
                package: package.to_owned(),
                claim: client.claim,
                has_instance: chosen.service.is_some(),
                on_preferred_port,
            });
        }
    }

    Ok(claims)
}

/// Are these the same command name?
///
/// [`crate::shims::dispatch`]' rule, which is the filesystem's rather than a courtesy: `MYSQL` and
/// `mysql` are one file on Windows and two here, so folding case on Unix would let a program
/// genuinely called `MYSQL` be dispatched as the other.
fn same_command(left: &str, right: &str) -> bool {
    match cfg!(windows) {
        true => left.eq_ignore_ascii_case(right),
        false => left == right,
    }
}

/// Which executable a package's recipe runs for one command name.
///
/// The compiled half of the answer, asked after [`claims`] has settled *which package* the name
/// belongs to: a shim has a winner and still needs to know that MariaDB's `mysqldump` is
/// `mariadb-dump`.
#[must_use]
pub fn executable_for(
    catalogue: &crate::generate::Catalogue,
    package: &str,
    name: &str,
) -> Option<&'static str> {
    catalogue
        .recipe(package)?
        .clients()
        .iter()
        .find_map(|client| same_command(client.name, name).then_some(client.executable))
}

/// One `services` row, as far as this decision cares about it.
#[derive(Debug, Clone)]
struct Instance {
    id: String,
    port: Option<u16>,
    bind_addr: String,
    version: String,
    install_path: String,
    provides_json: String,
}

/// Which install the clients of `package` should run out of, on this home.
///
/// `preferred_port` is the recipe's own wish — [`Recipe::preferred_port`](crate::generate::Recipe::preferred_port) —
/// and is the third rule above. `asked` is the first, already read off the environment by whoever
/// owns that environment: a shim reads its own variables, and the daemon has none to read.
///
/// # Errors
///
/// [`Error::NotFound`] when `asked` names no instance of this package, and when nothing of this
/// package is installed at all. [`Error::Database`] when the rows cannot be read, and
/// [`Error::UnreadablePackageRow`] when a recorded version or service id is not one this build can
/// parse.
pub async fn chosen(
    store: &Store,
    package: &str,
    preferred_port: Option<u16>,
    asked: Option<&ServiceId>,
) -> Result<Chosen> {
    let instances = instances(store, package).await?;

    // Step one, and it is checked against the instances of *this* package rather than against every
    // service: `MIXENGINE_MARIADB=redis@main` is a mistake worth naming, not a Redis to run
    // `mariadb-dump` out of.
    if let Some(asked) = asked {
        let found = instances
            .iter()
            .find(|instance| instance.id == asked.as_str())
            .ok_or_else(|| Error::NotFound {
                kind: "service",
                id: asked.as_str().to_owned(),
            })?;

        let install = Install {
            version: found.version.clone(),
            path: found.install_path.clone(),
            provides_json: found.provides_json.clone(),
        };

        return resolved(Some(found.clone()), &install);
    }

    match pick(instances, preferred_port) {
        Some(instance) => {
            let install = Install {
                version: instance.version.clone(),
                path: instance.install_path.clone(),
                provides_json: instance.provides_json.clone(),
            };

            resolved(Some(instance), &install)
        }

        // Rule four. The newest installed version, and nothing said about an endpoint.
        None => resolved(None, &newest(store, package).await?),
    }
}

/// The three things a `packages` row contributes to an answer.
///
/// A value rather than three arguments so that the two paths into [`resolved`] cannot pass them in
/// a different order — they are all strings, which is exactly the shape a compiler cannot check.
struct Install {
    version: String,
    path: String,
    provides_json: String,
}

/// Rule three, as a total order.
///
/// Written as a sort key rather than as SQL so that the tie-break is in one readable place and can
/// be tested without a database: `ORDER BY CASE WHEN port = ?` spread across a query is the kind of
/// expression that silently changes meaning when a column becomes nullable.
fn pick(mut instances: Vec<Instance>, preferred_port: Option<u16>) -> Option<Instance> {
    instances.sort_by(|left, right| {
        let key = |instance: &Instance| {
            (
                // The product's own port first — `false` sorts before `true`, so this is negated.
                !(preferred_port.is_some() && instance.port == preferred_port),
                // Then a row that has a port at all, then the lowest one. A port-less instance is
                // last because it is the one a client can be told nothing about.
                instance.port.is_none(),
                instance.port.unwrap_or(u16::MAX),
                instance.id.clone(),
            )
        };

        key(left).cmp(&key(right))
    });

    instances.into_iter().next()
}

/// Assemble the answer, parsing the values a row holds as text.
fn resolved(instance: Option<Instance>, install: &Install) -> Result<Chosen> {
    let listen = instance.as_ref().and_then(|instance| {
        let port = instance.port?;

        // A bind address that will not parse is a row nothing in this crate wrote, and the honest
        // answer is no endpoint rather than a guess: a client told `0.0.0.0` would dial a number
        // that is not an address on most systems.
        let address: SocketAddr = format!("{}:{port}", instance.bind_addr).parse().ok()?;

        Some(Upstream::Tcp(address))
    });

    let unreadable = |column: &'static str, value: &str| Error::UnreadablePackageRow {
        column,
        value: value.to_owned(),
    };

    Ok(Chosen {
        service: instance
            .map(|instance| {
                ServiceId::parse(instance.id.clone())
                    .map_err(|_| unreadable("services.id", &instance.id))
            })
            .transpose()?,
        version: PackageVersion::parse(install.version.clone())
            .map_err(|VersionError { value, .. }| unreadable("version", &value))?,
        install_path: PathBuf::from(&install.path),
        listen,
        // An unreadable map is an empty one rather than a refusal, which is `packages::remember`'s
        // own answer beside the same column: a row with no recorded executables is a thing that
        // happens, and it is `program` that turns it into a sentence naming the command.
        provides: serde_json::from_str(&install.provides_json).unwrap_or_default(),
    })
}

/// Every instance of `package`, with the install its row points at.
async fn instances(store: &Store, package: &str) -> Result<Vec<Instance>> {
    sqlx::query_as!(
        Instance,
        r#"SELECT services.id            AS "id!",
                  services.port          AS "port: u16",
                  services.bind_addr     AS "bind_addr!",
                  packages.version       AS "version!",
                  packages.install_path  AS "install_path!",
                  packages.provides_json AS "provides_json!"
           FROM services
           JOIN packages ON packages.id = services.package_id
           WHERE packages.name = ?"#,
        package
    )
    .fetch_all(store.pool())
    .await
    .map_err(|source| store.failure("read", source))
}

/// The newest installed version of `package`, for a home with no instance of it.
///
/// [`PackageVersion::cmp_precedence`] and not `Ord`, which is the string's order: `8.10.0` is newer
/// than `8.9.0` and sorts before it as text, so the obvious `ORDER BY version DESC` would hand a
/// person the older client every time a minor number reached ten.
async fn newest(store: &Store, package: &str) -> Result<Install> {
    let rows = sqlx::query!(
        "SELECT version, install_path, provides_json FROM packages WHERE name = ?",
        package
    )
    .fetch_all(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    let mut best: Option<(PackageVersion, Install)> = None;

    for row in rows {
        let version = PackageVersion::parse(row.version.clone()).map_err(
            |VersionError { value, .. }| Error::UnreadablePackageRow {
                column: "version",
                value,
            },
        )?;

        let newer = best
            .as_ref()
            .is_none_or(|(held, _)| version.cmp_precedence(held).is_gt());

        if newer {
            best = Some((
                version,
                Install {
                    version: row.version,
                    path: row.install_path,
                    provides_json: row.provides_json,
                },
            ));
        }
    }

    best.map(|(_, install)| install)
        .ok_or_else(|| Error::NotFound {
            kind: "package",
            id: package.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::Paths;

    /// T191: the program behind a published name is spelled as this system spells a path, so a
    /// service that will not start names a file a person can paste.
    #[test]
    fn a_program_is_spelled_the_way_this_system_spells_a_path() {
        let chosen = Chosen {
            service: None,
            version: PackageVersion::parse("1.0.0").expect("a version"),
            install_path: std::env::temp_dir()
                .join("packages")
                .join("redis")
                .join("1.0.0"),
            listen: None,
            provides: [("redis-cli".to_owned(), "bin/redis-cli".to_owned())].into(),
        };

        let program = chosen.program("redis", "redis-cli").expect("published");

        assert!(program.ends_with(std::path::Path::new("bin").join("redis-cli")));
        if cfg!(windows) {
            assert!(
                !program.display().to_string().contains('/'),
                "{}",
                program.display()
            );
        }
    }

    async fn store() -> (tempfile::TempDir, Store) {
        let home = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(&home.path().join(crate::paths::DATABASE_FILE_NAME))
            .await
            .expect("a database");
        (home, store)
    }

    /// One `packages` row, at the layout `packages::directory` describes.
    async fn install(store: &Store, package: &str, version: &str) {
        let paths = Paths::new(
            PathBuf::from("/home"),
            &crate::config::PathOverrides::default(),
        );
        let path = crate::packages::directory(
            &paths,
            package,
            &PackageVersion::parse(version).expect("a version"),
        );

        sqlx::query(
            "INSERT INTO packages
                 (name, version, install_path, installed_at, source_url, sha256, size_bytes,
                  provides_json)
             VALUES (?, ?, ?, '2026-09-15T00:00:00Z', 'https://example.invalid/x', '00', 1, '{}')",
        )
        .bind(package)
        .bind(version)
        .bind(path.display().to_string())
        .execute(store.pool())
        .await
        .expect("a packages row");
    }

    /// One `services` row against an already-installed version.
    async fn instantiate(
        store: &Store,
        service: &str,
        package: &str,
        version: &str,
        port: Option<u16>,
    ) {
        sqlx::query(
            "INSERT INTO services (id, package_id, instance_name, state, port)
             VALUES (?, (SELECT id FROM packages WHERE name = ? AND version = ?), ?, 'stopped', ?)",
        )
        .bind(service)
        .bind(package)
        .bind(version)
        .bind(service)
        .bind(port.map(i64::from))
        .execute(store.pool())
        .await
        .expect("a services row");
    }

    fn port_of(chosen: &Chosen) -> Option<u16> {
        match chosen.listen.as_ref()? {
            Upstream::Tcp(address) => Some(address.port()),
            Upstream::Socket(_) => None,
        }
    }

    /// `MIXENGINE_MARIADB`, and the same shape a runtime's own variable has.
    #[test]
    fn the_variable_is_named_after_the_package() {
        assert_eq!(override_env("mariadb"), "MIXENGINE_MARIADB");
        assert_eq!(override_env("php-fpm"), "MIXENGINE_PHP_FPM");
    }

    /// One instance is the answer with nothing to decide, and it carries the port every client of
    /// that instance has to be told about.
    #[tokio::test]
    async fn a_single_instance_is_the_answer() {
        let (_home, store) = store().await;
        install(&store, "mariadb", "12.3.2").await;
        instantiate(&store, "mariadb@main", "mariadb", "12.3.2", Some(3306)).await;

        let chosen = chosen(&store, "mariadb", Some(3306), None)
            .await
            .expect("an instance");

        assert_eq!(
            chosen.service.as_ref().map(ServiceId::as_str),
            Some("mariadb@main")
        );
        assert_eq!(chosen.version.as_str(), "12.3.2");
        assert_eq!(port_of(&chosen), Some(3306));
    }

    /// **The instance on the product's own port is what a person means by "the MariaDB"** — not the
    /// one the allocator pushed to 3307 because something already held 3306.
    #[tokio::test]
    async fn the_instance_on_the_products_own_port_wins() {
        let (_home, store) = store().await;
        install(&store, "mariadb", "12.3.2").await;
        // Deliberately inserted with the 3307 row first and named so that it sorts first too: if
        // either row order or the id decided this, the assertion below would pass by accident.
        instantiate(&store, "mariadb@a-legacy", "mariadb", "12.3.2", Some(3307)).await;
        instantiate(&store, "mariadb@main", "mariadb", "12.3.2", Some(3306)).await;

        let chosen = chosen(&store, "mariadb", Some(3306), None)
            .await
            .expect("an instance");

        assert_eq!(
            chosen.service.as_ref().map(ServiceId::as_str),
            Some("mariadb@main")
        );
        assert_eq!(port_of(&chosen), Some(3306));
    }

    /// Ports equal or absent and the answer is still the same one on every run: the lowest port,
    /// then the id ascending.
    #[tokio::test]
    async fn a_tie_is_broken_the_same_way_every_time() {
        let (_home, store) = store().await;
        install(&store, "redis", "8.10.0").await;
        instantiate(&store, "redis@sessions", "redis", "8.10.0", Some(6381)).await;
        instantiate(&store, "redis@cache", "redis", "8.10.0", Some(6380)).await;
        instantiate(&store, "redis@spare", "redis", "8.10.0", None).await;

        let chosen = chosen(&store, "redis", Some(6379), None)
            .await
            .expect("an instance");

        assert_eq!(
            chosen.service.as_ref().map(ServiceId::as_str),
            Some("redis@cache")
        );
        assert_eq!(
            port_of(&chosen),
            Some(6380),
            "the lowest port, not the port-less row"
        );
    }

    /// Step one is read before anything is counted.
    #[tokio::test]
    async fn an_asked_for_instance_wins_over_every_rule() {
        let (_home, store) = store().await;
        install(&store, "mariadb", "12.3.2").await;
        instantiate(&store, "mariadb@main", "mariadb", "12.3.2", Some(3306)).await;
        instantiate(&store, "mariadb@legacy", "mariadb", "12.3.2", Some(3307)).await;

        let asked = ServiceId::parse("mariadb@legacy").expect("a service id");
        let chosen = chosen(&store, "mariadb", Some(3306), Some(&asked))
            .await
            .expect("an instance");

        assert_eq!(chosen.service.as_ref(), Some(&asked));
        assert_eq!(port_of(&chosen), Some(3307));
    }

    /// An id naming no instance of this package is refused rather than silently ignored — including
    /// one that names a real service of some *other* package.
    #[tokio::test]
    async fn an_instance_that_is_not_there_is_refused() {
        let (_home, store) = store().await;
        install(&store, "mariadb", "12.3.2").await;
        install(&store, "redis", "8.10.0").await;
        instantiate(&store, "mariadb@main", "mariadb", "12.3.2", Some(3306)).await;
        instantiate(&store, "redis@main", "redis", "8.10.0", Some(6379)).await;

        let asked = ServiceId::parse("redis@main").expect("a service id");
        let error = chosen(&store, "mariadb", Some(3306), Some(&asked))
            .await
            .expect_err("a refusal");

        assert!(format!("{error}").contains("redis@main"), "{error}");
    }

    /// **`mysqldump -h db.example.com` is a real use of a client**, so a home with no instance still
    /// runs one — out of the newest version installed, and told nothing about an endpoint.
    ///
    /// The versions are chosen so that the string order and the version order disagree: `8.10.0` is
    /// newer than `8.9.0` and sorts before it as text.
    #[tokio::test]
    async fn no_instance_still_runs_the_newest_client() {
        let (_home, store) = store().await;
        install(&store, "redis", "8.9.0").await;
        install(&store, "redis", "8.10.0").await;

        let chosen = chosen(&store, "redis", Some(6379), None)
            .await
            .expect("a client");

        assert_eq!(chosen.service, None);
        assert_eq!(chosen.version.as_str(), "8.10.0");
        assert_eq!(chosen.listen, None);
        assert!(
            chosen.install_path.ends_with("8.10.0"),
            "{:?}",
            chosen.install_path
        );
    }

    /// A package with no row at all is the one case where there is nothing to run.
    #[tokio::test]
    async fn a_package_that_is_not_installed_is_refused() {
        let (_home, store) = store().await;

        let error = chosen(&store, "mariadb", Some(3306), None)
            .await
            .expect_err("a refusal");

        assert!(format!("{error}").contains("mariadb"), "{error}");
    }
}
