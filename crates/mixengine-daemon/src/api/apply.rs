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
mod ledger;
pub(crate) mod scaffold;
mod steps;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use mixengine_core::blueprints::manifest::BlueprintManifest;
use mixengine_proto::{
    AnswerSubject, AppliedDatabase, BlueprintApplied, BlueprintApply, BlueprintApplyResponse,
    BlueprintPlan, DatabaseCreate, DatabaseCredentialsQuery, DatabaseProtocol, Disposition,
    DomainAdd, Error, ErrorCode, ExtensionChoice, IssueOutcome, JobKind, LogLine, LogSubject,
    PackageTarget, PackageVersion, PlanAction, PlanStep, ProjectCreate, ProjectRef, Requirement,
    RuntimeKind, RuntimeTarget, ScaffoldConsent, ServiceCreate, ServiceId, SiteCreate, SiteRef,
    StepOutcome, StepResult, Stream, Timestamp, VersionAnswer, rpc,
};

use super::Api;
use crate::error::ToWire as _;
use crate::jobs::JobHandle;
use crate::requirements;
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
            ensured: Vec::new(),
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

                context.ensured.push(id);

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
                ..
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
                        // lose every `[[services]]` entry it should have carried.
                        services: Some(context.ensured.clone()),
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

                self.domains
                    .add(&DomainAdd {
                        // By the root rather than by a name: the site's own names are what this
                        // step is adding to, and a blueprint's project holds exactly one site —
                        // T77 refuses to capture a project with two.
                        site: SiteRef::Path(context.root.display().to_string()),
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

    /// Every version this apply will install, decided before it writes anything.
    ///
    /// **D9.** A plan reads this home's tables and never the index, so it holds a
    /// [`VersionConstraint`](mixengine_proto::VersionConstraint) where a release belongs. Turning
    /// one into a release needs the index, which needs the network — so it happens here, once, at
    /// the top of the job: a constraint nothing satisfies fails while the ledger is still empty and
    /// there is nothing to take back.
    async fn resolve(
        &self,
        plan: &BlueprintPlan,
        handle: &JobHandle,
    ) -> Result<BTreeMap<String, PackageVersion>, Error> {
        let mut resolved = BTreeMap::new();

        for step in &plan.steps {
            if !matches!(step.disposition, Disposition::Create) {
                continue;
            }

            match &step.action {
                PlanAction::InstallRuntime { kind, wanted } => {
                    handle
                        .progress(
                            0,
                            format!("looking up {} {}", kind.as_str(), wanted.as_str()),
                        )
                        .await;

                    resolved.insert(
                        steps::runtime_key(*kind),
                        self.runtimes.newest_satisfying(*kind, wanted).await?,
                    );
                }

                PlanAction::InstallPackage { package, wanted } => {
                    handle.progress(0, format!("looking up {package}")).await;

                    resolved.insert(
                        steps::package_key(package),
                        self.packages
                            .newest_satisfying(package, wanted.as_ref())
                            .await?,
                    );
                }

                _ => {}
            }
        }

        Ok(resolved)
    }

    /// The release every `Create` install step resolves to, without reporting progress — T152.
    ///
    /// [`Self::resolve`]'s lookups for a caller with no job: the dry run.
    async fn install_targets(
        &self,
        plan: &BlueprintPlan,
    ) -> Result<Vec<(String, PackageVersion)>, Error> {
        let mut targets = Vec::new();

        for step in &plan.steps {
            if !matches!(step.disposition, Disposition::Create) {
                continue;
            }

            match &step.action {
                PlanAction::InstallRuntime { kind, wanted } => targets.push((
                    kind.as_str().to_owned(),
                    self.runtimes.newest_satisfying(*kind, wanted).await?,
                )),
                PlanAction::InstallPackage { package, wanted } => targets.push((
                    package.clone(),
                    self.packages
                        .newest_satisfying(package, wanted.as_ref())
                        .await?,
                )),
                _ => {}
            }
        }

        Ok(targets)
    }

    /// What those releases lack on this machine, merged — T152, design D7.
    ///
    /// [`None`] when anything needed to judge could not be had: a dry run prints its plan regardless.
    async fn blueprint_needs(&self, plan: &BlueprintPlan) -> Option<Vec<Requirement>> {
        let targets = self.install_targets(plan).await.ok()?;
        let names: Vec<&str> = targets.iter().map(|(kind, _)| kind.as_str()).collect();
        let catalogue = self.runtimes.fetcher().index.kinds(&names).await.ok()?;
        let facts = requirements::facts();

        Some(mixengine_core::requirements::merged(
            targets.iter().flat_map(|(kind, version)| {
                requirements::of(&catalogue.index, kind, version.as_str(), &facts)
            }),
        ))
    }

    /// Refuse, or install what was agreed and judge again — before the first step writes anything.
    ///
    /// # Errors
    ///
    /// `dependency_missing` for a lack nothing agreed to fixes, and whatever
    /// [`requirements::satisfy`] refuses with.
    async fn meet_requirements(
        &self,
        plan: &BlueprintPlan,
        install_prerequisites: bool,
        handle: &JobHandle,
    ) -> Result<(), Error> {
        // A plan that installs nothing asks the index nothing — an apply that worked offline before
        // T152 still does.
        let targets = self.install_targets(plan).await?;
        if targets.is_empty() {
            return Ok(());
        }

        // **An index that cannot be read refuses nothing here**, on `requirements::gate`'s reasoning:
        // the install steps read it too, and report that in their own words.
        let fetcher = self.runtimes.fetcher();
        let names: Vec<&str> = targets.iter().map(|(kind, _)| kind.as_str()).collect();
        let Ok(catalogue) = fetcher.index.kinds(&names).await else {
            return Ok(());
        };

        let judge = || {
            let facts = requirements::facts();
            mixengine_core::requirements::merged(targets.iter().flat_map(|(kind, version)| {
                requirements::of(&catalogue.index, kind, version.as_str(), &facts)
            }))
        };
        let subject = requirements::Subject {
            name: "this blueprint".to_owned(),
            install: None,
        };

        let unmet = judge();
        if let Some(refused) = requirements::refusal(&subject, &unmet, install_prerequisites) {
            return Err(refused);
        }
        if !mixengine_core::requirements::needs_consent(&unmet) {
            return Ok(());
        }

        requirements::satisfy(fetcher, &unmet, handle).await?;
        requirements::refusal(&subject, &judge(), false).map_or(Ok(()), Err)
    }

    /// The installed version of a package that satisfies what the blueprint asked for.
    ///
    /// The newest of them, which is the same rule the plan used — applied to what is on disk *now*,
    /// because the install step just before this one may be what put it there.
    async fn installed_package(
        &self,
        package: &str,
        wanted: Option<&mixengine_proto::VersionConstraint>,
    ) -> Result<mixengine_proto::PackageVersion, Error> {
        let installed = mixengine_core::packages::records(&self.store, Some(package))
            .await
            .map_err(|error| error.to_wire())?;

        installed
            .into_iter()
            .filter(|record| wanted.is_none_or(|wanted| wanted.matches(&record.version)))
            .map(|record| record.version)
            .max_by(mixengine_proto::PackageVersion::cmp_precedence)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PreconditionFailed,
                    match wanted {
                        Some(wanted) => {
                            format!("no installed {package} satisfies {}", wanted.as_str())
                        }
                        None => format!("no {package} is installed"),
                    },
                )
                .with_hint("`mix package available` lists what the index offers")
            })
    }
}

