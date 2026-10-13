//! `mix package`.

use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    Error, JobSummary, PackageCatalogue, PackageFilter, PackageInstall, PackageList,
    PackageRemoval, PackageTarget, PackageUpgrade, PackageUpgradeQuery, PackageVersion,
    UpgradePlan, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::parse::runtime_version;
use crate::prerequisites::{Agreement, prerequisites_agreed};
use crate::render;
use crate::upgrades::{finish_upgrade, upgrade_confirmed};

/// `mix package …` — one subcommand per `package.*` method.
///
/// **Not runtimes.** PHP and Node are `mix runtime`, which has a default version and a shim behind
/// it; these are the servers a *service* is an instance of. What a package becomes once it is
/// installed is `mix service create`.
#[derive(Debug, Subcommand)]
pub(crate) enum PackageCommand {
    /// List the packages installed in this home.
    List(Named),

    /// List the versions the package index offers for this machine.
    ///
    /// Only packages this build knows how to configure and run: an entry MixEngine has no recipe for
    /// would unpack into a directory nothing could start.
    Available {
        #[command(flatten)]
        filter: Named,

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
        #[arg(long, value_name = "LINE", requires = "package")]
        line: Option<String>,
    },

    /// Download and install one version.
    Install {
        #[command(flatten)]
        package: WhichPackage,

        /// Return once the daemon has accepted the install, rather than once it has finished.
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
    /// Refused while a service is an instance of it, naming the services. `mix service delete` is
    /// what frees it, and deleting a service keeps its data directory.
    Uninstall {
        #[command(flatten)]
        package: WhichPackage,
    },

    /// Update one installed version to the newest release of its line, and move what uses it.
    ///
    /// Its instances move to the new version one at a time; the old one is removed afterwards
    /// unless `--keep` is given or an instance could not move. The plan is printed first.
    Upgrade {
        #[command(flatten)]
        package: WhichPackage,

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

    /// Record a version that is on disk but not listed, such as one an earlier install left.
    ///
    /// Nothing is downloaded. The folder is checked against the package index first.
    Adopt {
        #[command(flatten)]
        package: WhichPackage,
    },

    /// List versions that are on disk but not listed, and why each is not.
    Found,
}

/// Which package a listing is about, or every package.
#[derive(Debug, clap::Args)]
pub(crate) struct Named {
    /// Only this package. Every one of them when it is left out.
    #[arg(long, value_name = "PACKAGE")]
    package: Option<String>,
}

/// Which package a command acts on, which is the same question twice.
#[derive(Debug, clap::Args)]
pub(crate) struct WhichPackage {
    /// Which package, as `mix package available` lists it.
    #[arg(value_name = "PACKAGE")]
    package: String,

    /// Which version, exactly as `mix package available` lists it.
    #[arg(value_name = "VERSION", value_parser = runtime_version)]
    version: PackageVersion,
}

/// `mix package …`: one call, one rendering — except the install, which follows a job.
pub(crate) async fn package(
    command: PackageCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        PackageCommand::List(Named { package }) => {
            let filter = PackageFilter {
                package,
                refresh: false,
            };
            let list: PackageList =
                ask(&mut client, rpc::method::PACKAGE_LIST, encode(&filter)).await?;
            emit(&rendered(json, &list, || render::package_list(&list)))?;
        }

        PackageCommand::Available {
            filter: Named { package },
            refresh,
            all,
            line,
        } => {
            let filter = PackageFilter { package, refresh };
            let catalogue: PackageCatalogue = ask(
                &mut client,
                rpc::method::PACKAGE_LIST_AVAILABLE,
                encode(&filter),
            )
            .await?;
            emit(&rendered(json, &catalogue, || {
                render::package_catalogue(&catalogue, &render::catalogue::Lines { all, line })
            }))?;
        }

        PackageCommand::Install {
            package,
            no_wait,
            yes,
            ignore_requirements,
        } => {
            let target = PackageTarget {
                package: package.package,
                version: package.version,
            };
            let agreement = Agreement {
                yes,
                ignore_requirements,
            };

            let Some(install_prerequisites) = prerequisites_agreed(
                &mut client,
                rpc::method::PACKAGE_REQUIREMENTS,
                encode(&target),
                agreement,
                json,
            )
            .await?
            else {
                return Ok(ExitCode::FAILURE);
            };

            let asked = PackageInstall {
                target,
                install_prerequisites,
                ignore_requirements,
            };
            let started: JobSummary =
                ask(&mut client, rpc::method::PACKAGE_INSTALL, encode(&asked)).await?;

            if no_wait {
                emit(&rendered(json, &started, || {
                    render::job::job_status(&started)
                }))?;
                return Ok(ExitCode::SUCCESS);
            }

            let finished = follow(&mut client, started, json).await?;
            emit(&rendered(json, &finished, || {
                render::job::job_status(&finished)
            }))?;

            return Ok(match render::job::job_succeeded(&finished) {
                true => ExitCode::SUCCESS,
                false => ExitCode::FAILURE,
            });
        }

        PackageCommand::Found => {
            let found: mixengine_proto::PackageFoundList =
                ask(&mut client, rpc::method::PACKAGE_FOUND, None).await?;
            emit(&rendered(json, &found, || {
                render::catalogue::on_disk(
                    found.found.iter().map(|row| {
                        (
                            format!("{} {}", row.package, row.version),
                            &row.path,
                            &row.why,
                        )
                    }),
                    "package",
                )
            }))?;
        }

        PackageCommand::Adopt { package } => {
            let target = PackageTarget {
                package: package.package,
                version: package.version,
            };
            let summary: mixengine_proto::PackageSummary =
                ask(&mut client, rpc::method::PACKAGE_ADOPT, encode(&target)).await?;
            emit(&rendered(json, &summary, || {
                render::package_summary(&summary)
            }))?;
        }

        PackageCommand::Upgrade {
            package,
            to,
            keep,
            dry_run,
            yes,
            ignore_requirements,
            no_wait,
        } => {
            let query = PackageUpgradeQuery {
                package: package.package,
                from: package.version,
                to,
            };
            let plan: UpgradePlan = ask(
                &mut client,
                rpc::method::PACKAGE_UPGRADE_PLAN,
                encode(&query),
            )
            .await?;
            if !upgrade_confirmed(&plan, dry_run, yes, json)? {
                return Ok(match dry_run {
                    true => ExitCode::SUCCESS,
                    false => ExitCode::FAILURE,
                });
            }

            let target = PackageTarget {
                package: query.package.clone(),
                version: plan.to.clone(),
            };
            let Some(install_prerequisites) = prerequisites_agreed(
                &mut client,
                rpc::method::PACKAGE_REQUIREMENTS,
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

            // `grant`: the person has just agreed to a plan that says this machine may ask, which is
            // what `set-front-end` does after its own question.
            let asked = PackageUpgrade {
                query: PackageUpgradeQuery {
                    to: Some(plan.to.clone()),
                    ..query
                },
                keep,
                install_prerequisites,
                ignore_requirements,
                grant: true,
            };
            let started: JobSummary =
                ask(&mut client, rpc::method::PACKAGE_UPGRADE, encode(&asked)).await?;
            return finish_upgrade(&mut client, started, no_wait, json).await;
        }

        PackageCommand::Uninstall { package } => {
            let target = PackageTarget {
                package: package.package,
                version: package.version,
            };
            let removal: PackageRemoval =
                ask(&mut client, rpc::method::PACKAGE_UNINSTALL, encode(&target)).await?;
            emit(&rendered(json, &removal, || {
                render::package_removal(&removal)
            }))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}
