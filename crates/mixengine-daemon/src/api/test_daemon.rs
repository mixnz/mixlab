//! A daemon for the API's unit tests: a temporary home, a registry, and an [`Api`] assembled from
//! parts a test chooses — the machine, the declared services, the update feed.
//!
//! Inside `api` because it writes an `Api` struct literal, which names every private field; shared
//! by the test module of every handler file here.

use std::sync::Arc;
use std::time::Instant;

use mixengine_proto::rpc;
use mixengine_proto::{ServiceState, ServiceSummary};
use mixengine_testkit::Home;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::Api;
use super::rpc::answer;
use crate::services;
use crate::services::fixture::{self, EVENTUALLY};

/// The shutdown budget these tests give a daemon.
///
/// Generous, because nothing here is measuring how a budget runs out — that is the runner's
/// arithmetic and is tested where it lives. What these tests assert is the *order*: services
/// stopped, then the token cancelled, then the answer.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// A daemon with a home under it: a database, a registry and whatever it declares.
///
/// The `daemon.*` handlers read state captured at startup and touch neither, which is what let
/// this be a bare struct before T19a. `service.*` reads the `services` rows and the registry, so
/// the home is a real one and is held here for as long as the test needs it.
pub(super) struct Daemon {
    pub(super) _home: Home,
    pub(super) api: Arc<Api>,
    pub(super) services: Arc<services::Registry>,

    /// The same machine the API was built with, so a test can ask what it was told to do.
    ///
    /// Held as the concrete mock rather than as `dyn Host`, because what is worth asserting is
    /// the recording — `path_operations`, `restricted` — and the trait deliberately has no way
    /// to ask for it.
    pub(super) host: Arc<mixengine_platform::mock::Host>,

    /// The path this daemon believes it runs from, which `update.status` asks for its version
    /// once a `.pkg` has been handed over — roadmap task **T88f**.
    pub(super) installed_daemon: std::path::PathBuf,
}

impl Daemon {
    /// One call, decoded.
    pub(super) async fn call(&self, body: &str) -> Value {
        answer_json(&self.api, body).await
    }

    /// One `service.*` call, built from its parameters.
    pub(super) async fn ask(&self, method: &str, params: Value) -> Value {
        let body = serde_json::json!({
            "jsonrpc": "2.0", "method": method, "params": params, "id": 1
        });

        self.call(&body.to_string()).await
    }

