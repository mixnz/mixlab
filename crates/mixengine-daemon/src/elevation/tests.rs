use super::*;

/// T182b, D2. A batch holding a helper operation and others runs the helper's first, and keeps
/// the rest in the order they were queued.
#[test]
fn the_helper_operation_goes_first_in_its_batch() {
    let pending = |id: i64, op: PrivilegedOp| mixengine_proto::PendingOp {
        id: mixengine_proto::PendingOpId(id),
        description: op.describe(),
        op,
        requested_at: Timestamp(0),
    };

    let mut waiting = vec![
        pending(1, PrivilegedOp::hosts_apply(Vec::new())),
        pending(2, PrivilegedOp::AuditLogRemove {}),
        pending(3, PrivilegedOp::HelperInstall {}),
    ];

    put_the_helper_first(&mut waiting);

    let order: Vec<i64> = waiting.iter().map(|pending| pending.id.0).collect();
    assert_eq!(order, vec![3, 1, 2]);
}

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
            // **A directory this OS would install into, with nothing in it** — T85's D5, stated
            // rather than inherited. Anybody who has run the elevated suite on this machine has
            // a real one, and every assertion below is about the copy this fixture wrote beside
            // its own `mixengined`.
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

/// Put a helper at the path the fixture calls installed — roadmap task **T88d**.
///
/// **The ordinary machine, and the one every test about a *producer* means.** The fixtures
/// above describe a machine with nothing installed and a copy beside the daemon, because that
/// is the state T85's D5 needs stated rather than inherited; on such a machine every producer
/// now asks for the helper as well as for its own work, which would put a second row into
/// assertions written about the first. Calling this makes the bootstrap return early, so the
/// queue holds exactly what the producer asked for.
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

/// Wait for a job to end, so a test can assert on the row rather than on a race.
async fn finished(jobs: &Arc<crate::jobs::Jobs>, job: JobId) -> mixengine_proto::JobSummary {
    jobs.wait(job, mixengine_proto::Millis(5_000))
        .await
        .expect("the job ends")
}

/// Three rows where this build has one operation: `Probe` through the shipped path, and two
/// more written directly — the shape T41 will produce, and enough to prove a batch is a batch.
async fn three_waiting(elevation: &Arc<Elevation>) {
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    for (key, at) in [("second", 2), ("third", 3)] {
        sqlx::query(
            "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
                 VALUES ('{\"op\":\"probe\"}', ?, ?)",
        )
        .bind(key)
        .bind(at)
        .execute(elevation.store.pool())
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn a_machine_with_nothing_waiting_is_not_degraded() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    let summary = elevation.summary().await.expect("the queue is readable");

    assert_eq!(summary.pending, 0);
    assert!(summary.can_prompt);

    let status = elevation.status().await.expect("a status");
    assert!(status.pending.is_empty());
    assert!(status.helper.is_some(), "it is beside the program");
    assert!(status.reason.is_none());
    assert!(status.last.is_none(), "this daemon has granted nothing yet");
}

/// D8, from the side that publishes it: the event carries the whole queue, and an enqueue that
/// changed nothing publishes nothing at all.
#[tokio::test]
async fn enqueueing_announces_the_batch_and_a_repeat_announces_nothing() {
    let (_home, elevation, events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;
    let mut watching = events.subscribe();

    elevation
        .enqueue(&PrivilegedOp::Probe {})
        .await
        .expect("the row is written");

    let published = watching.next().await.expect("an event");
    let crate::api::events::Frame::Event(DaemonEvent::ElevationRequired { pending }) = published
    else {
        panic!("the wrong event: {published:?}")
    };
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].op, PrivilegedOp::Probe {});
    assert!(!pending[0].description.is_empty());

    elevation
        .enqueue(&PrivilegedOp::Probe {})
        .await
        .expect("a repeat is not an error");

    assert_eq!(
        elevation.summary().await.unwrap().pending,
        1,
        "one operation, however many times it was asked for"
    );
}

/// The other way out of a degraded mode, and the reason a decline is not a trap.
#[tokio::test]
async fn dropping_empties_the_queue_and_answers_with_what_is_left() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();
    let waiting = elevation.status().await.unwrap().pending;
    assert_eq!(waiting.len(), 1);

    let left = elevation
        .drop_pending(&ElevationDrop {
            op: Some(waiting[0].id),
        })
        .await
        .expect("the row goes");

    assert!(left.pending.is_empty());
    assert_eq!(elevation.summary().await.unwrap().pending, 0);
}

