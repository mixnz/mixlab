//! `mix blueprint`, and every question an apply asks on the way.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    AnswerSubject, BlueprintApplied, BlueprintApply, BlueprintApplyResponse, BlueprintCapture,
    BlueprintImport, BlueprintList, BlueprintPlan, BlueprintSummary, Disposition, ElevationStatus,
    Error, ErrorCode, JobId, JobOutcome, JobSummary, MismatchAnswer, PlanAction, ProjectRef,
    ScaffoldConsent, ServiceTarget, ServiceWalk, SignatureCheck, VersionAnswer, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::commands::logs::logs;
use crate::commands::project::{WhichProject, here, which};
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::parse::service_id;
use crate::prerequisites::agreed_to_prerequisites;
use crate::{confirm, render};

/// `mix blueprint …` — one subcommand per `blueprint.*` method this build has.
///
/// `export` and `delete` are deliberately absent: a blueprint's rendering is already on disk at
/// `blueprints/<slug>.toml`, so exporting one is copying a file rather than a daemon method, and
/// nothing has asked to delete one.
#[derive(Debug, Subcommand)]
pub(crate) enum BlueprintCommand {
    /// Write down what a project is made of.
    Capture {
        /// What to file it under: lower-case letters, digits and hyphens.
        // Positional rather than `--name`, because the flattened project argument is already called
        // `name` and clap refuses two arguments under one id. That was found by running the command
        // rather than by a test.
        #[arg(value_name = "NAME")]
        name: String,

        /// Which project. Defaults to whichever project the current directory is in.
        #[arg(long, value_name = "PROJECT")]
        project: Option<String>,

        /// What it is for.
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,

        /// Replace the blueprint already filed under this name.
        #[arg(long)]
        overwrite: bool,
    },

    /// Take in a blueprint somebody else wrote.
    ///
    /// **What arrives without a signature the gallery key vouches for is untrusted for good.**
    /// Nothing raises that afterwards, and it decides how loudly its `[scaffold]` command has to be
    /// agreed to before it runs.
    Import {
        /// The manifest to read.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// What to file it under. Defaults to the file's own name, without `.toml`.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,

        /// The detached signature to check it against. Defaults to `<FILE>.minisig` if that exists.
        #[arg(long, value_name = "FILE")]
        signature: Option<PathBuf>,

        /// Replace the blueprint already filed under that name.
        #[arg(long)]
        overwrite: bool,
    },

    /// Every blueprint this home holds.
    List,

