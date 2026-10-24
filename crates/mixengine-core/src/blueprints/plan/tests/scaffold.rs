//! The scaffold step: its command or archive, the directory it lands in, the names npm refuses, and
//! the `.env` lines asked for after it.

use super::*;

/// **`{project}` reaches the scaffold command too.** The first planner expanded the token into
/// domains, databases and accounts and cloned the command verbatim, so a blueprint naming the
/// project in its own command planned the token rather than the name. The command somebody agrees
/// to has to be the command that runs.
#[tokio::test]
async fn the_project_name_reaches_the_scaffold_command() {
    let (temp, store) = home().await;
    let mut manifest = a_manifest();
    manifest.scaffold = Some(crate::blueprints::manifest::Scaffold {
        command: "composer create-project laravel/laravel {project}".to_owned(),
        needs_empty_dir: false,
        needs_npm_safe_dir: false,
    });

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(manifest),
            project: "shop",
            root: &temp.path().join("shop"),
            answers: &[],
            scaffold_path: &a_path_holding(&temp, &["composer"]),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    let command = planned
        .steps
        .iter()
        .find_map(|step| match &step.action {
            PlanAction::RunScaffold { command } => Some(command.clone()),
            _ => None,
        })
        .expect("a scaffold step");

    assert_eq!(command, "composer create-project laravel/laravel shop");

    // And the confirmation is about the same string, because it is the one that will run.
    let shown = planned
        .steps
        .iter()
        .find_map(|step| match &step.disposition {
            Disposition::Confirm { what } => Some(what.clone()),
            _ => None,
        });

    assert_eq!(shown.as_deref(), Some(command.as_str()));
}

/// The plan says which blueprint it is applying and whether anybody vouched for it, so a client
/// shows the right words from the answer it already has.
#[tokio::test]
async fn a_plan_says_where_its_blueprint_came_from() {
    let (temp, store) = home().await;

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "borrowed",
            filed: &imported(a_manifest()),
            project: "shop",
            root: &temp.path().join("shop"),
            answers: &[],
            scaffold_path: nowhere(),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    assert_eq!(planned.source, BlueprintSource::Imported);
    assert!(!planned.trusted);
}

/// A scaffold command is shown, exactly as it would run, and agreed to rather than done.
#[tokio::test]
async fn a_scaffold_command_is_something_to_agree_to() {
    let (temp, store) = home().await;
    let mut manifest = a_manifest();
    manifest.scaffold = Some(crate::blueprints::manifest::Scaffold {
        command: "composer create-project laravel/laravel .".to_owned(),
        needs_empty_dir: false,
        needs_npm_safe_dir: false,
    });

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(manifest),
            project: "shop",
            root: &temp.path().join("shop"),
            answers: &[],
            scaffold_path: &a_path_holding(&temp, &["composer"]),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    assert!(matches!(
        &step_of(&planned, |action| matches!(
            action,
            PlanAction::RunScaffold { .. }
        ))
        .disposition,
        Disposition::Confirm { what } if what.contains("composer")
    ));
}

/// **A program the PATH does not hold is blocked here, not at the end of a job**: the step names
/// the program, and nothing else in the plan changes.
#[tokio::test]
async fn a_scaffold_whose_program_is_missing_is_blocked_here() {
    let (temp, store) = home().await;
    let mut manifest = a_manifest();
    manifest.scaffold = Some(crate::blueprints::manifest::Scaffold {
        command: "composer create-project laravel/laravel .".to_owned(),
        needs_empty_dir: false,
        needs_npm_safe_dir: false,
    });

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(manifest),
            project: "shop",
            root: &temp.path().join("shop"),
            answers: &[],
            scaffold_path: nowhere(),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    let scaffold = step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    });
    assert!(
        matches!(
            &scaffold.disposition,
            Disposition::Blocked { reason } if reason.contains("`composer`")
        ),
        "{scaffold:?}"
    );

    let blocked = planned
        .steps
        .iter()
        .filter(|step| matches!(step.disposition, Disposition::Blocked { .. }))
        .count();
    assert_eq!(
        blocked, 1,
        "only the scaffold is blocked: {:?}",
        planned.steps
    );
}

