//! `mix extension`: MixEngine's add-ons.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    Error, ErrorCode, ExtensionAvailable, ExtensionCatalogue, ExtensionConsent, ExtensionId,
    ExtensionInspect, ExtensionInspection, ExtensionInstall, ExtensionOrigin, ExtensionPlan,
    ExtensionPlanRequest, ExtensionRemoval, ExtensionTarget, ExtensionUninstall,
    InstalledExtensions, JobSummary, ServiceWalk, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::commands::project::here;
use crate::confirm::unanswered;
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::{confirm, render};

/// `mix extension …` — one subcommand per `extension.*` method this build has.
#[derive(Debug, clap::Subcommand)]
pub(crate) enum ExtensionCommand {
    /// Say what installing this extension here would produce.
    Inspect {
        /// The extension's directory, or its `extension.toml`.
        path: PathBuf,
    },

    /// What this home has installed.
    List,

    /// What the signed registry publishes.
    Available {
        /// Ask the registry again even if the cached copy is still fresh.
        ///
        /// The daemon otherwise answers from a cache for up to six hours, which is the wrong
        /// default for someone who just watched an extension get published and does not want to
        /// wait for their own machine to notice.
        #[arg(long)]
        refresh: bool,
    },

    /// Say what installing one would do, and change nothing.
    Plan {
        /// The extension's id in the registry.
        #[arg(conflicts_with = "path")]
        id: Option<String>,

        /// A directory to read instead of the registry. **Nothing vouches for one of these.**
        #[arg(long)]
        path: Option<PathBuf>,
    },

    /// Install one.
    Install {
        /// The extension's id in the registry.
        #[arg(conflicts_with = "path")]
        id: Option<String>,

        /// A directory to install instead of a registry entry. **Nothing vouches for one of
        /// these**, and the row records it as unsigned for as long as it stays installed.
        #[arg(long)]
        path: Option<PathBuf>,

        /// Install without asking about what it declares.
        #[arg(long)]
        yes: bool,

        /// Answer with the job rather than waiting for it.
        #[arg(long)]
        no_wait: bool,
    },

    /// Remove one.
    Uninstall {
        /// Which extension.
        id: String,

        /// Delete its data directory as well.
        ///
        /// **Kept when this is absent**, which is the answer that can be undone.
        #[arg(long)]
        delete_data: bool,
    },

    /// Start the service an extension runs as.
    Start {
        /// Which extension.
        id: String,
    },

    /// Stop it.
    Stop {
        /// Which extension.
        id: String,
    },
}