/// The id the plan's package and instance make, which the plan already checked they could.
///
/// Reaching the error here means the plan changed underneath this apply — a pair that could not be
/// spelled is `Blocked` at planning time, and a blocked plan never becomes a job.
fn identity(package: &str, instance: &str) -> Result<ServiceId, Error> {
    ServiceId::parse(package)
        .ok()
        .filter(|bare| bare.as_str() == instance)
        .or_else(|| ServiceId::parse(format!("{package}@{instance}")).ok())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::Internal,
                format!("{package}@{instance} cannot be a service id"),
            )
        })
}

/// Every question a plan asks, which is every `Choice` left in it.
///
/// Read from a plan built with **no** answers, so that "was this asked" and "was this answered" are
/// two readings rather than one reading the other has already altered.
fn questions(plan: &BlueprintPlan) -> Vec<AnswerSubject> {
    plan.steps
        .iter()
        .filter(|step| matches!(step.disposition, Disposition::Choice { .. }))
        .filter_map(|step| match &step.action {
            PlanAction::InstallRuntime { kind, .. } => Some(AnswerSubject::Runtime { kind: *kind }),

            // The pair travels apart in the action and together here, which is safe for exactly the
            // reason the answer type says: a service question only arises for an instance that
            // already exists, so the id was spellable before the question was asked.
            PlanAction::EnsureService {
                package, instance, ..
            } => ServiceId::parse(format!("{package}@{instance}"))
                .ok()
                .or_else(|| ServiceId::parse(package).ok())
                .map(|id| AnswerSubject::Service { id }),

            _ => None,
        })
        .collect()
}

