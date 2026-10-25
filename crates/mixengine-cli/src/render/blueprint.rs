//! `mix blueprint`: what was captured or imported, the plan, and what an apply did.

use mixengine_proto::{
    BlueprintApplied, BlueprintList, BlueprintPlan, BlueprintSummary, Disposition, PlanAction,
    PlanStep, SignatureCheck, SiteKind, StepResult,
};

/// `mix blueprint capture` — what was written down, and where to read it.
pub(crate) fn blueprint_captured(summary: &BlueprintSummary) -> String {
    format!(
        "captured {} from this project
  {}
",
        summary.slug, summary.file
    )
}

/// `mix blueprint import` — what was taken in, and whether anything vouched for it.
///
/// **The trust is said on the way in**: it is decided here once and never again, so this line is
/// the only moment a person is told what they now have.
///
/// **And which kind of untrusted it is.** A file that came with nothing and a file whose signature
/// did not verify are both untrusted and are not the same event: only the second is what the
/// gallery key exists to catch, and it used to arrive here as the first one's sentence.
pub(crate) fn blueprint_imported(summary: &BlueprintSummary) -> String {
    let vouched = match (summary.trusted, summary.signature) {
        (true, _) => "signed by the gallery key",

        // True of all three things the verifier folds together — a manifest edited after it was
        // signed, a signature from another key, and a file that is not a signature at all. Saying
        // "the bytes changed" would accuse the second and third of the first.
        (false, Some(SignatureCheck::Rejected)) => {
            "untrusted: a signature came with it, and it is not the gallery's"
        }

        (false, Some(SignatureCheck::Missing)) => {
            "untrusted: nothing came with it to vouch for it, and nothing will"
        }

        // A row written before the reason was recorded, or one whose reason this build cannot read:
        // the sentence this line has always had, which says the true half of what is known.
        (false, _) => "untrusted: nothing vouches for it, and nothing will",
    };

    format!(
        "imported {}: {vouched}
  {}
",
        summary.slug, summary.file
    )
}

/// `mix blueprint list` — every blueprint this home holds.
pub(crate) fn blueprint_list(list: &BlueprintList) -> String {
    if list.blueprints.is_empty() {
        return "no blueprints have been captured; `mix blueprint capture --name <name>` writes one
"
        .to_owned();
    }

    let mut out = format!(
        "{:<24}  {:<9}  {:<12}  {}
",
        "BLUEPRINT", "SOURCE", "TRUST", "DESCRIPTION"
    );

    for blueprint in &list.blueprints {
        out.push_str(&format!(
            "{:<24}  {:<9}  {:<12}  {}
",
            blueprint.slug,
            blueprint.source.as_str(),
            // A word rather than a colour, because `--json` carries the same fact and a listing
            // that only said it in ANSI would say it to nobody in a pipe.
            //
            // Three words rather than two. `mismatched` is the one worth scanning a table for:
            // somebody signed that file, and this is not what they signed.
            match (blueprint.trusted, blueprint.signature) {
                (true, _) => "signed",
                (false, Some(SignatureCheck::Missing)) => "unsigned",
                (false, Some(SignatureCheck::Rejected)) => "mismatched",
                (false, _) => "untrusted",
            },
            match blueprint.description.is_empty() {
                true => "—",
                false => blueprint.description.as_str(),
            }
        ));
    }

    out
}

/// `mix blueprint apply --dry-run` — every action, in the order it would happen.
///
/// **Words rather than a column of glyphs.** Nothing else in this file marks a line with `✓` or
/// `✗`, and a non-ASCII status column is one more thing to be wrong on a Windows console — while
/// the words are the vocabulary [`Disposition`] already has.
///
/// The elevation sentence is gathered to the end and said **once**: what a person needs to know
/// before they start is that they will be asked for a password, not which of six lines asks for it.
pub(crate) fn blueprint_plan(plan: &BlueprintPlan) -> String {
    let mut out = format!(
        "Plan: {} into project {} at {}

",
        plan.blueprint, plan.project, plan.root
    );

    let actions: Vec<&PlanAction> = plan.steps.iter().map(|step| &step.action).collect();

    for (position, step) in plan.steps.iter().enumerate() {
        out.push_str(&format!(
            "  {:<11} {}
",
            disposition_word(&step.disposition),
            with_site(step_said(step), &actions, position)
        ));
    }

    if plan.steps.iter().any(|step| step.elevates) {
        out.push_str(
            "
applying this asks for elevation once, to write the hosts file
",
        );
    }

    out
}