    /// The result of a call that was expected to succeed, as the type it documents.
    pub(super) async fn expect<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Value,
    ) -> T {
        let answer = self.ask(method, params).await;

        serde_json::from_value(answer["result"].clone())
            .unwrap_or_else(|error| panic!("{method} answered {answer}: {error}"))
    }

    /// What `service.status` says this service is doing.
    pub(super) async fn state(&self, id: &str) -> Option<ServiceState> {
        let summary: ServiceSummary = self
            .expect(
                rpc::method::SERVICE_STATUS,
                serde_json::json!({"service": id}),
            )
            .await;

        summary.state
    }

    /// Wait for a service to reach a state, for the calls that answer before it has.
    pub(super) async fn until(&self, id: &str, state: ServiceState) {
        let deadline = Instant::now() + EVENTUALLY;

        while Instant::now() < deadline {
            if self.state(id).await == Some(state) {
                return;
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        panic!(
            "{id} never reached {state}, and is {:?}",
            self.state(id).await
        );
    }

    /// Stop everything this daemon is supervising, as `serve` does on its way out.
    ///
    /// Called by every test that started something: a `Home` is a temporary directory, and on
    /// Windows one cannot be removed while a process it holds the log file of is still running.
    pub(super) async fn quiet(self) {
        self.services.shut_down().await;
    }
}

/// A daemon declaring `specs`, with a `services` row for each of `rows`.
///
/// The two lists are separate on purpose: a declaration and a row are different things, and the
/// one test that gives a service the first without the second is testing exactly that.
pub(super) async fn daemon(specs: Arc<dyn services::SpecSource>, rows: &[&str]) -> Daemon {
    daemon_on(specs, rows, mixengine_platform::mock::Host::with_home).await
}

/// The same daemon on a machine the caller describes — roadmap task **T97**.
///
/// **A constructor rather than a host**, because the mock's own constructors all take the home
/// and this harness is the only thing that knows where it is. What varies between the switch
/// tests is exactly one thing: whether this machine will let a given binary answer on 80 and
/// 443, which is not something a test can arrange any other way.
pub(super) async fn daemon_on<H>(
    specs: Arc<dyn services::SpecSource>,
    rows: &[&str],
    machine: H,
) -> Daemon
where
    H: FnOnce(std::path::PathBuf) -> mixengine_platform::mock::Host,
{
    daemon_reading(specs, rows, machine, None).await
}

/// The same daemon reading the update feed at `feed` — roadmap task **T88f**. [`None`] points it
/// at a URL nothing answers on, which is what every other test here wants.
pub(super) async fn daemon_reading<H>(
    specs: Arc<dyn services::SpecSource>,
    rows: &[&str],
    machine: H,
    feed: Option<crate::updates::FeedSource>,
) -> Daemon
where
    H: FnOnce(std::path::PathBuf) -> mixengine_platform::mock::Host,
{
    let (home, paths, store) = fixture::home(rows).await;

    // A daemon creates `cache/` at start, and the feed client writes its verified copy there.
    if feed.is_some() {
        std::fs::create_dir_all(paths.cache()).expect("a cache directory in a temporary home");
    }
    let events = super::Events::new();

    // Before the registry, which takes it: a start may have a first-run ritual to perform, and
    // that is a job — roadmap task T33.
    let jobs = Arc::new(crate::jobs::Jobs::new(
        &store,
        events.clone(),
        CancellationToken::new(),
    ));

    let services = Arc::new(services::Registry::new(
        &paths,
        &store,
        Arc::new(mixengine_platform::mock::Host::with_home(paths.root())),
        events.clone(),
        specs,
        CancellationToken::new(),
        Arc::clone(&jobs),
    ));

    // One transport, shared the way `main` shares it — roadmap task **T72b** — so this test
    // context stays an honest copy of what the daemon actually builds.
    let transport =
        mixengine_core::index::default_transport().expect("an HTTP client can be built");

    // Pointed at the published index, which nothing in this file asks anything of: these tests
    // are about dispatch, and every `runtime.*` method's own behaviour is proved against a
    // `MockRegistry` in `tests/runtimes.rs`, where there is a real socket to serve one over.
    // Constructing it here is still worth doing rather than stubbing — it is the one assertion
    // available that a daemon builds one at all without reaching the network to do it.
    let fetcher = crate::runtimes::Fetcher::new(
        &paths,
        &crate::runtimes::IndexSource::default(),
        transport.clone(),
    )
    .expect("the compiled-in index key is a key");
    let runtimes = crate::runtimes::Runtimes::new(
        &paths,
        &store,
        Arc::clone(&jobs),
        Arc::clone(&fetcher),
        Arc::clone(&services),
    );
    let packages = crate::packages::Packages::new(&paths, &store, Arc::clone(&jobs), fetcher);

    // A stand-in for the two binaries a release ships side by side. `shims::source` looks for
    // the shim *beside the program that is running*, and the program running these tests is a
    // test binary in `target/debug/deps` — so the pair is made here, inside the home this test
    // owns, and the copies that land in `bin/` are copies of a file with known contents.
    let installed = paths.root().join("installed-beside");
    std::fs::create_dir_all(&installed).expect("a directory in a temporary home");
    std::fs::write(
        installed.join(format!("mixengine-shim{}", std::env::consts::EXE_SUFFIX)),
        b"the shim, as far as a copy is concerned",
    )
    .expect("a file in a temporary home");
    // What `bin/` is filled from on Windows since T185, and harmless beside it elsewhere.
    std::fs::write(
        installed.join(format!(
            "mixengine-trampoline{}",
            std::env::consts::EXE_SUFFIX
        )),
        b"the trampoline, as far as a copy is concerned",
    )
    .expect("a file in a temporary home");

    let host = Arc::new(machine(paths.root().to_path_buf()));

    let shims = Arc::new(crate::shims::Shims::new(
        &paths,
        installed.join(format!("mixengined{}", std::env::consts::EXE_SUFFIX)),
        Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
        store.clone(),
        crate::services::spec::catalogue(),
    ));

    let autostart = Arc::new(crate::autostart::Autostart::new(
        installed.join(format!("mixengined{}", std::env::consts::EXE_SUFFIX)),
        paths.root().to_path_buf(),
        Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
    ));

    let elevation = crate::elevation::Elevation::new(
        &paths,
        &store,
        events.clone(),
        Arc::clone(&jobs),
        Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
        crate::elevation::Candidates {
            program: installed.join(format!("mixengined{}", std::env::consts::EXE_SUFFIX)),
            // A machine with no installed helper, stated rather than inherited — T85's D5. The
            // machine running these tests may well have one, and every assertion here is about
            // the copy this fixture puts beside its own `mixengined`.
            installed: None,
        },
        Arc::new(crate::dns::Dns::hosts_only_for_tests()),
    );

    let sites = crate::sites::Sites::new(
        &store,
        Arc::clone(&elevation),
        Arc::clone(&services),
        &paths,
        Arc::new(crate::mdns::Mdns::silent_for_tests()),
        crate::api::Events::new(),
    );

    let armed = Arc::new(super::Armed::default());

    // Pointed at a URL nothing answers on, which is the state these tests want: every one of
    // them asks what a daemon that has never read a feed says, and a fixture that could reach
    // the published one would be a test suite with an opinion about the network. Built before
    // `Api` because the uninstall reads it too (T185a).
    let updates = crate::updates::Updates::new(
        &paths,
        &store,
        &feed.unwrap_or_else(|| crate::updates::FeedSource {
            url: "http://127.0.0.1:1/latest.json".to_owned(),
            public_key: mixengine_core::updates::PUBLIC_KEY.to_owned(),
        }),
        Some(&installed.join(format!("mixengined{}", std::env::consts::EXE_SUFFIX))),
        Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
        events.clone(),
        transport.clone(),
    )
    .expect("the compiled-in updater key is a key");

    let api = Arc::new(Api {
        version: "0.1.0",
        credentials: crate::credentials::Facts {
            store: crate::credentials::Store::Home,
            choosable: true,
        },
        protocol: mixengine_proto::PROTOCOL_VERSION,
        pid: 4123,
        // The shipped default, so what these tests read is what a home reads.
        memory_over_minutes: mixengine_core::config::Services::default().memory_over_minutes,
        home: paths.root().display().to_string(),
        endpoint: "/tmp/mixengine/run/mixengined.sock".to_owned(),
        database: paths.database_file().display().to_string(),
        paths: paths.clone(),
        jobs: Arc::clone(&jobs),
        runtimes,
        php_extensions: crate::php_extensions::Extensions::new(
            &paths,
            &store,
            Arc::clone(&services),
        ),
        extensions: Arc::new(crate::extensions::Extensions::new(
            paths.clone(),
            store.clone(),
            Arc::clone(&jobs),
            Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
            mixengine_core::extensions::registry::client(
                &crate::runtimes::IndexSource::default().registry_url(),
                &crate::runtimes::IndexSource::default().public_key,
                paths.cache(),
                transport.clone(),
            )
            .expect("the compiled-in registry key is a key"),
            Arc::clone(&sites),
            Arc::clone(&services),
        )),
        packages,
        projects: crate::projects::Projects::new(&store),
        databases: crate::databases::Databases::new(
            Arc::clone(&services),
            Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
            store.clone(),
        ),
        blueprints: crate::blueprints::Blueprints::new(&store, &paths, "0.1.0"),
        sites: Arc::clone(&sites),
        doctor: crate::doctor::Doctor::new(
            &store,
            Arc::new(crate::dns::Dns::hosts_only_for_tests()),
            Arc::clone(&elevation),
            Arc::clone(&services),
            crate::domains::Domains::new(
                Arc::clone(&sites),
                &store,
                Arc::new(crate::dns::Dns::hosts_only_for_tests()),
                Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
            ),
            &paths,
            crate::crash::Reports::new(&paths, true),
        ),
        repairs: {
            let doctor = crate::doctor::Doctor::new(
                &store,
                Arc::new(crate::dns::Dns::hosts_only_for_tests()),
                Arc::clone(&elevation),
                Arc::clone(&services),
                crate::domains::Domains::new(
                    Arc::clone(&sites),
                    &store,
                    Arc::new(crate::dns::Dns::hosts_only_for_tests()),
                    Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
                ),
                &paths,
                crate::crash::Reports::new(&paths, true),
            );

            crate::repair::Repairs::new(
                doctor,
                Arc::clone(&elevation),
                Arc::clone(&services),
                &store,
                &paths,
            )
        },
        bundles: crate::diagnostics::Bundles::new(
            Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
            &paths,
            crate::crash::Reports::new(&paths, true),
        ),
        armed: Arc::clone(&armed),
        uninstall: crate::uninstall::Uninstall::new(
            &store,
            Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
            crate::uninstall::Doors {
                dns: Arc::new(crate::dns::Dns::hosts_only_for_tests()),
                services: Arc::clone(&services),
                shims: Arc::clone(&shims),
                autostart: Arc::clone(&autostart),
                elevation: Arc::clone(&elevation),
                certificates: crate::certs::Certificates::issuing(
                    &paths,
                    Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
                    store.clone(),
                ),
                armed: Arc::clone(&armed),
                updates: Arc::clone(&updates),
            },
            &paths,
        ),
        disk: crate::disk::Disk::new(&paths),
        certificates: crate::certs::Certificates::issuing(
            &paths,
            Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
            store.clone(),
        ),
        domains: crate::domains::Domains::new(
            sites,
            &store,
            Arc::new(crate::dns::Dns::hosts_only_for_tests()),
            Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
        ),
        shims,
        autostart,
        updates: Arc::clone(&updates),
        elevation,
        dns: Arc::new(crate::dns::Dns::hosts_only_for_tests()),
        // A sampler whose loop is never started: these tests answer method calls, and a snapshot
        // taken here would be this test binary's own reading rather than a home's.
        metrics: crate::metrics::sampler::Sampler::new(
            store.clone(),
            Arc::clone(&host) as Arc<dyn mixengine_platform::Host>,
            crate::metrics::watchers::Watchers::new(),
            &mixengine_core::config::Metrics::default(),
        )
        .handle(),
        store,
        services: Arc::clone(&services),
        started: super::Started::now(),
        events,
        // Cancelled by `daemon.shutdown` and by nothing else here — there is no accept loop in
        // these tests waiting on it, which is what makes the method's own test able to assert
        // that it was cancelled rather than watch a process exit.
        shutdown: super::Shutdown::new(CancellationToken::new(), SHUTDOWN_GRACE),
        front_end: tokio::sync::Mutex::new(()),
        upgrading: tokio::sync::Mutex::new(std::collections::BTreeMap::new()),
    });

    Daemon {
        _home: home,
        api,
        services,
        host,
        installed_daemon: installed.join(format!("mixengined{}", std::env::consts::EXE_SUFFIX)),
    }
}

/// A daemon that declares nothing, which is every home with no `services` rows in it.
pub(super) async fn undeclared() -> Daemon {
    daemon(Arc::new(fixture::Declared(Vec::new())), &[]).await
}

/// One call against a daemon with nothing declared, decoded.
pub(super) async fn call(body: &str) -> Value {
    undeclared().await.call(body).await
}

/// One call against a caller-supplied [`Api`], for the tests that need the same daemon twice.
pub(super) async fn answer_json(api: &Arc<Api>, body: &str) -> Value {
    let answered = answer(api, body.as_bytes())
        .await
        .expect("this call expects an answer");

    serde_json::from_slice(&answered).expect("the daemon answers JSON")
}

// `service.*` — T19a. What these exercise is the surface over T19's registry, which is why they
// are here and not there: the walk itself has its own tests next door, and what has never been
// proved before is that a request reaches it and comes back as something a client can render.

/// Two services, the second depending on the first — the shape every walk below is about.
pub(super) fn web_and_db() -> Arc<fixture::Declared> {
    Arc::new(fixture::Declared(vec![
        fixture::spec("db").build().expect("a usable spec"),
        fixture::spec("web")
            .depends_on(fixture::service("db"))
            .build()
            .expect("a usable spec"),
    ]))
}