/// D6, and the reason `can_prompt` is not merely `probe()`: a machine with every mechanism in
/// place and no helper beside the daemon cannot grant anything either, and the sentence a person
/// needs is different in the two cases.
#[tokio::test]
async fn a_machine_that_cannot_prompt_says_which_half_is_missing() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::unable_to_elevate(
        "/tmp/mixengine",
        "no polkit agent",
    ))
    .await;

    let status = elevation.status().await.expect("a status");

    assert!(!status.can_prompt);
    assert_eq!(status.reason.as_deref(), Some("no polkit agent"));
    assert!(
        status.helper.is_some(),
        "the helper is there; it is the prompt that is not"
    );
}

/// **The task line's own test**: three operations, one grant, one prompt.
///
/// `docs/decisions/0005-on-demand-elevation.md` calls elevating inside a loop a defect, and
/// this is that rule asserted rather than asserted-about. The pair the mock records is the whole
/// claim: one prompt, on the request the daemon had just written, with the helper it resolved.
#[tokio::test]
async fn three_operations_are_one_prompt_on_the_request_that_was_just_written() {
    let (_home, elevation, _events, machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    three_waiting(&elevation).await;
    assert_eq!(elevation.summary().await.unwrap().pending, 3);

    let started = elevation.grant().await.expect("a grant becomes a job");
    finished(&elevation.jobs, started.id).await;

    let raised = machine.prompts_raised();
    assert_eq!(raised.len(), 1, "three operations, one prompt: {raised:?}");
    assert_eq!(raised[0].request.file_name().unwrap(), "request.json");
    assert!(
        raised[0]
            .helper
            .ends_with(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
        "{:?}",
        raised[0].helper
    );

    // The single-use directory is removed however the grant ended — `response.json`'s existence
    // is the anti-replay check, and a directory left behind would make the *next* grant's fresh
    // one the only thing keeping that true.
    assert!(!raised[0].request.exists());
}

/// The mock raises nothing and writes nothing, which makes "the helper ran and left no report"
/// the default here rather than a case somebody had to think to write. T40a is explicit that
/// `Completed` does not promise a file: a crash is not a per-OS event.
#[tokio::test]
async fn a_helper_that_left_no_report_fails_the_job_and_keeps_every_row() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    let started = elevation.grant().await.unwrap();
    let ended = finished(&elevation.jobs, started.id).await;

    assert_eq!(ended.state, mixengine_proto::JobState::Failed);
    assert_eq!(
        elevation.summary().await.unwrap().pending,
        1,
        "nothing was reported, so nothing may be assumed applied"
    );
}

/// T166: a helper that refused its request writes nothing beside it, and what it said on stderr
/// is the only account of why. It has to reach the job's error, where the person who has just
/// typed a password reads it — not stop at a `debug` line.
#[tokio::test]
async fn a_helper_that_left_no_report_says_why_in_the_error() {
    let said = "mixengine-elevate: cannot read /Volumes/SSD/home/run/elevate/x/request.json: \
                    Operation not permitted (os error 1)";
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::elevation_saying("/tmp/mixengine", said)).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    let started = elevation.grant().await.unwrap();
    let ended = finished(&elevation.jobs, started.id).await;

    let Some(mixengine_proto::JobOutcome::Failed { error }) = ended.outcome else {
        panic!("a grant with no report fails: {:?}", ended.outcome);
    };
    assert!(error.message.ends_with(said), "{}", error.message);
    assert!(
        error.message.contains("left no report beside"),
        "{}",
        error.message
    );
    assert_eq!(elevation.summary().await.unwrap().pending, 1);
}

