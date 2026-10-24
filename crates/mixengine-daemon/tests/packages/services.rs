//! A package offered, installed, made a service and removed.

use super::*;

/// The whole of `package.*` in one test, because it is one sequence: a version is offered,
/// installed, listed and removed — and the directory on disk agrees at every step.
#[tokio::test]
async fn a_service_package_is_offered_installed_listed_and_removed() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    // Offered, and not yet here.
    let available = client.call("package.list_available", json!({})).await;
    let offered = &available["packages"];
    assert_eq!(
        offered.as_array().map(Vec::len),
        Some(1),
        "the package this build has no recipe for is not offered: {available}"
    );
    assert_eq!(offered[0]["package"], PACKAGE);
    assert_eq!(offered[0]["version"], VERSION);
    assert_eq!(offered[0]["installed"], false);
    assert_eq!(offered[0]["bytes"], fixture.packed.size());
    assert_eq!(
        available["stale"], false,
        "the registry answered, so nothing came out of a cache"
    );

    assert_eq!(
        client.call("package.list", json!({})).await["packages"],
        json!([]),
        "a home that has installed nothing lists nothing rather than failing"
    );

    // Installed.
    let installed = client.install(VERSION).await;
    assert_eq!(installed["state"], "succeeded", "{installed}");

    let package = &installed["outcome"]["result"];
    assert_eq!(package["package"], PACKAGE);
    assert_eq!(package["version"], VERSION);
    assert_eq!(
        package["services"],
        json!([]),
        "nothing can be an instance of a version installed a moment ago"
    );
    assert!(
        fixture.installed_at(VERSION).is_dir(),
        "the archive was unpacked where the row says it is"
    );

    // Listed, and no longer offered as something to install.
    let list = client.call("package.list", json!({})).await;
    assert_eq!(list["packages"].as_array().map(Vec::len), Some(1), "{list}");
    assert_eq!(list["packages"][0]["package"], PACKAGE);

    let available = client.call("package.list_available", json!({})).await;
    assert_eq!(
        available["packages"][0]["installed"], true,
        "the daemon composes this rather than leaving a client to cross-reference: {available}"
    );

    // Removed, directory and row together.
    let removal = client
        .call(
            "package.uninstall",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    assert_eq!(removal["removed"]["version"], VERSION);
    assert!(
        !fixture.installed_at(VERSION).exists(),
        "the directory goes with the row"
    );
    assert!(
        !fixture
            .installed_at(VERSION)
            .parent()
            .expect("a parent")
            .exists(),
        "the package's own folder goes with its last version"
    );
    assert_eq!(
        client.call("package.list", json!({})).await["packages"],
        json!([])
    );
}

/// A kind this build has no recipe for is refused at install, not at create — nobody spends a
/// download on a directory MixEngine could never start anything out of.
#[tokio::test]
async fn a_package_this_build_cannot_run_is_refused_with_what_it_can() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let error = client
        .refuse(
            "package.install",
            json!({"package": UNRUNNABLE, "version": UNRUNNABLE_VERSION}),
        )
        .await;

    assert_eq!(error["data"]["code"], "invalid_argument", "{error}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains(UNRUNNABLE)),
        "it names what was asked for: {error}"
    );
    assert!(
        error["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("caddy")),
        "and what exists instead: {error}"
    );
}

/// The same rule seen from the listing side, filtered rather than surveyed.
#[tokio::test]
async fn a_filter_naming_something_unrunnable_is_refused_rather_than_answered_empty() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let error = client
        .refuse("package.list_available", json!({"package": UNRUNNABLE}))
        .await;

    assert_eq!(error["data"]["code"], "invalid_argument", "{error}");
}

/// Installing the same version twice is two terminals, and the second is asking for the outcome the
/// first already reached.
#[tokio::test]
async fn installing_a_version_that_is_already_here_says_so_rather_than_downloading_it_again() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    let error = client
        .refuse(
            "package.install",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;

    assert_eq!(error["data"]["code"], "already_exists", "{error}");
}

