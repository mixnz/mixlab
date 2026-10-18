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

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mixengine_proto::{
    BlueprintPlan, Disposition, PackageVersion, PlanAction, PlanStep, RuntimeKind, ServiceId,
    SiteKind, VersionAnswer, VersionConstraint,
};

use crate::blueprints::manifest::PER_PROJECT;
use crate::blueprints::store::Filed;
use crate::generate::{Catalogue, recipe::Instancing};
use crate::{Result, Store, projects, runtimes, services};

mod steps;

use steps::{
    Made, answered_runtime, database_step, domain_step, ensure, extension, identity, instance_of,
    made, package, register, runtime,
};

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

        // **What the runtime steps above will have installed by the time this runs** (T78b, as
        // T185b left it): every `[runtimes]` kind, since each of their steps ends with that kind
        // on the disk whatever its disposition, and every kind already there.
        let mut available: BTreeSet<RuntimeKind> = manifest.runtimes.keys().copied().collect();
        available.extend(
            runtimes::records(store, None)
                .await?
                .into_iter()
                .map(|record| record.kind),
        );

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
                None => match crate::blueprints::program::disposition(
                    &command,
                    scaffold_path,
                    &available,
                ) {
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
mod tests;
