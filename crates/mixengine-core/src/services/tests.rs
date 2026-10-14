use std::path::Path;

use super::*;
use crate::ServiceError;

/// A `services` row, without the Phase 3 machinery that will eventually create one.
///
/// A service cannot exist without a parent even in a test — the `CHECK` demands one of the two
/// columns and `foreign_keys=ON` demands the row it names — which is the constraint doing its
/// job, not an obstacle to route around.
async fn service_row(store: &Store, id: &str, state: ServiceState) -> ServiceId {
    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES (?, '1.0.0', '/packages/x', '2026-08-12T00:00:00Z', 'https://example', 'ab')
             ON CONFLICT (name, version) DO NOTHING",
    )
    .bind(id)
    .execute(store.pool())
    .await
    .expect("a package for the service to belong to");

    sqlx::query(
        "INSERT INTO services (id, package_id, instance_name, state)
             VALUES (?, (SELECT id FROM packages WHERE name = ?), 'main', ?)",
    )
    .bind(id)
    .bind(id)
    .bind(state.as_str())
    .execute(store.pool())
    .await
    .expect("the service row");

    ServiceId::parse(id).expect("a valid id")
}

/// A `packages` row for a service to be an instance of.
async fn package(store: &Store, name: &str) {
    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES (?, '1.0.0', '/packages/x', '2026-08-21T00:00:00Z', 'https://example', 'ab')",
    )
    .bind(name)
    .execute(store.pool())
    .await
    .expect("a package for the service to belong to");
}

/// What a caller hands [`create`], with everything but the port left at its plainest.
fn declaration(id: &str, package: &str, port: Port) -> Declaration {
    Declaration {
        service: ServiceId::parse(id).expect("a valid id"),
        origin: Origin::Package {
            name: package.to_owned(),
            version: PackageVersion::parse("1.0.0").expect("a version"),
        },
        instance_name: "main".to_owned(),
        port,
        bind_addr: None,
        data_dir: None,
        autostart: false,
        overrides: "{}".to_owned(),
    }
}

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

async fn store() -> (tempfile::TempDir, Store) {
    let home = tempfile::tempdir().expect("a temporary directory");
    let store = Store::open(&home.path().join(crate::paths::DATABASE_FILE_NAME))
        .await
        .expect("a database");
    (home, store)
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
/// **T97.** A row reads back as the value that wrote it, which is what makes a switch's
/// rollback a second [`create`] rather than a reconstruction from a summary.
#[tokio::test]
async fn a_row_reads_back_as_the_declaration_that_wrote_it() {
    let (_home, store) = store().await;

    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES ('mariadb', '11.4.2', '/packages/mariadb/11.4.2', '2026-09-07T00:00:00Z',
                     'https://example.invalid/mariadb', 'ab')",
    )
    .execute(store.pool())
    .await
    .expect("a package for the service to belong to");

    let service = ServiceId::parse("mariadb@legacy").expect("a valid id");
    let written = Declaration {
        service: service.clone(),
        origin: Origin::Package {
            name: "mariadb".to_owned(),
            version: PackageVersion::parse("11.4.2").expect("a version"),
        },
        instance_name: "legacy".to_owned(),
        port: Port::Fixed(3307),
        bind_addr: Some("0.0.0.0".to_owned()),
        data_dir: Some("/somewhere/else".to_owned()),
        autostart: true,
        overrides: r#"{"innodb_buffer_pool_size":"512M"}"#.to_owned(),
    };

    create(
        &store,
        &mixengine_platform::mock::Host::with_home("/mixengine"),
        &written,
    )
    .await
    .expect("a service");

    let read = super::declaration(&store, &service)
        .await
        .expect("the row back");

    assert!(
        matches!(
            &read.origin,
            Origin::Package { name, version }
                if name == "mariadb" && version.as_str() == "11.4.2"
        ),
        "{:?}",
        read.origin
    );
    assert_eq!(read.service, service);
    assert_eq!(read.instance_name, "legacy");
    assert_eq!(
        read.port,
        Port::Fixed(3307),
        "a restore takes the number back as itself and does not go looking for one again"
    );
    assert_eq!(read.bind_addr.as_deref(), Some("0.0.0.0"));
    assert_eq!(read.data_dir.as_deref(), Some("/somewhere/else"));
    assert!(read.autostart);
    assert_eq!(read.overrides, written.overrides);
}

