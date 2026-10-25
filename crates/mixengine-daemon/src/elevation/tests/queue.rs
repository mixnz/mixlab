//! The queue: batches, enqueueing, dropping and grants.

use super::*;
use crate::elevation::Route;

/// A batch holding a helper operation and others runs the helper's first, and keeps
/// the rest in the order they were queued.
#[test]
fn the_helper_operation_goes_first_in_its_batch() {
    let pending = |id: i64, op: PrivilegedOp| mixengine_proto::PendingOp {
        id: mixengine_proto::PendingOpId(id),
        description: op.describe(),
        op,
        requested_at: Timestamp(0),
        needs_the_screen: None,
    };

    let mut waiting = vec![
        pending(1, PrivilegedOp::hosts_apply(None, Vec::new())),
        pending(2, PrivilegedOp::AuditLogRemove {}),
        pending(3, PrivilegedOp::HelperInstall {}),
    ];

    put_the_helper_first(&mut waiting);

    let order: Vec<i64> = waiting.iter().map(|pending| pending.id.0).collect();
    assert_eq!(order, vec![3, 1, 2]);
}

/// Wait for a job to end, so a test can assert on the row rather than on a race.
async fn finished(jobs: &Arc<crate::jobs::Jobs>, job: JobId) -> mixengine_proto::JobSummary {
    jobs.wait(job, mixengine_proto::Millis(5_000))
        .await
        .expect("the job ends")
}

/// Three rows: `Probe` through the shipped path, and two more written directly — enough to prove a
/// batch is a batch.
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

/// From the side that publishes it: the event carries the whole queue, and an enqueue that
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

/// The reason `can_prompt` is not merely `probe()`: a machine with every mechanism in
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

    let started = elevation
        .grant(Route::Dialog)
        .await
        .expect("a grant becomes a job");
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

/// The mock raises nothing and writes nothing, which makes "the helper ran and left no report" the
/// default here rather than a case somebody had to think to write. `Completed` does not promise a
/// file: a crash is not a per-OS event.
#[tokio::test]
async fn a_helper_that_left_no_report_fails_the_job_and_keeps_every_row() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    let started = elevation.grant(Route::Dialog).await.unwrap();
    let ended = finished(&elevation.jobs, started.id).await;

    assert_eq!(ended.state, mixengine_proto::JobState::Failed);
    assert_eq!(
        elevation.summary().await.unwrap().pending,
        1,
        "nothing was reported, so nothing may be assumed applied"
    );
}

/// A helper that refused its request writes nothing beside it, and what it said on stderr
/// is the only account of why. It has to reach the job's error, where the person who has just
/// typed a password reads it — not stop at a `debug` line.
#[tokio::test]
async fn a_helper_that_left_no_report_says_why_in_the_error() {
    let said = "mixengine-elevate: cannot read /Volumes/SSD/home/run/elevate/x/request.json: \
                    Operation not permitted (os error 1)";
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::elevation_saying("/tmp/mixengine", said)).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    let started = elevation.grant(Route::Dialog).await.unwrap();
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

    let started = elevation.grant(Route::Dialog).await.unwrap();
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

    let started = elevation.grant(Route::Dialog).await.unwrap();
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

/// On Linux the reason says what is missing and what works instead. It is worthless if the daemon
/// drops it, so it is asserted all the way through to `elevation.status`.
#[tokio::test]
async fn a_machine_that_cannot_prompt_keeps_the_reason_intact() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::unable_to_elevate(
        "/tmp/mixengine",
        "no polkit agent; pkexec is not installed",
    ))
    .await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    let error = elevation
        .grant(Route::Dialog)
        .await
        .expect_err("there is no way to raise a prompt here");

    assert_eq!(error.code, mixengine_proto::ErrorCode::PrivilegedRequired);
    assert!(error.to_string().contains("no polkit agent"), "{error}");
    assert_eq!(elevation.summary().await.unwrap().pending, 1);

    let status = elevation.status().await.unwrap();
    assert_eq!(
        status.reason.as_deref(),
        Some("no polkit agent; pkexec is not installed")
    );
}

/// One grant at a time, asserted on the slot rather than on a race: two concurrent grants are two
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
        .grant(Route::Dialog)
        .await
        .expect_err("one prompt at a time, for one queue");

    assert_eq!(error.code, mixengine_proto::ErrorCode::Conflict);
    assert!(error.to_string().contains("41"), "{error}");

    // And a slot left free is a slot a grant can take, or one failed grant would wedge the
    // daemon for as long as it runs.
    elevation.state.lock().unwrap().slot = Slot::Free;
    let started = elevation
        .grant(Route::Dialog)
        .await
        .expect("the slot was released");
    finished(&elevation.jobs, started.id).await;
    assert_eq!(elevation.state.lock().unwrap().slot, Slot::Free);
}

/// An empty queue asks for nothing. The helper refuses an empty batch outright, so spending a
/// prompt to find that out would be a dialog raised for no reason at all.
#[tokio::test]
async fn granting_an_empty_queue_raises_nothing() {
    let (_home, elevation, _events, machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    let error = elevation
        .grant(Route::Dialog)
        .await
        .expect_err("nothing is waiting");

    assert_eq!(error.code, mixengine_proto::ErrorCode::PreconditionFailed);
    assert!(machine.prompts_raised().is_empty());
}

/// No helper beside the daemon is a different sentence from no way to
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
        .grant(Route::Dialog)
        .await
        .expect_err("there is nothing to run");

    assert_eq!(error.code, mixengine_proto::ErrorCode::DependencyMissing);
    assert!(machine.prompts_raised().is_empty());
    assert_eq!(elevation.summary().await.unwrap().pending, 1);
}
