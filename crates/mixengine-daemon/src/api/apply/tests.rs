use mixengine_proto::{AnswerSubject, Disposition, PlanStep, ServiceId, VersionAnswer};
use mixengine_proto::{MismatchAnswer, PackageVersion, RuntimeKind, VersionConstraint};

use super::*;

/// The only writer of a job's log ring was once the scaffold step, so an apply that failed before
/// it — which is every apply that fails on a name — had an empty log, and a client's "show output"
/// button had nothing to show. The stream is part of the line rather than a detail of the
/// rendering: a client colouring its output needs to know which lines went badly without reading
/// them.
#[test]
fn a_step_is_narrated_on_the_stream_that_says_whether_it_went_well() {
    let starting = narration(Stream::Stdout, "installing php 8.4");

    assert_eq!(starting.stream, Stream::Stdout);
    assert_eq!(starting.text, "installing php 8.4");

    let failed = narration(Stream::Stderr, "creating the database shop failed: nope");

    assert_eq!(failed.stream, Stream::Stderr);
    assert!(failed.text.contains("nope"), "{}", failed.text);
}

fn step(action: PlanAction, disposition: Disposition) -> PlanStep {
    PlanStep {
        action,
        disposition,
        elevates: false,
    }
}

/// A plan whose last step is a blueprint's own command.
fn a_plan_with_a_scaffold(command: &str, trusted: bool) -> BlueprintPlan {
    let mut plan = a_plan(vec![step(
        PlanAction::RunScaffold {
            command: command.to_owned(),
        },
        Disposition::Confirm {
            what: command.to_owned(),
        },
    )]);

    plan.trusted = trusted;
    plan.source = match trusted {
        true => mixengine_proto::BlueprintSource::Captured,
        false => mixengine_proto::BlueprintSource::Imported,
    };

    plan
}

/// A plan whose command is blocked because its program is not on the PATH.
fn a_plan_with_a_blocked_scaffold(command: &str) -> BlueprintPlan {
    let mut plan = a_plan_with_a_scaffold(command, true);
    plan.steps[0].disposition = Disposition::Blocked {
        reason: "`composer` is not on the PATH the command would run with (<home>/bin, then \
                     the daemon's own PATH)"
            .to_owned(),
    };
    plan
}

/// **A blocked scaffold blocks the scaffold, not the apply.** Without a consent the plan is work,
/// and the step is left with its reason.
#[test]
fn a_blocked_scaffold_without_a_consent_is_not_a_refusal() {
    let plan = a_plan_with_a_blocked_scaffold("composer create-project laravel/laravel .");

    assert!(refusal(&[], &plan, &[]).is_none());
    assert!(consent_refusal(&plan, None).is_none());
}

/// With a consent it is refused up front, naming the program and what to do about it — and
/// never suggesting `bin/`, which is swept at every start.
#[test]
fn a_blocked_scaffold_with_a_consent_is_refused_naming_the_program() {
    let plan = a_plan_with_a_blocked_scaffold("composer create-project laravel/laravel .");

    let refused = consent_refusal(
        &plan,
        Some(&ScaffoldConsent {
            command: "composer create-project laravel/laravel .".to_owned(),
            untrusted: false,
            archive: None,
        }),
    )
    .expect("a refusal");

    assert_eq!(refused.code, ErrorCode::PreconditionFailed);
    assert!(refused.message.contains("`composer`"), "{refused:?}");
    let hint = refused.hint.as_deref().unwrap_or_default();
    assert!(hint.contains("restart"), "{hint}");
    assert!(!hint.contains("bin/"), "{hint}");
}

/// **A consent naming another command is consent to something else.** A blueprint can be
/// re-imported between the plan a person read and the apply they sent.
#[test]
fn a_consent_naming_another_command_refuses_the_apply() {
    let plan = a_plan_with_a_scaffold("composer create-project laravel/laravel shop", true);

    let refused = consent_refusal(
        &plan,
        Some(&ScaffoldConsent {
            command: "rm -rf /".to_owned(),
            untrusted: false,
            archive: None,
        }),
    )
    .expect("it is refused");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert!(
        refused.message.contains("composer create-project"),
        "{refused:?}"
    );
}

/// The half that matters: a blueprint re-imported without its signature would otherwise have
/// its command run under a consent given for a signed one.
#[test]
fn a_consent_that_thought_the_blueprint_was_signed_refuses_an_untrusted_one() {
    let plan = a_plan_with_a_scaffold("printf hello", false);

    let refused = consent_refusal(
        &plan,
        Some(&ScaffoldConsent {
            command: "printf hello".to_owned(),
            untrusted: false,
            archive: None,
        }),
    )
    .expect("it is refused");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert!(refused.message.contains("nothing vouches"), "{refused:?}");
}

