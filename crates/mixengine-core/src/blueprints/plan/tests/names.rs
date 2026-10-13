//! Names a plan derives or refuses: the project's slug and the token it expands to, service ids,
//! database names and domains, and which steps ask for a password.

use super::*;

/// **D11.** Adding a domain writes the hosts file, and nothing else in a plan asks for a
/// password.
#[tokio::test]
async fn only_the_steps_that_need_a_password_say_so() {
    let (temp, store) = home().await;

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(a_manifest()),
            project: "shop",
            root: &temp.path().join("shop"),
            answers: &[],
            scaffold_path: nowhere(),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    assert!(
        planned
            .steps
            .iter()
            .any(|step| step.elevates && matches!(step.action, PlanAction::AddDomain { .. }))
    );
    assert!(planned.steps.iter().all(|step| !step.elevates
        || matches!(
            step.action,
            PlanAction::AddDomain { .. } | PlanAction::IssueCertificate { .. }
        )));
}

/// **D10, and the case a project name makes reachable.** Project names allow spaces and upper
/// case; service ids do not. A dedicated instance for `My Blog` cannot be spelled, and that is
/// said here rather than discovered by T78 while writing the row.
#[tokio::test]
async fn a_project_name_no_service_id_can_hold_blocks_its_dedicated_instance() {
    let (temp, store) = home().await;
    let mut manifest = a_manifest();
    manifest.services[0].instance = Some(PER_PROJECT.to_owned());

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(manifest),
            project: "My Blog",
            root: &temp.path().join("my blog"),
            answers: &[],
            scaffold_path: nowhere(),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    assert!(
        matches!(
            &ensure_of(&planned, "mariadb").disposition,
            Disposition::Blocked { reason } if reason.contains("mariadb@My Blog")
        ),
        "{planned:?}"
    );
}

/// A directory that is already a project is not a place to put a second one.
#[tokio::test]
async fn a_root_that_is_already_a_project_is_blocked() {
    let (temp, store) = home().await;
    let root = temp.path().join("blog");
    std::fs::create_dir_all(&root).expect("a directory");

    crate::projects::create(
        &store,
        &crate::projects::Registration {
            name: "blog".to_owned(),
            root: root.clone(),
            pins: BTreeMap::new(),
        },
        mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
    )
    .await
    .expect("a project");

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(a_manifest()),
            project: "shop",
            root: &root,
            answers: &[],
            scaffold_path: nowhere(),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    assert!(
        matches!(
            &step_of(&planned, |action| matches!(
                action,
                PlanAction::RegisterProject { .. }
            ))
            .disposition,
            Disposition::Blocked { reason } if reason.contains("blog")
        ),
        "{planned:?}"
    );
}

/// **D1.** A project's name is a label a person reads; a database, a domain and a service id
/// each have a charset it does not have to satisfy. `{project}` expands to the handle between
/// them — `domains::slug`, which is the rule `project.create` has derived a domain with since
/// T39a.
#[tokio::test]
async fn the_token_expands_to_the_projects_slug_and_not_its_name() {
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
            project: "My Blog 1",
            root: &temp.path().join("my-blog-1"),
            answers: &[],
            scaffold_path: &a_path_holding(&temp, &["composer"]),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    // The name the person typed is what gets registered, untouched.
    let PlanAction::RegisterProject { name, .. } = &step_of(&planned, |action| {
        matches!(action, PlanAction::RegisterProject { .. })
    })
    .action
    else {
        panic!("a register step");
    };
    assert_eq!(name, "My Blog 1");

    // Everywhere else, the handle.
    let PlanAction::CreateDatabase { database, user, .. } = &step_of(&planned, |action| {
        matches!(action, PlanAction::CreateDatabase { .. })
    })
    .action
    else {
        panic!("a database step");
    };
    assert_eq!(database, "my-blog-1");
    assert_eq!(user, "my-blog-1");

    let PlanAction::AddDomain { domain, .. } = &step_of(&planned, |action| {
        matches!(action, PlanAction::AddDomain { .. })
    })
    .action
    else {
        panic!("a domain step");
    };
    assert_eq!(domain, "my-blog-1.test");

    let PlanAction::RunScaffold { command } = &step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    })
    .action
    else {
        panic!("a scaffold step");
    };
    assert_eq!(command, "composer create-project laravel/laravel my-blog-1");
}

