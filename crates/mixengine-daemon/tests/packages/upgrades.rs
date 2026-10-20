//! Updates within a line, and what an upgrade does to a running instance.

use super::*;

/// Two patches of `fakeservice`, the same archive — roadmap task **T193a**.
fn two_patches(packed: &Packed, url: &str) -> Value {
    let mut index = index(packed, url);
    let mut next = index["packages"][0].clone();
    next["version"] = json!("1.0.1");
    index["packages"].as_array_mut().expect("a list").push(next);
    index
}

/// **T193a.** An installed package with a newer patch of its line is named with that patch.
#[tokio::test]
async fn a_package_catalogue_says_lines_and_updates_too() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    let catalogue = client.call("package.list_available", json!({})).await;
    assert_eq!(
        catalogue["updates"],
        json!([{
            "package": PACKAGE, "from": "1.0.0", "to": "1.0.1", "to_installed": false, "needs": []
        }]),
        "{catalogue}"
    );
}

/// An update that kept its old version is not offered again, as for runtimes.
#[tokio::test]
async fn a_package_update_that_kept_its_version_is_not_offered_again() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    let job = client
        .call(
            "package.upgrade",
            json!({"package": PACKAGE, "from": VERSION, "keep": true}),
        )
        .await;
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(finished["state"], "succeeded", "{finished}");
    assert_eq!(
        finished["outcome"]["result"]["old"]["state"], "kept",
        "{finished}"
    );

    let catalogue = client.call("package.list_available", json!({})).await;
    assert_eq!(catalogue["updates"], json!([]), "{catalogue}");
}

/// `fakeservice@main` on `version`, started.
async fn a_running_instance(client: &mut Client, version: &str) {
    client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": version}),
        )
        .await;
    let started = client
        .call(
            "service.start",
            json!({"service": "fakeservice@main", "wait": true}),
        )
        .await;
    assert_eq!(started["complete"], true, "{started}");
}

/// **T193c, D6.** The instance moves, runs again, and the old version goes.
#[tokio::test]
async fn a_package_upgrade_moves_a_running_instance_and_removes_the_old_version() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");
    a_running_instance(&mut client, VERSION).await;

    let plan = client
        .call(
            "package.upgrade_plan",
            json!({"package": PACKAGE, "from": VERSION}),
        )
        .await;
    assert_eq!(plan["to"], "1.0.1", "{plan}");
    assert_eq!(plan["entries"][0]["item"]["item"], "instance", "{plan}");
    assert_eq!(plan["entries"][0]["item"]["restarts"], true, "{plan}");

    let job = client
        .call(
            "package.upgrade",
            json!({"package": PACKAGE, "from": VERSION}),
        )
        .await;
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(finished["state"], "succeeded", "{finished}");
    assert_eq!(
        finished["outcome"]["result"]["old"]["state"], "removed",
        "{finished}"
    );

    let services = client.call("service.list", json!({})).await;
    let main = services["services"]
        .as_array()
        .expect("T193 fixture")
        .iter()
        .find(|service| service["id"] == "fakeservice@main")
        .cloned()
        .unwrap_or_else(|| panic!("{services}"));
    assert_eq!(main["version"], "1.0.1", "{main}");
    assert_eq!(main["state"], "running", "{main}");
}

/// **T193c, Error handling.** A package being moved cannot be uninstalled.
#[tokio::test]
async fn a_package_being_upgraded_cannot_be_uninstalled() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    let upgrading = client
        .call(
            "package.upgrade",
            json!({"package": PACKAGE, "from": VERSION}),
        )
        .await;
    let refused = client
        .refuse(
            "package.uninstall",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    if refused["data"]["code"] != "not_found" {
        assert_eq!(refused["data"]["code"], "conflict", "{refused}");
    }
    client.finished(upgrading["id"].clone()).await;
}

