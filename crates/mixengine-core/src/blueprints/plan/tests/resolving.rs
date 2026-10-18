//! What a plan finds already done and what it has to do: the project, its runtimes, packages,
//! extensions, sites and the versions it pins.

use super::*;
use crate::sites;

/// **A plan names the project the way it will be registered** — roadmap task **T128**.
///
/// `projects::validated_name` trims, so `project.create` stores `Laravel`; the plan used to
/// carry `Laravel ` verbatim and hand it on to three readers that each compare it by equality.
/// Reported from a real apply, where the rollback of a failed one warned
/// `could not take back … Project { name: "Laravel " }: no such project: Laravel ` — it was
/// looking for a name nothing had ever been stored under.
#[tokio::test]
async fn a_name_is_planned_the_way_it_will_be_stored() {
    let (temp, store) = home().await;

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(a_manifest()),
            project: "  Laravel ",
            root: &temp.path().join("laravel"),
            answers: &[],
            scaffold_path: nowhere(),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    assert_eq!(
        planned.project, "Laravel",
        "what a client renders, and what the daemon carries as this apply's project handle"
    );

    let registered = step_of(&planned, |action| {
        matches!(action, PlanAction::RegisterProject { .. })
    });

    assert!(
        matches!(&registered.action, PlanAction::RegisterProject { name, .. } if name == "Laravel"),
        "{:?}",
        registered.action
    );
}

/// And the same name, applied twice, is the resumption it is — **T128**.
///
/// The comparisons in [`register`] are equalities against `projects.name`, so an untrimmed name
/// matched nothing: a second apply of a project this one had already registered planned a
/// `Create` against a root that was taken, and was blocked over its own first attempt.
#[tokio::test]
async fn an_untrimmed_name_resumes_the_project_it_already_registered() {
    let (temp, store) = home().await;
    let root = temp.path().join("laravel");
    std::fs::create_dir(&root).expect("the first apply's directory");

    projects::create(
        &store,
        &projects::Registration {
            name: "Laravel".to_owned(),
            root: root.clone(),
            pins: BTreeMap::new(),
        },
        mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
    )
    .await
    .expect("the first apply's project");

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(a_manifest()),
            project: "Laravel ",
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
        Disposition::Satisfied,
        "a project of this name at this root is this apply's own first step, already taken"
    );
}

/// **A registered project whose directory is gone is not already true.** The step that makes
/// the directory is this one, so planning it `Satisfied` left the blueprint's own command to be
/// started in a folder that was not there — which Windows reports as a shell that cannot start.
#[tokio::test]
async fn a_resumed_project_whose_directory_was_deleted_is_work_again() {
    let (temp, store) = home().await;
    let root = temp.path().join("laravel");

    projects::create(
        &store,
        &projects::Registration {
            name: "Laravel".to_owned(),
            root: root.clone(),
            pins: BTreeMap::new(),
        },
        mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
    )
    .await
    .expect("the first apply's project");

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(a_manifest()),
            project: "Laravel",
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
        Disposition::Create,
        "the directory has to be made again before anything runs in it"
    );
}

/// Everything the blueprint needs is already here, so only the new project's own things are
/// created — and `{project}` is expanded exactly once, into the domain.
#[tokio::test]
async fn a_home_that_already_has_everything_needs_nothing_installed() {
    let (temp, store) = home().await;
    an_installed_php(&store, "8.2.23", r#"{"xdebug":true}"#).await;
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
        step_of(&planned, |action| matches!(
            action,
            PlanAction::InstallRuntime { .. }
        ))
        .disposition,
        Disposition::Satisfied
    );
    assert_eq!(
        ensure_of(&planned, "mariadb").disposition,
        Disposition::Satisfied
    );
    assert_eq!(
        step_of(&planned, |action| matches!(
            action,
            PlanAction::SetPhpExtension { .. }
        ))
        .disposition,
        Disposition::Satisfied
    );

    let PlanAction::AddDomain { domain, .. } = &step_of(&planned, |action| {
        matches!(action, PlanAction::AddDomain { .. })
    })
    .action
    else {
        panic!("a domain step");
    };
    assert_eq!(domain, "shop.test");

    let PlanAction::CreateDatabase { database, user, .. } = &step_of(&planned, |action| {
        matches!(action, PlanAction::CreateDatabase { .. })
    })
    .action
    else {
        panic!("a database step");
    };
    assert_eq!((database.as_str(), user.as_str()), ("shop", "shop"));
}

