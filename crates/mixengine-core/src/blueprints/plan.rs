//! What applying a blueprint would do, decided before anything happens.
//!
//! Roadmap task **T77**; **T78** is what carries the result out.
//!
//! # One place decides, and the order is part of the answer
//!
//! The feature's acceptance criterion is that `--dry-run` matches exactly what the real run
//! performs. That is only enforceable while one function decides what the actions are, so this is
//! it: T78's executor consumes the list and may **fail**, but may not add a step, drop one or
//! reorder them. The order here is dependency order — project, runtimes, services, databases, site,
//! domains, certificate, extensions, scaffold — and is asserted by a test rather than left to the
//! shape of the code.
//!
//! What is deliberately *not* in a plan is anything only the execution can know: the port a new
//! instance lands on, a generated password, a rowid. A plan that named them would be a plan the
//! executor has to contradict.
//!
//! # It reads this home's tables, and nothing else
//!
//! **No index, no network** (D9). The mismatch prompt in the feature doc shows a download size, and
//! a size means asking the index, and the index has a network behind a six-hour cache — while this
//! is the command a person runs *because* they do not want anything to happen yet. So a version
//! nothing installed satisfies is `create`, without a size, and whether the index still publishes it
//! is discovered by the real run.
//!
//! # Everything that cannot be done is decided here
//!
//! **D10.** The point of a plan is that an apply does not get five actions into a project directory
//! before discovering that the sixth was impossible — so a name that is too long, a directory that
//! is already another project's, and a domain another site owns are all `blocked` at this point,
//! each naming what stands in the way.

use std::collections::BTreeMap;
use std::path::Path;

use mixengine_proto::{
    AnswerSubject, BlueprintPlan, Disposition, MismatchAnswer, PackageVersion, PlanAction,
    PlanStep, RuntimeKind, ServiceId, SiteKind, VersionAnswer, VersionConstraint,
};

use crate::blueprints::manifest::PER_PROJECT;
use crate::blueprints::store::Filed;
use crate::generate::{Catalogue, recipe::Instancing};
use crate::{Result, Store, projects, runtimes, services, sites};

/// The token a manifest writes where the project's own name went.
const TOKEN: &str = "{project}";

/// The front end a `[site]` gets on a home that has none — roadmap task **T115**.
///
/// **Caddy, and not a manifest key.** A blueprint may not name a front end: exactly one runs at a
/// time per home (T37), the choice is the home's and lives in `service.set_front_end`, and a
/// manifest that named one would apply differently on a machine that had already chosen. What
/// travels between machines is *this project is served over HTTP*, which `[site]` already says.
///
/// Caddy because it is this product's documented default and the server the warm-start budget was
/// measured against. A home that has chosen Nginx keeps it: the rule only fires where there is no
/// front end at all.
const FRONT_END: &str = "caddy";

/// What one apply is being planned for: everything the caller decided, in one value.
///
/// **A struct and not nine parameters**, which is what it was until T115 added the ninth. The
/// grouping is not arbitrary: `store` and the [`Catalogue`] are *this machine*, and everything here
/// is *this request* — which is also the split that decides what a second caller would have to
/// supply.
#[derive(Debug, Clone, Copy)]
pub struct Wanted<'a> {
    /// The slug this plan is for, carried through to [`BlueprintPlan::blueprint`].
    pub blueprint: &'a str,

    /// The manifest and how it arrived — its source, and whether anything vouched for it.
    pub filed: &'a Filed,

    /// What the new project is called, and what `{project}` expands to.
    pub project: &'a str,

    /// Where it would live. Absolute.
    pub root: &'a Path,

    /// The answers to the version questions this plan raises — roadmap task **T78**.
    pub answers: &'a [VersionAnswer],

    /// The `PATH` a `[scaffold]` command would run with — `<home>/bin`, then the daemon's own.
    ///
    /// Handed in rather than read here, so that a dry run and an apply judge the same string and a
    /// test can hand in a directory of its own (roadmap task **T78b**, its design's D3).
    pub scaffold_path: &'a std::ffi::OsStr,

    /// Whether to plan a front end where this home has none — roadmap task **T115**.
    ///
    /// Off is the default everywhere: an apply is about a project, and provisioning the machine it
    /// runs on is a wider thing that is asked for rather than assumed.
    pub front_end: bool,
}