/// ADR 0005: **a declined prompt is a normal outcome, never an error.** A failed job would put a
/// red line in `mix job list` for a person exercising a choice the design offers them.
#[tokio::test]
async fn a_decline_leaves_the_queue_alone_and_the_job_succeeds() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::declining_elevation("/tmp/mixengine")).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    let started = elevation.grant().await.unwrap();
    let ended = finished(&elevation.jobs, started.id).await;

    assert_eq!(ended.state, mixengine_proto::JobState::Succeeded);
    assert_eq!(elevation.summary().await.unwrap().pending, 1);

    let last = elevation
        .status()
        .await
        .unwrap()
        .last
        .expect("a last grant");
    assert_eq!(
        last.outcome,
        mixengine_proto::privileged::ElevationOutcome::Declined
    );
    assert_eq!(last.applied, 0);
    assert_eq!(last.still_pending, 1);

    // And the machine can still be asked: declined is not the same as impossible, which is the
    // distinction `probe()` exists to draw.
    assert!(elevation.summary().await.unwrap().can_prompt);
}

/// A grant that took rows out of the queue announces what is left, so a window that drew the
/// list closes it instead of offering an "Allow" the daemon will answer "nothing is waiting".
#[tokio::test]
async fn a_grant_that_applied_announces_the_queue_it_left() {
    let (_home, elevation, events, _machine) =
        registry(mock::Host::applying_elevation("/tmp/mixengine")).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();
    let mut watching = events.subscribe();

    let started = elevation.grant().await.unwrap();
    let ended = finished(&elevation.jobs, started.id).await;
    assert_eq!(ended.state, mixengine_proto::JobState::Succeeded);
    assert_eq!(elevation.summary().await.unwrap().pending, 0);

    let announced = loop {
        let published = tokio::time::timeout(std::time::Duration::from_secs(5), watching.next())
            .await
            .expect("an event within five seconds")
            .expect("the stream stays open");
        if let crate::api::events::Frame::Event(DaemonEvent::ElevationRequired { pending }) =
            published
        {
            break pending;
        }
    };
    assert!(announced.is_empty(), "{announced:?}");
}

/// On Linux the reason is the whole `pkexec` command a person is meant to type. It is worthless
/// if the daemon drops it, so it is asserted all the way through to `elevation.status`.
#[tokio::test]
async fn a_machine_that_cannot_prompt_keeps_the_reason_intact() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::unable_to_elevate(
        "/tmp/mixengine",
        "no polkit agent; run: pkexec /opt/mixengine/mixengine-elevate /…",
    ))
    .await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    let error = elevation
        .grant()
        .await
        .expect_err("there is no way to raise a prompt here");

    assert_eq!(error.code, mixengine_proto::ErrorCode::PrivilegedRequired);
    assert!(error.to_string().contains("pkexec"), "{error}");
    assert_eq!(elevation.summary().await.unwrap().pending, 1);

    let status = elevation.status().await.unwrap();
    assert_eq!(
        status.reason.as_deref(),
        Some("no polkit agent; run: pkexec /opt/mixengine/mixengine-elevate /…")
    );
}

/// D4's runtime half, asserted on the slot rather than on a race: two concurrent grants are two
/// prompts for one queue, and refusing is the only answer that cannot become a loop.
#[tokio::test]
async fn a_second_grant_while_one_is_in_flight_is_refused_and_names_the_first() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    // Put the slot where a running grant puts it, without a grant that has to be slowed down to
    // stay there. The mock answers instantly, so racing one would be a test about scheduling.
    elevation.state.lock().unwrap().slot = Slot::Running(JobId(41));

    let error = elevation
        .grant()
        .await
        .expect_err("one prompt at a time, for one queue");

    assert_eq!(error.code, mixengine_proto::ErrorCode::Conflict);
    assert!(error.to_string().contains("41"), "{error}");

    // And a slot left free is a slot a grant can take, or one failed grant would wedge the
    // daemon for as long as it runs.
    elevation.state.lock().unwrap().slot = Slot::Free;
    let started = elevation.grant().await.expect("the slot was released");
    finished(&elevation.jobs, started.id).await;
    assert_eq!(elevation.state.lock().unwrap().slot, Slot::Free);
}