/// **A folder deleted by hand reads as missing, and installing it again restores the same row.**
#[tokio::test]
async fn a_deleted_package_folder_is_missing_and_installing_it_again_restores_it() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");
    client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;

    std::fs::remove_dir_all(fixture.installed_at(VERSION)).expect("deleted by hand");

    let list = client.call("package.list", json!({})).await;
    assert_eq!(list["packages"][0]["missing"], true, "{list}");

    let restored = client.install(VERSION).await;
    assert_eq!(restored["state"], "succeeded", "{restored}");
    assert_eq!(
        restored["outcome"]["result"]["missing"], false,
        "{restored}"
    );
    assert_eq!(
        restored["outcome"]["result"]["services"],
        json!(["fakeservice@main"]),
        "the service still points at the same row"
    );
    assert!(fixture.installed_at(VERSION).is_dir());
}

/// **A version whose folder is gone is offered again.** This is what locks MixLab's steps panel
/// again over a devkit deleted by hand.
#[tokio::test]
async fn a_deleted_package_folder_is_offered_as_not_installed() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    std::fs::remove_dir_all(fixture.installed_at(VERSION)).expect("deleted by hand");

    let available = client.call("package.list_available", json!({})).await;
    assert_eq!(available["packages"][0]["installed"], false, "{available}");
}

/// The whole point of the task: an installed package becomes a service a person can start.
#[tokio::test]
async fn an_installed_package_becomes_a_service_and_can_be_deleted_again() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    let created = client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;
    assert_eq!(created["service"]["id"], "fakeservice@main");
    assert_eq!(created["service"]["state"], "stopped", "{created}");
    // A number, and not *the* number: the fixture recipe wishes for 41000, and `free` in this
    // workspace means free on the machine rather than free in a home of its own — anything else on
    // the runner holding it moves this service up, which is the allocator working. What the
    // allocator's own suite proves about which number lands is proved in
    // `mixengine_core::services::ports`.
    assert!(
        created["service"]["port"].is_u64(),
        "a service of a recipe that names a port was given none: {created}"
    );
    assert!(
        fixture.etc_for("fakeservice@main").is_dir(),
        "a create renders before it answers"
    );

    let list = client.call("service.list", Value::Null).await;
    assert_eq!(
        list["services"].as_array().map(Vec::len),
        Some(1),
        "a created service is a declared service: {list}"
    );

    // And the package it is an instance of is no longer one that can be removed.
    let held = client
        .refuse(
            "package.uninstall",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    assert_eq!(held["data"]["code"], "precondition_failed", "{held}");
    assert!(
        held["message"]
            .as_str()
            .is_some_and(|message| message.contains("fakeservice@main")),
        "it names what holds it: {held}"
    );

    let removal = client
        .call("service.delete", json!({"service": "fakeservice@main"}))
        .await;
    assert_eq!(removal["removed"]["id"], "fakeservice@main");
    assert!(
        !fixture.etc_for("fakeservice@main").exists(),
        "generated configuration is disposable and goes with the row"
    );
    assert_eq!(
        client.call("service.list", Value::Null).await["services"],
        json!([])
    );

    // Which frees the package.
    let removed = client
        .call(
            "package.uninstall",
            json!({"package": PACKAGE, "version": VERSION}),
        )
        .await;
    assert_eq!(removed["removed"]["version"], VERSION);
}

/// The recipe says how many instances it has, and the id is where a person meets the answer.
#[tokio::test]
async fn a_named_instance_recipe_refuses_an_id_with_no_instance() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    let error = client
        .refuse("service.create", json!({"id": PACKAGE, "version": VERSION}))
        .await;

    assert_eq!(error["data"]["code"], "invalid_argument", "{error}");
    assert!(
        error["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("fakeservice@")),
        "it shows the shape: {error}"
    );
}

