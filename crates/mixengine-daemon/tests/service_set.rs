//! `service.set` against a real `mixengined` over a real socket: moving a service to another port.
//!
//! **The services are `fakeservice` rows**, rendered by the daemon's own generator through a recipe
//! compiled into debug builds only — so these are ignored in a release build, for the reason
//! `tests/limits.rs` gives.
//!
//! The D-numbers here are those of
//! `docs/specs/2026-10-25-t210-moving-a-service-to-another-port-design.md`.

use std::net::{Ipv4Addr, TcpListener};

use mixengine_proto::rpc::method;
use mixengine_testkit::{Client, Daemon, Home, Service};
use serde_json::{Value, json};

/// The service most tests here move.
const SERVICE: &str = "fakeservice@main";

/// A port nothing on this machine holds, out of a band no `bind(0)` is ever handed.
fn a_free_port(after: u16) -> u16 {
    (after..after + 200)
        .find(|port| TcpListener::bind((Ipv4Addr::LOCALHOST, *port)).is_ok())
        .expect("a free port in the band")
}

/// A home with `services` declared, and a daemon answering for it.
async fn declared(services: &[Service]) -> (Home, Daemon) {
    let home = Home::new();
    let daemon = Daemon::start(env!("CARGO_BIN_EXE_mixengined"), &home);
    home.wait_until_listening().await;

    mixengine_testkit::create(home.endpoint(), &home.database_file(), services).await;

    (home, daemon)
}

async fn status(client: &mut Client, service: &str) -> Value {
    client
        .call(method::SERVICE_STATUS, json!({ "service": service }))
        .await
}

async fn started(client: &mut Client, service: &str) {
    let walk = client
        .call(
            method::SERVICE_START,
            json!({ "service": service, "wait": true }),
        )
        .await;
    assert_eq!(walk["failed"], Value::Null, "{walk}");
}

/// D4: a stopped service is moved and rendered, and stays stopped.
#[tokio::test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
async fn a_stopped_service_moves_and_stays_stopped() {
    let (home, _daemon) = declared(&[Service::new(SERVICE)]).await;
    let mut client = Client::connect(&home).await;
    let from = status(&mut client, SERVICE).await["port"].clone();
    let to = a_free_port(27_100);

    let moved = client
        .call(
            method::SERVICE_SET,
            json!({ "service": SERVICE, "port": to }),
        )
        .await;

    assert_eq!(moved["port"]["from"], from, "{moved}");
    assert_eq!(moved["port"]["to"], to, "{moved}");
    assert_eq!(moved["restarted"], false, "{moved}");
    assert_eq!(moved["service"]["state"], "stopped", "{moved}");
    assert_eq!(moved["service"]["port"], to, "{moved}");
    assert!(
        moved["not_followed"]
            .as_array()
            .is_some_and(|lines| !lines.is_empty()),
        "what the daemon cannot change was not named: {moved}"
    );
    assert_eq!(status(&mut client, SERVICE).await["port"], to);
}

/// D4: a running service is restarted, and is running again afterwards.
#[tokio::test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
async fn a_running_service_is_restarted_on_its_new_port() {
    let (home, _daemon) = declared(&[Service::new(SERVICE)]).await;
    let mut client = Client::connect(&home).await;
    started(&mut client, SERVICE).await;
    let before = status(&mut client, SERVICE).await;
    let to = a_free_port(27_300);

    let moved = client
        .call(
            method::SERVICE_SET,
            json!({ "service": SERVICE, "port": to }),
        )
        .await;

    assert_eq!(moved["restarted"], true, "{moved}");
    let after = status(&mut client, SERVICE).await;
    assert_eq!(after["state"], "running", "{after}");
    assert_eq!(after["port"], to, "{after}");
    assert_ne!(
        after["pid"], before["pid"],
        "the process was not replaced: {after}"
    );
}

/// D5: a start that fails on the new port puts the service back on the old one, running.
#[tokio::test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
async fn a_start_that_fails_on_the_new_port_goes_back() {
    let to = a_free_port(27_500);
    let (home, _daemon) =
        declared(&[Service::new(SERVICE).refuse_port(to).ready_timeout(2_000)]).await;
    let mut client = Client::connect(&home).await;
    started(&mut client, SERVICE).await;
    let from = status(&mut client, SERVICE).await["port"].clone();

    let answer = client
        .ask(
            method::SERVICE_SET,
            json!({ "service": SERVICE, "port": to }),
        )
        .await;

    let error = &answer["error"];
    assert!(
        !error.is_null(),
        "a start that failed was reported as a move: {answer}"
    );
    let hint = error["data"]["hint"].as_str().unwrap_or_default();
    assert!(
        hint.contains(&format!("back on {from}")),
        "the hint does not say where the service is now: {answer}"
    );

    let after = status(&mut client, SERVICE).await;
    assert_eq!(after["port"], from, "{after}");
    assert_eq!(after["state"], "running", "{after}");
}

