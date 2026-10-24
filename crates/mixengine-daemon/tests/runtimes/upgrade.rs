//! What updates: the catalogue's newest per line, upgrade plans, and the upgrades themselves.

use super::*;

/// Two patches of one line, and one release of another, all the same archive.
fn two_patches(packed: &Packed, url: &str) -> Value {
    let entry = |version: &str| {
        json!({
            "kind": "php",
            "version": version,
            "channel": "stable",
            "artifacts": [{
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "url": url,
                "sha256": packed.sha256,
                "size": packed.size(),
                // A pool renders only for an install that publishes its server, so the one
                // program stands in for all three.
                "provides": {
                    "php": program_name(),
                    "php-cgi": program_name(),
                    "php-fpm": program_name(),
                },
            }],
        })
    };

    json!({
        "schema": 1,
        "generated_at": "2026-09-29T06:55:12Z",
        "packages": [entry("8.3.33"), entry("8.3.34"), entry("8.2.29")],
    })
}

/// Three patches of one line, from the same archive.
fn three_patches(packed: &Packed, url: &str) -> Value {
    let mut index = two_patches(packed, url);
    let mut first = index["packages"][0].clone();
    first["version"] = json!("8.3.32");
    index["packages"]
        .as_array_mut()
        .expect("a list")
        .push(first);
    index
}

/// Every release says its line, one per line says it is the newest, and an installed version with a
/// newer patch is named with that patch.
#[tokio::test]
async fn the_catalogue_says_each_line_its_newest_and_what_updates() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;

    let job = client
        .call(
            "runtime.install",
            json!({"kind": "php", "version": "8.3.33"}),
        )
        .await;
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(finished["state"], "succeeded", "{finished}");

    let catalogue = client.call("runtime.list_available", json!({})).await;
    let row = |version: &str| {
        catalogue["runtimes"]
            .as_array()
            .expect("runtime.list_available answers a list")
            .iter()
            .find(|row| row["version"] == version)
            .cloned()
            .unwrap_or_else(|| panic!("{version} is listed: {catalogue}"))
    };

    assert_eq!(row("8.3.33")["line"], "8.3");
    assert_eq!(row("8.3.33")["newest_in_line"], false);
    assert_eq!(row("8.3.34")["newest_in_line"], true);
    assert_eq!(row("8.2.29")["line"], "8.2");
    assert_eq!(row("8.2.29")["newest_in_line"], true);

    assert_eq!(
        catalogue["updates"],
        json!([{
            "kind": "php", "from": "8.3.33", "to": "8.3.34", "to_installed": false, "needs": []
        }]),
        "{catalogue}"
    );
}

/// **Two installed patches of one line each get their own entry** — the plan's review focus 2.
#[tokio::test]
async fn two_installed_patches_of_one_line_each_get_an_update() {
    let fixture = Fixture::start_with(three_patches).await;
    let mut client = fixture.client().await;

    for version in ["8.3.32", "8.3.33"] {
        let job = client
            .call(
                "runtime.install",
                json!({"kind": "php", "version": version}),
            )
            .await;
        assert_eq!(
            client.finished(job["id"].clone()).await["state"],
            "succeeded"
        );
    }

    let catalogue = client.call("runtime.list_available", json!({})).await;
    let updates = catalogue["updates"].as_array().expect("a list");
    assert_eq!(updates.len(), 2, "{catalogue}");
    assert!(
        updates.iter().all(|update| update["to"] == "8.3.34"),
        "{catalogue}"
    );
}

/// A project `blog` with one PHP site on `pool`.
async fn a_php_site(fixture: &Fixture, client: &mut Client, pool: &str) {
    let root = fixture.home.path().join("blog");
    std::fs::create_dir_all(&root).expect("the project directory");
    client
        .call(
            "project.create",
            json!({"root": root.display().to_string(), "name": "blog"}),
        )
        .await;
    client
        .call(
            "site.create",
            json!({
                "project": {"name": "blog"},
                "domains": ["blog.test"],
                "kind": {"kind": "php-fpm", "pool": pool},
            }),
        )
        .await;
}