/// A version nobody installed is a missing step rather than a mistake, and the hint is the step.
#[tokio::test]
async fn creating_a_service_from_a_package_that_is_not_installed_names_the_install() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let error = client
        .refuse(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;

    assert_eq!(error["data"]["code"], "precondition_failed", "{error}");
    assert!(
        error["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("package install")),
        "{error}"
    );
}

/// One row that cannot be rendered fails the whole declared set, so a bad row left behind would take
/// `service.list` down with it.
#[tokio::test]
async fn a_create_that_cannot_be_rendered_leaves_the_home_as_it_was() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    let error = client
        .refuse(
            "service.create",
            json!({
                "id": "fakeservice@bad",
                "version": VERSION,
                "overrides": {"exit_afterr": 1},
            }),
        )
        .await;
    assert_eq!(error["data"]["code"], "invalid_argument", "{error}");

    let list = client.call("service.list", Value::Null).await;
    assert_eq!(
        list["services"],
        json!([]),
        "the row went with the failure: {list}"
    );
}

/// A delete keeps the data directory, and says which one it kept.
#[tokio::test]
async fn a_delete_keeps_the_data_directory_and_says_so() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;

    // Created by hand rather than by starting the service: what is being tested is that a delete
    // leaves a directory alone, and the cheapest way to have one is to make one.
    let data = fixture.home.path().join("data").join(PACKAGE).join("main");
    std::fs::create_dir_all(&data).expect("a data directory in a temporary home");

    let removal = client
        .call("service.delete", json!({"service": "fakeservice@main"}))
        .await;

    let kept = removal["data_kept"]
        .as_str()
        .unwrap_or_else(|| panic!("a data directory is named: {removal}"));
    assert!(kept.ends_with("main"), "{kept}");
    assert!(data.is_dir(), "it is named because it is still there");
}

/// A row deleted out from under a live process would leave the process with nothing describing it.
#[tokio::test]
async fn a_running_service_is_not_deleted_out_from_under_itself() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;

    let walk = client
        .call("service.start", json!({"service": "fakeservice@main"}))
        .await;
    assert_eq!(walk["reached"], json!(["fakeservice@main"]), "{walk}");

    let error = client
        .refuse("service.delete", json!({"service": "fakeservice@main"}))
        .await;

    assert_eq!(error["data"]["code"], "precondition_failed", "{error}");
    assert!(
        error["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("service stop")),
        "{error}"
    );

    // And it goes once it is stopped, which is what makes the refusal a step rather than a wall.
    client
        .call("service.stop", json!({"service": "fakeservice@main"}))
        .await;
    client
        .call("service.delete", json!({"service": "fakeservice@main"}))
        .await;
}

/// A site declaring a service is a statement about the future, and `--force` crosses it. After the
/// delete the site is still there with the link gone — the cascade, not a second write — which is
/// the whole reason a pool is an `Option`.
#[tokio::test]
async fn a_service_a_site_declares_is_refused_and_then_forced() {
    let fixture = Fixture::started_with_package().await;
    let mut client = fixture.client().await;

    client
        .call(
            "service.create",
            json!({"id": "fakeservice@main", "version": VERSION}),
        )
        .await;

    let root = fixture.home.path().join("blog");
    std::fs::create_dir_all(&root).expect("a project directory in a temporary home");

    client
        .call(
            "project.create",
            json!({"root": root.display().to_string(), "name": "blog"}),
        )
        .await;

    // `static`, because what is being tested is the link rather than PHP: a php-fpm site would ask
    // the resolver for a runtime this fixture deliberately has none of.
    client
        .call(
            "site.create",
            json!({
                "project": {"name": "blog"},
                "domains": ["blog.test"],
                "kind": {"kind": "static"},
                "services": ["fakeservice@main"],
            }),
        )
        .await;

    let refused = client
        .refuse("service.delete", json!({"service": "fakeservice@main"}))
        .await;
    assert_eq!(refused["data"]["code"], "precondition_failed", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("blog.test")),
        "it names the site that declares it: {refused}"
    );

    client
        .call(
            "service.delete",
            json!({"service": "fakeservice@main", "force": true}),
        )
        .await;

    let site = client
        .call("site.show", json!({"site": {"domain": "blog.test"}}))
        .await;
    assert_eq!(
        site["services"],
        json!([]),
        "the link went with the service: {site}"
    );
}