/// An empty queue asks for nothing. The helper refuses an empty batch outright, so spending a
/// prompt to find that out would be a dialog raised for no reason at all.
#[tokio::test]
async fn granting_an_empty_queue_raises_nothing() {
    let (_home, elevation, _events, machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    let error = elevation.grant().await.expect_err("nothing is waiting");

    assert_eq!(error.code, mixengine_proto::ErrorCode::PreconditionFailed);
    assert!(machine.prompts_raised().is_empty());
}

/// D9 and D11's second row: no helper beside the daemon is a different sentence from no way to
/// prompt, and it is answered before anything is written.
#[tokio::test]
async fn granting_without_a_helper_beside_the_daemon_is_dependency_missing() {
    let (home, elevation, _events, machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    std::fs::remove_file(
        home.path()
            .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
    )
    .expect("the helper goes");

    let error = elevation
        .grant()
        .await
        .expect_err("there is nothing to run");

    assert_eq!(error.code, mixengine_proto::ErrorCode::DependencyMissing);
    assert!(machine.prompts_raised().is_empty());
    assert_eq!(elevation.summary().await.unwrap().pending, 1);
}

/// D11: the machine already says what the database says it should, so nothing is queued. A row
/// here would put an operation on `mix status` whose only possible outcome is `AlreadyDone`.
#[tokio::test]
async fn a_machine_that_already_agrees_is_not_asked_for_permission() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::with_hosts(
        "/tmp/mixengine",
        ["127.0.0.1 blog.test"],
    ))
    .await;
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty(),
        "nothing to do is not something to ask about"
    );
}

/// And when it disagrees, exactly one operation is waiting and one event was published.
#[tokio::test]
async fn a_machine_that_disagrees_is_asked_once() {
    let (home, elevation, events, _machine) =
        registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
    with_an_installed_helper(&home);
    let mut watching = events.subscribe();
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();
    assert_eq!(waiting.len(), 1);
    assert!(waiting[0].description.contains("blog.test"), "{waiting:?}");

    assert!(matches!(
        watching.next().await,
        Some(crate::api::events::Frame::Event(
            DaemonEvent::ElevationRequired { .. }
        ))
    ));
}

/// **The seam T44 built, in both directions** — the T44 design, D4.
///
/// A home on the hosts file asks for the entry its sites declare. A home on DNS asks for an
/// *empty* block, which is not the same as asking for nothing: the server answers the whole
/// managed TLD by pattern, so an entry adds nothing, and the operation is what clears the names
/// the other mode wrote. Skipping the queue would leave them resolving to loopback for ever.
///
/// This is the test that stops D4 being tidied into "if DNS is on, do nothing".
#[tokio::test]
async fn a_home_on_dns_asks_for_an_empty_block_rather_than_for_nothing() {
    let (home, elevation, _events, _machine) = registry_resolving(
        mock::Host::with_hosts("/tmp/mixengine", ["127.0.0.1 blog.test"]),
        crate::dns::Dns::wired_for_tests(),
    )
    .await;
    with_an_installed_helper(&home);
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(
        waiting.len(),
        1,
        "the block the machine holds is not the block a DNS home wants"
    );
    assert_eq!(
        waiting[0].op,
        PrivilegedOp::hosts_apply(Vec::new()),
        "a home on DNS wants no managed names in that file"
    );
}

/// The same home, resolving the way every machine does until T45: the entry is asked for.
#[tokio::test]
async fn a_home_on_the_hosts_file_asks_for_the_names_its_sites_declare() {
    let (home, elevation, _events, _machine) = registry_resolving(
        mock::Host::with_hosts("/tmp/mixengine", []),
        crate::dns::Dns::hosts_only_for_tests(),
    )
    .await;
    with_an_installed_helper(&home);
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(waiting.len(), 1);
    assert!(waiting[0].description.contains("blog.test"), "{waiting:?}");
}

/// D2 asserted rather than described: two sites before anybody clicks Allow are one row holding
/// the *second* state, and one event per change rather than one row per change.
#[tokio::test]
async fn two_sites_before_a_grant_are_one_row_holding_the_second_state() {
    let (home, elevation, events, _machine) =
        registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
    with_an_installed_helper(&home);
    let mut watching = events.subscribe();

    a_site_named(&elevation.store, "blog.test").await;
    elevation.require_hosts().await.unwrap();

    a_site_named(&elevation.store, "shop.test").await;
    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert!(waiting[0].description.contains("blog.test"), "{waiting:?}");
    assert!(waiting[0].description.contains("shop.test"), "{waiting:?}");

    for expected in ["blog.test", "shop.test"] {
        let published = watching.next().await.expect("an event");
        let crate::api::events::Frame::Event(DaemonEvent::ElevationRequired { pending }) =
            published
        else {
            panic!("the wrong event: {published:?}")
        };
        assert!(pending[0].description.contains(expected), "{pending:?}");
    }

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), watching.next())
            .await
            .is_err(),
        "and no third event"
    );
}

