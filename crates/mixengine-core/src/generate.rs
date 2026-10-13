//! Turning declared state into configuration on disk, and into something the supervisor can run.
//!
//! Roadmap task **T30**. Two halves of one job, which is why they are one module:
//!
//! - **Config generation.** Every managed service's configuration is rendered from a template plus
//!   the user's overrides into `etc/<service-id>/`, atomically, skipping what has not changed and
//!   installing nothing that fails validation. That is [`document`] and [`settings`].
//! - **The `ServiceSpec` in front of it.** A `services` row is not a spec — it carries
//!   `package_id`, `port`, `data_dir`, `config_overrides_json` and `limits_json` — and what turns
//!   one into a runnable specification is the same knowledge that renders its config: a [`recipe`].
//!
//! So this module is also the answer to the port `mixengine-daemon` has been asking through since
//! T19 (`SpecSource`), and the thing that answers it is [`Generator`]. Before this, the daemon's
//! shipped source was `Undeclared` — an empty set, honest for a build that could not render a spec
//! at all.
//!
//! # What is generated is never read back
//!
//! `CLAUDE.md`'s rule, and this module is where it is kept: nothing here parses a file under `etc/`
//! into state. The only read is [`document::install`] comparing a rendering against what is on disk
//! to decide whether to write it, which is a *checksum* rather than a parse — it can tell you the
//! file is the one we would write, and it can tell you nothing about a file that is not.
//!
//! # Rendered on every walk
//!
//! [`Generator::declared`] renders and installs, and the registry asks it at the top of every
//! `service.*` call. That sounds like a lot of writing and is almost none: the diff in
//! [`document::install`] means a home whose state has not changed does no I/O beyond reading each
//! file once. The alternative — rendering only when something is started — leaves the two able to
//! disagree, and the way that is discovered is a service reloading a config from before the change
//! the user just made.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use mixengine_platform::PortBinding;
use mixengine_proto::{FrontEndServer, IdlePolicy, Millis, ResourceLimits, ServiceId, ServiceSpec};

pub mod ca;
pub mod databases;
pub mod document;
pub mod first_run;
pub mod recipe;
pub mod recipes;
pub mod served;
pub mod settings;
pub mod step;
pub mod welcome;

pub use databases::{Ask, Credentials, DatabaseAdmin, Found, Provisioning};
pub use document::{Document, Reason, Validator, Written};
pub use first_run::{DataDirectory, FirstRun, Ritual, SecretSpec};
pub use recipe::{
    Catalogue, Context, Endpoints, FrontEndAddition, Instancing, Recipe, Role, Source,
    TemplateFile, Upstream, Upstreams, program,
};
pub use recipes::{Caddy, Mariadb, PhpFpm, Postgres};
pub use served::{Served, ServedKind, Shared};
pub use settings::{Preset, Setting, Settings, Value};
pub use step::{SecretFile, Step};

use crate::{Error, Paths, Result, Store};

/// The `services` table, rendered.
///
/// Holds the home's layout, its database and the recipes this build can find — everything needed to
/// answer "what does this home declare" with no argument at all, which is what the daemon's
/// `SpecSource` is asked.
#[derive(Debug, Clone)]
pub struct Generator {
    /// Where `etc/`, `data/`, `run/` and `logs/` are.
    paths: Paths,

    /// The declared state.
    store: Store,

    /// What this build knows how to run.
    catalogue: Catalogue,

    /// What this system makes a program bind to answer on the ports a front end serves.
    ///
    /// Asked once, when the generator is built, because the mapping is a constant of the operating
    /// system — `PortAccess::bindings` is pure for exactly this reason.
    bindings: Vec<PortBinding>,

    /// Whether a site with nothing behind it answers with MixEngine's page — roadmap task **T124**.
    ///
    /// `config.sites.welcome_page`, carried for [`bindings`](Self::bindings)' reason: `config.toml`
    /// is read once at boot, and a generator that re-read it per render would be a second place this
    /// home's preferences are decided.
    welcome: bool,
}

/// One service, generated: what it will run, and what changed on the way.
#[derive(Debug, Clone)]
pub struct Generated {
    /// What the supervisor is to run.
    pub spec: ServiceSpec,

    /// Every file the recipe renders, and what installing it did.
    ///
    /// What a reload decision is made of, which is the reason it is reported at all — see
    /// [`Written::changed`].
    pub files: Vec<(PathBuf, Written)>,

    /// Files a swept directory carried that no document of this service's owns any more.
    ///
    /// Counted by [`Generated::changed`], and that is the whole reason it is reported: a walk whose
    /// only difference is a deleted site has to reach the reload, or the front end goes on serving
    /// a site nothing declares.
    pub removed: Vec<PathBuf>,

    /// What has to happen once before this service is ever started, if anything.
    ///
    /// Computed here because this is the only place both halves are in hand — the recipe, and a
    /// [`Context`] built from the row. Assembling it costs a clone of that context and nothing else;
    /// it is *performed* by the daemon, once, and only when the markers say it has not been.
    pub first_run: Option<FirstRun>,

    /// How this service's databases are administered, if it has any — roadmap task **T77a**.
    ///
    /// Computed here for [`Generated::first_run`]'s reason exactly: this is the only place both
    /// halves are in hand. It is *used* by the daemon, which holds the credentials it needs.
    pub provisioning: Option<Provisioning>,
}

impl Generated {
    /// Whether anything on disk is different from what it was.
    #[must_use]
    pub fn changed(&self) -> bool {
        !self.removed.is_empty() || self.files.iter().any(|(_, written)| written.changed())
    }
}

