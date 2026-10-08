//! The blueprints this build ships — roadmap task **T79**.
//!
//! Out here rather than beside the code because these are assertions about the *shipped set*: that
//! the files are readable, that each one is its own rendering, and that seeding a real home
//! with them is idempotent.

use mixengine_core::blueprints::gallery::{self, ENTRIES};
use mixengine_core::blueprints::manifest;
use mixengine_core::blueprints::plan::plan;
use mixengine_core::blueprints::store as blueprint_store;
use mixengine_core::blueprints::store::Filed;
use mixengine_core::blueprints::trust::Trust;
use mixengine_core::{Paths, Store, open_home};
use mixengine_proto::{BlueprintSource, Disposition, PlanAction, RuntimeKind};
use tempfile::TempDir;

/// **Every gallery file is exactly what the renderer would write** — the T79 design, D2. Without
/// this the file in this repository, the `manifest_toml` column and the file in a user's home are
/// three different texts for one blueprint, and a `diff` between any two of them means nothing.
#[test]
fn every_gallery_blueprint_is_its_own_rendering() {
    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest)
            .unwrap_or_else(|error| panic!("{} does not parse: {error}", entry.slug));

        assert_eq!(
            manifest::render(&manifest),
            entry.manifest,
            "{} is not canonical — replace the file with what `render` returns",
            entry.slug
        );
    }
}

/// The set the roadmap names, spelled the way a person types it on a command line.
#[test]
fn the_gallery_is_the_set_the_roadmap_names() {
    let slugs: Vec<_> = ENTRIES.iter().map(|entry| entry.slug).collect();

    assert_eq!(
        slugs,
        [
            "cakephp",
            "codeigniter",
            "craft",
            "django",
            "drupal",
            "express-mongodb",
            "laravel",
            "laravel-mongodb",
            "nextjs",
            "php-mysql",
            "rails",
            "statamic",
            "static",
            "strapi",
            "symfony",
            "vite",
            "wordpress",
            "yii"
        ],
        "the gallery is listed in slug order, which is the order a listing shows it in"
    );

    for entry in ENTRIES {
        mixengine_core::blueprints::store::validated_slug(entry.slug)
            .unwrap_or_else(|error| panic!("{} cannot be a filename: {error}", entry.slug));
    }
}

/// **Ten carry a command and the rest do not** — D8, widened by T205's five PHP entries. Asserted rather than left to a reading of the
/// files, because a scaffold added to `wordpress` or `django` by a later edit is exactly the change
/// this task decided against.
///
/// The eight without one are not a shortfall. A gallery command has to be non-interactive — there is
/// no timeout, so a prompt hangs a job for good — spelled the same for `cmd.exe` and `sh`, with a
/// program for its first word, and it may not write into a shared runtime: that last rule is what
/// removes Django's `pip install django` and Rails' `gem install rails`, both of which reach every
/// project using that runtime. `vite` and `strapi` are kept out by the first: `create-vite` and
/// `create-strapi-app` ask questions no flag reliably silences. And `php-mysql` is the kind of
/// project that has no initialiser at all, which is the point of it. `express-mongodb` would have
/// only `express-generator`, unmaintained and an Express a major version behind, so it unpacks a
/// starter this repository writes instead (an archive, not a command).
///
/// `laravel-mongodb` runs `laravel`'s command and stops there: the `composer require` that adds
/// MongoDB's Eloquent driver would be a second command joined to the first, so its
/// `[blueprint] description` says it instead.
#[test]
fn only_the_entries_that_can_run_a_command_carry_one() {
    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        let expected = matches!(
            entry.slug,
            "laravel"
                | "laravel-mongodb"
                | "symfony"
                | "nextjs"
                | "drupal"
                | "cakephp"
                | "codeigniter"
                | "craft"
                | "statamic"
                | "yii"
        );

        assert_eq!(
            manifest.scaffold.is_some(),
            expected,
            "{} carries the wrong answer about a scaffold command",
            entry.slug
        );
    }
}

