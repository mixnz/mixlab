//! `mix service`, and switching the front end.

use std::io::Write as _;
use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    Error, ErrorCode, FrontEndReport, FrontEndServer, FrontEndSwitch, IdleReport, JobOutcome,
    JobSummary, PackageVersion, Priority, ProjectRef, ResetCredential, ResourceLimits,
    SaveResources, SaveResourcesSet, ServiceAutostartSet, ServiceCreate, ServiceCreation,
    ServiceDelete, ServiceId, ServiceIdleSet, ServiceLimitsReport, ServiceLimitsSet, ServiceList,
    ServiceQuery, ServiceRemoval, ServiceRole, ServiceSummary, ServiceTarget, ServiceWalk, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::commands::logs::logs;
use crate::confirm::unanswered;
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::parse::{idle_after, runtime_version, service_id};
use crate::{confirm, render};

/// `mix service …` — one subcommand per `service.*` method, and nothing that is not one.
#[derive(Debug, Subcommand)]
pub(crate) enum ServiceCommand {
    /// List every declared service and what it is doing.
    List,

    /// Describe one service.
    ///
    /// The id is required, where `start` and the rest take an optional one: a status with no
    /// subject is a `list` that was typed wrongly, and answering it as a list would hide that.
    Status {
        /// The service to describe.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,
    },

    /// Print what a service has been printing.
    ///
    /// The one `mix service` subcommand that is not a `service.*` method: output is a stream, and a
    /// JSON-RPC call cannot be one, so the lines arrive on a connection of their own.
    //
    // The reasoning is ADR 0009, `docs/decisions/0009-logs-travel-on-their-own-stream.md`. It is
    // a `//` comment and not a `///` one deliberately: this text is `mix service logs --help`, and
    // since T90 it is also a page of the user handbook — a link to a path only a checkout of this
    // repository has is a dead link for every reader of both.
    Logs {
        /// The service to read.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// How many of the lines already printed to begin with.
        #[arg(long, short = 'n', value_name = "LINES", default_value_t = 200)]
        lines: usize,

        /// Keep printing as the service prints, rather than stopping at what it already said.
        ///
        /// Survives the service crashing and being restarted: what is being followed is the
        /// service, not one run of its process.
        #[arg(long, short)]
        follow: bool,
    },

    /// What this service may take, and what this machine will actually enforce of it.
    ///
    /// With no subcommand: read it. `set` replaces it, `clear` removes it.
    Limits {
        /// The service to read or cap.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        #[command(subcommand)]
        command: Option<LimitsCommand>,
    },

    /// When this service is stopped for being unused, and what is holding it open.
    ///
    /// With no flag: read it. One of the three flags replaces it.
    ///
    /// Nothing idles by default in this build: a stopped service stays stopped until you start it,
    /// so switching this on is a choice you make per service.
    Idle {
        /// The service to read or set.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// Stop it once nothing has used it for this long, such as `30m`, `2h` or `90m`.
        #[arg(long, value_name = "DURATION", group = "idle_change", value_parser = idle_after)]
        after: Option<u32>,

        /// Never stop it for being unused, whatever a later release makes the default.
        #[arg(long, group = "idle_change")]
        never: bool,

        /// Go back to whatever its recipe wants, which in this build is never.
        #[arg(long, group = "idle_change")]
        default: bool,
    },

    /// Whether this home stops services nobody is using ("Save battery" in MixLab).
    ///
    /// With no flag: read it. Off unless you turn it on. While it is off, a service is never
    /// stopped for being idle unless you gave it a time with `mix service idle`. While it is on, a
    /// PHP pool nobody used for half an hour, or a database or cache for an hour, is stopped and
    /// started again by the next request that needs it.
    ///
    /// Setting this starts and stops nothing. The next idle check reads it.
    SaveResources {
        /// Stop services nobody is using.
        #[arg(long, group = "save_resources_change")]
        on: bool,

        /// Do not.
        #[arg(long, group = "save_resources_change")]
        off: bool,
    },

