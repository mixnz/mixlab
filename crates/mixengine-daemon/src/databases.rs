//! `database.create` — roadmap task **T77a**.
//!
//! Thin, like every handler here: it validates two names, makes sure the instance is up, and hands
//! the work to [`crate::services::databases`]. What it adds is the two refusals a caller can act on
//! — a package with no databases at all, and a service this home does not declare — and it tells
//! them apart, because "no such service: redis@main" would send somebody looking for a service that
//! is right there.
//!
//! **One provisioning at a time per instance** — the T77a design, D10. PostgreSQL's conditional
//! creation reads and then writes, so two callers racing for the same database would put one of them
//! into `database "blog" already exists`. The map here folds the second into waiting for the first,
//! on the precedent of the one [`crate::packages`] holds.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use mixengine_core::Store;
use mixengine_core::generate::databases::{Ask, validated_identifier};
use mixengine_core::services::handoff::{self, CREDENTIAL_ENV, Connection};
use mixengine_core::window;
use mixengine_platform::{InstalledApp, Located, Started};
use mixengine_proto::{
    DatabaseAccount, DatabaseClientQuery, DatabaseClientReport, DatabaseCreate,
    DatabaseCredentials, DatabaseCredentialsQuery, DatabaseHandoff, DatabaseOpen, DesktopClient,
    Error, ErrorCode, Launch, SecretAddress, ServiceId,
};
use tokio::sync::Mutex;

use crate::error::ToWire as _;
use crate::services::databases::Making;

/// What a database will be handed to: which state to report, and what to start — roadmap tasks
/// **T107** and **T165**.
///
/// The scheme is not a member. The only client is this install's window, and its scheme is
/// [`window::SCHEME`].
struct Client {
    /// What `mix database client` prints, and what a refused `open` carries back.
    state: DesktopClient,

    /// How to start it, when this install has it.
    app: Option<InstalledApp>,
}

/// The `database.*` half of the API.
#[derive(Debug)]
pub(crate) struct Databases {
    /// What declares a service, remembers how its databases are made, and can start it.
    services: Arc<crate::services::Registry>,

    /// This machine, for its credential store and — roadmap task **T83** — the desktop client.
    host: Arc<dyn mixengine_platform::Host>,

    /// This home's tables: which service listens where.
    store: Store,

    /// One provisioning at a time per instance — see the module note.
    busy: Mutex<HashMap<ServiceId, Arc<Mutex<()>>>>,
}

impl Databases {
    /// The one of these the API holds.
    pub(crate) fn new(
        services: Arc<crate::services::Registry>,
        host: Arc<dyn mixengine_platform::Host>,
        store: Store,
    ) -> Arc<Self> {
        Arc::new(Self {
            services,
            host,
            store,
            busy: Mutex::new(HashMap::new()),
        })
    }

    /// `database.create` — make a database and the account that reaches it.
    ///
    /// # Errors
    ///
    /// `invalid_argument` for a name that cannot be one, and for a package with no databases;
    /// `not_found` for a service this home does not declare; `conflict` for an account MixEngine
    /// holds no credential for; `precondition_failed` for an instance that will not start.
    pub(crate) async fn create(&self, asked: &DatabaseCreate) -> Result<DatabaseAccount, Error> {
        self.make(asked).await.map_err(Making::into_wire)
    }