/// **And all five of those commands are project-initialisers, so all five say so.**
///
/// `composer create-project .` refuses a directory holding anything at all, `create-next-app`
/// refuses one holding anything it would overwrite, and before the key existed the plan let all
/// three through and left the refusal to the last step of an apply — after the project, the
/// runtimes, the database, the site, the domain and the certificate had been made.
///
/// Asserted over the shipped set rather than over one file, because the next gallery entry that
/// carries an initialiser is the one that would quietly go back to failing late.
#[test]
fn every_gallery_command_that_initialises_a_project_asks_for_an_empty_directory() {
    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        let Some(scaffold) = manifest.scaffold else {
            continue;
        };

        assert!(
            scaffold.needs_empty_dir,
            "{}'s `{}` is an initialiser and has to say so",
            entry.slug, scaffold.command
        );
    }
}

/// **The npm one asks, and only the npm one** — roadmap task **T120c**.
///
/// Four of the five commands are `composer create-project`, which takes its package name from its
/// argument and does not care what the folder is called; the fifth reads the folder's basename and
/// npm judges it. A flag that spread to the other four would refuse folders `composer` installs
/// into happily, which is the over-blocking the design's D6 names as the costly mistake.
#[test]
fn only_the_command_that_names_itself_after_the_directory_asks_for_an_npm_name() {
    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        let Some(scaffold) = manifest.scaffold else {
            continue;
        };

        assert_eq!(
            scaffold.needs_npm_safe_dir,
            entry.slug == "nextjs",
            "{} says needs_npm_safe_dir = {}",
            entry.slug,
            scaffold.needs_npm_safe_dir
        );
    }
}

/// **WordPress is the one archive, and the one file at schema 2** — roadmap task **T205**, D2 and
/// ADR 0061: every other gallery file stays readable by every installed build.
#[test]
fn only_archive_and_dotenv_entries_are_schema_2() {
    // `wordpress` borrows its publisher's release; `express-mongodb` unpacks a starter this
    // repository writes (`src/blueprints/starters/`), because no maintained initialiser makes an
    // Express server that talks to MongoDB; `php-mysql` and `static` unpack one only into an
    // empty folder (`when_empty`), so a cloned repository keeps its own files. `rails` and `django`
    // offer their database URL as a `.env` key (T205a), which changes the apply too.
    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        let is_archive = matches!(
            entry.slug,
            "wordpress" | "express-mongodb" | "php-mysql" | "static"
        );
        let offers_dotenv = matches!(entry.slug, "rails" | "django");

        assert_eq!(manifest.archive.is_some(), is_archive, "{}", entry.slug);
        assert_eq!(
            entry.manifest.starts_with("schema = 2\n"),
            is_archive || offers_dotenv,
            "{} is written at the wrong schema",
            entry.slug
        );
    }
}

/// Where the gallery's own starters are published: beside the signed gallery, on the release the
/// packaging repository's `publish-blueprints` moves, so one run publishes both from one commit.
const STARTERS: &str = "https://github.com/mixnz/mixengine-packages/releases/download/blueprints/";

/// **A starter the gallery names is a starter this tree holds**: `publish-blueprints` zips
/// `starters/<name>/` as `<name>-starter.zip` with `<name>/` at its top, and a URL with nothing
/// behind it would fail the apply on a 404. An archive scaffold strips that folder by name; a step
/// may name one too (`django`'s `startproject --template`, which strips it by itself).
#[test]
fn every_starter_archive_is_in_the_tree() {
    let starters = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/blueprints/starters");
    // A folder is published zipped as `<name>-starter.zip`; a file directly under `starters/`
    // (T205a's `rails-template.rb`) is published as itself.
    let starter = |slug: &str, url: &str| -> Option<String> {
        let file = url.strip_prefix(STARTERS)?;
        if let Some(name) = file.strip_suffix("-starter.zip") {
            let folder = starters.join(name);
            assert!(
                folder.is_dir() && std::fs::read_dir(&folder).unwrap().next().is_some(),
                "{slug}: starters/{name} is not in the tree"
            );
            return Some(name.to_string());
        }
        assert!(
            starters.join(file).is_file(),
            "{slug}: starters/{file} is not in the tree"
        );
        None
    };

    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        if let Some(archive) = manifest.archive.as_ref()
            && let Some(name) = starter(entry.slug, &archive.url)
        {
            assert_eq!(
                archive.strip.as_deref(),
                Some(name.as_str()),
                "{}",
                entry.slug
            );
        }
        for step in &manifest.next_steps {
            let Some(run) = step.run.as_deref() else {
                continue;
            };
            for word in run.split_whitespace() {
                starter(entry.slug, word);
            }
        }
    }
}