/// And the other direction, which is the same fact: a consent given about an untrusted
/// blueprint is not an answer about a signed one.
#[test]
fn a_consent_that_thought_the_blueprint_was_untrusted_refuses_a_signed_one() {
    let plan = a_plan_with_a_scaffold("printf hello", true);

    let refused = consent_refusal(
        &plan,
        Some(&ScaffoldConsent {
            command: "printf hello".to_owned(),
            untrusted: true,
            archive: None,
        }),
    )
    .expect("it is refused");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
}

/// A consent for a plan that runs no command at all answers a plan made against something else.
#[test]
fn a_consent_for_a_plan_with_no_command_is_refused() {
    let plan = a_plan(vec![step(php(), Disposition::Create)]);

    assert!(
        consent_refusal(
            &plan,
            Some(&ScaffoldConsent {
                command: "printf hello".to_owned(),
                untrusted: false,
                archive: None,
            }),
        )
        .is_some()
    );
}

/// **A consent names the archive it agreed to.**
#[test]
fn a_consent_naming_the_archive_is_accepted_and_another_url_is_refused() {
    let url = "https://wordpress.org/latest.zip";
    let mut plan = a_plan(vec![step(
        PlanAction::FetchArchive {
            url: url.to_owned(),
            strip: None,
        },
        Disposition::Confirm {
            what: url.to_owned(),
        },
    )]);
    plan.trusted = true;
    plan.source = mixengine_proto::BlueprintSource::Captured;

    let agreed = ScaffoldConsent {
        command: String::new(),
        archive: Some(url.to_owned()),
        untrusted: false,
    };
    assert!(consent_refusal(&plan, Some(&agreed)).is_none());

    let other = ScaffoldConsent {
        archive: Some("https://evil.example/latest.zip".to_owned()),
        ..agreed
    };
    assert!(consent_refusal(&plan, Some(&other)).is_some());
}

/// **No consent is not a refusal**: everything else is applied and the step says what was left.
#[test]
fn no_consent_is_not_a_refusal() {
    let plan = a_plan_with_a_scaffold("printf hello", true);

    assert!(consent_refusal(&plan, None).is_none());
}

/// A consent that matches the plan and what was said about it is work.
#[test]
fn a_matching_consent_is_work() {
    let plan = a_plan_with_a_scaffold("printf hello", false);

    assert!(
        consent_refusal(
            &plan,
            Some(&ScaffoldConsent {
                command: "printf hello".to_owned(),
                untrusted: true,
                archive: None,
            }),
        )
        .is_none()
    );
}

/// **A consent names what was read**, `consent_refusal`'s rule for a `.env` key.
#[test]
fn a_key_the_plan_does_not_write_is_refused() {
    let plan = a_plan(vec![step(
        PlanAction::WriteDotenv {
            key: "DATABASE_URL".to_owned(),
            path: ".env".to_owned(),
        },
        Disposition::Confirm {
            what: "DATABASE_URL in .env".to_owned(),
        },
    )]);
    assert!(dotenv_refusal(&plan, &["DATABASE_URL".to_owned()]).is_none());
    assert!(dotenv_refusal(&plan, &[]).is_none());
    let refused = dotenv_refusal(&plan, &["OTHER".to_owned()]).expect("refused");
    assert!(refused.message.contains("OTHER"), "{}", refused.message);
}

/// **The account the apply made**, which the step may have renamed.
#[test]
fn the_dotenv_url_names_the_account_the_apply_made() {
    let made = AppliedDatabase {
        service: mixengine_proto::ServiceId::parse("postgres@main").expect("an id"),
        database: "blog".to_owned(),
        user: "blog-2".to_owned(),
    };
    assert_eq!(
        dotenv_value(
            &made,
            mixengine_proto::DatabaseProtocol::Postgres,
            "127.0.0.1",
            5433,
            "p@ss"
        ),
        "postgres://blog-2:p%40ss@127.0.0.1:5433/blog"
    );
}

fn a_plan(steps: Vec<PlanStep>) -> BlueprintPlan {
    BlueprintPlan {
        blueprint: "blog-stack".to_owned(),
        project: "shop".to_owned(),
        root: "/tmp/shop".to_owned(),
        steps,
        source: mixengine_proto::BlueprintSource::Captured,
        trusted: true,
        signature: None,
    }
}

