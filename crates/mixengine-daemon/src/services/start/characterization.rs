//! The moves one life of a service makes today, pinned from the outside before the walk that makes
//! them is rewritten.
//!
//! Each test records every `ServiceStateChanged` the service publishes, in order, and compares the
//! whole sequence. A change in the order of two writes, a reason that moved, or a state that is no
//! longer passed through fails here.

use std::time::Duration;

use mixengine_proto::{
    Backoff, DaemonEvent, HealthCheck, HealthProbe, Millis, ReadyCheck, RestartPolicy,
    ServiceSpecBuilder,
};
use mixengine_testkit::FakeService;

use super::super::fixture::{Declared, EVENTUALLY, arguments, home, registry, service, spec};
use super::*;
use crate::api::events::{Frame, Subscription};

/// Anything quicker than this did not wait for a ready check or a probe to finish.
const QUICK: Duration = Duration::from_secs(4);

/// Every move `id` makes until `done` says the last one has been seen.
async fn moves(
    watching: &mut Subscription,
    id: &ServiceId,
    mut done: impl FnMut(ServiceState) -> bool,
) -> Vec<(ServiceState, StateReason)> {
    let mut seen = Vec::new();

    loop {
        let frame = tokio::time::timeout(EVENTUALLY, watching.next())
            .await
            .unwrap_or_else(|_| panic!("no further move after {seen:?}"))
            .expect("the stream is still open");

        let Frame::Event(DaemonEvent::ServiceStateChanged(change)) = frame else {
            continue;
        };
        if &change.service != id {
            continue;
        }

        seen.push((change.to, change.reason));
        if done(change.to) {
            return seen;
        }
    }
}

/// A restart policy that tries again quickly, `retries` times.
fn retrying(retries: u32) -> RestartPolicy {
    RestartPolicy::OnFailure {
        max_retries: retries,
        window: Millis::from_secs(60),
        backoff: Backoff {
            initial: Millis(100),
            max: Millis(100),
            ..Backoff::default()
        },
    }
}

fn running(fake: &FakeService) -> ServiceSpecBuilder {
    spec("svc").args(arguments(fake))
}

/// Start `svc`, stop it `pause` after `stop_after` says so, and return every move it made and
/// how long the stop took.
async fn start_and_stop(
    declared: ServiceSpec,
    stop_after: impl FnMut(ServiceState) -> bool,
    pause: Duration,
    until: impl FnMut(ServiceState) -> bool,
) -> (Vec<(ServiceState, StateReason)>, Duration) {
    let (_home, paths, store) = home(&["svc"]).await;
    let registry = registry(&paths, &store, Arc::new(Declared(vec![declared])));
    let graph = registry.graph().await.expect("one declared service");
    let starting = graph.start_plan([&service("svc")]).expect("a plan");
    let stopping = graph.stop_plan([&service("svc")]).expect("a plan");

    let mut recording = registry.events.subscribe();
    let mut trigger = registry.events.subscribe();

    let mut stopped_in = Duration::ZERO;
    let stop = async {
        moves(&mut trigger, &service("svc"), stop_after).await;
        tokio::time::sleep(pause).await;
        let began = std::time::Instant::now();
        registry.stop(&stopping).await;
        stopped_in = began.elapsed();
    };

    tokio::time::timeout(EVENTUALLY, async {
        tokio::join!(registry.start(&graph, &starting), stop)
    })
    .await
    .expect("the start and the stop were both answered");

    let seen = moves(&mut recording, &service("svc"), until).await;

    registry.shutdown.cancel();
    tokio::time::timeout(EVENTUALLY, registry.shut_down())
        .await
        .expect("every runner finished");

    (seen, stopped_in)
}

/// Start `svc` and return every move it makes until `until`, with nobody asking it to stop.
async fn left_alone(
    declared: ServiceSpec,
    until: impl FnMut(ServiceState) -> bool,
) -> Vec<(ServiceState, StateReason)> {
    let (_home, paths, store) = home(&["svc"]).await;
    let registry = registry(&paths, &store, Arc::new(Declared(vec![declared])));
    let graph = registry.graph().await.expect("one declared service");
    let starting = graph.start_plan([&service("svc")]).expect("a plan");

    let mut recording = registry.events.subscribe();

    tokio::time::timeout(EVENTUALLY, registry.start(&graph, &starting))
        .await
        .expect("the walk was answered");
    let seen = moves(&mut recording, &service("svc"), until).await;

    registry.shutdown.cancel();
    tokio::time::timeout(EVENTUALLY, registry.shut_down())
        .await
        .expect("every runner finished");

    seen
}

fn is(wanted: ServiceState) -> impl FnMut(ServiceState) -> bool {
    move |state| state == wanted
}

#[tokio::test]
async fn ready_then_stopped() {
    let (seen, _) = start_and_stop(
        running(&FakeService::new()).build().expect("a spec"),
        is(ServiceState::Running),
        Duration::ZERO,
        is(ServiceState::Stopped),
    )
    .await;

    assert_eq!(
        seen,
        vec![
            (ServiceState::Starting, StateReason::Requested),
            (ServiceState::Running, StateReason::Ready),
            (ServiceState::Stopping, StateReason::Requested),
            (ServiceState::Stopped, StateReason::Requested),
        ]
    );
}