impl Fixture {
    /// `fakeservice` 1.0.0, and a 1.0.1 whose program is not one — T193c, added at execution.
    ///
    /// **A second archive rather than a second name in the first**, because a server's program is
    /// found by its package's name inside the install (`generate::program`), not through
    /// `provides`; and `fakeservice` has no smoke test, so the broken build installs and fails only
    /// when it is started — which is the moment D6 step 4 is about.
    async fn start_with_a_broken_next_patch() -> Self {
        let packing = match cfg!(windows) {
            true => Packing::Zip,
            false => Packing::TarZst,
        };
        let packed = FakePackage::new(packing)
            .executable(&program_name())
            .build(&format!("{PACKAGE}-{VERSION}"));
        let broken = FakePackage::new(packing)
            .file(&program_name(), b"not a program")
            .build(&format!("{PACKAGE}-1.0.1"));

        let registry = MockRegistry::start(&json!({
            "schema": 1,
            "generated_at": "2026-09-29T06:55:12Z",
            "packages": [],
        }))
        .await;

        let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
        let broken_url = registry.publish_asset(&broken.path(), broken.bytes.clone());

        let mut published = index(&packed, &url);
        let mut next = published["packages"][0].clone();
        next["version"] = json!("1.0.1");
        next["artifacts"][0]["url"] = json!(broken_url);
        next["artifacts"][0]["sha256"] = json!(broken.sha256);
        next["artifacts"][0]["size"] = json!(broken.size());
        published["packages"]
            .as_array_mut()
            .expect("a list")
            .push(next);
        registry.publish(&published);

        let home = Home::new();
        let daemon = started(&home, &registry);
        home.wait_until_listening().await;

        Self {
            home,
            _registry: registry,
            _daemon: daemon,
            packed,
        }
    }
}

/// **T193c, D6 step 4 — added at execution.** An instance that does not start on the new patch is
/// put back on the old one and started again, and the old version is kept.
///
/// **And the job fails**, saying so — the final review's finding: a job that ended `succeeded`
/// here let `mix package upgrade` exit zero and MixLab say nothing, over an update that did not
/// happen.
#[tokio::test]
async fn a_package_that_does_not_start_on_the_new_patch_goes_back() {
    let fixture = Fixture::start_with_a_broken_next_patch().await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");
    a_running_instance(&mut client, VERSION).await;

    let job = client
        .call(
            "package.upgrade",
            json!({"package": PACKAGE, "from": VERSION}),
        )
        .await;
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(
        finished["state"],
        "failed",
        "an update that moved nothing is a failed job\n{finished}\n--- daemon ---\n{}",
        fixture.home.daemon_log()
    );
    let said = finished["outcome"]["error"]["message"]
        .as_str()
        .unwrap_or_default();
    assert!(
        said.contains("fakeservice@main") && said.contains(VERSION),
        "the error names the instance and the version it is back on: {finished}"
    );

    let services = client.call("service.list", json!({})).await;
    let main = services["services"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|service| service["id"] == "fakeservice@main")
        .cloned()
        .unwrap_or_else(|| panic!("{services}"));
    assert_eq!(main["version"], VERSION, "back on the old patch: {main}");
    assert_eq!(main["state"], "running", "and running again: {main}");
}

/// `package.list_available` names the package whose file could not be read — roadmap task
/// **T196**, its design's D5, the twin of the runtime test.
#[tokio::test]
async fn a_package_that_cannot_be_read_is_named() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    fixture._registry.corrupt_kind(PACKAGE);

    let listed = client
        .call("package.list_available", json!({"refresh": true}))
        .await;

    assert_eq!(
        listed["packages"].as_array().map(Vec::len),
        Some(0),
        "{listed}"
    );
    let gaps = listed["unavailable"]
        .as_array()
        .expect("a daemon that says what it could not read");
    assert_eq!(gaps.len(), 1, "{listed}");
    assert_eq!(gaps[0]["name"], PACKAGE);

    let refused = client
        .refuse(
            "package.requirements",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("could not be read for")),
        "{refused}"
    );
}
