//! A grant that waits for a terminal to run the helper instead of raising a dialog (ADR 0065).

use std::time::Duration;

use mixengine_proto::privileged::{ElevationOutcome, OpOutcome, PrivilegedResponse};
use mixengine_proto::{ByHand, JobState, Millis};

use super::*;
use crate::elevation::Route;

/// A machine with no dialog, an operation waiting, and a wait short enough to run out in a test.
async fn waiting_machine(deadline: Duration) -> (tempfile::TempDir, Arc<Elevation>) {
    let (home, mut elevation, _events, _machine) = registry(mock::Host::unable_to_elevate(
        "/tmp/mixengine",
        "no polkit agent; pkexec is not installed",
    ))
    .await;
    Arc::get_mut(&mut elevation)
        .expect("nothing else holds the registry yet")
        .by_hand_wait = deadline;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    (home, elevation)
}

/// What `elevation.status` says while the grant waits, once the job has got that far.
async fn the_waiting_command(elevation: &Elevation) -> ByHand {
    for _ in 0..200 {
        if let Some(waiting) = elevation.status().await.unwrap().waiting {
            return waiting;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    panic!("the grant never said what it is waiting for");
}

/// Answer a request as the helper would, with every operation `AlreadyDone`.
fn the_helper_answers(waiting: &ByHand) -> (PathBuf, Vec<u8>) {
    let request = PathBuf::from(&waiting.request);
    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&request).expect("the request is on disk"))
            .expect("the request is JSON");

    let report = PrivilegedResponse {
        version: mixengine_proto::PROTOCOL_VERSION,
        elevate_version: "0.1.0".to_owned(),
        nonce: written["nonce"].as_str().expect("a nonce").to_owned(),
        elevated: true,
        supported_ops: vec!["probe".to_owned()],
        audit_log: PathBuf::from("/var/log/mixengine/elevate.log"),
        results: vec![OpOutcome::AlreadyDone],
    };

    (
        request
            .parent()
            .expect("a request lives in its own directory")
            .join(mixengine_proto::privileged::RESPONSE_FILE_NAME),
        serde_json::to_vec(&report).unwrap(),
    )
}

/// Wait for the job to end, and answer with it.
async fn finished(elevation: &Elevation, job: JobId) -> mixengine_proto::JobSummary {
    let summary = elevation
        .jobs
        .wait(job, Millis(10_000))
        .await
        .expect("the job exists");
    assert!(summary.state.is_finished(), "{summary:?}");

    summary
}

#[tokio::test]
async fn a_by_hand_grant_is_not_refused_where_no_dialog_can_be_raised() {
    let (home, elevation) = waiting_machine(Duration::from_secs(5)).await;

    let started = elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .expect("a terminal can ask");
    let waiting = the_waiting_command(&elevation).await;

    assert_eq!(waiting.job, started.id);
    assert!(
        Path::new(&waiting.request).starts_with(home.path().join("run").join("elevate")),
        "{}",
        waiting.request
    );
    assert!(
        Path::new(&waiting.helper).is_absolute(),
        "{}",
        waiting.helper
    );
    elevation.jobs.cancel(started.id).await.unwrap();
}