/// The PHP an extension step names, from a plan of [`a_manifest`] against this home.
async fn the_extensions_php(store: &Store, root: &Path) -> Option<PackageVersion> {
    let planned = plan(
        store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(a_manifest()),
            project: "shop",
            root,
            answers: &[],
            scaffold_path: nowhere(),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    let PlanAction::SetPhpExtension { runtime, .. } = &step_of(&planned, |action| {
        matches!(action, PlanAction::SetPhpExtension { .. })
    })
    .action
    else {
        panic!("an extension step");
    };

    runtime.clone()
}

/// **A home with no PHP names no PHP.** The runtime step above installs it, so which one it is
/// is known only once that step has run — and the apply resolves it then. A placeholder version
/// here was carried into the apply verbatim, and every Laravel on a fresh machine reported
/// `no such runtime: php 0.0.0`.
#[tokio::test]
async fn an_extension_on_a_home_without_php_names_no_version() {
    let (temp, store) = home().await;

    assert_eq!(
        the_extensions_php(&store, &temp.path().join("shop")).await,
        None
    );
}

/// **The PHP the project pins, not the newest one here.** An extension turned on for 8.4
/// is no use to a project that runs 8.2.
#[tokio::test]
async fn an_extension_goes_on_the_php_the_project_pins_rather_than_the_newest() {
    let (temp, store) = home().await;
    an_installed_php(&store, "8.2.23", "{}").await;
    another_installed_php(&store, 2, "8.4.1").await;

    assert_eq!(
        the_extensions_php(&store, &temp.path().join("shop")).await,
        Some(PackageVersion::parse("8.2.23").expect("a version"))
    );
}

/// A different patch release is a question for a person, not a decision for the daemon.
#[tokio::test]
async fn another_version_of_an_installed_runtime_is_a_choice_rather_than_an_install() {
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
                PlanAction::InstallRuntime { .. }
            ))
            .disposition,
            Disposition::Choice { installed, .. } if installed.as_str() == "8.2.29"
        ),
        "{planned:?}"
    );
}

/// Nothing of that language installed is an install, without a size: the plan never asks the
/// index (D9).
#[tokio::test]
async fn a_runtime_this_home_does_not_have_is_an_install() {
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

    assert_eq!(
        step_of(&planned, |action| matches!(
            action,
            PlanAction::InstallRuntime { .. }
        ))
        .disposition,
        Disposition::Create
    );
}