/// What is not an update is refused before anything else is read.
#[tokio::test]
async fn an_upgrade_plan_refuses_what_is_not_an_update() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    let job = client
        .call(
            "runtime.install",
            json!({"kind": "php", "version": "8.3.33"}),
        )
        .await;
    assert_eq!(
        client.finished(job["id"].clone()).await["state"],
        "succeeded"
    );

    let not_installed = client
        .refuse(
            "runtime.upgrade_plan",
            json!({"kind": "php", "from": "8.2.29"}),
        )
        .await;
    assert_eq!(
        not_installed["data"]["code"], "invalid_argument",
        "{not_installed}"
    );

    let other_line = client
        .refuse(
            "runtime.upgrade_plan",
            json!({"kind": "php", "from": "8.3.33", "to": "8.2.29"}),
        )
        .await;
    assert_eq!(
        other_line["data"]["code"], "invalid_argument",
        "{other_line}"
    );
    assert!(
        other_line["message"]
            .as_str()
            .expect("the refusal has a message")
            .contains("switch"),
        "{other_line}"
    );
}

/// A plan names the site that moves and the default, and changes nothing.
#[tokio::test]
async fn an_upgrade_plan_names_what_moves_and_writes_nothing() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    let job = client
        .call(
            "runtime.install",
            json!({"kind": "php", "version": "8.3.33"}),
        )
        .await;
    assert_eq!(
        client.finished(job["id"].clone()).await["state"],
        "succeeded"
    );
    a_php_site(&fixture, &mut client, "php-fpm@8.3.33").await;

    let plan = client
        .call(
            "runtime.upgrade_plan",
            json!({"kind": "php", "from": "8.3.33"}),
        )
        .await;

    assert_eq!(plan["to"], "8.3.34", "{plan}");
    assert_eq!(plan["to_installed"], false);
    let items: Vec<&str> = plan["entries"]
        .as_array()
        .expect("the plan lists its entries")
        .iter()
        .map(|entry| entry["item"]["item"].as_str().expect("an item name"))
        .collect();
    assert!(items.contains(&"site"), "{plan}");
    assert!(items.contains(&"default"), "{plan}");
    assert_eq!(plan["old"]["state"], "will_be_removed", "{plan}");

    let site = client
        .call("site.show", json!({"site": {"domain": "blog.test"}}))
        .await;
    assert!(
        site.to_string().contains("php-fpm@8.3.33"),
        "a plan moved nothing: {site}"
    );
}

/// The site moves to the new pool, the default follows, and the old version is gone.
#[tokio::test]
async fn an_upgrade_moves_the_site_and_the_default_and_removes_the_old_version() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    let job = client
        .call(
            "runtime.install",
            json!({"kind": "php", "version": "8.3.33"}),
        )
        .await;
    assert_eq!(
        client.finished(job["id"].clone()).await["state"],
        "succeeded"
    );
    a_php_site(&fixture, &mut client, "php-fpm@8.3.33").await;

    let job = client
        .call("runtime.upgrade", json!({"kind": "php", "from": "8.3.33"}))
        .await;
    assert_eq!(job["kind"], "runtime.upgrade");
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(finished["state"], "succeeded", "{finished}");
    assert_eq!(
        finished["outcome"]["result"]["old"]["state"], "removed",
        "{finished}"
    );

    let site = client
        .call("site.show", json!({"site": {"domain": "blog.test"}}))
        .await;
    assert!(site.to_string().contains("php-fpm@8.3.34"), "{site}");

    let installed = client
        .call("runtime.list_installed", json!({"kind": "php"}))
        .await;
    let versions: Vec<&str> = installed["runtimes"]
        .as_array()
        .expect("runtime.list_installed answers a list")
        .iter()
        .map(|row| row["version"].as_str().expect("each row names its version"))
        .collect();
    assert_eq!(versions, ["8.3.34"], "{installed}");
    assert_eq!(installed["runtimes"][0]["default"], true, "{installed}");
}

