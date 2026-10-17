//! A service's state: idle, transitions, failures, who stopped it, and its process.

use super::*;

/// Each of the column's three states round-trips, and each is distinguishable afterwards.
///
/// The pair that matters is `Some(0)` and `None`: both are a service that will not be stopped,
/// and a client that could not tell them apart would say "switched off here" to somebody whose
/// real answer is "no default exists yet" — and send them to change a setting that was never
/// the cause.
#[tokio::test]
async fn idle_minutes_round_trips_through_all_three_states() {
    let (_home, store) = store().await;
    let service = service_row(&store, "fakeservice", ServiceState::Stopped).await;

    assert_eq!(
        idle_minutes(&store, &service).await.expect("the column"),
        None,
        "a row nobody has set is null, not zero"
    );

    set_idle(&store, &service, Some(45)).await.expect("a write");
    assert_eq!(
        idle_minutes(&store, &service).await.expect("the column"),
        Some(45)
    );

    set_idle(&store, &service, Some(0)).await.expect("a write");
    assert_eq!(
        idle_minutes(&store, &service).await.expect("the column"),
        Some(0),
        "zero is stored as zero rather than collapsed back to null"
    );

    set_idle(&store, &service, None).await.expect("a write");
    assert_eq!(
        idle_minutes(&store, &service).await.expect("the column"),
        None
    );
}

/// A service nobody declared is a `NotFound` rather than a silent no-op.
///
/// `UPDATE … WHERE id = ?` affects no rows and reports success, so without this check a client
/// that misspelled a service id would be told its setting was applied.
#[tokio::test]
async fn setting_idle_on_a_service_that_is_not_declared_is_refused() {
    let (_home, store) = store().await;
    let missing = ServiceId::parse("nothing@here").expect("a valid id");

    assert!(matches!(
        set_idle(&store, &missing, Some(10)).await,
        Err(Error::NotFound {
            kind: "service",
            ..
        })
    ));

    assert!(matches!(
        idle_minutes(&store, &missing).await,
        Err(Error::NotFound {
            kind: "service",
            ..
        })
    ));
}

const NOW: Timestamp = Timestamp(1_760_000_000_000);

#[tokio::test]
async fn a_transition_is_written_and_handed_back() {
    let (_home, store) = store().await;
    let id = service_row(&store, "caddy", ServiceState::Stopped).await;

    let change = transition(
        &store,
        &id,
        ServiceState::Starting,
        StateReason::Requested,
        NOW,
    )
    .await
    .expect("stopped can start");

    assert_eq!(change.from, ServiceState::Stopped);
    assert_eq!(change.to, ServiceState::Starting);
    assert_eq!(change.service, id);
    assert_eq!(change.at, NOW);

    assert_eq!(
        state(&store, &id).await.expect("the row"),
        ServiceState::Starting,
        "the value handed back is the value that survived the commit"
    );
}

/// **T200b, D5.** A move into `failed` keeps the sentence; a move into `running` forgets it.
#[tokio::test]
async fn a_failure_is_noted_with_its_sentence_and_forgotten_once_running() {
    let (_home, store) = store().await;
    let id = service_row(&store, "pool", ServiceState::Starting).await;

    transition_noting(
        &store,
        &id,
        ServiceState::Failed,
        StateReason::SpawnFailed,
        Some("the environment entry X: no credential is stored at mixengine/a/b"),
        NOW,
    )
    .await
    .expect("a starting service can fail");

    let note = record(&store, &id)
        .await
        .expect("the row")
        .last_failure
        .expect("a note");
    assert_eq!(note.reason, StateReason::SpawnFailed);
    assert_eq!(note.at, NOW);
    assert!(
        note.detail.contains("no credential is stored"),
        "{}",
        note.detail
    );

    for to in [ServiceState::Starting, ServiceState::Running] {
        transition(&store, &id, to, StateReason::Requested, NOW)
            .await
            .expect("an edge the machine has");
    }
    assert_eq!(
        record(&store, &id).await.expect("the row").last_failure,
        None,
        "a failure that has been recovered from is history"
    );
}

/// **T200b, D5.** With no sentence, the reason's own words are the note — a client never
/// translates a `StateReason`.
#[tokio::test]
async fn a_failure_without_a_sentence_is_noted_in_the_reasons_words() {
    let (_home, store) = store().await;
    let id = service_row(&store, "pool", ServiceState::Starting).await;

    transition(
        &store,
        &id,
        ServiceState::Failed,
        StateReason::SpawnFailed,
        NOW,
    )
    .await
    .expect("a starting service can fail");

    let note = record(&store, &id)
        .await
        .expect("the row")
        .last_failure
        .expect("a note");
    assert_eq!(note.detail, StateReason::SpawnFailed.to_string());
    assert_eq!(
        records(&store).await.expect("the rows")["pool"].last_failure,
        Some(note),
        "the listing reads the same note"
    );
}

