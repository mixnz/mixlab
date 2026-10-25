use mixengine_proto::{Backoff, DaemonEvent, Millis, ReadyCheck, RestartPolicy};
use mixengine_testkit::FakeService;

use super::super::fixture::{
    Declared, EVENTUALLY, SILENCE, arguments, home, registry, registry_on, row, service, spec,
};
use super::*;

/// A service that dies before it is ever ready, under a policy that never gives up.
///
/// The backoff is far longer than either test that uses this needs, deliberately: what both
/// assert about is the gap between two attempts, and a short wait would race them into asserting
/// against a service that had moved on to the next one.
fn crash_looping(id: &str) -> ServiceSpec {
    let broken = FakeService::new().never_ready().exit_after(50).exit_code(3);

    spec(id)
        .args(arguments(&broken))
        .restart(RestartPolicy::Always {
            backoff: Backoff {
                initial: Millis::from_secs(30),
                max: Millis::from_secs(30),
                ..Backoff::default()
            },
        })
        .build()
        .expect("a usable spec")
}

/// A service that comes up, stays up for a moment and then dies, under the same never-give-up
/// policy and the same long backoff.
///
/// **The slow ready is the point.** Between the registration and `Running` lies the one window in
/// which [`Registry::begin`] can ask a runner that is not in a backoff, and what a test needs to
/// be able to do is land a second walk inside it. A second is far longer than the two database
/// writes a walk takes to get there, and the exit is late enough to be unambiguously after it.
fn crashes_after_coming_up(id: &str) -> ServiceSpec {
    let brittle = FakeService::new()
        .ready_after(1_000)
        .exit_after(3_000)
        .exit_code(3);

    spec(id)
        .args(arguments(&brittle))
        .restart(RestartPolicy::Always {
            backoff: Backoff {
                initial: Millis::from_secs(30),
                max: Millis::from_secs(30),
                ..Backoff::default()
            },
        })
        .build()
        .expect("a usable spec")
}

/// A provisioning statement goes to a *running* server, so the method that provisions starts one
/// — and starting one that is already up is not a failure to report to anybody.
///
/// What this is really asserting is the second call: `ensure_running` is reached on every
/// `database.create`, including the ones on a home where everything is already up, and a version of
/// it that answered "already started" as an error would make the ordinary case the failing one.
#[tokio::test]
async fn ensuring_a_service_is_running_is_idempotent() {
    let (_home, paths, store) = home(&["caddy"]).await;
    let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
    let registry = registry(&paths, &store, Arc::new(declared));

    registry
        .ensure_running(&service("caddy"))
        .await
        .expect("it comes up");
    registry
        .ensure_running(&service("caddy"))
        .await
        .expect("and it is still up");

    let (state, _) = row(&store, &service("caddy")).await;
    assert_eq!(state, ServiceState::Running);

    let graph = registry.graph().await.expect("one declared service");
    let stopping = graph.stop_plan([&service("caddy")]).expect("a plan");
    registry.stop(&stopping).await;
}

/// A service this home does not declare is a refusal rather than a start.
#[tokio::test]
async fn ensuring_a_service_nothing_declares_is_refused() {
    let (_home, paths, store) = home(&["caddy"]).await;
    let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
    let registry = registry(&paths, &store, Arc::new(declared));

    registry
        .ensure_running(&service("mariadb"))
        .await
        .expect_err("nothing declares it");
}