/// A read that fails is not a reason to refuse a site. The helper is the authority on what is in
/// that file, and it will say so on the screen T64 built — a better place for "your hosts file
/// has two BEGIN markers" than a site creation's error.
#[tokio::test]
async fn a_hosts_file_that_cannot_be_read_is_still_asked_about() {
    let (home, elevation, _events, _machine) = registry(mock::Host::unable_to_read_the_hosts_file(
        "/tmp/mixengine",
        "two BEGIN markers",
    ))
    .await;
    with_an_installed_helper(&home);
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    assert_eq!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .len(),
        1
    );
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

/// D7: a machine that needs a grant and has not got one leaves exactly one row in the queue and
/// announces it once.
#[tokio::test]
async fn a_front_end_on_a_machine_with_no_grant_asks_for_one() {
    let (home, elevation, events, _machine) = registry(mock::Host::without_port_access(
        "/tmp/mixengine",
        mixengine_platform::PortAccessMethod::Capability,
        "the binary holds no capability",
    ))
    .await;
    with_an_installed_helper(&home);
    let mut watching = events.subscribe();

    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy")))
        .await
        .unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].op.name(), "port-access-grant");
    assert!(matches!(
        watching.next().await,
        Some(crate::api::events::Frame::Event(
            DaemonEvent::ElevationRequired { .. }
        ))
    ));

    // A second start adds no second row: the dedupe key is the kind, and the state has not
    // changed — the T41 design, D2, which this operation reuses unchanged.
    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy")))
        .await
        .unwrap();

    assert_eq!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .len(),
        1
    );
}

/// A machine that already allows it spends no prompt, which is the whole reason the disk is read
/// before the queue is written.
#[tokio::test]
async fn a_machine_that_already_allows_it_asks_for_nothing() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::with_port_access(
        "/tmp/mixengine",
        mixengine_platform::PortAccessMethod::Capability,
    ))
    .await;

    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy")))
        .await
        .unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// A home with no front end has no binary to name, and on Linux the question cannot even be
/// asked without one — D12's reason for the producer being one-directional.
#[tokio::test]
async fn a_home_with_no_front_end_asks_for_nothing() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::without_port_access(
        "/tmp/mixengine",
        mixengine_platform::PortAccessMethod::Capability,
        "the binary holds no capability",
    ))
    .await;

    elevation.require_port_access(None).await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// T85's D6, the row that queues: a helper shipped beside the daemon and none installed.
///
/// The fixture's `Candidates::installed` is `None`, so this states the machine rather than
/// inheriting whichever one the tests are running on.
#[tokio::test]
async fn a_machine_with_no_installed_helper_is_asked_to_have_one() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    elevation
        .require_helper()
        .await
        .expect("the row is written");

    let pending = mixengine_core::elevation::pending(&elevation.store)
        .await
        .expect("the queue reads back");

    assert_eq!(
        pending
            .iter()
            .filter(|op| op.op == PrivilegedOp::HelperInstall {})
            .count(),
        1,
        "{pending:?}"
    );
}

/// And asking twice is still one row.
///
/// Worth its own test because this is the first operation whose dedupe key is a bare name with
/// no data behind it: `Probe`'s key is its serialisation, and every other operation's is derived
/// from a plan. A start that asked for this on every boot and got a row each time would put a
/// growing list on the one screen whose job is to say what a prompt will change.
#[tokio::test]
async fn asking_twice_for_the_helper_queues_one_operation() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    elevation.require_helper().await.expect("once");
    elevation.require_helper().await.expect("twice");

    let pending = mixengine_core::elevation::pending(&elevation.store)
        .await
        .expect("the queue reads back");

    assert_eq!(
        pending
            .iter()
            .filter(|op| op.op == PrivilegedOp::HelperInstall {})
            .count(),
        1,
        "{pending:?}"
    );
}

/// T88d. A machine whose helper an uninstall removed still ships a source, so the next thing
/// that needs root puts the installation in front of itself — without the daemon restart
/// `require_helper` alone would have needed.
#[tokio::test]
async fn a_producer_on_a_machine_with_no_installed_helper_asks_for_one() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(
        waiting
            .iter()
            .filter(|row| row.op == PrivilegedOp::HelperInstall {})
            .count(),
        1,
        "{waiting:?}"
    );
}