/// **T200b, Review Focus 3.** A note this build cannot read is no note, not an error.
#[tokio::test]
async fn an_unreadable_failure_note_reads_as_none() {
    let (_home, store) = store().await;
    let id = service_row(&store, "pool", ServiceState::Failed).await;
    sqlx::query(
        "UPDATE services SET last_failure_json = '{\"kind\":\"from a newer build\"}' WHERE id = ?",
    )
    .bind(id.as_str())
    .execute(store.pool())
    .await
    .expect("the row");

    assert_eq!(
        record(&store, &id).await.expect("the row").last_failure,
        None
    );
}

/// Walk a service up and back down the way a supervisor does, arriving at `stopped` for `reason`.
async fn down(store: &Store, id: &ServiceId, reason: StateReason) {
    for to in [
        ServiceState::Starting,
        ServiceState::Running,
        ServiceState::Stopping,
    ] {
        transition(store, id, to, StateReason::Requested, NOW)
            .await
            .expect("an edge the machine has");
    }

    transition(store, id, ServiceState::Stopped, reason, NOW)
        .await
        .expect("a stopping service can stop");
}

/// **A stop somebody asked for is theirs** — the design's D8, which T123 does not touch.
///
/// `mix service stop mariadb@main` followed by the next connection starting it again is the tool
/// overruling its user, and this is the word that forbids it.
#[tokio::test]
async fn a_stop_somebody_asked_for_is_recorded_as_theirs() {
    let (_home, store) = store().await;
    let id = service_row(&store, "caddy", ServiceState::Stopped).await;

    down(&store, &id, StateReason::Requested).await;

    assert_eq!(
        record(&store, &id).await.expect("the row").stopped_by,
        StoppedBy::Person,
    );
}

/// **A shutdown is not one of those** — roadmap task **T123**.
///
/// `daemon.shutdown` and `service.stop` walk the same `Registry::stop`, so this row is the only
/// place the difference can survive to the next boot — and on-demand activation is what reads it
/// there. Recorded as a person's, every service on the machine came back from every restart
/// unwakeable, and every site through one answered 502 until somebody started it by hand.
#[tokio::test]
async fn a_stop_the_daemon_made_on_its_way_out_is_not_recorded_as_a_person_s() {
    let (_home, store) = store().await;
    let id = service_row(&store, "caddy", ServiceState::Stopped).await;

    down(&store, &id, StateReason::Shutdown).await;

    assert_eq!(
        record(&store, &id).await.expect("the row").stopped_by,
        StoppedBy::Daemon,
    );
}

/// **Nor is a process that went away under the daemon** — roadmap task **T123**.
///
/// What a restarted machine leaves: rows claiming to be running, reconciled to `stopped` by a
/// daemon that finds no process behind them. Nobody decided this, so nothing about it may keep a
/// connection from starting the service again.
#[tokio::test]
async fn a_process_that_vanished_is_nobody_s_stop() {
    let (_home, store) = store().await;
    let id = service_row(&store, "caddy", ServiceState::Stopped).await;

    down(&store, &id, StateReason::Vanished).await;

    assert_eq!(
        record(&store, &id).await.expect("the row").stopped_by,
        StoppedBy::Daemon,
    );
}

/// **And a service that is not stopped is nobody's stop at all** — roadmap task **T123**.
///
/// The column is written on the way out of `stopped` as well as into it, so a row can never
/// answer with a stop that is over. A reader that asks while the service is running gets the
/// truth rather than a leftover.
#[tokio::test]
async fn leaving_stopped_takes_the_stop_with_it() {
    let (_home, store) = store().await;
    let id = service_row(&store, "caddy", ServiceState::Stopped).await;

    down(&store, &id, StateReason::Requested).await;
    transition(
        &store,
        &id,
        ServiceState::Starting,
        StateReason::Requested,
        NOW,
    )
    .await
    .expect("a stopped service can start");

    assert_eq!(
        record(&store, &id).await.expect("the row").stopped_by,
        StoppedBy::Never,
    );
}

/// **A row that has never run has never been stopped** — roadmap task **T123**.
///
/// `service.create` writes `stopped`, because there is nowhere else for a service to begin. The
/// column's default is what keeps that from reading as a decision somebody made.
#[tokio::test]
async fn a_service_that_has_never_run_was_stopped_by_nobody() {
    let (_home, store) = store().await;
    let id = service_row(&store, "caddy", ServiceState::Stopped).await;

    assert_eq!(
        record(&store, &id).await.expect("the row").stopped_by,
        StoppedBy::Never,
    );
}