/// One row, resolved as far as it can be before anything else's address is known.
///
/// The generator's first pass builds one of these per row. It exists because of one dependency: a
/// site's `fastcgi_pass` names where its pool listens, and the pool's own recipe is the only thing
/// that knows — so every context has to exist before the first file is rendered.
#[derive(Debug)]
struct Prepared {
    recipe: Arc<dyn Recipe>,
    context: Context,
    limits: ResourceLimits,
    /// How long this service may look idle before it is stopped, or [`None`] for never.
    ///
    /// Already resolved against the recipe's default and the column's three states — see
    /// [`Generator::prepare`]. Kept as a duration rather than as a whole `IdlePolicy` because the
    /// other half of one is the probe, and a probe may name an endpoint that is not computed until
    /// after this struct is built.
    idle_after: Option<Millis>,
}

/// One `services` row joined to **both** of the tables a parent could be in.
///
/// A struct rather than the query's own anonymous record, because two call sites read it and
/// `sqlx::query!` gives each of them a different type.
///
/// Seven nullable columns rather than three, because the join that did not match contributes nulls
/// and the `CHECK` on `services` guarantees exactly one of the two groups is whole. Resolving that
/// into one answer is [`Parent::of`], which is also where a row that matched neither is refused.
#[derive(Debug)]
struct Row {
    id: String,
    instance_name: String,
    port: Option<i64>,
    bind_addr: String,
    data_dir: Option<String>,
    overrides: String,
    limits: String,
    /// `NULL` is "use the recipe's default", `0` is "never", and `n` is minutes.
    idle_minutes: Option<i64>,
    /// The port the activator listens on, for a service that listens on TCP and can be started by a
    /// connection. `NULL` for every other row, which is most of them — T70.
    activation_port: Option<i64>,
    package: Option<String>,
    package_version: Option<String>,
    package_path: Option<String>,
    package_provides: Option<String>,
    runtime: Option<String>,
    runtime_version: Option<String>,
    runtime_path: Option<String>,
    runtime_provides: Option<String>,
    /// The third parent — roadmap task **T81**. Only the id: everything else an extension's row
    /// says is in `extensions`, read once per walk rather than joined per service.
    extension: Option<String>,
}

/// One service, and what would change on disk if it were rendered now.
///
/// The answer [`Generator::drift`] gives per row. A service whose [`drift`](Self::drift) is empty is
/// one whose installed configuration is already what its row renders to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDrift {
    /// Which service.
    pub service: ServiceId,

    /// What would change.
    pub drift: document::Drift,
}

/// Which install supplies the binary behind one service, resolved from the two halves of a [`Row`].
///
/// `Parent` and not `Origin`: the public [`services::Origin`](crate::services::Origin) is what a
/// caller *asks* for and this is what a row *has*, and `recipe.rs` already has a private `Origin` of
/// its own for the `package` half of a rendering.
#[derive(Debug)]
struct Parent {
    /// The name the recipe is found under, which is also what `data/<package>` is named after.
    package: String,

    /// The installed version, as upstream writes it.
    version: String,

    /// Where that install is unpacked.
    install_path: String,

    /// What it calls its executables, and where each one is inside the directory.
    provides: BTreeMap<String, String>,
}

impl Parent {
    /// The parent an installed extension is — roadmap task **T81**.
    ///
    /// **No `provides` map**, and that is not an omission: `provides` exists because a package's
    /// archive keeps its publisher's layout and a recipe has to be told where the binary landed. An
    /// extension's manifest names its own program directly, so there is nothing here to look up.
    fn of_extension(installed: &crate::extensions::store::Installed) -> Self {
        Self {
            package: installed.id.as_str().to_owned(),
            version: installed.version().as_str().to_owned(),
            install_path: installed.install_dir.display().to_string(),
            provides: BTreeMap::new(),
        }
    }

    /// Read a row's parent, whichever of the two it has.
    ///
    /// **The recipe's name for a runtime is the id's own half**, and that is the one asymmetry worth
    /// stating: a `packages` row names itself `caddy` and the service is `caddy`, while a
    /// `runtime_installs` row names itself `php` and the service is `php-fpm@8.3.33`. What finds a
    /// recipe is `ServiceId::name()` either way — the rule `recipe.rs` already states — so a pool
    /// takes its name from the id and the runtime's kind stops here.
    ///
    /// **A package publishes a `provides` map now, and a row written without one carries
    /// an empty one** — which is honest rather than a placeholder. A Caddy installed before that
    /// column existed is served by [`Context::program`], which asks this map nothing. What an empty
    /// map costs is a recipe that does ask — MariaDB, whose seven commands are not one binary at the
    /// install root — and the answer it gets names the reinstall that would fill it in. See
    /// [`Context::provided`].
    fn of(row: &mut Row, service: &ServiceId) -> Result<Self> {
        let unreadable = |value: &str| Error::UnreadableServiceRow {
            service: service.as_str().to_owned(),
            column: "package_id",
            value: value.to_owned(),
        };

        match (
            row.package.take(),
            row.package_version.take(),
            row.package_path.take(),
            row.package_provides.take(),
        ) {
            (Some(package), Some(version), Some(install_path), Some(provides)) => {
                let provides = serde_json::from_str(&provides).map_err(|source| {
                    Error::UnreadableServiceDocument {
                        service: service.as_str().to_owned(),
                        column: "provides_json",
                        source,
                    }
                })?;

                return Ok(Self {
                    package,
                    version,
                    install_path,
                    provides,
                });
            }
            (None, None, None, None) => {}
            _ => return Err(unreadable("a packages row that is only half there")),
        }

        match (
            row.runtime.take(),
            row.runtime_version.take(),
            row.runtime_path.take(),
            row.runtime_provides.take(),
        ) {
            (Some(_kind), Some(version), Some(install_path), Some(provides)) => {
                let provides = serde_json::from_str(&provides).map_err(|source| {
                    Error::UnreadableServiceDocument {
                        service: service.as_str().to_owned(),
                        column: "provides_json",
                        source,
                    }
                })?;

                Ok(Self {
                    package: service.name().to_owned(),
                    version,
                    install_path,
                    provides,
                })
            }

            // The `CHECK` on `services` makes this unreachable through the database's own rules, so
            // reaching it means a row somebody wrote by hand or a runtime removed out from under
            // one. Named rather than defaulted, because a service silently rendered against no
            // install is a service that fails much later and somewhere else.
            _ => Err(unreadable(
                "none of a package, a runtime install or an extension",
            )),
        }
    }
}