/// Why a consent may not be spent on this plan, or [`None`] — roadmap task **T78a**, its design's
/// D4.
///
/// **A consent names what was read.** A blueprint can be re-imported between the plan a person read
/// and the apply they sent, so a consent naming another command is consent to something else, and
/// one that thought the blueprint was signed is consent given under a fact that has changed. Both
/// are refused here, before anything is touched.
///
/// **No consent is not a refusal.** The scaffold step ends `NotRun` and everything else is applied,
/// which is T78's position kept: a blueprint must not become worthless because nobody answered one
/// question.
fn consent_refusal(plan: &BlueprintPlan, consent: Option<&ScaffoldConsent>) -> Option<Error> {
    let consent = consent?;

    // **A command or an archive, whichever this plan has** — roadmap task **T205**, D2. Both are
    // the one step a person agrees to, and the consent names what was shown: the command, or the
    // URL.
    let planned_step = plan.steps.iter().find(|step| {
        matches!(
            step.action,
            PlanAction::RunScaffold { .. } | PlanAction::FetchArchive { .. }
        )
    });
    let planned = planned_step.and_then(|step| match &step.action {
        PlanAction::RunScaffold { command } => Some(command.as_str()),
        PlanAction::FetchArchive { url, .. } => Some(url.as_str()),
        _ => None,
    });
    let agreed = consent
        .archive
        .as_deref()
        .unwrap_or(consent.command.as_str());

    let Some(planned) = planned else {
        return Some(
            Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "nothing in this plan runs `{agreed}`; this agrees to a plan made against \
                     another blueprint, or another moment"
                ),
            )
            .with_hint("`mix blueprint apply --dry-run` prints the steps this plan does have"),
        );
    };

    if planned != agreed {
        return Some(
            Error::new(
                ErrorCode::InvalidArgument,
                format!("this plan runs `{planned}`, and what was agreed to was `{agreed}`"),
            )
            .with_hint("the blueprint changed since the plan was read; read it again"),
        );
    }

    if consent.untrusted == plan.trusted {
        return Some(
            Error::new(
                ErrorCode::InvalidArgument,
                match plan.trusted {
                    true => format!(
                        "`{planned}` was agreed to as an untrusted blueprint's command, and {} \
                         is signed",
                        plan.blueprint
                    ),
                    false => format!(
                        "`{planned}` was agreed to as a signed blueprint's command, and nothing \
                         vouches for {}",
                        plan.blueprint
                    ),
                },
            )
            .with_hint("the blueprint was imported again since the plan was read; read it again"),
        );
    }

    // **Agreed to, and cannot run** — roadmap task **T78b**, its design's D5. Refused here, in the
    // plan's own words, rather than as a shell's complaint at the end of a job. The hint never says
    // `bin/`: that directory is swept of strangers at every start (T26).
    if let Some(PlanStep {
        disposition: Disposition::Blocked { reason },
        ..
    }) = planned_step
    {
        return Some(
            Error::new(ErrorCode::PreconditionFailed, reason.clone()).with_hint(
                "install it, put it on your PATH and restart the daemon, or apply without \
                 agreeing to the command, and everything else is still applied",
            ),
        );
    }

    None
}

/// Why this apply may not start, or [`None`].
///
/// **Three refusals, in the order they are cheapest to explain**: something that cannot be done at
/// all, a question nobody answered, and an answer to a question nobody asked. All of them are
/// decided here rather than five actions into a project directory, which is the whole point of
/// having planned first (the T77 design, D10).
fn refusal(
    questions: &[AnswerSubject],
    plan: &BlueprintPlan,
    answers: &[VersionAnswer],
) -> Option<Error> {
    // **A blocked scaffold blocks the scaffold and not the apply** (T78b, D5): the one optional
    // step, whose refusal — when a consent names it — is `consent_refusal`'s.
    let blocked: Vec<String> = plan
        .steps
        .iter()
        .filter(|step| !matches!(step.action, PlanAction::RunScaffold { .. }))
        .filter_map(|step| match &step.disposition {
            Disposition::Blocked { reason } | Disposition::Unsupported { reason } => {
                Some(reason.clone())
            }
            _ => None,
        })
        .collect();

    if !blocked.is_empty() {
        return Some(
            Error::new(
                ErrorCode::PreconditionFailed,
                format!(
                    "this blueprint cannot be applied here: {}",
                    blocked.join("; ")
                ),
            )
            .with_hint(
                "`mix blueprint apply --dry-run` prints every step and what stands in its way",
            ),
        );
    }

    let unanswered: Vec<String> = questions
        .iter()
        .filter(|subject| !answers.iter().any(|given| &&given.subject == subject))
        .map(ToString::to_string)
        .collect();

    if !unanswered.is_empty() {
        return Some(
            Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "this blueprint asks for a version this machine does not have, and nobody \
                     answered for: {}",
                    unanswered.join(", ")
                ),
            )
            .with_hint(
                "`mix blueprint apply` asks each question at the terminal; `--install-missing` and \
                 `--use-installed` answer them all",
            ),
        );
    }

    let unasked: Vec<String> = answers
        .iter()
        .filter(|given| !questions.contains(&given.subject))
        .map(|given| given.subject.to_string())
        .collect();

    match unasked.is_empty() {
        true => None,
        false => Some(
            Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "nothing in this plan asks about: {}; this answers a plan made against \
                     another machine, or another moment",
                    unasked.join(", ")
                ),
            )
            .with_hint("`mix blueprint apply --dry-run` prints the questions this plan does ask"),
        ),
    }
}

