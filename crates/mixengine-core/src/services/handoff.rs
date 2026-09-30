//! Handing one database service to a desktop client — roadmap task **T83**.
//!
//! Two pure things and one read. [`address`] joins a `services` row to its recipe the way
//! [`crate::extensions::database::endpoint`] does, and disagrees with it on purpose about Redis:
//! phpMyAdmin cannot administer a cache, and MixLab can open one. [`url`] spells the connection the
//! way the client reads it, and [`encode`] is the ten lines that keep a crate out of the tree for
//! one function.
//!
//! **No password anywhere in this module.** The URL names the variable the password travels in
//! (the design's D2) and the daemon fills that variable; nothing here sees the value.

use std::fmt::Write as _;
use std::net::IpAddr;

use mixengine_proto::{DatabaseProtocol, ServiceId};

use crate::generate::Catalogue;
use crate::home::HomeId;
use crate::{Error, Result, Store};

/// The environment variable a credential is handed over in.
///
/// T82a's name, re-exported rather than restated: one constant, two consumers.
pub use crate::extensions::pools::CREDENTIAL_ENV;

/// Where one account's password lives inside the keyring's
/// [`mixengine`](mixengine_proto::KEYRING_SERVICE) namespace.
///
/// `<home-id>/<service-id>/<user>` — `9f3c1a77b204/mariadb@main/root`. The service id rather than
/// the package name, because two instances of one server are two databases with two different
/// passwords; and the home in front of both, because the credential store is one per **user** and
/// a user may have several homes.
///
/// **The home is roadmap task T126, and it was found by an outage.** The address used to be
/// `<service-id>/<user>` and nothing else, so every `MIXENGINE_HOME` on a machine shared one entry:
/// a sandbox home bootstrapping its own `mariadb@main` overwrote the root password of a home that
/// had been running for a day, and that home's server — which keeps its own copy in its data
/// directory — answered `ERROR 1045` to everything from that second on, including its own shutdown
/// command. [`crate::home`] is what the prefix comes from, and
/// [ADR 0032](https://github.com/mixnz/mixlab/blob/master/docs/decisions/0032-a-keyring-address-names-the-home-it-belongs-to.md)
/// is why it is an id the home carries rather than a hash of where it sits.
///
/// **One composition, and roadmap task T84 is why it is here rather than in three places.** Until
/// this task the string was spelled by
/// [`Context::secret_address`](crate::generate::recipe::Context::secret_address) for the recipes,
/// again by `database.open` for the handoff, and read back by the daemon's credential reader. The
/// convention is published to another application — MixLab reads these entries — but it reads the
/// [`SecretAddress`](mixengine_proto::SecretAddress) the daemon *hands it* rather than composing
/// one, which is what let T126 change the shape at all.
#[must_use]
pub fn secret_key(home: &HomeId, service: &ServiceId, user: &str) -> String {
    format!("{home}/{}/{user}", service.as_str())
}

/// The address this one had before it named a home — roadmap task **T126**.
///
/// `<home-id>/<rest>` with the home taken off, or [`None`] for a string that has no home in it and
/// is therefore already the old shape. Derived rather than passed in, so nothing has to carry two
/// addresses around to read one credential.
///
/// **What reads it is the migration path and nothing else**: a daemon finding no entry at the new
/// address looks at the old one, and moves what it finds. See `mixengined`'s `secrets` module.
///
/// **The first segment has to *be* a home id**, not merely be there — `mariadb@main/root` is an
/// address from before this task and has nothing older behind it. Without that check every miss on
/// an old address would cost a second lookup at a key nothing ever wrote (`root`), which is a
/// keyring round trip and, on Linux, a D-Bus one.
#[must_use]
pub fn secret_key_before_homes(key: &str) -> Option<&str> {
    let (home, rest) = key.split_once('/')?;

    HomeId::parse(home).is_some().then_some(rest)
}