/// `mix extension …` — read a manifest and say what installing it would produce.
pub(crate) async fn extension(
    command: ExtensionCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        ExtensionCommand::Inspect { path } => {
            // Resolved here, because the daemon has no idea what directory this process is in and
            // a relative path sent as it was typed would be read against the wrong one.
            let asked = ExtensionInspect {
                path: here(Some(path))?.display().to_string(),
            };

            let inspection: ExtensionInspection =
                ask(&mut client, rpc::method::EXTENSION_INSPECT, encode(&asked)).await?;

            emit(&rendered(json, &inspection, || {
                render::extension::extension_inspection(&inspection)
            }))?;
        }

        ExtensionCommand::List => {
            let list: InstalledExtensions =
                ask(&mut client, rpc::method::EXTENSION_LIST, encode(&())).await?;

            emit(&rendered(json, &list, || {
                render::extension::installed_extensions(&list)
            }))?;
        }

        ExtensionCommand::Available { refresh } => {
            let asked = ExtensionAvailable { refresh };
            let catalogue: ExtensionCatalogue = ask(
                &mut client,
                rpc::method::EXTENSION_AVAILABLE,
                encode(&asked),
            )
            .await?;

            emit(&rendered(json, &catalogue, || {
                render::extension::extension_catalogue(&catalogue)
            }))?;
        }

        ExtensionCommand::Plan { id, path } => {
            let asked = ExtensionPlanRequest {
                source: origin(id, path)?,
            };
            let plan: ExtensionPlan =
                ask(&mut client, rpc::method::EXTENSION_PLAN, encode(&asked)).await?;

            emit(&rendered(json, &plan, || {
                render::extension::extension_plan(&plan)
            }))?;
        }

        ExtensionCommand::Install {
            id,
            path,
            yes,
            no_wait,
        } => {
            let source = origin(id, path)?;

            // **The plan is read before anything is installed, and the consent names it.** Two
            // calls rather than one because that is what makes the question answerable: the daemon
            // has no keyboard, and the permissions a person is agreeing to arrive with the listing
            // rather than with the artifact.
            let plan: ExtensionPlan = ask(
                &mut client,
                rpc::method::EXTENSION_PLAN,
                encode(&ExtensionPlanRequest {
                    source: source.clone(),
                }),
            )
            .await?;

            if !yes && !agreed_to_install(&plan, json)? {
                return Ok(ExitCode::FAILURE);
            }

            let asked = ExtensionInstall {
                consent: ExtensionConsent {
                    id: plan.id.clone(),
                    version: plan.version.clone(),
                    signed: plan.signed,
                    network: plan.permissions.network,
                },
                source,
            };

            let started: JobSummary =
                ask(&mut client, rpc::method::EXTENSION_INSTALL, encode(&asked)).await?;

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

            let succeeded = render::job::job_succeeded(&finished);

            return Ok(match succeeded {
                true => ExitCode::SUCCESS,
                false => ExitCode::FAILURE,
            });
        }

        ExtensionCommand::Uninstall { id, delete_data } => {
            let asked = ExtensionUninstall {
                id: extension_id(&id)?,
                delete_data,
            };
            let removal: ExtensionRemoval = ask(
                &mut client,
                rpc::method::EXTENSION_UNINSTALL,
                encode(&asked),
            )
            .await?;

            emit(&rendered(json, &removal, || {
                render::extension::extension_removal(&removal)
            }))?;
        }

        ExtensionCommand::Start { id } => {
            let asked = ExtensionTarget {
                id: extension_id(&id)?,
            };
            let walk: ServiceWalk =
                ask(&mut client, rpc::method::EXTENSION_START, encode(&asked)).await?;

            emit(&rendered(json, &walk, || {
                render::service::service_walk(render::service::Walked::Start, &walk)
            }))?;
        }

        ExtensionCommand::Stop { id } => {
            let asked = ExtensionTarget {
                id: extension_id(&id)?,
            };
            let walk: ServiceWalk =
                ask(&mut client, rpc::method::EXTENSION_STOP, encode(&asked)).await?;

            emit(&rendered(json, &walk, || {
                render::service::service_walk(render::service::Walked::Stop, &walk)
            }))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Which of the two an `install` or a `plan` names, refusing neither and both.
fn origin(id: Option<String>, path: Option<PathBuf>) -> Result<ExtensionOrigin, Error> {
    match (id, path) {
        // Resolved here, because the daemon has no idea what directory this process is in and a
        // relative path sent as it was typed would be read against the wrong one.
        (None, Some(path)) => Ok(ExtensionOrigin::Path {
            path: here(Some(path))?.display().to_string(),
        }),

        (Some(id), None) => Ok(ExtensionOrigin::Registry {
            id: extension_id(&id)?,
        }),

        _ => Err(Error::new(
            ErrorCode::InvalidArgument,
            "name an extension from the registry, or --path a directory",
        )),
    }
}

/// An id the wire will accept, refused here rather than by the daemon.
fn extension_id(given: &str) -> Result<ExtensionId, Error> {
    ExtensionId::parse(given).map_err(|source| {
        Error::new(
            ErrorCode::InvalidArgument,
            format!("{given} is not an extension id: {source}"),
        )
    })
}

/// Ask about what an extension declares, and answer whether to go on.
///
/// **What it prints is what the daemon will be told was shown.** `permissions.services` is a
/// disclosure and not a boundary (ADR 0014), and the rendering says so — an extension runs as this
/// account, so what it may reach is what this account may reach.
fn agreed_to_install(plan: &ExtensionPlan, json: bool) -> Result<bool, Error> {
    if json {
        return Err(unanswered());
    }

    match confirm::ask(&format!(
        "{}
install it? [y/N] ",
        render::extension::extension_plan(plan)
    )) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            let _ = writeln!(std::io::stderr(), "nothing was installed");
            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}
