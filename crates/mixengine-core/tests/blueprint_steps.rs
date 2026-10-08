//! `projects.blueprint_id` and what is read through it — roadmap task **T205**, D5.

use std::collections::BTreeMap;

use mixengine_core::blueprints::trust::Trust;
use mixengine_core::blueprints::{manifest, steps, store as blueprint_store};
use mixengine_core::{Paths, Store, open_home, projects};
use mixengine_proto::BlueprintSource;
use tempfile::TempDir;

/// A blueprint named `name` with one step.
fn with_steps(name: &str) -> String {
    format!(
        "schema = 1\n\n[blueprint]\nname = \"{name}\"\ncreated_at = \"2026-10-08T00:00:00Z\"\n\n\
         [blueprint.created_on]\nos = \"any\"\nversion = \"0.0.1\"\n\n\
         [[next_steps]]\nkind = \"once\"\nrun = \"php artisan app:install {{project}}\"\n"
    )
}

/// `tests/blueprint_gallery.rs`' helper, unchanged.
async fn home() -> (TempDir, Paths, Store) {
    let temp = TempDir::new().expect("a temporary directory");
    let opened = open_home(
        None,
        &mixengine_platform::mock::Host::with_home(temp.path().join("MixEngine")),
    )
    .expect("a home");
    let store = Store::open(opened.paths.database_file())
        .await
        .expect("a database");

    (temp, opened.paths, store)
}

async fn filed(store: &Store, paths: &Paths, slug: &str, text: &str, trust: Trust) {
    blueprint_store::save(
        store,
        paths,
        &manifest::read(text).expect("reads"),
        slug,
        BlueprintSource::Imported,
        trust,
        true,
    )
    .await
    .expect("filed");
}

/// `plan.rs`' `a_project` test helper, returning the whole record.
async fn project(store: &Store, temp: &TempDir, name: &str) -> projects::ProjectRecord {
    let root = temp.path().join(name);
    std::fs::create_dir_all(&root).expect("a directory");

    projects::create(
        store,
        &projects::Registration {
            name: name.to_owned(),
            root,
            pins: BTreeMap::new(),
        },
        mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
    )
    .await
    .expect("a project")
}

#[tokio::test]
async fn a_project_reads_its_steps_through_its_blueprint() {
    let (temp, paths, store) = home().await;
    filed(&store, &paths, "mine", &with_steps("x"), Trust::Inherent).await;
    let app = project(&store, &temp, "app").await;

    projects::adopt_blueprint(&store, "app", "mine")
        .await
        .expect("adopted");

    let read = steps::of_project(&store, &app)
        .await
        .expect("read")
        .expect("some");
    assert!(read.trusted);
    assert_eq!(
        read.steps[0].run.as_deref(),
        Some("php artisan app:install app")
    );

    let (declared, _) = steps::declared(&store, app.id)
        .await
        .expect("read")
        .expect("some");
    assert_eq!(
        declared.next_steps[0].run.as_deref(),
        Some("php artisan app:install {project}"),
        "stored unexpanded, so capture writes it back unchanged"
    );
}

#[tokio::test]
async fn adopting_never_replaces_a_blueprint_already_named() {
    let (temp, paths, store) = home().await;
    filed(&store, &paths, "one", &with_steps("first"), Trust::Inherent).await;
    filed(
        &store,
        &paths,
        "two",
        &with_steps("second"),
        Trust::Inherent,
    )
    .await;
    let app = project(&store, &temp, "app").await;

    projects::adopt_blueprint(&store, "app", "one")
        .await
        .expect("adopted");
    projects::adopt_blueprint(&store, "app", "two")
        .await
        .expect("a no-op");

    let (manifest, _) = steps::declared(&store, app.id)
        .await
        .expect("read")
        .expect("some");
    assert_eq!(manifest.blueprint.name, "first");
}

#[tokio::test]
async fn an_unsigned_overwrite_turns_trust_off() {
    let (temp, paths, store) = home().await;
    filed(&store, &paths, "mine", &with_steps("x"), Trust::Inherent).await;
    let app = project(&store, &temp, "app").await;
    projects::adopt_blueprint(&store, "app", "mine")
        .await
        .expect("adopted");

    // What `blueprint.import --overwrite` of a file with no signature leaves.
    filed(&store, &paths, "mine", &with_steps("x"), Trust::Unsigned).await;

    let read = steps::of_project(&store, &app)
        .await
        .expect("read")
        .expect("some");
    assert!(!read.trusted);
}

#[tokio::test]
async fn a_project_with_no_blueprint_has_no_steps() {
    let (temp, _paths, store) = home().await;
    let app = project(&store, &temp, "app").await;
    assert_eq!(steps::of_project(&store, &app).await.expect("read"), None);
}
