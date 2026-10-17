use mixengine_proto::rpc;
use mixengine_proto::{Millis, ReadyCheck, ServiceState};
use mixengine_testkit::FakeService;
use serde_json::Value;

use super::*;
use crate::api::test_daemon::{daemon, undeclared, web_and_db};
use crate::services::fixture;

#[tokio::test]
async fn a_home_that_declares_nothing_lists_nothing_rather_than_failing() {
    // The honest answer for this build, and the one the registry, the graph and the walk all
    // handle without a special case: `Undeclared` until T30 renders a row into a spec.
    let list: ServiceList = undeclared()
        .await
        .expect(rpc::method::SERVICE_LIST, Value::Null)
        .await;

    assert!(list.services.is_empty(), "{list:?}");
}

#[tokio::test]
async fn a_listing_names_the_declared_set_with_what_each_row_says() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    let list: ServiceList = daemon.expect(rpc::method::SERVICE_LIST, Value::Null).await;

    let ids: Vec<&str> = list.services.iter().map(|one| one.id.as_str()).collect();
    assert_eq!(
        ids,
        ["db", "web"],
        "in id order, which is a listing's order"
    );

    let web = &list.services[1];
    assert_eq!(web.state, Some(ServiceState::Stopped), "what the row says");
    assert!(!web.supervised, "nothing has been started");
    assert_eq!(web.pid, None);
    assert_eq!(
        web.depends_on,
        vec![fixture::service("db")],
        "the graph's edge, which is what makes a start order explicable"
    );
}

// The project scope — roadmap task **T125**. What the set *is* belongs to
// `core::sites::services_of`, which is tested where it lives; what is proved here is the three
// requests this surface has to answer with a refusal rather than with a walk.

#[tokio::test]
async fn a_start_scoped_to_a_project_nothing_answers_to_is_not_found() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    let answer = daemon
        .ask(
            rpc::method::SERVICE_START,
            serde_json::json!({"project": {"name": "nope"}}),
        )
        .await;

    assert_eq!(answer["error"]["data"]["code"], "not_found");
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("nope"),
        "{answer}"
    );
}

/// **Two subjects is refused rather than resolved by precedence.** Picking one would carry out
/// the half the caller did not mean — silently, on somebody's machine.
#[tokio::test]
async fn a_start_naming_a_service_and_a_project_is_refused() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    let answer = daemon
        .ask(
            rpc::method::SERVICE_START,
            serde_json::json!({"service": "db", "project": {"name": "blog"}}),
        )
        .await;

    assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
    assert_eq!(
        daemon.state("db").await,
        Some(ServiceState::Stopped),
        "a refused request starts nothing"
    );
}

/// **A reset names one service, and no service at all is refused by the type** — roadmap task
/// **T127**.
///
/// T125's finding was that `ServiceTarget::service` is optional and [`None`] means *the whole
/// home*; a repair that could be asked for with no subject is that mistake with worse
/// consequences. `ResetCredential::service` is required, so this is a deserialisation refusal
/// rather than a check somebody could delete.
#[tokio::test]
async fn a_credential_reset_requires_a_service() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    for params in [
        serde_json::json!({}),
        serde_json::json!({ "project": { "name": "blog" } }),
    ] {
        let answer = daemon
            .ask(rpc::method::SERVICE_RESET_CREDENTIAL, params.clone())
            .await;

        assert_eq!(
            answer["error"]["data"]["code"], "invalid_argument",
            "{params} {answer}"
        );
    }
}

/// **A service that keeps no credential is refused before anything is stopped** — **T127**.
///
/// The second assertion is the one worth having: a repair that refused *after* the stop would
/// take a web server down to tell somebody they named the wrong thing.
#[tokio::test]
async fn a_service_with_no_credential_is_refused_without_being_stopped() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    daemon
        .ask(
            rpc::method::SERVICE_START,
            serde_json::json!({ "service": "web" }),
        )
        .await;

    let answer = daemon
        .ask(
            rpc::method::SERVICE_RESET_CREDENTIAL,
            serde_json::json!({ "service": "web", "wait": true }),
        )
        .await;

    assert_eq!(
        answer["error"]["data"]["code"], "invalid_argument",
        "{answer}"
    );
    assert_eq!(
        daemon.state("web").await,
        Some(ServiceState::Running),
        "a refused reset stopped the service anyway"
    );
}