impl Generator {
    /// A generator for this home.
    ///
    /// `bindings` is what the platform layer says this system makes a program bind to answer on 80
    /// and 443 — [`PortAccess::bindings`](mixengine_platform::PortAccess::bindings). It is a value
    /// rather than a call because `mixengine-core` may not ask what system it is on, and because a
    /// generator is built once and renders on every walk.
    #[must_use]
    pub fn new(
        paths: Paths,
        store: Store,
        catalogue: Catalogue,
        bindings: Vec<PortBinding>,
        welcome: bool,
    ) -> Self {
        Self {
            paths,
            store,
            catalogue,
            bindings,
            welcome,
        }
    }

    /// What this home declares, prepared but not yet written anywhere.
    ///
    /// **The step both [`declared`](Self::declared) and [`drift`](Self::drift) start from**, and the
    /// reason it is one function: the two ask the same question of the same rows and differ only in
    /// what they do with the answer. Two walks would be two definitions of "what this home
    /// declares", and the front end — whose configuration is a function of every site and of where
    /// every pool listens — is where they would come apart.
    ///
    /// **All or nothing.** One row that cannot be prepared fails the call rather than being left out
    /// of the answer, because a service that silently disappears from `mix service list` is a service
    /// somebody goes looking for in the wrong place. What they get instead is a message naming the
    /// row and what is wrong with it.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the rows cannot be read; and per row, whatever
    /// [`generate`](Self::generate) reports.
    async fn declarations(&self) -> Result<(Vec<Prepared>, Vec<Served>)> {
        self.declarations_with(None).await
    }

    /// [`declarations`](Self::declarations), with one extension that is not installed yet counted as
    /// though it were — roadmap task **T81c**.
    ///
    /// **The only caller is [`would_serve`](Self::would_serve)**, and the parameter exists because
    /// the question it asks cannot be asked any other way: *would this home's front end accept the
    /// configuration it is about to be given?* is a question about a set the table does not hold.
    /// Writing the row first and rolling it back would put the answer after the change it is
    /// supposed to prevent.
    ///
    /// `pending` contributes fragments and nothing else. It has no `services` row, so it runs
    /// nothing and is served nothing; the row it will get is written later, by the install this
    /// judgement is standing in front of.
    async fn declarations_with(
        &self,
        pending: Option<&crate::extensions::store::Installed>,
    ) -> Result<(Vec<Prepared>, Vec<Served>)> {
        let rows = sqlx::query_as!(
            Row,
            r#"SELECT s.id                    AS "id!: String",
                      s.instance_name         AS "instance_name!: String",
                      s.port                  AS "port: i64",
                      s.bind_addr             AS "bind_addr!: String",
                      s.data_dir              AS "data_dir: String",
                      s.config_overrides_json AS "overrides!: String",
                      s.limits_json           AS "limits!: String",
                      s.idle_minutes          AS "idle_minutes: i64",
                      s.activation_port       AS "activation_port: i64",
                      p.name                  AS "package: String",
                      p.version               AS "package_version: String",
                      p.install_path          AS "package_path: String",
                      p.provides_json         AS "package_provides: String",
                      r.kind                  AS "runtime: String",
                      r.version               AS "runtime_version: String",
                      r.install_path          AS "runtime_path: String",
                      r.provides_json         AS "runtime_provides: String",
                      s.extension_id          AS "extension: String"
               FROM services s
               LEFT JOIN packages p         ON p.id = s.package_id
               LEFT JOIN runtime_installs r ON r.id = s.runtime_install_id
               ORDER BY s.id"#
        )
        .fetch_all(self.store.pool())
        .await
        .map_err(|source| self.store.failure("read", source))?;

        // **Two passes, and the first one writes nothing.** A site's configuration names the address
        // its pool listens on, and only the pool's own recipe knows what that is — so every context
        // has to be built before any file is rendered. The pass costs a `Context` per row out of a
        // row already fetched.
        let mut prepared = Vec::with_capacity(rows.len());

        // **Read once for the whole walk** — roadmap task T81. An extension's recipe is built out
        // of its row, and a join per service would fetch the same manifests again for every one of
        // them. A home has a handful of extensions and none at all is the common case, so this is
        // one query answering nothing most of the time.
        let mut installed: BTreeMap<String, crate::extensions::store::Installed> =
            crate::extensions::store::all(&self.store)
                .await?
                .into_iter()
                .map(|one| (one.id.as_str().to_owned(), one))
                .collect();

        if let Some(pending) = pending {
            installed.insert(pending.id.as_str().to_owned(), pending.clone());
        }

        // **Rendered once for the whole walk, beside the manifests it comes out of** — roadmap task
        // **T81c**. A fragment's placeholders are the extension's, so the substitution belongs where
        // the extension's row is; what a front-end recipe is handed is text.
        let fragments = Self::fragments(installed.values())?;

        // **Which pools carry a credential, read once for the whole walk** — roadmap task **T82a**,
        // that design's D4. Beside the manifests and the fragments for their reason: answering it
        // reads the extension sites, their frozen database links and each database's recipe, and a
        // join per service would ask those questions once per row.
        let credentials = crate::extensions::pools::credentials(&self.store).await?;

        // **Which home this is, read once for the whole walk** — roadmap task **T126**, and beside
        // the two above for their reason. Every spec that carries an `EnvValue::Keyring` names an
        // address this is the first segment of, so a walk that asked per row would ask the same
        // question of the same table once per service for ever.
        let home = crate::home::id(&self.store).await?;

        // **Whether the home saves resources, read once for the whole walk** — roadmap task
        // **T167b**. It decides the idle policy of every row nobody set, so it is one answer for
        // the walk rather than one read per row.
        let saving = crate::services::save_resources::get(&self.store).await?;

        for row in rows {
            prepared.push(self.prepare(
                row,
                &home,
                &installed,
                &fragments,
                &credentials,
                saving,
            )?);
        }

        let mut upstreams = BTreeMap::new();

        for one in &prepared {
            if let Some(listen) = one.recipe.upstream(&one.context)? {
                // Asked only of a service something points at, so a recipe with no upstream is never
                // asked for an activator either — T70. The pair is inserted together, which is what
                // makes it impossible for a site to name one service's pool and another's activator.
                let activator = one.recipe.activator(&one.context)?;

                upstreams.insert(one.context.service.clone(), Upstreams { listen, activator });
            }
        }

        let served =
            served::served(&self.store, &upstreams, self.paths.certs(), &installed).await?;

        Ok((prepared, served))
    }

