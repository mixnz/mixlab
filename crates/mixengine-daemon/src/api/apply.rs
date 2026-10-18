//! `blueprint.apply`: carrying out the plan T77 decided — roadmap task **T78**.
//!
//! **The plan is core's, the execution is the daemon's** (the T78 design, D1).
//! `mixengine_core::blueprints::plan` reads this home's tables and decides the list; every action in
//! that list is a capability this daemon already has, and half of them — an install, a rendering, a
//! supervisor reload, a keyring write — are things `mixengine-core` deliberately cannot do. So the
//! executor is written as an `impl Api`, exactly as [`super::create`] is and for the same reason:
//! `Api` is the one type holding `projects`, `runtimes`, `packages`, `sites`, `domains`,
//! `certificates`, `extensions` and `databases` at once, and a `Blueprints` given eight more fields
//! would be a second assembly of the same handles.
//!
//! # Nothing here decides what the steps are
//!
//! The executor consumes `Vec<PlanStep>` and may **fail**, but may not add a step, drop one or
//! reorder them — the invariant T77 wrote down, and the only way `--dry-run` can promise to match
//! the real run.

mod account;
mod consent;
mod dotenv;
mod ledger;
mod prerequisites;
pub(crate) mod scaffold;
mod steps;

pub(crate) use consent::identity;
use consent::{consent_refusal, questions, refusal};
use dotenv::dotenv_refusal;
#[cfg(test)]
use dotenv::dotenv_value;

use std::path::PathBuf;
use std::sync::Arc;

use mixengine_core::blueprints::manifest::BlueprintManifest;
use mixengine_proto::{
    AppliedDatabase, BlueprintApplied, BlueprintApply, BlueprintApplyResponse, BlueprintPlan,
    DatabaseCreate, DomainAdd, Error, ErrorCode, ExtensionChoice, IssueOutcome, JobKind, LogLine,
    LogSubject, PackageTarget, PlanAction, ProjectCreate, ProjectRef, RuntimeKind, RuntimeTarget,
    ScaffoldConsent, ServiceCreate, SiteCreate, SiteRef, StepOutcome, StepResult, Stream,
    Timestamp, rpc,
};

use super::Api;
use crate::error::ToWire as _;
use crate::jobs::JobHandle;
use ledger::{Kept, Made};
use steps::Context;