/// **D1, and the reason the comment above the scaffold expansion is now true.** A slug holds no
/// shell metacharacter, so a project name full of them reaches the shell as a handle rather
/// than as a second command.
#[tokio::test]
async fn a_name_holding_shell_metacharacters_reaches_the_shell_as_a_slug() {
    let (temp, store) = home().await;
    let mut manifest = a_manifest();
    manifest.scaffold = Some(crate::blueprints::manifest::Scaffold {
        command: "echo {project}".to_owned(),
        needs_empty_dir: false,
        needs_npm_safe_dir: false,
    });

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(manifest),
            project: "a; rm -rf $HOME",
            root: &temp.path().join("a"),
            answers: &[],
            scaffold_path: &a_path_holding(&temp, &["echo"]),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    let PlanAction::RunScaffold { command } = &step_of(&planned, |action| {
        matches!(action, PlanAction::RunScaffold { .. })
    })
    .action
    else {
        panic!("a scaffold step");
    };

    assert_eq!(command, "echo a-rm-rf-home");
    assert!(!command.contains(';'), "{command}");
    assert!(!command.contains('$'), "{command}");
}

/// **D2.** The planner's whole promise is that an apply does not get five actions into a
/// project directory before discovering the sixth was impossible. Before T120 the database step
/// checked only the account's *length*, so a name the server would refuse outright was a green
/// dry run and a failure after three packages had been downloaded.
#[tokio::test]
async fn a_database_name_the_server_would_refuse_is_blocked_at_plan_time() {
    let (temp, store) = home().await;
    let mut manifest = a_manifest();
    manifest.services[0].database = Some("Not A Name".to_owned());
    manifest.services[0].user = Some("Not A Name".to_owned());

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

    let Disposition::Blocked { reason } = &step_of(&planned, |action| {
        matches!(action, PlanAction::CreateDatabase { .. })
    })
    .disposition
    else {
        panic!("the database step should be blocked: {planned:?}");
    };

    assert!(reason.contains("lower-case"), "{reason}");
}

/// **D2 again, for the other name space.** A blueprint from somebody else's machine can name a
/// TLD this home does not answer for, and finding that out from `site.create` is finding it out
/// after the database exists.
#[tokio::test]
async fn a_domain_that_is_not_one_is_blocked_at_plan_time() {
    let (temp, store) = home().await;
    let mut manifest = a_manifest();
    manifest.sites.first_mut().expect("a site").domain_pattern = "{project}.example.com".to_owned();

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

    assert!(
        matches!(
            step_of(&planned, |action| matches!(
                action,
                PlanAction::AddDomain { .. }
            ))
            .disposition,
            Disposition::Blocked { .. }
        ),
        "{planned:?}"
    );
}

/// **D3.** A name with no ASCII in it cannot be slugged, so the token stays where it is — and
/// every step that uses it says *that*, rather than complaining about a character set the
/// person never typed a character of.
#[tokio::test]
async fn a_name_with_nothing_to_slug_blocks_the_steps_that_need_the_token() {
    let (temp, store) = home().await;
    let mut manifest = a_manifest();
    manifest.scaffold = Some(crate::blueprints::manifest::Scaffold {
        command: "echo {project}".to_owned(),
        needs_empty_dir: false,
        needs_npm_safe_dir: false,
    });

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(manifest),
            project: "日本",
            root: &temp.path().join("x"),
            answers: &[],
            scaffold_path: &a_path_holding(&temp, &["echo"]),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    let using_the_token: [fn(&PlanAction) -> bool; 3] = [
        |action| matches!(action, PlanAction::CreateDatabase { .. }),
        |action| matches!(action, PlanAction::AddDomain { .. }),
        |action| matches!(action, PlanAction::RunScaffold { .. }),
    ];

    for matcher in using_the_token {
        let Disposition::Blocked { reason } = &step_of(&planned, matcher).disposition else {
            panic!("every step using the token is blocked: {planned:?}");
        };

        assert!(reason.contains(TOKEN), "{reason}");
    }
}
