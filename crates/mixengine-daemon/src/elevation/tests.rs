use super::*;

mod asking;
mod by_hand;
mod queue;
mod withdrawal;

use mixengine_platform::mock;

/// A registry over a temporary home, with a machine that accepts every prompt.
///
/// The helper is a **file that exists** and is never run: everything in this module stops at
/// `mock::Host`, which records the prompt and raises nothing.
async fn registry(
    machine: mock::Host,
) -> (tempfile::TempDir, Arc<Elevation>, Events, Arc<mock::Host>) {
    registry_resolving(machine, crate::dns::Dns::hosts_only_for_tests()).await
}

/// The same, for the two tests that care which way this home resolves a name.
async fn registry_resolving(
    machine: mock::Host,
    dns: crate::dns::Dns,
) -> (tempfile::TempDir, Arc<Elevation>, Events, Arc<mock::Host>) {
    let home = tempfile::tempdir().expect("a temporary home");
    let paths = Paths::new(
        home.path().to_path_buf(),
        &mixengine_core::config::PathOverrides::default(),
    );
    std::fs::create_dir_all(paths.run()).expect("the run directory");

    let program = home
        .path()
        .join(format!("mixengined{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&program, b"not run").expect("a program to be found beside");
    std::fs::write(
        home.path()
            .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
        b"not run",
    )
    .expect("a helper to be found beside it");

    let store = Store::open(&home.path().join("mixengine.db"))
        .await
        .expect("a fresh database migrates");
    let events = Events::new();
    let jobs = Arc::new(crate::jobs::Jobs::new(
        &store,
        events.clone(),
        tokio_util::sync::CancellationToken::new(),
    ));

    let machine = Arc::new(machine);
    let elevation = Elevation::new(
        &paths,
        &store,
        events.clone(),
        jobs,
        Arc::clone(&machine) as Arc<dyn Host>,
        Candidates {
            program,
            // **A directory this OS would install into, with nothing in it**, stated rather than
            // inherited. Anybody who has run the elevated suite on this machine has a real one, and
            // every assertion below is about the copy this fixture wrote beside its own
            // `mixengined`.
            //
            // `Some(a path that is not there)` and not `None`: those are two different machines.
            // `None` is one that will not name a directory at all, where nothing can be
            // installed and `require_helper` therefore asks for nothing.
            installed: Some(
                home.path()
                    .join("system")
                    .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
            ),
        },
        Arc::new(dns),
    );

    (home, elevation, events, machine)
}

/// Put a helper at the path the fixture calls installed.
///
/// **The ordinary machine, and the one every test about a *producer* means.** The fixtures above
/// describe a machine with nothing installed and a copy beside the daemon, because that is the
/// state the "nothing installed" case needs stated rather than inherited; on such a machine every
/// producer now asks for the helper as well as for its own work, which would put a second row into
/// assertions written about the first. Calling this makes the bootstrap return early, so the queue
/// holds exactly what the producer asked for.
///
/// Nothing runs the file and nothing reads who owns it: `helper` asks that only when somebody
/// wants a path to hand a prompt, and no test that calls this does.
fn with_an_installed_helper(home: &tempfile::TempDir) {
    let directory = home.path().join("system");
    std::fs::create_dir_all(&directory).expect("the directory this OS would install into");
    std::fs::write(
        directory.join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
        b"not run",
    )
    .expect("an installed helper");
}

/// A project and a site holding one domain, written straight into the store.
///
/// What this suite is about is the producer's decision; `Sites` has its own tests, and going
/// through the API here would put two things under one assertion.
async fn a_site_named(store: &Store, domain: &str) {
    // One project per call, because a root is unique and two sites under one root would be a
    // second thing this fixture had to decide.
    let root = std::env::temp_dir().join(format!("mixengine-t41-{domain}"));

    let project = mixengine_core::projects::create(
        store,
        &mixengine_core::projects::Registration {
            name: domain.replace('.', "-"),
            root,
            pins: std::collections::BTreeMap::new(),
        },
        Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
    )
    .await
    .expect("a project");

    mixengine_core::sites::create(
        store,
        &mixengine_core::sites::NewSite {
            owner: mixengine_core::sites::SiteOwner::Project(project.id),
            doc_root: String::new(),
            kind: mixengine_proto::SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec![domain.to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");
}