    /// Every service this home declares, configured and ready to run.
    ///
    /// **The whole set**, because the caller's next move is a
    /// [`ServiceGraph`](crate::services::ServiceGraph): dependencies, cycles and start order are
    /// properties of a set.
    ///
    /// **[`Generated`] and not [`ServiceSpec`]**, because the specification is only half of what a
    /// walk needs to know. The other half is whether anything on disk moved — [`Generated::changed`]
    /// — which is what turns "the configuration is up to date" into "the process reading it has been
    /// told", and it is knowledge only this call has: by the time a spec is in a caller's hands the
    /// file has already been written and a second look at it would compare a rendering with itself.
    ///
    /// [`drift`](Self::drift) is the read-only half, and answers what this would change.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the rows cannot be read; per row, whatever
    /// [`generate`](Self::generate) reports; and per row, whatever installing it reports.
    pub async fn declared(&self) -> Result<Vec<Generated>> {
        let (prepared, served) = self.declarations().await?;
        let mut generated = Vec::with_capacity(prepared.len());

        for one in prepared {
            generated.push(self.install(one, &served).await?);
        }

        Ok(generated)
    }

    /// One service's settings, merged, without rendering or installing anything.
    ///
    /// **Read-only, and that is the whole reason it exists** — roadmap task **T53**.
    /// [`generate`](Self::generate) goes through [`declared`](Self::declared), which installs; a
    /// caller that only wants a number would rewrite this home's configuration to get it, and can
    /// reload a running server as a side effect of asking a question. This takes
    /// [`drift`](Self::drift)'s door for the reason stated there — rendering is pure, and nothing
    /// before it touches a disk.
    ///
    /// **The same merge and not a second one.** What comes back is what a rendering of this service
    /// would be made with, so a caller and a template cannot come to disagree about a value. The
    /// first caller is `mix cert status`, which connects to the front end's `https_port`; the only
    /// other way to learn that number is to read it out of the generated configuration, and
    /// `CLAUDE.md` forbids parsing a generated file back into state.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the rows cannot be read, whatever merging the overrides reports,
    /// and [`Error::NotFound`] when nothing in this home declares this service.
    pub async fn settings(&self, service: &ServiceId) -> Result<Settings> {
        let (prepared, _served) = self.declarations().await?;

        prepared
            .into_iter()
            .find(|one| one.context.service() == service)
            .map(|one| one.context.settings().clone())
            .ok_or_else(|| Error::NotFound {
                kind: "service",
                id: service.as_str().to_owned(),
            })
    }

    /// What [`declared`](Self::declared) would change on disk, asked without changing it.
    ///
    /// Roadmap task **T47b**, and the read `mix doctor`'s tenth check makes. Rendering is pure — it
    /// builds [`Document`]s in memory — and the comparison is [`document::drift`], so nothing is
    /// staged, nothing is validated and no directory is created. That is what lets a check whose
    /// whole guarantee is "this writes nothing" ask the question at all.
    ///
    /// **A home with no rows drifts in no way**, and answers an empty list rather than an error.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the rows cannot be read; per row, whatever
    /// [`generate`](Self::generate) reports; and [`Error::Io`] when a generated directory cannot
    /// be read.
    pub async fn drift(&self) -> Result<Vec<ServiceDrift>> {
        let (prepared, served) = self.declarations().await?;
        let mut drifts = Vec::with_capacity(prepared.len());

        for one in &prepared {
            drifts.push(ServiceDrift {
                service: one.context.service.clone(),
                drift: document::drift(
                    &one.context.etc,
                    &Self::documents(one, &served)?,
                    one.recipe.swept(),
                )
                .await?,
            });
        }

        Ok(drifts)
    }