/// **And a machine that already agrees is still asked for nothing.** The bootstrap sits at the
/// enqueue rather than at the top of the producer, so a site creation that needed no hosts
/// entry raises no prompt — which is the promise `docs/architecture/security-model.md` makes
/// about creating a site, and the reason this row is here beside the one above.
#[tokio::test]
async fn a_producer_that_asks_for_nothing_asks_for_no_helper_either() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::with_hosts(
        "/tmp/mixengine",
        ["127.0.0.1 blog.test"],
    ))
    .await;
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty(),
        "nothing to do is not something to ask about"
    );
}

/// Nothing installed **and** nothing to install from: a row that could never be applied is
/// worse than the refusal `elevation.grant` already gives, so none is written.
#[tokio::test]
async fn a_producer_with_no_helper_anywhere_asks_for_no_install() {
    let (home, elevation, _events, _machine) =
        registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
    a_site_named(&elevation.store, "blog.test").await;

    std::fs::remove_file(
        home.path()
            .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
    )
    .expect("the helper goes");

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert!(
        !waiting
            .iter()
            .any(|row| row.op == PrivilegedOp::HelperInstall {}),
        "{waiting:?}"
    );
}

/// T88d, and the other half of the rule above: a helper installation already waiting is
/// dropped before an uninstall asks for the removal, so the batch does not install the file it
/// is about to remove.
///
/// **Here rather than in `crate::uninstall`'s own test module**, which has no fixture holding a
/// queue and a machine — this one does, and building a second would be two answers to what a
/// pending operation is.
#[tokio::test]
async fn an_uninstall_drops_a_helper_installation_that_is_already_waiting() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    elevation
        .require_helper()
        .await
        .expect("the installation is asked for");

    crate::uninstall::drop_helper_installations(&elevation)
        .await
        .expect("the row goes");

    elevation
        .enqueue(&PrivilegedOp::HelperRemove {})
        .await
        .expect("the removal is asked for");

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0].op, PrivilegedOp::HelperRemove {});
}

/// **The uninstall's own door is untouched by T88d's rule.** `crate::uninstall` enqueues
/// directly, so a batch that removes the helper never first installs one.
#[tokio::test]
async fn enqueueing_directly_asks_for_no_helper() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    elevation
        .enqueue(&PrivilegedOp::HelperRemove {})
        .await
        .expect("the row is written");

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0].op, PrivilegedOp::HelperRemove {});
}

/// D6's first row: a daemon with no helper beside it has nothing to install, so it asks for
/// nothing. Distinct from "it is already installed", and the reason it is a row of its own is
/// that this is the state a moved binary is in — and `elevation.grant` already tells a person
/// about *that* where they can act on it.
#[tokio::test]
async fn a_daemon_with_no_helper_beside_it_asks_for_no_install() {
    let (home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    std::fs::remove_file(
        home.path()
            .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
    )
    .expect("the helper goes");

    elevation.require_helper().await.expect("nothing is asked");

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// A probe that fails is not a reason to ask for a prompt, and not a reason to fail a start
/// either. It is logged, and the machine carries on — the opposite of `require_hosts`, and
/// deliberately: there the helper is the authority on the file's contents and can refuse with a
/// reason on the screen; here a probe that could not read one attribute tells us nothing about
/// what to ask for.
#[tokio::test]
async fn a_probe_that_fails_asks_for_nothing_and_does_not_fail() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::unable_to_probe_port_access(
        "/tmp/mixengine",
        "this filesystem carries no extended attributes",
    ))
    .await;

    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy")))
        .await
        .expect("a probe that failed is not an error to the caller");

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// Windows: the method says nothing is needed, so nothing is asked for even though the front end
/// answers on 80. No `#[cfg]` anywhere above this crate is what makes that true.
#[tokio::test]
async fn a_machine_that_reserves_no_ports_asks_for_nothing() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::with_port_access(
        "/tmp/mixengine",
        mixengine_platform::PortAccessMethod::Direct,
    ))
    .await;

    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy.exe")))
        .await
        .unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// The probe after a grant is spent on the two operations that write the helper's file, and on