/// **A Django project answers on its `.test` domain** — measured on `django`: the stock
/// `startproject` leaves `ALLOWED_HOSTS` empty, which under `DEBUG` admits only `localhost`, so
/// every page through MixEngine's site was *DisallowedHost*; and a form posted over HTTPS needs its
/// origin trusted as well. The gallery's template says both.
#[test]
fn django_starts_from_a_template_that_admits_its_test_domain() {
    let entry = ENTRIES.iter().find(|entry| entry.slug == "django").unwrap();
    let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
    let start = manifest
        .next_steps
        .iter()
        .filter_map(|step| step.run.as_deref())
        .find(|run| run.contains("startproject"))
        .expect("a startproject step");
    assert!(
        start.contains(&format!("--template {STARTERS}django-starter.zip")),
        "{start}"
    );

    let settings = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/blueprints/starters/django/project_name/settings.py-tpl"),
    )
    .expect("the template's settings");
    assert!(settings.contains("ALLOWED_HOSTS = ['.test', 'localhost', '127.0.0.1', '[::1]']"));
    assert!(settings.contains("CSRF_TRUSTED_ORIGINS = ['https://*.test']"));
    assert!(settings.contains("SECRET_KEY = '{{ secret_key }}'"));
    // T205a: the database MixEngine made, read from `.env`, and `.env` kept out of git.
    assert!(settings.contains("_dotenv('DATABASE_URL')"));
    assert!(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/blueprints/starters/django/.gitignore")
            .is_file()
    );
}

/// **Whether the browser opens by itself** — T205, D7 and the last column of D13's table: it
/// opens when no `once` or `serve` step is needed.
#[test]
fn whether_the_browser_opens_by_itself_matches_the_design() {
    use mixengine_proto::NextStepKind;

    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        let needed = manifest
            .next_steps
            .iter()
            .any(|step| step.kind != NextStepKind::Open && !step.optional);

        let expected = !matches!(
            entry.slug,
            "nextjs" | "strapi" | "express-mongodb" | "django" | "rails" | "vite"
        );
        assert_eq!(
            !needed, expected,
            "{} opens the browser by itself: {}",
            entry.slug, !needed
        );
    }
}

/// **An `npm run` step has its packages installed before it** — measured on `laravel`, whose
/// `composer create-project` writes a `package.json` and installs nothing from it, so `npm run dev`
/// answered *Cannot find package 'vite'*. An earlier step installs them (`npm install`, a
/// `npx create-…` initialiser), or the scaffold does (`create-next-app` through `npx`).
#[test]
fn every_npm_run_step_comes_after_its_packages_are_installed() {
    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        let mut installed = manifest
            .scaffold
            .as_ref()
            .is_some_and(|scaffold| scaffold.command.starts_with("npx "));

        for step in &manifest.next_steps {
            let Some(run) = step.run.as_deref() else {
                continue;
            };
            if run.starts_with("npm run ") {
                assert!(
                    installed,
                    "{}: `{run}` comes before anything installs its packages",
                    entry.slug
                );
            }
            if run.starts_with("npm install") || run.starts_with("npx create-") {
                installed = true;
            }
        }
    }
}

/// **A step the person has to write code for is never required** — measured on `express-mongodb`,
/// whose required `node index.js` ran a file nothing creates, so *Run the required steps* always
/// ended at *Cannot find module*. Run the required steps has to be able to finish.
#[test]
fn no_required_step_runs_a_file_nothing_creates() {
    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        for step in &manifest.next_steps {
            let Some(run) = step.run.as_deref() else {
                continue;
            };
            let runs_a_file = run
                .strip_prefix("node ")
                .is_some_and(|file| file.ends_with(".js"));
            assert!(
                !runs_a_file || step.optional,
                "{}: `{run}` is required and nothing writes its file",
                entry.slug
            );
        }
    }
}