    /// Would this home's front end accept the configuration installing `pending` would give it? —
    /// roadmap task **T81c**, the design's D5.
    ///
    /// Renders the whole home with `pending`'s `[[recipe.front_end]]` fragments in it and shows the
    /// front end's own binary the result, through
    /// [`document::judge`] — which stages, runs the checker and installs nothing.
    /// A fragment the server will not parse is therefore refused **before** anything is downloaded,
    /// unpacked or written, and the home is byte-identical afterwards either way.
    ///
    /// **What is judged is the fragment with the ports the manifest asked for**, not the ports it
    /// will hold: allocation happens when the rows are written, which is after this. A port number
    /// is a token in both configuration languages and cannot change whether one parses, so the
    /// judgement carries over — what does not carry over is the claim that the server saw the exact
    /// bytes that will be installed. It saw the exact *shape*.
    ///
    /// **A home with no front end judges nothing and passes.** There is no binary to ask. The
    /// fragment is judged the first time a front end renders, which is when one is installed.
    ///
    /// # Errors
    ///
    /// [`Error::ConfigRejected`] carrying what the front end said, and whatever rendering this
    /// home's declarations costs.
    pub async fn would_serve(&self, pending: &crate::extensions::store::Installed) -> Result<()> {
        // An extension that adds nothing to the front end changes nothing about its configuration,
        // and judging it would put a process launch into every `extension.install` — including
        // every one of the kinds that could not carry a fragment if they wanted to.
        if pending
            .manifest
            .recipe
            .as_ref()
            .is_none_or(|recipe| recipe.front_end.is_empty())
        {
            return Ok(());
        }

        let (prepared, served) = self.declarations_with(Some(pending)).await?;

        for one in &prepared {
            if !matches!(one.recipe.role(), Role::FrontEnd(_)) {
                continue;
            }

            document::judge(
                &one.context.etc,
                &Self::documents(one, &served)?,
                one.recipe.validator(&one.context).as_ref(),
            )
            .await?;
        }

        Ok(())
    }

    /// Every service something can start by connecting to it, and the pair of addresses that takes
    /// — roadmap task **T70**.
    ///
    /// The activator's address first and the service's own second, which is the order they are used
    /// in: the daemon holds the first and dials the second once the service is up. A service whose
    /// recipe has no activator is not in the map at all.
    ///
    /// **Computed from the same rows and the same contexts a render uses**, so the address the
    /// daemon binds and the address a site file names cannot disagree — which is the failure this
    /// method exists to make impossible, because what it looks like is a site that 502s and an
    /// activator sitting on an address nothing dials.
    ///
    /// # Errors
    ///
    /// Whatever preparing a row costs, and whatever computing either address costs — a home too
    /// deeply nested for the derived socket path, most of all.
    pub async fn activators(&self) -> Result<BTreeMap<ServiceId, (Upstream, Upstream)>> {
        // Through `declarations` rather than a query of its own: the addresses have to be the ones
        // a render would compute, and a second row query here would be a second chance to compute
        // them from something slightly different.
        let (prepared, _served) = self.declarations().await?;

        let mut activators = BTreeMap::new();

        for one in prepared {
            let Some(listen) = one.recipe.upstream(&one.context)? else {
                continue;
            };

            if let Some(activator) = one.recipe.activator(&one.context)? {
                activators.insert(one.context.service.clone(), (activator, listen));
            }
        }

        Ok(activators)
    }

    /// Every address in this home that a connection may start a *stopped* service at — **T70a**.
    ///
    /// **Through `declarations` for [`activators`](Self::activators)' reason**: the addresses have
    /// to be the ones a render computed, and a second query here would be a second chance to
    /// compute them from something slightly different — which is exactly what would happen to a
    /// socket path, whose value is a function of how deep this home is.
    ///
    /// **A service with nothing to hold is absent rather than present and empty.** The caller
    /// iterates this map, and an empty entry is a service it would log about having done nothing
    /// for.
    ///
    /// # Errors
    ///
    /// Whatever preparing every service's context costs, and whatever a recipe reports about an
    /// address it cannot compute.
    pub async fn held_while_stopped(&self) -> Result<BTreeMap<ServiceId, Vec<Upstream>>> {
        let (prepared, _served) = self.declarations().await?;

        let mut held = BTreeMap::new();

        for one in prepared {
            let addresses = one.recipe.held_while_stopped(&one.context)?;

            if !addresses.is_empty() {
                held.insert(one.context.service.clone(), addresses);
            }
        }

        Ok(held)
    }

    /// Every file this service has, rendered.
    ///
    /// **One definition for both callers.** [`install`](Self::install) writes these and
    /// [`drift`](Self::drift) compares them; a second assembly of the set would be a second answer
    /// to "what does this service render to", and the front end's site files are exactly where two
    /// answers would diverge.
    fn documents(prepared: &Prepared, served: &[Served]) -> Result<Vec<Document>> {
        let mut documents = recipe::render(prepared.recipe.as_ref(), &prepared.context)?;

        // **Appended to the recipe's own set, not installed by a path of its own** — T43's D1. The
        // checker judges a staging directory, so a site file written anywhere else would be
        // invisible to `caddy validate` and present at run time.
        if matches!(prepared.recipe.role(), Role::FrontEnd(_)) {
            documents.extend(prepared.recipe.sites(&prepared.context, served)?);

            // **And what this home's extensions added** — roadmap task **T81c**. Under the same
            // condition and appended to the same set, for T43's D1 exactly: a fragment installed by
            // a path of its own would be invisible to the checker and present at run time.
            documents.extend(prepared.recipe.fragments(&prepared.context)?);
        }

        Ok(documents)
    }