    /// Whether this service starts when MixEngine does.
    ///
    /// With no flag: read it. `mix autostart` is a different question: whether this *machine*
    /// starts a daemon for this home when you log in.
    ///
    /// A service that something set here depends on is started too, whether or not it is set
    /// itself: a pool whose database is missing is a pool that fails its health check.
    ///
    /// Setting this starts and stops nothing. What it changes is what the next daemon start walks.
    Autostart {
        /// The service to read or set.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// Start it when MixEngine starts.
        #[arg(long, group = "autostart_change")]
        on: bool,

        /// Do not.
        #[arg(long, group = "autostart_change")]
        off: bool,
    },

    /// Create a service from an installed package.
    ///
    /// The part of the id before `@` is the package it is an instance of, which is why there is no
    /// separate argument for it: `mariadb@main` is an instance of `mariadb`. A package that runs
    /// only once, such as Caddy, is named without an `@` at all.
    Create {
        /// The service to create.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// Which installed version of its package to run.
        #[arg(value_name = "VERSION", value_parser = runtime_version)]
        version: PackageVersion,

        /// The port it listens on. The recipe's own default when it is left out.
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,

        /// The address it binds. `127.0.0.1` when it is left out.
        #[arg(long, value_name = "ADDR")]
        bind: Option<String>,

        /// Where its data lives. The home's own layout when it is left out, and never a directory
        /// another service already keeps its data in.
        #[arg(long, value_name = "DIR")]
        data_dir: Option<String>,

        /// Start it whenever the daemon starts.
        #[arg(long)]
        autostart: bool,
    },

    /// Delete a service, keeping its data directory.
    ///
    /// Takes the row and the configuration generated from it. **Never the data**, because that is
    /// somebody's databases. The answer names the directory that was left, so nobody has to go
    /// looking.
    Delete {
        /// The service to delete.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// Delete it even though a site declares it.
        #[arg(long)]
        force: bool,
    },

    /// Which program every site in this home is reached through.
    ///
    /// Answered from the service listing: what a service is *for* travels on its summary, so there
    /// is no second question to ask and no client anywhere decides that `nginx` means "front end".
    FrontEnd,

    /// Change it.
    ///
    /// Stops the front end this home is on, swaps the row, renders every site for the new one and
    /// starts it. Every site is unreachable while that happens.
    ///
    /// **On Linux the new server needs this machine's permission to answer on 80 and 443**, because
    /// that permission is written into the binary and the new binary does not have it. A machine
    /// where nobody grants it stays on the front end it had, and says so.
    SetFrontEnd {
        /// The program to move to.
        #[arg(value_name = "SERVER", value_enum)]
        server: FrontEndServerArg,

        /// Which installed version of it. The newest installed when it is left out.
        #[arg(long, value_name = "VERSION", value_parser = runtime_version)]
        version: Option<PackageVersion>,

        /// Answer the question in advance.
        #[arg(long, short)]
        yes: bool,

        /// Return once the daemon has accepted the switch rather than once it has made it.
        #[arg(long)]
        no_wait: bool,
    },

    /// Start a service, and everything it depends on.
    Start(StartTarget),

    /// Stop a service, and everything that depends on it.
    Stop(Target),

    /// Stop a service and what depends on it, then start that same set again.
    Restart(Target),

    /// Re-set this database's superuser password inside its own data directory.
    ///
    /// For a server that refuses the password MixEngine holds for it, with `ERROR 1045` or
    /// `password authentication failed`. A database keeps its own copy of that password inside its
    /// data directory, and this machine's credential store holds the other. They are written
    /// together when the service first starts and can only come apart afterwards. Once they have,
    /// nothing can log in to put them back, because every way of changing the copy inside the
    /// directory needs the password that was lost.
    ///
    /// This stops the service and everything that depends on it, writes the password this home
    /// holds into the data directory through the server's own offline bootstrap, and starts back
    /// what went down. **Every database in it is kept.** `mix job list` holds the account of what
    /// ran.
    ///
    /// Only the database servers keep a password of their own; anything else is refused before
    /// anything stops.
    ResetCredential {
        /// The database to repair: `mariadb@main`, `postgres@shop`.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// Do not ask before stopping the service.
        #[arg(long, short = 'y')]
        yes: bool,

        /// Answer as soon as the repair has been accepted, rather than when it has finished.
        #[arg(long)]
        no_wait: bool,
    },

    /// List service data an earlier install left, and whether each can be adopted.
    Found,