#[tokio::test]
async fn a_service_starts_runs_and_stops() {
    let (_home, paths, store) = home(&["caddy"]).await;
    let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
    let registry = registry(&paths, &store, Arc::new(declared));

    let graph = registry.graph().await.expect("one declared service");
    let plan = graph.start_plan([&service("caddy")]).expect("a plan");

    let walk = registry.start(&graph, &plan).await;

    assert_eq!(walk.reached, vec![service("caddy")], "{walk:?}");
    assert!(walk.failed.is_none(), "{walk:?}");

    let (state, pid) = row(&store, &service("caddy")).await;
    assert_eq!(state, ServiceState::Running);
    assert!(pid.is_some(), "the pid of a running service is recorded");

    // The log file is the supervisor's, and this is the one assertion that it is being written
    // for a service the *registry* started rather than one a supervisor test spawned.
    let log = paths
        .service_logs(&service("caddy"))
        .join(mixengine_supervisor::logs::CURRENT_LOG_FILE_NAME);
    assert!(log.is_file(), "{} was not written", log.display());

    let stopping = graph.stop_plan([&service("caddy")]).expect("a plan");
    let stopped = registry.stop(&stopping).await;

    assert_eq!(stopped.reached, vec![service("caddy")], "{stopped:?}");
    assert!(stopped.failed.is_none(), "{stopped:?}");

    let (state, pid) = row(&store, &service("caddy")).await;
    assert_eq!(state, ServiceState::Stopped);
    assert_eq!(pid, None, "a stopped service names no process");
}

/// The detail the runner persisted with the reason reaches the walk, so the sentence a caller
/// prints first is the runner's. A spec that names a credential nothing holds is the failure that
/// found this.
#[tokio::test]
async fn a_walk_carries_the_detail_the_runner_wrote_with_the_reason() {
    let (_home, paths, store) = home(&["mariadb"]).await;
    let declared = Declared(vec![
        spec("mariadb")
            .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb/root")
            .build()
            .expect("a usable spec"),
    ]);
    let registry = registry(&paths, &store, Arc::new(declared));

    let graph = registry.graph().await.expect("one declared service");
    let plan = graph.start_plan([&service("mariadb")]).expect("a plan");
    let walk = registry.start(&graph, &plan).await;

    let (failed, fault) = walk.failed.expect("the start failed");
    assert_eq!(failed, service("mariadb"));
    assert_eq!(fault.reason, Some(StateReason::SpawnFailed));
    assert!(
        fault.detail.as_deref().is_some_and(|detail| {
            detail.contains("no credential is stored at mixengine/mariadb/root")
        }),
        "{fault:?}"
    );
}

/// `ensure_running` says the detail first, and the reason when there is no detail.
#[tokio::test]
async fn ensure_running_says_the_runners_detail_before_the_state_machines_reason() {
    let (_home, paths, store) = home(&["mariadb"]).await;
    let declared = Declared(vec![
        spec("mariadb")
            .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb/root")
            .build()
            .expect("a usable spec"),
    ]);
    let registry = registry(&paths, &store, Arc::new(declared));

    let error = registry
        .ensure_running(&service("mariadb"))
        .await
        .expect_err("it cannot start");

    assert!(
        error
            .message
            .starts_with("mariadb did not start: the environment entry"),
        "{}",
        error.message
    );
    assert!(
        !error.message.contains("could not be started at all"),
        "the state machine's sentence is not the first one any more: {}",
        error.message
    );
}

#[tokio::test]
async fn a_service_that_never_becomes_ready_fails_rather_than_waiting_forever() {
    let (_home, paths, store) = home(&["slow"]).await;

    let fake = FakeService::new().never_ready();
    let declared = Declared(vec![
        spec("slow")
            .args(arguments(&fake))
            .ready(ReadyCheck::LogPattern {
                regex: mixengine_testkit::service::READY_LINE.to_owned(),
                timeout: Millis(750),
            })
            .build()
            .expect("a usable spec"),
    ]);
    let registry = registry(&paths, &store, Arc::new(declared));

    let graph = registry.graph().await.expect("one declared service");
    let plan = graph.start_plan([&service("slow")]).expect("a plan");

    let walk = registry.start(&graph, &plan).await;

    assert!(
        matches!(
            &walk.failed,
            Some((id, Fault { reason: Some(StateReason::ReadyTimeout { after }), .. }))
                if id == &service("slow") && *after == Millis(750)
        ),
        "a ready timeout says how long it waited: {walk:?}"
    );

    let (state, pid) = row(&store, &service("slow")).await;
    assert_eq!(state, ServiceState::Failed);
    assert_eq!(
        pid, None,
        "a service that was killed for never becoming ready names no process"
    );
}