/// **A framework that reads its own database settings says where they are** — measured on
/// `rails`: `rails new --database=postgresql` points at `<name>_development` with no account, so
/// `rails server` answered every page with *ConnectionNotEstablished* while MixEngine's database
/// sat there under another name. The step that writes the settings carries `credentials`, so the
/// panel draws the account beside it.
#[test]
fn rails_new_carries_the_database_account() {
    let entry = ENTRIES
        .iter()
        .find(|entry| entry.slug == "rails")
        .expect("rails is in the gallery");
    let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
    let step = manifest
        .next_steps
        .iter()
        .find(|step| {
            step.run
                .as_deref()
                .is_some_and(|run| run.starts_with("rails new"))
        })
        .expect("rails new is a step");
    assert!(step.credentials, "rails new does not carry credentials");
}

/// **Rails and Django read the database MixEngine made from `.env`** — roadmap task **T205a**.
/// Rails under `DEVELOPMENT_DATABASE_URL`, because it merges `DATABASE_URL` into whichever
/// environment runs and `bin/rails test` would purge the development database; and each shows the
/// account, for a person who left the box unticked.
#[test]
fn rails_and_django_offer_their_database_url_and_show_the_account() {
    for (slug, key) in [
        ("rails", "DEVELOPMENT_DATABASE_URL"),
        ("django", "DATABASE_URL"),
    ] {
        let entry = ENTRIES.iter().find(|entry| entry.slug == slug).unwrap();
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        assert_eq!(
            manifest
                .services
                .iter()
                .find_map(|service| service.dotenv.as_deref()),
            Some(key),
            "{slug}"
        );
        assert!(
            manifest.next_steps.iter().any(|step| step.credentials),
            "{slug}"
        );
    }

    let rails = ENTRIES.iter().find(|entry| entry.slug == "rails").unwrap();
    let rails = manifest::read(rails.manifest).expect("a gallery blueprint");
    assert!(
        rails
            .next_steps
            .iter()
            .filter_map(|step| step.run.as_deref())
            .any(|run| run.contains(&format!("-m {STARTERS}rails-template.rb")))
    );
}

/// **A web installer on PostgreSQL is told so** — measured on `craft`: its installer offers MySQL
/// first, and a person who kept it against PostgreSQL's port waited on *MySQL server has gone
/// away*. An `open` step that asks for the database names the driver when it is not MySQL's.
#[test]
fn an_installer_on_postgres_says_postgres() {
    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        if !manifest
            .services
            .iter()
            .any(|service| service.name == "postgres")
        {
            continue;
        }
        for step in &manifest.next_steps {
            if step.kind != mixengine_proto::NextStepKind::Open || !step.credentials {
                continue;
            }
            let note = step.note.as_deref().unwrap_or("");
            assert!(
                note.contains("PostgreSQL"),
                "{}: the installer at {:?} does not say PostgreSQL",
                entry.slug,
                step.path
            );
        }
    }
}

/// **Every step expands for a name that is not a slug** — T205's review focus: `My Blog` must
/// become `my-blog`, never reach a command as itself, and never leave the token behind.
#[test]
fn every_gallery_step_expands_for_a_name_with_spaces() {
    for entry in ENTRIES {
        let manifest = manifest::read(entry.manifest).expect("a gallery blueprint");
        for step in mixengine_core::blueprints::steps::expanded(&manifest.next_steps, "My Blog") {
            if let Some(run) = step.run {
                assert!(!run.contains("{project}"), "{}: {run}", entry.slug);
                assert!(!run.contains("My Blog"), "{}: {run}", entry.slug);
            }
        }
    }
}

/// An opened home with its database, both in a directory the test owns — `tests/store.rs`' helper.
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