/// `mix blueprint apply` — what the apply did, step by step.
///
/// **Every step, including the ones that needed nothing**: a second apply whose every line says
/// *already true* is what tells a person the first one finished, and a rendering that hid them would
/// hide exactly that.
///
/// What did **not** run is gathered to the end and said in full, because a `[scaffold]` command is
/// the one line somebody has to act on themselves.
pub(crate) fn blueprint_applied(applied: &BlueprintApplied) -> String {
    let mut out = format!(
        "Applied {} as {} at {}\n\n",
        applied.blueprint, applied.project, applied.root
    );

    let actions: Vec<&PlanAction> = applied.steps.iter().map(|step| &step.action).collect();

    for (position, step) in applied.steps.iter().enumerate() {
        out.push_str(&format!(
            "  {:<11} {}\n",
            match &step.result {
                StepResult::Done { .. } => "done",
                StepResult::AlreadyTrue => "already",
                StepResult::NotRun { .. } => "not run",
                // **A step that ran and did not succeed.** Told apart from one nothing attempted,
                // because what a person does next differs: a failure is theirs to read, and a skip
                // is theirs to decide about.
                StepResult::Failed { .. } => "failed",
                _ => "unknown",
            },
            with_site(action_said(&step.action), &actions, position)
        ));

        // What differed from the plan, under the step it belongs to: fourteen spaces, which is the
        // two of the indent plus the eleven of the status column plus its trailing space, so the
        // sentence lines up with the action it is about.
        if let StepResult::Done { note: Some(note) } = &step.result {
            out.push_str(&format!("              {note}\n"));
        }
    }

    for step in &applied.steps {
        match &step.result {
            StepResult::NotRun { why } | StepResult::Failed { why } => {
                out.push_str(&format!("\n{why}\n"));
            }

            _ => {}
        }
    }

    if let Some(steps) = &applied.next_steps {
        out.push_str(&next_steps_said(steps));
    }

    out
}

/// A `site` line with the site it makes.
///
/// Read off the step after it, as the daemon's executor reads it: a site's names are the `domain`
/// steps straight after it, so a plan with two sites reads as two rather than as one long run of
/// names under two identical lines.
fn with_site(said: String, actions: &[&PlanAction], position: usize) -> String {
    match (actions.get(position), actions.get(position + 1)) {
        (Some(PlanAction::CreateSite { .. }), Some(PlanAction::AddDomain { domain, .. })) => {
            said.replacen("site ", &format!("site {domain}: "), 1)
        }
        _ => said,
    }
}

/// The steps a blueprint left, one line each.
pub(super) fn next_steps_said(steps: &mixengine_proto::NextSteps) -> String {
    use mixengine_proto::NextStepKind;

    let mut out = String::from("\nNext, in the project directory:\n");

    for step in &steps.steps {
        let what = match step.kind {
            NextStepKind::Open => format!("open {}", step.path.as_deref().unwrap_or("/")),
            _ => step.run.clone().unwrap_or_default(),
        };

        // **Which site it is for**, when the blueprint has several.
        let site = step.site.as_ref().map(|site| format!("({site})"));

        let mut tail = Vec::new();
        if let Some(site) = &site {
            tail.push(site.as_str());
        }
        if step.kind == NextStepKind::Serve {
            tail.push("(keeps running)");
        }
        if step.optional {
            tail.push("(optional)");
        }
        if let Some(note) = step.note.as_deref() {
            tail.push(note);
        }

        match tail.is_empty() {
            true => out.push_str(&format!("  {what}\n")),
            false => out.push_str(&format!("  {what}   {}\n", tail.join(" "))),
        }
    }

    // The same file the scaffold came from, so the same caution.
    if !steps.trusted {
        out.push_str(
            "These come from a blueprint nobody vouches for; read them before running them.\n",
        );
    }

    out
}