/// A manifest whose command refuses a directory that holds anything.
fn asking_for_an_empty_directory() -> crate::blueprints::manifest::BlueprintManifest {
    let mut manifest = a_manifest();
    manifest.scaffold = Some(crate::blueprints::manifest::Scaffold {
        command: "composer create-project laravel/laravel . --no-interaction".to_owned(),
        needs_empty_dir: true,
        needs_npm_safe_dir: false,
    });

    manifest
}

/// **A directory that already holds something is decided here, not by composer's exit code.**
///
/// `composer create-project .` stops at the first entry a directory holds, and before this the plan
/// said nothing about it — so an apply registered the project, installed the runtimes, made the
/// database, the site, the domain and the certificate, and *then* the last step said the directory
/// was not empty — exactly what a plan exists to say first.
#[tokio::test]
async fn a_directory_that_already_holds_something_blocks_the_scaffold() {
    let (temp, store) = home().await;
    let root = temp.path().join("shop");
    std::fs::create_dir_all(&root).expect("a directory");
    std::fs::write(root.join("README.md"), "mine").expect("a file");

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(asking_for_an_empty_directory()),
            project: "shop",
            root: &root,
            answers: &[],
            scaffold_path: &a_path_holding(&temp, &["composer"]),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    let scaffold = step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    });

    // The reason names what is in the way, because "not empty" sends somebody to look at a
    // directory that appears empty in a file manager that hides dotfiles.
    assert!(
        matches!(
            &scaffold.disposition,
            Disposition::Blocked { reason } if reason.contains("README.md")
        ),
        "{scaffold:?}"
    );

    let blocked = planned
        .steps
        .iter()
        .filter(|step| matches!(step.disposition, Disposition::Blocked { .. }))
        .count();
    assert_eq!(
        blocked, 1,
        "only the scaffold is blocked: {:?}",
        planned.steps
    );
}

/// A manifest whose scaffold is an archive.
fn fetching(url: &str) -> crate::blueprints::manifest::BlueprintManifest {
    let mut manifest = a_manifest();
    manifest.archive = Some(crate::blueprints::manifest::Archive {
        url: url.to_owned(),
        strip: Some("wordpress".to_owned()),
        needs_empty_dir: true,
        when_empty: false,
    });
    manifest
}

/// A starter unpacked only into an empty folder: `php-mysql`'s and `static`'s shape.
fn starting(url: &str) -> crate::blueprints::manifest::BlueprintManifest {
    let mut manifest = a_manifest();
    manifest.archive = Some(crate::blueprints::manifest::Archive {
        url: url.to_owned(),
        strip: Some("static".to_owned()),
        needs_empty_dir: false,
        when_empty: true,
    });
    manifest
}

fn offering_dotenv() -> crate::blueprints::manifest::BlueprintManifest {
    let mut manifest = a_manifest();
    manifest.services = vec![BlueprintService {
        name: "postgres".to_owned(),
        version: None,
        instance: Some("main".to_owned()),
        database: Some("{project}".to_owned()),
        user: Some("{project}".to_owned()),
        dotenv: Some("DATABASE_URL".to_owned()),
    }];
    manifest
}

/// **The `.env` line is asked for, and after the scaffold.** After, because `create-project .`
/// refuses a folder that already holds a `.env`.
#[tokio::test]
async fn a_dotenv_key_is_asked_for_after_the_scaffold() {
    let (temp, store) = home().await;
    let mut manifest = offering_dotenv();
    manifest.archive = starting("https://x.org/static-starter.zip").archive;

    let planned = planned_for(&store, &temp, &temp.path().join("site"), manifest).await;
    let at = |wanted: fn(&PlanAction) -> bool| {
        planned
            .steps
            .iter()
            .position(|step| wanted(&step.action))
            .expect("planned")
    };
    assert!(
        at(|action| matches!(action, PlanAction::FetchArchive { .. }))
            < at(|action| matches!(action, PlanAction::WriteDotenv { .. }))
    );

    let step = step_of(&planned, |action| {
        matches!(action, PlanAction::WriteDotenv { .. })
    });
    assert_eq!(
        step.action,
        PlanAction::WriteDotenv {
            key: "DATABASE_URL".to_owned(),
            path: ".env".to_owned()
        }
    );
    assert_eq!(
        step.disposition,
        Disposition::Confirm {
            what: "DATABASE_URL in .env".to_owned()
        }
    );
}