/// One line of an apply's own narration.
///
/// **A job's log carries what the apply did, and not only what somebody else's command printed** —
/// roadmap task **T120**, its design's D5. Until then `LogSubject::Job` was written by exactly one
/// step, the `[scaffold]` command, so a failure anywhere before it left a log with nothing in it —
/// which is precisely what a client rendering that log then had to show. `mix job logs <id>` gets
/// the same narration, with no client change at all.
///
/// [`Stream::Stderr`] for a step that did not go well, on the ordinary convention: a client
/// colouring its output has something to colour by.
impl Api {
    /// The `.env` line — roadmap task **T205a**: the URL of the database this apply made, appended
    /// to the person's file.
    ///
    /// **A failure is a failed step**, as the scaffold's is: the project works, and the person can
    /// set the key by hand. No sentence here carries the value.
    async fn write_dotenv(&self, context: &steps::Context, key: &str, path: &str) -> StepResult {
        // **The account this apply made**, not the plan's: T202 may have renamed it.
        let Some(made) = context.database.clone() else {
            return StepResult::Failed {
                why: format!("{key} was not written to {path}: no database was made"),
            };
        };

        let outcome = async {
            let address = mixengine_core::services::handoff::address(&self.store, &made.service)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("{} is not a database a client opens", made.service))?;
            let credentials = self
                .databases
                .credentials(&DatabaseCredentialsQuery {
                    service: made.service.clone(),
                    user: Some(made.user.clone()),
                })
                .await
                .map_err(|error| error.message)?;
            let value = dotenv_value(
                &made,
                address.protocol,
                &address.host.to_string(),
                address.port,
                &credentials.password,
            );
            mixengine_core::blueprints::dotenv::write(&context.root, key, &value)
                .map_err(|error| error.to_string())
        }
        .await;

        match outcome {
            Ok(true) => StepResult::Done { note: None },
            Ok(false) => StepResult::AlreadyTrue,
            Err(why) => StepResult::Failed {
                why: format!("{key} was not written to {path}: {why}"),
            },
        }
    }
}

/// A key agreed to that this plan does not write, or [`None`] — roadmap task **T205a**, on
/// [`consent_refusal`]'s rule: a consent names what was read.
fn dotenv_refusal(plan: &BlueprintPlan, agreed: &[String]) -> Option<Error> {
    let offered: Vec<&str> = plan
        .steps
        .iter()
        .filter_map(|step| match &step.action {
            PlanAction::WriteDotenv { key, .. } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    let stray = agreed.iter().find(|key| !offered.contains(&key.as_str()))?;

    Some(
        Error::new(
            ErrorCode::InvalidArgument,
            format!("this plan writes no {stray} to .env, and that is what was agreed to"),
        )
        .with_hint("the blueprint changed since the plan was read; read it again"),
    )
}

/// The URL written for the database this apply made — roadmap task **T205a**.
fn dotenv_value(
    made: &AppliedDatabase,
    protocol: DatabaseProtocol,
    host: &str,
    port: u16,
    password: &str,
) -> String {
    mixengine_core::blueprints::dotenv::database_url(
        protocol.as_str(),
        &made.user,
        password,
        host,
        port,
        &made.database,
    )
}

fn narration(stream: Stream, text: impl Into<String>) -> LogLine {
    LogLine {
        stream,
        at: Timestamp::from_system_time(std::time::SystemTime::now()),
        text: text.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use mixengine_proto::{MismatchAnswer, PackageVersion, RuntimeKind, VersionConstraint};

    /// **D5.** Before roadmap task **T120** the only writer of a job's log ring was the scaffold
    /// step, so an apply that failed before it — which is every apply that fails on a name — had an
    /// empty log, and a client's "show output" button had nothing to show. The stream is part of
    /// the line rather than a detail of the rendering: a client colouring its output needs to know
    /// which lines went badly without reading them.
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

    /// **A blocked scaffold blocks the scaffold, not the apply** — roadmap task **T78b**, its
    /// design's D5. Without a consent the plan is work, and the step is left with its reason.
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

    /// **A consent naming another command is consent to something else** — roadmap task **T78a**,
    /// its design's D4. A blueprint can be re-imported between the plan a person read and the apply
    /// they sent.
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

    /// **A consent names the archive it agreed to** — roadmap task **T205**, D2.
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

    /// **A consent names what was read** — roadmap task **T205a**, `consent_refusal`'s rule for
    /// a `.env` key.
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

    /// **The account the apply made**, which T202 may have renamed — roadmap task **T205a**.
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
}