/// What applying `manifest` under `project` would do.
///
/// What the caller decided is [`Wanted`]; `store` and `catalogue` are this machine.
///
/// # Errors
///
/// [`crate::Error::Database`] when a table cannot be read. Everything a person did wrong is a
/// [`Disposition::Blocked`] step rather than an error: a plan that refused to be printed would be a
/// plan that could not tell you *why*.
pub async fn plan(
    store: &Store,
    catalogue: &Catalogue,
    wanted: &Wanted<'_>,
) -> Result<BlueprintPlan> {
    let &Wanted {
        blueprint,
        filed,
        project,
        root,
        answers,
        scaffold_path,
        front_end,
    } = wanted;

    let manifest = &filed.manifest;
    let mut steps = Vec::new();

    // **The name, the way `project.create` will store it** — roadmap task **T128**.
    // [`projects::validated_name`] trims, so a plan carrying what was typed and a row holding what
    // was stored are two spellings of one project — and everything downstream compares them by
    // equality. Reported from a real apply of `Laravel `: the resumption check in [`register`] found
    // neither the project nor a collision and planned a `Create` that its own root then blocked;
    // `ProjectRef::Name(plan.project)` could not find the row the site had to hang off; and the
    // rollback of a failed apply warned `no such project: Laravel ` about a project it had just
    // made. One trim, before anything reads the name, rather than three comparisons taught to be
    // lenient.
    //
    // **A name that is not one is left exactly as it arrived**, because [`register`] is what says so
    // — as a `Blocked` step naming the reason, which is this function's contract: everything a
    // person did wrong is a step they can read rather than an error that prints no plan at all.
    let validated = projects::validated_name(project);
    let project = validated.as_deref().unwrap_or(project);

    // **What `{project}` becomes** — roadmap task **T120**, its design's D1. A project's *name* is a
    // label a person reads, and `projects::validated_name` admits a space, a capital and a `;` into
    // it; a database identifier, a DNS label, a `ServiceId` instance and a shell command each have a
    // narrower charset than that. [`crate::domains::slug`] is the one rule that crosses all four,
    // and `project.create` has derived a project's default domain with it since T39a — so this is
    // that same handle, derived once, rather than a second answer to a question with one.
    //
    // [`None`] is a name with no ASCII in it to slug. The token is then left unexpanded and each
    // step that uses it refuses it (D3), which is why that is not an error here: a manifest that
    // never mentions `{project}` is unaffected by a name nothing can be made of.
    let handle = crate::domains::slug(project);
    let handle = handle.as_deref();

    // **Decided before the project step, because the pins it registers are what these answers
    // settle** (D7). The order of the steps themselves is unchanged: the runtimes are pushed
    // straight after the register, which is where they have always been.
    let mut runtimes = Vec::new();
    let mut pins = BTreeMap::new();

    for (kind, wanted) in &manifest.runtimes {
        let (step, pin) = runtime(store, *kind, wanted, answered_runtime(answers, *kind)).await?;

        pins.insert(*kind, pin);
        runtimes.push(step);
    }

    // What the project will pin PHP to, answers included: an extension belongs on that PHP.
    let php_pin = pins.get(&RuntimeKind::Php).cloned();

    let (registered, mine) = register(store, project, root, pins).await?;
    steps.push(registered);
    steps.extend(runtimes);

    // **A site needs something to serve it, when the caller asked for one** — roadmap task T115.
    // `core::sites` is explicit that "a home with no front end renders nothing and this succeeds",
    // so a manifest with a `[site]` applied to a fresh machine ends with a project, a database, a
    // site row, a domain, a certificate — and nothing listening. That is the whole of the complaint
    // this phase is written against, and `services.md` already names it: *"a first run that offers
    // to do it for them is not built"*.
    //
    // **Asked for, and not always**, which is the correction this task made to its own design. An
    // apply is about a project; provisioning the machine it runs on is a wider thing, and doing it
    // unasked would make every apply on a home with no web server download one — including the ones
    // that were deliberately about something else. So `front_end` travels in the request, defaulted
    // off, and the caller that sets it is the caller whose sentence is *get me a working site*.
    //
    // Expressed with the two actions a `[[services]]` entry already produces, so the plan gains two
    // familiar lines, the wire gains nothing, and the version question — where the package is not
    // installed — is asked through the machinery a client can already answer. A home that has
    // chosen a front end, Nginx included, plans `Satisfied` for both and is left alone.
    //
    // **Here and not beside the `[site]` step**, which is where it reads: the order of a plan is by
    // *kind* — project, runtimes, packages, services, databases, site, domains — and it is asserted
    // by `the_steps_are_in_dependency_order`, so an install and an ensure pushed after the database
    // steps would be a plan out of order. First among the services rather than last, because it is
    // the one every site on the machine is reached through and nothing here depends on it.
    if front_end
        && !manifest.sites.is_empty()
        && services::front_end::held_by(store, catalogue)
            .await?
            .is_none()
    {
        // **The instance name comes from the recipe, never from a convention here.** A front end is
        // `Instancing::Single` — there is one Caddy, and `service.create` refuses `caddy@main` in as
        // many words — so a hardcoded `"main"` would plan a step the executor is guaranteed to be
        // refused on. Asked of the catalogue so that a future front end which is *not* a singleton
        // is planned correctly without this line being revisited.
        let instance = match catalogue
            .recipe(FRONT_END)
            .map(|recipe| recipe.instancing())
        {
            Some(Instancing::Single) | None => FRONT_END,
            Some(Instancing::Named) => "main",
        };

        steps.push(package(store, FRONT_END, None).await?);
        steps.push(ensure(store, FRONT_END, instance, None, false, answers).await?);
    }

    // Each `[[services]]` entry's id as its `EnsureService` step names it, by position — what a
    // site's `services` list resolves to (T204a, D3).
    let mut ensured_ids: Vec<Option<ServiceId>> = Vec::with_capacity(manifest.services.len());

    for service in &manifest.services {
        let instance =
            instance_of(store, &service.name, service.instance.as_deref(), project).await;
        let dedicated = service.instance.as_deref() == Some(PER_PROJECT);
        ensured_ids.push(identity(&service.name, &instance));

        steps.push(package(store, &service.name, service.version.as_ref()).await?);
        steps.push(
            ensure(
                store,
                &service.name,
                &instance,
                service.version.as_ref(),
                dedicated,
                answers,
            )
            .await?,
        );

        if let Some(database) = &service.database {
            let user = service.user.clone().unwrap_or_else(|| database.clone());
            steps.push(database_step(
                &service.name,
                &instance,
                &expand(database, handle),
                &expand(&user, handle),
            ));
        }
    }

    // **One group per site, in file order** — roadmap task **T204a**, D4: create, its names, its
    // certificate. A group rather than a tier per action kind, because the executor reads a site's
    // names off the steps straight after it.
    let several = manifest.sites.len() > 1;
    let mut claimed: Vec<String> = Vec::new();

    for site in &manifest.sites {
        let mut names = Vec::new();
        names.push(expand(&site.domain_pattern, handle));
        names.extend(site.aliases.iter().map(|alias| expand(alias, handle)));

        // **What this site links, as the ensure steps name it** (D3). The reader already refused an
        // item naming no entry or two, so a miss here is an id nothing can spell, which `ensure`
        // has already blocked.
        let services = site.services.as_ref().map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let named =
                        crate::blueprints::manifest::linked_service(item, &manifest.services)
                            .ok()?;
                    let position = manifest
                        .services
                        .iter()
                        .position(|service| std::ptr::eq(service, named))?;
                    ensured_ids.get(position).cloned().flatten()
                })
                .collect()
        });

        let action = PlanAction::CreateSite {
            kind: match &site.kind {
                // Which pool a new site uses is decided on the machine that makes it.
                SiteKind::PhpFpm { .. } => SiteKind::PhpFpm { pool: None },
                other => other.clone(),
            },
            // What this machine will make, so spelled the way it spells a path (T191); the
            // manifest keeps `/`, and `site.create` accepts either.
            doc_root: crate::paths::native_relative(&site.doc_root),
            https: site.https,
            // Carried whole — roadmap task **T135**. A php-fpm route's pool is already `None` in
            // the manifest, on capture's own rule, so there is nothing to clear here.
            routes: site.routes.clone(),
            services,
        };

        // **D2 again, and the step that found it out.** A project this apply already made, already
        // holding its site, is not a second site waiting to be created — and a plan that said
        // otherwise made a resumed apply fail on `already_exists`. Which site is *this* one is
        // [`made`]'s question (T204a, D4).
        steps.push(match made(store, mine, several, &names, &claimed).await? {
            Made::No => PlanStep {
                action,
                disposition: Disposition::Create,
                elevates: false,
            },
            Made::Yes => satisfied(action),
            Made::Shared { with } => blocked(
                action,
                format!(
                    "{} and {with} are one site in this project, and the blueprint describes them \
                     as two",
                    names[0]
                ),
            ),
        });

        for (position, domain) in names.iter().enumerate() {
            steps.push(domain_step(store, domain, position == 0, mine).await?);
        }

        if site.https {
            steps.push(PlanStep {
                action: PlanAction::IssueCertificate {
                    domains: names.clone(),
                },
                disposition: Disposition::Create,
                // On a machine that has never issued one, this installs the authority.
                elevates: true,
            });
        }

        claimed.extend(names);
    }

    if let Some(php) = &manifest.php {
        let installed = php_of(store, php_pin.as_ref()).await?;

        for name in &php.extensions {
            steps.push(extension(store, installed.as_ref(), name).await?);
        }
    }

    if let Some(scaffold) = &manifest.scaffold {
        // **Expanded here, with everything else** — roadmap task **T78a**, its design's D6. The
        // command a person is shown is the command that runs.
        //
        // **The substitution is safe in front of a shell because what is substituted is the
        // handle** — roadmap task **T120**, its design's D4. [`crate::domains::slug`] answers in
        // `[a-z0-9-]` and nothing else, which holds no shell metacharacter. This line used to say
        // that the *project's name* had been through that charset. It had not, and
        // `projects::validated_name` admits `;`, `$` and a backtick to this day — so a project
        // called `a; rm -rf $HOME` reached the shell as two commands.
        let command = expand(&scaffold.command, handle);

        steps.push(PlanStep {
            // Arbitrary code from whoever wrote the blueprint. What answers this is the consent in
            // the apply request (T78a, D4); here it is shown, exactly as it would run — or blocked,
            // when its program is a bare name the PATH does not hold (T78b, D1), when the token
            // did not expand (T120, D3), when it names itself after its directory and this one's
            // name is not one npm will take (T120c, D5), or when it asked for an empty directory
            // and this one is not.
            //
            // **A shell is the one name space with no validator**, so this check is the whole of
            // what stands between a handle that could not be made and a command carrying a literal
            // `{project}` into somebody's project directory.
            //
            // **The PATH is judged before the directory** — a machine with no `composer` on it is
            // not made applicable by emptying a folder, so that is the sentence worth reading
            // first. Only one reason is ever shown, which is why the order is written down.
            //
            // **And the directory's name before its contents** (T120c): emptying a folder whose
            // name is going to be refused anyway is work nobody gets back.
            disposition: match unexpanded(&command) {
                Some(reason) => Disposition::Blocked { reason },
                None => match crate::blueprints::program::disposition(&command, scaffold_path) {
                    Disposition::Confirm { what } => {
                        let refusal = scaffold
                            .needs_npm_safe_dir
                            .then(|| not_an_npm_name(root))
                            .flatten()
                            .or_else(|| scaffold.needs_empty_dir.then(|| occupied(root)).flatten());

                        match refusal {
                            Some(reason) => Disposition::Blocked { reason },
                            None => Disposition::Confirm { what },
                        }
                    }
                    judged => judged,
                },
            },
            action: PlanAction::RunScaffold { command },
            elevates: false,
        });
    }

    if let Some(archive) = &manifest.archive {
        // **The scaffold's place and the scaffold's gate** — roadmap task **T205**, D2. Unpacking
        // is not running, but the PHP it writes runs the moment somebody opens the site, so the
        // URL is shown and agreed to exactly as a command is.
        let disposition = if !crate::blueprints::archive::format_known(&archive.url) {
            let file = archive
                .url
                .split(['?', '#'])
                .next()
                .unwrap_or(&archive.url)
                .rsplit('/')
                .next()
                .unwrap_or_default();
            let suffix = file
                .rsplit_once('.')
                .map(|(_, extension)| format!(".{extension}"))
                .unwrap_or_else(|| "no suffix".to_owned());

            Disposition::Blocked {
                reason: format!(
                    "{} ends in {suffix}, and this build unpacks .zip, .tar.gz and .tar.zst",
                    archive.url
                ),
            }
        } else if archive.when_empty && occupied(root).is_some() {
            // **A starter steps aside for code already there**: a cloned folder keeps its own
            // files, and the apply goes on around them.
            Disposition::Satisfied
        } else {
            match archive.needs_empty_dir.then(|| occupied(root)).flatten() {
                Some(reason) => Disposition::Blocked { reason },
                None => Disposition::Confirm {
                    what: archive.url.clone(),
                },
            }
        };

        steps.push(PlanStep {
            action: PlanAction::FetchArchive {
                url: archive.url.clone(),
                strip: archive.strip.clone(),
            },
            disposition,
            elevates: false,
        });
    }

    // **After the scaffold, and asked for** — roadmap task **T205a**. After, because
    // `create-project .` refuses a folder that already holds a `.env`; asked, because it writes a
    // password into the person's own file. A key already there is theirs (`Satisfied`).
    if let Some(key) = manifest
        .services
        .iter()
        .find_map(|service| service.dotenv.as_ref())
    {
        use crate::blueprints::dotenv;

        let present = std::fs::read_to_string(root.join(dotenv::FILE))
            .is_ok_and(|text| dotenv::has_key(&text, key));
        let action = PlanAction::WriteDotenv {
            key: key.clone(),
            path: dotenv::FILE.to_owned(),
        };
        steps.push(match present {
            true => satisfied(action),
            false => PlanStep {
                action,
                disposition: Disposition::Confirm {
                    what: format!("{key} in {}", dotenv::FILE),
                },
                elevates: false,
            },
        });
    }

    Ok(BlueprintPlan {
        blueprint: blueprint.to_owned(),
        project: project.to_owned(),
        root: root.display().to_string(),
        steps,
        source: filed.source,
        trusted: filed.trusted,
        signature: filed.signature,
    })
}