#[tokio::test]
async fn a_dotenv_key_already_there_is_satisfied() {
    let (temp, store) = home().await;
    let root = temp.path().join("site");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(root.join(".env"), "export DATABASE_URL=mine\n").expect("env");

    let planned = planned_for(&store, &temp, &root, offering_dotenv()).await;
    let step = step_of(&planned, |action| {
        matches!(action, PlanAction::WriteDotenv { .. })
    });
    assert_eq!(step.disposition, Disposition::Satisfied);
}

#[tokio::test]
async fn a_dotenv_key_missing_from_an_existing_env_is_asked_for() {
    let (temp, store) = home().await;
    let root = temp.path().join("site");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(root.join(".env"), "OTHER=1").expect("env");

    let planned = planned_for(&store, &temp, &root, offering_dotenv()).await;
    let step = step_of(&planned, |action| {
        matches!(action, PlanAction::WriteDotenv { .. })
    });
    assert!(
        matches!(step.disposition, Disposition::Confirm { .. }),
        "{:?}",
        step.disposition
    );
}

/// **A starter steps aside for code already there.** A folder somebody cloned keeps its own
/// `index.html`; the archive is neither unpacked over it nor a reason to stop the apply.
#[tokio::test]
async fn a_starter_into_a_full_directory_is_satisfied() {
    let (temp, store) = home().await;
    let root = temp.path().join("site");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(root.join("index.html"), "mine").expect("a file");

    let planned = planned_for(
        &store,
        &temp,
        &root,
        starting("https://x.org/static-starter.zip"),
    )
    .await;
    let step = step_of(&planned, |action| {
        matches!(action, PlanAction::FetchArchive { .. })
    });
    assert_eq!(step.disposition, Disposition::Satisfied);
}

#[tokio::test]
async fn a_starter_into_an_empty_directory_is_agreed_to() {
    let (temp, store) = home().await;
    let planned = planned_for(
        &store,
        &temp,
        &temp.path().join("site"),
        starting("https://x.org/static-starter.zip"),
    )
    .await;
    let step = step_of(&planned, |action| {
        matches!(action, PlanAction::FetchArchive { .. })
    });
    assert_eq!(
        step.disposition,
        Disposition::Confirm {
            what: "https://x.org/static-starter.zip".to_owned()
        }
    );
}

/// **The scaffold's gate, with the URL where the command would be.**
#[tokio::test]
async fn an_archive_is_something_to_agree_to_naming_its_url() {
    let (temp, store) = home().await;
    let root = temp.path().join("wp");
    let planned = planned_for(
        &store,
        &temp,
        &root,
        fetching("https://wordpress.org/latest.zip"),
    )
    .await;

    let step = step_of(&planned, |action| {
        matches!(action, PlanAction::FetchArchive { .. })
    });
    assert_eq!(
        step.disposition,
        Disposition::Confirm {
            what: "https://wordpress.org/latest.zip".to_owned()
        }
    );
}

#[tokio::test]
async fn an_archive_into_a_full_directory_is_blocked() {
    let (temp, store) = home().await;
    let root = temp.path().join("wp");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(root.join("README"), "x").expect("a file");

    let planned = planned_for(
        &store,
        &temp,
        &root,
        fetching("https://wordpress.org/latest.zip"),
    )
    .await;
    let step = step_of(&planned, |action| {
        matches!(action, PlanAction::FetchArchive { .. })
    });
    assert!(
        matches!(step.disposition, Disposition::Blocked { .. }),
        "{:?}",
        step.disposition
    );
}

