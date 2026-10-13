use super::*;

use mixengine_proto::{BlueprintSource, SignatureCheck};

use crate::blueprints::manifest::BlueprintManifest;

use std::collections::BTreeMap;

use crate::blueprints::manifest::{BlueprintService, BlueprintSite, Header, Php, Provenance};

mod names;
mod resolving;
mod scaffold;

async fn home() -> (tempfile::TempDir, Store) {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let store = Store::open(&temp.path().join("mixengine.db"))
        .await
        .expect("a database");

    (temp, store)
}

/// A `PATH` holding these programs and nothing else, on this system's rule.
///
/// Each is a copy of this test binary under the program's name: the copy keeps the execute
/// bit `execvp` wants, and `EXE_SUFFIX` gives it the extension `cmd.exe` wants — without a
/// `#[cfg]` naming either system, which `workspace_layering` keeps out of this crate.
fn a_path_holding(temp: &tempfile::TempDir, programs: &[&str]) -> std::ffi::OsString {
    let tools = temp.path().join("tools");
    std::fs::create_dir_all(&tools).expect("a tools directory");
    let itself = std::env::current_exe().expect("this test binary");

    for name in programs {
        let file = tools.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(&itself, &file).expect("a program");
    }

    tools.into_os_string()
}

/// A `PATH` holding nothing.
fn nowhere() -> &'static std::ffi::OsStr {
    std::ffi::OsStr::new("")
}

fn a_manifest() -> BlueprintManifest {
    BlueprintManifest {
        schema: 1,
        blueprint: Header {
            name: "blog-stack".to_owned(),
            description: String::new(),
            created_at: "2026-09-01T09:00:00Z".to_owned(),
            created_on: Provenance {
                os: "linux".to_owned(),
                version: "0.1.0".to_owned(),
            },
        },
        runtimes: [(
            RuntimeKind::Php,
            VersionConstraint::parse("8.2.23").expect("a constraint"),
        )]
        .into_iter()
        .collect(),
        sites: vec![BlueprintSite {
            kind: SiteKind::PhpFpm { pool: None },
            doc_root: "public".to_owned(),
            https: true,
            domain_pattern: "{project}.test".to_owned(),
            aliases: Vec::new(),
            routes: Vec::new(),
            services: None,
        }],
        services: vec![BlueprintService {
            name: "mariadb".to_owned(),
            version: Some(VersionConstraint::parse("11.4.3").expect("a constraint")),
            instance: Some("main".to_owned()),
            database: Some("{project}".to_owned()),
            user: Some("{project}".to_owned()),
            dotenv: None,
        }],
        php: Some(Php {
            extensions: vec!["xdebug".to_owned()],
        }),
        scaffold: None,
        archive: None,
        next_steps: Vec::new(),
    }
}

/// A blueprint this machine wrote down itself, which is what nearly every test here is about.
fn captured(manifest: BlueprintManifest) -> Filed {
    Filed {
        manifest,
        source: BlueprintSource::Captured,
        trusted: true,
        signature: None,
    }
}

/// One somebody handed over, with nothing vouching for it.
fn imported(manifest: BlueprintManifest) -> Filed {
    Filed {
        manifest,
        source: BlueprintSource::Imported,
        trusted: false,
        signature: Some(SignatureCheck::Missing),
    }
}

async fn an_installed_php(store: &Store, version: &str, choices: &str) {
    sqlx::query(
        r#"INSERT INTO runtime_installs
                   (id, kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256, extension_choices_json, extensions_json)
               VALUES (1, 'php', ?1, 'stable', '/runtimes/php', '2026-09-01T00:00:00Z', 1,
                       'https://example.invalid/php', 'ab', ?2,
                       '{"shared":["xdebug"],"enabled":[],"compiled_in":[]}')"#,
    )
    .bind(version)
    .bind(choices)
    .execute(store.pool())
    .await
    .expect("a runtime install");
}

/// One more PHP beside [`an_installed_php`]'s, under its own row.
async fn another_installed_php(store: &Store, id: i64, version: &str) {
    sqlx::query(
        r#"INSERT INTO runtime_installs
                   (id, kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256, extension_choices_json, extensions_json)
               VALUES (?1, 'php', ?2, 'stable', '/runtimes/php-other', '2026-09-01T00:00:00Z', 1,
                       'https://example.invalid/php', 'ab', '{}',
                       '{"shared":["xdebug"],"enabled":[],"compiled_in":[]}')"#,
    )
    .bind(id)
    .bind(version)
    .execute(store.pool())
    .await
    .expect("another runtime install");
}

async fn an_installed_mariadb(store: &Store, version: &str) {
    sqlx::query(
        "INSERT INTO packages (id, name, version, install_path, installed_at, source_url, sha256)
             VALUES (1, 'mariadb', ?1, '/packages/mariadb', '2026-09-01T00:00:00Z',
                     'https://example.invalid/m', 'ab')",
    )
    .bind(version)
    .execute(store.pool())
    .await
    .expect("a package");

    sqlx::query(
        "INSERT INTO services (id, package_id, instance_name, state, port)
             VALUES ('mariadb@main', 1, 'main', 'stopped', 3306)",
    )
    .execute(store.pool())
    .await
    .expect("a service");
}

fn step_of(planned: &BlueprintPlan, wanted: impl Fn(&PlanAction) -> bool) -> &PlanStep {
    planned
        .steps
        .iter()
        .find(|step| wanted(&step.action))
        .expect("the step")
}

/// The `InstallPackage` or `EnsureService` step for one package.
///
/// **Since T115 a plan holds more than one of each.** A manifest with a `[site]` applied to a
/// home with no front end plans Caddy as well, so a test asking for "the install step" would
/// get whichever came first and assert about the wrong service.
fn for_package<'a>(planned: &'a BlueprintPlan, name: &str) -> Vec<&'a PlanStep> {
    planned
        .steps
        .iter()
        .filter(|step| match &step.action {
            PlanAction::InstallPackage { package, .. }
            | PlanAction::EnsureService { package, .. } => package == name,
            _ => false,
        })
        .collect()
}

/// That package's `InstallPackage` step.
fn install_of<'a>(planned: &'a BlueprintPlan, name: &str) -> &'a PlanStep {
    for_package(planned, name)
        .into_iter()
        .find(|step| matches!(step.action, PlanAction::InstallPackage { .. }))
        .expect("an install step for this package")
}

/// And its `EnsureService` step.
fn ensure_of<'a>(planned: &'a BlueprintPlan, name: &str) -> &'a PlanStep {
    for_package(planned, name)
        .into_iter()
        .find(|step| matches!(step.action, PlanAction::EnsureService { .. }))
        .expect("an ensure step for this package")
}

async fn planned_for(
    store: &Store,
    temp: &tempfile::TempDir,
    root: &Path,
    manifest: crate::blueprints::manifest::BlueprintManifest,
) -> BlueprintPlan {
    plan(
        store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "wordpress",
            filed: &captured(manifest),
            project: "wp",
            root,
            answers: &[],
            scaffold_path: &a_path_holding(temp, &[]),
            front_end: false,
        },
    )
    .await
    .expect("a plan")
}