/// **A project has no stop.** The set includes the front end every *other* site on the machine
/// is reached through, so a project-scoped stop is a machine-wide outage wearing one project's
/// name — and an accepted request would be exactly that, not an error a user can see.
#[tokio::test]
async fn a_stop_and_a_restart_have_no_project_scope() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    for method in [rpc::method::SERVICE_STOP, rpc::method::SERVICE_RESTART] {
        let answer = daemon
            .ask(method, serde_json::json!({"project": {"name": "blog"}}))
            .await;

        assert_eq!(
            answer["error"]["data"]["code"], "invalid_argument",
            "{method} answered {answer}"
        );
    }
}

/// **T97.** A listing says what each service is *for*, so a client never has to decide that
/// `nginx` means "front end" for itself.
///
/// Written with nginx rather than Caddy on purpose, which is `front_end::held_by`'s own test
/// rule: the answer has to come from the recipe, and a lookup that had happened to be a
/// comparison against the string `caddy` would pass the other way round and fail this.
#[tokio::test]
async fn a_listing_says_what_each_service_is_for() {
    let declared = Arc::new(fixture::Declared(vec![
        fixture::spec("mariadb@main")
            .build()
            .expect("a usable spec"),
        fixture::spec("nginx").build().expect("a usable spec"),
    ]));
    let daemon = daemon(declared, &["mariadb@main", "nginx"]).await;

    let list: ServiceList = daemon.expect(rpc::method::SERVICE_LIST, Value::Null).await;

    let role = |id: &str| {
        list.services
            .iter()
            .find(|one| one.id.as_str() == id)
            .unwrap_or_else(|| panic!("{id} is in the listing"))
            .role
    };

    assert_eq!(
        role("nginx"),
        Some(ServiceRole::FrontEnd {
            server: mixengine_proto::FrontEndServer::Nginx
        }),
        "the program every site is reached through, by what its recipe is for"
    );
    assert_eq!(
        role("mariadb@main"),
        Some(ServiceRole::Other {}),
        "a database is not one of them"
    );
}

#[tokio::test]
async fn a_service_that_is_declared_and_has_no_row_is_reported_without_a_state() {
    // Not a case a finished MixEngine reaches — from T30 a declaration is rendered *from* a row
    // — and reported rather than smoothed into `stopped`, because a service that claims to be
    // stopped and then refuses to start explains nothing to whoever declared it.
    let daemon = daemon(web_and_db(), &["db"]).await;

    let list: ServiceList = daemon.expect(rpc::method::SERVICE_LIST, Value::Null).await;

    assert_eq!(list.services[0].state, Some(ServiceState::Stopped), "db");
    assert_eq!(list.services[1].state, None, "web, which has no row");
}

#[tokio::test]
async fn the_status_of_a_service_nobody_declared_is_not_found() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    let answer = daemon
        .ask(
            rpc::method::SERVICE_STATUS,
            serde_json::json!({"service": "mailpit"}),
        )
        .await;

    assert_eq!(answer["error"]["data"]["code"], "not_found");
    assert_eq!(
        answer["error"]["code"], -32000,
        "the method ran and the work did not: an application error, not a protocol one"
    );
}

#[tokio::test]
async fn a_status_with_no_service_is_refused_rather_than_answered_as_a_listing() {
    let answer = undeclared()
        .await
        .ask(rpc::method::SERVICE_STATUS, serde_json::json!({}))
        .await;

    assert_eq!(answer["error"]["code"], -32602, "{answer}");
    assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
}