#[tokio::test]
async fn an_archive_of_an_unknown_format_is_blocked_by_its_suffix() {
    let (temp, store) = home().await;
    let planned = planned_for(
        &store,
        &temp,
        &temp.path().join("wp"),
        fetching("https://x.org/a.rar"),
    )
    .await;
    let step = step_of(&planned, |action| {
        matches!(action, PlanAction::FetchArchive { .. })
    });
    let Disposition::Blocked { reason } = &step.disposition else {
        panic!("{:?}", step.disposition)
    };
    assert!(reason.contains(".rar"), "{reason}");
}

/// A manifest whose command takes its package name from the directory it runs in.
fn naming_itself_after_its_directory() -> crate::blueprints::manifest::BlueprintManifest {
    let mut manifest = a_manifest();
    manifest.scaffold = Some(crate::blueprints::manifest::Scaffold {
        command: "npx --yes create-next-app@latest . --yes".to_owned(),
        needs_empty_dir: false,
        needs_npm_safe_dir: true,
    });

    manifest
}

/// The scaffold step of a plan for `manifest`, applied at `root`.
async fn scaffolding_at(
    temp: &tempfile::TempDir,
    store: &crate::store::Store,
    manifest: crate::blueprints::manifest::BlueprintManifest,
    root: &Path,
) -> BlueprintPlan {
    plan(
        store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(manifest),
            project: "shop",
            root,
            answers: &[],
            scaffold_path: &a_path_holding(temp, &["npx"]),
            front_end: false,
        },
    )
    .await
    .expect("a plan")
}

/// **The row that decides the rule.** `next_js_1` is not a slug — [`crate::domains::slug`] turns
/// the underscore into a hyphen — and npm accepts it, installs into it and writes
/// `"name": "next_js_1"`. Measured on 2026-09-13 against
/// `npx --yes create-next-app@latest . --yes`. A check built on `slug` would refuse a directory
/// that works, which is the mistake this test exists to keep out.
#[tokio::test]
async fn an_underscored_directory_is_not_refused() {
    let (temp, store) = home().await;
    let root = temp.path().join("next_js_1");
    std::fs::create_dir_all(&root).expect("a directory");

    let planned = scaffolding_at(&temp, &store, naming_itself_after_its_directory(), &root).await;

    let scaffold = step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    });

    assert!(
        !matches!(scaffold.disposition, Disposition::Blocked { .. }),
        "{scaffold:?}"
    );
}

/// Capitals and a space, which is the failure this task was reported over.
#[tokio::test]
async fn a_directory_npm_will_not_name_a_package_after_is_blocked_with_the_name_to_use() {
    let (temp, store) = home().await;
    let root = temp.path().join("Next.js 1");
    std::fs::create_dir_all(&root).expect("a directory");

    let planned = scaffolding_at(&temp, &store, naming_itself_after_its_directory(), &root).await;

    let scaffold = step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    });

    let Disposition::Blocked { reason } = &scaffold.disposition else {
        panic!("{scaffold:?}");
    };

    assert!(reason.contains("Next.js 1"), "{reason}");
    // **The answer, not the rule**: the reason suggests a slug.
    assert!(reason.contains("next-js-1"), "{reason}");
}

/// Each of these is refused by `create-next-app`, measured the same day, so refusing them here
/// is not over-blocking.
#[tokio::test]
async fn the_other_names_npm_refuses_are_blocked_too() {
    let (temp, store) = home().await;

    for name in ["app(1)", "app~x", ".hidden", "_leading"] {
        let root = temp.path().join(name);
        std::fs::create_dir_all(&root).expect("a directory");

        let planned =
            scaffolding_at(&temp, &store, naming_itself_after_its_directory(), &root).await;

        let scaffold = step_of(&planned, |action| {
            matches!(action, PlanAction::RunScaffold { .. })
        });

        assert!(
            matches!(scaffold.disposition, Disposition::Blocked { .. }),
            "{name} must be blocked: {scaffold:?}"
        );
    }
}

