//! What the home already holds: packages, the project this apply made, pins and the answers to them.

use super::*;

/// **D8.** A machine with no MariaDB at all is the ordinary case for the feature's headline
/// scenario — a blueprint from somebody else's machine — and the plan says so before anything is
/// written, rather than letting `service.create` refuse halfway through.
#[tokio::test]
async fn a_package_this_home_does_not_have_is_planned_as_an_install() {
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

    let step = install_of(&planned, "mariadb");
    assert_eq!(step.disposition, Disposition::Create);

    let PlanAction::InstallPackage { package, wanted } = &step.action else {
        panic!("an install step");
    };
    assert_eq!(package, "mariadb");
    assert_eq!(
        wanted.as_ref().map(VersionConstraint::as_str),
        Some("11.4.3")
    );
}

/// And a home that already has it needs nothing.
#[tokio::test]
async fn a_package_already_on_disk_needs_no_install() {
    let (temp, store) = home().await;
    an_installed_mariadb(&store, "11.4.3").await;

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

    assert_eq!(
        install_of(&planned, "mariadb").disposition,
        Disposition::Satisfied
    );
}

/// **D2.** A second apply of the same blueprint is how a failed one is resumed, so the project
/// this apply would make — already made, at this root — is not a collision. It is the first half
/// of this apply having succeeded.
#[tokio::test]
async fn the_project_this_apply_already_made_is_satisfied_rather_than_blocked() {
    let (temp, store) = home().await;
    let root = temp.path().join("shop");
    a_project(&store, "shop", &root).await;

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

    assert_eq!(
        step_of(&planned, |action| matches!(
            action,
            PlanAction::RegisterProject { .. }
        ))
        .disposition,
        Disposition::Satisfied
    );
}

/// And the narrowness is the point: the same name somewhere else is still two projects
/// colliding, which is what the block was written for.
#[tokio::test]
async fn the_same_name_at_another_root_is_still_blocked() {
    let (temp, store) = home().await;
    a_project(&store, "shop", &temp.path().join("elsewhere")).await;

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

    assert!(matches!(
        step_of(&planned, |action| matches!(
            action,
            PlanAction::RegisterProject { .. }
        ))
        .disposition,
        Disposition::Blocked { .. }
    ));
}

/// A name this project's own site already answers to is done, not taken — which is what makes
/// the domain half of a resumed apply pass. Any other site holding it is still `Blocked`, and
/// the test above this one says so.
#[tokio::test]
async fn a_domain_this_projects_own_site_holds_is_satisfied() {
    let (temp, store) = home().await;
    let root = temp.path().join("shop");
    let project = a_project(&store, "shop", &root).await;

    sites::create(
        &store,
        &sites::NewSite {
            owner: crate::sites::SiteOwner::Project(project),
            doc_root: "public".to_owned(),
            kind: SiteKind::PhpFpm { pool: None },
            https_enabled: true,
            https_redirect: false,
            domains: vec!["shop.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");

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

    assert_eq!(
        step_of(&planned, |action| matches!(
            action,
            PlanAction::AddDomain { .. }
        ))
        .disposition,
        Disposition::Satisfied
    );
}

/// **D7.** The project a blueprint makes is pinned to what the blueprint asked for; without
/// this the site resolves to whatever PHP this machine defaults to, and a capture of the new
/// project comes back with no `[runtimes]` at all.
#[tokio::test]
async fn the_project_is_registered_with_the_pins_the_blueprint_asks_for() {
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

    let PlanAction::RegisterProject { pins, .. } = &step_of(&planned, |action| {
        matches!(action, PlanAction::RegisterProject { .. })
    })
    .action
    else {
        panic!("a register step");
    };

    assert_eq!(
        pins.get(&RuntimeKind::Php).map(VersionConstraint::as_str),
        Some("8.2.23")
    );
}

/// **D6 and D7 are one decision.** "Use the installed one" is not just a skipped download: it is
/// what the project asks for from now on, or the two answers would leave identical machines
/// behind and the question would be theatre.
#[tokio::test]
async fn answering_use_installed_pins_the_project_to_the_version_this_machine_has() {
    let (temp, store) = home().await;
    an_installed_php(&store, "8.2.29", "{}").await;

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(a_manifest()),
            project: "shop",
            root: &temp.path().join("shop"),
            answers: &[VersionAnswer {
                subject: AnswerSubject::Runtime {
                    kind: RuntimeKind::Php,
                },
                answer: MismatchAnswer::UseInstalled,
            }],
            scaffold_path: nowhere(),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    assert_eq!(
        step_of(&planned, |action| matches!(
            action,
            PlanAction::InstallRuntime { .. }
        ))
        .disposition,
        Disposition::Satisfied,
        "the answer settles the question, so nothing is left to ask"
    );

    let PlanAction::RegisterProject { pins, .. } = &step_of(&planned, |action| {
        matches!(action, PlanAction::RegisterProject { .. })
    })
    .action
    else {
        panic!("a register step");
    };

    assert_eq!(
        pins.get(&RuntimeKind::Php).map(VersionConstraint::as_str),
        Some("8.2.29"),
        "the pin follows the answer"
    );
}

/// The other answer leaves the pin where the blueprint put it, and turns the question into work.
#[tokio::test]
async fn answering_install_keeps_the_blueprints_own_pin() {
    let (temp, store) = home().await;
    an_installed_php(&store, "8.2.29", "{}").await;

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(a_manifest()),
            project: "shop",
            root: &temp.path().join("shop"),
            answers: &[VersionAnswer {
                subject: AnswerSubject::Runtime {
                    kind: RuntimeKind::Php,
                },
                answer: MismatchAnswer::Install,
            }],
            scaffold_path: nowhere(),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    assert_eq!(
        step_of(&planned, |action| matches!(
            action,
            PlanAction::InstallRuntime { .. }
        ))
        .disposition,
        Disposition::Create
    );

    let PlanAction::RegisterProject { pins, .. } = &step_of(&planned, |action| {
        matches!(action, PlanAction::RegisterProject { .. })
    })
    .action
    else {
        panic!("a register step");
    };

    assert_eq!(
        pins.get(&RuntimeKind::Php).map(VersionConstraint::as_str),
        Some("8.2.23")
    );
}

/// **An existing instance cannot be moved to another version by this build**, and the plan is
/// where that is said — a blocked step naming the way out, rather than a failure five actions
/// into a project directory (D10).
#[tokio::test]
async fn asking_to_install_over_an_existing_instance_is_blocked_and_names_the_other_answer() {
    let (temp, store) = home().await;
    an_installed_mariadb(&store, "11.4.5").await;

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(a_manifest()),
            project: "shop",
            root: &temp.path().join("shop"),
            answers: &[VersionAnswer {
                subject: AnswerSubject::Service {
                    id: ServiceId::parse("mariadb@main").expect("an id"),
                },
                answer: MismatchAnswer::Install,
            }],
            scaffold_path: nowhere(),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    assert!(
        matches!(
            &ensure_of(&planned, "mariadb").disposition,
            Disposition::Blocked { reason } if reason.contains("use_installed")
        ),
        "{planned:?}"
    );
}