/// **A home nobody has touched holds the gallery, trusted** — the T79 design, D1 and D3.
#[tokio::test]
async fn a_fresh_home_is_seeded_with_the_whole_gallery() {
    let (_temp, paths, store) = home().await;

    let seeded = gallery::seed(&store, &paths).await.expect("a seeded home");
    assert_eq!(seeded.written.len(), ENTRIES.len(), "{seeded:?}");

    let listed = blueprint_store::records(&store, &paths)
        .await
        .expect("a listing");
    assert_eq!(listed.len(), ENTRIES.len());

    for summary in &listed {
        assert_eq!(summary.source, BlueprintSource::Builtin, "{summary:?}");
        assert!(summary.trusted, "{summary:?}");
        assert!(
            std::path::Path::new(&summary.file).exists(),
            "the rendering is missing: {summary:?}"
        );
    }
}

/// **The second start writes nothing at all** — D4. This is the assertion the decision exists for:
/// every CLI test in this workspace starts a daemon, and eleven file writes on each of those is a cost
/// with nothing on the other side of it.
#[tokio::test]
async fn seeding_a_home_that_is_already_seeded_writes_nothing() {
    let (_temp, paths, store) = home().await;

    gallery::seed(&store, &paths).await.expect("a seeded home");
    let again = gallery::seed(&store, &paths).await.expect("a second seed");

    assert!(again.written.is_empty(), "it wrote rows again: {again:?}");
    assert!(again.rendered.is_empty(), "it wrote files again: {again:?}");
    assert_eq!(again.left.len(), ENTRIES.len(), "{again:?}");
}

/// **A row somebody else owns is never touched** — D6. Capturing over `laravel` takes `--overwrite`
/// and makes the row this machine's own; no upgrade takes that slug back.
#[tokio::test]
async fn a_captured_row_survives_a_seed() {
    let (_temp, paths, store) = home().await;
    let mine = manifest::read(
        r#"schema = 1

[blueprint]
name = "Mine"
created_at = "2026-09-01T00:00:00Z"

[blueprint.created_on]
os = "windows"
version = "0.1.0"
"#,
    )
    .expect("a manifest of my own");

    blueprint_store::save(
        &store,
        &paths,
        &mine,
        "laravel",
        BlueprintSource::Captured,
        Trust::Inherent,
        false,
    )
    .await
    .expect("a capture under the gallery's slug");

    gallery::seed(&store, &paths).await.expect("a seed");

    let filed = blueprint_store::filed_of(&store, "laravel")
        .await
        .expect("the row");
    assert_eq!(filed.source, BlueprintSource::Captured);
    assert_eq!(filed.manifest.blueprint.name, "Mine");
}

/// **A builtin row that drifted is put back** — D5's repair property: a home whose gallery was
/// edited or emptied is mended by starting the daemon, exactly as `bin/` is.
#[tokio::test]
async fn a_builtin_row_that_was_edited_is_restored() {
    let (_temp, paths, store) = home().await;
    gallery::seed(&store, &paths).await.expect("a seed");

    sqlx::query("UPDATE blueprints SET manifest_toml = 'schema = 1' WHERE id = 'static'")
        .execute(store.pool())
        .await
        .expect("an edited row");

    let again = gallery::seed(&store, &paths).await.expect("a second seed");
    assert_eq!(again.written, vec!["static".to_owned()], "{again:?}");

    let filed = blueprint_store::filed_of(&store, "static")
        .await
        .expect("the row");
    assert_eq!(filed.manifest.blueprint.name, "Static site");
}

/// A rendering deleted from `blueprints/` comes back without the row being rewritten.
#[tokio::test]
async fn a_deleted_rendering_is_written_again() {
    let (_temp, paths, store) = home().await;
    gallery::seed(&store, &paths).await.expect("a seed");

    std::fs::remove_file(blueprint_store::file(&paths, "django")).expect("the rendering");

    let again = gallery::seed(&store, &paths).await.expect("a second seed");
    assert!(again.written.is_empty(), "the row was rewritten: {again:?}");
    assert_eq!(again.rendered, vec!["django".to_owned()], "{again:?}");
}