fn php() -> PlanAction {
    PlanAction::InstallRuntime {
        kind: RuntimeKind::Php,
        wanted: VersionConstraint::parse("8.2.23").expect("a constraint"),
    }
}

fn a_php_question() -> Disposition {
    Disposition::Choice {
        installed: PackageVersion::parse("8.2.29").expect("a version"),
        wanted: VersionConstraint::parse("8.2.23").expect("a constraint"),
    }
}

/// A plan with nothing in its way and nothing to ask is one an apply may start.
#[test]
fn a_plan_that_is_all_work_is_not_refused() {
    let plan = a_plan(vec![step(php(), Disposition::Create)]);

    assert!(refusal(&[], &plan, &[]).is_none());
}

/// **The whole point of having planned first**: a blocked step is refused here, naming what
/// stands in the way, rather than five actions into a project directory.
#[test]
fn a_blocked_step_refuses_the_apply_and_says_what_is_in_the_way() {
    let plan = a_plan(vec![step(
        PlanAction::AddDomain {
            domain: "shop.test".to_owned(),
            primary: true,
        },
        Disposition::Blocked {
            reason: "shop.test is already answered by blog.test".to_owned(),
        },
    )]);

    let refused = refusal(&[], &plan, &[]).expect("a refusal");

    assert_eq!(refused.code, ErrorCode::PreconditionFailed);
    assert!(refused.message.contains("shop.test"), "{refused:?}");
}

/// A question nobody answered is a question, and a daemon has no keyboard.
#[test]
fn an_unanswered_question_refuses_the_apply_and_names_the_question() {
    let plan = a_plan(vec![step(php(), a_php_question())]);

    let refused = refusal(
        &[AnswerSubject::Runtime {
            kind: RuntimeKind::Php,
        }],
        &plan,
        &[],
    )
    .expect("a refusal");

    assert!(refused.message.contains("php"), "{refused:?}");
    assert!(
        refused
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("--use-installed")),
        "{refused:?}"
    );
}

/// The one that was asked and answered stops being a refusal, and it is matched by subject —
/// so an answer about PHP does not settle a question about MariaDB.
#[test]
fn an_answer_settles_its_own_question_and_no_other() {
    let plan = a_plan(vec![step(php(), a_php_question())]);
    let asked = [
        AnswerSubject::Runtime {
            kind: RuntimeKind::Php,
        },
        AnswerSubject::Service {
            id: ServiceId::parse("mariadb@main").expect("an id"),
        },
    ];

    let refused = refusal(
        &asked,
        &plan,
        &[VersionAnswer {
            subject: AnswerSubject::Runtime {
                kind: RuntimeKind::Php,
            },
            answer: MismatchAnswer::UseInstalled,
        }],
    )
    .expect("the other question is still open");

    assert!(refused.message.contains("mariadb@main"), "{refused:?}");
    assert!(!refused.message.contains("php"), "{refused:?}");
}

/// **Somebody answering a question nobody asked is somebody holding a plan that has since
/// changed**, and saying so is cheaper than applying their answer to nothing.
#[test]
fn an_answer_to_a_question_this_plan_does_not_ask_is_refused_by_name() {
    let plan = a_plan(vec![step(php(), Disposition::Satisfied)]);

    let refused = refusal(
        &[],
        &plan,
        &[VersionAnswer {
            subject: AnswerSubject::Runtime {
                kind: RuntimeKind::Php,
            },
            answer: MismatchAnswer::UseInstalled,
        }],
    )
    .expect("a refusal");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert!(refused.message.contains("php"), "{refused:?}");
}

/// The pair the plan carried apart becomes an id here, the same way the plan checked it could:
/// a single-instance package is its own id, and everything else is `package@instance`.
#[test]
fn a_package_and_an_instance_become_the_id_the_plan_checked() {
    assert_eq!(
        identity("mariadb", "main").expect("an id").as_str(),
        "mariadb@main"
    );
    assert_eq!(identity("caddy", "caddy").expect("an id").as_str(), "caddy");
    assert!(identity("mariadb", "My Blog").is_err());
}

/// The questions are read off the actions, and only where the disposition is one.
#[test]
fn only_a_step_that_asks_becomes_a_question() {
    let plan = a_plan(vec![
        step(php(), a_php_question()),
        step(
            PlanAction::EnsureService {
                package: "mariadb".to_owned(),
                instance: "main".to_owned(),
                version: None,
                dedicated: false,
            },
            Disposition::Satisfied,
        ),
    ]);

    assert_eq!(
        questions(&plan),
        vec![AnswerSubject::Runtime {
            kind: RuntimeKind::Php
        }]
    );
}