    /// What applying one would do.
    Apply {
        /// Which blueprint.
        #[arg(value_name = "BLUEPRINT")]
        blueprint: String,

        /// What the new project is called, and what `{project}` becomes.
        #[arg(long, value_name = "NAME")]
        project: String,

        /// Where it goes. Defaults to a directory named for the project, in the current one.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,

        /// Stop after planning, and print the plan.
        ///
        /// Sent as it is typed rather than insisted on here: whether this build can carry an apply
        /// out is the daemon's to say, and a client that refused to ask would be holding a rule of
        /// its own.
        #[arg(long)]
        dry_run: bool,

        /// Answer every version question by installing what the blueprint asks for.
        #[arg(long, conflicts_with = "use_installed")]
        install_missing: bool,

        /// Install a web server too, where this home has none.
        ///
        /// A home with no front end serves no site, and nothing installs one by itself. With this,
        /// a blueprint that declares a site plans the default web server as well. A home that
        /// already has one, Caddy or nginx, is left alone.
        #[arg(long)]
        with_front_end: bool,

        /// Start the services this apply creates whenever MixEngine starts.
        ///
        /// Only what it creates: a server this home already had is left as its owner set it. Read
        /// and changed afterwards with `mix service autostart`.
        #[arg(long)]
        autostart: bool,

        /// Start the services this project needs once the apply is done.
        ///
        /// These are the project's sites, the database and pool they use, and the front end they
        /// are reached through, not every service of this home. They start after the permission
        /// prompt, so every site's name already resolves when it comes up.
        //
        // Roadmap task T125: which services those are is the daemon's answer and not this
        // command's. Until T125 the only set a client could ask for was every service this home
        // declares.
        #[arg(long)]
        start: bool,

        /// Answer every version question by using what this machine already has.
        #[arg(long)]
        use_installed: bool,

        /// Run the blueprint's own `[scaffold]` command without asking first.
        ///
        /// For a blueprint the gallery signed. An unsigned one takes the other flag, and neither
        /// covers the other: a script that runs somebody's unsigned command should say so on the
        /// line that does it.
        #[arg(long, conflicts_with = "run_untrusted_scaffold")]
        run_scaffold: bool,

        /// Run an **untrusted** blueprint's own `[scaffold]` command without asking first.
        ///
        /// Nothing vouches for what this runs. The command is still printed before it starts.
        #[arg(long)]
        run_untrusted_scaffold: bool,

        /// Write the database's URL, password included, to the project's `.env` without asking
        /// first, for every key the blueprint offers.
        #[arg(long)]
        write_dotenv: bool,

        /// Spend the one elevation prompt at the end without asking first.
        #[arg(long)]
        grant: bool,

        /// Install what the blueprint's releases need of this machine first, without asking: the
        /// Microsoft Visual C++ Redistributable, on Windows. Windows still asks for approval.
        ///
        /// This is a flag of its own rather than part of `--yes`, because an apply asks several
        /// questions and one flag answering all of them would answer ones nobody read.
        #[arg(long)]
        install_prerequisites: bool,

        /// Apply even though MixEngine judges this machine lacks something the releases need.
        #[arg(long)]
        ignore_requirements: bool,
    },
}