    /// Turn service data an earlier install left back into a service, with a new admin password.
    ///
    /// The databases and accounts in it are kept. The service is left stopped.
    Adopt {
        /// A service `mix service found` lists: `mariadb@main`.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,
    },
}

/// What can be done to a service's limits.
#[derive(Debug, clap::Subcommand)]
pub(crate) enum LimitsCommand {
    /// Replace every limit on this service.
    ///
    /// **Every field, not only the ones named.** A flag left out is that field's default (uncapped,
    /// or ordinary priority), so `set --cpu 50` clears a memory ceiling that was there. All three
    /// fields of the result are printed, so a cleared limit is on the screen.
    // Deliberate: composing a partial change would mean reading the current value and merging it,
    // which is business logic a client may not hold.
    Set {
        /// A ceiling on CPU, as a percentage of one core. Left out: uncapped.
        #[arg(long, value_name = "PERCENT")]
        cpu: Option<u8>,

        /// A ceiling on memory, in megabytes. Left out: uncapped.
        #[arg(long, value_name = "MB")]
        memory: Option<u32>,

        /// How this service competes for CPU.
        #[arg(long, value_name = "PRIORITY", default_value = "normal")]
        priority: PriorityArg,
    },

    /// Remove every limit from this service.
    ///
    /// A named operation rather than a `set` with three absent flags, so that "uncap this" is
    /// something a person can type rather than something they have to infer.
    Clear,
}

/// [`FrontEndServer`] on a command line.
///
/// Its own type for [`PriorityArg`]'s reason — `clap::ValueEnum` cannot be derived for a type in
/// another crate — and the words are the same ones the API spells, because they are the same two
/// programs.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum FrontEndServerArg {
    /// Caddy, the default front end.
    // What this project picks when there is a choice: ADR 0004.
    Caddy,

    /// nginx, which is a first-class alternative and not a lesser one.
    Nginx,
}

impl From<FrontEndServerArg> for FrontEndServer {
    fn from(arg: FrontEndServerArg) -> Self {
        match arg {
            FrontEndServerArg::Caddy => Self::Caddy,
            FrontEndServerArg::Nginx => Self::Nginx,
        }
    }
}

/// [`Priority`] on a command line.
///
/// Its own type because `clap::ValueEnum` cannot be derived for a type in another crate, and because
/// the words a person types are this crate's to choose.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum PriorityArg {
    /// Competes with everything else the user is running.
    Normal,

    /// Yields to foreground work.
    Background,
}

impl From<PriorityArg> for Priority {
    fn from(arg: PriorityArg) -> Self {
        match arg {
            PriorityArg::Normal => Self::Normal,
            PriorityArg::Background => Self::Background,
        }
    }
}

/// What `start`, `stop` and `restart` take, which is the same question three times.
#[derive(Debug, clap::Args)]
pub(crate) struct Target {
    /// The service to act on. Every declared service when it is left out.
    ///
    /// Naming one does not mean acting on only that one: a plan is the transitive set, and what the
    /// daemon walked comes back in the answer.
    #[arg(value_name = "SERVICE", value_parser = service_id)]
    service: Option<ServiceId>,

    /// Return once the daemon has accepted the plan, rather than once it has walked it.
    ///
    /// `mix` waits by default, because `mix service start db && …` is a sentence about the database
    /// being up: an answer sent before the walk would exit `0` for a service that never came up.
    #[arg(long)]
    no_wait: bool,
}

/// One service or all of them, for the two verbs that have no scope of their own.
fn walk_target(target: &Target) -> ServiceTarget {
    ServiceTarget {
        service: target.service.clone(),
        project: None,
        wait: !target.no_wait,
    }
}

/// What `start` takes, which is [`Target`] plus the one scope only a start has.
///
/// **A type of its own rather than a field on [`Target`]** — roadmap task **T125**. The field would
/// have to be refused on `stop` and `restart`, and a flag a command lists in its own `--help` in
/// order to reject it is a flag that reads as a gap. What a project-scoped stop would mean is a
/// different set from this one — this includes the front end every *other* site is reached
/// through — so the daemon keeps its refusal for a hand-written request, and `mix` never offers it.
#[derive(Debug, clap::Args)]
pub(crate) struct StartTarget {
    #[command(flatten)]
    target: Target,