impl Api {
    /// `blueprint.apply` — what applying one would do, and (from this task) doing it.
    ///
    /// # Errors
    ///
    /// `not_found` for a blueprint nothing is filed under; `invalid_argument` for a root that is not
    /// absolute, for a question nobody answered and for an answer nothing asked for;
    /// `precondition_failed` for a plan holding a step that cannot be done; and the wire error of a
    /// table that could not be read.
    pub(crate) async fn blueprint_apply(
        self: &Arc<Self>,
        asked: &BlueprintApply,
    ) -> Result<BlueprintApplyResponse, Error> {
        let (manifest, plan) = self.blueprints.planned(asked).await?;

        if asked.dry_run {
            let needs = match asked.ignore_requirements {
                true => None,
                false => self.blueprint_needs(&plan).await,
            };
            return Ok(BlueprintApplyResponse::Planned { plan, needs });
        }

        // **Two plannings, both of them pure reads**, and it is what makes both refusals trivial:
        // one list says what was *asked*, the other says what was *decided*. Reading the questions
        // off a plan the answers have already altered would be reading one list twice.
        let asking = BlueprintApply {
            answers: Vec::new(),
            ..asked.clone()
        };
        let (_, unanswered) = self.blueprints.planned(&asking).await?;

        if let Some(refused) = refusal(&questions(&unanswered), &plan, &asked.answers) {
            return Err(refused);
        }

        // **Checked here, where nothing has been written yet** (T78a, D4). A consent that names
        // another command, or that was given about a blueprint whose trust has since changed, is
        // refused rather than spent.
        if let Some(refused) = consent_refusal(&plan, asked.scaffold.as_ref()) {
            return Err(refused);
        }

        if let Some(refused) = dotenv_refusal(&plan, &asked.dotenv) {
            return Err(refused);
        }

        let kind = JobKind::parse(rpc::method::BLUEPRINT_APPLY)
            .expect("`blueprint.apply` is a method name, which is what a job kind is");
        let api = Arc::clone(self);
        let consent = asked.scaffold.clone();
        let dotenv = asked.dotenv.clone();
        let autostart = asked.autostart;
        let prerequisites = (asked.install_prerequisites, asked.ignore_requirements);

        let started = self
            .jobs
            .begin(&kind, move |handle| async move {
                let applied = api
                    .perform(
                        &plan,
                        &manifest,
                        (consent, dotenv),
                        autostart,
                        prerequisites,
                        &handle,
                    )
                    .await;

                // **The ring does not outlive the job it was opened for** — roadmap task **T120**,
                // its design's D5. One two-hundred-line ring per apply, in a map nothing prunes, is
                // growth nothing bounds; the entry has been left behind for scaffold jobs since
                // T78a, and narrating every step would make that universal.
                //
                // Both paths, which is why the `?` is below rather than above: a failed apply is
                // exactly the one somebody reads the log of.
                //
                // **`if_unwatched` is the whole of the rule, and its limit.** A client still
                // following at this moment keeps the entry — which is right, because its own
                // `broadcast::Receiver` is what the stream is reading from and nothing should be
                // taken out from under it — and nothing sweeps that entry once the stream does
                // close. So a job somebody watched to the end still leaves one ring behind, exactly
                // as `services::logs` does for a service whose last reader disconnects after the
                // runner has gone. Bounded by the number of jobs a person watches in one run of the
                // daemon; the sweep that would close it belongs with that shared surface rather
                // than here.
                api.services()
                    .logs()
                    .forget_if_unwatched(&LogSubject::Job { id: handle.id() });

                serde_json::to_value(applied?).map_err(|error| {
                    Error::new(
                        ErrorCode::Internal,
                        format!("what the apply did would not encode: {error}"),
                    )
                })
            })
            .await?;

        Ok(BlueprintApplyResponse::Started { job: started })
    }