/// `mix blueprint …` — capture one, list them, see what applying one would do.
pub(crate) async fn blueprint(
    command: BlueprintCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        BlueprintCommand::Capture {
            name,
            project,
            description,
            overwrite,
        } => {
            let capture = BlueprintCapture {
                project: which(WhichProject { name: project })?,
                name,
                description,
                overwrite,
            };
            let summary: BlueprintSummary = ask(
                &mut client,
                rpc::method::BLUEPRINT_CAPTURE,
                encode(&capture),
            )
            .await?;
            emit(&rendered(json, &summary, || {
                render::blueprint::blueprint_captured(&summary)
            }))?;
        }

        BlueprintCommand::Import {
            file,
            name,
            signature,
            overwrite,
        } => {
            // Resolved here, because the daemon has no idea what directory this process is in and a
            // relative path sent as it was typed would be read against the wrong one.
            let import = BlueprintImport {
                path: here(Some(file))?.display().to_string(),
                signature: match signature {
                    Some(path) => Some(here(Some(path))?.display().to_string()),
                    None => None,
                },
                name,
                overwrite,
            };

            let summary: BlueprintSummary =
                ask(&mut client, rpc::method::BLUEPRINT_IMPORT, encode(&import)).await?;

            emit(&rendered(json, &summary, || {
                render::blueprint::blueprint_imported(&summary)
            }))?;
        }

        BlueprintCommand::List => {
            let list: BlueprintList = ask(&mut client, rpc::method::BLUEPRINT_LIST, None).await?;
            emit(&rendered(json, &list, || {
                render::blueprint::blueprint_list(&list)
            }))?;
        }

        BlueprintCommand::Apply {
            blueprint,
            project,
            path,
            dry_run,
            install_missing,
            use_installed,
            with_front_end,
            // Bound under another name: `autostart` in this function is already the daemon
            // autostarter every command carries — `mix autostart` is about the daemon and this flag
            // is about the services an apply creates.
            autostart: services_autostart,
            start,
            run_scaffold,
            run_untrusted_scaffold,
            write_dotenv,
            grant,
            install_prerequisites,
            ignore_requirements,
        } => {
            // **Where it goes, and who names it** — roadmap task **T120a**. A path somebody typed
            // is sent as typed; with none, the daemon is told to make one *under* this directory,
            // and it names that directory with the project's handle.
            //
            // The naming is deliberately not done here. This binary may not depend on
            // `mixengine-core` (`mixengine-proto/tests/workspace_layering.rs`), so it cannot call
            // `domains::slug` — and restating the charset would be a second copy of the rule T120
            // spent a task consolidating, in the client the ban on business logic is about. A
            // client knows where it is standing; the daemon knows what things are called.
            let (root, root_is_parent) = match path {
                Some(path) => (here(Some(path))?, false),
                None => (here(None)?, true),
            };

            let mut apply = BlueprintApply {
                blueprint,
                project,
                root: root.display().to_string(),
                root_is_parent,
                dry_run: true,
                answers: Vec::new(),
                // Filled in below, once the plan says whether there is a command to agree to and
                // who wrote it — roadmap task **T78a**.
                scaffold: None,
                // Filled in below, once the plan says which `.env` keys it offers — T205a.
                dotenv: Vec::new(),
                // Carried on the dry run as well as on the real one, which is what keeps the
                // feature's own acceptance criterion true: `--dry-run` prints the actions the real
                // run performs, so a flag that changed the plan may not be added afterwards.
                front_end: with_front_end,
                autostart: services_autostart,
                // Answered below, once the plan says what its releases lack — T152.
                install_prerequisites: false,
                ignore_requirements,
            };

            // **The plan comes first either way** (the T78 design, D6). A dry run stops here; a real
            // apply needs it because the questions are in it, and a daemon has no keyboard to ask
            // them with.
            let planned: BlueprintApplyResponse =
                ask(&mut client, rpc::method::BLUEPRINT_APPLY, encode(&apply)).await?;

            let BlueprintApplyResponse::Planned { plan, needs } = &planned else {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "the daemon answered a dry run with something other than a plan",
                ));
            };

            emit(&rendered(json, plan, || {
                render::blueprint::blueprint_plan(plan)
            }))?;

            let needs = needs.as_deref().unwrap_or_default();
            if !json && !needs.is_empty() {
                emit(&render::prerequisites::requirements(needs))?;
            }

            if dry_run {
                return Ok(ExitCode::SUCCESS);
            }

            let Some(answers) = answered(plan, install_missing, use_installed, json)? else {
                // Cancelling is an answer and not a failure: nothing was asked of the machine, and
                // the same command works when the person has decided.
                return Ok(ExitCode::SUCCESS);
            };

            // **The command is shown and agreed to, every apply** — roadmap task **T78a**. The
            // consent carries the command it was given about, so a blueprint that changed between
            // this plan and the apply below cannot be run under it.
            let consent = agreed_to_scaffold(plan, run_scaffold, run_untrusted_scaffold, json)?;

            // **Its own question** — roadmap task **T205a**. The line holds a password, so it is
            // asked apart from the command and never answered by `--run-scaffold`.
            let dotenv = agreed_to_dotenv(plan, write_dotenv, json);

            // **Asked once for the whole plan** (T152, D7), after the version and scaffold questions
            // so every question the apply raises is answered before any of it starts.
            let Some(agreed) = agreed_to_prerequisites(needs, install_prerequisites, json)? else {
                return Ok(ExitCode::SUCCESS);
            };
            apply.install_prerequisites = agreed;

            apply.dry_run = false;
            apply.answers = answers;
            apply.scaffold = consent;
            apply.dotenv = dotenv;

            let started: BlueprintApplyResponse =
                ask(&mut client, rpc::method::BLUEPRINT_APPLY, encode(&apply)).await?;

            let BlueprintApplyResponse::Started { job } = started else {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "the daemon answered an apply with something other than a job",
                ));
            };

            // **A second connection, because two streams cannot share one** — roadmap task
            // **T78a**. The job's own progress comes back on this client's `job.wait`, and what the
            // blueprint's command prints comes down `GET /logs/job/<id>` for as long as it runs. A
            // log that cannot be opened is not worth failing an apply over: the work goes on and the
            // outcome still says what happened.
            let watching = watch_job_log(endpoint, autostart, job.id, json).await;

            let finished = follow(&mut client, job, json).await?;

            if let Some(watching) = watching {
                watching.abort();
            }
            emit(&rendered(json, &finished, || {
                render::job::job_status(&finished)
            }))?;

            let mut a_step_failed = false;

            if let Some(JobOutcome::Succeeded { result }) = &finished.outcome
                && let Ok(applied) = serde_json::from_value::<BlueprintApplied>(result.clone())
            {
                emit(&rendered(json, &applied, || {
                    render::blueprint::blueprint_applied(&applied)
                }))?;

                a_step_failed = render::blueprint::blueprint_had_a_failed_step(&applied);
            }

            if !render::job::job_succeeded(&finished) {
                return Ok(ExitCode::FAILURE);
            }

            // **The job succeeded and a step did not** — roadmap task **T78a**, its design's D7. The
            // apply applied everything it was asked to; what failed is the blueprint's own command,
            // and a shell that chained on this has to hear it.
            if a_step_failed {
                return Ok(ExitCode::FAILURE);
            }

            // **The client is what spends the prompt** (D10): the apply queued the hosts entries and
            // the daemon never raises a dialog on its own initiative, so the last thing this command
            // does is offer the one prompt that makes the new site reachable.
            let spent = granted(&mut client, grant, json).await?;

            if !start || spent != ExitCode::SUCCESS {
                return Ok(spent);
            }

            // **After the elevation and not inside the job** — roadmap task **T117**. An apply never
            // raises a prompt; it queues what needs one and the client spends it above. A front end
            // started before that would serve the new site at a name this machine does not resolve
            // and with a certificate no store trusts — a browser error at the end of a progress bar.
            //
            // **This project's services, not this home's** — roadmap task **T125**. Until then the
            // target was empty, meaning *every service this home declares*, and a home with four
            // PHP versions and three databases started all of them to put one site up. What the
            // apply needs is a set no client may derive — so the daemon derives it, from the sites
            // this project now has, and `--start` asks for it by name.
            let walk: ServiceWalk = ask(
                &mut client,
                rpc::method::SERVICE_START,
                encode(&ServiceTarget {
                    service: None,
                    project: Some(ProjectRef::Name(apply.project.clone())),
                    wait: true,
                }),
            )
            .await?;

            emit(&rendered(json, &walk, || {
                render::service::service_walk(render::service::Walked::Start, &walk)
            }))?;

            return Ok(match walk.failed {
                None => ExitCode::SUCCESS,
                Some(_) => ExitCode::FAILURE,
            });
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// There was nobody to put the question to.
/// The answers to a plan's version questions, or [`None`] when somebody cancelled.
///
/// **One question per mismatch, and the flags answer them all** (the T78 design, D6). A person
/// answers them one at a time; a script passes `--install-missing` or `--use-installed` and is never
/// asked. Standard input at end of file with a question outstanding is a refusal naming the two
/// flags, on `confirm.rs`' standing rule: what must not happen is a prompt nobody is there to see.
fn answered(
    plan: &BlueprintPlan,
    install_missing: bool,
    use_installed: bool,
    json: bool,
) -> Result<Option<Vec<VersionAnswer>>, Error> {
    let mut answers = Vec::new();

    for step in &plan.steps {
        let Disposition::Choice { installed, wanted } = &step.disposition else {
            continue;
        };

        let Some(subject) = subject_of(&step.action) else {
            continue;
        };

        let answer = match (install_missing, use_installed) {
            (true, _) => MismatchAnswer::Install,
            (_, true) => MismatchAnswer::UseInstalled,

            (false, false) => {
                // A `--json` run has nobody at a keyboard by construction, and a question it cannot
                // ask is one it must not answer on somebody's behalf.
                if json {
                    return Err(unanswerable_question());
                }

                match confirm::choose(&format!(
                    "{subject} {} is not installed. [i]nstall it, [u]se the installed {}, or \
                     [c]ancel? ",
                    wanted.as_str(),
                    installed.as_str()
                )) {
                    confirm::Choice::Install => MismatchAnswer::Install,
                    confirm::Choice::UseInstalled => MismatchAnswer::UseInstalled,

                    confirm::Choice::Cancel => {
                        let _ = writeln!(
                            std::io::stderr(),
                            "nothing was applied; run the same command again when you have decided"
                        );

                        return Ok(None);
                    }

                    confirm::Choice::Unanswerable => return Err(unanswerable_question()),
                }
            }
        };

        answers.push(VersionAnswer { subject, answer });
    }

    Ok(Some(answers))
}