#[tokio::test]
async fn a_by_hand_grant_raises_no_dialog() {
    let (_home, elevation, _events, machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    let started = elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .unwrap();
    let waiting = the_waiting_command(&elevation).await;
    let (report, bytes) = the_helper_answers(&waiting);
    std::fs::write(report, bytes).unwrap();
    finished(&elevation, started.id).await;

    assert!(
        machine.prompts_raised().is_empty(),
        "the terminal raised the prompt, not the daemon"
    );
}

#[tokio::test]
async fn a_report_written_beside_the_request_settles_the_queue() {
    let (_home, elevation) = waiting_machine(Duration::from_secs(5)).await;

    let started = elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .unwrap();
    let waiting = the_waiting_command(&elevation).await;
    let (report, bytes) = the_helper_answers(&waiting);
    std::fs::write(report, bytes).unwrap();

    let summary = finished(&elevation, started.id).await;

    assert_eq!(summary.state, JobState::Succeeded, "{summary:?}");
    let status = elevation.status().await.unwrap();
    assert!(status.pending.is_empty(), "{:?}", status.pending);
    assert_eq!(status.waiting, None);
    assert!(
        !Path::new(&waiting.request).exists(),
        "the single-use directory is removed"
    );
}

#[tokio::test]
async fn a_half_written_report_is_read_once_it_is_whole() {
    let (_home, elevation) = waiting_machine(Duration::from_secs(5)).await;

    let started = elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .unwrap();
    let waiting = the_waiting_command(&elevation).await;
    let (report, bytes) = the_helper_answers(&waiting);
    std::fs::write(&report, b"").unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    std::fs::write(&report, bytes).unwrap();

    let summary = finished(&elevation, started.id).await;

    assert_eq!(summary.state, JobState::Succeeded, "{summary:?}");
    assert!(elevation.status().await.unwrap().pending.is_empty());
}

#[tokio::test]
async fn a_cancelled_wait_is_a_decline_and_keeps_every_row() {
    let (_home, elevation) = waiting_machine(Duration::from_secs(5)).await;

    let started = elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .unwrap();
    the_waiting_command(&elevation).await;
    elevation.jobs.cancel(started.id).await.unwrap();
    finished(&elevation, started.id).await;

    let status = elevation.status().await.unwrap();
    assert_eq!(
        status.last.map(|last| last.outcome),
        Some(ElevationOutcome::Declined)
    );
    assert_eq!(status.pending.len(), 1);
    assert_eq!(status.waiting, None);
    elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .expect("the slot was given back");
}

#[tokio::test]
async fn nobody_running_the_command_ends_as_a_decline() {
    let (_home, elevation) = waiting_machine(Duration::from_millis(500)).await;

    let started = elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .unwrap();
    let waiting = the_waiting_command(&elevation).await;
    finished(&elevation, started.id).await;

    let status = elevation.status().await.unwrap();
    assert_eq!(
        status.last.map(|last| last.outcome),
        Some(ElevationOutcome::Declined)
    );
    assert_eq!(status.pending.len(), 1);
    assert_eq!(status.waiting, None);
    assert!(!Path::new(&waiting.request).exists());
}

#[tokio::test]
async fn a_second_grant_while_one_waits_names_the_waiting_command() {
    let (_home, elevation) = waiting_machine(Duration::from_secs(5)).await;

    let started = elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .unwrap();
    the_waiting_command(&elevation).await;

    let error = elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .expect_err("one grant at a time");

    assert_eq!(error.code, mixengine_proto::ErrorCode::Conflict);
    assert!(
        error.to_string().contains("mix elevation status"),
        "{error}"
    );
    elevation.jobs.cancel(started.id).await.unwrap();
}

#[tokio::test]
async fn a_dialog_grant_is_unchanged_where_none_can_be_raised() {
    let (_home, elevation) = waiting_machine(Duration::from_secs(5)).await;

    let error = elevation
        .grant(Route::Dialog)
        .await
        .expect_err("there is no way to raise a dialog here");

    assert_eq!(error.code, mixengine_proto::ErrorCode::PrivilegedRequired);
    assert_eq!(elevation.status().await.unwrap().waiting, None);
}

/// A machine where a terminal cannot grant `probe`, as macOS cannot add to its System keychain,
/// with `probe` and an empty hosts block waiting.
async fn a_machine_with_a_screen_only_row() -> (tempfile::TempDir, Arc<Elevation>, Arc<mock::Host>)
{
    let (home, elevation, _events, machine) =
        registry(mock::Host::with_home("/tmp/mixengine").needing_the_screen_for("probe")).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();
    elevation
        .enqueue(&PrivilegedOp::hosts_apply(vec![]))
        .await
        .unwrap();

    (home, elevation, machine)
}

#[tokio::test]
async fn status_marks_what_a_terminal_cannot_grant() {
    let (_home, elevation, _machine) = a_machine_with_a_screen_only_row().await;

    let pending = elevation.status().await.unwrap().pending;

    let marked: Vec<(&str, Option<&str>)> = pending
        .iter()
        .map(|row| (row.op.name(), row.needs_the_screen.as_deref()))
        .collect();
    assert_eq!(
        marked,
        vec![
            ("probe", Some(mock::NEEDS_THE_SCREEN)),
            ("hosts-apply", None)
        ]
    );
}

#[tokio::test]
async fn a_terminal_grant_leaves_what_needs_the_screen_queued() {
    let (_home, elevation, _machine) = a_machine_with_a_screen_only_row().await;

    let started = elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .unwrap();
    let waiting = the_waiting_command(&elevation).await;
    let request = std::fs::read_to_string(&waiting.request).unwrap();
    assert!(!request.contains("\"probe\""), "{request}");
    assert!(request.contains("hosts-apply"), "{request}");

    let (report, bytes) = the_helper_answers(&waiting);
    std::fs::write(report, bytes).unwrap();
    finished(&elevation, started.id).await;

    let left: Vec<&str> = elevation
        .status()
        .await
        .unwrap()
        .pending
        .iter()
        .map(|row| row.op.name())
        .collect();
    assert_eq!(left, vec!["probe"], "kept for a grant raised at the screen");
}

#[tokio::test]
async fn a_terminal_grant_of_nothing_but_screen_rows_is_refused_before_a_job() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine").needing_the_screen_for("probe")).await;
    elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

    let error = elevation
        .grant(Route::ByHand { over_ssh: true })
        .await
        .expect_err("a terminal cannot grant the only row");

    assert_eq!(error.code, mixengine_proto::ErrorCode::PrivilegedRequired);
    assert!(
        error.to_string().contains(mock::NEEDS_THE_SCREEN),
        "{error}"
    );
    assert!(error.to_string().contains("mix elevation grant"), "{error}");
    assert_eq!(elevation.status().await.unwrap().pending.len(), 1);
    assert_eq!(elevation.status().await.unwrap().last, None);
}

#[tokio::test]
async fn a_dialog_grant_still_carries_what_needs_the_screen() {
    let (_home, elevation, machine) = a_machine_with_a_screen_only_row().await;

    let started = elevation.grant(Route::Dialog).await.unwrap();
    finished(&elevation, started.id).await;

    let raised = machine.prompts_raised();
    assert_eq!(raised.len(), 1);
    assert!(raised[0].body.contains("\"probe\""), "{}", raised[0].body);
}

/// At the machine's own screen, `sudo` in a terminal still reaches the dialog the operation asks
/// in, so nothing is left behind: only a grant from afar is.
#[tokio::test]
async fn a_terminal_at_the_screen_carries_what_needs_the_screen() {
    let (_home, elevation, _machine) = a_machine_with_a_screen_only_row().await;

    let started = elevation
        .grant(Route::ByHand { over_ssh: false })
        .await
        .unwrap();
    let waiting = the_waiting_command(&elevation).await;
    let request = std::fs::read_to_string(&waiting.request).unwrap();

    assert!(request.contains("\"probe\""), "{request}");
    elevation.jobs.cancel(started.id).await.unwrap();
}
