//! Markers, adoption, a restored copy of the last home, and what `bin/` holds.

use super::*;

/// **`refresh` reaches the registry instead of answering from a still-fresh cache.**
///
/// `mixengine_core::index::Client::catalogue` only asks the network again once the cache is older
/// than `FRESH_FOR` — six hours, which no test can wait out. `refresh` is the escape hatch: a person
/// who just watched a new version get published should not have to wait for the old one's six hours
/// to run out, and this is the only way to prove the flag does that rather than nothing.
#[tokio::test]
async fn refresh_bypasses_a_fresh_cache() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    // Fetches and caches the index the fixture published.
    let first = client.call("runtime.list_available", json!({})).await;
    assert_eq!(
        first["runtimes"].as_array().map(Vec::len),
        Some(1),
        "{first}"
    );

    // Republished with the one offered version gone. The cache is still fresh, so an ordinary call
    // keeps answering from it — the behaviour `refresh` exists to bypass, proved here so the test
    // below is proof of the flag and not of a registry that always answers the same way.
    fixture.registry.publish(&json!({
        "schema": 1,
        "generated_at": "2026-08-14T06:55:13Z",
        "packages": [],
    }));

    let cached = client.call("runtime.list_available", json!({})).await;
    assert_eq!(
        cached["runtimes"].as_array().map(Vec::len),
        Some(1),
        "a fresh cache is not asked about again: {cached}"
    );

    let refreshed = client
        .call("runtime.list_available", json!({"refresh": true}))
        .await;
    assert_eq!(
        refreshed["runtimes"].as_array().map(Vec::len),
        Some(0),
        "`refresh` reaches the registry instead of answering from the cache: {refreshed}"
    );
}

/// **T182f, D1.** An install leaves its marker beside what it unpacked, naming what was installed,
/// so a home that loses its database can record the directory again.
#[tokio::test]
async fn an_installed_runtime_carries_its_marker() {
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

    assert_eq!(marker["what"], "runtime", "{marker}");
    assert_eq!(marker["kind"], "php", "{marker}");
    assert_eq!(marker["version"], VERSION, "{marker}");
    assert_eq!(marker["sha256"], fixture.packed.sha256, "{marker}");
}

/// **T182f, the machine it was reported from.** A PHP installed by one home is on disk when the
/// next home starts without its database: it is listed as installed and as the default.
#[tokio::test]
async fn a_runtime_the_last_home_installed_is_recorded_by_the_next_one() {
    let mut fixture = Fixture::start().await;
    let installed = fixture.client().await.install(VERSION).await;
    assert_eq!(installed["state"], "succeeded", "{installed}");

    fixture.reinstall().await;
    let mut client = fixture.client().await;

    let list = client.call("runtime.list_installed", json!({})).await;
    assert_eq!(list["runtimes"][0]["version"], VERSION, "{list}");
    assert_eq!(list["runtimes"][0]["default"], true, "{list}");
}

/// **And one installed before markers existed**: recorded from the index once its files are there
/// and it runs, in the background so the start is not held up by the network.
#[tokio::test]
async fn a_runtime_from_before_markers_is_recorded_from_the_index() {
    let mut fixture = Fixture::start().await;
    fixture.client().await.install(VERSION).await;
    std::fs::remove_file(
        fixture
            .installed_at(VERSION)
            .join(".mixengine-install.json"),
    )
    .expect("the marker");

    fixture.reinstall().await;
    let mut client = fixture.client().await;

    // Polled, because the pass runs in the background and there is no job to wait on; bounded by
    // PATIENCE so a pass that never records fails with the listing it last saw.
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let list = client.call("runtime.list_installed", json!({})).await;
        if list["runtimes"][0]["version"] == VERSION {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never recorded: {list}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        fixture
            .installed_at(VERSION)
            .join(".mixengine-install.json")
            .is_file(),
        "and it was given its marker, so the next start does not ask the index"
    );
}