    /// Every service one project needs, instead of one or all.
    ///
    /// Its sites' databases and caches, the php-fpm pool they name, and the front end they are
    /// reached through, as the daemon works them out.
    // Only the daemon can: the set is `site_service_links`, and no client may derive it.
    #[arg(long, value_name = "PROJECT", conflicts_with = "service")]
    project: Option<String>,
}

/// Which service in a listing is the front end, as the daemon answered — roadmap task **T97**.
///
/// **Three answers and not an [`Option`]**, because the third is a wire fact and not a home fact: a
/// daemon built before `ServiceSummary::role` sends none, and reading that as *no front end* would
/// tell somebody their sites are unserved when they are being served perfectly well.
#[derive(Debug)]
enum Found<'a> {
    /// The row whose role says so.
    On(&'a ServiceSummary),

    /// Every row answered, and none of them is one.
    None,

    /// No row answered at all: this daemon predates the member.
    Unanswered,
}

/// Pick it out, by what the daemon said a service is *for*.
///
/// Not a client deciding anything — the role is the daemon's own answer, from the crate that owns
/// the recipes — this is where it is read. ADR 0026's enforcement clause is that no client maps a
/// package name to a role, and this function is `mix` obeying it.
fn the_front_end(list: &ServiceList) -> Found<'_> {
    if let Some(service) = list
        .services
        .iter()
        .find(|service| matches!(service.role, Some(ServiceRole::FrontEnd { .. })))
    {
        return Found::On(service);
    }

    // A home with no services at all has no front end, and that is an answer whatever the daemon's
    // build: there is no row whose role could have been missing.
    match list.services.is_empty() || list.services.iter().any(|service| service.role.is_some()) {
        true => Found::None,
        false => Found::Unanswered,
    }
}

/// `mix service set-front-end <server>` — roadmap task **T97**.
///
/// **The question is asked before the call and not after it**, on `mix cleanup`'s rule: a switch
/// stops the web server every site in this home is reached through, and somebody who typed it needs
/// to know that before it happens rather than to read it in a report.
///
/// `grant: true` is sent because the person has just been told that a permission prompt may appear
/// — which is T64's rule met — and because the switch cannot go ahead without the grant on the one
/// system that needs it.
async fn set_front_end(
    client: &mut Client,
    json: bool,
    server: FrontEndServer,
    version: Option<PackageVersion>,
    yes: bool,
    no_wait: bool,
) -> Result<ExitCode, Error> {
    if !yes {
        let list: ServiceList = ask(client, rpc::method::SERVICE_LIST, None).await?;

        if !agreed_to_switch(&list, server, json)? {
            // Saying no is an answer and not a failure — `mix uninstall`'s rule. Nothing moved, so
            // the same command works when the person is ready.
            return Ok(ExitCode::SUCCESS);
        }
    }

    let started: JobSummary = ask(
        client,
        rpc::method::SERVICE_SET_FRONT_END,
        encode(&FrontEndSwitch {
            server,
            version,
            grant: true,
        }),
    )
    .await?;

    if no_wait {
        emit(&rendered(json, &started, || render::job_status(&started)))?;
        return Ok(ExitCode::SUCCESS);
    }

    let finished = follow(client, started, json).await?;

    let Some(JobOutcome::Succeeded { result }) = finished.outcome.clone() else {
        emit(&rendered(json, &finished, || render::job_status(&finished)))?;
        return Ok(ExitCode::FAILURE);
    };

    let report: FrontEndReport = serde_json::from_value(result).map_err(|error| {
        Error::new(
            ErrorCode::Internal,
            format!(
                "mix {} cannot read the front-end report: {error}",
                env!("CARGO_PKG_VERSION")
            ),
        )
    })?;

    emit(&rendered(json, &report, || {
        render::front_end_report(&report)
    }))?;

    Ok(match report.wanted_more() {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    })
}