#[tokio::test]
async fn starting_one_service_starts_what_it_depends_on_and_says_what_it_reached() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    let walk: ServiceWalk = daemon
        .expect(
            rpc::method::SERVICE_START,
            serde_json::json!({"service": "web"}),
        )
        .await;

    assert!(walk.complete, "the default is to wait: {walk:?}");
    assert_eq!(
        walk.planned,
        vec![fixture::service("db"), fixture::service("web")],
        "a plan is the transitive set, in the order it was walked"
    );
    assert_eq!(walk.reached, walk.planned, "{walk:?}");
    assert!(walk.failed.is_none(), "{walk:?}");

    let summary: ServiceSummary = daemon
        .expect(
            rpc::method::SERVICE_STATUS,
            serde_json::json!({"service": "db"}),
        )
        .await;
    assert_eq!(summary.state, Some(ServiceState::Running));
    assert!(summary.supervised, "a task is holding it");
    assert!(summary.pid.is_some(), "the process it is running as");

    daemon.quiet().await;
}

#[tokio::test]
async fn a_start_that_is_not_waited_for_answers_with_the_plan_and_walks_behind_it() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    let walk: ServiceWalk = daemon
        .expect(
            rpc::method::SERVICE_START,
            serde_json::json!({"service": "web", "wait": false}),
        )
        .await;

    assert!(!walk.complete, "nothing has happened yet: {walk:?}");
    assert_eq!(walk.planned.len(), 2, "the plan is still what was accepted");
    assert!(
        walk.reached.is_empty() && walk.failed.is_none(),
        "an empty walk, and it says so through `complete` rather than by looking finished"
    );

    // The walk carries on inside the daemon, which is the whole of what this mode promises.
    daemon.until("web", ServiceState::Running).await;

    daemon.quiet().await;
}

#[tokio::test]
async fn a_dependency_that_fails_stops_the_walk_and_blocks_what_needed_it() {
    let never = FakeService::new().never_ready();
    let specs = Arc::new(fixture::Declared(vec![
        fixture::spec("db")
            .args(fixture::arguments(&never))
            .ready(ReadyCheck::LogPattern {
                regex: mixengine_testkit::service::READY_LINE.to_owned(),
                timeout: Millis(750),
            })
            .build()
            .expect("a usable spec"),
        fixture::spec("web")
            .depends_on(fixture::service("db"))
            .build()
            .expect("a usable spec"),
    ]));
    let daemon = daemon(specs, &["db", "web"]).await;

    let walk: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, Value::Null).await;

    assert!(walk.reached.is_empty(), "{walk:?}");
    let failed = walk.failed.clone().expect("the walk stopped somewhere");
    assert_eq!(failed.service, fixture::service("db"));
    assert!(
        matches!(
            failed.reason,
            Some(mixengine_proto::StateReason::ReadyTimeout { .. })
        ),
        "the reason the transition carried, not one invented here: {failed:?}"
    );
    assert_eq!(
        walk.blocked,
        vec![fixture::service("web")],
        "fail-fast: what needed it was never spawned"
    );

    // And the row says the same thing, with the edge that broke named on it.
    assert_eq!(daemon.state("web").await, Some(ServiceState::Failed));

    daemon.quiet().await;
}

#[tokio::test]
async fn stopping_a_service_takes_down_what_depends_on_it_first() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    let _: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, Value::Null).await;

    let walk: ServiceWalk = daemon
        .expect(
            rpc::method::SERVICE_STOP,
            serde_json::json!({"service": "db"}),
        )
        .await;

    assert_eq!(
        walk.planned,
        vec![fixture::service("web"), fixture::service("db")],
        "the opposite walk: a site is never left pointed at a database that is going away"
    );
    assert_eq!(walk.reached, walk.planned);
    assert!(
        walk.failed.is_none(),
        "a stop has no state it fails to reach"
    );

    assert_eq!(daemon.state("db").await, Some(ServiceState::Stopped));
    assert_eq!(daemon.state("web").await, Some(ServiceState::Stopped));

    daemon.quiet().await;
}