/// Whether any step of an apply ran and failed.
///
/// **What the exit status is read off.** The job succeeded: the apply did everything it was asked
/// and the report is complete, and the command's own exit code is the command's news. A shell still
/// has to hear it, and this is where it does.
pub(crate) fn blueprint_had_a_failed_step(applied: &BlueprintApplied) -> bool {
    applied
        .steps
        .iter()
        .any(|step| matches!(step.result, StepResult::Failed { .. }))
}

/// The one word a disposition is printed as.
fn disposition_word(disposition: &Disposition) -> &'static str {
    match disposition {
        Disposition::Satisfied => "installed",
        Disposition::Create => "create",
        Disposition::Choice { .. } => "asks",
        Disposition::Confirm { .. } => "confirm",
        Disposition::Blocked { .. } => "blocked",
        Disposition::Unsupported { .. } => "unsupported",
        // A disposition a later build added. Printed as something rather than hidden, because a
        // step nobody can see is a step nobody can decide about.
        _ => "unknown",
    }
}

/// What one step says about itself, including the reason where it has one.
fn step_said(step: &PlanStep) -> String {
    let said = action_said(&step.action);

    match &step.disposition {
        Disposition::Choice { installed, .. } => {
            format!("{said}; {} is installed", installed.as_str())
        }
        Disposition::Blocked { reason } | Disposition::Unsupported { reason } => {
            format!("{said}: {reason}")
        }
        _ => said,
    }
}

