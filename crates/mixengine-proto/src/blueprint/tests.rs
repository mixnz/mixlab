use super::*;

/// T202, D2. A `done` written by a build from before the note still reads, a `done` with
/// nothing to say still writes as before, and a note round-trips.
#[test]
fn a_done_step_reads_without_a_note_and_carries_one() {
    let bare: StepResult = serde_json::from_str(r#"{"result":"done"}"#).expect("reads");
    assert_eq!(bare, StepResult::Done { note: None });

    assert_eq!(
        serde_json::to_value(StepResult::Done { note: None }).expect("writes"),
        serde_json::json!({ "result": "done" })
    );

    let said = "the account shop is somebody else's, so this project's is shop-2";
    let json = serde_json::to_value(StepResult::Done {
        note: Some(said.to_owned()),
    })
    .expect("writes");
    assert_eq!(json["result"], "done");
    assert_eq!(json["note"], said);
}

/// **T152.** A dry run from before T152 has no `needs`, and still parses.
#[test]
fn a_planned_answer_without_needs_is_the_answer_it_always_was() {
    let planned: BlueprintApplyResponse = serde_json::from_value(serde_json::json!({
        "outcome": "planned",
        "plan": {
            "blueprint": "laravel", "project": "blog", "root": "/tmp/blog", "steps": []
        }
    }))
    .expect("the shape every client has read since T77");

    assert!(matches!(
        planned,
        BlueprintApplyResponse::Planned { needs: None, .. }
    ));
}

/// The discriminator travels inside the object, so a client switches on one field — the shape
/// [`crate::DaemonEvent`] and [`SiteKind`] already use, and the reason a variant added in a
/// later phase arrives at an older client as an object it can ignore.
#[test]
fn an_action_and_its_disposition_are_flat_tagged_objects() {
    let step = PlanStep {
        action: PlanAction::InstallRuntime {
            kind: RuntimeKind::Php,
            wanted: VersionConstraint::parse("8.2.23").expect("a constraint"),
        },
        disposition: Disposition::Choice {
            installed: PackageVersion::parse("8.2.29").expect("a version"),
            wanted: VersionConstraint::parse("8.2.23").expect("a constraint"),
        },
        elevates: false,
    };

    let json = serde_json::to_value(&step).expect("a step encodes");

    assert_eq!(json["action"]["action"], "install_runtime");
    assert_eq!(json["action"]["wanted"], "8.2.23");
    assert_eq!(json["disposition"]["disposition"], "choice");
    assert_eq!(json["disposition"]["installed"], "8.2.29");

    let back: PlanStep = serde_json::from_value(json).expect("and decodes");
    assert_eq!(back, step);
}

/// One method, two answers. A client knows which it got from the object rather than from its
/// own request, which is what keeps `--dry-run` and a real apply one method (T77's argument,
/// one task on).
#[test]
fn an_apply_answers_either_a_plan_or_a_job() {
    let planned = BlueprintApplyResponse::Planned {
        plan: BlueprintPlan {
            blueprint: "blog-stack".to_owned(),
            project: "shop".to_owned(),
            root: "/tmp/shop".to_owned(),
            steps: Vec::new(),
            source: BlueprintSource::Captured,
            trusted: true,
            signature: None,
        },
        needs: None,
    };

    let json = serde_json::to_value(&planned).expect("it encodes");

    assert_eq!(json["outcome"], "planned");
    assert_eq!(json["plan"]["project"], "shop");
}

/// A step that did not run says why in words a person reads, because the whole of what T78
/// leaves undone — a `[scaffold]` command — is a sentence somebody has to act on.
#[test]
fn a_step_that_did_not_run_carries_its_reason() {
    let outcome = StepOutcome {
        action: PlanAction::RunScaffold {
            command: "composer create-project laravel/laravel .".to_owned(),
        },
        result: StepResult::NotRun {
            why: "running a blueprint's own command arrives with roadmap task T78a".to_owned(),
        },
    };

    let json = serde_json::to_value(&outcome).expect("it encodes");

    assert_eq!(json["result"]["result"], "not_run");
    assert!(
        json["result"]["why"]
            .as_str()
            .is_some_and(|why| why.contains("T78a")),
        "{json}"
    );
}

/// **A step that ran and failed is its own outcome** — roadmap task **T78a**, its design's D7,
/// and not the same fact as one nothing attempted.
#[test]
fn a_step_that_ran_and_failed_is_not_a_step_that_was_skipped() {
    let failed = StepOutcome {
        action: PlanAction::RunScaffold {
            command: "composer install".to_owned(),
        },
        result: StepResult::Failed {
            why: "it exited with 1".to_owned(),
        },
    };

    let json = serde_json::to_value(&failed).expect("it encodes");
    assert_eq!(json["result"]["result"], "failed");

    let back: StepOutcome = serde_json::from_value(json).expect("and decodes");
    assert_eq!(back, failed);
}

/// A plan says what it is applying, so a client shows the right words about a command it is
/// about to offer without a second call (D5).
#[test]
fn a_plan_carries_where_its_blueprint_came_from() {
    let plan = BlueprintPlan {
        blueprint: "borrowed".to_owned(),
        project: "shop".to_owned(),
        root: "/tmp/shop".to_owned(),
        steps: Vec::new(),
        source: BlueprintSource::Imported,
        trusted: false,
        signature: None,
    };

    let json = serde_json::to_value(&plan).expect("it encodes");
    assert_eq!(json["source"], "imported");
    assert_eq!(json["trusted"], false);

    // And a plan from before this task reads as what it could only have been.
    let older: BlueprintPlan = serde_json::from_value(serde_json::json!({
        "blueprint": "blog-stack",
        "project": "shop",
        "root": "/tmp/shop",
        "steps": []
    }))
    .expect("a plan older than this task");

    assert_eq!(older.source, BlueprintSource::Captured);
}

/// A source is a closed word, because the column stores it and T78a's trust marking reads it.
#[test]
fn a_source_is_one_of_three_words_in_both_directions() {
    assert_eq!(
        serde_json::to_value(BlueprintSource::Captured).expect("encodes"),
        serde_json::json!("captured")
    );

    assert_eq!(
        BlueprintSource::parse("imported"),
        Some(BlueprintSource::Imported)
    );
    assert_eq!(BlueprintSource::parse("borrowed"), None);
}

/// A version the blueprint does not pin is left out rather than sent as `null`: an older client
/// reading this reads *no version asked for*, which is what it means.
#[test]
fn a_service_without_a_pinned_version_carries_no_key_for_one() {
    let action = PlanAction::EnsureService {
        package: "redis".to_owned(),
        instance: "main".to_owned(),
        version: None,
        dedicated: false,
    };

    let json = serde_json::to_value(&action).expect("it encodes");

    assert!(json.get("version").is_none(), "{json}");
}

/// **The reason rides beside the answer, not instead of it** — roadmap task **T79b**.
/// [`BlueprintSummary::trusted`] is what behaviour reads; this is what a person is told.
#[test]
fn a_summary_says_why_it_is_not_trusted() {
    let summary = BlueprintSummary {
        slug: "borrowed".to_owned(),
        name: "borrowed".to_owned(),
        description: String::new(),
        created_at: "2026-09-01T00:00:00Z".to_owned(),
        source: BlueprintSource::Imported,
        trusted: false,
        signature: Some(SignatureCheck::Rejected),
        file: "/tmp/borrowed.toml".to_owned(),
    };

    let json = serde_json::to_value(&summary).expect("it encodes");
    assert_eq!(json["trusted"], false);
    assert_eq!(json["signature"], "rejected");

    let back: BlueprintSummary = serde_json::from_value(json).expect("and decodes");
    assert_eq!(back, summary);
}

/// A row older than this task carries no reason, and says so rather than guessing at one.
#[test]
fn a_summary_older_than_this_task_has_no_reason() {
    let older: BlueprintSummary = serde_json::from_value(serde_json::json!({
        "slug": "borrowed",
        "name": "borrowed",
        "description": "",
        "created_at": "2026-09-01T00:00:00Z",
        "source": "imported",
        "trusted": false,
        "file": "/tmp/borrowed.toml"
    }))
    .expect("a summary older than this task");

    assert_eq!(older.signature, None);
}

/// **A word this build does not know must not take the response down with it** — roadmap task
/// **T79b**, its design's D3. A `mix` older than a variant some later phase adds would
/// otherwise fail to parse a whole listing over the one field on it that is decoration.
#[test]
fn a_reason_this_build_does_not_know_is_no_reason() {
    let newer: BlueprintSummary = serde_json::from_value(serde_json::json!({
        "slug": "borrowed",
        "name": "borrowed",
        "description": "",
        "created_at": "2026-09-01T00:00:00Z",
        "source": "imported",
        "trusted": false,
        "signature": "revoked",
        "file": "/tmp/borrowed.toml"
    }))
    .expect("a summary from a newer daemon still decodes");

    assert_eq!(newer.signature, None);
    assert!(
        !newer.trusted,
        "the answer survives a reason it cannot read"
    );
}

/// The plan carries it too, because the `[scaffold]` question is where it matters most.
#[test]
fn a_plan_carries_why_its_blueprint_is_not_trusted() {
    let plan = BlueprintPlan {
        blueprint: "borrowed".to_owned(),
        project: "shop".to_owned(),
        root: "/tmp/shop".to_owned(),
        steps: Vec::new(),
        source: BlueprintSource::Imported,
        trusted: false,
        signature: Some(SignatureCheck::Missing),
    };

    let json = serde_json::to_value(&plan).expect("it encodes");
    assert_eq!(json["signature"], "missing");

    let back: BlueprintPlan = serde_json::from_value(json).expect("and decodes");
    assert_eq!(back, plan);
}

/// One spelling for the column and the wire, on [`BlueprintSource::as_str`]'s rule.
#[test]
fn a_reason_has_one_spelling() {
    for check in SignatureCheck::ALL {
        assert_eq!(SignatureCheck::parse(check.as_str()), Some(check));
    }

    assert_eq!(SignatureCheck::parse("revoked"), None);
}
#[test]
fn a_next_step_writes_only_what_it_carries() {
    let step = NextStep {
        kind: NextStepKind::Serve,
        run: Some("npm run dev".to_owned()),
        path: None,
        note: None,
        optional: false,
        credentials: false,
        site: None,
    };

    assert_eq!(
        serde_json::to_value(&step).expect("writes"),
        serde_json::json!({ "kind": "serve", "run": "npm run dev" })
    );

    let read: NextStep =
        serde_json::from_str(r#"{"kind":"open","path":"/cp","optional":true}"#).expect("reads");
    assert_eq!(read.kind, NextStepKind::Open);
    assert!(read.optional);
    assert!(!read.credentials);
}

#[test]
fn write_dotenv_is_tagged_like_the_other_actions() {
    let action = PlanAction::WriteDotenv {
        key: "DATABASE_URL".to_owned(),
        path: ".env".to_owned(),
    };
    let json = serde_json::to_value(&action).expect("encodes");
    assert_eq!(json["action"], "write_dotenv");
    assert_eq!(json["key"], "DATABASE_URL");
    assert_eq!(
        serde_json::from_value::<PlanAction>(json).expect("decodes"),
        action
    );
}

#[test]
fn fetch_archive_is_tagged_like_the_other_actions() {
    let action = PlanAction::FetchArchive {
        url: "https://wordpress.org/latest.zip".to_owned(),
        strip: Some("wordpress".to_owned()),
    };

    assert_eq!(
        serde_json::to_value(&action).expect("writes"),
        serde_json::json!({
            "action": "fetch_archive",
            "url": "https://wordpress.org/latest.zip",
            "strip": "wordpress"
        })
    );
}

#[test]
fn an_applied_report_from_an_older_daemon_still_reads() {
    let applied: BlueprintApplied =
        serde_json::from_str(r#"{"blueprint":"laravel","project":"shop","root":"/x","steps":[]}"#)
            .expect("reads");

    assert_eq!(applied.next_steps, None);
    assert_eq!(applied.database, None);
}