/// What a version question is about, where the action is one that can raise one.
fn subject_of(action: &PlanAction) -> Option<AnswerSubject> {
    match action {
        PlanAction::InstallRuntime { kind, .. } => Some(AnswerSubject::Runtime { kind: *kind }),

        PlanAction::EnsureService {
            package, instance, ..
        } => service_id(&format!("{package}@{instance}"))
            .or_else(|_| service_id(package))
            .ok()
            .map(|id| AnswerSubject::Service { id }),

        _ => None,
    }
}

/// Spend the one elevation prompt an apply queued, having asked first unless told not to.
///
/// **A client is the only thing allowed to raise one** — `docs/architecture/daemon-and-ipc.md`'s
/// rule, which the daemon's own elevation queue is built around. An apply enqueues; this is where
/// somebody says yes.
async fn granted(client: &mut Client, grant: bool, json: bool) -> Result<ExitCode, Error> {
    let waiting: ElevationStatus = ask(client, rpc::method::ELEVATION_STATUS, None).await?;

    // Nothing waiting is the ordinary end of an apply on a machine that already had the names in
    // its hosts file — or one that cannot prompt at all, which left the queue where it was.
    if waiting.pending.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }

    // **An apply that worked is not a failure because nobody was there to say yes.** The names are
    // in the daemon's queue and one command spends them, so what a `--json` run and a closed
    // standard input get is that sentence rather than an error about a question nobody heard.
    if !grant && (json || !asked_to_grant(&waiting)) {
        let _ = writeln!(
            std::io::stderr(),
            "{} still waiting; `mix elevation grant` writes them",
            waiting.pending.len()
        );

        return Ok(ExitCode::SUCCESS);
    }

    let started: JobSummary = ask(client, rpc::method::ELEVATION_GRANT, None).await?;
    let finished = follow(client, started, json).await?;
    emit(&rendered(json, &finished, || {
        render::job::job_status(&finished)
    }))?;

    Ok(match render::job::job_succeeded(&finished) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    })
}