/// The failure a user can do something about, in place of the one they cannot.
///
/// The same shape as the test above — a service that never becomes ready — with one difference:
/// the machine says a program MixEngine does not manage is on the port this service declared.
/// "not ready within 750ms" is true of both and sends the reader to the service's own log,
/// where a fixture that never announces itself has nothing to say.
#[tokio::test]
async fn a_service_whose_port_is_held_by_another_program_says_which_one() {
    let (_home, paths, store) = home(&["slow"]).await;

    let fake = FakeService::new().never_ready();
    let declared = Declared(vec![
        spec("slow")
            .args(arguments(&fake))
            .ports([3306])
            .ready(ReadyCheck::LogPattern {
                regex: mixengine_testkit::service::READY_LINE.to_owned(),
                timeout: Millis(750),
            })
            .build()
            .expect("a usable spec"),
    ]);
    let squatter = mixengine_platform::mock::Host::with_a_port_held(
        paths.root(),
        3306,
        mixengine_platform::PortHolder {
            pid: Some(4242),
            name: Some("mysqld.exe".to_owned()),
        },
    );
    let registry = registry_on(&paths, &store, Arc::new(declared), Arc::new(squatter));

    let graph = registry.graph().await.expect("one declared service");
    let plan = graph.start_plan([&service("slow")]).expect("a plan");

    let walk = registry.start(&graph, &plan).await;

    assert!(
        matches!(
            &walk.failed,
            Some((id, Fault { reason: Some(StateReason::PortInUse { port, program: Some(program), .. }), .. }))
                if id == &service("slow") && *port == 3306 && program == "mysqld.exe"
        ),
        "the reason names the program on the port, not the symptom: {walk:?}"
    );

    let (state, _) = row(&store, &service("slow")).await;
    assert_eq!(state, ServiceState::Failed);
}

/// The invariant behind the check-and-register being one step: whatever else two starts do,
/// there is never a second process for a service that already has one.
#[tokio::test]
async fn starting_a_service_that_is_already_up_does_not_spawn_a_second_one() {
    let (_home, paths, store) = home(&["caddy"]).await;
    let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
    let registry = registry(&paths, &store, Arc::new(declared));

    let graph = registry.graph().await.expect("one declared service");
    let plan = graph.start_plan([&service("caddy")]).expect("a plan");

    registry.start(&graph, &plan).await;
    let (_, first) = row(&store, &service("caddy")).await;
    assert!(first.is_some(), "the first start recorded a process");

    let again = registry.start(&graph, &plan).await;

    assert_eq!(again.reached, vec![service("caddy")], "{again:?}");
    assert!(again.failed.is_none(), "{again:?}");
    assert_eq!(
        row(&store, &service("caddy")).await.1,
        first,
        "the process being supervised is still the one the first start spawned"
    );
    assert_eq!(
        lock(&registry.running).len(),
        1,
        "a second runner would be one the registry can no longer name"
    );

    let stopping = graph.stop_plan([&service("caddy")]).expect("a plan");
    registry.stop(&stopping).await;
}