/// Ask, once, in front of what is about to happen — roadmap task **T97**.
///
/// **Asking for the server a home is already on is a yes**, on `agreed_to_cleanup`'s rule: a command
/// that asked *"change nothing?"* is one people learn to answer without reading.
///
/// **`--json` never asks**, and never acts instead.
fn agreed_to_switch(list: &ServiceList, server: FrontEndServer, json: bool) -> Result<bool, Error> {
    let standing = match the_front_end(list) {
        Found::On(service) => Some(service),
        // A home with none has nothing to stop, and a daemon that will not say is one the switch
        // itself refuses a moment later — neither is a reason to withhold the question.
        Found::None | Found::Unanswered => None,
    };

    if standing.is_some_and(|service| service.id.name() == server.package()) {
        return Ok(true);
    }

    if json {
        return Err(unanswered());
    }

    // Printed rather than held back, because the question below is refused under `--json` anyway:
    // what somebody is about to allow is what they are shown.
    emit(&match standing {
        Some(service) => format!(
            "{} is this home's front end.\nswitching to {server} stops it, renders every site for \
             {server} and starts it; no site is reachable while that happens.\n",
            service.id,
            server = server.package()
        ),
        None => format!(
            "this home has no front end; {} becomes one, and is left stopped.\n",
            server.package()
        ),
    })?;

    emit(
        "this machine may ask permission for it to answer on 80 and 443, and anything else \
         MixEngine is already waiting for is asked for at the same time; `mix elevation status` \
         lists that.\n",
    )?;

    match confirm::ask(&format!("\nswitch to {}? [y/N] ", server.package())) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            // On stderr, beside the question it answers.
            let _ = writeln!(std::io::stderr(), "nothing was changed");

            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}

/// Ask before a repair stops a database and rewrites the credential inside it — **T127**.
///
/// **Asked rather than assumed, and `--yes` is the way past it**, because this is the one `mix
/// service` subcommand that changes something inside a data directory. What it says is what somebody
/// weighing it needs: what stops, what is rewritten, and — the part that decides it — that the
/// databases are kept.
///
/// # Errors
///
/// [`unanswered`] where there is nobody to ask: a script reaching this needs to be told which flag
/// says yes in advance, rather than to have one assumed for it.
fn agreed_to_reset(service: &ServiceId) -> Result<bool, Error> {
    match confirm::ask(&format!(
        "\n{service} and everything that depends on it will be stopped, and its superuser password \
         rewritten inside its data directory.\nEvery database in it is kept.\n\nre-set the \
         credential? [y/N] "
    )) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            // On stderr, beside the question it answers.
            let _ = writeln!(std::io::stderr(), "nothing was changed");

            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}