/// The project itself: its name, the versions it will ask for, and whether this directory is free.
///
/// Answers with the rowid of the project this apply is *about* where there already is one, which is
/// what [`domain_step`] needs to tell a name this apply already claimed from a name somebody else
/// holds.
async fn register(
    store: &Store,
    project: &str,
    root: &Path,
    pins: BTreeMap<RuntimeKind, VersionConstraint>,
) -> Result<(PlanStep, Option<i64>)> {
    let action = PlanAction::RegisterProject {
        name: project.to_owned(),
        root: root.display().to_string(),
        pins,
    };

    if let Err(error) = projects::validated_name(project) {
        return Ok((blocked(action, error.to_string()), None));
    }

    let registered = projects::records(store).await?;
    let here = mixengine_platform::paths::in_full(root);

    // **Resumption is a re-plan** (the T78 design, D2). A project of this name *at this root* is
    // this apply's own first step, already taken, and calling that a collision would make a failed
    // apply impossible to run again. Both halves have to match: anything narrower is two projects
    // colliding, which is what the blocks below are for.
    //
    // **Already taken only while its directory is still there.** This step is the one that makes
    // the directory, so a row whose folder somebody has since deleted is work again: planned
    // `Satisfied`, nothing would make it, and the scaffold would be started in a folder that is not
    // there. The executor finds the row and makes only the directory.
    if let Some(mine) = registered.iter().find(|record| {
        record.name == project && mixengine_platform::paths::in_full(&record.root) == here
    }) {
        let step = match root.is_dir() {
            true => satisfied(action),
            false => PlanStep {
                action,
                disposition: Disposition::Create,
                elevates: false,
            },
        };

        return Ok((step, Some(mine.id)));
    }

    if registered.iter().any(|record| record.name == project) {
        return Ok((
            blocked(
                action,
                format!("a project called {project} is already registered elsewhere"),
            ),
            None,
        ));
    }

    if let Some(holder) = registered
        .iter()
        .find(|record| mixengine_platform::paths::in_full(&record.root) == here)
    {
        return Ok((
            blocked(
                action,
                format!("{} is already the project {}", root.display(), holder.name),
            ),
            None,
        ));
    }

    Ok((
        PlanStep {
            action,
            disposition: Disposition::Create,
            elevates: false,
        },
        None,
    ))
}