/// A service with no port has none coming back, and not one somebody would have to allocate.
#[tokio::test]
async fn a_row_with_no_port_reads_back_as_a_service_that_has_none() {
    let (_home, store) = store().await;
    let service = service_row(&store, "caddy", ServiceState::Stopped).await;

    let read = super::declaration(&store, &service)
        .await
        .expect("the row back");

    assert_eq!(read.port, Port::None);
    assert!(!read.autostart);
    assert_eq!(
        read.bind_addr.as_deref(),
        Some("127.0.0.1"),
        "the column's own default, read back as the value it holds"
    );
}

#[tokio::test]
async fn a_declaration_of_a_service_that_is_not_there_is_named_as_such() {
    let (_home, store) = store().await;
    let id = ServiceId::parse("nginx").expect("a valid id");

    let error = super::declaration(&store, &id)
        .await
        .expect_err("no such row");

    assert!(
        matches!(&error, Error::NotFound { kind: "service", id } if id == "nginx"),
        "{error:?}"
    );
}

/// A row whose binary comes from an installed runtime rather than from a package.
///
/// The whole of T32's schema change seen from the only place that writes it: `create` resolves
/// `runtime_installs` instead of `packages`, and the row that lands names one parent.
#[tokio::test]
async fn a_service_can_come_from_a_runtime_install() {
    let (_home, store) = store().await;

    sqlx::query(
        "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url, sha256)
             VALUES ('php', '8.3.33', 'stable', '/runtimes/php/8.3.33', '2026-08-19T00:00:00Z',
                     1, 'https://example.invalid/php', 'abc')",
    )
    .execute(store.pool())
    .await
    .expect("a runtime install");

    let service = ServiceId::parse("php-fpm@8.3.33").expect("a valid id");

    create(
        &store,
        &mixengine_platform::mock::Host::with_home("/mixengine"),
        &Declaration {
            service,
            origin: Origin::Runtime {
                kind: RuntimeKind::Php,
                version: PackageVersion::parse("8.3.33").expect("a version"),
            },
            instance_name: "8.3.33".to_owned(),
            port: Port::None,
            bind_addr: None,
            data_dir: None,
            autostart: false,
            overrides: "{}".to_owned(),
        },
    )
    .await
    .expect("a pool for an installed PHP");

    let (package_id, runtime_install_id): (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT package_id, runtime_install_id FROM services WHERE id = 'php-fpm@8.3.33'",
    )
    .fetch_one(store.pool())
    .await
    .expect("the row that was written");

    assert_eq!(package_id, None, "a pool has no package to point at");
    assert!(
        runtime_install_id.is_some(),
        "the row points at the PHP it runs out of"
    );
}

/// A recipe's preferred port is a wish; what lands in the row is what the machine allowed.
///
/// The two halves have to agree or the service is configured for a port it was never given:
/// what [`create`] answers is what a client prints, and what the column holds is what the
/// template renders from.
#[tokio::test]
async fn an_allocated_port_is_the_one_that_lands_in_the_row() {
    let (_home, store) = store().await;
    package(&store, "mariadb").await;

    let preferred = ports::tests::a_free_run(2);
    let _squatter = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, preferred))
        .expect("a squatter");

    let written = create(
        &store,
        &mixengine_platform::mock::Host::with_home("/mixengine"),
        &declaration("mariadb@main", "mariadb", Port::Allocate { preferred }),
    )
    .await
    .expect("a service");

    assert!(
        written.port > Some(preferred),
        "the preferred port was held by this test's own listener"
    );
    assert!(
        written.moved_from.is_some(),
        "a service that did not get 3306 has to say so"
    );

    let column: Option<i64> =
        sqlx::query_scalar("SELECT port FROM services WHERE id = 'mariadb@main'")
            .fetch_one(store.pool())
            .await
            .expect("the row that was written");

    assert_eq!(column, written.port.map(i64::from));
}