/// What one action says about itself, whatever became of it.
///
/// Split from [`step_said`] so that a plan and the report of an apply say the same words about the
/// same action — two renderings would be two vocabularies for one list.
fn action_said(action: &PlanAction) -> String {
    match action {
        PlanAction::RegisterProject { name, root, pins } => {
            // The pins are on this line rather than folded into the runtime steps below, because
            // they are what the *project* will ask for from now on — which is a different fact from
            // what is being installed, and the one a person is really applying a blueprint for.
            let asks = match pins.is_empty() {
                true => String::new(),
                false => format!(
                    ", asking for {}",
                    pins.iter()
                        .map(|(kind, wanted)| format!("{} {}", kind.as_str(), wanted.as_str()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };

            format!("project {name} at {root}{asks}")
        }
        PlanAction::InstallRuntime { kind, wanted } => {
            format!("{} {}", kind.as_str(), wanted.as_str())
        }
        PlanAction::InstallPackage { package, wanted } => match wanted {
            Some(wanted) => format!("{package} {}", wanted.as_str()),
            None => package.clone(),
        },
        PlanAction::EnsureService {
            package,
            instance,
            version,
            dedicated,
        } => format!(
            "{package} {}@{instance}{}",
            version
                .as_ref()
                .map(|version| format!("{} ", version.as_str()))
                .unwrap_or_default(),
            match dedicated {
                true => ", this project's own",
                false => ", reusing the shared instance",
            }
        ),
        PlanAction::CreateDatabase { database, user, .. } => {
            format!("database {database}, user {user}")
        }
        PlanAction::CreateSite {
            kind,
            doc_root,
            https,
            ..
        } => format!(
            "site {} at {}{}",
            site_kind_word(kind),
            match doc_root.is_empty() {
                true => "the project root",
                false => doc_root.as_str(),
            },
            match https {
                true => ", https",
                false => "",
            }
        ),
        PlanAction::AddDomain { domain, primary } => match primary {
            true => format!("domain {domain}"),
            false => format!("domain {domain}, an alias"),
        },
        PlanAction::IssueCertificate { domains } => {
            format!("certificate for {}", domains.join(", "))
        }
        // **The line says how far this reaches.** Extension choices belong to an installed runtime,
        // so this changes the PHP every project on this machine runs — which belongs here, at the
        // moment somebody is deciding, rather than in documentation.
        PlanAction::SetPhpExtension { runtime, name } => match runtime {
            Some(runtime) => format!(
                "php extension {name}: changes PHP {} for every project here",
                runtime.as_str()
            ),
            // Nothing installed answers yet: the runtime step installs the PHP this lands on.
            None => format!(
                "php extension {name} on the PHP this installs, for every project that uses it"
            ),
        },
        PlanAction::RunScaffold { command } => format!("run `{command}`"),
        PlanAction::FetchArchive { url, .. } => format!("download and unpack {url}"),
        PlanAction::WriteDotenv { key, path } => {
            format!("write {key} to {path}, with the database password")
        }
        _ => "something this build cannot describe".to_owned(),
    }
}

/// The word a site kind is printed as in a plan.
fn site_kind_word(kind: &SiteKind) -> &'static str {
    match kind {
        SiteKind::PhpFpm { .. } => "php-fpm",
        SiteKind::Static => "static",
        SiteKind::ReverseProxy { .. } => "reverse-proxy",
        SiteKind::NodeApp { .. } => "node-app",
    }
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{PackageVersion, RuntimeKind, StepOutcome, VersionConstraint};

    use super::*;

    /// A plan with one of everything the renderer has a branch for.
    fn a_plan() -> BlueprintPlan {
        BlueprintPlan {
            blueprint: "laravel-php82".to_owned(),
            project: "shop".to_owned(),
            root: "/home/dev/shop".to_owned(),
            steps: vec![
                PlanStep {
                    action: PlanAction::InstallRuntime {
                        kind: mixengine_proto::RuntimeKind::Php,
                        wanted: mixengine_proto::VersionConstraint::parse("8.2.23")
                            .expect("a constraint"),
                    },
                    disposition: Disposition::Satisfied,
                    elevates: false,
                },
                PlanStep {
                    action: PlanAction::SetPhpExtension {
                        runtime: Some(PackageVersion::parse("8.2.23").expect("a version")),
                        name: "xdebug".to_owned(),
                    },
                    disposition: Disposition::Create,
                    elevates: false,
                },
                PlanStep {
                    action: PlanAction::AddDomain {
                        domain: "shop.test".to_owned(),
                        primary: true,
                    },
                    disposition: Disposition::Blocked {
                        reason: "shop.test is already answered by blog.test".to_owned(),
                    },
                    elevates: true,
                },
                PlanStep {
                    action: PlanAction::IssueCertificate {
                        domains: vec!["shop.test".to_owned()],
                    },
                    disposition: Disposition::Create,
                    elevates: true,
                },
            ],
            source: mixengine_proto::BlueprintSource::Captured,
            trusted: true,
            signature: None,
        }
    }

    /// **What is left to do ends the report.** A `serve` step says it keeps running, an optional
    /// one says so, and an `open` step names its path.
    #[test]
    fn an_apply_with_steps_ends_with_what_to_run() {
        let step = |kind, run: Option<&str>, path: Option<&str>, note: Option<&str>, optional| {
            mixengine_proto::NextStep {
                kind,
                run: run.map(str::to_owned),
                path: path.map(str::to_owned),
                note: note.map(str::to_owned),
                optional,
                credentials: false,
                site: None,
            }
        };
        let applied = BlueprintApplied {
            blueprint: "nextjs".to_owned(),
            project: "shop".to_owned(),
            root: "/tmp/shop".to_owned(),
            steps: Vec::new(),
            next_steps: Some(mixengine_proto::NextSteps {
                trusted: true,
                steps: vec![
                    step(
                        mixengine_proto::NextStepKind::Once,
                        Some("npm install"),
                        None,
                        None,
                        false,
                    ),
                    step(
                        mixengine_proto::NextStepKind::Serve,
                        Some("npm run dev"),
                        None,
                        Some("port 3000"),
                        false,
                    ),
                    step(
                        mixengine_proto::NextStepKind::Open,
                        None,
                        Some("/cp"),
                        None,
                        true,
                    ),
                ],
            }),
            database: None,
        };

        let said = blueprint_applied(&applied);
        assert!(said.contains("Next, in the project directory:"), "{said}");
        assert!(said.contains("  npm install\n"), "{said}");
        assert!(
            said.contains("  npm run dev   (keeps running) port 3000\n"),
            "{said}"
        );
        assert!(said.contains("  open /cp   (optional)\n"), "{said}");
        assert!(!said.contains("nobody vouches"), "{said}");
    }

    /// **Untrusted steps say so.**
    #[test]
    fn untrusted_steps_are_read_before_they_are_run() {
        let said = next_steps_said(&mixengine_proto::NextSteps {
            trusted: false,
            steps: vec![mixengine_proto::NextStep {
                kind: mixengine_proto::NextStepKind::Once,
                run: Some("npm install".to_owned()),
                path: None,
                note: None,
                optional: false,
                credentials: false,
                site: None,
            }],
        });
        assert!(said.contains("nobody vouches"), "{said}");
    }

    /// **Every step, and the one that did not run said in full.** A scaffold command nobody ran is
    /// the one line a person has to act on themselves, so it is not folded into a count.
    #[test]
    fn an_apply_prints_every_step_and_spells_out_what_did_not_run() {
        let applied = BlueprintApplied {
            blueprint: "blog-stack".to_owned(),
            project: "shop".to_owned(),
            root: "/tmp/shop".to_owned(),
            steps: vec![
                StepOutcome {
                    action: PlanAction::RegisterProject {
                        name: "shop".to_owned(),
                        root: "/tmp/shop".to_owned(),
                        pins: std::collections::BTreeMap::new(),
                    },
                    result: StepResult::Done { note: None },
                },
                StepOutcome {
                    action: PlanAction::InstallRuntime {
                        kind: RuntimeKind::Php,
                        wanted: VersionConstraint::parse("8.2.23").expect("a constraint"),
                    },
                    result: StepResult::AlreadyTrue,
                },
                StepOutcome {
                    action: PlanAction::RunScaffold {
                        command: "composer install".to_owned(),
                    },
                    result: StepResult::NotRun {
                        why: "`composer install` was not run: nobody agreed to it".to_owned(),
                    },
                },
            ],
            next_steps: None,
            database: None,
        };

        let rendered = super::blueprint_applied(&applied);

        assert!(rendered.contains("done"), "{rendered}");
        assert!(rendered.contains("already"), "{rendered}");
        assert!(rendered.contains("composer install"), "{rendered}");
        assert!(rendered.contains("nobody agreed to it"), "{rendered}");
    }

    /// **A step that ran and failed reads as that, not as one that was skipped.** What a person
    /// does next differs between the two, and the exit status differs with it.
    #[test]
    fn a_failed_step_prints_its_exit_rather_than_a_skip() {
        let applied = BlueprintApplied {
            blueprint: "borrowed".to_owned(),
            project: "shop".to_owned(),
            root: "/tmp/shop".to_owned(),
            steps: vec![StepOutcome {
                action: PlanAction::RunScaffold {
                    command: "composer install".to_owned(),
                },
                result: StepResult::Failed {
                    why: "`composer install` exited with 1".to_owned(),
                },
            }],
            next_steps: None,
            database: None,
        };

        let rendered = super::blueprint_applied(&applied);

        assert!(rendered.contains("failed"), "{rendered}");
        assert!(!rendered.contains("not run"), "{rendered}");
        assert!(rendered.contains("exited with 1"), "{rendered}");
        assert!(super::blueprint_had_a_failed_step(&applied));
    }

    /// What differed from the plan sits under the step it belongs to, indented past the status
    /// column, so the account a project ended up with is found where the step is.
    #[test]
    fn a_done_step_with_a_note_prints_it_under_the_step() {
        let applied = BlueprintApplied {
            blueprint: "laravel".to_owned(),
            project: "shop".to_owned(),
            root: "/tmp/shop".to_owned(),
            steps: vec![StepOutcome {
                action: PlanAction::CreateDatabase {
                    package: "mariadb".to_owned(),
                    instance: "main".to_owned(),
                    database: "shop".to_owned(),
                    user: "shop".to_owned(),
                },
                result: StepResult::Done {
                    note: Some(
                        "the account shop is somebody else's, so this project's is shop-2"
                            .to_owned(),
                    ),
                },
            }],
            next_steps: None,
            database: None,
        };

        let rendered = super::blueprint_applied(&applied);

        let lines: Vec<&str> = rendered.lines().collect();
        let step = lines
            .iter()
            .position(|line| line.starts_with("  done"))
            .expect("the step line");
        assert_eq!(
            lines[step + 1],
            "              the account shop is somebody else's, so this project's is shop-2"
        );
    }

    /// An import says which of the two things a person now has, because it is the only moment they
    /// are told: the trust is decided there and never again.
    #[test]
    fn an_import_says_whether_anything_vouched_for_it() {
        let untrusted = super::blueprint_imported(&BlueprintSummary {
            slug: "borrowed".to_owned(),
            name: "borrowed".to_owned(),
            description: String::new(),
            created_at: "2026-09-01T00:00:00Z".to_owned(),
            source: mixengine_proto::BlueprintSource::Imported,
            trusted: false,
            signature: Some(mixengine_proto::SignatureCheck::Missing),
            file: "/home/dev/.mixengine/blueprints/borrowed.toml".to_owned(),
        });

        assert!(untrusted.contains("untrusted"), "{untrusted}");
        assert!(untrusted.contains("nothing will"), "{untrusted}");
    }

    /// **Which kind of untrusted.** A file nobody signed and a file whose signature did not verify
    /// are both untrusted and are not the same event, and the second is the one worth reading
    /// twice.
    #[test]
    fn an_import_says_which_kind_of_untrusted_it_is() {
        let summary = |signature| BlueprintSummary {
            slug: "borrowed".to_owned(),
            name: "borrowed".to_owned(),
            description: String::new(),
            created_at: "2026-09-01T00:00:00Z".to_owned(),
            source: mixengine_proto::BlueprintSource::Imported,
            trusted: false,
            signature,
            file: "/home/dev/.mixengine/blueprints/borrowed.toml".to_owned(),
        };

        let missing = super::blueprint_imported(&summary(Some(SignatureCheck::Missing)));
        assert!(missing.contains("nothing came with it"), "{missing}");

        // True of all three things the verifier folds together — edited after signing, signed by
        // another key, and not a signature at all. "The bytes changed" would accuse the last two of
        // the first.
        let rejected = super::blueprint_imported(&summary(Some(SignatureCheck::Rejected)));
        assert!(rejected.contains("not the gallery's"), "{rejected}");
        assert!(!rejected.contains("nothing came with it"), "{rejected}");

        // A row older than this task kept no reason, and keeps the sentence it has always had.
        let older = super::blueprint_imported(&summary(None));
        assert!(older.contains("nothing vouches for it"), "{older}");
    }

    /// The listing says it in one word, because a table is where somebody scans six blueprints at
    /// once — and `--json` carries the same three facts for anything that is not a person.
    #[test]
    fn the_listing_says_which_kind_of_untrusted_each_one_is() {
        let row = |slug: &str, trusted, signature| BlueprintSummary {
            slug: slug.to_owned(),
            name: slug.to_owned(),
            description: String::new(),
            created_at: "2026-09-01T00:00:00Z".to_owned(),
            source: mixengine_proto::BlueprintSource::Imported,
            trusted,
            signature,
            file: format!("/home/dev/.mixengine/blueprints/{slug}.toml"),
        };

        let listed = super::blueprint_list(&BlueprintList {
            blueprints: vec![
                row("good", true, Some(SignatureCheck::Verified)),
                row("bare", false, Some(SignatureCheck::Missing)),
                row("stale", false, Some(SignatureCheck::Rejected)),
                row("older", false, None),
            ],
        });

        let line = |slug: &str| {
            listed
                .lines()
                .find(|line| line.starts_with(slug))
                .unwrap_or_else(|| panic!("no row for {slug}: {listed}"))
                .to_owned()
        };

        assert!(line("good").contains("signed"), "{listed}");
        assert!(line("bare").contains("unsigned"), "{listed}");
        assert!(line("stale").contains("mismatched"), "{listed}");
        assert!(line("older").contains("untrusted"), "{listed}");
    }

    /// Words, not glyphs — and the reason a step is blocked travels with the step, because a person
    /// reading "blocked" without it has to go looking.
    #[test]
    fn a_plan_prints_one_line_per_step_and_says_who_holds_a_taken_domain() {
        let rendered = super::blueprint_plan(&a_plan());

        assert!(rendered.contains("installed"), "{rendered}");
        assert!(rendered.contains("blocked"), "{rendered}");
        assert!(
            rendered.contains("already answered by blog.test"),
            "{rendered}"
        );
        assert!(
            !rendered.contains('\u{2713}') && !rendered.contains('\u{2717}'),
            "a status glyph crept into a file that has never had one:\n{rendered}"
        );
    }

    /// A plan with two sites says which site each `site` line makes.
    #[test]
    fn a_plan_with_two_sites_names_each_one() {
        let site = || PlanStep {
            action: PlanAction::CreateSite {
                kind: mixengine_proto::SiteKind::Static,
                doc_root: String::new(),
                https: true,
                routes: Vec::new(),
                services: None,
            },
            disposition: Disposition::Create,
            elevates: false,
        };
        let name = |domain: &str| PlanStep {
            action: PlanAction::AddDomain {
                domain: domain.to_owned(),
                primary: true,
            },
            disposition: Disposition::Create,
            elevates: true,
        };
        let mut plan = a_plan();
        plan.steps = vec![site(), name("shop.test"), site(), name("docs.shop.test")];

        let rendered = super::blueprint_plan(&plan);
        assert!(rendered.contains("site shop.test"), "{rendered}");
        assert!(rendered.contains("site docs.shop.test"), "{rendered}");
    }

    /// A step that belongs to one site says which.
    #[test]
    fn a_step_with_a_site_says_it() {
        let steps = mixengine_proto::NextSteps {
            trusted: true,
            steps: vec![mixengine_proto::NextStep {
                kind: mixengine_proto::NextStepKind::Serve,
                run: Some("npm run dev".to_owned()),
                path: None,
                note: None,
                optional: false,
                credentials: false,
                site: Some("vite.shop.test".to_owned()),
            }],
        };

        assert!(super::next_steps_said(&steps).contains("npm run dev   (vite.shop.test)"));
    }

    /// Said once, at the end, so a person knows before they start rather than four lines in.
    #[test]
    fn a_plan_that_would_ask_for_a_password_says_so_once() {
        let rendered = super::blueprint_plan(&a_plan());

        assert_eq!(rendered.matches("elevation").count(), 1, "{rendered}");
    }

    /// Enabling an extension changes the PHP every project on this machine runs, and that belongs
    /// on the line somebody is deciding from.
    #[test]
    fn enabling_an_extension_says_that_it_reaches_past_this_project() {
        let rendered = super::blueprint_plan(&a_plan());

        assert!(rendered.contains("every project here"), "{rendered}");
    }

    /// An empty home says what to type next, the way every other empty listing here does.
    #[test]
    fn a_home_with_no_blueprints_says_how_to_make_one() {
        let rendered = super::blueprint_list(&BlueprintList {
            blueprints: Vec::new(),
        });

        assert!(rendered.contains("mix blueprint capture"), "{rendered}");
    }
}