/// `mix service …`: one call, one rendering, and an exit code that means what a shell expects.
///
/// **A walk that failed is an answer and not an error**, which is why this returns an
/// [`ExitCode`] rather than reporting through [`report`](crate::output::report): a plan of six services where the fourth
/// fails leaves three running, one failed and two never tried, and all of that goes to stdout in
/// both renderings. What the failure changes is the exit status, so `mix service start db && …`
/// stops where a person reading the output would.
pub(crate) async fn service(
    command: ServiceCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    // The walk methods differ by one string and one verb, so they are one arm with both in it —
    // three copies of this block would be three places for the two to drift apart.
    let (method, walked, params) = match &command {
        ServiceCommand::List => {
            let list: ServiceList = ask(&mut client, rpc::method::SERVICE_LIST, None).await?;
            emit(&rendered(json, &list, || render::service_list(&list)))?;
            return Ok(ExitCode::SUCCESS);
        }

        ServiceCommand::Status { service } => {
            let query = ServiceQuery {
                service: service.clone(),
            };
            let summary: ServiceSummary =
                ask(&mut client, rpc::method::SERVICE_STATUS, encode(&query)).await?;
            emit(&rendered(json, &summary, || {
                render::service_status(&summary)
            }))?;
            return Ok(ExitCode::SUCCESS);
        }

        ServiceCommand::Limits { service, command } => {
            let report: ServiceLimitsReport = match command {
                // A read. `ServiceTarget` rather than a bare id because that is the shape every
                // other `service.*` read takes, and the daemon refuses one with no service named.
                None => {
                    let target = ServiceTarget {
                        service: Some(service.clone()),
                        project: None,
                        wait: false,
                    };

                    ask(&mut client, rpc::method::SERVICE_LIMITS, encode(&target)).await?
                }

                Some(LimitsCommand::Set {
                    cpu,
                    memory,
                    priority,
                }) => {
                    let asked = ServiceLimitsSet {
                        service: service.clone(),
                        limits: ResourceLimits {
                            cpu_percent: *cpu,
                            memory_mb: *memory,
                            priority: Priority::from(*priority),
                        },
                    };

                    ask(&mut client, rpc::method::SERVICE_SET_LIMITS, encode(&asked)).await?
                }

                // The same method, with the value that means "nothing". One door rather than two,
                // so there is no second place for the rules to be applied differently.
                Some(LimitsCommand::Clear) => {
                    let asked = ServiceLimitsSet {
                        service: service.clone(),
                        limits: ResourceLimits::default(),
                    };

                    ask(&mut client, rpc::method::SERVICE_SET_LIMITS, encode(&asked)).await?
                }
            };

            emit(&rendered(json, &report, || render::service_limits(&report)))?;
            return Ok(ExitCode::SUCCESS);
        }

        ServiceCommand::Idle {
            service,
            after,
            never,
            default,
        } => {
            // The three flags are one `clap` group, so at most one of them is set and this is a
            // read when none is. `--default` is the absent value and `--never` is zero: the two
            // that look alike from outside and are stored differently on purpose.
            let change = match (*after, *never, *default) {
                (Some(minutes), _, _) => Some(Some(minutes)),
                (None, true, _) => Some(Some(0)),
                (None, false, true) => Some(None),
                (None, false, false) => None,
            };

            let report: IdleReport = match change {
                None => {
                    let target = ServiceTarget {
                        service: Some(service.clone()),
                        project: None,
                        wait: false,
                    };

                    ask(&mut client, rpc::method::SERVICE_IDLE, encode(&target)).await?
                }

                Some(minutes) => {
                    let asked = ServiceIdleSet {
                        service: service.clone(),
                        minutes,
                    };

                    ask(&mut client, rpc::method::SERVICE_SET_IDLE, encode(&asked)).await?
                }
            };

            emit(&rendered(json, &report, || render::service_idle(&report)))?;
            return Ok(ExitCode::SUCCESS);
        }

        ServiceCommand::SaveResources { on, off } => {
            // The two flags are one `clap` group, so at most one is set and neither means read.
            let answer: SaveResources = match (*on, *off) {
                (false, false) => {
                    ask(&mut client, rpc::method::SERVICE_SAVE_RESOURCES, None).await?
                }
                (wanted, _) => {
                    ask(
                        &mut client,
                        rpc::method::SERVICE_SET_SAVE_RESOURCES,
                        encode(&SaveResourcesSet { on: wanted }),
                    )
                    .await?
                }
            };

            emit(&rendered(json, &answer, || render::save_resources(answer)))?;
            return Ok(ExitCode::SUCCESS);
        }

        ServiceCommand::Autostart { service, on, off } => {
            // The two flags are one `clap` group, so at most one is set and neither means read.
            let summary: ServiceSummary = match (*on, *off) {
                (false, false) => {
                    let query = ServiceQuery {
                        service: service.clone(),
                    };

                    ask(&mut client, rpc::method::SERVICE_STATUS, encode(&query)).await?
                }

                (wanted, _) => {
                    let asked = ServiceAutostartSet {
                        service: service.clone(),
                        autostart: wanted,
                    };

                    ask(
                        &mut client,
                        rpc::method::SERVICE_SET_AUTOSTART,
                        encode(&asked),
                    )
                    .await?
                }
            };

            emit(&rendered(json, &summary, || {
                render::service_autostart(&summary)
            }))?;
            return Ok(ExitCode::SUCCESS);
        }

        ServiceCommand::Create {
            service,
            version,
            port,
            bind,
            data_dir,
            autostart,
        } => {
            let create = ServiceCreate {
                id: service.clone(),
                version: version.clone(),
                port: *port,
                bind_addr: bind.clone(),
                data_dir: data_dir.clone(),
                // Only when it was asked for: `false` and "nobody said" are the same row, and
                // sending the first would put a default of ours on the wire as a decision.
                autostart: autostart.then_some(true),
                overrides: None,
            };
            let creation: ServiceCreation =
                ask(&mut client, rpc::method::SERVICE_CREATE, encode(&create)).await?;
            emit(&rendered(json, &creation, || {
                render::service_creation(&creation)
            }))?;
            return Ok(ExitCode::SUCCESS);
        }

        ServiceCommand::Found => {
            let found: mixengine_proto::ServiceFoundList =
                ask(&mut client, rpc::method::SERVICE_FOUND, None).await?;
            emit(&rendered(json, &found, || render::service_found(&found)))?;
            return Ok(ExitCode::SUCCESS);
        }

        ServiceCommand::Adopt { service } => {
            let summary: mixengine_proto::ServiceSummary = ask(
                &mut client,
                rpc::method::SERVICE_ADOPT,
                encode(&mixengine_proto::ServiceAdopt {
                    service: service.clone(),
                }),
            )
            .await?;
            emit(&rendered(json, &summary, || {
                render::service_adopted(&summary)
            }))?;
            return Ok(ExitCode::SUCCESS);
        }

        ServiceCommand::FrontEnd => {
            let list: ServiceList = ask(&mut client, rpc::method::SERVICE_LIST, None).await?;

            return match the_front_end(&list) {
                // `--json` answers with the summary itself, or `null`, so a script asks
                // `.id` and `.state` where the API names them.
                Found::On(service) => {
                    emit(&rendered(json, &service, || {
                        render::front_end(Some(service))
                    }))?;
                    Ok(ExitCode::SUCCESS)
                }

                Found::None => {
                    emit(&rendered(json, &Option::<ServiceSummary>::None, || {
                        render::front_end(None)
                    }))?;
                    Ok(ExitCode::SUCCESS)
                }

                // ADR 0019's cost, paid in the one place that reads the member: a daemon from before
                // T97 sends no role at all, and guessing from the package names is the thing this
                // command exists so that nobody does.
                Found::Unanswered => Err(Error::new(
                    ErrorCode::PreconditionFailed,
                    "this daemon does not say what a service is for, so which one is the front \
                     end cannot be read",
                )
                .with_hint(
                    "it is older than this `mix`; restart it so the build you installed is the \
                     one answering",
                )),
            };
        }

        ServiceCommand::SetFrontEnd {
            server,
            version,
            yes,
            no_wait,
        } => {
            return set_front_end(
                &mut client,
                json,
                (*server).into(),
                version.clone(),
                *yes,
                *no_wait,
            )
            .await;
        }

        ServiceCommand::Delete { service, force } => {
            let asked = ServiceDelete {
                target: ServiceQuery {
                    service: service.clone(),
                },
                force: *force,
            };
            let removal: ServiceRemoval =
                ask(&mut client, rpc::method::SERVICE_DELETE, encode(&asked)).await?;
            emit(&rendered(json, &removal, || {
                render::service_removal(&removal)
            }))?;
            return Ok(ExitCode::SUCCESS);
        }

        ServiceCommand::Logs {
            service,
            lines,
            follow,
        } => {
            return logs(
                &mut client,
                &format!("logs/service/{service}"),
                *lines,
                *follow,
                json,
            )
            .await;
        }

        ServiceCommand::Start(start) => (
            rpc::method::SERVICE_START,
            render::Walked::Start,
            encode(&ServiceTarget {
                service: start.target.service.clone(),
                project: start.project.clone().map(ProjectRef::Name),
                wait: !start.target.no_wait,
            }),
        ),
        ServiceCommand::Stop(target) => (
            rpc::method::SERVICE_STOP,
            render::Walked::Stop,
            encode(&walk_target(target)),
        ),
        ServiceCommand::Restart(target) => (
            rpc::method::SERVICE_RESTART,
            render::Walked::Restart,
            encode(&walk_target(target)),
        ),

        // **Its own params type**, which is why every arm here encodes its own: T125's finding was
        // that a target with no service means *the whole home*, and a repair may not be askable
        // that way.
        ServiceCommand::ResetCredential {
            service,
            yes,
            no_wait,
        } => {
            if !*yes && !agreed_to_reset(service)? {
                return Ok(ExitCode::SUCCESS);
            }

            (
                rpc::method::SERVICE_RESET_CREDENTIAL,
                render::Walked::ResetCredential,
                encode(&ResetCredential {
                    service: service.clone(),
                    wait: !*no_wait,
                }),
            )
        }
    };

    let walk: ServiceWalk = ask(&mut client, method, params).await?;
    emit(&rendered(json, &walk, || {
        render::service_walk(walked, &walk)
    }))?;

    Ok(match walk.failed {
        None => ExitCode::SUCCESS,
        Some(_) => ExitCode::FAILURE,
    })
}