/// Whether somebody at the keyboard said yes to spending the prompt.
///
/// Anything but a typed yes — including a closed standard input — is *not now*, which is a state the
/// same command gets out of and never an error: the queue is untouched either way.
fn asked_to_grant(waiting: &ElevationStatus) -> bool {
    matches!(
        confirm::ask(&format!(
            "{}\ngrant now? [y/N] ",
            render::elevation::elevation_prompt(waiting)
        )),
        confirm::Answer::Yes
    )
}

/// The refusal for a version question nothing could answer.
fn unanswerable_question() -> Error {
    Error::new(
        ErrorCode::InvalidArgument,
        "this blueprint asks for a version this machine does not have, and nothing answered the \
         question",
    )
    .with_hint(
        "`--install-missing` installs what it asks for; `--use-installed` takes what is here",
    )
}

/// Agreement to run the blueprint's own command, or [`None`] where there is nothing to agree to.
///
/// Roadmap task **T78a**, its design's D4 and D15. **The command is printed exactly as it will run**
/// — a confirmation that paraphrased would be a confirmation to something else — and the flag that
/// answers it depends on who wrote the blueprint: `--run-scaffold` for one the gallery signed, and
/// `--run-untrusted-scaffold` for one nobody vouches for. Neither covers the other, so a script that
/// runs somebody's unsigned command says so on the line that does it.
///
/// Declining is not a failure. The apply goes ahead without the command and the step comes back
/// saying it was left, which is a project a person can use and one line they can run themselves.
///
/// # Errors
///
/// `invalid_argument` when there is a question and nothing to answer it with: a `--json` run, or a
/// standard input that is closed — [`answered`]'s `Unanswerable` rule, unchanged.
fn agreed_to_scaffold(
    plan: &BlueprintPlan,
    run_scaffold: bool,
    run_untrusted_scaffold: bool,
    json: bool,
) -> Result<Option<ScaffoldConsent>, Error> {
    // **A command or an archive** — roadmap task **T205**, D2. What is shown and agreed to is the
    // command, or the archive's URL; the consent names whichever it was. A starter planned
    // `Satisfied` over a full folder will not run, so there is nothing to agree to.
    let Some((command, archive)) = plan
        .steps
        .iter()
        .filter(|step| step.disposition != mixengine_proto::Disposition::Satisfied)
        .find_map(|step| match &step.action {
            PlanAction::RunScaffold { command } => Some((command.clone(), false)),
            PlanAction::FetchArchive { url, .. } => Some((url.clone(), true)),
            _ => None,
        })
    else {
        return Ok(None);
    };

    let untrusted = !plan.trusted;
    let consent = |untrusted: bool| match archive {
        true => ScaffoldConsent {
            command: String::new(),
            archive: Some(command.clone()),
            untrusted,
        },
        false => ScaffoldConsent {
            command: command.clone(),
            archive: None,
            untrusted,
        },
    };

    let given = match untrusted {
        true => run_untrusted_scaffold,
        false => run_scaffold,
    };

    if given {
        return Ok(Some(consent(untrusted)));
    }

    // The flag for the other kind of blueprint is not an answer about this one, and saying so is
    // more use than running the command or silently skipping it.
    let wrong_flag = match untrusted {
        true => run_scaffold,
        false => run_untrusted_scaffold,
    };

    if wrong_flag {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            match untrusted {
                true => format!(
                    "nothing vouches for {}, so `--run-scaffold` does not answer for it",
                    plan.blueprint
                ),
                false => format!(
                    "{} is signed, so `--run-untrusted-scaffold` does not answer for it",
                    plan.blueprint
                ),
            },
        )
        .with_hint(match untrusted {
            true => "`--run-untrusted-scaffold` runs it anyway",
            false => "`--run-scaffold` runs it",
        }));
    }

    // **Nobody to ask is not a refusal here, and that is a departure from `answered`.** A version
    // question has no safe default — the two answers leave different machines — so a `--json` run
    // with one outstanding is refused. This question does have one: not running somebody else's
    // command leaves a project that works and a line saying what was left, and there is no flag for
    // *no*, so refusing would make "apply this without its command" impossible from a script.
    if json {
        return Ok(unasked(&command, untrusted));
    }

    let vouched = vouching(untrusted, plan.signature);

    let question = match archive {
        true => format!(
            "{vouched} It wants to download and unpack, into the new project's directory:\n\n    \
             {command}\n\nDo it? [y/N] "
        ),
        false => format!(
            "{vouched} It wants to run, in the new project's directory:\n\n    {command}\n\nRun \
             it? [y/N] "
        ),
    };

    match confirm::ask(&question) {
        confirm::Answer::Yes => Ok(Some(consent(untrusted))),

        // Declining leaves the step as a sentence and applies everything else, which is what the
        // daemon does with an apply that carries no consent at all.
        confirm::Answer::No => Ok(None),

        confirm::Answer::Unanswerable => Ok(unasked(&command, untrusted)),
    }
}

