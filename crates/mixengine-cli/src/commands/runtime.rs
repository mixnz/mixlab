//! `mix runtime`, its PHP extensions, and the install that asks about prerequisites first.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    Error, ErrorCode, ExtensionChange, ExtensionChoice, ExtensionList, JobSummary,
    PackageCatalogue, PackageFilter, PackageVersion, ResolvedRuntime, RuntimeCatalogue,
    RuntimeFilter, RuntimeInstall, RuntimeKind, RuntimeList, RuntimeQuestion, RuntimeRemoval,
    RuntimeSummary, RuntimeTarget, RuntimeUninstall, RuntimeUpgrade, RuntimeUpgradeQuery,
    UpgradePlan, VersionConstraint, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::parse::{runtime_kind, runtime_version, version_constraint};
use crate::prerequisites::{Agreement, prerequisites_agreed};
use crate::render;
use crate::upgrades::{finish_upgrade, upgrade_confirmed};

/// `mix runtime …` — one subcommand per `runtime.*` method, and nothing that is not one.
///
/// The two listings are two commands rather than one with a flag, because they answer two different
/// questions — what is here, and what could be — and the second one reaches the network while the
/// first reads a table. A `--available` on the first would hide that difference behind a flag.
#[derive(Debug, Subcommand)]
pub(crate) enum RuntimeCommand {
    /// List the runtimes installed in this home.
    List(Kind),

    /// List the versions the package index offers for this machine.
    Available {
        #[command(flatten)]
        filter: Kind,

        /// Ask the package index again even if the cached copy is still fresh.
        ///
        /// The daemon otherwise answers from a cache for up to six hours, which is the wrong
        /// default for someone who just watched a version get published and does not want to wait
        /// for their own machine to notice.
        #[arg(long)]
        refresh: bool,

        /// Print every release rather than one row per line.
        #[arg(long, conflicts_with = "line")]
        all: bool,

        /// Print every release of this line (`8.4`, `22`).
        #[arg(long, value_name = "LINE", requires = "kind")]
        line: Option<String>,
    },

    /// Download and install one version.
    Install {
        #[command(flatten)]
        runtime: Which,

        /// Return once the daemon has accepted the install, rather than once it has finished.
        ///
        /// `mix` waits by default, because `mix runtime install php 8.3.33 && …` is a sentence about
        /// PHP being there. What comes back instead is the job, which `mix job wait` can be pointed
        /// at later.
        #[arg(long)]
        no_wait: bool,

        /// Install what this version needs of the machine first, without asking: the Microsoft
        /// Visual C++ Redistributable, on Windows. Windows still asks for approval.
        #[arg(long)]
        yes: bool,

        /// Install even though MixEngine judges this machine lacks something the version needs.
        /// The version is still run once before it is kept.
        #[arg(long)]
        ignore_requirements: bool,
    },

    /// Remove one installed version.
    ///
    /// Refused while a registered project pins it, naming the projects, and while the php-fpm pool
    /// that runs out of it is running. `--force` crosses the first and never the second.
    Uninstall {
        #[command(flatten)]
        runtime: Which,

        /// Remove it even though a registered project pins it.
        #[arg(long)]
        force: bool,
    },

    /// Update one installed version to the newest release of its line, and move what uses it.
    ///
    /// Sites, the default and pins move to the new version; the old one is removed afterwards
    /// unless `--keep` is given or something still needs it. The plan is printed first.
    Upgrade {
        #[command(flatten)]
        runtime: Which,

        /// Update to this release of the same line instead of the newest.
        #[arg(long, value_name = "VERSION", value_parser = runtime_version)]
        to: Option<PackageVersion>,

        /// Keep the old version installed.
        #[arg(long)]
        keep: bool,

        /// Print the plan and change nothing.
        #[arg(long)]
        dry_run: bool,

        /// Do not ask: agree to the update and to what the new version needs of the machine.
        #[arg(long)]
        yes: bool,

        /// Update even though MixEngine judges this machine lacks something the new version needs.
        #[arg(long)]
        ignore_requirements: bool,

        /// Return once the daemon has accepted the update, rather than once it has finished.
        #[arg(long)]
        no_wait: bool,
    },