/// Somebody who names a port has already decided, and is not moved off it.
///
/// The allocation exists because a recipe's 3306 is a *wish*. `--port 3307` is not one, and a
/// daemon that answered it with 3308 because something was listening would be overruling the
/// one instruction in the call — the start that then fails says who holds it (T38), which is
/// the honest place for that news.
#[tokio::test]
async fn a_port_the_caller_named_is_written_down_even_when_it_is_busy() {
    let (_home, store) = store().await;
    package(&store, "mariadb").await;

    let squatter =
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("a squatter");
    let named = squatter.local_addr().expect("its address").port();

    let written = create(
        &store,
        &mixengine_platform::mock::Host::with_home("/mixengine"),
        &declaration("mariadb@main", "mariadb", Port::Fixed(named)),
    )
    .await
    .expect("a service");

    assert_eq!(written.port, Some(named));
    assert_eq!(
        written.moved_from, None,
        "nothing was moved, so there is nothing to explain"
    );
}

/// Two creates racing for one preferred port come out with two ports.
///
/// **This is what the critical section is for**, and it is the failure that would otherwise
/// reach a user as a second database that starts, binds nothing and dies: both calls read a
/// table neither has written to, both bind-test a port neither has taken, and both write down
/// the same number.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_creates_racing_for_one_port_are_given_two() {
    let (_home, store) = store().await;
    package(&store, "mariadb").await;
    package(&store, "mysql").await;

    let host = mixengine_platform::mock::Host::with_home("/mixengine");
    let preferred = {
        let probe =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("a probe");
        probe.local_addr().expect("its address").port()
    };

    let one = declaration("mariadb@main", "mariadb", Port::Allocate { preferred });
    let other = declaration("mysql@main", "mysql", Port::Allocate { preferred });

    let (first, second) = tokio::join!(create(&store, &host, &one), create(&store, &host, &other),);

    let first = first.expect("a service").port.expect("a port");
    let second = second.expect("a service").port.expect("a port");

    assert_ne!(
        first, second,
        "both services were told to listen on {first}"
    );
}

/// Two services pointed at one data directory, which is the conflict a port conflict is not.
///
/// A port two services share is a start that fails and says who holds it (T38). A *directory*
/// two database servers share is two `mysqld`s over one set of InnoDB files, and what it costs
/// is the data rather than the start — so it is refused where it is written down rather than
/// discovered where it is opened. Only an explicit `data_dir` can reach this: the layout the
/// generator derives is `data/<package>/<instance>` and two instances cannot collide in it.
#[tokio::test]
async fn a_data_directory_another_service_holds_is_refused() {
    let (home, store) = store().await;
    package(&store, "mariadb").await;
    package(&store, "mysql").await;

    let host = mixengine_platform::mock::Host::with_home("/mixengine");
    let shared = home.path().join("shared").display().to_string();

    create(
        &store,
        &host,
        &Declaration {
            data_dir: Some(shared.clone()),
            ..declaration("mariadb@main", "mariadb", Port::None)
        },
    )
    .await
    .expect("the first service to claim the directory");

    let error = create(
        &store,
        &host,
        &Declaration {
            data_dir: Some(shared.clone()),
            ..declaration("mysql@main", "mysql", Port::None)
        },
    )
    .await
    .expect_err("two servers over one data directory");

    let said = error.to_string();

    assert!(
        said.contains(&shared) && said.contains("mariadb@main"),
        "the refusal names the directory and whoever already holds it, and said: {said}"
    );
}