/// A `PATH` holding exactly these programs, on this system's rule, so that what is asserted is the
/// gallery's own commands and not what a CI runner happens to have.
///
/// Each is a copy of this test binary under the program's name: the copy keeps the execute bit
/// `execvp` wants, and `EXE_SUFFIX` gives it the extension `cmd.exe` wants — without a `#[cfg]`
/// naming either system.
fn a_path_holding(temp: &TempDir, programs: &[&str]) -> std::ffi::OsString {
    let tools = temp.path().join("tools");
    std::fs::create_dir_all(&tools).expect("a tools directory");
    let itself = std::env::current_exe().expect("this test binary");

    for name in programs {
        let file = tools.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(&itself, &file).expect("a program");
    }

    tools.into_os_string()
}

/// What each gallery blueprint plans on a machine holding nothing at all but `programs` — which is
/// the ordinary case for one, since a person applying `laravel` has very often never installed PHP.
async fn planned_with(slug: &str, programs: &[&str]) -> mixengine_proto::BlueprintPlan {
    planned_for(slug, programs, "shop").await
}

/// The same, for a project called something in particular.
///
/// **A parameter since roadmap task T120**, which is about what a project's name may be: every
/// caller before it wanted `shop`, a name that is already a slug and therefore says nothing about
/// what happens to one that is not.
async fn planned_for(
    slug: &str,
    programs: &[&str],
    project: &str,
) -> mixengine_proto::BlueprintPlan {
    let (temp, _paths, store) = home().await;
    let entry = ENTRIES
        .iter()
        .find(|entry| entry.slug == slug)
        .expect("a gallery blueprint");

    let filed = Filed {
        manifest: manifest::read(entry.manifest).expect("a manifest"),
        source: BlueprintSource::Builtin,
        trusted: true,
        signature: None,
    };

    plan(
        &store,
        &mixengine_core::generate::Catalogue::builtin(),
        &mixengine_core::blueprints::plan::Wanted {
            blueprint: slug,
            filed: &filed,
            project,
            root: std::path::Path::new("/projects/shop"),
            answers: &[],
            scaffold_path: &a_path_holding(&temp, programs),
            front_end: false,
        },
    )
    .await
    .expect("a plan")
}

/// The programs the gallery's own commands name, and nothing more.
const GALLERY_PROGRAMS: &[&str] = &["composer", "npx"];

async fn planned(slug: &str) -> mixengine_proto::BlueprintPlan {
    planned_with(slug, GALLERY_PROGRAMS).await
}

/// **Every one of the thirteen plans without a blocked step** — nothing in the gallery asks for
/// something this build cannot do on a machine that has nothing installed but the two programs the
/// gallery's commands name.
#[tokio::test]
async fn every_gallery_blueprint_plans_on_a_machine_with_nothing_installed() {
    for entry in ENTRIES {
        let planned = planned(entry.slug).await;

        assert!(
            !planned.steps.is_empty(),
            "{} planned nothing at all",
            entry.slug
        );
        assert!(
            !planned
                .steps
                .iter()
                .any(|step| matches!(step.disposition, Disposition::Blocked { .. })),
            "{} plans a step this build cannot carry out: {:?}",
            entry.slug,
            planned.steps
        );
    }
}

/// **On a machine without `composer`, the entries that run it are blocked at exactly one step
/// and it is the command** — roadmap task **T78b**, widened by T205's five PHP entries. The gap
/// the product does not close (T25 keeps `composer` out of the shims) is on the screen rather than
/// at the end of the job, and the others plan clean because `npx` is a shim every home has.
#[tokio::test]
async fn without_composer_only_the_entries_that_need_it_are_blocked_and_only_at_the_command() {
    for entry in ENTRIES {
        let planned = planned_with(entry.slug, &["npx"]).await;
        let blocked: Vec<_> = planned
            .steps
            .iter()
            .filter(|step| matches!(step.disposition, Disposition::Blocked { .. }))
            .collect();

        match entry.slug {
            "laravel" | "laravel-mongodb" | "symfony" | "drupal" | "cakephp" | "codeigniter"
            | "craft" | "statamic" | "yii" => {
                assert_eq!(blocked.len(), 1, "{}: {:?}", entry.slug, planned.steps);
                assert!(
                    matches!(blocked[0].action, PlanAction::RunScaffold { .. }),
                    "{}: {:?}",
                    entry.slug,
                    blocked[0]
                );
                assert!(
                    matches!(
                        &blocked[0].disposition,
                        Disposition::Blocked { reason } if reason.contains("`composer`")
                    ),
                    "{}: {:?}",
                    entry.slug,
                    blocked[0]
                );
            }
            _ => assert!(blocked.is_empty(), "{}: {blocked:?}", entry.slug),
        }
    }
}