#[tokio::test]
async fn what_depends_on_a_failure_is_never_spawned() {
    let (_home, paths, store) = home(&["db", "web"]).await;

    // Dies without ever announcing itself. **`never_ready` is not decoration**: a service that
    // printed its ready line and then died would be racing the ready check against the exit,
    // which is a real behaviour `ready::wait` biases towards the exit for — but it is not what
    // this test is about, and a test whose subject is decided by a race is a test that passes
    // most of the time.
    let broken = FakeService::new().never_ready().exit_after(50).exit_code(3);

    let declared = Declared(vec![
        spec("db")
            .args(arguments(&broken))
            .build()
            .expect("a usable spec"),
        spec("web")
            .depends_on(service("db"))
            .build()
            .expect("a usable spec"),
    ]);
    let registry = registry(&paths, &store, Arc::new(declared));

    let graph = registry.graph().await.expect("two declared services");
    let plan = graph.start_plan([&service("web")]).expect("a plan");

    let walk = registry.start(&graph, &plan).await;

    assert!(
        matches!(&walk.failed, Some((id, _)) if id == &service("db")),
        "the walk stops at the service that failed: {walk:?}"
    );
    assert_eq!(walk.blocked, vec![service("web")], "{walk:?}");

    assert_eq!(row(&store, &service("db")).await.0, ServiceState::Failed);
    assert_eq!(row(&store, &service("web")).await.0, ServiceState::Failed);
}

/// A walk waits for the attempt in flight and **not** for the policy to give up, because a policy
/// is allowed never to: `RestartPolicy::Always` has no ceiling, so nothing about a service under
/// it will ever reach `Failed`, and a walk that waited for that would never come back at all.
#[tokio::test]
async fn a_service_whose_policy_never_gives_up_does_not_hold_the_walk_for_ever() {
    let (_home, paths, store) = home(&["db"]).await;
    let registry = registry(
        &paths,
        &store,
        Arc::new(Declared(vec![crash_looping("db")])),
    );

    let graph = registry.graph().await.expect("one declared service");
    let plan = graph.start_plan([&service("db")]).expect("a plan");

    let walk = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
        .await
        .expect("the walk was answered after the first attempt");

    assert!(
        matches!(
            &walk.failed,
            Some((id, Fault { reason: Some(StateReason::Exited { code: Some(3) }), .. }))
                if id == &service("db")
        ),
        "the walk says how the attempt ended rather than that the policy ran out: {walk:?}"
    );
    assert_eq!(
        row(&store, &service("db")).await.0,
        ServiceState::Restarting,
        "the runner is still putting it back, which is what its policy asks for"
    );

    registry.shutdown.cancel();
    tokio::time::timeout(EVENTUALLY, registry.shut_down())
        .await
        .expect("every runner finished");
}

/// The other half of that rule, and the half the default policy lives on: a ceiling is something
/// a walk *can* wait for, so it does. `OnFailure`'s first crash is a transient the runner
/// recovers from a backoff later, and a walk that took it for an answer would leave the tier
/// below `Failed` and unsupervised beside a service that went on to come up by itself.
#[tokio::test]
async fn a_walk_waits_out_the_retries_a_bounded_policy_is_allowed() {
    let (_home, paths, store) = home(&["db", "web"]).await;
    let broken = FakeService::new().never_ready().exit_after(50).exit_code(3);

    let declared = Declared(vec![
        spec("db")
            .args(arguments(&broken))
            // One retry, and a backoff short enough that what the test spends its time on is
            // the wait being *taken* rather than the wait itself.
            .restart(RestartPolicy::OnFailure {
                max_retries: 1,
                window: Millis::from_secs(300),
                backoff: Backoff {
                    initial: Millis(50),
                    max: Millis(50),
                    ..Backoff::default()
                },
            })
            .build()
            .expect("a usable spec"),
        spec("web")
            .depends_on(service("db"))
            .build()
            .expect("a usable spec"),
    ]);
    let registry = registry(&paths, &store, Arc::new(declared));

    let graph = registry.graph().await.expect("two declared services");
    let plan = graph.start_plan([&service("web")]).expect("a plan");

    let walk = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
        .await
        .expect("the walk was answered once the policy ran out");

    // Two attempts and the crash loop that ended them, not the first `Exited`: that reason here
    // would be a walk that gave up while the runner was still going to try again.
    assert!(
        matches!(
            &walk.failed,
            Some((id, Fault { reason: Some(StateReason::CrashLoop { attempts: 2, .. }), .. }))
                if id == &service("db")
        ),
        "the walk is answered by the policy running out, not by one crash: {walk:?}"
    );
    assert_eq!(walk.blocked, vec![service("web")], "{walk:?}");
    assert_eq!(row(&store, &service("db")).await.0, ServiceState::Failed);
    assert_eq!(
        row(&store, &service("web")).await.1,
        None,
        "`web` was never spawned"
    );
}