/// The `.env` keys the person agrees to have written — roadmap task **T205a**.
///
/// Asked once per key, after the scaffold's question. **Nobody to ask writes nothing**, the
/// scaffold's rule for a question with a safe default: the project works without the line, and the
/// step's outcome says how to set it.
fn agreed_to_dotenv(plan: &BlueprintPlan, write_dotenv: bool, json: bool) -> Vec<String> {
    let unasked = |key: &str| {
        let _ = writeln!(
            std::io::stderr(),
            "mix: {key} was not written to .env; nothing here could be asked. `--write-dotenv` \
             agrees to it."
        );
        false
    };

    plan.steps
        .iter()
        .filter_map(|step| match (&step.action, &step.disposition) {
            (PlanAction::WriteDotenv { key, .. }, Disposition::Confirm { .. }) => Some(key.clone()),
            _ => None,
        })
        .filter(|key| {
            if write_dotenv {
                return true;
            }
            if json {
                return unasked(key);
            }
            match confirm::ask(&format!(
                "\nWrite {key}, with the database password, to .env? [y/N] "
            )) {
                confirm::Answer::Yes => true,
                confirm::Answer::No => false,
                confirm::Answer::Unanswerable => unasked(key),
            }
        })
        .collect()
}

/// What a person is told about the blueprint whose command they are being asked to run.
///
/// **Which kind of untrusted, and not merely that it is** — roadmap task **T79b**. This is the
/// moment the reason is worth most: somebody is about to run a stranger's command, and "a signature
/// came with this and it is not the gallery's" is a different thing to weigh than "nobody signed
/// it". It changes what is *said* and nothing about what is allowed — one flag still answers for
/// both kinds (the T79b design's D8).
///
/// A function of its own because the question it belongs to is only asked on a real apply, with a
/// keyboard in front of it: a sentence reachable no other way is a sentence nothing can check.
fn vouching(untrusted: bool, signature: Option<SignatureCheck>) -> &'static str {
    match (untrusted, signature) {
        (false, _) => "This blueprint is signed.",

        // True of all three things the verifier folds together, which is why it does not say the
        // bytes changed: a colleague's own key and a corrupt `.minisig` land here too.
        (true, Some(SignatureCheck::Rejected)) => {
            "A signature came with this blueprint and it is not the gallery's."
        }

        // Nothing came with it, or the row is older than the reason.
        (true, _) => "Nothing vouches for this blueprint.",
    }
}

