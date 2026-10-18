//! The steps [`plan`] is made of: the project, its runtimes, packages, databases, domains and
//! extensions, each planned as an install, a reuse or a refusal.
//!
//! [`plan`]: super::plan

use std::collections::BTreeMap;
use std::path::Path;

use mixengine_proto::{
    AnswerSubject, Disposition, MismatchAnswer, PackageVersion, PlanAction, PlanStep, RuntimeKind,
    ServiceId, VersionAnswer, VersionConstraint,
};

use crate::blueprints::manifest::PER_PROJECT;
use crate::{Result, Store, projects, runtimes, services, sites};

use super::{blocked, satisfied, unexpanded};

/// The project itself: its name, the versions it will ask for, and whether this directory is free.
///
/// Answers with the rowid of the project this apply is *about* where there already is one, which is
/// what [`domain_step`] needs to tell a name this apply already claimed from a name somebody else
/// holds.
pub(super) async fn register(
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
pub(super) enum Made {
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
pub(super) async fn made(
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
pub(super) fn answered_runtime(
    answers: &[VersionAnswer],
    kind: RuntimeKind,
) -> Option<MismatchAnswer> {
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
pub(super) async fn runtime(
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
pub(super) async fn instance_of(
    store: &Store,
    name: &str,
    instance: Option<&str>,
    project: &str,
) -> String {
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
pub(super) fn identity(package: &str, instance: &str) -> Option<ServiceId> {
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
pub(super) async fn package(
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
pub(super) async fn ensure(
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
pub(super) fn database_step(package: &str, instance: &str, database: &str, user: &str) -> PlanStep {
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
pub(super) async fn domain_step(
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
pub(super) async fn extension(
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