#[tokio::test]
async fn an_upgrade_asked_to_keep_keeps_the_old_version() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    let job = client
        .call(
            "runtime.install",
            json!({"kind": "php", "version": "8.3.33"}),
        )
        .await;
    assert_eq!(
        client.finished(job["id"].clone()).await["state"],
        "succeeded"
    );

    let job = client
        .call(
            "runtime.upgrade",
            json!({"kind": "php", "from": "8.3.33", "keep": true}),
        )
        .await;
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(
        finished["outcome"]["result"]["old"]["state"], "kept",
        "{finished}"
    );

    let installed = client
        .call("runtime.list_installed", json!({"kind": "php"}))
        .await;
    assert_eq!(
        installed["runtimes"]
            .as_array()
            .expect("runtime.list_installed answers a list")
            .len(),
        2,
        "{installed}"
    );
}

/// **An update that kept its old version is not offered again**: `8.3.33` stays beside `8.3.34`
/// because it was asked to, and offering the same move for as long as it stays would only ask the
/// question that was just answered. Once `8.3.34` is gone, the offer is back.
#[tokio::test]
async fn an_update_that_kept_its_version_is_not_offered_again() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    let job = client
        .call(
            "runtime.install",
            json!({"kind": "php", "version": "8.3.33"}),
        )
        .await;
    assert_eq!(
        client.finished(job["id"].clone()).await["state"],
        "succeeded"
    );

    let job = client
        .call(
            "runtime.upgrade",
            json!({"kind": "php", "from": "8.3.33", "keep": true}),
        )
        .await;
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(finished["state"], "succeeded", "{finished}");

    let catalogue = client.call("runtime.list_available", json!({})).await;
    assert_eq!(catalogue["updates"], json!([]), "{catalogue}");

    client
        .call(
            "runtime.uninstall",
            json!({"kind": "php", "version": "8.3.34"}),
        )
        .await;
    let catalogue = client.call("runtime.list_available", json!({})).await;
    assert_eq!(
        catalogue["updates"],
        json!([{
            "kind": "php", "from": "8.3.33", "to": "8.3.34", "to_installed": false, "needs": []
        }]),
        "{catalogue}"
    );
}

/// **Review focus 4.**
#[tokio::test]
async fn an_upgrade_to_an_installed_version_moves_without_downloading() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    for version in ["8.3.33", "8.3.34"] {
        let job = client
            .call(
                "runtime.install",
                json!({"kind": "php", "version": version}),
            )
            .await;
        assert_eq!(
            client.finished(job["id"].clone()).await["state"],
            "succeeded"
        );
    }

    let plan = client
        .call(
            "runtime.upgrade_plan",
            json!({"kind": "php", "from": "8.3.33"}),
        )
        .await;
    assert_eq!(plan["to_installed"], true, "{plan}");
    assert_eq!(plan["bytes"], 0, "{plan}");

    let job = client
        .call("runtime.upgrade", json!({"kind": "php", "from": "8.3.33"}))
        .await;
    assert_eq!(
        client.finished(job["id"].clone()).await["state"],
        "succeeded"
    );
}

