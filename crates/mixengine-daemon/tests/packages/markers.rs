//! Markers, adoption and the instances a start finds.

use super::*;

/// **`refresh` reaches the registry instead of answering from a still-fresh cache.**
///
/// [`tests/runtimes.rs`](../../mixengine-daemon/tests/runtimes.rs)'s test of the same name, for
/// `package.list_available` — the two share one `mixengine_core::index::Client<Index>` under the
/// daemon, so this is also proof that `runtime available --refresh` and `package available
/// --refresh` do not each refresh only the other's half of one document.
#[tokio::test]
async fn refresh_bypasses_a_fresh_cache() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    // Fetches and caches the index the fixture published.
    let first = client.call("package.list_available", json!({})).await;
    assert_eq!(
        first["packages"].as_array().map(Vec::len),
        Some(1),
        "{first}"
    );

    // Republished with nothing offered. The cache is still fresh, so an ordinary call keeps
    // answering from it — the behaviour `refresh` exists to bypass.
    fixture._registry.publish(&json!({
        "schema": 1,
        "generated_at": "2026-08-19T06:55:13Z",
        "packages": [],
    }));

    let cached = client.call("package.list_available", json!({})).await;
    assert_eq!(
        cached["packages"].as_array().map(Vec::len),
        Some(1),
        "a fresh cache is not asked about again: {cached}"
    );

    let refreshed = client
        .call("package.list_available", json!({"refresh": true}))
        .await;
    assert_eq!(
        refreshed["packages"].as_array().map(Vec::len),
        Some(0),
        "`refresh` reaches the registry instead of answering from the cache: {refreshed}"
    );
}

/// A package install leaves its marker too, for the same reason a runtime does.
#[tokio::test]
async fn an_installed_package_carries_its_marker() {
    let fixture = Fixture::start().await;
    let installed = fixture.client().await.install(VERSION).await;
    assert_eq!(installed["state"], "succeeded", "{installed}");

    let marker: Value = serde_json::from_slice(
        &std::fs::read(
            fixture
                .installed_at(VERSION)
                .join(".mixengine-install.json"),
        )
        .expect("the marker is in the install directory"),
    )
    .expect("the marker is JSON");

    assert_eq!(marker["what"], "package", "{marker}");
    assert_eq!(marker["package"], PACKAGE, "{marker}");
    assert_eq!(marker["version"], VERSION, "{marker}");
}

/// `package.adopt` answers a recorded version's row, and refuses one not on disk by name.
#[tokio::test]
async fn adopting_a_package_answers_its_row_or_says_it_is_not_there() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    let installed = client.install(VERSION).await;
    assert_eq!(installed["state"], "succeeded", "{installed}");

    let adopted = client
        .call(
            "package.adopt",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    assert_eq!(adopted["version"], VERSION, "{adopted}");

    let refused = client
        .refuse(
            "package.adopt",
            json!({"package": PACKAGE, "version": "9.9.7"}),
        )
        .await;
    assert_eq!(refused["data"]["code"], "not_found", "{refused}");
}

