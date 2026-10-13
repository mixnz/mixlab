//! `mix project`, and which project a command means when it is not told.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    Error, ErrorCode, ProjectCreate, ProjectDetail, ProjectExport, ProjectList, ProjectQuery,
    ProjectRef, ProjectRemoval, ProjectUpdate, RuntimeKind, VersionConstraint, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::output::{emit, rendered};
use crate::parse::pin;
use crate::render;

/// `mix project …` — one subcommand per `project.*` method, and nothing that is not one.
///
/// `import` is an **alias** on `create` rather than a seventh subcommand: both produce one row, and
/// what makes a create an import is the `mixengine.toml` already lying in the directory rather than
/// a different call. An alias is the same subcommand under a second name, so the rule above holds.
#[derive(Debug, Subcommand)]
pub(crate) enum ProjectCommand {
    /// Register a directory as a project.
    ///
    /// With no `--name` and no `--pin`, whatever the `mixengine.toml` in that directory says is
    /// used. That is how you adopt a colleague's checkout.
    #[command(alias = "import")]
    Create {
        /// The project's root. Defaults to the current directory.
        #[arg(value_name = "DIR")]
        root: Option<PathBuf>,

        /// What to call it. Defaults to the manifest's name, then to the directory's own.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,

        /// Pin a language, as `php=^8.3`. May be given more than once.
        #[arg(long = "pin", value_name = "RUNTIME=VERSION", value_parser = pin)]
        pins: Vec<(RuntimeKind, VersionConstraint)>,
    },

    /// List the projects this home has been told about.
    List,

    /// Show one, with its pins in the order they take effect.
    Show {
        #[command(flatten)]
        project: WhichProject,
    },

    /// Change a project's name, root or pins.
    ///
    /// `--pin` **replaces** every pin rather than adding to one: `--clear-pins` with no `--pin`
    /// removes them all, and leaving both out changes nothing.
    Update {
        #[command(flatten)]
        project: WhichProject,

        /// A new name.
        //
        // `id` spelled out because the flattened project argument is also called `name`, and clap
        // refuses two arguments under one id — it did so at *parse* time, so `mix project update
        // blog --name blogging` panicked instead of running. Found by T77's
        // `every_command_is_one_clap_can_build`, which is now what stops the next one.
        #[arg(long, id = "new_name", value_name = "NAME")]
        name: Option<String>,

        /// A new root, for a repository that moved.
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,

        /// Pin a language, as `php=^8.3`. Replaces every pin the project had.
        #[arg(long = "pin", value_name = "RUNTIME=VERSION", value_parser = pin)]
        pins: Vec<(RuntimeKind, VersionConstraint)>,

        /// Remove every pin.
        #[arg(long, conflicts_with = "pins")]
        clear_pins: bool,
    },

    /// Hold this project's services out of idle shutdown while you are working on it.
    ///
    /// A verb of its own rather than a flag on `update`, because it is a thing you do to a project
    /// for an afternoon and not part of what the project *is*.
    ///
    /// It reaches the PHP pool this project's sites name. It does not yet reach the database they
    /// query, because nothing in MixEngine records which database a project uses.
    #[command(name = "keep-warm")]
    KeepWarm {
        #[command(flatten)]
        project: WhichProject,

        /// Stop keeping it warm.
        #[arg(long)]
        off: bool,
    },

    /// Forget a project. The directory is left exactly as it is.
    Delete {
        #[command(flatten)]
        project: WhichProject,
    },

    /// Write the project into `<root>/mixengine.toml`, keeping everything else in the file.
    Export {
        #[command(flatten)]
        project: WhichProject,
    },
}

/// Which project a command is about, which is the same question four times.
///
/// **The default is the directory you are in**, not a name this client invents: with no argument
/// `mix` sends the working directory and the daemon walks up to the nearest registered root — the
/// same walk the shim does.
#[derive(Debug, clap::Args)]
pub(crate) struct WhichProject {
    /// The project's name. Defaults to whichever project the current directory is in.
    #[arg(value_name = "PROJECT")]
    pub(crate) name: Option<String>,
}