/// Whether a site this apply describes is already made — roadmap task **T204a**, D4.
enum Made {
    /// Nothing of this project answers to its names.
    No,

    /// A site of this project does.
    Yes,

    /// The site that does also answers to `with`, a name of an entry planned before this one.
    Shared { with: String },
}

/// Whether one site is already made.
///
/// **Made when a site of this project answers to any of its names**, primary or alias, so a
/// renamed primary kept as an alias still finds it. A one-site blueprint keeps the older question
/// — *does this project have a site* — so a site renamed outright still resumes.
///
/// `claimed` is every name of the entries planned before this one: a site holding one of those as
/// well is two entries collapsed into one site here, which neither merging nor a third site fixes.
/// [`None`] is a project that does not exist yet, which holds nothing.
async fn made(
    store: &Store,
    project: Option<i64>,
    several: bool,
    names: &[String],
    claimed: &[String],
) -> Result<Made> {
    let Some(project) = project else {
        return Ok(Made::No);
    };
    let held = sites::records(store, Some(project)).await?;

    if !several {
        return Ok(match held.is_empty() {
            true => Made::No,
            false => Made::Yes,
        });
    }

    let Some(site) = held
        .iter()
        .find(|site| site.domains.iter().any(|domain| names.contains(domain)))
    else {
        return Ok(Made::No);
    };

    Ok(
        match site.domains.iter().find(|domain| claimed.contains(domain)) {
            Some(other) => Made::Shared {
                with: other.clone(),
            },
            None => Made::Yes,
        },
    )
}

/// The answer for one language, where somebody gave one.
fn answered_runtime(answers: &[VersionAnswer], kind: RuntimeKind) -> Option<MismatchAnswer> {
    answers.iter().find_map(|given| match &given.subject {
        AnswerSubject::Runtime { kind: asked } if *asked == kind => Some(given.answer),
        _ => None,
    })
}

/// The answer for one instance, where somebody gave one.
fn answered_service(answers: &[VersionAnswer], id: &ServiceId) -> Option<MismatchAnswer> {
    answers.iter().find_map(|given| match &given.subject {
        AnswerSubject::Service { id: asked } if asked == id => Some(given.answer),
        _ => None,
    })
}

/// One `[runtimes]` entry against what is installed, and the pin the project gets from it.
///
/// **The pin is half of the answer** (D7). Without it "install 8.2.23" and "use the installed
/// 8.2.29" would leave identical machines behind and the question would be theatre.
async fn runtime(
    store: &Store,
    kind: RuntimeKind,
    wanted: &VersionConstraint,
    answer: Option<MismatchAnswer>,
) -> Result<(PlanStep, VersionConstraint)> {
    let action = PlanAction::InstallRuntime {
        kind,
        wanted: wanted.clone(),
    };

    let installed = runtimes::records(store, Some(kind)).await?;

    if installed
        .iter()
        .any(|record| wanted.matches(&record.version))
    {
        return Ok((satisfied(action), wanted.clone()));
    }

    // Newest first, because that is the one an "use what is installed" answer would take.
    let newest = installed
        .into_iter()
        .max_by(|left, right| left.version.cmp_precedence(&right.version));

    Ok(match (newest, answer) {
        // Nothing to choose between: there is no question here, whatever anybody answered.
        (None, _) => (
            PlanStep {
                action,
                disposition: Disposition::Create,
                elevates: false,
            },
            wanted.clone(),
        ),

        (Some(record), None) => (
            PlanStep {
                action,
                disposition: Disposition::Choice {
                    installed: record.version,
                    wanted: wanted.clone(),
                },
                elevates: false,
            },
            wanted.clone(),
        ),

        (Some(_), Some(MismatchAnswer::Install)) => (
            PlanStep {
                action,
                disposition: Disposition::Create,
                elevates: false,
            },
            wanted.clone(),
        ),

        // A version this store holds is one it could name a directory after, so it is a constraint
        // too; the fallback is unreachable rather than lenient.
        (Some(record), Some(MismatchAnswer::UseInstalled)) => (
            satisfied(action),
            VersionConstraint::parse(record.version.as_str()).unwrap_or_else(|_| wanted.clone()),
        ),
    })
}