/// **The entries that run Composer ask for it** — roadmap task **T27c**, its design's D6 — so a
/// machine without one reads `create composer 2` where T78b had it read `blocked`.
#[tokio::test]
async fn the_composer_blueprints_ask_for_it_and_nothing_else_does() {
    for entry in ENTRIES {
        let planned = planned(entry.slug).await;
        let asks = planned.steps.iter().any(|step| {
            matches!(
                &step.action,
                PlanAction::InstallRuntime {
                    kind: RuntimeKind::Composer,
                    ..
                }
            )
        });
        assert_eq!(
            asks,
            matches!(
                entry.slug,
                "laravel"
                    | "laravel-mongodb"
                    | "symfony"
                    | "drupal"
                    | "cakephp"
                    | "codeigniter"
                    | "craft"
                    | "statamic"
                    | "yii"
            ),
            "{}: {:?}",
            entry.slug,
            planned.steps
        );
    }
}

/// **`{project}` is expanded everywhere it appears, and nowhere is it left as a token** — the T78a
/// D6 property, held for the shipped set: a gallery blueprint that planned the literal `{project}`
/// would create a database called `{project}` on somebody's machine.
#[tokio::test]
async fn no_gallery_plan_carries_an_unexpanded_token() {
    for entry in ENTRIES {
        let planned = planned(entry.slug).await;

        assert!(
            !format!("{:?}", planned.steps).contains("{project}"),
            "{} left a token in its plan: {:?}",
            entry.slug,
            planned.steps
        );
    }
}

/// **Every blueprint this build ships works for a name a person would actually type** — roadmap
/// task **T120**.
///
/// Before it, `{project}` was substituted verbatim, so `My Project 1` produced the database name
/// `My Project 1` and the domain `My Project 1.test`: a green dry run, and then an apply that failed
/// on the first of those after the directory had been made and three packages downloaded. Every
/// entry in the gallery uses the token, so every entry was affected — which is why this is asserted
/// over the shipped set rather than over one fixture.
#[tokio::test]
async fn every_gallery_blueprint_plans_for_a_name_that_is_not_already_a_slug() {
    for entry in ENTRIES {
        let planned = planned_for(entry.slug, GALLERY_PROGRAMS, "My Project 1").await;

        assert!(
            !planned
                .steps
                .iter()
                .any(|step| matches!(step.disposition, Disposition::Blocked { .. })),
            "{} blocks a step for a name with a space and a capital in it: {:?}",
            entry.slug,
            planned.steps
        );

        for step in &planned.steps {
            match &step.action {
                PlanAction::CreateDatabase { database, user, .. } => {
                    assert_eq!(database, "my-project-1", "{}", entry.slug);
                    assert_eq!(user, "my-project-1", "{}", entry.slug);
                }
                PlanAction::AddDomain { domain, .. } => {
                    assert!(
                        domain.starts_with("my-project-1."),
                        "{}: {domain}",
                        entry.slug
                    );
                }
                // The name the person typed, untouched — it is a label they read, not an identifier.
                PlanAction::RegisterProject { name, .. } => {
                    assert_eq!(name, "My Project 1", "{}", entry.slug);
                }
                _ => {}
            }
        }
    }
}