/// `mix project …`: one call, one rendering.
pub(crate) async fn project(
    command: ProjectCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        ProjectCommand::Create { root, name, pins } => {
            let create = ProjectCreate {
                root: here(root)?.display().to_string(),
                name,
                pins: (!pins.is_empty()).then(|| pins.into_iter().collect()),
            };
            let detail: ProjectDetail =
                ask(&mut client, rpc::method::PROJECT_CREATE, encode(&create)).await?;
            emit(&rendered(json, &detail, || render::project_detail(&detail)))?;
        }

        ProjectCommand::List => {
            let list: ProjectList = ask(&mut client, rpc::method::PROJECT_LIST, None).await?;
            emit(&rendered(json, &list, || render::project_list(&list)))?;
        }

        ProjectCommand::Show { project } => {
            let query = ProjectQuery {
                project: which(project)?,
            };
            let detail: ProjectDetail =
                ask(&mut client, rpc::method::PROJECT_SHOW, encode(&query)).await?;
            emit(&rendered(json, &detail, || render::project_detail(&detail)))?;
        }

        ProjectCommand::Update {
            project,
            name,
            root,
            pins,
            clear_pins,
        } => {
            let update = ProjectUpdate {
                project: which(project)?,
                name,
                root: root.map(|root| root.display().to_string()),
                pins: match (clear_pins, pins.is_empty()) {
                    (true, _) => Some(std::collections::BTreeMap::new()),
                    (false, true) => None,
                    (false, false) => Some(pins.into_iter().collect()),
                },
                // `mix project update` changes what a project *is*; keeping it warm is a thing you
                // do to it while you work, and has its own verb.
                keep_warm: None,
            };
            let detail: ProjectDetail =
                ask(&mut client, rpc::method::PROJECT_UPDATE, encode(&update)).await?;
            emit(&rendered(json, &detail, || render::project_detail(&detail)))?;
        }

        ProjectCommand::KeepWarm { project, off } => {
            let update = ProjectUpdate {
                project: which(project)?,
                name: None,
                root: None,
                pins: None,
                keep_warm: Some(!off),
            };
            let detail: ProjectDetail =
                ask(&mut client, rpc::method::PROJECT_UPDATE, encode(&update)).await?;
            emit(&rendered(json, &detail, || render::project_detail(&detail)))?;
        }

        ProjectCommand::Delete { project } => {
            let query = ProjectQuery {
                project: which(project)?,
            };
            let removal: ProjectRemoval =
                ask(&mut client, rpc::method::PROJECT_DELETE, encode(&query)).await?;
            emit(&rendered(json, &removal, || {
                render::project_removal(&removal)
            }))?;
        }

        ProjectCommand::Export { project } => {
            let query = ProjectQuery {
                project: which(project)?,
            };
            let exported: ProjectExport =
                ask(&mut client, rpc::method::PROJECT_EXPORT, encode(&query)).await?;
            emit(&rendered(json, &exported, || {
                render::project_export(&exported)
            }))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

pub(crate) fn which(project: WhichProject) -> Result<ProjectRef, Error> {
    match project.name {
        Some(name) => Ok(ProjectRef::Name(name)),
        None => Ok(ProjectRef::Path(here(None)?.display().to_string())),
    }
}

/// A directory argument, or the one this process is in.
pub(crate) fn here(given: Option<PathBuf>) -> Result<PathBuf, Error> {
    match given {
        Some(path) if path.is_absolute() => Ok(path),
        Some(path) => working_directory().map(|cwd| cwd.join(path)),
        None => working_directory(),
    }
}

/// Where this `mix` was run, which is what a project reference defaults to.
fn working_directory() -> Result<PathBuf, Error> {
    std::env::current_dir().map_err(|error| {
        Error::new(
            ErrorCode::Io,
            format!("this process has no working directory: {error}"),
        )
    })
}