/// Which instance name a `[[services]]` entry means on *this* machine.
///
/// `per-project` becomes the new project's own name. An absent instance follows the lookup
/// [`crate::manifest`] documents — the bare package name first, which is what a single-instance
/// package such as `caddy` is actually called, and `main` after it.
///
/// The pair may still be unspellable as a [`ServiceId`] — a project called `My Blog` cannot name an
/// instance — and that is [`ensure`]'s to report, not this function's to paper over.
async fn instance_of(store: &Store, name: &str, instance: Option<&str>, project: &str) -> String {
    match instance {
        Some(PER_PROJECT) => project.to_owned(),
        Some(named) => named.to_owned(),
        None => match ServiceId::parse(name) {
            Ok(bare) if services::record(store, &bare).await.is_ok() => name.to_owned(),
            _ => "main".to_owned(),
        },
    }
}

/// The id that pair would have, or [`None`] when it cannot be spelled as one.
fn identity(package: &str, instance: &str) -> Option<ServiceId> {
    ServiceId::parse(package)
        .ok()
        .filter(|bare| bare.as_str() == instance)
        .or_else(|| ServiceId::parse(format!("{package}@{instance}")).ok())
}

/// Whether the package an instance would run is on this disk at all.
///
/// **T77 planned this for languages and not for services** (D8), and `service.create` refuses with
/// `precondition_failed` when the version it is asked for is not installed — a plan discovering the
/// impossible five actions into a project directory, which is the whole thing a plan exists to
/// prevent, and the ordinary case for a blueprint that came from somebody else's machine.
///
/// **It never asks a question.** A version mismatch is a question about an *instance* that already
/// exists, and [`ensure`] is where it is asked; where there is no instance yet there is nothing to
/// reuse and nothing to choose between.
async fn package(
    store: &Store,
    name: &str,
    wanted: Option<&VersionConstraint>,
) -> Result<PlanStep> {
    let action = PlanAction::InstallPackage {
        package: name.to_owned(),
        wanted: wanted.cloned(),
    };

    let installed = crate::packages::records(store, Some(name)).await?;

    let have = match wanted {
        Some(wanted) => installed
            .iter()
            .any(|record| wanted.matches(&record.version)),
        // Nothing pinned: any version of it is the version this blueprint asked for.
        None => !installed.is_empty(),
    };

    Ok(match have {
        true => satisfied(action),
        false => PlanStep {
            action,
            disposition: Disposition::Create,
            elevates: false,
        },
    })
}

/// Whether that instance is already here, and at the right version.
async fn ensure(
    store: &Store,
    package: &str,
    instance: &str,
    wanted: Option<&VersionConstraint>,
    dedicated: bool,
    answers: &[VersionAnswer],
) -> Result<PlanStep> {
    let action = PlanAction::EnsureService {
        package: package.to_owned(),
        instance: instance.to_owned(),
        version: wanted.cloned(),
        dedicated,
    };

    // **D10.** A pair no id can be spelled from is decided here, with the reason, rather than at
    // the moment T78 tries to write the row.
    let Some(id) = identity(package, instance) else {
        return Ok(blocked(
            action,
            format!("{package}@{instance} cannot be a service id"),
        ));
    };

    if services::record(store, &id).await.is_err() {
        return Ok(PlanStep {
            action,
            disposition: Disposition::Create,
            elevates: false,
        });
    }

    let installed = services::version(store, &id).await?;

    Ok(match (wanted, installed) {
        (Some(wanted), Some(installed)) if !wanted.matches(&installed) => {
            match answered_service(answers, &id) {
                // Reusing what is here is the one thing this build can do about it.
                Some(MismatchAnswer::UseInstalled) => satisfied(action),

                // **A blocked step and not an error.** Repointing an existing instance at another
                // version is a database upgrade under somebody's data directory, and this build has
                // no method for it: `service.create` and `service.delete` are the two ends of a
                // row's life with nothing between them. Said here, which is where every other
                // impossibility is said (D10).
                Some(MismatchAnswer::Install) => blocked(
                    action,
                    format!(
                        "{id} is already running {installed}, and this build cannot move an \
                         existing instance to another version; answer `use_installed` to reuse \
                         it, or give the blueprint `instance = \"per-project\"` for one of its own"
                    ),
                ),

                None => PlanStep {
                    action,
                    disposition: Disposition::Choice {
                        installed,
                        wanted: wanted.clone(),
                    },
                    elevates: false,
                },
            }
        }
        _ => satisfied(action),
    })
}

/// The database and the account, or the reason neither can be made under this name.
///
/// **Asks the function that owns the rule** — roadmap task **T120**, its design's D2. This used to
/// check the account's length against a copy of MySQL's limit and nothing else, so a name the server
/// would refuse outright planned as `Create` and failed after the directory, the runtimes and the
/// packages were already on disk. [`crate::generate::databases::validated_identifier`] is the same
/// call `database.create` makes, so a plan and an apply now agree by construction rather than by two
/// people keeping two rules in step.
fn database_step(package: &str, instance: &str, database: &str, user: &str) -> PlanStep {
    let action = PlanAction::CreateDatabase {
        package: package.to_owned(),
        instance: instance.to_owned(),
        database: database.to_owned(),
        user: user.to_owned(),
    };

    let refusal = unexpanded(database)
        .or_else(|| unexpanded(user))
        .or_else(|| {
            [database, user]
                .into_iter()
                .find_map(|name| crate::generate::databases::validated_identifier(name).err())
                .map(|error| error.to_string())
        });

    match refusal {
        Some(reason) => blocked(action, reason),
        None => PlanStep {
            action,
            disposition: Disposition::Create,
            elevates: false,
        },
    }
}