/// Service data an earlier home left under `data/` is listed with the version that would open it,
/// and adopting it makes a stopped service whose data directory is that one. A directory whose
/// first run never finished is listed with why it cannot be adopted, and left alone.
#[tokio::test]
async fn a_found_instance_is_listed_and_adopting_it_makes_it_a_stopped_service() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    let kept = fixture.home.path().join("data").join(PACKAGE).join("main");
    std::fs::create_dir_all(&kept).expect("a kept data directory");
    std::fs::write(kept.join("somebody.db"), b"data").expect("its data");
    std::fs::write(kept.join(".mixengine-ready"), format!("{VERSION}\n")).expect("its marker");

    let half = fixture.home.path().join("data").join(PACKAGE).join("half");
    std::fs::create_dir_all(&half).expect("a half-made data directory");
    std::fs::write(half.join("somebody.db"), b"data").expect("its data");

    let found = client.call("service.found", Value::Null).await;
    let rows = found["found"].as_array().expect("a list");
    let row = |id: &str| {
        rows.iter()
            .find(|row| row["service"] == id)
            .unwrap_or_else(|| panic!("{id} is listed: {found}"))
            .clone()
    };
    assert_eq!(row("fakeservice@main")["opens_with"], VERSION, "{found}");
    assert!(row("fakeservice@half")["why_not"].is_string(), "{found}");

    let doctor = client.call("daemon.doctor", Value::Null).await.to_string();
    assert!(
        doctor.contains("mix service found"),
        "the doctor names found service data: {doctor}"
    );

    let adopted = client
        .call("service.adopt", json!({"service": "fakeservice@main"}))
        .await;
    assert_eq!(adopted["id"], "fakeservice@main", "{adopted}");
    assert_eq!(adopted["state"], "stopped", "{adopted}");
    assert!(
        kept.join("somebody.db").is_file(),
        "its data is where it was"
    );

    let again = client
        .call("service.adopt", json!({"service": "fakeservice@main"}))
        .await;
    assert_eq!(
        again["id"], "fakeservice@main",
        "adopting twice answers the service: {again}"
    );

    let found = client.call("service.found", Value::Null).await;
    assert!(
        found["found"]
            .as_array()
            .is_some_and(|rows| rows.iter().all(|row| row["service"] != "fakeservice@main")),
        "an adopted instance is no longer found: {found}"
    );

    let refused = client
        .refuse("service.adopt", json!({"service": "fakeservice@half"}))
        .await;
    assert_eq!(refused["data"]["code"], "precondition_failed", "{refused}");
    assert!(
        half.join("somebody.db").is_file(),
        "a refused adopt touches nothing"
    );

    let refused = client
        .refuse("service.adopt", json!({"service": "fakeservice@nope"}))
        .await;
    assert_eq!(refused["data"]["code"], "not_found", "{refused}");
}

/// `package.found` lists a package directory with no row, with why.
#[tokio::test]
async fn a_package_directory_with_no_row_is_listed_with_why() {
    let fixture = Fixture::start().await;
    let stray = fixture.installed_at("9.9.9");
    std::fs::create_dir_all(&stray).expect("a directory nothing installed");

    let mut client = fixture.client().await;
    let found = client.call("package.found", json!({})).await;
    let row = &found["found"][0];
    assert_eq!(row["package"], PACKAGE, "{found}");
    assert_eq!(row["version"], "9.9.9", "{found}");
    assert!(
        row["why"].as_str().is_some_and(|why| why.contains("index")),
        "{found}"
    );
    assert!(stray.is_dir(), "listing touches nothing");
}

/// **A restore comes first.** While a copy of the earlier home can be restored, its service data is
/// not offered for adopting one by one: adopting a database first would give this home a service of
/// its own, and the restore that brings back the projects and sites would then be refused.
#[tokio::test]
async fn service_data_is_not_offered_while_a_restore_is() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    let kept = fixture.home.path().join("data").join(PACKAGE).join("main");
    std::fs::create_dir_all(&kept).expect("a kept data directory");
    std::fs::write(kept.join("somebody.db"), b"data").expect("its data");
    std::fs::write(kept.join(".mixengine-ready"), format!("{VERSION}\n")).expect("its marker");

    let scratch = tempfile::tempdir().expect("a scratch home");
    let other = mixengine_core::Store::open(&scratch.path().join("mixengine.db"))
        .await
        .expect("an earlier home's database");
    let data = fixture.home.path().join("data");
    mixengine_core::adopt::snapshot::write(&other, &data)
        .await
        .expect("a copy");
    other.close().await;

    let found = client.call("service.found", Value::Null).await;
    assert_eq!(
        found["found"],
        json!([]),
        "a restore is offered instead: {found}"
    );

    std::fs::remove_file(data.join(".mixengine-state.db")).expect("the copy");
    let found = client.call("service.found", Value::Null).await;
    assert_eq!(
        found["found"][0]["service"], "fakeservice@main",
        "with no copy, the data is offered again: {found}"
    );
}