/// The headline blueprint, step by step: a machine with nothing on it installs two languages, two
/// servers, makes a database, a site, a name and a certificate, turns an extension on, and offers
/// the command.
#[tokio::test]
async fn laravel_plans_the_whole_stack() {
    let planned = planned("laravel").await;
    let has =
        |wanted: fn(&PlanAction) -> bool| planned.steps.iter().any(|step| wanted(&step.action));

    assert!(has(|action| matches!(
        action,
        PlanAction::RegisterProject { .. }
    )));
    assert!(has(|action| matches!(
        action,
        PlanAction::InstallRuntime { kind, .. } if *kind == RuntimeKind::Php
    )));
    assert!(has(|action| matches!(
        action,
        PlanAction::InstallRuntime { kind, .. } if *kind == RuntimeKind::Node
    )));
    assert!(has(
        |action| matches!(action, PlanAction::CreateDatabase { database, .. } if database == "shop")
    ));
    assert!(has(|action| matches!(
        action,
        PlanAction::CreateSite { .. }
    )));
    assert!(has(
        |action| matches!(action, PlanAction::AddDomain { domain, .. } if domain == "shop.test")
    ));
    assert!(has(|action| matches!(
        action,
        PlanAction::IssueCertificate { .. }
    )));
    assert!(has(
        |action| matches!(action, PlanAction::SetPhpExtension { name, .. } if name == "redis")
    ));
    assert!(has(|action| matches!(
        action,
        PlanAction::RunScaffold { .. }
    )));
}

/// **The two MongoDB blueprints ask for the server and never for a database** — roadmap phase 19.
///
/// The `mongodb` recipe runs with access control off and answers no `databases()`: a MongoDB database
/// exists once something writes to it, and there is no account to make. A `database` key on either
/// entry would plan a `CreateDatabase` step the apply is refused at, after the downloads. And the PHP
/// one turns the driver's extension on, because a machine that turned it off is exactly the one a
/// Laravel app talking to MongoDB fails on with nothing pointing at why.
#[tokio::test]
async fn the_mongodb_blueprints_plan_a_server_and_no_database() {
    for slug in ["express-mongodb", "laravel-mongodb"] {
        let planned = planned(slug).await;
        let has =
            |wanted: fn(&PlanAction) -> bool| planned.steps.iter().any(|step| wanted(&step.action));

        assert!(
            has(|action| matches!(
                action,
                PlanAction::InstallPackage { package, .. } if package == "mongodb"
            )),
            "{slug}: {:?}",
            planned.steps
        );
        assert!(
            has(|action| matches!(
                action,
                PlanAction::EnsureService { package, instance, .. }
                    if package == "mongodb" && instance == "main"
            )),
            "{slug}: {:?}",
            planned.steps
        );
        assert!(
            !has(|action| matches!(action, PlanAction::CreateDatabase { .. })),
            "{slug}: {:?}",
            planned.steps
        );
        assert_eq!(
            has(|action| matches!(
                action,
                PlanAction::SetPhpExtension { name, .. } if name == "mongodb"
            )),
            slug == "laravel-mongodb",
            "{slug}: {:?}",
            planned.steps
        );
    }
}

/// **The three without a command plan no command** — D8, asserted on the plan rather than on the
/// file, because the plan is what an apply carries out.
#[tokio::test]
async fn the_blueprints_with_no_command_plan_no_command() {
    for slug in ["wordpress", "django", "static"] {
        let planned = planned(slug).await;

        assert!(
            !planned
                .steps
                .iter()
                .any(|step| matches!(step.action, PlanAction::RunScaffold { .. })),
            "{slug} planned a command it does not carry"
        );
    }
}

/// **The display names are deliberately not slugs** — roadmap task **T79a**, its design's D9. This
/// is what makes the daemon's hand-import test mean something: if somebody renamed `Static site` to
/// `static`, that test would keep passing for a reason that had stopped being true, and the fallback
/// it guards would be untested again.
#[test]
fn the_gallery_display_names_are_not_slugs() {
    let names: Vec<String> = ENTRIES
        .iter()
        .map(|entry| {
            manifest::read(entry.manifest)
                .expect("a gallery blueprint")
                .blueprint
                .name
        })
        .collect();

    assert!(
        names.iter().any(|name| name.contains('.')),
        "no gallery name carries a dot any more: {names:?}"
    );
    assert!(
        names.iter().any(|name| name.contains(' ')),
        "no gallery name carries a space any more: {names:?}"
    );

    for name in &names {
        assert!(
            blueprint_store::validated_slug(name).is_err(),
            "{name} is spelled as a slug, which is not what `[blueprint] name` is for"
        );
    }
}