#[tokio::test]
async fn an_edge_the_machine_does_not_have_leaves_the_row_alone() {
    let (_home, store) = store().await;
    let id = service_row(&store, "caddy", ServiceState::Stopped).await;

    let error = transition(&store, &id, ServiceState::Running, StateReason::Ready, NOW)
        .await
        .expect_err("a stopped service cannot be running without starting");

    assert!(
        matches!(
            error,
            Error::Service(ServiceError::IllegalTransition {
                from: ServiceState::Stopped,
                to: ServiceState::Running,
                ..
            })
        ),
        "{error:?}"
    );
    assert_eq!(
        state(&store, &id).await.expect("the row"),
        ServiceState::Stopped,
        "the refused transition rolled back rather than half-applying"
    );
}

#[tokio::test]
async fn a_service_that_is_not_there_is_named_as_such() {
    let (_home, store) = store().await;
    let id = ServiceId::parse("mariadb@main").expect("a valid id");

    let error = state(&store, &id).await.expect_err("no such row");

    assert!(
        matches!(&error, Error::NotFound { kind: "service", id } if id == "mariadb@main"),
        "{error:?}"
    );
}

/// The constraint added in `0001_initial.sql` and the enum have to agree, or one of them is
/// decoration: every state the machine can be in must be storable, and nothing else may be.
#[tokio::test]
async fn the_column_accepts_every_state_and_nothing_else() {
    let (_home, store) = store().await;

    for state in ServiceState::ALL {
        service_row(&store, &format!("svc-{state}"), state).await;
    }

    let refused = sqlx::query(
        "INSERT INTO services (id, package_id, instance_name, state)
             VALUES ('bogus', (SELECT id FROM packages LIMIT 1), 'main', 'crashed')",
    )
    .execute(store.pool())
    .await;

    assert!(
        refused.is_err(),
        "the CHECK let a word through that ServiceState cannot read back"
    );
}

/// The pair, and the clearing of it, are one round trip: what T18 adopts is what T19 wrote.
#[tokio::test]
async fn the_process_a_service_is_running_as_is_written_and_then_cleared() {
    let (_home, store) = store().await;
    let id = service_row(&store, "caddy", ServiceState::Starting).await;

    started(&store, &id, 4321, Some(1_234_567), NOW)
        .await
        .expect("the row takes a pid");

    let (pid, start_time, at): (Option<i64>, Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT pid, pid_start_time, last_started_at FROM services WHERE id = ?")
            .bind(id.as_str())
            .fetch_one(store.pool())
            .await
            .expect("the row");

    assert_eq!(pid, Some(4321));
    assert_eq!(at, Some(NOW.0), "epoch milliseconds, not text");
    assert_eq!(
        start_time,
        Some(1_234_567),
        "the reading is stored as it was given, since only the OS that made it can read it"
    );

    ended(&store, &id, Some(3)).await.expect("the row lets go");

    let (pid, start_time, code): (Option<i64>, Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT pid, pid_start_time, last_exit_code FROM services WHERE id = ?")
            .bind(id.as_str())
            .fetch_one(store.pool())
            .await
            .expect("the row");

    assert_eq!(
        pid, None,
        "a row that kept a dead pid is a row the next daemon would adopt"
    );
    assert_eq!(
        start_time, None,
        "and half an identity is one the next daemon would have to guess at"
    );
    assert_eq!(code, Some(3));
}

#[tokio::test]
async fn recording_a_process_against_a_service_that_is_not_there_names_it() {
    let (_home, store) = store().await;
    let id = ServiceId::parse("caddy").expect("a valid id");

    let error = started(&store, &id, 1, None, NOW)
        .await
        .expect_err("no such row");

    assert!(
        matches!(&error, Error::NotFound { kind: "service", id } if id == "caddy"),
        "{error:?}"
    );
}

/// What a hand-edited database looks like from in here.
///
/// Asked of the function rather than through a doctored database on purpose. The `CHECK` above
/// makes the row unreachable through any write of ours, so producing one would mean disabling
/// the constraint with `PRAGMA writable_schema` — which is per *connection*, and this store
/// hands out four of them. That test would be about SQLite's schema cache; this one is about
/// what the reader does when the word does not parse, which is the part we wrote.
#[test]
fn a_state_this_build_does_not_know_blames_the_row() {
    let id = ServiceId::parse("caddy").expect("a valid id");

    let error = parse_state(&id, "crashed".to_owned()).expect_err("not a service state");

    assert!(
        matches!(&error, Error::Service(ServiceError::UnknownServiceState { service, value })
            if service == "caddy" && value == "crashed"),
        "{error:?}"
    );
}