/// Say on stderr that a command was left unrun because there was nobody to ask, and leave it.
///
/// Roadmap task **T78a**. The apply goes ahead: what it makes is a project somebody can use, and
/// the step's own outcome says the command is still theirs to run.
fn unasked(command: &str, untrusted: bool) -> Option<ScaffoldConsent> {
    let flag = match untrusted {
        true => "--run-untrusted-scaffold",
        false => "--run-scaffold",
    };

    let _ = writeln!(
        std::io::stderr(),
        "mix: `{command}` was not run; nothing here could be asked. `{flag}` agrees to it."
    );

    None
}

/// Print what a job's own command says, for as long as it says anything.
///
/// Roadmap task **T78a**. A task rather than part of the wait, because the two are different
/// streams: progress comes back on `job.wait` and output comes down `GET /logs/job/<id>`, and one
/// connection cannot carry both. [`None`] when the daemon cannot be reached a second time, which is
/// a log this command does without rather than an apply it refuses to run.
async fn watch_job_log(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    job: JobId,
    json: bool,
) -> Option<tokio::task::JoinHandle<()>> {
    let mut client = Client::connect(endpoint, autostart).await.ok()?;
    let path = format!("logs/job/{}", job.0);

    Some(tokio::spawn(async move {
        let _ = logs(&mut client, &path, 0, true, json).await;
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixengine_proto::BlueprintPlan;
    use mixengine_proto::PlanAction;
    use mixengine_proto::SignatureCheck;

    /// **`--run-scaffold` agrees to an archive by its URL** — roadmap task **T205**, D2. The
    /// consent names what the plan showed, and an archive shows a URL, not a command.
    #[test]
    fn the_scaffold_flag_agrees_to_an_archive_by_its_url() {
        let url = "https://wordpress.org/latest.zip";
        let plan = BlueprintPlan {
            blueprint: "wordpress".to_owned(),
            project: "wp".to_owned(),
            root: "/tmp/wp".to_owned(),
            steps: vec![mixengine_proto::PlanStep {
                action: PlanAction::FetchArchive {
                    url: url.to_owned(),
                    strip: Some("wordpress".to_owned()),
                },
                disposition: mixengine_proto::Disposition::Confirm {
                    what: url.to_owned(),
                },
                elevates: false,
            }],
            source: mixengine_proto::BlueprintSource::Builtin,
            trusted: true,
            signature: None,
        };

        let consent = agreed_to_scaffold(&plan, true, false, true)
            .expect("an answer")
            .expect("a consent");

        assert_eq!(consent.archive.as_deref(), Some(url));
        assert_eq!(consent.command, "");
        assert!(!consent.untrusted);
    }

    /// **A starter skipped over a full folder asks nothing** — roadmap task **T205**. `php-mysql`
    /// and `static` plan their archive `Satisfied` over a cloned repository, so there is nothing to
    /// agree to: no `[y/N]` is printed for a step that will not run, and `--run-scaffold` spends
    /// no consent on it.
    #[test]
    fn a_satisfied_archive_asks_for_no_consent() {
        let url = "https://x.org/static-starter.zip";
        let plan = BlueprintPlan {
            blueprint: "static".to_owned(),
            project: "site".to_owned(),
            root: "/tmp/site".to_owned(),
            steps: vec![mixengine_proto::PlanStep {
                action: PlanAction::FetchArchive {
                    url: url.to_owned(),
                    strip: Some("static".to_owned()),
                },
                disposition: mixengine_proto::Disposition::Satisfied,
                elevates: false,
            }],
            source: mixengine_proto::BlueprintSource::Builtin,
            trusted: true,
            signature: None,
        };

        assert!(
            agreed_to_scaffold(&plan, true, false, false)
                .expect("no question")
                .is_none()
        );
    }

    fn plan_writing(key: &str) -> BlueprintPlan {
        BlueprintPlan {
            blueprint: "rails".to_owned(),
            project: "blog".to_owned(),
            root: "/tmp/blog".to_owned(),
            steps: vec![mixengine_proto::PlanStep {
                action: PlanAction::WriteDotenv {
                    key: key.to_owned(),
                    path: ".env".to_owned(),
                },
                disposition: mixengine_proto::Disposition::Confirm {
                    what: format!("{key} in .env"),
                },
                elevates: false,
            }],
            source: mixengine_proto::BlueprintSource::Builtin,
            trusted: true,
            signature: None,
        }
    }

    /// **`--write-dotenv` agrees to every key; `--json` without it to none** — roadmap task
    /// **T205a**.
    #[test]
    fn the_dotenv_flag_agrees_to_each_key_and_a_json_run_to_none() {
        let plan = plan_writing("DATABASE_URL");
        assert_eq!(
            agreed_to_dotenv(&plan, true, true),
            vec!["DATABASE_URL".to_owned()]
        );
        assert!(agreed_to_dotenv(&plan, false, true).is_empty());
    }

    #[test]
    fn a_satisfied_key_is_not_asked_about() {
        let mut plan = plan_writing("DATABASE_URL");
        plan.steps[0].disposition = mixengine_proto::Disposition::Satisfied;
        assert!(agreed_to_dotenv(&plan, true, false).is_empty());
    }

    /// **The question names which kind of untrusted it is** — roadmap task **T79b**. It is asked
    /// only on a real apply, so this is where the three sentences are checked.
    #[test]
    fn the_scaffold_question_says_what_is_known_about_the_blueprint() {
        assert!(super::vouching(false, Some(SignatureCheck::Verified)).contains("is signed"));

        let rejected = super::vouching(true, Some(SignatureCheck::Rejected));
        assert!(rejected.contains("not the gallery's"), "{rejected}");

        let missing = super::vouching(true, Some(SignatureCheck::Missing));
        assert!(missing.contains("Nothing vouches"), "{missing}");

        // A row older than this task, or one whose reason this build cannot read: the sentence it
        // has always had, which says the true half of what is known.
        assert_eq!(super::vouching(true, None), missing);
    }
}