/// One name, and who already answers to it.
async fn domain_step(
    store: &Store,
    domain: &str,
    primary: bool,
    project: Option<i64>,
) -> Result<PlanStep> {
    let action = PlanAction::AddDomain {
        domain: domain.to_owned(),
        primary,
    };

    // **A domain is checked before it is looked up** — roadmap task **T120**, its design's D2.
    // Asking who holds a name that is not a name at all answers "nobody", and the step then plans as
    // `Create` for something `site.create` will refuse — a plan promising what it has not checked.
    if let Some(reason) = unexpanded(domain).or_else(|| {
        crate::domains::normalised(domain, false)
            .err()
            .map(|error| error.to_string())
    }) {
        return Ok(blocked(action, reason));
    }

    Ok(match sites::by_domain(store, domain).await? {
        // Already ours, which is what a resumed apply looks like from here (D2). Narrower than "the
        // name is taken" by exactly one condition, and that condition is the whole difference
        // between running an apply twice and being told to go away.
        Some(holder)
            if project.is_some_and(|project| {
                holder.owner == crate::sites::SiteOwner::Project(project)
            }) =>
        {
            satisfied(action)
        }

        Some(holder) => blocked(
            action,
            format!(
                "{domain} is already answered by {}",
                holder
                    .domains
                    .first()
                    .map_or("another site", |primary| primary.as_str())
            ),
        ),
        // Writing the hosts file is what needs the prompt, and saying so before anything starts is
        // the point of D11.
        None => PlanStep {
            action,
            disposition: Disposition::Create,
            elevates: true,
        },
    })
}

/// One extension against the PHP that would run it.
async fn extension(
    store: &Store,
    installed: Option<&PackageVersion>,
    name: &str,
) -> Result<PlanStep> {
    let Some(version) = installed else {
        // Nothing to enable it on yet; the runtime step above already says the PHP is coming, and
        // the apply resolves which one it is once that step has run.
        return Ok(PlanStep {
            action: PlanAction::SetPhpExtension {
                runtime: None,
                name: name.to_owned(),
            },
            disposition: Disposition::Create,
            elevates: false,
        });
    };

    let action = PlanAction::SetPhpExtension {
        runtime: Some(version.clone()),
        name: name.to_owned(),
    };

    let state = crate::runtimes::extensions::state(store, RuntimeKind::Php, version).await?;

    Ok(match state.loaded().iter().any(|loaded| loaded == name) {
        true => satisfied(action),
        false => PlanStep {
            action,
            disposition: Disposition::Create,
            elevates: false,
        },
    })
}

/// The installed PHP the project will run once it is registered, where one answers already.
///
/// **The same answer [`crate::resolve::runtime`] gives the registered project**: the newest
/// installed version its pin matches, or the default where it pins none. The newest PHP on the
/// machine was the answer here before, and an extension turned on for 8.4 does nothing for a
/// project that runs 8.2.
async fn php_of(store: &Store, pin: Option<&VersionConstraint>) -> Result<Option<PackageVersion>> {
    let installed = runtimes::records(store, Some(RuntimeKind::Php)).await?;

    Ok(match pin {
        Some(pin) => installed
            .into_iter()
            .filter(|record| pin.matches(&record.version))
            .max_by(|left, right| left.version.cmp_precedence(&right.version)),
        None => installed.into_iter().find(|record| record.default),
    }
    .map(|record| record.version))
}

/// `{project}` becomes the project's **handle** — its name as [`crate::domains::slug`] makes it.
/// **Once, here**, so no later branch can expand it differently.
///
/// A `handle` of [`None`] is a project name with no ASCII in it to slug, and the token is then left
/// exactly where it is. That is deliberate — roadmap task **T120**, its design's D3: every name
/// space this value reaches refuses `{project}` on its own rule, so the refusal happens at plan time
/// and names the token that could not be expanded, rather than a substitution nobody asked for
/// reaching a shell.
pub(crate) fn expand(value: &str, handle: Option<&str>) -> String {
    match handle {
        Some(handle) => value.replace(TOKEN, handle),
        None => value.to_owned(),
    }
}

/// A step that needs nothing done.
fn satisfied(action: PlanAction) -> PlanStep {
    PlanStep {
        action,
        disposition: Disposition::Satisfied,
        elevates: false,
    }
}

/// The reason a name still holding `{project}` cannot be used, or [`None`] when it holds no token.
///
/// **A better sentence than the charset's** — roadmap task **T120**, its design's D3. A name that
/// could not be expanded fails [`crate::generate::databases::validated_identifier`] and
/// [`crate::domains::normalised`] too, but on a character the person never typed: telling somebody
/// that `{` is not allowed in a database name sends them looking for a `{` in their own. This says
/// what actually happened.
fn unexpanded(name: &str) -> Option<String> {
    name.contains(TOKEN)
        .then(|| format!("there is nothing in the project's name to expand {TOKEN} into"))
}

/// How many of a directory's entries a refusal names before it starts counting them.
const NAMED: usize = 3;

/// The reason `root` is no place for a command that needs an empty directory, or [`None`].
///
/// **[`None`] for three different directories**, and deliberately so: one that is not there yet —
/// which is the ordinary case, since an apply is usually what creates it — one that holds nothing,
/// and one this daemon cannot read. The last is T78b's D2 one subject along: every doubt resolves
/// to *not judging*, because a false `blocked` stops a blueprint that would have worked, and a
/// directory whose listing fails is a doubt rather than an answer.
///
/// **It names what is in the way** rather than saying "not empty". The entry a person hits this on
/// is very often `.git`, or what an apply that stopped partway left behind, and a file manager
/// hiding dotfiles shows them an empty folder while they read that it is not.
fn occupied(root: &Path) -> Option<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return None;
    };

    // Sorted, because `read_dir` answers in whatever order the file system happens to hold and a
    // reason that reads differently on two runs is a reason nobody can quote.
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();

    if names.is_empty() {
        return None;
    }

    names.sort();

    let listed = match names.len() > NAMED {
        true => format!(
            "{}, and {} more",
            names[..NAMED].join(", "),
            names.len() - NAMED
        ),
        false => names.join(", "),
    };

    Some(format!(
        "{} already holds {listed}; this blueprint's command needs a directory with nothing in \
         it at all, hidden files included. Point the apply at another directory, or clear this \
         one; an earlier apply that stopped partway leaves files here too.",
        root.display()
    ))
}

/// The punctuation npm will take in a package name, beside letters and digits.
const NPM_PUNCTUATION: [char; 3] = ['-', '_', '.'];

/// The longest name npm will register.
const NPM_LIMIT: usize = 214;