    /// Make one installed version the one its kind resolves to.
    Default {
        #[command(flatten)]
        runtime: Which,
    },

    /// Record a version that is on disk but not listed, such as one an earlier install left.
    ///
    /// Nothing is downloaded. The folder is checked against the package index first.
    Adopt {
        #[command(flatten)]
        runtime: Which,
    },

    /// List versions that are on disk but not listed, and why each is not.
    Found,

    /// Which extensions an installed PHP build loads.
    // Under `runtime` rather than as `mix php ext …`, which is what
    // `docs/features/runtime-versions.md` wrote: a per-language command family for one language is
    // a noun this CLI would then owe every other runtime.
    Ext {
        #[command(subcommand)]
        command: ExtCommand,
    },

    /// Say which installed version a directory uses, and why that one.
    ///
    /// It answers the question `php -v` answers, without running anything. The reason is the point:
    /// when a version surprises you, what you want to know is which of the four sources decided it.
    Resolve {
        /// Which language.
        #[arg(value_name = "RUNTIME", value_parser = runtime_kind)]
        kind: RuntimeKind,

        /// Use this version or range instead of what the directory says.
        ///
        /// Exact (`8.3.33`), a series (`8.3`, `8`) or a caret (`^8.3`), resolved against what is
        /// installed and never against what could be downloaded.
        #[arg(long, value_name = "VERSION", value_parser = version_constraint)]
        version: Option<VersionConstraint>,

        /// Resolve as if this were the working directory.
        #[arg(long, value_name = "DIR")]
        cwd: Option<PathBuf>,
    },
}

/// `mix runtime ext …` — the two `runtime.*_extension*` methods, as three verbs.
///
/// Three rather than two because `enable`/`disable` is what a person types; the wire carries one
/// method with a boolean, which is the daemon's shape rather than the sentence's.
#[derive(Debug, Subcommand)]
pub(crate) enum ExtCommand {
    /// List what this build has, and why each is on or off.
    List(WhichPhp),

    /// Load one on every PHP process of this version.
    Enable {
        /// The extension, as the listing spells it.
        #[arg(value_name = "EXTENSION")]
        name: String,

        #[command(flatten)]
        php: WhichPhp,
    },

    /// Stop loading one.
    Disable {
        /// The extension, as the listing spells it.
        #[arg(value_name = "EXTENSION")]
        name: String,

        #[command(flatten)]
        php: WhichPhp,
    },
}

/// Which PHP a `mix runtime ext` command is about.
///
/// **The default is not this client's to invent.** With no `--php`, the version is whatever
/// `runtime.resolve` answers for this directory — the same order the shim and the GUI get, decided
/// once, in the daemon.
#[derive(Debug, clap::Args)]
pub(crate) struct WhichPhp {
    /// The version, exactly as it is installed. Defaults to the one `php` resolves to here.
    #[arg(long = "php", value_name = "VERSION", value_parser = runtime_version)]
    version: Option<PackageVersion>,
}

/// Which kind a listing is about, or every kind.
#[derive(Debug, clap::Args)]
pub(crate) struct Kind {
    /// Only this language. Every one of them when it is left out.
    #[arg(long, value_name = "RUNTIME", value_parser = runtime_kind)]
    kind: Option<RuntimeKind>,
}

/// Which runtime a command acts on, which is the same question three times.
#[derive(Debug, clap::Args)]
pub(crate) struct Which {
    /// Which language.
    #[arg(value_name = "RUNTIME", value_parser = runtime_kind)]
    kind: RuntimeKind,

    /// Which version, exactly as `mix runtime available` lists it.
    ///
    /// Required, and never a range like `8.3`: choosing a version from a range answers with what is
    /// installed, and an install would be choosing between versions none of which are here yet.
    /// `mix runtime resolve` is where a range belongs.
    #[arg(value_name = "VERSION", value_parser = runtime_version)]
    version: PackageVersion,
}