    /// The walk: every step of the plan, in the plan's own order.
    ///
    /// **Adds no step, drops none, reorders none** — the invariant T77 wrote down. It may fail, and
    /// what it does about that is the next task's.
    async fn perform(
        &self,
        plan: &BlueprintPlan,
        manifest: &BlueprintManifest,
        (consent, dotenv): (Option<ScaffoldConsent>, Vec<String>),
        autostart: bool,
        prerequisites: (bool, bool),
        handle: &JobHandle,
    ) -> Result<BlueprintApplied, Error> {
        // **Nothing is written until every version is known** (D9). A plan holds constraints, and
        // only the index can say which release satisfies one — so it is asked here, where a failure
        // costs nothing because the ledger is still empty. Since T152, the same holds for whether
        // this machine runs those releases.
        let resolved = self.resolve(plan, handle).await?;
        let (install_prerequisites, ignore_requirements) = prerequisites;
        if !ignore_requirements {
            self.meet_requirements(plan, install_prerequisites, handle)
                .await?;
        }

        let mut context = Context {
            project: plan.project.clone(),
            root: PathBuf::from(&plan.root),
            ledger: ledger::Ledger::default(),
            consent,
            autostart,
            resolved,
            database: None,
            dotenv,
        };

        // **Opened once, before the first step** — roadmap task **T120**, its design's D5. The ring
        // is per job and `feeding` is what gives it a size: without this call `keep` is zero, so
        // every line below is published to whoever is already connected and kept for nobody — which
        // is exactly what a client connecting one step late used to find.
        let log = self
            .services()
            .logs()
            .feeding(&LogSubject::Job { id: handle.id() }, scaffold::RING_LINES);

        let total = plan.steps.len().max(1);
        let mut outcomes = Vec::with_capacity(plan.steps.len());

        for (position, step) in plan.steps.iter().enumerate() {
            // **A cancellation stops where it is and does not roll back** (D5). What has been done
            // is done, and running the apply again continues from here; throwing that away is not
            // what somebody who asked to stop was asking for.
            if handle.is_cancelled() {
                tracing::info!(
                    job = %handle.id(),
                    project = plan.project,
                    done = position,
                    "an apply was cancelled; what it had made is left for a second run"
                );
                break;
            }

            let percent = u8::try_from(position * 100 / total).unwrap_or(100);
            handle
                .progress(percent, steps::describe(&step.action))
                .await;

            // The same sentence the progress bar carries, kept where it can be read back. A bar
            // shows one line at a time and forgets the last one; this is the list of everything
            // this apply has done, which is what somebody reads *after* it has gone wrong.
            log.record(narration(Stream::Stdout, steps::describe(&step.action)));

            let result = match steps::untouched_with_consent(
                step,
                context.consent.as_ref(),
                &context.dotenv,
            ) {
                Some(result) => result,
                None => {
                    match self
                        .carry_out(plan, position, manifest, &mut context, handle)
                        .await
                    {
                        Ok(result) => result,

                        // **A failure is where the ledger is spent** (D4). What is undone belongs
                        // to the project; what is kept is named in the error, because a thing
                        // nobody was told about is a thing nobody ever cleans up.
                        Err(error) => {
                            let stubborn = ledger::unwind(self, &context.ledger).await;
                            let kept = ledger::left_behind(&context.ledger);

                            let said = match stubborn.is_empty() {
                                true => format!("{}{kept}", error.message),
                                false => format!(
                                    "{}{kept}; and this apply could not take back: {}",
                                    error.message,
                                    stubborn.join(", ")
                                ),
                            };

                            // The sentence the caller gets, in the log the caller can scroll back
                            // through — so what went wrong sits at the end of what was being done.
                            log.record(narration(Stream::Stderr, said.clone()));

                            return Err(Error::new(error.code, said).with_hint(
                                error.hint.unwrap_or_else(|| {
                                    "running the apply again picks up where this one stopped"
                                        .to_owned()
                                }),
                            ));
                        }
                    }
                }
            };

            // **What differed from the plan is said as it happens** (T202, D2), on the bar and in
            // the log, and kept in the outcome for whoever reads the report afterwards.
            if let StepResult::Done { note: Some(note) } = &result {
                handle.progress(percent, note.clone()).await;
                log.record(narration(Stream::Stdout, note.clone()));
            }

            outcomes.push(StepOutcome {
                action: step.action.clone(),
                result,
            });
        }

        log.record(narration(Stream::Stdout, "the apply has finished"));

        Ok(BlueprintApplied {
            blueprint: plan.blueprint.clone(),
            project: plan.project.clone(),
            root: plan.root.clone(),
            steps: outcomes,
            // **What is left to do** — roadmap task **T205**, D6: the blueprint's steps for this
            // project, expanded, with the row's trust beside them.
            next_steps: mixengine_core::blueprints::steps::of_manifest(
                manifest,
                &plan.project,
                plan.trusted,
            ),
            database: context.database.clone(),
        })
    }