/// The reason `root`'s name is no place for a command that names itself after its directory, or
/// [`None`] — roadmap task **T120c**.
///
/// **npm's rule and not [`crate::domains::slug`]'s**, which is the distinction this function exists
/// to hold. `slug` turns every character outside `a-z0-9` into a hyphen, underscore included, so a
/// check built on it refuses `next_js_1` — a directory `create-next-app` accepts, installs into and
/// writes `"name": "next_js_1"` for. A DNS label and a package name are different rules, and this
/// is the package one.
///
/// **Where it is unsure it permits**, the design's D6. A refusal here forbids somebody a thing that
/// would have worked and offers no way round it; a miss costs one wasted install and falls back to
/// the failure's own words. So a path with no final component at all — a drive root — is not
/// judged.
///
/// `slug` still has a job here, and it is the other one: it always answers in a charset every
/// branch below accepts, so it is what the reason *suggests*.
pub fn not_an_npm_name(root: &Path) -> Option<String> {
    let name = root.file_name()?.to_string_lossy().into_owned();

    let because = if name.is_empty() || name.chars().count() > NPM_LIMIT {
        "npm takes a name of one to two hundred and fourteen characters"
    } else if name.starts_with('.') || name.starts_with('_') {
        "npm takes no name beginning with a dot or an underscore"
    } else if name.chars().any(|character| character.is_ascii_uppercase()) {
        "npm takes no capital letters in a package name"
    } else if !name.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || NPM_PUNCTUATION.contains(&character)
    }) {
        "npm takes only lower-case letters, digits, and `-`, `_` or `.`"
    } else {
        return None;
    };

    // The answer rather than the rule: `slug` always produces a name the branches above accept.
    let suggestion = match crate::domains::slug(&name) {
        Some(slug) => format!("; rename the folder to `{slug}` and apply again"),
        None => String::new(),
    };

    Some(format!(
        "this command takes its package name from the folder it runs in, and `{name}` is not one \
         npm will accept: {because}{suggestion}"
    ))
}