/// nothing else — least of all on an uninstall, whose last operation removes the audit log the
/// probe would put back on a machine whose token is already elevated.
#[test]
fn only_a_batch_that_installed_or_replaced_the_helper_is_worth_a_probe() {
    let batch = |ops: &[PrivilegedOp]| -> Vec<mixengine_proto::PendingOp> {
        ops.iter()
            .enumerate()
            .map(|(index, op)| mixengine_proto::PendingOp {
                id: mixengine_proto::PendingOpId(index as i64),
                description: op.describe(),
                op: op.clone(),
                requested_at: mixengine_proto::Timestamp(0),
            })
            .collect()
    };

    assert!(may_have_changed_the_helper(&batch(&[
        PrivilegedOp::hosts_apply(vec![]),
        PrivilegedOp::HelperInstall {},
    ])));
    assert!(may_have_changed_the_helper(&batch(&[
        PrivilegedOp::HelperReplace {}
    ])));

    assert!(!may_have_changed_the_helper(&batch(&[])));
    assert!(!may_have_changed_the_helper(&batch(&[
        PrivilegedOp::hosts_apply(vec![]),
    ])));
    assert!(!may_have_changed_the_helper(&batch(&[
        PrivilegedOp::HelperRemove {},
        PrivilegedOp::AuditLogRemove {},
    ])));
}

/// **T179.** A site is created, deleted before anybody clicks Allow, and the row it queued goes
/// with it: the queue never describes a home that no longer declares the name, and a grant
/// cannot write one into the machine's hosts file.
#[tokio::test]
async fn a_hosts_entry_nobody_declares_any_more_leaves_the_queue() {
    let (home, elevation, events, _machine) =
        registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
    with_an_installed_helper(&home);
    let mut watching = events.subscribe();

    a_site_named(&elevation.store, "gone.local").await;
    elevation.require_hosts().await.unwrap();
    assert_eq!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        watching.next().await,
        Some(crate::api::events::Frame::Event(
            DaemonEvent::ElevationRequired { .. }
        ))
    ));

    sqlx::query("DELETE FROM sites")
        .execute(elevation.store.pool())
        .await
        .unwrap();
    elevation.require_hosts().await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty(),
        "the queue still holds a name this home no longer declares"
    );

    let Some(crate::api::events::Frame::Event(DaemonEvent::ElevationRequired { pending })) =
        watching.next().await
    else {
        panic!("a withdrawal publishes the queue that is left")
    };
    assert!(pending.is_empty(), "{pending:?}");
}

/// A machine this daemon could not read is a machine it knows nothing about, so the queue is
/// left exactly as it was (the T179 design, D2).
#[tokio::test]
async fn a_hosts_file_that_cannot_be_read_withdraws_nothing() {
    let (home, elevation, _events, _machine) = registry(mock::Host::unable_to_read_the_hosts_file(
        "/tmp/mixengine",
        "the file is not readable",
    ))
    .await;
    with_an_installed_helper(&home);

    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'hosts-apply', 1)",
    )
    .execute(elevation.store.pool())
    .await
    .unwrap();

    elevation.require_hosts().await.unwrap();

    assert_eq!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .len(),
        1,
        "a failed reading withdrew a row"
    );
}

/// **T179**, for the resolver: a machine that already routes what it should has nothing waiting
/// for it, whatever an earlier reading queued.
#[tokio::test]
async fn a_resolver_that_already_routes_withdraws_what_was_queued() {
    let (home, elevation, _events, _machine) = registry_resolving(
        mock::Host::with_resolver(
            "/tmp/mixengine",
            mixengine_platform::ResolverMethod::Nrpt,
            &mixengine_proto::domains::WIRED_TLDS,
        ),
        crate::dns::Dns::wired_for_tests(),
    )
    .await;
    with_an_installed_helper(&home);

    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'resolver', 1)",
    )
    .execute(elevation.store.pool())
    .await
    .unwrap();

    elevation.require_resolver().await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty(),
        "the resolver row outlived the machine agreeing"
    );
}

