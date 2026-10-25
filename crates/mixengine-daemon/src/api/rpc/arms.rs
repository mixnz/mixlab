//! The arms of the routing table that answer straight from a subsystem, with no handler in `api/`
//! to sit beside: `path.*`, `autostart.*` and `database.*`. Tests only.

use std::sync::Arc;

use mixengine_proto::rpc;
use mixengine_proto::{AtBoot, AutostartReport, PathReport};
use serde_json::Value;

use crate::api::test_daemon::{call, daemon, undeclared};
use crate::services::fixture;

/// The three `path.*` methods over the dispatcher.
///
/// Against `mock::Host`, which is what makes this runnable at all: the real implementations
/// write a registry value or a file in the person's own home, and a suite that exercised them
/// would be a `cargo test` that edits the PATH of whoever ran it. The two real ones have their
/// own tests inside `mixengine-platform`, against a key and a home they create themselves.
#[tokio::test]
async fn the_path_is_reported_then_taken_and_then_given_back() {
    let daemon = undeclared().await;

    let before: PathReport = daemon.expect(rpc::method::PATH_STATUS, Value::Null).await;
    assert!(!before.on_path, "{before:?}");
    assert!(before.directory.ends_with("bin"), "{before:?}");

    // The status did not fill `bin/`, and says so rather than listing the table.
    assert!(before.commands.is_empty(), "{before:?}");

    let installed: PathReport = daemon.expect(rpc::method::PATH_INSTALL, Value::Null).await;
    assert!(installed.on_path);
    assert!(installed.places.iter().all(|place| place.changed));
    // This home has no runtime installed, so `bin/` fronts none of the compiled names —
    // a `node` there would hide a Node the person installed themselves.
    assert!(installed.commands.is_empty(), "{installed:?}");
    assert!(installed.stale.is_empty());

    // Idempotent, and it says which of the two it was — a client that reported a write it did
    // not perform would be indistinguishable from one that did.
    let again: PathReport = daemon.expect(rpc::method::PATH_INSTALL, Value::Null).await;
    assert!(again.on_path);
    assert!(again.places.iter().all(|place| !place.changed), "{again:?}");

    let removed: PathReport = daemon
        .expect(rpc::method::PATH_UNINSTALL, Value::Null)
        .await;
    assert!(!removed.on_path);

    // The shims stay: removing the home is what removes them.
    assert_eq!(removed.commands, installed.commands);

    // Two installs and an uninstall. The status is absent, which is the point: a read is not a
    // mutation, and the mock records only what changed the machine or tried to.
    assert_eq!(daemon.host.path_operations().len(), 3);

    daemon.quiet().await;
}

/// The three `autostart.*` methods over the dispatcher.
///
/// Against `mock::Host`, for the `path.*` test's reason one step louder: the real
/// implementations register a logon task, a LaunchAgent or a systemd user unit for the account
/// running them, and a suite that exercised them would be a `cargo test` that arranges for a
/// daemon to start at every login of whoever ran it. The three real ones have their own tests
/// inside `mixengine-platform`, against a scratch name and a directory they create themselves.
#[tokio::test]
async fn the_autostart_entry_is_reported_then_registered_and_then_taken_away() {
    let daemon = undeclared().await;

    let before: AutostartReport = daemon
        .expect(rpc::method::AUTOSTART_STATUS, Value::Null)
        .await;
    assert!(!before.enabled, "{before:?}");
    assert!(!before.changed, "a status never claims a write: {before:?}");
    assert!(!before.for_this_home, "{before:?}");
    assert!(before.command.is_empty(), "{before:?}");

    let enabled: AutostartReport = daemon
        .expect(rpc::method::AUTOSTART_ENABLE, Value::Null)
        .await;
    assert!(enabled.enabled && enabled.changed, "{enabled:?}");
    assert!(
        enabled.for_this_home,
        "the entry names this daemon's own home: {enabled:?}"
    );
    assert!(
        enabled.command.iter().any(|word| word == "--home"),
        "{enabled:?}"
    );

    // Idempotent, and it says which of the two it was — the `path.*` distinction, and for its
    // reason.
    let again: AutostartReport = daemon
        .expect(rpc::method::AUTOSTART_ENABLE, Value::Null)
        .await;
    assert!(again.enabled, "{again:?}");
    assert!(!again.changed, "the second enable wrote: {again:?}");

    let disabled: AutostartReport = daemon
        .expect(rpc::method::AUTOSTART_DISABLE, Value::Null)
        .await;
    assert!(!disabled.enabled && disabled.changed, "{disabled:?}");
    assert!(disabled.command.is_empty(), "{disabled:?}");

    // Two enables and a disable. The status is absent for the `path.*` recorder's reason: a
    // read is not a mutation.
    assert_eq!(daemon.host.autostart_operations().len(), 3);

    daemon.quiet().await;
}