/// **D10.** The owner is named, because "taken" without a name sends somebody hunting.
#[tokio::test]
async fn a_domain_another_site_owns_is_blocked_and_says_who_has_it() {
    let (temp, store) = home().await;

    let project = crate::projects::create(
        &store,
        &crate::projects::Registration {
            name: "blog".to_owned(),
            root: temp.path().join("blog"),
            pins: BTreeMap::new(),
        },
        mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
    )
    .await
    .expect("a project");

    crate::sites::create(
        &store,
        &crate::sites::NewSite {
            owner: crate::sites::SiteOwner::Project(project.id),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["shop.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site holding the name");

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

    let Disposition::Blocked { reason } = &step_of(&planned, |action| {
        matches!(action, PlanAction::AddDomain { .. })
    })
    .disposition
    else {
        panic!("the domain step should be blocked: {planned:?}");
    };

    assert!(reason.contains("shop.test"), "{reason}");
}

/// **D10 again**: a name whose expansion cannot be a database account is refused at dry-run,
/// not five actions into an apply.
///
/// **T120** moved the rule itself: the limit is enforced by the function that owns it rather
/// than by a copy of its number here, so the sentence is now `validated_identifier`'s own.
#[tokio::test]
async fn a_project_name_too_long_for_a_database_account_is_blocked_here() {
    let (temp, store) = home().await;
    let long = "a".repeat(40);

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "blog-stack",
            filed: &captured(a_manifest()),
            project: &long,
            root: &temp.path().join("long"),
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

    assert!(reason.contains("thirty-two"), "{reason}");
}

/// **D8.** The order is what T78 executes, so it is asserted rather than left to chance.
#[tokio::test]
async fn the_steps_are_in_dependency_order() {
    let (temp, store) = home().await;
    let mut manifest = two_sites();
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
            front_end: true,
        },
    )
    .await
    .expect("a plan");

    // **Tiers, not one rank per action kind** — and the correction is the point. Install,
    // ensure and create-database are *one* tier because the planner walks a service at a time:
    // a manifest with two `[[services]]` has always produced install, ensure, install, ensure,
    // and a rank per kind only looked monotonic because this fixture had one service. T115 made
    // that visible by adding a front end ahead of them, and what it broke was the assertion and
    // never the order.
    let tier = |action: &PlanAction| match action {
        PlanAction::RegisterProject { .. } => 0,
        PlanAction::InstallRuntime { .. } => 1,
        PlanAction::InstallPackage { .. }
        | PlanAction::EnsureService { .. }
        | PlanAction::CreateDatabase { .. } => 2,
        // **One tier for the sites** (T204a, D4): each site is its own group inside it, which
        // the assertion at the end of this test says.
        PlanAction::CreateSite { .. }
        | PlanAction::AddDomain { .. }
        | PlanAction::IssueCertificate { .. } => 3,
        PlanAction::SetPhpExtension { .. } => 4,
        PlanAction::RunScaffold { .. } | PlanAction::FetchArchive { .. } => 5,
        _ => 6,
    };

    let tiers: Vec<_> = planned
        .steps
        .iter()
        .map(|step| tier(&step.action))
        .collect();

    assert!(
        tiers.windows(2).all(|pair| pair[0] <= pair[1]),
        "{tiers:?} is not dependency order"
    );

    // And inside that tier, each service is still install → ensure → database, which is the
    // half the tiers above no longer say. Read per package, so two services interleaving
    // cannot hide one of them being out of order.
    for package in ["caddy", "mariadb"] {
        let positions: Vec<usize> = planned
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| match &step.action {
                PlanAction::InstallPackage { package: named, .. }
                | PlanAction::EnsureService { package: named, .. }
                | PlanAction::CreateDatabase { package: named, .. } => named == package,
                _ => false,
            })
            .map(|(position, _)| position)
            .collect();

        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{package}'s steps are out of order: {positions:?}"
        );
    }

    // **Inside the sites tier, each site is its own group** (T204a, D4): create, primary,
    // aliases, certificate — and the next site starts only after.
    assert_eq!(
        site_words(&planned),
        [
            "site",
            "shop.test",
            "www.shop.test",
            "cert",
            "site",
            "vite.shop.test",
            "cert"
        ]
    );
}

/// Two sites: the PHP app with MariaDB, and a Vite proxy that links nothing (T204a).
fn two_sites() -> BlueprintManifest {
    let mut manifest = a_manifest();
    manifest.schema = 3;
    manifest.sites[0].services = Some(vec!["mariadb".to_owned()]);
    manifest.sites[0].aliases = vec!["www.{project}.test".to_owned()];
    manifest.sites.push(BlueprintSite {
        kind: SiteKind::ReverseProxy {
            upstream: "http://127.0.0.1:5173".to_owned(),
        },
        doc_root: String::new(),
        https: true,
        domain_pattern: "vite.{project}.test".to_owned(),
        aliases: Vec::new(),
        routes: Vec::new(),
        services: Some(Vec::new()),
    });
    manifest
}

async fn planned_two(store: &Store, temp: &tempfile::TempDir, root: &Path) -> BlueprintPlan {
    plan(
        store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "two",
            filed: &captured(two_sites()),
            project: "shop",
            root,
            answers: &[],
            scaffold_path: &a_path_holding(temp, &[]),
            front_end: false,
        },
    )
    .await
    .expect("a plan")
}

/// The site actions in order, one short word each, for the order assertions.
fn site_words(planned: &BlueprintPlan) -> Vec<String> {
    planned
        .steps
        .iter()
        .filter_map(|step| match &step.action {
            PlanAction::CreateSite { .. } => Some("site".to_owned()),
            PlanAction::AddDomain { domain, .. } => Some(domain.clone()),
            PlanAction::IssueCertificate { .. } => Some("cert".to_owned()),
            _ => None,
        })
        .collect()
}

/// The disposition of every `CreateSite`, in order.
fn site_dispositions(planned: &BlueprintPlan) -> Vec<Disposition> {
    planned
        .steps
        .iter()
        .filter(|step| matches!(step.action, PlanAction::CreateSite { .. }))
        .map(|step| step.disposition.clone())
        .collect()
}