/// D3: another row's port is refused by that row's name.
#[tokio::test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
async fn a_port_another_service_holds_is_refused_by_name() {
    let other = "fakeservice@other";
    let (home, _daemon) = declared(&[Service::new(SERVICE), Service::new(other)]).await;
    let mut client = Client::connect(&home).await;
    let taken = status(&mut client, other).await["port"].clone();

    let answer = client
        .ask(
            method::SERVICE_SET,
            json!({ "service": SERVICE, "port": taken }),
        )
        .await;

    assert_eq!(
        answer["error"]["data"]["code"], "already_exists",
        "{answer}"
    );
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains(other)),
        "{answer}"
    );
}

/// Review focus: a service on its way down is not moved under the stop.
#[tokio::test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
async fn a_stopping_service_is_refused() {
    let (home, _daemon) = declared(&[Service::new(SERVICE).ignore_stop().stop_grace(20_000)]).await;
    let mut client = Client::connect(&home).await;
    started(&mut client, SERVICE).await;

    client
        .call(
            method::SERVICE_STOP,
            json!({ "service": SERVICE, "wait": false }),
        )
        .await;

    // The stop is under way once the row says so.
    for _ in 0..100 {
        if status(&mut client, SERVICE).await["state"] == "stopping" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let answer = client
        .ask(
            method::SERVICE_SET,
            json!({ "service": SERVICE, "port": a_free_port(27_700) }),
        )
        .await;

    assert_eq!(
        answer["error"]["data"]["code"], "precondition_failed",
        "{answer}"
    );
}

/// D7: the daemon says which services may move.
#[tokio::test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
async fn a_summary_says_whether_it_may_move() {
    let (home, _daemon) = declared(&[Service::new(SERVICE)]).await;
    let mut client = Client::connect(&home).await;

    assert_eq!(status(&mut client, SERVICE).await["port_movable"], true);
}

/// D1: a request that names no setting is refused in words.
#[tokio::test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
async fn a_request_with_nothing_to_set_is_refused() {
    let (home, _daemon) = declared(&[Service::new(SERVICE)]).await;
    let mut client = Client::connect(&home).await;

    let answer = client
        .ask(method::SERVICE_SET, json!({ "service": SERVICE }))
        .await;

    assert_eq!(
        answer["error"]["data"]["code"], "invalid_argument",
        "{answer}"
    );
}

/// **Two moves of one service at once are taken one after the other.** Each is a stop and a start;
/// interleaved, the second reads a service the first is halfway through restarting and is refused,
/// or the row ends on one port while the process listens on the other.
#[tokio::test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
async fn two_moves_at_once_are_taken_in_turn() {
    let (home, _daemon) = declared(&[Service::new(SERVICE).ready_after(300)]).await;
    let mut client = Client::connect(&home).await;
    started(&mut client, SERVICE).await;
    let first = a_free_port(27_900);
    let second = a_free_port(first + 1);

    let mut one = Client::connect(&home).await;
    let mut two = Client::connect(&home).await;
    let (a, b) = tokio::join!(
        one.ask(
            method::SERVICE_SET,
            json!({ "service": SERVICE, "port": first })
        ),
        two.ask(
            method::SERVICE_SET,
            json!({ "service": SERVICE, "port": second })
        ),
    );

    assert!(a["error"].is_null(), "the first move was refused: {a}");
    assert!(b["error"].is_null(), "the second move was refused: {b}");

    let after = status(&mut client, SERVICE).await;
    assert_eq!(after["state"], "running", "{after}");
    assert!(
        after["port"] == first || after["port"] == second,
        "the row ended on neither port: {after}"
    );
}

/// **A move that fails and goes back does not undo a move that succeeded beside it.** Without the
/// two being taken in turn, the failed one's put-back lands after the other's write, and the
/// service ends on its first port while `mix` was told it moved.
#[tokio::test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
async fn a_failed_move_does_not_undo_one_beside_it() {
    let refused = a_free_port(28_100);
    let accepted = a_free_port(refused + 1);
    let (home, _daemon) = declared(&[Service::new(SERVICE)
        .refuse_port(refused)
        .ready_timeout(1_500)])
    .await;
    let mut client = Client::connect(&home).await;
    started(&mut client, SERVICE).await;

    let mut one = Client::connect(&home).await;
    let mut two = Client::connect(&home).await;
    let (a, b) = tokio::join!(
        one.ask(
            method::SERVICE_SET,
            json!({ "service": SERVICE, "port": refused })
        ),
        async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            two.ask(
                method::SERVICE_SET,
                json!({ "service": SERVICE, "port": accepted }),
            )
            .await
        },
    );

    assert!(
        !a["error"].is_null(),
        "a port that does not start was reported as a move: {a}"
    );
    assert!(b["error"].is_null(), "the second move was refused: {b}");

    let after = status(&mut client, SERVICE).await;
    assert_eq!(
        after["port"], accepted,
        "the move that succeeded was undone: {after}"
    );
    assert_eq!(after["state"], "running", "{after}");
}