/// A directory that fails the check is left, named by `mix doctor`, and nothing is deleted.
#[tokio::test]
async fn a_directory_that_cannot_be_recorded_is_named_by_doctor_and_kept() {
    let mut fixture = Fixture::start().await;
    let stray = fixture.installed_at("9.9.8");
    std::fs::create_dir_all(&stray).expect("a directory nothing installed");
    fixture.reinstall().await;

    let mut client = fixture.client().await;
    // Polled for the reason the test above gives.
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let report = client.call("daemon.doctor", json!({})).await;
        let text = report.to_string();
        if text.contains("9.9.8") {
            assert!(text.contains("not recorded"), "{report}");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "doctor never named it: {report}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(stray.is_dir(), "nothing was deleted");
}

/// **T182f, by hand.** A directory the start walk left — here because its marker names another
/// version — is recorded by `runtime.adopt` once the index vouches for it, and its marker is put
/// right. A version already recorded answers its row, and one not on disk is refused by name.
#[tokio::test]
async fn adopting_one_version_checks_it_against_the_index() {
    let mut fixture = Fixture::start().await;
    fixture.client().await.install(VERSION).await;
    let marker = fixture
        .installed_at(VERSION)
        .join(".mixengine-install.json");
    let written = std::fs::read_to_string(&marker).expect("the marker");
    std::fs::write(&marker, written.replace(VERSION, "8.3.32")).expect("a wrong marker");

    fixture.reinstall().await;
    let mut client = fixture.client().await;
    assert_eq!(
        client.call("runtime.list_installed", json!({})).await["runtimes"],
        json!([]),
        "a marker naming another version is not recorded on its own"
    );

    let adopted = client
        .call("runtime.adopt", json!({"kind": "php", "version": VERSION}))
        .await;
    assert_eq!(adopted["version"], VERSION, "{adopted}");
    assert_eq!(adopted["default"], true, "{adopted}");
    assert!(
        std::fs::read_to_string(&marker)
            .expect("the marker")
            .contains(VERSION),
        "the marker now names what is there"
    );

    let again = client
        .call("runtime.adopt", json!({"kind": "php", "version": VERSION}))
        .await;
    assert_eq!(
        again["version"], VERSION,
        "adopting it twice answers its row: {again}"
    );

    let refused = client
        .refuse("runtime.adopt", json!({"kind": "php", "version": "9.9.7"}))
        .await;
    assert_eq!(refused["data"]["code"], "not_found", "{refused}");
}

/// **T182i.** `runtime.found` lists a directory the start left, with why — what the window draws
/// beside **Adopt** — and after the adopt it is gone.
#[tokio::test]
async fn a_runtime_the_start_left_is_listed_with_why_until_it_is_adopted() {
    let mut fixture = Fixture::start().await;
    fixture.client().await.install(VERSION).await;
    let marker = fixture
        .installed_at(VERSION)
        .join(".mixengine-install.json");
    let written = std::fs::read_to_string(&marker).expect("the marker");
    std::fs::write(&marker, written.replace(VERSION, "8.3.32")).expect("a wrong marker");

    fixture.reinstall().await;
    let mut client = fixture.client().await;

    let found = client.call("runtime.found", json!({})).await;
    let row = &found["found"][0];
    assert_eq!(row["kind"], "php", "{found}");
    assert_eq!(row["version"], VERSION, "{found}");
    assert!(
        row["why"]
            .as_str()
            .is_some_and(|why| why.contains("another install")),
        "{found}"
    );

    client
        .call("runtime.adopt", json!({"kind": "php", "version": VERSION}))
        .await;
    let found = client.call("runtime.found", json!({})).await;
    assert_eq!(
        found["found"],
        json!([]),
        "an adopted version is no longer listed: {found}"
    );
}

/// **T182h.** A copy of the earlier home's state in a kept folder is offered by the next home and
/// restored: the project comes back, the pool the start already made for the recorded PHP is kept
/// rather than duplicated, and the copy is not offered twice.
#[tokio::test]
async fn a_copy_of_the_last_home_is_offered_and_restored() {
    let mut fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.install(VERSION).await;
    let root = fixture.home.path().join("work").join("shop");
    std::fs::create_dir_all(&root).expect("a project folder");
    client
        .call(
            "project.create",
            json!({"name": "shop", "root": root.display().to_string()}),
        )
        .await;
    drop(client);

    // What the uninstaller does when it keeps a folder: a copy of the state, written into it.
    let data = fixture.home.path().join("data");
    std::fs::create_dir_all(&data).expect("the data folder");
    fixture.daemon.take();
    let store = mixengine_core::Store::open(&fixture.home.path().join("mixengine.db"))
        .await
        .expect("the old home's database");
    mixengine_core::adopt::snapshot::write(&store, &data)
        .await
        .expect("a copy");
    store.close().await;

    fixture.reinstall().await;
    let mut client = fixture.client().await;

    let previous = client.call("home.previous", json!({})).await;
    assert_eq!(previous["copy"]["projects"], 1, "{previous}");

    let restored = client.call("home.restore", json!({})).await;
    assert_eq!(restored["projects"], 1, "{restored}");

    let projects = client.call("project.list", json!({})).await;
    assert!(projects.to_string().contains("shop"), "{projects}");

    let again = client.call("home.previous", json!({})).await;
    assert!(
        again["copy"].is_null(),
        "a restored copy is not offered again: {again}"
    );
}

/// **T185b.** A home with no PHP has no `php` in `bin/`; installing one puts it there as the install
/// ends, without waiting for the rescan, and uninstalling the last one takes it away again.
#[tokio::test]
async fn bin_holds_php_only_while_a_php_is_installed() {
    let fixture = Fixture::start().await;
    let php = fixture
        .home
        .path()
        .join("bin")
        .join(format!("php{}", std::env::consts::EXE_SUFFIX));
    assert!(!php.exists(), "a home with no PHP fronts none");

    let mut client = fixture.client().await;
    let installed = client.install(VERSION).await;
    assert_eq!(installed["state"], "succeeded", "{installed}");
    assert!(php.exists(), "the install refreshed bin/ as it ended");

    client
        .call(
            "runtime.uninstall",
            json!({"kind": "php", "version": VERSION}),
        )
        .await;
    assert!(!php.exists(), "the last PHP took its name with it");
}

/// **T185b.** Two Nodes, and only one of them has `yarn`: `bin/yarn` is there while that one is,
/// and removing it takes `yarn` away in the same call — not at some later pass — while `node`
/// stays, because a Node is still installed.
#[tokio::test]
async fn the_last_node_with_a_tool_takes_the_tool_with_it() {
    let mut fixture = Fixture::start().await;
    fixture.record_node("22.20.0").await;
    fixture.record_node("24.19.0").await;
    fixture.install_globally("24.19.0", "yarn");

    // Rows written behind the daemon's back are what a start finds; a restart is that start.
    fixture.restart().await;
    assert!(
        fixture.command("yarn").exists(),
        "a start found the yarn in Node 24"
    );
    assert!(fixture.command("node").exists(), "and fronts node");

    let mut client = fixture.client().await;
    client
        .call(
            "runtime.uninstall",
            json!({"kind": "node", "version": "24.19.0"}),
        )
        .await;

    assert!(
        !fixture.command("yarn").exists(),
        "the uninstall answered with yarn still in bin/, though no Node left has it"
    );
    assert!(
        fixture.command("node").exists(),
        "node went too, though Node 22 is still installed"
    );
}

/// **T185b.** `npm install -g` tells nobody. The daemon hears the bindir change and `bin/` follows,
/// both ways, with nothing asked of it and nothing polling.
#[tokio::test]
async fn a_tool_installed_into_a_runtime_becomes_a_command_unasked() {
    let mut fixture = Fixture::start().await;
    fixture.record_node("22.20.0").await;
    fixture.restart().await;
    assert!(
        !fixture.command("yarn").exists(),
        "nothing has installed yarn yet"
    );

    fixture.install_globally("22.20.0", "yarn");
    fixture.command_becomes("yarn", true).await;

    fixture.uninstall_globally("22.20.0", "yarn");
    fixture.command_becomes("yarn", false).await;
}