/// **The invariant behind reading readiness rather than a task's liveness.** A runner stays alive
/// for as long as it keeps putting a service back, and a service in the gap between two attempts
/// is not somewhere a dependent can be started against.
#[tokio::test]
async fn a_service_in_a_restart_backoff_is_not_reported_as_up() {
    let (_home, paths, store) = home(&["db", "web"]).await;
    let declared = Declared(vec![
        crash_looping("db"),
        spec("web")
            .depends_on(service("db"))
            .build()
            .expect("a usable spec"),
    ]);
    let registry = registry(&paths, &store, Arc::new(declared));

    let graph = registry.graph().await.expect("two declared services");
    let plan = graph.start_plan([&service("web")]).expect("a plan");

    let first = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
        .await
        .expect("the first walk was answered");

    assert_eq!(first.blocked, vec![service("web")], "{first:?}");

    // What the second walk arrives to: a runner that is alive, and a service that is not up.
    assert_eq!(
        row(&store, &service("db")).await.0,
        ServiceState::Restarting
    );

    let again = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
        .await
        .expect("the second walk was answered");

    assert!(
        matches!(&again.failed, Some((id, _)) if id == &service("db")),
        "a supervised service that is not up has to stop the walk: {again:?}"
    );
    assert!(
        again.reached.is_empty(),
        "nothing in this plan is up, so nothing was reached: {again:?}"
    );
    assert_eq!(
        lock(&registry.running).len(),
        1,
        "the second walk spawned a second runner for a service that already has one"
    );
    assert_eq!(
        row(&store, &service("web")).await.1,
        None,
        "`web` was never spawned against a database that is between crashes"
    );

    registry.shutdown.cancel();
    tokio::time::timeout(EVENTUALLY, registry.shut_down())
        .await
        .expect("every runner finished");
}

/// **The other half of that rule.** Reading a crash-looping runner is right about the
/// service not being up and useless as an answer to somebody who has just *asked* for it to
/// start: nothing in that path could shorten the backoff the runner is sitting in, so every
/// attempt re-walked the tier, emitted two more events and spawned nothing. A start now reaches
/// the runner.
#[tokio::test]
async fn an_explicit_start_cuts_short_the_backoff_a_crash_loop_is_sitting_in() {
    let (_home, paths, store) = home(&["db"]).await;
    let registry = registry(
        &paths,
        &store,
        Arc::new(Declared(vec![crash_looping("db")])),
    );

    let graph = registry.graph().await.expect("one declared service");
    let plan = graph.start_plan([&service("db")]).expect("a plan");

    let first = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
        .await
        .expect("the first walk was answered");

    assert!(
        matches!(&first.failed, Some((id, _)) if id == &service("db")),
        "{first:?}"
    );
    assert_eq!(
        row(&store, &service("db")).await.0,
        ServiceState::Restarting,
        "the runner is in the backoff this test is about"
    );

    // Subscribed while that backoff is being served, so the only events on this stream are the
    // ones the second walk causes. Nothing else is running and a 30 second wait is silent.
    let mut watching = registry.events.subscribe();

    // **The timeout is the assertion.** `crash_looping`'s backoff is longer than `EVENTUALLY`, and
    // this walk is not answered until the attempt the request causes has been decided — so a
    // request that did not reach the runner could not be answered here at all.
    let again = tokio::time::timeout(EVENTUALLY, registry.start(&graph, &plan))
        .await
        .expect("the request cut the backoff short rather than waiting it out");

    assert!(
        matches!(
            &again.failed,
            Some((id, Fault { reason: Some(StateReason::Exited { code: Some(3) }), .. }))
                if id == &service("db")
        ),
        "the walk is answered by the attempt the request caused: {again:?}"
    );
    assert_eq!(
        lock(&registry.running).len(),
        1,
        "the service was asked to start again, not given a second runner"
    );

    // And it went back as a *request* and not as the policy coming round, which is the difference
    // `Restarts::recovered` records and the reason somebody reading the log needs.
    let frame = tokio::time::timeout(EVENTUALLY, watching.next())
        .await
        .expect("the stream is not silent")
        .expect("the stream is still open");

    let crate::api::events::Frame::Event(DaemonEvent::ServiceStateChanged(change)) = frame else {
        panic!("the first thing the second walk published was not a state change: {frame:?}");
    };

    assert_eq!(change.to, ServiceState::Starting);
    assert_eq!(change.reason, StateReason::Requested);

    registry.shutdown.cancel();
    tokio::time::timeout(EVENTUALLY, registry.shut_down())
        .await
        .expect("every runner finished");
}