/// The one that says what `restart` means, and the reason it is not stop-then-start-the-same-id.
#[tokio::test]
async fn restarting_a_dependency_puts_back_everything_it_took_down() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    let _: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, Value::Null).await;
    let before = daemon
        .expect::<ServiceSummary>(
            rpc::method::SERVICE_STATUS,
            serde_json::json!({"service": "db"}),
        )
        .await
        .pid;

    let walk: ServiceWalk = daemon
        .expect(
            rpc::method::SERVICE_RESTART,
            serde_json::json!({"service": "db"}),
        )
        .await;

    // Stopping `db` takes `web` with it; starting `db` alone would have left it there.
    assert_eq!(
        walk.planned,
        vec![fixture::service("db"), fixture::service("web")],
        "what the stop covered, in start order: {walk:?}"
    );
    assert_eq!(walk.reached, walk.planned, "{walk:?}");

    let after = daemon
        .expect::<ServiceSummary>(
            rpc::method::SERVICE_STATUS,
            serde_json::json!({"service": "db"}),
        )
        .await;

    assert_eq!(after.state, Some(ServiceState::Running));
    assert_ne!(after.pid, before, "a restart is a different process");
    assert_eq!(
        daemon.state("web").await,
        Some(ServiceState::Running),
        "the dependent came back rather than being left where the stop put it"
    );

    daemon.quiet().await;
}

/// The other half of what `restart` means: it puts back what it took down, and nothing else.
#[tokio::test]
async fn restarting_a_dependency_leaves_a_dependent_that_was_down_where_it_was() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    // Only `db`. A plan for one service pulls in what it depends on, and `web` depends on `db`
    // rather than the other way about, so `web` is deliberately left stopped.
    let _: ServiceWalk = daemon
        .expect(
            rpc::method::SERVICE_START,
            serde_json::json!({"service": "db"}),
        )
        .await;
    assert_eq!(daemon.state("web").await, Some(ServiceState::Stopped));

    let walk: ServiceWalk = daemon
        .expect(
            rpc::method::SERVICE_RESTART,
            serde_json::json!({"service": "db"}),
        )
        .await;

    assert_eq!(
        walk.planned,
        vec![fixture::service("db")],
        "the stop covered `web` and took nothing down there: {walk:?}"
    );
    assert_eq!(
        daemon.state("web").await,
        Some(ServiceState::Stopped),
        "a restart of a dependency is not a request to start its dependents"
    );

    daemon.quiet().await;
}

#[tokio::test]
async fn starting_a_service_that_is_not_declared_is_not_found_rather_than_a_walk() {
    let daemon = daemon(web_and_db(), &["db", "web"]).await;

    let answer = daemon
        .ask(
            rpc::method::SERVICE_START,
            serde_json::json!({"service": "mailpit"}),
        )
        .await;

    assert_eq!(answer["error"]["data"]["code"], "not_found");
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("mailpit")),
        "{answer}"
    );
}

/// **The code is the failure's own, not one this handler picked** — R8.
///
/// It used to be `internal` for every source failure whatever it was, and the fixture is why:
/// `SpecSource` returned `anyhow::Error`, the wire mapping downcast to find a
/// `mixengine_core::Error` behind it, and a fixture that raised a bare string had none to find.
/// The running daemon never took that path — its source renders through `mixengine-core` and so
/// always carried a code — so what this test was pinning down was the hole and not the product.
/// With the trait typed, a package that is not installed answers `not_found` and points at
/// `mix package list`, which is the sentence `error.rs` has argued for since T30: a user who
/// misspelled a setting must not be sent to file a bug report.
#[tokio::test]
async fn a_source_that_cannot_answer_is_reported_with_the_failure_s_own_code() {
    let daemon = daemon(Arc::new(fixture::Unavailable), &[]).await;

    let answer = daemon.ask(rpc::method::SERVICE_LIST, Value::Null).await;

    assert_eq!(answer["error"]["data"]["code"], "not_found");
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("no such package")),
        "the source's own complaint survives the trip: {answer}"
    );
}

#[tokio::test]
async fn a_target_says_nothing_and_means_every_declared_service() {
    // The three spellings of "no parameters" that `no_params` accepts, on a method that does
    // take them: a client sending any of them is asking about everything.
    for params in [Value::Null, serde_json::json!({}), serde_json::json!([])] {
        let daemon = undeclared().await;
        let walk: ServiceWalk = daemon.expect(rpc::method::SERVICE_START, params).await;

        assert!(walk.planned.is_empty(), "nothing is declared: {walk:?}");
        assert!(walk.complete);
    }
}