    /// One step that is work.
    ///
    /// Takes the plan and the position rather than the step alone, because the site step reads the
    /// names off the steps that follow it (D14).
    async fn carry_out(
        &self,
        plan: &BlueprintPlan,
        position: usize,
        _manifest: &BlueprintManifest,
        context: &mut Context,
        handle: &JobHandle,
    ) -> Result<StepResult, Error> {
        let action = &plan.steps[position].action;

        // The slice of the job's bar this step owns, so that an install reporting 0–100 of itself
        // lands inside its own step rather than dragging the whole bar back (D13).
        let total = plan.steps.len().max(1);
        let from = u8::try_from(position * 100 / total).unwrap_or(100);
        let to = u8::try_from((position + 1) * 100 / total).unwrap_or(100);

        match action {
            PlanAction::RegisterProject { name, root, pins } => {
                // `project.create` takes a root that exists, and the whole point of an apply is a
                // directory that does not yet. Making it is this step's; **removing it is nobody's**
                // — a rollback keeps the directory and names it (D4), on `project.delete`'s standing
                // rule that the files were never ours.
                mixengine_core::paths::create_dir(&PathBuf::from(root))
                    .map_err(|error| error.to_wire())?;
                context
                    .ledger
                    .keeping(Kept::Directory { path: root.clone() });

                // **A resumed apply whose directory was deleted** is planned as work to make that
                // directory again, and the row is already there. Done without the ledger's
                // `Made::Project`: a rollback of this apply must not delete a project an earlier
                // one registered.
                let here = mixengine_platform::paths::in_full(std::path::Path::new(root));
                let registered =
                    mixengine_core::projects::find(&self.store, &ProjectRef::Name(name.clone()))
                        .await
                        .map_err(|error| error.to_wire())?;

                if registered.is_some_and(|project| {
                    mixengine_platform::paths::in_full(&project.root) == here
                }) {
                    // **The project points at its blueprint** — roadmap task **T205**, D5. Only where
                    // none is recorded, so a resumed apply mends an older row and never repoints one.
                    mixengine_core::projects::adopt_blueprint(&self.store, name, &plan.blueprint)
                        .await
                        .map_err(|error| error.to_wire())?;

                    return Ok(StepResult::Done { note: None });
                }

                context
                    .ledger
                    .attempting(Made::Project { name: name.clone() });

                self.projects
                    .create(&ProjectCreate {
                        root: root.clone(),
                        name: Some(name.clone()),
                        // **The pins are the point** (D7): without them the site resolves to
                        // whatever this machine defaults to, and a capture of this project would
                        // come back with no `[runtimes]` at all.
                        pins: Some(pins.clone()),
                    })
                    .await?;

                // **The project points at its blueprint** — roadmap task **T205**, D5. Only where
                // none is recorded, so a resumed apply mends an older row and never repoints one.
                mixengine_core::projects::adopt_blueprint(&self.store, name, &plan.blueprint)
                    .await
                    .map_err(|error| error.to_wire())?;

                Ok(StepResult::Done { note: None })
            }

            PlanAction::EnsureService {
                package,
                instance,
                version,
                dedicated,
            } => {
                let id = identity(package, instance)?;

                // Read now rather than from the plan, because the install step just before this one
                // may be what put it there.
                let installed = self.installed_package(package, version.as_ref()).await?;

                // **Only a dedicated instance goes in the ledger.** A shared one this apply created
                // may already have another project pointing at it by the time this fails, and one
                // that was already here was never this apply's to take away.
                if *dedicated {
                    context.ledger.attempting(Made::Service { id: id.clone() });
                }

                self.service_create(&ServiceCreate {
                    id: id.clone(),
                    version: installed,
                    port: None,
                    bind_addr: None,
                    data_dir: None,
                    // **T116.** Only reached for an instance this apply is creating — a step that
                    // planned `Satisfied` never gets here — so the flag cannot re-decide a service
                    // somebody else's project left stopped on purpose.
                    //
                    // **`None` and not `Some(false)` when nobody asked** — T167e. `Some(false)` is a
                    // person saying no, and would keep a front end this apply creates from starting
                    // with the daemon, which `service.create` now does for a front end by default.
                    autostart: context.autostart.then_some(true),
                    overrides: None,
                })
                .await?;

                Ok(StepResult::Done { note: None })
            }

            PlanAction::CreateDatabase {
                package,
                instance,
                database,
                user,
            } => {
                let service = identity(package, instance)?;

                // **Kept whatever happens next** (D4). `database.create` is idempotent, so a resumed
                // apply finds it and moves on; dropping it to tidy up a failure is the expensive
                // direction to be wrong in, and there is no `database.drop` in this product anyway.
                context.ledger.keeping(Kept::Database {
                    service: service.clone(),
                    name: database.clone(),
                });

                // **The plan's name, or the next free one** (T202, D1) — `database.create` refuses
                // an account somebody else made, and a blueprint has nobody at the keyboard to
                // pick another. The note is D2's sentence, for the step's outcome and the log.
                let made = account::with_a_free_name(
                    self.databases.as_ref(),
                    &DatabaseCreate {
                        service: service.clone(),
                        database: database.clone(),
                        user: Some(user.clone()),
                        // A blueprint never names a password — T77b's `--password` is a person
                        // choosing one at the keyboard, not a value a plan can carry.
                        password: None,
                    },
                )
                .await?;

                tracing::info!(
                    database,
                    user = made.account.user,
                    "the blueprint's database and the account that reaches it are there"
                );

                // **What really happened, for the apply's answer** — roadmap task **T205**, D6.
                context.database = Some(AppliedDatabase {
                    service,
                    database: made.account.database.clone(),
                    user: made.account.user.clone(),
                });

                Ok(StepResult::Done { note: made.note })
            }

            PlanAction::InstallRuntime { kind, .. } => {
                let version = context.resolution(&steps::runtime_key(*kind))?;

                // **Kept by a rollback, and named** (D4): a runtime belongs to the machine, and it
                // is what a resumed apply would otherwise download all over again.
                context.ledger.keeping(Kept::Runtime {
                    kind: *kind,
                    version: version.clone(),
                });

                self.runtimes
                    .perform(
                        &RuntimeTarget {
                            kind: *kind,
                            version,
                        },
                        &handle.slice(from, to),
                    )
                    .await?;

                Ok(StepResult::Done { note: None })
            }

            PlanAction::InstallPackage { package, .. } => {
                let version = context.resolution(&steps::package_key(package))?;

                context.ledger.keeping(Kept::Package {
                    package: package.clone(),
                    version: version.clone(),
                });

                self.packages
                    .perform(
                        &PackageTarget {
                            package: package.clone(),
                            version,
                        },
                        &handle.slice(from, to),
                    )
                    .await?;

                Ok(StepResult::Done { note: None })
            }

            PlanAction::CreateSite {
                kind,
                doc_root,
                https,
                routes,
                services,
            } => {
                // **The one place the walk looks ahead** (D14): a site cannot be created nameless,
                // and the names are read off the plan's own steps rather than expanded a second
                // time — so there stays exactly one place where `{project}` became `shop`.
                let domains = steps::names_after(plan, position);

                context.ledger.attempting(Made::Site {
                    domain: domains.first().cloned().unwrap_or_default(),
                });

                // **One call, several steps** (D3): `site.create` writes the row, queues the hosts
                // entries and issues the certificate, so the `AddDomain` and `IssueCertificate`
                // steps that follow find themselves already true when their turn comes.
                self.sites
                    .create(&SiteCreate {
                        project: ProjectRef::Name(context.project.clone()),
                        // Every field is named, so nothing falls through to the manifest's sites.
                        from: None,
                        domains: Some(domains),
                        doc_root: Some(doc_root.clone()),
                        kind: Some(kind.clone()),
                        // **The links matter beyond the moment** (D14): a site created without them
                        // has an empty `site_service_links`, and a capture of this project would
                        // lose every `[[services]]` entry it should have carried. Its own list when
                        // the blueprint gave one, and otherwise every service the *plan* made sure
                        // of — a shared instance that was already here is never carried out, and
                        // is no less this project's (T204a, D3).
                        services: Some(match services {
                            Some(listed) => listed.clone(),
                            None => steps::ensured_in(plan),
                        }),
                        // **Written with the site, not after it** — roadmap task **T135**. There is
                        // no `AddRoute` step and there should not be: a route is a column of the
                        // site rather than a name the hosts file has to learn, so it costs no
                        // elevation and nothing can observe the site without them.
                        routes: Some(routes.clone()),
                        https: Some(*https),
                        // A blueprint describes what a site *is*; a redirect is a fact about one
                        // home's traffic, which no manifest declares — roadmap task **T98**.
                        https_redirect: None,
                        // A `.local` name is one this plan would have blocked before the job began.
                        accept_risky_tld: false,
                    })
                    .await?;

                Ok(StepResult::Done { note: None })
            }

            PlanAction::AddDomain { domain, .. } => {
                // Every action is an ensure (D3). The site step above created this name with the
                // site; a plan that reached here without it — a site that already existed, a name
                // added to one — still gets its name.
                if self.answers_to(domain).await? {
                    return Ok(StepResult::AlreadyTrue);
                }

                // **The site this name belongs to** — roadmap task **T204a**, D4: the first
                // name of its group that already answers. A project may hold several sites, so
                // its root names none of them; the root is only the answer for a plan with no
                // group to read, which no plan this build makes is.
                let mut site = None;
                for name in steps::group_names(plan, position) {
                    if name != *domain && self.answers_to(&name).await? {
                        site = Some(SiteRef::Domain(name));
                        break;
                    }
                }

                self.domains
                    .add(&DomainAdd {
                        site: site
                            .unwrap_or_else(|| SiteRef::Path(context.root.display().to_string())),
                        domain: domain.clone(),
                        accept_risky_tld: false,
                    })
                    .await?;

                Ok(StepResult::Done { note: None })
            }

            PlanAction::IssueCertificate { domains } => {
                let Some(name) = domains.first() else {
                    return Ok(StepResult::AlreadyTrue);
                };

                // **A certificate that could not be issued does not undo a project.** `site.create`
                // already takes this position — *a site that exists is worth more than a certificate
                // that does not, and `mix doctor` reports the gap* — and rolling a whole apply back
                // over one would be the expensive direction to be wrong in. So it is reported as a
                // step that did not run, with the reason.
                let (site, _) = self.sites.expect(&SiteRef::Domain(name.clone())).await?;

                match self.certificates.issue(Some(site)).await {
                    // **Reported by what became true** (D3). `site.create` above will already have
                    // issued this one, and an issue that finds a usable certificate says `Reused` —
                    // which is `already true` and not a second certificate.
                    Ok(report) => Ok(match report.sites.first().map(|one| &one.outcome) {
                        Some(IssueOutcome::Issued {}) => StepResult::Done { note: None },

                        Some(IssueOutcome::Refused { because }) => StepResult::NotRun {
                            why: format!("no certificate was issued for {name}: {because}"),
                        },

                        // `Reused`, `NotWanted`, and a report with nothing in it: nothing was done
                        // and nothing needed doing.
                        _ => StepResult::AlreadyTrue,
                    }),

                    Err(error) => Ok(StepResult::NotRun {
                        why: format!(
                            "no certificate was issued for {name}: {}; `mix cert issue` tries \
                             again, and `mix doctor` says what is missing",
                            error.message
                        ),
                    }),
                }
            }

            PlanAction::SetPhpExtension { name, .. } => {
                // **Kept by a rollback, and named** (D4): an extension choice belongs to an
                // installed runtime, so it reaches every project on this machine — and turning it
                // back off to tidy up would change somebody else's PHP.
                context
                    .ledger
                    .keeping(Kept::Extension { name: name.clone() });

                // **Which PHP is resolved now, not read from the plan.** On a machine that had no
                // PHP the plan could not name one — the runtime step above has just installed it —
                // and the question is the one every `php` in this directory will ask: the
                // project's pin, or the default where it pins none.
                let resolved = mixengine_core::resolve::runtime(
                    &self.store,
                    &mixengine_core::resolve::Question {
                        kind: RuntimeKind::Php,
                        cwd: Some(&context.root),
                        explicit: None,
                    },
                )
                .await;

                let turned = match resolved {
                    Ok(php) => {
                        let version = php.runtime.version;

                        self.php_extensions
                            .set(&ExtensionChoice {
                                runtime: RuntimeTarget {
                                    kind: RuntimeKind::Php,
                                    version: version.clone(),
                                },
                                name: name.clone(),
                                enabled: true,
                            })
                            .await
                            .map(|_| ())
                            .map_err(|error| {
                                format!(
                                    "{}. `mix runtime ext enable {name} --php {}` tries again",
                                    error.message,
                                    version.as_str()
                                )
                            })
                    }

                    Err(error) => Err(format!(
                        "this project has no PHP to run it ({}). `mix runtime ext enable {name}` \
                         tries again once one is installed",
                        error.to_wire().message
                    )),
                };

                // **The certificate's rule, and for its reason**: a project without `xdebug` is a
                // project somebody can work in, and taking their site away over an extension the
                // index does not offer would be the expensive direction to be wrong in. The line
                // says which one, so it can be turned on by hand.
                match turned {
                    Ok(()) => Ok(StepResult::Done { note: None }),

                    Err(reason) => Ok(StepResult::NotRun {
                        why: format!("the PHP extension {name} was not turned on: {reason}"),
                    }),
                }
            }

            PlanAction::RunScaffold { command } => {
                // **The ledger is not spent by this step** (T78a, D8): it answers a `StepResult`
                // rather than an error, so a command that exits non-zero never reaches the
                // unwinding path below. What it leaves is a project that works — the site serves,
                // the database is there — and destroying that because somebody's post-install
                // script failed is the more expensive direction to be wrong in.
                let log = self
                    .services()
                    .logs()
                    .feeding(&LogSubject::Job { id: handle.id() }, scaffold::RING_LINES);

                let outcome = scaffold::run_command(
                    command,
                    &context.root,
                    &scaffold::environment(self.paths()),
                    log.as_ref(),
                    Some(handle),
                )
                .await;

                // The whole of what this apply had left to do, so the bar is honest about it even
                // where the command printed nothing.
                handle
                    .progress(100, "the blueprint's own command has ended")
                    .await;

                Ok(outcome)
            }

            PlanAction::WriteDotenv { key, path } => {
                Ok(self.write_dotenv(context, key, path).await)
            }

            PlanAction::FetchArchive { url, strip } => {
                // **A failure is a failed step, not a failed apply** — roadmap task **T205**, D2, on
                // the scaffold's own rule: the project it made works, and tearing it down over a
                // download is the more expensive direction to be wrong in.
                let installer = mixengine_core::install::Installer::new(self.paths().cache())
                    .map_err(|error| error.to_wire())?;
                let staging = self
                    .paths()
                    .cache()
                    .join("scaffold")
                    .join(handle.id().to_string());

                handle.progress(from, format!("downloading {url}")).await;

                let outcome = mixengine_core::blueprints::archive::fetch(
                    &installer,
                    url,
                    strip.as_deref(),
                    &staging,
                    &context.root,
                )
                .await;

                handle
                    .progress(to, "the blueprint's archive is unpacked")
                    .await;

                Ok(match outcome {
                    Ok(()) => StepResult::Done { note: None },
                    Err(error) => StepResult::Failed {
                        why: format!("{url} was not unpacked: {error}"),
                    },
                })
            }

            other => Err(Error::new(
                ErrorCode::Internal,
                format!(
                    "this build does not yet carry out {}",
                    steps::describe(other)
                ),
            )),
        }
    }

    /// Whether some site on this machine already answers to a name.
    async fn answers_to(&self, domain: &str) -> Result<bool, Error> {
        mixengine_core::sites::by_domain(&self.store, &domain.to_ascii_lowercase())
            .await
            .map(|found| found.is_some())
            .map_err(|error| error.to_wire())
    }
}

fn narration(stream: Stream, text: impl Into<String>) -> LogLine {
    LogLine {
        stream,
        at: Timestamp::from_system_time(std::time::SystemTime::now()),
        text: text.into(),
    }
}

#[cfg(test)]
mod tests;