/// `mix runtime …`: one call, one rendering — except the install, which is one call and a wait.
pub(crate) async fn runtime(
    command: RuntimeCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        RuntimeCommand::List(Kind { kind }) => {
            let filter = RuntimeFilter {
                kind,
                refresh: false,
            };
            let list: RuntimeList = ask(
                &mut client,
                rpc::method::RUNTIME_LIST_INSTALLED,
                encode(&filter),
            )
            .await?;
            emit(&rendered(json, &list, || {
                render::runtime::runtime_list(&list)
            }))?;
        }

        RuntimeCommand::Available {
            filter: Kind { kind },
            refresh,
            all,
            line,
        } => {
            let filter = RuntimeFilter { kind, refresh };
            let catalogue: RuntimeCatalogue = ask(
                &mut client,
                rpc::method::RUNTIME_LIST_AVAILABLE,
                encode(&filter),
            )
            .await?;
            emit(&rendered(json, &catalogue, || {
                render::runtime::runtime_catalogue(
                    &catalogue,
                    &render::catalogue::Lines { all, line },
                )
            }))?;
        }

        RuntimeCommand::Install {
            runtime,
            no_wait,
            yes,
            ignore_requirements,
        } => {
            let agreement = Agreement {
                yes,
                ignore_requirements,
            };
            return install(&mut client, target(runtime), agreement, no_wait, json).await;
        }

        RuntimeCommand::Upgrade {
            runtime,
            to,
            keep,
            dry_run,
            yes,
            ignore_requirements,
            no_wait,
        } => {
            let query = RuntimeUpgradeQuery {
                kind: runtime.kind,
                from: runtime.version,
                to,
            };
            let plan: UpgradePlan = ask(
                &mut client,
                rpc::method::RUNTIME_UPGRADE_PLAN,
                encode(&query),
            )
            .await?;
            if !upgrade_confirmed(&plan, dry_run, yes, json)? {
                return Ok(match dry_run {
                    true => ExitCode::SUCCESS,
                    false => ExitCode::FAILURE,
                });
            }

            let target = RuntimeTarget {
                kind: query.kind,
                version: plan.to.clone(),
            };
            let Some(install_prerequisites) = prerequisites_agreed(
                &mut client,
                rpc::method::RUNTIME_REQUIREMENTS,
                encode(&target),
                Agreement {
                    yes,
                    ignore_requirements,
                },
                json,
            )
            .await?
            else {
                return Ok(ExitCode::FAILURE);
            };

            let asked = RuntimeUpgrade {
                query: RuntimeUpgradeQuery {
                    to: Some(plan.to.clone()),
                    ..query
                },
                keep,
                install_prerequisites,
                ignore_requirements,
            };
            let started: JobSummary =
                ask(&mut client, rpc::method::RUNTIME_UPGRADE, encode(&asked)).await?;
            return finish_upgrade(&mut client, started, no_wait, json).await;
        }

        RuntimeCommand::Uninstall { runtime, force } => {
            let asked = RuntimeUninstall {
                target: target(runtime),
                force,
            };
            let removal: RuntimeRemoval =
                ask(&mut client, rpc::method::RUNTIME_UNINSTALL, encode(&asked)).await?;
            emit(&rendered(json, &removal, || {
                render::runtime::runtime_removal(&removal)
            }))?;
        }

        RuntimeCommand::Found => {
            let found: mixengine_proto::RuntimeFoundList =
                ask(&mut client, rpc::method::RUNTIME_FOUND, None).await?;
            emit(&rendered(json, &found, || {
                render::catalogue::on_disk(
                    found
                        .found
                        .iter()
                        .map(|row| (format!("{} {}", row.kind, row.version), &row.path, &row.why)),
                    "runtime",
                )
            }))?;
        }

        RuntimeCommand::Adopt { runtime } => {
            let summary: RuntimeSummary = ask(
                &mut client,
                rpc::method::RUNTIME_ADOPT,
                encode(&target(runtime)),
            )
            .await?;
            emit(&rendered(json, &summary, || {
                render::runtime::runtime_summary(&summary)
            }))?;
        }

        RuntimeCommand::Default { runtime } => {
            let summary: RuntimeSummary = ask(
                &mut client,
                rpc::method::RUNTIME_SET_DEFAULT,
                encode(&target(runtime)),
            )
            .await?;
            emit(&rendered(json, &summary, || {
                render::runtime::runtime_summary(&summary)
            }))?;
        }

        RuntimeCommand::Ext { command } => {
            let (php, choice) = match command {
                ExtCommand::List(php) => (php, None),
                ExtCommand::Enable { name, php } => (php, Some((name, true))),
                ExtCommand::Disable { name, php } => (php, Some((name, false))),
            };

            let runtime = which_php(&mut client, php).await?;

            match choice {
                None => {
                    let list: ExtensionList = ask(
                        &mut client,
                        rpc::method::RUNTIME_LIST_EXTENSIONS,
                        encode(&runtime),
                    )
                    .await?;
                    emit(&rendered(json, &list, || {
                        render::runtime::extension_list(&list)
                    }))?;
                }

                Some((name, enabled)) => {
                    let choice = ExtensionChoice {
                        runtime,
                        name,
                        enabled,
                    };
                    let change: ExtensionChange = ask(
                        &mut client,
                        rpc::method::RUNTIME_SET_EXTENSION,
                        encode(&choice),
                    )
                    .await?;
                    emit(&rendered(json, &change, || {
                        render::runtime::extension_change(&change)
                    }))?;
                }
            }
        }

        RuntimeCommand::Resolve { kind, version, cwd } => {
            let question = question(kind, version, cwd)?;
            let resolved: ResolvedRuntime =
                ask(&mut client, rpc::method::RUNTIME_RESOLVE, encode(&question)).await?;
            emit(&rendered(json, &resolved, || {
                render::runtime::runtime_resolved(&resolved)
            }))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// `mix runtime install`: start the download, and follow it unless told not to.
///
/// **Waiting is the client's own decision and not a second API.** The daemon answers a job the
/// instant it has one, which is what keeps an eighty-megabyte download off the RPC call; what a
/// person typing this wants is for the command to end when PHP is there, and what a script wants is
/// an exit status that means it. So `mix` polls `job.wait` until the job ends — each poll is one
/// round trip over a local socket — and prints the progress it passes on **stderr**, so that stdout
/// still carries exactly one answer and `--json` still emits exactly one object.
///
/// **What the machine lacks is asked about first**, once, before anything is started.
async fn install(
    client: &mut Client,
    target: RuntimeTarget,
    agreement: Agreement,
    no_wait: bool,
    json: bool,
) -> Result<ExitCode, Error> {
    let Some(install_prerequisites) = prerequisites_agreed(
        client,
        rpc::method::RUNTIME_REQUIREMENTS,
        encode(&target),
        agreement,
        json,
    )
    .await?
    else {
        return Ok(ExitCode::FAILURE);
    };

    let (kind, version) = (target.kind, target.version.clone());
    let asked = RuntimeInstall {
        target,
        install_prerequisites,
        ignore_requirements: agreement.ignore_requirements,
    };
    let started: JobSummary = ask(client, rpc::method::RUNTIME_INSTALL, encode(&asked)).await?;

    if no_wait {
        emit(&rendered(json, &started, || {
            render::job::job_status(&started)
        }))?;
        return Ok(ExitCode::SUCCESS);
    }

    let finished = follow(client, started, json).await?;

    emit(&rendered(json, &finished, || {
        render::job::job_status(&finished)
    }))?;

    if render::job::job_succeeded(&finished) && !json {
        said_what_it_lacks(client, kind, &version).await?;
    }

    Ok(match render::job::job_succeeded(&finished) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    })
}

/// Say what the release just installed cannot do, and what adds it.
///
/// **From the cached index, and never at the install's expense.** A listing that cannot be read
/// costs this line and nothing else: the install has already succeeded.
async fn said_what_it_lacks(
    client: &mut Client,
    kind: RuntimeKind,
    version: &PackageVersion,
) -> Result<(), Error> {
    let Ok(catalogue) = ask::<RuntimeCatalogue>(
        client,
        rpc::method::RUNTIME_LIST_AVAILABLE,
        encode(&RuntimeFilter {
            kind: Some(kind),
            ..RuntimeFilter::default()
        }),
    )
    .await
    else {
        return Ok(());
    };

    let Some(release) = catalogue
        .runtimes
        .iter()
        .find(|release| release.kind == kind && &release.version == version)
    else {
        return Ok(());
    };

    let offered = ask::<PackageCatalogue>(
        client,
        rpc::method::PACKAGE_LIST_AVAILABLE,
        encode(&PackageFilter {
            package: Some("msys2".to_owned()),
            ..PackageFilter::default()
        }),
    )
    .await
    .ok();
    let newest = offered.as_ref().and_then(|catalogue| {
        catalogue
            .packages
            .iter()
            .map(|release| &release.version)
            .max_by(|a, b| a.cmp_precedence(b))
    });

    // `installed` is the daemon's answer, which leaves out a devkit whose folder is gone.
    let installed = offered
        .as_ref()
        .is_some_and(|catalogue| catalogue.packages.iter().any(|release| release.installed));
    let devkit = match (installed, newest) {
        (true, _) => render::runtime::Devkit::Installed,
        (false, Some(version)) => render::runtime::Devkit::Offered(version.as_str()),
        (false, None) => render::runtime::Devkit::NotOffered,
    };

    if let Some(line) = render::runtime::lacks_after_install(&release.lacks, devkit) {
        emit(&line)?;
    }

    Ok(())
}

/// The wire shape of "which runtime", from the two arguments a person typed.
fn target(Which { kind, version }: Which) -> RuntimeTarget {
    RuntimeTarget { kind, version }
}

/// Which PHP `mix runtime ext` was told about, or the one this directory resolves to.
///
/// The fallback is a **call** and not a rule of this client's: which version a directory uses is
/// `runtime.resolve`'s answer, and a `mix` that worked it out for itself would be the second opinion
/// this whole product exists to prevent.
async fn which_php(
    client: &mut Client,
    WhichPhp { version }: WhichPhp,
) -> Result<RuntimeTarget, Error> {
    let kind = RuntimeKind::Php;

    if let Some(version) = version {
        return Ok(RuntimeTarget { kind, version });
    }

    let resolved: ResolvedRuntime = ask(
        client,
        rpc::method::RUNTIME_RESOLVE,
        encode(&question(kind, None, None)?),
    )
    .await?;

    Ok(RuntimeTarget {
        kind,
        version: resolved.runtime.version,
    })
}

/// The wire shape of "which version does this directory use", and the one place `mix` reads the
/// environment below `main`.
///
/// **It has to be read here rather than at `main`**, which is where `docs/standards/rust.md` puts
/// configuration, and the exception is narrow enough to state exactly: the variable's *name* depends
/// on the kind the user just named — `MIXENGINE_PHP`, `MIXENGINE_NODE` — so nothing above the parse
/// knows which one to look at. The name itself is still not this client's to invent:
/// [`RuntimeKind::override_env`] is in `mixengine-proto`, so the shim and the GUI read the same one.
///
/// And it is read by *this* process rather than by the daemon on purpose. `MIXENGINE_PHP=8.1 php -v`
/// is a sentence about the shell it was typed in; a daemon consulting its own environment would
/// answer with whatever it happened to be started with, for everybody at once.
fn question(
    kind: RuntimeKind,
    version: Option<VersionConstraint>,
    cwd: Option<PathBuf>,
) -> Result<RuntimeQuestion, Error> {
    let variable = kind.override_env();

    let version = match version {
        Some(version) => Some(version),

        // An empty value is "not set", which is how a shell unsets one for a single command. Every
        // other value is meant, so one that is not a version is refused rather than skipped past —
        // a `MIXENGINE_PHP` that quietly does nothing is the exact failure this whole command
        // exists to explain.
        None => match std::env::var(variable) {
            Ok(value) if value.is_empty() => None,
            Ok(value) => Some(VersionConstraint::parse(value).map_err(|error| {
                Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{variable} is set to something that is not a version: {error}"),
                )
                .with_hint(
                    "a version (`8.3.33`), a series (`8.3`) or a caret (`^8.3`), the same forms \
                     `mixengine.toml` accepts",
                )
            })?),
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{variable} is set to something that is not text"),
                ));
            }
        },
    };

    Ok(RuntimeQuestion {
        kind,
        // The directory `mix` was run in, unless one was named. A process with no working directory
        // at all — a deleted one on Unix — asks the question without it rather than failing: the
        // flag and the default still answer, and the daemon says which of them did.
        cwd: cwd
            .or_else(|| std::env::current_dir().ok())
            .map(|path| path.display().to_string()),
        version,
    })
}