/// A relative path and the absolute one it means are one directory.
///
/// `--data-dir db` resolves against the shell it was typed in, and the row beside it may hold
/// the same directory written out in full — from the GUI, or from an earlier create in another
/// terminal. The strings share nothing; the servers would share every file. A `.` segment needs
/// no help here, because [`PathBuf`](std::path::PathBuf) compares components and drops those on
/// its own — what this asserts is the part that has to be asked for.
#[tokio::test]
async fn a_relative_data_directory_is_the_absolute_one_it_resolves_to() {
    let (_home, store) = store().await;
    package(&store, "mariadb").await;
    package(&store, "mysql").await;

    let host = mixengine_platform::mock::Host::with_home("/mixengine");
    let relative = Path::new("target").join("t36-shared-data");
    let absolute = std::env::current_dir()
        .expect("a working directory")
        .join(&relative);

    create(
        &store,
        &host,
        &Declaration {
            data_dir: Some(absolute.display().to_string()),
            ..declaration("mariadb@main", "mariadb", Port::None)
        },
    )
    .await
    .expect("the first service to claim the directory");

    let error = create(
        &store,
        &host,
        &Declaration {
            data_dir: Some(relative.display().to_string()),
            ..declaration("mysql@main", "mysql", Port::None)
        },
    )
    .await
    .expect_err("the same directory, named the short way");

    assert!(
        matches!(
            error,
            Error::Service(ServiceError::DataDirectoryTaken { .. })
        ),
        "refused for the directory rather than for something else: {error}"
    );
}

/// **T183.** Each of the three parents answers for its own version, and a listing and a single
/// read agree.
#[tokio::test]
async fn a_record_carries_the_version_of_the_program_it_runs() {
    let (_home, store) = store().await;

    // A server, out of a `packages` row — `service_row` installs its package at 1.0.0.
    let server = service_row(&store, "fakeservice", ServiceState::Stopped).await;

    // php-fpm, out of a `runtime_installs` row.
    sqlx::query(
        "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url, sha256)
             VALUES ('php', '8.3.33', 'stable', '/runtimes/php/8.3.33', '2026-09-24T00:00:00Z',
                     1, 'https://example.invalid/php', 'abc')",
    )
    .execute(store.pool())
    .await
    .expect("a runtime install");
    sqlx::query(
        "INSERT INTO services (id, runtime_install_id, instance_name, state)
             VALUES ('php-fpm@8.3.33', (SELECT id FROM runtime_installs LIMIT 1), '8.3.33',
                     'stopped')",
    )
    .execute(store.pool())
    .await
    .expect("a php-fpm row");

    // An extension's service: its version is the extension's, not a program's.
    sqlx::query(
        "INSERT INTO extensions (id, name, version, kind, manifest_json, install_dir, data_dir,
                                     source, signed, installed_at)
             VALUES ('mailpit', 'Mailpit', '0.9.0', 'service', '{}', '/ext/mailpit',
                     '/data/extensions/mailpit', 'path', 0, '2026-09-24T00:00:00Z')",
    )
    .execute(store.pool())
    .await
    .expect("an extension row");
    sqlx::query(
        "INSERT INTO services (id, extension_id, instance_name, state)
             VALUES ('mailpit', 'mailpit', 'mailpit', 'stopped')",
    )
    .execute(store.pool())
    .await
    .expect("an extension's service row");

    let listed = records(&store).await.expect("the rows");
    let version_of = |id: &str| listed[id].version.as_ref().map(PackageVersion::as_str);

    assert_eq!(version_of("fakeservice"), Some("1.0.0"));
    assert_eq!(version_of("php-fpm@8.3.33"), Some("8.3.33"));
    assert_eq!(
        version_of("mailpit"),
        None,
        "an extension's version is not the program's"
    );

    assert_eq!(
        record(&store, &server).await.expect("the row").version,
        listed["fakeservice"].version,
        "the single read and the listing must not drift"
    );
}