/// An uninstall of a version an upgrade is moving is refused.
#[tokio::test]
async fn a_version_being_upgraded_cannot_be_uninstalled() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    let job = client
        .call(
            "runtime.install",
            json!({"kind": "php", "version": "8.3.33"}),
        )
        .await;
    assert_eq!(
        client.finished(job["id"].clone()).await["state"],
        "succeeded"
    );

    let upgrading = client
        .call("runtime.upgrade", json!({"kind": "php", "from": "8.3.33"}))
        .await;
    let refused = client
        .refuse(
            "runtime.uninstall",
            json!({"kind": "php", "version": "8.3.33"}),
        )
        .await;
    // On a fast machine the upgrade can finish before this call lands, and then 8.3.33 is simply
    // gone; only a conflict says anything about the rule, so that is what is asserted when it
    // is not `not_found`.
    if refused["data"]["code"] != "not_found" {
        assert_eq!(refused["data"]["code"], "conflict", "{refused}");
    }
    client.finished(upgrading["id"].clone()).await;
}

/// The phpMyAdmin fixture, with its `[web-app.runtime].requires` set to `requires`.
fn web_app_requiring(requires: &str) -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix("mixengine-web-app")
        .tempdir()
        .expect("a temporary directory");
    let manifest = mixengine_testkit::extension::PHPMYADMIN
        .replace("requires = \"^8.0\"", &format!("requires = \"{requires}\""));
    assert!(
        manifest.contains(requires),
        "the fixture's requires line moved"
    );
    std::fs::write(directory.path().join("extension.toml"), manifest).expect("a manifest");
    std::fs::create_dir_all(directory.path().join("phpMyAdmin-5.2.3-all-languages"))
        .expect("a doc root");

    directory
}

/// A `web-app` whose `requires` only the old patch satisfies stays on it, and the old version is
/// kept for it.
#[tokio::test]
async fn a_web_app_that_needs_the_old_patch_keeps_it() {
    let fixture = Fixture::start_with(two_patches).await;
    let mut client = fixture.client().await;
    let job = client
        .call(
            "runtime.install",
            json!({"kind": "php", "version": "8.3.33"}),
        )
        .await;
    assert_eq!(
        client.finished(job["id"].clone()).await["state"],
        "succeeded"
    );

    // The fixture declares a database, so this home has to run one.
    declare::database(
        &fixture.home.database_file(),
        "mariadb@main",
        "mariadb",
        3306,
    )
    .await;

    let directory = web_app_requiring("8.3.33");
    let source = json!({"type": "path", "path": directory.path().display().to_string()});
    let plan = client
        .call("extension.plan", json!({"source": source}))
        .await;
    let started = client
        .call(
            "extension.install",
            json!({
                "source": source,
                "consent": {
                    "id": plan["id"],
                    "version": plan["version"],
                    "signed": plan["signed"],
                    "network": plan["permissions"]["network"],
                },
            }),
        )
        .await;
    let installed = client.finished(started["id"].clone()).await;
    assert_eq!(
        installed["state"],
        "succeeded",
        "{installed}\n{}",
        fixture.home.daemon_log()
    );

    let upgrade = client
        .call(
            "runtime.upgrade_plan",
            json!({"kind": "php", "from": "8.3.33"}),
        )
        .await;
    let pool = upgrade["entries"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|entry| entry["item"]["item"] == "extension_pool")
        .cloned()
        .unwrap_or_else(|| panic!("the web-app's pool is in the plan: {upgrade}"));
    assert_eq!(pool["item"]["pool"], "php-fpm@phpmyadmin", "{upgrade}");
    assert_eq!(pool["item"]["moves"], false, "{upgrade}");
    assert_eq!(upgrade["old"]["state"], "will_be_kept", "{upgrade}");

    let job = client
        .call("runtime.upgrade", json!({"kind": "php", "from": "8.3.33"}))
        .await;
    let finished = client.finished(job["id"].clone()).await;
    assert_eq!(finished["state"], "succeeded", "{finished}");
    assert_eq!(
        finished["outcome"]["result"]["old"]["state"], "kept",
        "{finished}"
    );

    let listed = client
        .call("runtime.list_installed", json!({"kind": "php"}))
        .await;
    assert_eq!(
        listed["runtimes"].as_array().expect("a list").len(),
        2,
        "8.3.33 stays for the web-app: {listed}"
    );
}