/// **A blueprint that did not declare it is never judged by it.** Three of the gallery's four
/// scaffolds take their package name from an argument and do not care what the folder is called.
#[tokio::test]
async fn a_scaffold_that_did_not_ask_is_not_judged_on_its_directorys_name() {
    let (temp, store) = home().await;
    let root = temp.path().join("Next.js 1");
    std::fs::create_dir_all(&root).expect("a directory");

    let mut manifest = naming_itself_after_its_directory();
    manifest
        .scaffold
        .as_mut()
        .expect("a scaffold")
        .needs_npm_safe_dir = false;

    let planned = scaffolding_at(&temp, &store, manifest, &root).await;

    let scaffold = step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    });

    assert!(
        !matches!(scaffold.disposition, Disposition::Blocked { .. }),
        "{scaffold:?}"
    );
}

/// **The name is asked before the contents.** Emptying a folder that will be refused for its
/// name is work nobody gets back, so that is the sentence worth reading first.
#[tokio::test]
async fn a_badly_named_directory_that_is_also_full_is_refused_for_its_name() {
    let (temp, store) = home().await;
    let root = temp.path().join("Next.js 1");
    std::fs::create_dir_all(&root).expect("a directory");
    std::fs::write(root.join("README.md"), "mine").expect("a file");

    let mut manifest = naming_itself_after_its_directory();
    manifest
        .scaffold
        .as_mut()
        .expect("a scaffold")
        .needs_empty_dir = true;

    let planned = scaffolding_at(&temp, &store, manifest, &root).await;

    let scaffold = step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    });

    let Disposition::Blocked { reason } = &scaffold.disposition else {
        panic!("blocked for something: {scaffold:?}");
    };

    assert!(reason.contains("next-js-1"), "{reason}");
    assert!(!reason.contains("README.md"), "{reason}");
}

/// The directory an apply is about usually does not exist yet — that is the happy path, and it
/// is the one a false `blocked` here would cost.
#[tokio::test]
async fn a_directory_that_is_not_there_yet_is_a_directory_that_is_empty() {
    let (temp, store) = home().await;

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(asking_for_an_empty_directory()),
            project: "shop",
            root: &temp.path().join("shop"),
            answers: &[],
            scaffold_path: &a_path_holding(&temp, &["composer"]),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    let scaffold = step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    });
    assert!(
        matches!(scaffold.disposition, Disposition::Confirm { .. }),
        "{scaffold:?}"
    );
}

/// A directory somebody made in a file manager and picked with the folder picker, which is what
/// the desktop's Quick Start hands over.
#[tokio::test]
async fn a_directory_that_is_there_and_empty_is_still_offered() {
    let (temp, store) = home().await;
    let root = temp.path().join("shop");
    std::fs::create_dir_all(&root).expect("a directory");

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(asking_for_an_empty_directory()),
            project: "shop",
            root: &root,
            answers: &[],
            scaffold_path: &a_path_holding(&temp, &["composer"]),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    let scaffold = step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    });
    assert!(
        matches!(scaffold.disposition, Disposition::Confirm { .. }),
        "{scaffold:?}"
    );
}

/// **And a command that did not ask is left alone.** `composer install` on a tree somebody
/// cloned is a scaffold whose directory is *supposed* to hold something, and every manifest
/// written before the key existed is one of these.
#[tokio::test]
async fn a_scaffold_that_did_not_ask_runs_wherever_it_was_pointed() {
    let (temp, store) = home().await;
    let root = temp.path().join("shop");
    std::fs::create_dir_all(&root).expect("a directory");
    std::fs::write(root.join("composer.json"), "{}").expect("a file");

    let mut manifest = a_manifest();
    manifest.scaffold = Some(crate::blueprints::manifest::Scaffold {
        command: "composer install".to_owned(),
        needs_empty_dir: false,
        needs_npm_safe_dir: false,
    });

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(manifest),
            project: "shop",
            root: &root,
            answers: &[],
            scaffold_path: &a_path_holding(&temp, &["composer"]),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    let scaffold = step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    });
    assert!(
        matches!(scaffold.disposition, Disposition::Confirm { .. }),
        "{scaffold:?}"
    );
}