/// A version nobody can parse is a listing with one blank cell, not a listing that fails.
#[tokio::test]
async fn a_version_that_does_not_parse_is_reported_as_none() {
    let (_home, store) = store().await;
    let service = service_row(&store, "fakeservice", ServiceState::Stopped).await;
    sqlx::query("UPDATE packages SET version = 'not a version' WHERE name = 'fakeservice'")
        .execute(store.pool())
        .await
        .expect("a hand-edited row");

    assert_eq!(
        record(&store, &service).await.expect("the row").version,
        None
    );
    assert_eq!(
        records(&store).await.expect("the rows")["fakeservice"].version,
        None
    );
}

/// The `CHECK` is the whole guarantee that [`Origin`] is not a suggestion.
///
/// Written through raw SQL rather than through [`create`], because `create` cannot express
/// either of these — which is the point: what is being asserted is that a hand-edited database,
/// or a future writer nobody has written yet, cannot express them either.
#[tokio::test]
async fn a_row_names_one_parent_and_not_two_and_not_none() {
    let (_home, store) = store().await;

    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES ('caddy', '2.11.4', '/packages/caddy', '2026-08-19T00:00:00Z',
                     'https://example.invalid/caddy', 'ab')",
    )
    .execute(store.pool())
    .await
    .expect("a package");

    sqlx::query(
        "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url, sha256)
             VALUES ('php', '8.3.33', 'stable', '/runtimes/php/8.3.33', '2026-08-19T00:00:00Z',
                     1, 'https://example.invalid/php', 'abc')",
    )
    .execute(store.pool())
    .await
    .expect("a runtime install");

    let both = sqlx::query(
        "INSERT INTO services (id, package_id, runtime_install_id, instance_name, state)
             VALUES ('both', (SELECT id FROM packages LIMIT 1),
                     (SELECT id FROM runtime_installs LIMIT 1), 'both', 'stopped')",
    )
    .execute(store.pool())
    .await;
    assert!(both.is_err(), "a service with two parents was accepted");

    let neither = sqlx::query(
        "INSERT INTO services (id, instance_name, state) VALUES ('orphan', 'orphan', 'stopped')",
    )
    .execute(store.pool())
    .await;
    assert!(neither.is_err(), "a service with no parent was accepted");
}

/// **A service stopped for a credential reset may not be woken by a connection** — roadmap task
/// **T127**.
///
/// **Asserted through [`StoppedBy::may_be_woken`] and not through the reason word**, because
/// [`StoppedBy::of`] reaches [`StoppedBy::Daemon`] through a catch-all arm: a reason that is not
/// named there becomes wakeable, and nothing fails to compile over it. What that costs is a
/// server started against a data directory a reset is part-way through writing — the one thing
/// the repair holds the service down for.
#[test]
fn a_stop_for_a_credential_reset_may_not_be_woken() {
    let stopped_by = StoppedBy::of(&StateReason::CredentialReset);

    assert_eq!(stopped_by, StoppedBy::Person);
    assert!(!stopped_by.may_be_woken());
}

/// And the daemon's own stops are still wakeable, which is the half the arm above must not break.
///
/// **[`StateReason::SuperuserRefused`] is in here although it never reaches this function** —
/// roadmap task **T127a**. It is only ever carried into [`ServiceState::Failed`] and this is
/// consulted on the way into [`ServiceState::Stopped`], so the catch-all arm is what would
/// answer for it. This says which answer that has to be if the word ever does arrive: a database
/// that refused its superuser is not a stop anybody asked for, and a connection may still try.
#[test]
fn the_daemons_own_stops_are_still_wakeable() {
    for reason in [
        StateReason::Shutdown,
        StateReason::Idle {
            after: mixengine_proto::Millis(600_000),
        },
        StateReason::SuperuserRefused {
            said: "FATAL:  password authentication failed for user \"postgres\"".to_owned(),
        },
    ] {
        let stopped_by = StoppedBy::of(&reason);

        assert_eq!(stopped_by, StoppedBy::Daemon, "{reason:?}");
        assert!(stopped_by.may_be_woken(), "{reason:?}");
    }
}