/// **The limit of that, and the half a request must not outlive.** A runner is only listening for
/// one while it is sitting out a backoff, so a request that arrives mid-start is *kept* — and if
/// that start succeeds, nothing consumes it for as long as the service stays up. The crash after
/// that would then be released the instant it entered its backoff, its ladder reset and its move
/// published as `Requested`, on behalf of somebody who asked an hour earlier and got what they
/// asked for. The start that answers a request is what takes it.
#[tokio::test]
async fn a_start_asked_for_mid_start_is_taken_by_the_start_that_answers_it() {
    let (_home, paths, store) = home(&["db"]).await;
    let registry = registry(
        &paths,
        &store,
        Arc::new(Declared(vec![crashes_after_coming_up("db")])),
    );

    let graph = registry.graph().await.expect("one declared service");
    let plan = graph.start_plan([&service("db")]).expect("a plan");

    // The two walks a shared dependency produces, and the only way to reach the window this
    // test is about: one of them registers the runner, the other finds it `Deciding` a second
    // short of ready and asks it to start — which is a request no `wait_out` is going to take.
    let (first, second) = tokio::time::timeout(EVENTUALLY, async {
        tokio::join!(registry.start(&graph, &plan), registry.start(&graph, &plan))
    })
    .await
    .expect("both walks were answered");

    assert!(first.failed.is_none(), "{first:?}");
    assert!(second.failed.is_none(), "{second:?}");
    assert_eq!(
        lock(&registry.running).len(),
        1,
        "the second walk asked the first walk's runner rather than spawning another"
    );

    // Subscribed once the service is up, so the only events on this stream are the ones its
    // crash causes — and the request, if it survived, is the only thing that could cause more.
    let mut watching = registry.events.subscribe();

    loop {
        let frame = tokio::time::timeout(EVENTUALLY, watching.next())
            .await
            .expect("the service crashed as the fixture says it does")
            .expect("the stream is still open");

        let crate::api::events::Frame::Event(DaemonEvent::ServiceStateChanged(change)) = frame
        else {
            continue;
        };

        if change.to == ServiceState::Restarting {
            assert_eq!(
                change.reason,
                StateReason::Exited { code: Some(3) },
                "the crash is the fixture's, and nobody asked for it"
            );

            break;
        }
    }

    // **The silence is the assertion.** The backoff this runner has just entered is thirty
    // seconds and nothing has asked for anything since the start that succeeded; a request left
    // over from that start would end it here, within a millisecond, as a `Starting` reading
    // `Requested`.
    let next = tokio::time::timeout(SILENCE, watching.next()).await;

    assert!(
        next.is_err(),
        "the runner left a backoff nobody asked it to leave: {next:?}"
    );

    registry.shutdown.cancel();
    tokio::time::timeout(EVENTUALLY, registry.shut_down())
        .await
        .expect("every runner finished");
}