/// **T179**, for the trust store: a home with no authority has nothing to be trusted, so an
/// install queued for an authority that is gone goes with it (the design's Settled 1).
#[tokio::test]
async fn a_home_with_no_authority_withdraws_a_queued_trust_install() {
    let (home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;
    with_an_installed_helper(&home);

    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'trust-store', 1)",
    )
    .execute(elevation.store.pool())
    .await
    .unwrap();

    elevation.require_trust_store(None).await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// **T179**, for the port grant: a binary that already holds what it needs withdraws the grant
/// nobody answered. **Withdrawing is not revoking** — the machine keeps the capability.
#[tokio::test]
async fn a_binary_that_already_answers_withdraws_the_grant_nobody_answered() {
    let (home, elevation, _events, _machine) = registry(mock::Host::with_port_access(
        "/tmp/mixengine",
        mixengine_platform::PortAccessMethod::Capability,
    ))
    .await;
    with_an_installed_helper(&home);

    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'port-access', 1)",
    )
    .execute(elevation.store.pool())
    .await
    .unwrap();

    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy")))
        .await
        .unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// A probe that could not read the machine says nothing about what to stop asking for, so a
/// home with no front end leaves the queue alone (the T179 design, D2).
#[tokio::test]
async fn a_home_with_no_front_end_withdraws_nothing() {
    let (home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;
    with_an_installed_helper(&home);

    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'port-access', 1)",
    )
    .execute(elevation.store.pool())
    .await
    .unwrap();

    elevation.require_port_access(None).await.unwrap();

    assert_eq!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .len(),
        1,
        "a home that cannot read the machine withdrew a row anyway"
    );
}

/// **T180.** A grant that carried a firewall plan records it, so the producer can tell a plan
/// this machine already holds from one it does not.
#[tokio::test]
async fn a_settled_firewall_plan_is_recorded() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    let plan = mixengine_proto::privileged::FirewallPlan {
        ports: vec![80, 443],
        label: "MixEngine — shared sites".to_owned(),
    };
    let waiting = vec![mixengine_proto::PendingOp {
        id: mixengine_proto::PendingOpId(7),
        op: PrivilegedOp::FirewallApply { plan: plan.clone() },
        description: "open two ports".to_owned(),
        requested_at: mixengine_proto::Timestamp(1),
    }];

    for outcome in [
        OpOutcome::Applied {
            detail: "two rules".to_owned(),
        },
        OpOutcome::AlreadyDone,
        OpOutcome::Unmanaged {
            reason: "this machine has no firewall to write".to_owned(),
            manual: "open the port yourself".to_owned(),
        },
    ] {
        mixengine_core::updates::records::clear(&elevation.store, FIREWALL_APPLIED)
            .await
            .unwrap();

        elevation
            .remember_the_firewall(&waiting, &[(mixengine_proto::PendingOpId(7), outcome)])
            .await;

        let recorded: Option<mixengine_proto::privileged::FirewallPlan> =
            mixengine_core::updates::records::get(&elevation.store, FIREWALL_APPLIED)
                .await
                .unwrap();
        assert_eq!(recorded.as_ref(), Some(&plan));
    }
}

/// An operation that was refused, unsupported or failed changed nothing, so it records nothing:
/// the next sharing change has to ask again.
#[tokio::test]
async fn a_firewall_plan_that_did_not_land_records_nothing() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    let plan = mixengine_proto::privileged::FirewallPlan {
        ports: vec![80],
        label: "MixEngine — shared sites".to_owned(),
    };
    let waiting = vec![mixengine_proto::PendingOp {
        id: mixengine_proto::PendingOpId(3),
        op: PrivilegedOp::FirewallApply { plan },
        description: "open one port".to_owned(),
        requested_at: mixengine_proto::Timestamp(1),
    }];

    for outcome in [
        OpOutcome::Refused {
            reason: "no".to_owned(),
        },
        OpOutcome::Unsupported {
            reason: "not here".to_owned(),
        },
        OpOutcome::Failed {
            message: "the tool said no".to_owned(),
        },
    ] {
        elevation
            .remember_the_firewall(
                &waiting,
                &[(mixengine_proto::PendingOpId(3), outcome.clone())],
            )
            .await;

        let recorded: Option<mixengine_proto::privileged::FirewallPlan> =
            mixengine_core::updates::records::get(&elevation.store, FIREWALL_APPLIED)
                .await
                .unwrap();
        assert_eq!(recorded, None, "{outcome:?} recorded a plan");
    }
}