/// A site of `project` answering to `domains`.
async fn a_site_holding(store: &Store, project: i64, domains: &[&str]) {
    sites::create(
        store,
        &sites::NewSite {
            owner: crate::sites::SiteOwner::Project(project),
            doc_root: "public".to_owned(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: domains.iter().map(|domain| (*domain).to_owned()).collect(),
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");
}

/// **T204a, D4.** One group per site, in file order — the executor reads a site's names off
/// the steps straight after it.
#[tokio::test]
async fn several_sites_plan_one_group_each_in_file_order() {
    let (temp, store) = home().await;
    let planned = planned_two(&store, &temp, &temp.path().join("shop")).await;

    assert_eq!(
        site_words(&planned),
        [
            "site",
            "shop.test",
            "www.shop.test",
            "cert",
            "site",
            "vite.shop.test",
            "cert"
        ]
    );
}

/// **T204a, D3.** Each site carries what it links, as the ensure steps name it; `[site]`
/// carries `None`.
#[tokio::test]
async fn each_site_carries_its_own_links() {
    let (temp, store) = home().await;
    let planned = planned_two(&store, &temp, &temp.path().join("shop")).await;

    let links: Vec<_> = planned
        .steps
        .iter()
        .filter_map(|step| match &step.action {
            PlanAction::CreateSite { services, .. } => Some(services.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        links,
        vec![
            Some(vec![ServiceId::parse("mariadb@main").expect("an id")]),
            Some(Vec::new()),
        ]
    );

    let one = planned_for(&store, &temp, &temp.path().join("one"), a_manifest()).await;
    assert!(
        one.steps
            .iter()
            .any(|step| matches!(&step.action, PlanAction::CreateSite { services: None, .. }))
    );
}

/// **T204a, D3.** `per-project` links the project's own instance, as its ensure names it.
#[tokio::test]
async fn a_link_to_a_dedicated_instance_names_the_projects_own() {
    let (temp, store) = home().await;
    let mut manifest = two_sites();
    manifest.services[0].instance = Some(PER_PROJECT.to_owned());

    let planned = plan(
        &store,
        &Catalogue::builtin(),
        &Wanted {
            blueprint: "two",
            filed: &captured(manifest),
            project: "shop",
            root: &temp.path().join("shop"),
            answers: &[],
            scaffold_path: &a_path_holding(&temp, &[]),
            front_end: false,
        },
    )
    .await
    .expect("a plan");

    let first = planned
        .steps
        .iter()
        .find_map(|step| match &step.action {
            PlanAction::CreateSite { services, .. } => Some(services.clone()),
            _ => None,
        })
        .expect("a site");
    assert_eq!(
        first,
        Some(vec![ServiceId::parse("mariadb@shop").expect("an id")])
    );
}

/// **T204a, D4.** A resumed apply: site 1 is there, found by an alias; site 2 is not.
#[tokio::test]
async fn a_site_already_made_is_satisfied_by_any_of_its_names_and_the_other_is_work() {
    let (temp, store) = home().await;
    let root = temp.path().join("shop");
    let project = a_project(&store, "shop", &root).await;
    a_site_holding(&store, project, &["www.shop.test"]).await;

    let planned = planned_two(&store, &temp, &root).await;

    assert_eq!(
        site_dispositions(&planned),
        vec![Disposition::Satisfied, Disposition::Create]
    );
}

/// **T204a, D4.** One site holding two entries' names is not a guess an apply makes.
#[tokio::test]
async fn one_site_answering_for_two_entries_blocks_the_second() {
    let (temp, store) = home().await;
    let root = temp.path().join("shop");
    let project = a_project(&store, "shop", &root).await;
    a_site_holding(&store, project, &["shop.test", "vite.shop.test"]).await;

    let planned = planned_two(&store, &temp, &root).await;
    let dispositions = site_dispositions(&planned);

    assert_eq!(dispositions[0], Disposition::Satisfied);
    match &dispositions[1] {
        Disposition::Blocked { reason } => assert!(
            reason.contains("shop.test") && reason.contains("vite.shop.test"),
            "{reason}"
        ),
        other => panic!("not blocked: {other:?}"),
    }
}

/// **T204a, D4.** A one-site blueprint keeps the older question: any site of this project.
#[tokio::test]
async fn a_one_site_blueprint_resumes_on_any_site_of_the_project() {
    let (temp, store) = home().await;
    let root = temp.path().join("shop");
    let project = a_project(&store, "shop", &root).await;
    a_site_holding(&store, project, &["renamed.test"]).await;

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

    assert_eq!(site_dispositions(&planned), vec![Disposition::Satisfied]);
}