/// The boot half over the dispatcher, against `mock::Host` for the reason above: the real one
/// turns on the linger of whoever runs the suite.
#[tokio::test]
async fn starting_at_boot_is_turned_on_and_off_and_the_login_entry_stays() {
    let daemon = undeclared().await;

    let at_boot: AutostartReport = daemon
        .expect(rpc::method::AUTOSTART_ENABLE_AT_BOOT, Value::Null)
        .await;
    assert!(at_boot.enabled && at_boot.for_this_home, "{at_boot:?}");
    assert_eq!(at_boot.at_boot, AtBoot::On);

    let login_only: AutostartReport = daemon
        .expect(rpc::method::AUTOSTART_DISABLE_AT_BOOT, Value::Null)
        .await;
    assert!(login_only.enabled, "the login entry stays: {login_only:?}");
    assert_eq!(login_only.at_boot, AtBoot::Off);

    daemon.quiet().await;
}

/// None of the three takes parameters, and a client that sent some is told so.
#[tokio::test]
async fn an_autostart_call_with_parameters_is_refused() {
    let answer =
        call(r#"{"jsonrpc":"2.0","method":"autostart.status","params":{"home":"/tmp"},"id":1}"#)
            .await;

    assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
}

/// A service with no database vocabulary is told so **in those words**.
///
/// Deliberately not `unsupported`: that code means *this operating system cannot*, and every
/// system this ships to runs the fixture service perfectly well. What has no databases is the
/// package. The same distinction the first planner drew for `blueprint.apply`.
#[tokio::test]
async fn creating_a_database_on_a_service_that_has_none_is_refused_by_name() {
    let daemon = daemon(
        Arc::new(fixture::Declared(vec![
            fixture::spec("db").build().expect("a usable spec"),
        ])),
        &["db"],
    )
    .await;

    let answer = daemon
        .ask(
            rpc::method::DATABASE_CREATE,
            serde_json::json!({"service": "db", "database": "blog"}),
        )
        .await;

    assert_eq!(
        answer["error"]["data"]["code"], "invalid_argument",
        "{answer}"
    );
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("db"),
        "{answer}"
    );

    daemon.quiet().await;
}

/// A service this home does not declare is a different miss, and says so: "no such service"
/// would send somebody looking for a service that is right there, and this is the case where it
/// genuinely is not.
#[tokio::test]
async fn creating_a_database_on_a_service_nothing_declares_is_not_found() {
    let daemon = daemon(Arc::new(fixture::Declared(Vec::new())), &[]).await;

    let answer = daemon
        .ask(
            rpc::method::DATABASE_CREATE,
            serde_json::json!({"service": "mariadb@main", "database": "blog"}),
        )
        .await;

    assert_eq!(answer["error"]["data"]["code"], "not_found", "{answer}");

    daemon.quiet().await;
}

/// `database.client`, `database.open` and `database.credentials` reach their handler, and a
/// service nothing declares is the same miss to all three.
/// What the methods *do* is proved beside them, in `crate::databases`, on a mock host that
/// records what it was asked to start.
#[tokio::test]
async fn asking_where_a_service_nothing_declares_could_be_opened_is_not_found() {
    let daemon = daemon(Arc::new(fixture::Declared(Vec::new())), &[]).await;

    for method in [
        rpc::method::DATABASE_CLIENT,
        rpc::method::DATABASE_OPEN,
        rpc::method::DATABASE_CREDENTIALS,
    ] {
        let answer = daemon
            .ask(method, serde_json::json!({"service": "mariadb@main"}))
            .await;

        assert_eq!(answer["error"]["data"]["code"], "not_found", "{answer}");
    }

    daemon.quiet().await;
}

/// A name that could not be a database name is refused **before** anything is started: a
/// caller's typo should not first cost a database server coming up.
#[tokio::test]
async fn a_database_name_that_is_not_one_is_refused_before_anything_starts() {
    let daemon = daemon(Arc::new(fixture::Declared(Vec::new())), &[]).await;

    let answer = daemon
        .ask(
            rpc::method::DATABASE_CREATE,
            serde_json::json!({"service": "mariadb@main", "database": "Blog; DROP"}),
        )
        .await;

    assert_eq!(
        answer["error"]["data"]["code"], "invalid_argument",
        "{answer}"
    );

    daemon.quiet().await;
}