    /// Every installed extension's `[[recipe.front_end]]`, rendered — roadmap task **T81c**.
    ///
    /// **Here rather than in a recipe**, because the placeholders in a fragment are the
    /// *extension's*: `{install_dir}` means the directory that extension was installed into, and
    /// only its row knows where that is. The server each one names travels beside it so that
    /// [`prepare`](Self::prepare) can give a front end its own and no others.
    ///
    /// # Errors
    ///
    /// [`Error::ExtensionField`] for a fragment whose placeholders cannot be rendered, naming
    /// `recipe.front_end[<n>]` — a manifest that was installed and can no longer be applied is
    /// reported rather than skipped, which is the rule `php_ini` already follows.
    fn fragments<'a>(
        installed: impl Iterator<Item = &'a crate::extensions::store::Installed>,
    ) -> Result<Vec<(FrontEndServer, FrontEndAddition)>> {
        let mut fragments = Vec::new();

        for one in installed {
            let Some(recipe) = &one.manifest.recipe else {
                continue;
            };

            let context = crate::extensions::render::Context::installed(one);

            for (index, entry) in recipe.front_end.iter().enumerate() {
                let field = format!("recipe.front_end[{index}]");

                fragments.push((
                    entry.server,
                    FrontEndAddition {
                        extension: one.id.as_str().to_owned(),
                        fragment: crate::extensions::render::fragment(
                            &one.id,
                            &field,
                            &entry.fragment,
                            &context,
                            entry.server.into(),
                        )?,
                    },
                ));
            }
        }

        Ok(fragments)
    }

    /// Generate one service's configuration and specification.
    ///
    /// **The whole home is rendered and one answer is picked out**, which is not a shortcut: a front
    /// end's configuration is a function of every site in the database and of where every pool
    /// listens, so "render this one service" is not a smaller job than rendering all of them. It was
    /// one before T43, and pretending it still is would mean a second, subtly different assembly of
    /// the same map.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when there is no such service, and everything
    /// [`declared`](Self::declared) can report.
    pub async fn generate(&self, service: &ServiceId) -> Result<Generated> {
        self.declared()
            .await?
            .into_iter()
            .find(|one| one.spec.id() == service)
            .ok_or_else(|| Error::NotFound {
                kind: "service",
                id: service.as_str().to_owned(),
            })
    }

    /// One row, as far as it can be taken before anything else's address is known: look up the
    /// recipe, merge the overrides, build the context.
    ///
    /// Nothing here writes, which is what makes a first pass over every row affordable.
    fn prepare(
        &self,
        mut row: Row,
        home: &crate::home::HomeId,
        installed: &BTreeMap<String, crate::extensions::store::Installed>,
        fragments: &[(FrontEndServer, FrontEndAddition)],
        credentials: &BTreeMap<ServiceId, crate::extensions::pools::Credential>,
        saving: bool,
    ) -> Result<Prepared> {
        let service =
            ServiceId::parse(row.id.clone()).map_err(|source| Error::UnreadableServiceRow {
                service: row.id.clone(),
                column: "id",
                value: source.to_string(),
            })?;

        // **An extension brings its own recipe** — roadmap task **T81**, the design's D7. A recipe
        // is what *this build* knows and an extension is what a home installed, so it cannot come
        // out of the catalogue; what it comes out of is the manifest in its row, through
        // `ExtensionRecipe`. Everything below this point — ports, limits, the idle policy, the
        // activation port — is then the same code every other service goes through, which is the
        // whole reason an extension is a `services` row at all.
        let (parent, recipe): (Parent, Arc<dyn Recipe>) = match row.extension.take() {
            Some(id) => {
                let installed = installed
                    .get(&id)
                    .ok_or_else(|| Error::UnreadableServiceRow {
                        service: row.id.clone(),
                        column: "extension_id",
                        value: format!("{id}, which is not an installed extension"),
                    })?;

                (
                    Parent::of_extension(installed),
                    Arc::new(crate::extensions::recipe::ExtensionRecipe::new(
                        installed.clone(),
                    )),
                )
            }
            None => {
                let parent = Parent::of(&mut row, &service)?;
                let recipe = self
                    .catalogue
                    .recipe(&parent.package)
                    .ok_or_else(|| Error::NoRecipe {
                        service: row.id.clone(),
                        package: parent.package.clone(),
                        known: self.catalogue.packages().map(str::to_owned).collect(),
                    })?
                    .clone();

                (parent, recipe)
            }
        };

        let port = match row.port {
            None => None,
            Some(port) => Some(
                u16::try_from(port).map_err(|_| Error::UnreadableServiceRow {
                    service: row.id.clone(),
                    column: "port",
                    value: port.to_string(),
                })?,
            ),
        };

        // The activator's own port — roadmap task T70. Read exactly as `port` above is, and
        // separate from it for the reason the column is separate: it is allocated, not derived, so
        // there is nothing here to compute it from.
        let activation_port = match row.activation_port {
            None => None,
            Some(port) => Some(
                u16::try_from(port).map_err(|_| Error::UnreadableServiceRow {
                    service: row.id.clone(),
                    column: "activation_port",
                    value: port.to_string(),
                })?,
            ),
        };

        let settings = Settings::merge(recipe.settings(), &row.overrides, &service)?;

        let limits: ResourceLimits = serde_json::from_str(&row.limits).map_err(|source| {
            Error::UnreadableServiceDocument {
                service: row.id.clone(),
                column: "limits_json",
                source,
            }
        })?;

        // **The row's half of an idle policy, and its three states — roadmap task T69.** A policy
        // cannot be assembled here: the other half is the recipe's probe, and a probe may name an
        // endpoint, which is computed a few lines below this. What is decided here is only whether
        // there is a policy at all and how long it waits.
        let idle_after = match row.idle_minutes {
            // Nobody has said. The recipe's default, which is nothing for every recipe (ADR 0041),
            // unless the home is saving resources — then the recipe's saving number (T167b).
            None => recipe
                .idle_default()
                .or_else(|| saving.then(|| recipe.idle_when_saving()).flatten()),

            // Said, and said no. Outranks the recipe deliberately: a default arriving in a later
            // release must not switch idle-stopping back on behind the person who turned it off.
            Some(0) => None,

            Some(minutes) => {
                let minutes = u64::try_from(minutes).map_err(|_| Error::UnreadableServiceRow {
                    service: row.id.clone(),
                    column: "idle_minutes",
                    value: minutes.to_string(),
                })?;

                Some(Millis::from_secs(minutes.saturating_mul(60)))
            }
        };

        let mut context = Context {
            etc: self.paths.etc().join(service.as_str()),
            etc_root: self.paths.etc().to_path_buf(),

            // The row wins, and the fallback is the package's rather than `data/<service-id>`: two
            // instances of one server are `mariadb@main` and `mariadb@legacy`, and a directory named
            // after the *package* is what makes them siblings in a listing rather than two unrelated
            // names. How far down that goes is the recipe's answer, not this function's.
            data: row.data_dir.map_or_else(
                || match recipe.instancing() {
                    // A server that exists once has no instance half to spend, and `data/caddy/caddy`
                    // reads as a mistake to whoever finds it.
                    Instancing::Single => self.paths.data().join(&parent.package),
                    Instancing::Named => self
                        .paths
                        .data()
                        .join(&parent.package)
                        .join(&row.instance_name),
                },
                PathBuf::from,
            ),
            run: self.paths.run().to_path_buf(),
            logs: self.paths.service_logs(&service),
            package: parent.package,
            version: parent.version,
            install_path: PathBuf::from(parent.install_path),
            provides: parent.provides,
            port,
            activation_port,
            bind: row.bind_addr,
            settings,
            endpoints: recipe::Endpoints::default(),
            bindings: self.bindings.clone(),

            // **This home's authority, for a front end to hand to a phone** — roadmap task T75.
            // Read here rather than in a recipe, on the rule `bindings` follows: a recipe is a
            // function of its context, and a recipe that went looking at a disk would be a second
            // place this path is spelled. A home with no authority renders no copy of one.
            authority: std::fs::read_to_string(crate::certs::ca::certificate_path(
                self.paths.certs(),
            ))
            .ok(),

            // **Whether a site with nothing behind it answers with a page of ours** — roadmap task
            // T124, `config.sites.welcome_page`. A value on the generator for `bindings`' reason: a
            // recipe is a function of its context, and `config.toml` is read once at boot and not
            // once per render.
            welcome: self.welcome,

            // **What this home's extensions add to this front end** — roadmap task T81c. On the
            // front end's context and on nothing else, and already filtered to the fragments
            // written for *this* front end: a Caddyfile fragment is not something the nginx recipe
            // should have to know it must skip.
            fragments: match recipe.role() {
                Role::FrontEnd(server) => fragments
                    .iter()
                    .filter(|(written_for, _)| *written_for == server)
                    .map(|(_, addition)| addition.clone())
                    .collect(),
                Role::Other => Vec::new(),
            },
            secrets: BTreeMap::new(),

            // **What this service's processes are handed at spawn** — roadmap task **T82a**, that
            // design's D4. An address and never a value: `mixengine-core` may not reach a credential
            // store, so what a recipe puts on its spec is an `EnvValue::Keyring` and the supervisor
            // does the reading. [`None`] for every service but the pool of a `web-app` extension
            // that declared `signs_in`.
            credential: credentials.get(&service).cloned(),
            // **The certificate this service presents, when a usable pair is on disk** — roadmap
            // task T99. Read here rather than issued, on `authority`'s rule and on `drift`'s: a
            // drift check renders what the last install rendered, so it must see the pair that
            // install saw. Issuing is `install`'s, just before the render.
            certificate: Self::certificate_of(self.paths.certs(), &service),

            // **Which home a credential of this service's belongs to** — roadmap task **T126**. On
            // the context because `Context::secret_address` is where an address is composed, and
            // the whole of T126 is that the address has to say this.
            home: home.clone(),
            service,
        };

        // Asked once and stored, rather than recomputed by the template and again by the spec: the
        // whole point is that there is one answer. Before the render, because the template reads it.
        context.endpoints = recipe.endpoints(&context)?;

        Ok(Prepared {
            recipe,
            context,
            limits,
            idle_after,
        })
    }

    /// What `certs/services/` holds for this service, as the template and the spec name it —
    /// roadmap task **T99**. [`None`] unless a usable pair is there now.
    fn certificate_of(
        certs: &std::path::Path,
        service: &ServiceId,
    ) -> Option<recipe::ServiceCertificate> {
        match crate::certs::service::read(certs, service, std::time::SystemTime::now()) {
            mixengine_proto::CertState::Present { cert } => Some(recipe::ServiceCertificate {
                certificate: crate::certs::service::certificate_path(certs, service),
                key: crate::certs::service::key_path(certs, service),
                fingerprint: cert.fingerprint,
            }),
            _ => None,
        }
    }

    /// Give this service a certificate covering `names`, and say what is on disk afterwards —
    /// roadmap task **T99**.
    ///
    /// **Every failure is a warning and a [`None`]**, on the design's D4: a home with no authority,
    /// a machine that will not make a key, a directory that will not write — each leaves the server
    /// to generate its own certificate, slowly, and start. What is returned is read back off the
    /// disk rather than described from the write, so the fingerprint the header carries is the one
    /// a client will see.
    fn issue(
        certs: &std::path::Path,
        service: &ServiceId,
        names: &[String],
    ) -> Option<recipe::ServiceCertificate> {
        match crate::certs::service::ensure(certs, service, names, std::time::SystemTime::now()) {
            Ok((issued, mixengine_proto::CertState::Present { cert })) => {
                if issued == crate::certs::leaf::Issued::Written {
                    tracing::info!(
                        service = service.as_str(),
                        fingerprint = cert.fingerprint,
                        sans = ?cert.sans,
                        "a certificate was issued for this service"
                    );
                }
                Self::certificate_of(certs, service)
            }
            Ok((_, state)) => {
                tracing::warn!(
                    service = service.as_str(),
                    ?state,
                    "this service starts without a certificate of its own, and the server will \
                     generate one"
                );
                None
            }
            Err(error) => {
                tracing::warn!(
                    service = service.as_str(),
                    %error,
                    "this service starts without a certificate of its own, and the server will \
                     generate one"
                );
                None
            }
        }
    }

    /// A prepared row, all the way to a spec: render, add the sites if it is the front end, install,
    /// build.
    ///
    /// The order is forced and each step depends on the last. Installing before building the spec
    /// is the one that could be argued: a spec that will not build leaves a configuration on disk
    /// for a service that cannot start. That is the right way round — the config is what a person
    /// reads to work out *why* it will not start, and a spec that does not build is a bug in a
    /// recipe rather than a state anybody has to recover from.
    async fn install(&self, mut prepared: Prepared, served: &[Served]) -> Result<Generated> {
        // **A certificate for a recipe that asks for one, before anything is rendered** — roadmap
        // task **T99**, the design's D4. Here rather than in `prepare` because this is the step
        // that writes: the pair is a file the configuration names and the server will not make,
        // exactly what the `logs/` and data directories below are. And before `documents`, because
        // the template reads the fingerprint into its header. A failure here is a warning and a
        // context carrying `None` — never a start refused: the template then says nothing about
        // TLS and the server does what it did before this task.
        if let Some(names) = prepared.recipe.certificate(&prepared.context) {
            prepared.context.certificate =
                Self::issue(self.paths.certs(), prepared.context.service(), &names);
        }

        // Rendered before the row is taken apart, and through the helper `drift` uses, so the set
        // that gets installed is the set that gets compared. The role is what selects the recipe, on
        // T37's rule — a home has at most one front end, because `service.create` refuses a second.
        let documents = Self::documents(&prepared, served)?;

        let Prepared {
            recipe,
            context,
            limits,
            idle_after,
        } = prepared;

        // Before the render is judged, because a validator judges a *running* configuration and a
        // running configuration names places. php-fpm opens its `error_log` during `--test` and
        // fails the whole file when the directory is not there — and the service log directory is
        // otherwise created by the log sink, which is to say at the first start, which is after
        // this. The supervisor still creates it: this is the earlier of two idempotent calls, not a
        // move of the responsibility.
        crate::paths::create_dir(&context.logs)?;

        // **And the data directory, for the same reason one step out.** A server that names its own
        // data directory in its own configuration does not create it: Redis reads `dir` and refuses
        // the whole file with `FATAL CONFIG FILE ERROR … No such file or directory`, and a service
        // whose working directory is missing does not even reach its first line — memcached fails to
        // spawn. The two recipes that came before them hid this, because a first-run ritual creates
        // the directory it is about to bootstrap into, and Caddy makes its own storage.
        //
        // Creating it empty changes nothing for those rituals: `first_run::inspect` answers
        // [`DataDirectory::Empty`] for a directory that is missing *and* for one that is there with
        // nothing in it, and an empty datadir is the one thing Windows' `mariadb-install-db` accepts.
        //
        // [`DataDirectory::Empty`]: first_run::DataDirectory::Empty
        crate::paths::create_dir(&context.data)?;

        // And the scratch directory a recipe asked for, before the ritual that is the first thing
        // to need it: a `mysqld` pointed at a temporary directory that is not there refuses to
        // start, in bootstrap mode as in any other.
        if let Some(scratch) = context.scratch() {
            crate::paths::create_dir(scratch)?;
        }

        let installed = document::install(
            &context.etc,
            &documents,
            recipe.swept(),
            recipe.validator(&context).as_ref(),
        )
        .await?;

        let files = documents
            .iter()
            .map(|document| document.relative().to_path_buf())
            .zip(installed.written)
            .collect();

        let mut builder = recipe
            .spec(&context)?
            .limits(limits)
            // **Off the recipe and never off a row** — roadmap task T71a. Whether this program
            // survives a restart under memory pressure is a fact about the program, so it is carried
            // onto every spec the recipe renders rather than joined from anything per-home.
            .restart_over_memory(recipe.restart_over_memory_default());

        // **Both halves or neither.** A probe with no duration is a measurement nobody acts on, and
        // a duration with no probe is the wall clock an `IdlePolicy` exists to not be — so a recipe
        // that declares no probe is never idle-stopped however its row is set.
        if let (Some(after), Some(probe)) = (idle_after, recipe.idle_probe(&context)) {
            builder = builder.idle(IdlePolicy { after, probe });
        }

        let spec = builder.build().map_err(|source| Error::Unrunnable {
            service: context.service.as_str().to_owned(),
            source,
        })?;

        let first_run = recipe
            .ritual()
            .map(|ritual| FirstRun::new(&context, ritual));

        let provisioning = recipe
            .databases()
            .map(|admin| Provisioning::new(&context, admin));

        Ok(Generated {
            spec,
            files,
            removed: installed.removed,
            first_run,
            provisioning,
        })
    }
}

#[cfg(test)]
mod tests;