/// Where one database service listens, and what it speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    /// What a client speaks to it.
    pub protocol: DatabaseProtocol,

    /// The address the row binds.
    pub host: IpAddr,

    /// The port the row declares.
    pub port: u16,

    /// The recipe's administrator — `root`, `postgres` — or [`None`] for a server with no accounts.
    pub administrator: Option<String>,

    /// Whether the recipe knows how to make a database on it — roadmap task **T155**. `false` for
    /// Redis and MongoDB.
    pub creates_databases: bool,
}

/// What a URL is rendered from.
#[derive(Debug, Clone, Copy)]
pub struct Connection<'a> {
    /// The client's scheme: the window's, `mixlab` (`crate::window::SCHEME`).
    pub scheme: &'a str,

    /// The label the client names the tab with: the service id.
    pub label: &'a str,

    /// Where to connect.
    pub address: &'a Address,

    /// The account to sign in as, where the server has accounts.
    pub user: Option<&'a str>,

    /// A database to land in.
    pub database: Option<&'a str>,

    /// The key half of the keyring address the password sits at, where there is one.
    ///
    /// **The key and never the namespace** — roadmap task **T84**, the design's D5. MixLab registers
    /// `mixlab://` with the operating system, so a URL is something a web page can make it receive; a
    /// URL that could name the *credential store's namespace* would be a way to read any secret on
    /// the machine and post it to a stranger's server as a password. A key it names reaches only
    /// MixEngine's own entries, which is the same set it could reach by naming a `label` and a
    /// `user`, so it adds no reach at all.
    ///
    /// It travels rather than being derived from `label` and `user` because the composition is then
    /// MixEngine's alone to change: a rule spelled out on both sides is a rule that drifts.
    pub secret_key: Option<&'a str>,
}