/// A step that cannot be done, and why.
fn blocked(action: PlanAction, reason: String) -> PlanStep {
    PlanStep {
        action,
        disposition: Disposition::Blocked { reason },
        elevates: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use mixengine_proto::{BlueprintSource, SignatureCheck};

    use crate::blueprints::manifest::BlueprintManifest;

    use std::collections::BTreeMap;

    use crate::blueprints::manifest::{BlueprintService, BlueprintSite, Header, Php, Provenance};

    async fn home() -> (tempfile::TempDir, Store) {
        let temp = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(&temp.path().join("mixengine.db"))
            .await
            .expect("a database");

        (temp, store)
    }

    /// A `PATH` holding these programs and nothing else, on this system's rule.
    ///
    /// Each is a copy of this test binary under the program's name: the copy keeps the execute
    /// bit `execvp` wants, and `EXE_SUFFIX` gives it the extension `cmd.exe` wants — without a
    /// `#[cfg]` naming either system, which `workspace_layering` keeps out of this crate.
    fn a_path_holding(temp: &tempfile::TempDir, programs: &[&str]) -> std::ffi::OsString {
        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&tools).expect("a tools directory");
        let itself = std::env::current_exe().expect("this test binary");

        for name in programs {
            let file = tools.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            std::fs::copy(&itself, &file).expect("a program");
        }

        tools.into_os_string()
    }

    /// A `PATH` holding nothing.
    fn nowhere() -> &'static std::ffi::OsStr {
        std::ffi::OsStr::new("")
    }

    fn a_manifest() -> BlueprintManifest {
        BlueprintManifest {
            schema: 1,
            blueprint: Header {
                name: "blog-stack".to_owned(),
                description: String::new(),
                created_at: "2026-09-01T09:00:00Z".to_owned(),
                created_on: Provenance {
                    os: "linux".to_owned(),
                    version: "0.1.0".to_owned(),
                },
            },
            runtimes: [(
                RuntimeKind::Php,
                VersionConstraint::parse("8.2.23").expect("a constraint"),
            )]
            .into_iter()
            .collect(),
            sites: vec![BlueprintSite {
                kind: SiteKind::PhpFpm { pool: None },
                doc_root: "public".to_owned(),
                https: true,
                domain_pattern: "{project}.test".to_owned(),
                aliases: Vec::new(),
                routes: Vec::new(),
                services: None,
            }],
            services: vec![BlueprintService {
                name: "mariadb".to_owned(),
                version: Some(VersionConstraint::parse("11.4.3").expect("a constraint")),
                instance: Some("main".to_owned()),
                database: Some("{project}".to_owned()),
                user: Some("{project}".to_owned()),
                dotenv: None,
            }],
            php: Some(Php {
                extensions: vec!["xdebug".to_owned()],
            }),
            scaffold: None,
            archive: None,
            next_steps: Vec::new(),
        }
    }

    /// A blueprint this machine wrote down itself, which is what nearly every test here is about.
    fn captured(manifest: BlueprintManifest) -> Filed {
        Filed {
            manifest,
            source: BlueprintSource::Captured,
            trusted: true,
            signature: None,
        }
    }

    /// One somebody handed over, with nothing vouching for it.
    fn imported(manifest: BlueprintManifest) -> Filed {
        Filed {
            manifest,
            source: BlueprintSource::Imported,
            trusted: false,
            signature: Some(SignatureCheck::Missing),
        }
    }

    async fn an_installed_php(store: &Store, version: &str, choices: &str) {
        sqlx::query(
            r#"INSERT INTO runtime_installs
                   (id, kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256, extension_choices_json, extensions_json)
               VALUES (1, 'php', ?1, 'stable', '/runtimes/php', '2026-09-01T00:00:00Z', 1,
                       'https://example.invalid/php', 'ab', ?2,
                       '{"shared":["xdebug"],"enabled":[],"compiled_in":[]}')"#,
        )
        .bind(version)
        .bind(choices)
        .execute(store.pool())
        .await
        .expect("a runtime install");
    }

    /// One more PHP beside [`an_installed_php`]'s, under its own row.
    async fn another_installed_php(store: &Store, id: i64, version: &str) {
        sqlx::query(
            r#"INSERT INTO runtime_installs
                   (id, kind, version, channel, install_path, installed_at, size_bytes, source_url,
                    sha256, extension_choices_json, extensions_json)
               VALUES (?1, 'php', ?2, 'stable', '/runtimes/php-other', '2026-09-01T00:00:00Z', 1,
                       'https://example.invalid/php', 'ab', '{}',
                       '{"shared":["xdebug"],"enabled":[],"compiled_in":[]}')"#,
        )
        .bind(id)
        .bind(version)
        .execute(store.pool())
        .await
        .expect("another runtime install");
    }

    async fn an_installed_mariadb(store: &Store, version: &str) {
        sqlx::query(
            "INSERT INTO packages (id, name, version, install_path, installed_at, source_url, sha256)
             VALUES (1, 'mariadb', ?1, '/packages/mariadb', '2026-09-01T00:00:00Z',
                     'https://example.invalid/m', 'ab')",
        )
        .bind(version)
        .execute(store.pool())
        .await
        .expect("a package");

        sqlx::query(
            "INSERT INTO services (id, package_id, instance_name, state, port)
             VALUES ('mariadb@main', 1, 'main', 'stopped', 3306)",
        )
        .execute(store.pool())
        .await
        .expect("a service");
    }

    fn step_of(planned: &BlueprintPlan, wanted: impl Fn(&PlanAction) -> bool) -> &PlanStep {
        planned
            .steps
            .iter()
            .find(|step| wanted(&step.action))
            .expect("the step")
    }

    /// The `InstallPackage` or `EnsureService` step for one package.
    ///
    /// **Since T115 a plan holds more than one of each.** A manifest with a `[site]` applied to a
    /// home with no front end plans Caddy as well, so a test asking for "the install step" would
    /// get whichever came first and assert about the wrong service.
    fn for_package<'a>(planned: &'a BlueprintPlan, name: &str) -> Vec<&'a PlanStep> {
        planned
            .steps
            .iter()
            .filter(|step| match &step.action {
                PlanAction::InstallPackage { package, .. }
                | PlanAction::EnsureService { package, .. } => package == name,
                _ => false,
            })
            .collect()
    }

    /// That package's `InstallPackage` step.
    fn install_of<'a>(planned: &'a BlueprintPlan, name: &str) -> &'a PlanStep {
        for_package(planned, name)
            .into_iter()
            .find(|step| matches!(step.action, PlanAction::InstallPackage { .. }))
            .expect("an install step for this package")
    }

    /// And its `EnsureService` step.
    fn ensure_of<'a>(planned: &'a BlueprintPlan, name: &str) -> &'a PlanStep {
        for_package(planned, name)
            .into_iter()
            .find(|step| matches!(step.action, PlanAction::EnsureService { .. }))
            .expect("an ensure step for this package")
    }

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

    /// A project registered at `root`, as the two resumption tests need one.
    async fn a_project(store: &Store, name: &str, root: &std::path::Path) -> i64 {
        std::fs::create_dir_all(root).expect("a directory");

        crate::projects::create(
            store,
            &crate::projects::Registration {
                name: name.to_owned(),
                root: root.to_path_buf(),
                pins: BTreeMap::new(),
            },
            mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
        )
        .await
        .expect("a project")
        .id
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

    /// **`{project}` reaches the scaffold command too** — roadmap task **T78a**, its design's D6.
    /// T77 expanded the token into domains, databases and accounts and cloned the command verbatim,
    /// so a blueprint naming the project in its own command planned the token rather than the name.
    /// The command somebody agrees to has to be the command that runs.
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

    /// The plan says which blueprint it is applying and whether anybody vouched for it (D5), so a
    /// client shows the right words from the answer it already has.
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

    /// **A program the PATH does not hold is blocked here, not at the end of a job** — roadmap task
    /// **T78b**, its design's D1: the step names the program, and nothing else in the plan changes.
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
    /// `composer create-project .` stops at the first entry a directory holds, and before this the
    /// plan said nothing about it — so an apply registered the project, installed the runtimes,
    /// made the database, the site, the domain and the certificate, and *then* the last step said
    /// the directory was not empty. D10's whole subject.
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

    /// A manifest whose scaffold is an archive — roadmap task **T205**, D2.
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

    /// **The `.env` line is asked for, and after the scaffold** — roadmap task **T205a**. After,
    /// because `create-project .` refuses a folder that already holds a `.env`.
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

    /// **A starter steps aside for code already there** — roadmap task **T205**. A folder somebody
    /// cloned keeps its own `index.html`; the archive is neither unpacked over it nor a reason to
    /// stop the apply.
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

    async fn planned_for(
        store: &Store,
        temp: &tempfile::TempDir,
        root: &Path,
        manifest: crate::blueprints::manifest::BlueprintManifest,
    ) -> BlueprintPlan {
        plan(
            store,
            &Catalogue::builtin(),
            &Wanted {
                blueprint: "wordpress",
                filed: &captured(manifest),
                project: "wp",
                root,
                answers: &[],
                scaffold_path: &a_path_holding(temp, &[]),
                front_end: false,
            },
        )
        .await
        .expect("a plan")
    }

    /// **The scaffold's gate, with the URL where the command would be** — T205, D2.
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

    /// **The row that decides the rule** — roadmap task **T120c**. `next_js_1` is not a slug —
    /// [`crate::domains::slug`] turns the underscore into a hyphen — and npm accepts it, installs
    /// into it and writes `"name": "next_js_1"`. Measured on 2026-09-13 against
    /// `npx --yes create-next-app@latest . --yes`. A check built on `slug` would refuse a directory
    /// that works, which is the mistake this test exists to keep out.
    #[tokio::test]
    async fn an_underscored_directory_is_not_refused() {
        let (temp, store) = home().await;
        let root = temp.path().join("next_js_1");
        std::fs::create_dir_all(&root).expect("a directory");

        let planned =
            scaffolding_at(&temp, &store, naming_itself_after_its_directory(), &root).await;

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

        let planned =
            scaffolding_at(&temp, &store, naming_itself_after_its_directory(), &root).await;

        let scaffold = step_of(&planned, |action| {
            matches!(action, PlanAction::RunScaffold { .. })
        });

        let Disposition::Blocked { reason } = &scaffold.disposition else {
            panic!("{scaffold:?}");
        };

        assert!(reason.contains("Next.js 1"), "{reason}");
        // **The answer, not the rule** — the design's D7.
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

    /// **A blueprint that did not declare it is never judged by it** — the whole of the design's
    /// D3. Three of the gallery's four scaffolds take their package name from an argument and do
    /// not care what the folder is called.
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
    /// is the one a false `blocked` here would cost (T78b, D2).
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
        manifest.sites.first_mut().expect("a site").domain_pattern =
            "{project}.example.com".to_owned();

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
}