#[tokio::test]
async fn ready_then_crashed_then_restarted() {
    let crashes = FakeService::new()
        .ready_after(100)
        .exit_after(1_000)
        .exit_code(3);

    let seen = left_alone(
        running(&crashes)
            .restart(retrying(1))
            .build()
            .expect("a spec"),
        is(ServiceState::Failed),
    )
    .await;

    assert_eq!(
        seen,
        vec![
            (ServiceState::Starting, StateReason::Requested),
            (ServiceState::Running, StateReason::Ready),
            (
                ServiceState::Restarting,
                StateReason::Exited { code: Some(3) }
            ),
            (
                ServiceState::Starting,
                StateReason::BackoffElapsed { attempt: 1 }
            ),
            (ServiceState::Running, StateReason::Ready),
            (
                ServiceState::Failed,
                StateReason::CrashLoop {
                    attempts: 2,
                    window: Millis::from_secs(60),
                    tail: vec![mixengine_testkit::service::READY_LINE.to_owned()],
                },
            ),
        ]
    );
}

#[tokio::test]
async fn crashing_until_it_is_given_up_on() {
    let crashes = FakeService::new().never_ready().exit_after(50).exit_code(3);

    let seen = left_alone(
        running(&crashes)
            .restart(retrying(2))
            .build()
            .expect("a spec"),
        is(ServiceState::Failed),
    )
    .await;

    assert_eq!(
        seen,
        vec![
            (ServiceState::Starting, StateReason::Requested),
            (
                ServiceState::Restarting,
                StateReason::Exited { code: Some(3) }
            ),
            (
                ServiceState::Starting,
                StateReason::BackoffElapsed { attempt: 1 }
            ),
            (
                ServiceState::Restarting,
                StateReason::Exited { code: Some(3) }
            ),
            (
                ServiceState::Starting,
                StateReason::BackoffElapsed { attempt: 2 }
            ),
            (
                ServiceState::Failed,
                StateReason::CrashLoop {
                    attempts: 3,
                    window: Millis::from_secs(60),
                    tail: vec![],
                },
            ),
        ]
    );
}

#[tokio::test]
async fn never_ready_in_time() {
    let seen = left_alone(
        running(&FakeService::new().never_ready())
            .ready(ReadyCheck::LogPattern {
                regex: mixengine_testkit::service::READY_LINE.to_owned(),
                timeout: Millis(500),
            })
            .build()
            .expect("a spec"),
        is(ServiceState::Failed),
    )
    .await;

    assert_eq!(
        seen,
        vec![
            (ServiceState::Starting, StateReason::Requested),
            (
                ServiceState::Failed,
                StateReason::ReadyTimeout { after: Millis(500) },
            ),
        ]
    );
}

#[tokio::test]
async fn exited_cleanly_before_ready() {
    let seen = left_alone(
        running(&FakeService::new().never_ready().exit_after(50).exit_code(0))
            .restart(retrying(2))
            .build()
            .expect("a spec"),
        is(ServiceState::Stopped),
    )
    .await;

    assert_eq!(
        seen,
        vec![
            (ServiceState::Starting, StateReason::Requested),
            (
                ServiceState::Stopping,
                StateReason::Exited { code: Some(0) }
            ),
            (ServiceState::Stopped, StateReason::Exited { code: Some(0) }),
        ]
    );
}

#[tokio::test]
async fn stopped_while_starting() {
    let (seen, stopped_in) = start_and_stop(
        running(&FakeService::new().ready_after(10_000))
            .build()
            .expect("a spec"),
        is(ServiceState::Starting),
        Duration::ZERO,
        is(ServiceState::Stopped),
    )
    .await;

    assert_eq!(
        seen,
        vec![
            (ServiceState::Starting, StateReason::Requested),
            (ServiceState::Stopping, StateReason::Requested),
            (ServiceState::Stopped, StateReason::Requested),
        ]
    );
    assert!(
        stopped_in < QUICK,
        "the stop waited for the ready check: {stopped_in:?}"
    );
}

#[tokio::test]
async fn stopped_mid_probe() {
    let hangs = HealthCheck {
        probe: HealthProbe::Command {
            program: FakeService::program(),
            args: arguments(&FakeService::new().never_ready()),
        },
        // The first probe is made one interval after `Running`, and an interval may not be
        // shorter than the probe's timeout, so both are long enough to stand out against `QUICK`.
        interval: Millis::from_secs(8),
        timeout: Millis::from_secs(8),
        failures_before_degraded: 1,
        successes_before_running: 1,
    };

    let (seen, stopped_in) = start_and_stop(
        running(&FakeService::new())
            .health(hangs)
            .build()
            .expect("a spec"),
        is(ServiceState::Running),
        Duration::from_millis(8_500),
        is(ServiceState::Stopped),
    )
    .await;

    assert_eq!(
        seen,
        vec![
            (ServiceState::Starting, StateReason::Requested),
            (ServiceState::Running, StateReason::Ready),
            (ServiceState::Stopping, StateReason::Requested),
            (ServiceState::Stopped, StateReason::Requested),
        ]
    );
    assert!(
        stopped_in < QUICK,
        "the stop waited for the probe: {stopped_in:?}"
    );
}