/// The address of one service, or [`None`] for a service no database client opens.
///
/// A row with no port is answered the same way: every database recipe binds a TCP port on all three
/// systems (T34c), so a row without one is a service nothing can dial.
///
/// **A service with no package is one of those, not a missing service.** `services` carries
/// `CHECK (((package_id IS NOT NULL) + (runtime_install_id IS NOT NULL) + (extension_id IS NOT
/// NULL)) = 1)`: a service has exactly one parent of three, and only one of the three is a package.
/// So the join is a `LEFT JOIN` — an inner one dropped every php-fpm pool and every extension's
/// service, and the `ok_or_else` below then reported "no such service" about a row `service.list`
/// lists and `mix service status` reads. The three `database.*` methods built on this all repeated
/// it, with a hint telling the reader to run the very command that disagreed.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such row, [`Error::Database`] when it cannot be read.
pub async fn address(store: &Store, service: &ServiceId) -> Result<Option<Address>> {
    let id = service.as_str();

    let row = sqlx::query!(
        r#"SELECT p.name AS "package?", s.port, s.bind_addr
         FROM services s
         LEFT JOIN packages p ON p.id = s.package_id
         WHERE s.id = ?"#,
        id
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    .ok_or_else(|| Error::NotFound {
        kind: "service",
        id: id.to_owned(),
    })?;

    // A service that runs out of a runtime install or an extension has no package to name a recipe,
    // and nothing opens one — the same answer a package this build has no recipe for gets.
    let Some(package) = row.package else {
        return Ok(None);
    };

    let catalogue = Catalogue::builtin();
    let Some(recipe) = catalogue.recipe(&package) else {
        return Ok(None);
    };
    let (Some(protocol), Some(port)) = (recipe.protocol(), row.port) else {
        return Ok(None);
    };

    Ok(Some(Address {
        protocol,
        host: crate::services::ports::bind_address(Some(row.bind_addr.as_str())),
        // The column is an `INTEGER`; a value outside a port's range is a row nothing wrote.
        port: u16::try_from(port).unwrap_or_default(),
        administrator: recipe.administrator().map(str::to_owned),
        creates_databases: recipe.databases().is_some(),
    }))
}

/// The URL the client is started with.
///
/// `<scheme>://connect?kind=…&host=…&port=…[&user=…][&database=…]&label=…[&password_env=…][&secret_key=…]`.
/// `password_env` and `secret_key` are present exactly when `user` is: they say where the password
/// is now and where it stays, and there is none to say anything about for a server with no accounts.
#[must_use]
pub fn url(connection: &Connection<'_>) -> String {
    let address = connection.address;
    let mut rendered = format!(
        "{}://connect?kind={}&host={}&port={}",
        connection.scheme,
        address.protocol.as_str(),
        encode(&address.host.to_string()),
        address.port
    );

    if let Some(user) = connection.user {
        let _ = write!(rendered, "&user={}", encode(user));
    }
    if let Some(database) = connection.database {
        let _ = write!(rendered, "&database={}", encode(database));
    }
    let _ = write!(rendered, "&label={}", encode(connection.label));
    if connection.user.is_some() {
        let _ = write!(rendered, "&password_env={CREDENTIAL_ENV}");
    }

    // **Roadmap task T84, the design's D5.** The key half of the address, so a client saving this
    // connection can point at MixEngine's entry rather than keeping a second copy of what is in it.
    // The namespace is a convention both applications hold and is deliberately not here.
    if let Some(key) = connection.secret_key {
        let _ = write!(rendered, "&secret_key={}", encode(key));
    }

    rendered
}

/// Percent-encode everything outside RFC 3986's unreserved set.
#[must_use]
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address_of(protocol: DatabaseProtocol, administrator: Option<&str>) -> Address {
        Address {
            protocol,
            host: IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            port: 3306,
            administrator: administrator.map(str::to_owned),
            creates_databases: true,
        }
    }

    /// **One composition, published to another application** — roadmap task **T84**, the design's
    /// D6. The address MixLab is told to read and the address MixEngine writes are the same string
    /// because they are the same function, not because two `format!`s agree by inspection.
    #[test]
    fn the_key_a_recipe_writes_and_the_key_a_handoff_names_are_one_function() {
        let service = ServiceId::parse("mariadb@main").expect("an id");

        let home = HomeId::parse("9f3c1a77b204").expect("an id");

        assert_eq!(
            secret_key(&home, &service, "root"),
            "9f3c1a77b204/mariadb@main/root"
        );
        assert_eq!(
            secret_key(&home, &service, "blog"),
            "9f3c1a77b204/mariadb@main/blog"
        );

        // What the daemon's compatibility read looks at, and the one thing it must never do to an
        // address that already has no home in it.
        assert_eq!(
            secret_key_before_homes(&secret_key(&home, &service, "root")),
            Some("mariadb@main/root")
        );
        assert_eq!(secret_key_before_homes("root"), None);

        // An address from before this task has nothing older behind it, and saying so is what
        // keeps a miss to one lookup.
        assert_eq!(secret_key_before_homes("mariadb@main/root"), None);
        assert_eq!(
            secret_key_before_homes("extensions/phpmyadmin/config"),
            None
        );
    }

    /// Every recipe answers, and only the databases say a word.
    #[test]
    fn the_databases_speak_a_protocol_and_nothing_else_does() {
        let catalogue = Catalogue::builtin();
        let says = |package: &str| catalogue.recipe(package).expect("a recipe").protocol();

        assert_eq!(says("mariadb"), Some(DatabaseProtocol::Mysql));
        assert_eq!(says("mysql"), Some(DatabaseProtocol::Mysql));
        assert_eq!(says("postgres"), Some(DatabaseProtocol::Postgres));
        assert_eq!(says("redis"), Some(DatabaseProtocol::Redis));
        assert_eq!(says("mongodb"), Some(DatabaseProtocol::Mongodb));
        assert_eq!(says("memcached"), None);
        assert_eq!(says("caddy"), None);
        assert_eq!(says("php-fpm"), None);
    }

    /// The whole shape, with every optional part present.
    #[test]
    fn a_url_carries_the_address_the_account_and_the_variable_name() {
        let address = address_of(DatabaseProtocol::Mysql, Some("root"));
        let rendered = url(&Connection {
            scheme: "mixlab",
            label: "mariadb@main",
            address: &address,
            user: Some("blog"),
            database: Some("blog"),
            secret_key: Some("mariadb@main/blog"),
        });

        assert_eq!(
            rendered,
            "mixlab://connect?kind=mysql&host=127.0.0.1&port=3306&user=blog&database=blog\
             &label=mariadb%40main&password_env=MIXENGINE_DB_PASSWORD\
             &secret_key=mariadb%40main%2Fblog"
        );
    }

    /// **The key travels; the namespace never does** — roadmap task **T84**, the design's D5.
    ///
    /// MixLab registers `mixlab://` with the operating system, so a URL is something a web page can
    /// make it receive. One that could name the credential store's namespace would be a way to
    /// read any secret on the machine and send it to a stranger's server as a password; one naming
    /// a key reaches only MixEngine's own entries.
    #[test]
    fn a_url_carries_the_key_half_of_the_address_and_never_the_namespace() {
        let address = address_of(DatabaseProtocol::Mysql, Some("root"));
        let rendered = url(&Connection {
            scheme: "mixlab",
            label: "mariadb@main",
            address: &address,
            user: Some("blog"),
            database: None,
            secret_key: Some("mariadb@main/blog"),
        });

        assert!(
            rendered.contains("&secret_key=mariadb%40main%2Fblog"),
            "{rendered}"
        );
        assert!(!rendered.contains("secret_service"), "{rendered}");
        assert!(
            !rendered.contains(mixengine_proto::KEYRING_SERVICE),
            "the namespace is a convention, not a parameter: {rendered}"
        );
        assert!(!rendered.contains("password="), "{rendered}");
    }

    /// A server with no accounts hands over an address and a label, and names no variable — there
    /// is nothing in the environment for the client to read.
    #[test]
    fn a_redis_url_names_no_account_and_no_variable() {
        let mut address = address_of(DatabaseProtocol::Redis, None);
        address.port = 6379;
        let rendered = url(&Connection {
            scheme: "mixlab",
            label: "redis@main",
            address: &address,
            user: None,
            database: None,
            secret_key: None,
        });

        assert_eq!(
            rendered,
            "mixlab://connect?kind=redis&host=127.0.0.1&port=6379&label=redis%40main"
        );
        assert!(!rendered.contains("password"), "{rendered}");

        // And no key either — roadmap task **T84**. There is no entry, so there is nothing to
        // point a saved connection at.
        assert!(!rendered.contains("secret_key"), "{rendered}");
    }

    /// Everything outside the unreserved set is escaped, and the unreserved set is left alone.
    #[test]
    fn encoding_escapes_what_a_query_string_cannot_carry() {
        assert_eq!(encode("mariadb@main"), "mariadb%40main");
        assert_eq!(encode("a b&c=d/e"), "a%20b%26c%3Dd%2Fe");
        assert_eq!(encode("plain-name_1.0~x"), "plain-name_1.0~x");
        assert_eq!(encode("é"), "%C3%A9");
    }

    /// The row and the recipe together, over a real store: the address of a database, a cache a
    /// client opens, a cache nothing opens, and a service that is not there.
    #[tokio::test]
    async fn an_address_is_read_off_the_row_and_the_recipe() {
        let (_temp, store) = home().await;
        a_service(&store, "mariadb@main", "mariadb", 3307).await;
        a_service(&store, "redis@main", "redis", 6379).await;
        a_service(&store, "memcached@main", "memcached", 11211).await;

        let mariadb = address(&store, &ServiceId::parse("mariadb@main").expect("an id"))
            .await
            .expect("it reads")
            .expect("a database");
        assert_eq!(mariadb.protocol, DatabaseProtocol::Mysql);
        assert_eq!(mariadb.port, 3307);
        assert_eq!(mariadb.administrator.as_deref(), Some("root"));
        assert!(mariadb.creates_databases);

        let redis = address(&store, &ServiceId::parse("redis@main").expect("an id"))
            .await
            .expect("it reads")
            .expect("a cache a client opens");
        assert_eq!(redis.protocol, DatabaseProtocol::Redis);
        assert_eq!(redis.administrator, None);
        assert!(!redis.creates_databases, "a cache makes no databases");

        assert!(
            address(&store, &ServiceId::parse("memcached@main").expect("an id"))
                .await
                .expect("it reads")
                .is_none()
        );

        let missing = address(&store, &ServiceId::parse("postgres@main").expect("an id"))
            .await
            .expect_err("no such row");
        assert!(matches!(missing, Error::NotFound { .. }), "{missing}");
    }

    /// **A service whose row has no package is a service, not a missing one.** `php-fpm@8.4.24`
    /// is exactly that on a real home: it runs out of an installed *runtime*, so `package_id` is
    /// `NULL`, while `service.list` lists it like any other. Reading it through a join that drops
    /// the row answers "no such service" about something the very next command shows — and the
    /// three `database.*` methods built on this all repeat the lie.
    #[tokio::test]
    async fn a_service_with_no_package_is_not_a_missing_service() {
        let (_temp, store) = home().await;
        a_service_from_a_runtime(&store, "php-fpm@8.4.24", "8.4.24", 9000).await;

        let answered = address(&store, &ServiceId::parse("php-fpm@8.4.24").expect("an id"))
            .await
            .expect("a service with no package is still a service");

        assert!(
            answered.is_none(),
            "nothing opens a php-fpm pool: {answered:?}"
        );
    }

    /// A `services` row that runs out of an installed runtime rather than a package.
    ///
    /// `services` carries `CHECK (((package_id IS NOT NULL) + (runtime_install_id IS NOT NULL) +
    /// (extension_id IS NOT NULL)) = 1)` — a service has **exactly one** parent of three, and only
    /// one of those three is a package. That constraint is the whole reason this fixture exists.
    async fn a_service_from_a_runtime(store: &Store, service: &str, version: &str, port: i64) {
        let instance = service.split('@').nth(1).unwrap_or("main");

        let runtime_id = sqlx::query_scalar!(
            "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url, sha256)
             VALUES ('php', ?, 'release', '/runtimes/php', '2026-09-03T00:00:00Z', 1,
                     'https://example.invalid/php.zip', 'ab')
             RETURNING id",
            version
        )
        .fetch_one(store.pool())
        .await
        .expect("a runtime install row");

        sqlx::query!(
            "INSERT INTO services (id, runtime_install_id, instance_name, state, port, bind_addr)
             VALUES (?, ?, ?, 'stopped', ?, '127.0.0.1')",
            service,
            runtime_id,
            instance,
            port
        )
        .execute(store.pool())
        .await
        .expect("a service row");
    }

    /// An empty home with the migrations applied.
    async fn home() -> (tempfile::TempDir, Store) {
        let temp = tempfile::tempdir().expect("a temporary home");
        let store = Store::open(&temp.path().join("mixengine.db"))
            .await
            .expect("a store");
        (temp, store)
    }

    /// A `packages` row and the `services` row that runs out of it.
    async fn a_service(store: &Store, service: &str, package: &str, port: i64) {
        let instance = service.split('@').nth(1).unwrap_or("main");

        let package_id = sqlx::query_scalar!(
            "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES (?, '1.0.0', '/packages/x', '2026-09-03T00:00:00Z',
                     'https://example.invalid/x.zip', 'ab')
             ON CONFLICT (name, version) DO UPDATE SET name = excluded.name
             RETURNING id",
            package
        )
        .fetch_one(store.pool())
        .await
        .expect("a package row");

        sqlx::query!(
            "INSERT INTO services (id, package_id, instance_name, state, port, bind_addr)
             VALUES (?, ?, ?, 'stopped', ?, '127.0.0.1')",
            service,
            package_id,
            instance,
            port
        )
        .execute(store.pool())
        .await
        .expect("a service row");
    }
}