    /// [`Databases::create`], with the one refusal the blueprint's step reads by type — roadmap
    /// task **T202**, D1.
    ///
    /// # Errors
    ///
    /// [`Making::Foreign`] for an account on the server this home holds no credential for; every
    /// other refusal of [`Databases::create`] as [`Making::Failed`].
    pub(crate) async fn make(&self, asked: &DatabaseCreate) -> Result<DatabaseAccount, Making> {
        // Refused before the instance is started, on `blueprint.capture`'s reasoning: a name that
        // was never going to work should not first cost a database server coming up.
        let database = validated_identifier(&asked.database).map_err(|error| error.to_wire())?;
        let user = validated_identifier(asked.user.as_deref().unwrap_or(&database))
            .map_err(|error| error.to_wire())?;
        let password = asked
            .password
            .as_deref()
            .map(mixengine_core::generate::databases::validated_password)
            .transpose()
            .map_err(|error| error.to_wire())?;

        let provisioning = self.vocabulary(&asked.service).await?;

        // Held across the start and the statements — design D10.
        let gate = {
            let mut busy = self.busy.lock().await;

            Arc::clone(
                busy.entry(asked.service.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _turn = gate.lock().await;

        self.services.ensure_running(&asked.service).await?;

        let ask = Ask { database, user };
        let made = crate::services::databases::ensure(
            &self.host,
            &provisioning,
            &asked.service,
            &ask,
            password,
        )
        .await?;

        Ok(DatabaseAccount {
            service: asked.service.clone(),
            database: ask.database,
            secret: SecretAddress::of(provisioning.secret_address(&ask.user)),
            user: ask.user,
            made,
        })
    }

    /// How this service's databases are made, or which of the two misses it was.
    async fn vocabulary(
        &self,
        service: &ServiceId,
    ) -> Result<mixengine_core::generate::Provisioning, Error> {
        // The graph first, because it is what fills the registry's map: a daemon that has served
        // nothing yet remembers no provisioning for anything.
        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;

        if graph.spec(service).is_none() {
            return Err(mixengine_core::Error::Graph(
                mixengine_core::services::GraphError::NoSuchService {
                    id: service.clone(),
                },
            )
            .to_wire());
        }

        self.services.provisioning_for(service).ok_or_else(|| {
            mixengine_core::Error::NoDatabaseVocabulary {
                package: service.name().to_owned(),
            }
            .to_wire()
        })
    }

    /// `database.client` — where this instance could be opened. Reads only — roadmap task **T83**.
    ///
    /// # Errors
    ///
    /// `not_found` for a service this home does not declare; whatever locating the application
    /// costs on this system.
    pub(crate) async fn client(
        &self,
        asked: &DatabaseClientQuery,
    ) -> Result<DatabaseClientReport, Error> {
        let address = handoff::address(&self.store, &asked.service)
            .await
            .map_err(|error| error.to_wire())?;
        let client = self.locate_client().await?.state;

        // **Composed, not looked up** — roadmap task **T84**, the design's D6. This method still
        // touches the credential store not at all: the address is what the recipe's administrator
        // and the service id say it is, which is exactly what makes the convention askable before
        // anything has been opened.
        let home = self.home().await?;
        let secret = address.as_ref().and_then(|address| {
            address
                .administrator
                .as_deref()
                .map(|user| SecretAddress::of(handoff::secret_key(&home, &asked.service, user)))
        });

        Ok(DatabaseClientReport {
            service: asked.service.clone(),
            protocol: address.as_ref().map(|address| address.protocol),
            secret,
            creates_databases: address.map(|address| address.creates_databases),
            client,
        })
    }

    /// Which home this is, for the first segment of every credential address — roadmap task
    /// **T126**.
    ///
    /// Read per call rather than held on this type: it is one indexed row, the calls that need it
    /// are already several round trips to a database server, and a copy cached here would be a
    /// second place the home's identity lives.
    async fn home(&self) -> Result<mixengine_core::home::HomeId, Error> {
        mixengine_core::home::id(&self.store)
            .await
            .map_err(|error| error.to_wire())
    }

    /// `database.credentials` — the password held for one account. Reads only — roadmap task
    /// **T77b**.
    ///
    /// # Errors
    ///
    /// `invalid_argument` for a service no client opens or one with no accounts to read a
    /// password for; `not_found` for a service this home does not declare; `precondition_failed`
    /// for an account — administrator included — MixEngine holds no credential for.
    pub(crate) async fn credentials(
        &self,
        asked: &DatabaseCredentialsQuery,
    ) -> Result<DatabaseCredentials, Error> {
        let user = asked
            .user
            .as_deref()
            .map(validated_identifier)
            .transpose()
            .map_err(|error| error.to_wire())?;

        let address = handoff::address(&self.store, &asked.service)
            .await
            .map_err(|error| error.to_wire())?
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{} is not a database a client opens", asked.service),
                )
            })?;

        if user.is_some() && !address.protocol.has_accounts() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("{} has no accounts to sign in as", asked.service),
            )
            .with_hint("leave `--user` off: the server has no account to read a password for"));
        }

        let account = user
            .or_else(|| address.administrator.clone())
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{} has no accounts to sign in as", asked.service),
                )
            })?;

        let at = handoff::secret_key(&self.home().await?, &asked.service, &account);
        let password = self
            .credential(
                &asked.service,
                &account,
                &at,
                address.administrator.as_deref(),
            )
            .await?;

        Ok(DatabaseCredentials {
            service: asked.service.clone(),
            user: account,
            secret: SecretAddress::of(at),
            password,
        })
    }

    /// `database.open` — hand this instance to this install's window — roadmap tasks **T83** and **T165**.
    ///
    /// The order is the design's data flow: validate, address, locate, start, read the credential,
    /// launch. "No client" comes back as a state before anything is started, and
    /// the credential is read as late as the order allows.
    ///
    /// # Errors
    ///
    /// `invalid_argument` for a name that cannot be one, a service no client opens, or an account
    /// on a server without accounts; `not_found` for a service this home does not declare;
    /// `precondition_failed` for an instance that will not start or an account MixEngine holds no
    /// credential for; `process_failed` for an application that died within a second of starting.
    pub(crate) async fn open(&self, asked: &DatabaseOpen) -> Result<DatabaseHandoff, Error> {
        let user = asked
            .user
            .as_deref()
            .map(validated_identifier)
            .transpose()
            .map_err(|error| error.to_wire())?;
        let database = asked
            .database
            .as_deref()
            .map(validated_identifier)
            .transpose()
            .map_err(|error| error.to_wire())?;

        let address = handoff::address(&self.store, &asked.service)
            .await
            .map_err(|error| error.to_wire())?
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{} is not a database a desktop client opens", asked.service),
                )
            })?;

        if user.is_some() && !address.protocol.has_accounts() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("{} has no accounts to sign in as", asked.service),
            )
            .with_hint("leave `--user` off: the client connects without one"));
        }

        let located = self.locate_client().await?;
        let Some(app) = located.app else {
            return Ok(DatabaseHandoff {
                service: asked.service.clone(),
                protocol: address.protocol,
                user: None,
                database,
                secret: None,
                client: located.state,
                launched: None,
            });
        };
        let client = located.state;

        self.services.ensure_running(&asked.service).await?;

        let account = user.or_else(|| address.administrator.clone());
        let (secret, env) = match &account {
            Some(account) => {
                // The shared composition — roadmap task **T84**. The recipe that wrote this entry
                // and the handoff that reads it name one function, so the two cannot drift.
                let at = handoff::secret_key(&self.home().await?, &asked.service, account);
                let password = self
                    .credential(
                        &asked.service,
                        account,
                        &at,
                        address.administrator.as_deref(),
                    )
                    .await?;
                (
                    Some(SecretAddress::of(at)),
                    BTreeMap::from([(CREDENTIAL_ENV.to_owned(), password)]),
                )
            }
            None => (None, BTreeMap::new()),
        };

        let url = handoff::url(&Connection {
            scheme: window::SCHEME,
            label: asked.service.as_str(),
            address: &address,
            user: account.as_deref(),
            database: database.as_deref(),
            secret_key: secret.as_ref().map(|at| at.key.as_str()),
        });

        let launched = self.launch(&app, url, env).await?;

        Ok(DatabaseHandoff {
            service: asked.service.clone(),
            protocol: address.protocol,
            user: account,
            database,
            secret,
            client,
            launched: Some(launched),
        })
    }

    /// The client as a state, and how to start it — roadmap tasks **T107** and **T165**.
    ///
    /// **This install's window, or nothing** (ADR 0038). No extension is consulted: the only
    /// application a database is handed to is the MixLab this MixEngine installed, found where that
    /// install put it.
    async fn locate_client(&self) -> Result<Client, Error> {
        Ok(match self.locate_window().await? {
            Located::Installed(app) => Client {
                state: DesktopClient::Installed {
                    name: window::NAME.to_owned(),
                    program: app.program.display().to_string(),
                },
                app: Some(app),
            },
            Located::NotInstalled { .. } => Client {
                state: DesktopClient::NoClient,
                app: None,
            },
        })
    }

    /// This install's own window, off the runtime — roadmap task **T107**.
    ///
    /// A handful of `stat`s, and still through `spawn_blocking`: the platform layer's contract is a
    /// blocking call, and a caller reading this should not have to know that it is cheap this month.
    async fn locate_window(&self) -> Result<Located, Error> {
        let host = Arc::clone(&self.host);

        tokio::task::spawn_blocking(move || {
            host.desktop_apps()
                .locate_window(window::EXECUTABLE, window::BUNDLE)
        })
        .await
        .map_err(|_| {
            Error::new(
                ErrorCode::Internal,
                "the task locating the window did not finish".to_owned(),
            )
        })?
        .map_err(|error| error.to_wire())
    }

    /// The account's password, read now — the moment of the handoff and no earlier.
    async fn credential(
        &self,
        service: &ServiceId,
        account: &str,
        at: &str,
        administrator: Option<&str>,
    ) -> Result<String, Error> {
        crate::services::databases::read(&self.host, at)
            .await?
            .ok_or_else(|| {
                if Some(account) == administrator {
                    Error::new(
                        ErrorCode::PreconditionFailed,
                        format!("{service} has no superuser credential in this machine's keyring"),
                    )
                    .with_hint(
                        "that password is written by the service's first run; `mix service \
                         start` performs it",
                    )
                } else {
                    Error::new(
                        ErrorCode::PreconditionFailed,
                        format!("MixEngine holds no credential for `{account}` on {service}"),
                    )
                    .with_hint(format!(
                        "`mix database create {service} --name <database> --user {account}` \
                         makes one"
                    ))
                }
            })
    }

    /// Start the client, off the runtime, and judge it — the design's D8.
    async fn launch(
        &self,
        app: &InstalledApp,
        url: String,
        env: BTreeMap<String, String>,
    ) -> Result<Launch, Error> {
        let host = Arc::clone(&self.host);
        let app = app.clone();
        let program = app.program.display().to_string();

        let started = tokio::task::spawn_blocking(move || {
            host.desktop_apps()
                .launch(&app, &[std::ffi::OsString::from(url)], &env)
        })
        .await
        .map_err(|_| {
            Error::new(
                ErrorCode::Internal,
                "the task starting the client did not finish".to_owned(),
            )
        })?
        .map_err(|error| error.to_wire())?;

        match started {
            Started::Running { pid } => Ok(Launch::Running { pid }),
            Started::HandedOn => Ok(Launch::HandedOn),
            Started::Failed { status } => Err(Error::new(
                ErrorCode::ProcessFailed,
                format!("the client exited a moment after it was started ({status})"),
            )
            .with_hint(format!("run it by hand to read what it says: {program}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mixengine_platform::mock::Host as MockHost;
    use mixengine_platform::{Host as _, KEYRING_SERVICE};
    use mixengine_proto::{
        DatabaseClientQuery, DatabaseOpen, DatabaseProtocol, DesktopClient, ErrorCode, Launch,
        ServiceId,
    };

    use super::*;
    use crate::services::fixture;

    fn id(text: &str) -> ServiceId {
        ServiceId::parse(text).expect("an id")
    }

    /// A home with the given database rows, a fakeservice declared under each id, on `host`.
    async fn databases(
        host: Arc<MockHost>,
        rows: &[(&str, &str, i64)],
    ) -> (mixengine_testkit::Home, Arc<Databases>) {
        let (home, paths, store) = fixture::home(&[]).await;

        for (service, package, port) in rows {
            let instance = service.split('@').nth(1).unwrap_or("main");
            let package_id: i64 = sqlx::query_scalar(
                "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
                 VALUES (?, '1.0.0', '/packages/x', '2026-09-03T00:00:00Z', 'https://example', 'ab')
                 ON CONFLICT (name, version) DO UPDATE SET name = excluded.name RETURNING id",
            )
            .bind(package)
            .fetch_one(store.pool())
            .await
            .expect("a package row");

            sqlx::query(
                "INSERT INTO services (id, package_id, instance_name, state, port, bind_addr)
                 VALUES (?, ?, ?, 'stopped', ?, '127.0.0.1')",
            )
            .bind(service)
            .bind(package_id)
            .bind(instance)
            .bind(port)
            .execute(store.pool())
            .await
            .expect("a service row");
        }

        let specs = rows
            .iter()
            .map(|(service, _, _)| fixture::spec(service).build().expect("a spec"))
            .collect();
        let services = Arc::new(fixture::registry_on(
            &paths,
            &store,
            Arc::new(fixture::Declared(specs)),
            Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
        ));

        (home, Databases::new(services, host, store))
    }

    fn open(service: &str, user: Option<&str>, database: Option<&str>) -> DatabaseOpen {
        DatabaseOpen {
            service: id(service),
            user: user.map(str::to_owned),
            database: database.map(str::to_owned),
        }
    }

    /// An install with no window is a state, to both methods, and nothing is started for it.
    #[tokio::test]
    async fn with_no_window_the_answer_is_no_client() {
        let host = Arc::new(MockHost::with_home(std::env::temp_dir()));
        let (_home, databases) = databases(host, &[("redis@main", "redis", 6379)]).await;

        let report = databases
            .client(&DatabaseClientQuery {
                service: id("redis@main"),
            })
            .await
            .expect("answers");
        assert_eq!(report.protocol, Some(DatabaseProtocol::Redis));
        assert_eq!(report.creates_databases, Some(false));
        assert_eq!(report.client, DesktopClient::NoClient);

        let handoff = databases
            .open(&open("redis@main", None, None))
            .await
            .expect("a state");
        assert_eq!(handoff.client, DesktopClient::NoClient);
        assert_eq!(handoff.launched, None);
    }

    /// **The merged product's client** — roadmap task **T107**. Nothing is installed besides
    /// MixEngine, and `mix database open` still lands somewhere.
    #[tokio::test]
    async fn the_window_is_the_client() {
        let host = Arc::new(MockHost::with_window(
            std::env::temp_dir(),
            "/opt/mixengine/mixlab",
        ));
        let (_home, databases) = databases(host, &[("redis@main", "redis", 6379)]).await;

        let report = databases
            .client(&DatabaseClientQuery {
                service: id("redis@main"),
            })
            .await
            .expect("answers");

        match report.client {
            DesktopClient::Installed { name, program } => {
                assert_eq!(name, mixengine_core::window::NAME);
                assert!(program.contains("mixlab"), "{program}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// The URL a window handoff produces carries the window's own scheme, and it is the window that
    /// is started — T83's rule, on the new path.
    #[tokio::test]
    async fn a_window_handoff_carries_the_scheme_and_starts_the_window() {
        let host = Arc::new(MockHost::with_window(
            std::env::temp_dir(),
            "/opt/mixengine/mixlab",
        ));
        let (_home, databases) =
            databases(Arc::clone(&host), &[("redis@main", "redis", 6379)]).await;

        let handoff = databases
            .open(&open("redis@main", None, None))
            .await
            .expect("opened");
        assert_eq!(handoff.launched, Some(Launch::Running { pid: 4242 }));

        let launched = host.launched();
        assert_eq!(launched.len(), 1, "{launched:?}");
        assert!(
            launched[0].program.ends_with("mixlab"),
            "{:?}",
            launched[0].program
        );
        let url = launched[0]
            .args
            .last()
            .expect("the URL")
            .to_string_lossy()
            .into_owned();
        assert!(
            url.starts_with(&format!("{}://connect?", mixengine_core::window::SCHEME)),
            "{url}"
        );
    }

    /// A service no client opens: `protocol: null` to `client`, a refusal by name to `open` — the
    /// T77a distinction between the package and the operating system.
    #[tokio::test]
    async fn a_cache_with_no_protocol_is_a_state_to_client_and_a_refusal_to_open() {
        let host = Arc::new(MockHost::with_home(std::env::temp_dir()));
        let (_home, databases) = databases(host, &[("memcached@main", "memcached", 11211)]).await;

        let report = databases
            .client(&DatabaseClientQuery {
                service: id("memcached@main"),
            })
            .await
            .expect("answers");
        assert_eq!(report.protocol, None);
        assert_eq!(report.creates_databases, None);

        let refused = databases
            .open(&open("memcached@main", None, None))
            .await
            .expect_err("refused");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(
            refused.message.contains("memcached@main"),
            "{}",
            refused.message
        );
    }

    /// A server with no accounts is handed over with no variable at all, and refuses `--user`.
    #[tokio::test]
    async fn a_redis_is_opened_with_no_account_and_no_variable() {
        let host = Arc::new(MockHost::with_window(
            std::env::temp_dir(),
            "/opt/mixengine/mixlab",
        ));
        let (_home, databases) =
            databases(Arc::clone(&host), &[("redis@main", "redis", 6379)]).await;

        let handoff = databases
            .open(&open("redis@main", None, None))
            .await
            .expect("opened");
        assert_eq!(handoff.launched, Some(Launch::Running { pid: 4242 }));
        assert_eq!(handoff.secret, None);

        let launched = host.launched();
        assert_eq!(launched.len(), 1);
        let url = launched[0]
            .args
            .last()
            .expect("the URL")
            .to_string_lossy()
            .into_owned();
        assert!(
            url.starts_with("mixlab://connect?kind=redis&host=127.0.0.1&port=6379"),
            "{url}"
        );
        assert!(!url.contains("password"), "{url}");
        // A server with no accounts has no entry to point a saved connection at — T84.
        assert!(!url.contains("secret_key"), "{url}");
        assert!(launched[0].env_names.is_empty());

        let refused = databases
            .open(&open("redis@main", Some("x"), None))
            .await
            .expect_err("no accounts");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
    }

    /// **A MongoDB is a Redis to this method** — roadmap task **T155**: no accounts, no variable,
    /// no form to make a database, and the kind the window reads as its Mongo tab.
    #[tokio::test]
    async fn a_mongodb_is_opened_with_no_account_and_offers_no_database_to_create() {
        let host = Arc::new(MockHost::with_window(
            std::env::temp_dir(),
            "/opt/mixengine/mixlab",
        ));
        let (_home, databases) =
            databases(Arc::clone(&host), &[("mongodb@main", "mongodb", 27017)]).await;

        let report = databases
            .client(&DatabaseClientQuery {
                service: id("mongodb@main"),
            })
            .await
            .expect("answers");
        assert_eq!(report.protocol, Some(DatabaseProtocol::Mongodb));
        assert_eq!(report.secret, None);
        assert_eq!(report.creates_databases, Some(false));

        databases
            .open(&open("mongodb@main", None, Some("blog")))
            .await
            .expect("opened");

        let launched = host.launched();
        let url = launched[0]
            .args
            .last()
            .expect("the URL")
            .to_string_lossy()
            .into_owned();
        assert!(
            url.starts_with("mixlab://connect?kind=mongodb&host=127.0.0.1&port=27017"),
            "{url}"
        );
        assert!(url.contains("&database=blog"), "{url}");
        assert!(launched[0].env_names.is_empty());
    }

    /// **The design's D2, at the daemon.** The password is in the environment under the one name
    /// and nowhere in the URL; a missing credential is a precondition and starts nothing.
    #[tokio::test]
    async fn a_database_is_opened_with_the_credential_in_the_environment_and_not_the_url() {
        let host = Arc::new(MockHost::with_window(
            std::env::temp_dir(),
            "/opt/mixengine/mixlab",
        ));
        let (_home, databases) =
            databases(Arc::clone(&host), &[("mariadb@main", "mariadb", 3306)]).await;

        // **`client` says where the credential is, and reads nothing to find out** — roadmap task
        // **T84**, the design's D6. Asked before anything has been stored, so an answer here can
        // only have been composed.
        let report = databases
            .client(&DatabaseClientQuery {
                service: id("mariadb@main"),
            })
            .await
            .expect("a report");
        let address = report.secret.as_ref().expect("an address");
        assert_eq!(address.service, KEYRING_SERVICE);
        assert_eq!(address.key, at(&databases, "mariadb@main/root").await);
        assert_eq!(
            report.creates_databases,
            Some(true),
            "a server that makes databases keeps its form"
        );

        let missing = databases
            .open(&open("mariadb@main", None, None))
            .await
            .expect_err("no credential yet");
        assert_eq!(missing.code, ErrorCode::PreconditionFailed);
        assert!(host.launched().is_empty());

        // **Stored at the address a build before T126 would have used**, which is what makes the
        // two assertions below one test of the compatibility path: the read finds it, the answer
        // names this home's address, and the entry has been moved there by the time it returns.
        host.keyring()
            .set_secret(KEYRING_SERVICE, "mariadb@main/root", "s3cret-value")
            .expect("stored");

        let handoff = databases
            .open(&open("mariadb@main", None, Some("blog")))
            .await
            .expect("opened");
        let address = handoff.secret.as_ref().expect("an address");
        assert_eq!(address.service, mixengine_platform::KEYRING_SERVICE);
        assert_eq!(address.key, at(&databases, "mariadb@main/root").await);
        assert_eq!(handoff.user.as_deref(), Some("root"));

        let launched = host.launched();
        let url = launched[0]
            .args
            .last()
            .expect("the URL")
            .to_string_lossy()
            .into_owned();
        // The home's half of the address is percent-encoded like the rest of it — roadmap task
        // **T126** — so what the window is handed is one opaque key it looks up, exactly as before.
        let encoded = at(&databases, "mariadb@main/root")
            .await
            .replace('@', "%40")
            .replace('/', "%2F");

        assert!(
            url.contains(&format!(
                "kind=mysql&host=127.0.0.1&port=3306&user=root&database=blog&label=mariadb%40main&password_env=MIXENGINE_DB_PASSWORD&secret_key={encoded}"
            )),
            "{url}"
        );
        assert!(!url.contains("s3cret"), "{url}");
        // **The key travels, the namespace does not** — roadmap task **T84**, the design's D5.
        assert!(!url.contains(KEYRING_SERVICE), "{url}");
        assert_eq!(
            launched[0].env_names,
            vec!["MIXENGINE_DB_PASSWORD".to_owned()]
        );

        let named = databases
            .open(&open("mariadb@main", Some("blog"), None))
            .await
            .expect_err("no such account of ours");
        assert_eq!(named.code, ErrorCode::PreconditionFailed);
        assert!(named.message.contains("blog"), "{}", named.message);
    }

    fn credentials_query(service: &str, user: Option<&str>) -> DatabaseCredentialsQuery {
        DatabaseCredentialsQuery {
            service: id(service),
            user: user.map(str::to_owned),
        }
    }

    /// The address this home spells `key` at — roadmap task **T126**.
    ///
    /// Composed rather than written down because a home's id is random and per home; what the tests
    /// below assert is that the *service's* half is right and that the home's half is there at all.
    async fn at(databases: &Databases, key: &str) -> String {
        let home = mixengine_core::home::id(&databases.store)
            .await
            .expect("a migrated home has an id");

        format!("{home}/{key}")
    }

    /// **The administrator by default** — roadmap task **T77b** — exactly as `database.open`'s own
    /// default: the two commands answer the same question for a person.
    #[tokio::test]
    async fn credentials_defaults_to_the_administrator() {
        let host = Arc::new(MockHost::with_home(std::env::temp_dir()));
        host.keyring()
            .set_secret(KEYRING_SERVICE, "mariadb@main/root", "root-secret")
            .expect("the mock store takes it");
        let (_home, databases) =
            databases(Arc::clone(&host), &[("mariadb@main", "mariadb", 3306)]).await;

        let answer = databases
            .credentials(&credentials_query("mariadb@main", None))
            .await
            .expect("answers");

        assert_eq!(answer.user, "root");

        // The entry was written at the pre-T126 address and is still answered — and the address
        // handed back is this home's, which is where the read has just moved it.
        assert_eq!(answer.password, "root-secret");
        assert_eq!(answer.secret.key, at(&databases, "mariadb@main/root").await);
        assert_eq!(
            host.keyring()
                .secret(KEYRING_SERVICE, &at(&databases, "mariadb@main/root").await)
                .expect("a read")
                .as_deref(),
            Some("root-secret"),
            "the compatibility read moves what it finds — `crate::secrets`"
        );
    }

    /// A named account with no entry is the same refusal `database.open` already gives, with the
    /// same hint pointing at `database create`.
    #[tokio::test]
    async fn credentials_for_an_account_with_no_entry_is_precondition_failed() {
        let host = Arc::new(MockHost::with_home(std::env::temp_dir()));
        let (_home, databases) =
            databases(Arc::clone(&host), &[("mariadb@main", "mariadb", 3306)]).await;

        let error = databases
            .credentials(&credentials_query("mariadb@main", Some("blog")))
            .await
            .expect_err("no entry for blog yet");

        assert_eq!(error.code, ErrorCode::PreconditionFailed);
    }

    /// A service with no accounts refuses by name, as `database.open --user` already does.
    #[tokio::test]
    async fn credentials_on_a_service_with_no_accounts_is_invalid_argument() {
        let host = Arc::new(MockHost::with_home(std::env::temp_dir()));
        let (_home, databases) =
            databases(Arc::clone(&host), &[("redis@main", "redis", 6379)]).await;

        let error = databases
            .credentials(&credentials_query("redis@main", None))
            .await
            .expect_err("redis has no accounts");

        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}
