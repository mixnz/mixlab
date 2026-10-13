//! `mix` — the reference client. It renders what the daemon returns and decides nothing itself.
//!
//! Every command here is one RPC and one rendering. That is the rule from
//! [CLAUDE.md](../../../CLAUDE.md) — *no business logic in clients* — and it is why this binary is
//! shaped the way it is: the only decisions it makes on its own are which home it is talking about,
//! whether to start a daemon that is not running, and how to put the answer on screen. Everything
//! else is `mixengined`'s, including the wording of every failure.
//!
//! **Failures are the wire error, always.** Whether the daemon refused a call or `mix` never
//! reached one, what comes out is a `mixengine_proto::Error` — a stable code, one sentence, and a
//! hint where there is something to do. A script gets the same object out of `--json` in both cases
//! and can branch on `code` without caring which side of the socket produced it.

mod autostart;
mod client;
mod commands;
mod confirm;
mod docs;
mod error;
mod home;
mod jobs;
mod output;
mod parse;
mod prerequisites;
mod render;
mod upgrades;

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory as _, Parser, Subcommand};
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    DaemonStatus, Error, ErrorCode, JobOutcome, JobSummary, Removal, ResidueId, ServiceId,
    UninstallQuery, UninstallReport, UpdateApplied, UpdateApply, UpdateCheck, UpdateDecide,
    UpdateDecision, UpdateFinish, UpdateHandOver, UpdateHandedOver, UpdatePlacement, UpdateStatus,
    rpc,
};

use autostart::Autostart;
use client::{Client, ask, encode};
use commands::autostart::{AutostartCommand, autostart_entry};
use commands::blueprint::{BlueprintCommand, blueprint};
use commands::cert::{CertCommand, cert};
use commands::daemon::{DaemonCommand, daemon_credential_store, daemon_stop};
use commands::database::{DatabaseCommand, database};
use commands::disk::{cleanup, disk};
use commands::doctor::{bundle, doctor, self_repair};
use commands::domain::{DomainCommand, domain};
use commands::elevation::{ElevationCommand, elevation};
use commands::extension::{ExtensionCommand, extension};
use commands::home::{HomeCommand, home};
use commands::job::{JobCommand, job};
use commands::logs::logs;
use commands::metrics::metrics;
use commands::package::{PackageCommand, package};
use commands::path::{PathCommand, path};
use commands::project::{ProjectCommand, project};
use commands::runtime::{RuntimeCommand, runtime};
use commands::service::{ServiceCommand, service};
use commands::site::{SiteCommand, site};
use commands::status::{status, storage};
use confirm::unanswered;
use jobs::follow;
use output::{emit, rendered, report};
use parse::service_id;

/// What `--version` prints.
///
/// **A build nobody released is worth saying out loud** — T95. It defaults to its own home
/// directory, so a bug report pasting this line answers "which MixEngine is this, and which
/// directory was it looking at" in one go, and `packaging/*/build.sh` refuses an artifact that
/// carries the note.
const VERSION: &str = if mixengine_platform::RELEASE {
    env!("CARGO_PKG_VERSION")
} else {
    concat!(env!("CARGO_PKG_VERSION"), " (development build)")
};

/// Command line of the client. Configuration enters the program here and is passed down; nothing
/// deeper reads the environment on its own.
#[derive(Debug, Parser)]
#[command(name = "mix", version = VERSION, about = "MixEngine command line")]
struct Args {
    /// Root directory of the MixEngine installation to talk to.
    ///
    /// Defaults to the OS convention, the same one `mixengined` uses, so the two always talk about
    /// the same daemon.
    #[arg(long, global = true, env = "MIXENGINE_HOME", value_name = "DIR")]
    home: Option<PathBuf>,

    /// Emit machine-readable JSON instead of the human-facing rendering.
    #[arg(long, global = true)]
    json: bool,

    /// Fail instead of starting a daemon when none is running for this home.
    ///
    /// `mix` normally starts one, which is what makes the first command a person types work. The
    /// flag is for the caller that wants a question answered rather than a machine changed: a
    /// monitoring check, or a CI step that should not create a home as a side effect of asking
    /// whether one is there.
    #[arg(long, global = true)]
    no_autostart: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the daemon's health, version and what it is currently running.
    Status,

    /// Show where this home keeps the directories that grow, and whether that can still change.
    ///
    /// **Starts no daemon and needs none.** `runtimes/`, `packages/`, `data/` and `logs/` can each
    /// be moved to another disk by `[paths]` in `config.toml`, or by starting `mixengined` with
    /// `--runtimes`, `--packages`, `--data` or `--logs`. That choice is free only until the first
    /// runtime, package or service is installed. From then on, where each of them lives is recorded
    /// against it.
    ///
    /// The answer comes from `mixengined` itself, run once: whether anything is installed is a
    /// question about rows in this home's database, and `mix` does not open one.
    Storage,

    /// Control the daemon itself.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },

    /// Read the MixLab handbook, offline, in English or Vietnamese.
    ///
    /// With no topic it lists them. It talks to no daemon and needs no home, because the pages are
    /// compiled into this binary. That is why `mix docs install` answers on a machine where nothing
    /// starts. The same pages are published at <https://mixnz.github.io/mixlab/>, as HTML for a
    /// person and as plain Markdown for a program.
    Docs {
        /// Which topic. Omit it to list them.
        topic: Option<String>,

        /// Which language: `en` or `vi`. An unrecognised one is answered in English.
        #[arg(long, value_name = "CODE", env = "MIXENGINE_LANG")]
        lang: Option<String>,

        /// Print the whole command reference as Markdown, instead of a topic.
        ///
        /// It is built from this binary's own command definitions, so it cannot describe a flag the
        /// binary does not have. It is English only, because those definitions are.
        ///
        /// It does not conflict with `--lang`: that flag carries `MIXENGINE_LANG`, and a variable
        /// somebody exported once should not be able to refuse a command.
        // `docs/guide/en/cli.md` is generated from this output by `packaging/docs.sh --reference`.
        #[arg(long, conflicts_with = "topic")]
        reference: bool,
    },

    /// Install, remove and choose between language runtimes.
    Runtime {
        #[command(subcommand)]
        command: RuntimeCommand,
    },

    /// Install and remove the servers, databases and caches a service runs.
    Package {
        #[command(subcommand)]
        command: PackageCommand,
    },

    /// Register the directories this home knows about, and what they pin.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },

    /// Declare what is served out of a project's directory, and at what name.
    Site {
        #[command(subcommand)]
        command: SiteCommand,
    },

    /// Write down what a project is made of, and see what applying that somewhere else would do.
    Blueprint {
        #[command(subcommand)]
        command: BlueprintCommand,
    },

    /// Read an `extension.toml` without installing anything.
    Extension {
        #[command(subcommand)]
        command: ExtensionCommand,
    },

    /// Make a database on one of this home's database servers, and an account that reaches it.
    #[command(visible_alias = "db")]
    Database {
        #[command(subcommand)]
        command: DatabaseCommand,
    },

    /// Show what MixEngine is costing this machine: CPU and memory, per service and for the daemon.
    ///
    /// One reading and out by default. `--watch` opens the live stream, which also puts the daemon
    /// on its one-second rate. When nobody is looking, it samples once a minute.
    Metrics {
        /// Keep printing, a block per reading, until interrupted.
        #[arg(long)]
        watch: bool,

        /// Read the recorded history instead, starting this far back: `30m`, `2h`, `1d`.
        #[arg(long, conflicts_with = "watch")]
        since: Option<String>,

        /// One subject only. Omit for every service and the daemon.
        #[arg(long, value_name = "SERVICE", value_parser = service_id)]
        service: Option<ServiceId>,

        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Examine this machine and say what is wrong with it.
    ///
    /// Reports and repairs nothing unless `--repair` is passed. Exits non-zero when it found a
    /// problem, so a script can ask.
    Doctor {
        /// Repair everything that can be repaired, and ask for the rest.
        ///
        /// Repairs inside this home are made at once. Anything needing an administrator is queued,
        /// shown, and then granted with one prompt for the whole batch.
        #[arg(long)]
        repair: bool,

        /// Do not ask before raising the prompt. Only with `--repair`.
        #[arg(long, requires = "repair")]
        yes: bool,

        /// Return as soon as the grant has started, rather than waiting for it. Only with
        /// `--repair`.
        #[arg(long, requires = "repair")]
        no_wait: bool,

        /// Write one diagnostics archive and print where it went.
        ///
        /// Everything a bug report needs in one file: the findings above, this daemon's status,
        /// what this machine is, any crash reports this home has recorded, and the tail of the log.
        /// Whatever was deliberately left out is named beside them.
        #[arg(long, conflicts_with = "repair")]
        bundle: bool,

        /// Copy the archive here as well. Only with `--bundle`.
        #[arg(long, requires = "bundle", value_name = "FILE")]
        out: Option<PathBuf>,
    },

    /// Update MixEngine itself.
    ///
    /// Checks for a newer release and shows its version, its size and what changed before asking.
    /// On yes, the daemon downloads it, checks the signature, runs the new `mixengined` once to be
    /// sure this machine will start it, stops what it is supervising, replaces the binaries and
    /// exits. This command then starts the new daemon, which starts your services again.
    ///
    /// `mixengine-elevate` is never replaced here. It runs as root, and updating it needs an
    /// elevation prompt of its own.
    ///
    /// A copy of MixEngine that a package manager installed is not updated by this: it says so, and
    /// names the directory.
    ///
    /// On a Mac that installed MixLab from the .pkg, the next .pkg is downloaded, checked and
    /// opened in Installer.app instead, and nothing is stopped. When the installation is done, `mix
    /// self-update --finish` restarts MixEngine on the new version.
    SelfUpdate {
        /// Check and print what is available. Installs nothing.
        #[arg(long)]
        check: bool,

        /// Answer the prompt in advance, for a script with nobody at the keyboard.
        #[arg(long, conflicts_with = "check")]
        yes: bool,

        /// Finish an update Installer.app has installed: stop the services, start the new daemon,
        /// and start them again.
        #[arg(long, conflicts_with_all = ["check", "yes"])]
        finish: bool,
    },

    /// Where this home's disk has gone, and what would take each part back.
    ///
    /// Five categories (runtimes, data, logs, certs and cache) plus everything else. Each row says
    /// what would reclaim it: your databases never, a runtime only through `mix runtime uninstall`,
    /// the certificates only by losing HTTPS until they are issued again, and the logs and the
    /// cache by `mix cleanup`.
    Disk,

    /// Bring back an earlier install's projects, sites and services from the folders it kept.
    Home {
        #[command(subcommand)]
        command: HomeCommand,
    },

    /// Take back what is safe to lose: rotated log files and the download cache.
    ///
    /// Nothing else, whatever `mix disk` says the total is. Your databases, your installed
    /// runtimes, your certificates, the log files being written right now and this home's crash
    /// reports are all out of reach: this command matches file names, it does not sweep the home.
    ///
    /// Refuses while another job is running, because a cleanup empties the directory a download
    /// resumes from.
    Cleanup {
        /// Leave the rotated log files where they are.
        #[arg(long)]
        keep_logs: bool,

        /// Leave the download cache where it is.
        #[arg(long)]
        keep_cache: bool,

        /// Answer the confirmation in advance, for a script with nobody at the keyboard.
        #[arg(long)]
        yes: bool,

        /// Start the work and print the job, rather than waiting for it to finish.
        #[arg(long = "no-wait")]
        no_wait: bool,
    },

    /// Take MixEngine off this machine.
    ///
    /// Undoes everything MixEngine has written outside its own directory (the hosts block, the DNS
    /// routing, the port grant, the certificate authority, the firewall rules, the login entry,
    /// your PATH entry, the privileged helper and its audit log), and then removes the directory
    /// itself.
    ///
    /// `--dry-run` names every one of them and changes nothing. Exits non-zero when anything it
    /// acted on is still there, so a script can ask.
    Uninstall {
        /// List what would be removed, and remove nothing.
        #[arg(long)]
        dry_run: bool,

        /// Leave this home's directory where it is, and undo only what is outside it.
        ///
        /// Keeps the databases in `data/`, the certificates and everything else this home holds. The
        /// daemon keeps running, because there is still a home for it to serve.
        #[arg(long)]
        keep_home: bool,

        /// Leave the directories `[paths]` moved out of the home where they are.
        ///
        /// Its own choice, apart from `--keep-home`: a home can go while `data/` on another disk
        /// stays, or the reverse. A directory that was never moved is inside the home.
        #[arg(long)]
        keep_relocated: bool,

        /// Also remove the program itself, where the macOS package placed it: the commands in
        /// /usr/local/bin, MixLab.app and the package receipt.
        ///
        /// Without it the program stays, and the plan's `package` row says so. On Linux the package
        /// manager removes the program, and the row names the command.
        #[arg(long)]
        package: bool,

        /// With `--dry-run`: print only the relocated directories, one path per line.
        ///
        /// For a program to read. The Windows uninstaller shows them before it asks anything.
        #[arg(long, requires = "dry_run")]
        relocated: bool,

        /// With `--dry-run`: print only the programs in the way, one per line, with the pid and the
        /// folder each one uses.
        ///
        /// Prints nothing when nothing is in the way. For a program to read: the Windows uninstaller
        /// shows the list before it removes anything.
        #[arg(long, requires = "dry_run", conflicts_with = "relocated")]
        blocked: bool,

        /// Answer the confirmation in advance, for a script with nobody at the keyboard.
        #[arg(long, conflicts_with = "dry_run")]
        yes: bool,

        /// Start the work and print the job, rather than waiting for it to finish.
        #[arg(long = "no-wait", conflicts_with = "dry_run")]
        no_wait: bool,
    },

    /// Add, remove and diagnose the names this home answers for.
    Domain {
        #[command(subcommand)]
        command: DomainCommand,
    },

    /// Inspect and control the services this home declares.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },

    /// Watch the long operations this daemon is running.
    Job {
        #[command(subcommand)]
        command: JobCommand,
    },

    /// Put this home's commands on your PATH, or take them off again.
    Path {
        #[command(subcommand)]
        command: PathCommand,
    },

    /// Start this home's daemon when you log in, or stop doing that.
    Autostart {
        #[command(subcommand)]
        command: AutostartCommand,
    },

    /// See what needs an administrator's permission, ask for it once, or forget it.
    Elevation {
        #[command(subcommand)]
        command: ElevationCommand,
    },

    /// Look at the certificate authority this home signs its sites with.
    Cert {
        #[command(subcommand)]
        command: CertCommand,
    },
}

fn main() -> ExitCode {
    let args = Args::parse();
    let json = args.json;

    // Answered here, above `run`, and deliberately: `run` resolves a home and most commands then
    // dial a daemon, and `mix docs` must do neither. The page somebody reaches for is usually the
    // one that explains why nothing starts — roadmap task T90.
    if let Command::Docs {
        topic,
        lang,
        reference,
    } = &args.command
    {
        return docs_command(topic.as_deref(), lang.as_deref(), *reference, json);
    }

    match run(args) {
        Ok(code) => code,
        Err(error) => {
            report(&error, json);
            ExitCode::FAILURE
        }
    }
}

/// `mix docs`. Returns rather than exits, so the exit code is the one every other command uses.
///
/// The failure here is **not** a `mixengine_proto::Error`, which is the one place this binary
/// departs from the rule at the top of this file — and it is the same reason the command exists: no
/// daemon was asked anything, so there is no wire failure to report. What a caller gets is a
/// sentence naming every topic there is.
fn docs_command(topic: Option<&str>, lang: Option<&str>, reference: bool, json: bool) -> ExitCode {
    if reference {
        print!("{}", docs::reference(&Args::command()));
        return ExitCode::SUCCESS;
    }

    let locale = docs::resolve_locale(lang);

    let Some(topic) = topic else {
        print!("{}", docs::topics(locale));
        return ExitCode::SUCCESS;
    };

    match docs::look_up(locale, topic) {
        Ok(page) => {
            if json {
                let value = serde_json::json!({
                    "topic": page.slug,
                    "locale": locale.code(),
                    "title": page.title,
                    "url": page.url(),
                    "body": page.body(),
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("a JSON object serialises")
                );
            } else {
                print!("{}", docs::render(page));
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// Everything the command does, with one way out for a failure.
///
/// `current_thread`, because a client sends one request and exits: the multi-thread runtime the
/// daemon needs would be several worker threads started to wait on a single socket, paid for on
/// every `mix` invocation in a shell prompt.
#[tokio::main(flavor = "current_thread")]
async fn run(args: Args) -> Result<ExitCode, Error> {
    let host = mixengine_platform::host();
    let root = home::resolve_root(args.home.as_deref(), host.as_ref())?;
    let endpoint = home::endpoint(&root)?;

    // Prepared either way, and not because it is free: deciding *whether* to autostart here rather
    // than inside the client is what keeps "this run may start a daemon" a property of the command
    // line and not of a code path somewhere below.
    let autostart = (!args.no_autostart).then(|| Autostart::for_home(&root));

    // Dialled by the command and not here. Every command there is today needs a daemon, but that is
    // a fact about `status` rather than about `mix` — `mix doctor` (T47) has to be able to describe
    // a home that has none — and connecting above the match would have made starting one the first
    // thing every future command did, whether or not it had anything to ask.
    match args.command {
        Command::Status => status(&endpoint, autostart.as_ref(), args.json).await,

        // Not `.await`ed and reaching no endpoint: this one runs a program and reads its stdout.
        Command::Storage => storage(&root, args.json),
        // **Never autostarts, whatever the flags say**, and it is the one command that decides this
        // for itself: starting a daemon in order to ask it to stop is a machine left exactly as it
        // was found, one process later. A home with nothing running is told so as the wire error for
        // a daemon that is not there, which is the same sentence every other command gets.
        Command::Daemon {
            command: DaemonCommand::Stop,
        } => daemon_stop(&endpoint, args.json).await,
        Command::Daemon {
            command: DaemonCommand::CredentialStore { store },
        } => daemon_credential_store(store, &endpoint, autostart.as_ref(), args.json).await,
        Command::Runtime { command } => {
            runtime(command, &endpoint, autostart.as_ref(), args.json).await
        }
        Command::Package { command } => {
            package(command, &endpoint, autostart.as_ref(), args.json).await
        }
        Command::Project { command } => {
            project(command, &endpoint, autostart.as_ref(), args.json).await
        }
        Command::Site { command } => site(command, &endpoint, autostart.as_ref(), args.json).await,
        Command::Blueprint { command } => {
            blueprint(command, &endpoint, autostart.as_ref(), args.json).await
        }
        Command::Extension { command } => {
            extension(command, &endpoint, autostart.as_ref(), args.json).await
        }
        Command::Database { command } => {
            database(command, &endpoint, autostart.as_ref(), args.json).await
        }
        Command::Metrics {
            watch,
            since,
            service,
            json,
        } => {
            metrics(
                &endpoint,
                autostart.as_ref(),
                watch,
                since.as_deref(),
                service.as_ref(),
                args.json || json,
            )
            .await
        }
        Command::Doctor {
            repair,
            yes,
            no_wait,
            bundle: wanted,
            out,
        } => match (repair, wanted) {
            (true, _) => self_repair(&endpoint, autostart.as_ref(), args.json, yes, no_wait).await,
            (false, true) => bundle(&endpoint, autostart.as_ref(), args.json, out.as_deref()).await,
            (false, false) => doctor(&endpoint, autostart.as_ref(), args.json).await,
        },
        Command::Disk => disk(&endpoint, autostart.as_ref(), args.json).await,
        Command::Home { command } => home(&endpoint, autostart.as_ref(), args.json, &command).await,
        Command::Cleanup {
            keep_logs,
            keep_cache,
            yes,
            no_wait,
        } => {
            cleanup(
                &endpoint,
                autostart.as_ref(),
                args.json,
                keep_logs,
                keep_cache,
                yes,
                no_wait,
            )
            .await
        }
        Command::Uninstall {
            dry_run,
            keep_home,
            keep_relocated,
            package,
            relocated,
            blocked,
            yes,
            no_wait,
        } => {
            let wanted = UninstallAsk {
                dry_run,
                keep_home,
                keep_relocated,
                package,
                relocated,
                blocked,
                yes,
                no_wait,
            };

            uninstall(&endpoint, autostart.as_ref(), args.json, wanted).await
        }
        Command::SelfUpdate { check, yes, finish } => {
            let asked = SelfUpdateAsk { check, yes, finish };
            self_update(&endpoint, autostart.as_ref(), args.json, asked).await
        }
        Command::Domain { command } => {
            domain(command, &endpoint, autostart.as_ref(), args.json).await
        }
        Command::Service { command } => {
            service(command, &endpoint, autostart.as_ref(), args.json).await
        }
        Command::Job { command } => job(command, &endpoint, autostart.as_ref(), args.json).await,
        Command::Path { command } => path(command, &endpoint, autostart.as_ref(), args.json).await,
        Command::Autostart { command } => {
            autostart_entry(command, &endpoint, autostart.as_ref(), args.json).await
        }
        Command::Elevation { command } => {
            elevation(command, &endpoint, autostart.as_ref(), args.json).await
        }
        Command::Cert { command } => cert(command, &endpoint, autostart.as_ref(), args.json).await,
        // Answered in `main`, above the home resolution this function opens with. The arm is here
        // because the match is exhaustive and should stay that way: a command added tomorrow must
        // be a compiler error here rather than a silent fall-through.
        Command::Docs { .. } => unreachable!("mix docs is answered before a home is resolved"),
    }
}

/// `mix uninstall` — roadmap task **T87**.
///
/// **The plan is asked for first and always**, even on the way to the real thing: what a person is
/// about to allow is what they are shown, which is T64's rule applied to the one command that cannot
/// be undone. `--dry-run` is that same call and then nothing else.
///
/// **Exit `3` is "something is in the way"** (the T182 design, D4): a process running from a
/// directory that would be removed. Distinct from `1`, so the Windows uninstaller can tell *close
/// this and try again* from *this failed*.
async fn uninstall(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    wanted: UninstallAsk,
) -> Result<ExitCode, Error> {
    let UninstallAsk {
        dry_run,
        keep_home,
        keep_relocated,
        package,
        relocated,
        blocked,
        yes,
        no_wait,
    } = wanted;

    let mut client = Client::connect(endpoint, autostart).await?;

    // T182b, D8: which process this is, by pid *and* the moment it began, so the wait at the end is
    // for this daemon and not for whatever the OS later hands the pid to. A daemon that will not say
    // is waited for the old way, by its endpoint going quiet.
    let daemon = ask::<DaemonStatus>(&mut client, rpc::method::DAEMON_STATUS, None)
        .await
        .ok()
        .and_then(|status| {
            mixengine_platform::process::started_at(status.pid)
                .ok()
                .flatten()
                .map(|began| (status.pid, began))
        });

    let query = UninstallQuery {
        keep_home,
        keep_relocated,
        // A plan raises nothing whatever this says; sent as it will be sent to the act, so the two
        // calls are visibly one question asked twice.
        grant: false,
        // T182e: the listing the uninstaller reads while its banner is up names folders and
        // nothing else, so it does not pay for reading the handle table.
        skip_holders: relocated,
        package,
    };

    let planned: UninstallReport = ask(
        &mut client,
        rpc::method::DAEMON_UNINSTALL_PLAN,
        encode(&query),
    )
    .await?;

    // T182, D9: the listing the Windows uninstaller reads — the directories `[paths]` moved out,
    // and not the tombstones beside them, which are garbage and not a choice.
    if relocated {
        for item in &planned.items {
            if item.id == ResidueId::RelocatedDirectory
                && !item.location.contains(mixengine_platform::tombstone::MARK)
            {
                emit(&format!("{}\n", item.location))?;
            }
        }

        return Ok(ExitCode::SUCCESS);
    }

    // T182e, D6: the listing the uninstaller's "close these first" page reads — one line per program
    // in the way, and nothing else. A question and not a refusal, so it succeeds either way. ASCII
    // only: the uninstaller reads it through `nsExec`, which spells a dash as three other characters.
    if blocked {
        for item in &planned.items {
            if matches!(item.outcome, Removal::Blocked { .. }) {
                emit(&format!("{}: {}\n", item.what, item.location))?;
            }
        }

        return Ok(ExitCode::SUCCESS);
    }

    // **One document per run under `--json`, and the plan is not it.** The plan is printed so that a
    // person can read what they are about to allow; a caller reading JSON is not being asked
    // anything, and two objects on one stdout is not JSON at all. Under `--dry-run` the plan *is*
    // the answer, so there it is printed either way.
    if dry_run || !json {
        emit(&rendered(json, &planned, || {
            render::uninstall_report(&planned)
        }))?;
    }

    if planned.blocked() {
        return Ok(ExitCode::from(BLOCKED));
    }

    if dry_run {
        return Ok(ExitCode::SUCCESS);
    }

    if !yes && !agreed_to_uninstall(&planned, keep_home, keep_relocated, json)? {
        // Saying no is an answer and not a failure — `mix elevation grant`'s rule. Nothing was
        // removed, so the same command works when the person is ready.
        return Ok(ExitCode::SUCCESS);
    }

    // Measured now, from the plan, while nothing is being removed: what `left_behind` says while it
    // waits is how much there is to remove (T182b).
    let removing = bytes_going(&planned);

    let started: JobSummary = ask(
        &mut client,
        rpc::method::DAEMON_UNINSTALL,
        encode(&UninstallQuery {
            // The plan above *is* the batch this allows, and it has just been shown or answered for
            // in advance — which is T64's rule met, so the prompt is raised inside the one job the
            // caller is already following.
            grant: true,
            // And the act always looks: something stuck that appeared since the plan refuses here.
            skip_holders: false,
            ..query
        }),
    )
    .await?;

    if no_wait {
        emit(&rendered(json, &started, || render::job_status(&started)))?;
        return Ok(ExitCode::SUCCESS);
    }

    let finished = follow(&mut client, started, json).await?;

    let Some(JobOutcome::Succeeded { result }) = finished.outcome.clone() else {
        emit(&rendered(json, &finished, || render::job_status(&finished)))?;
        return Ok(ExitCode::FAILURE);
    };

    let report: UninstallReport = serde_json::from_value(result).map_err(|error| {
        Error::new(
            ErrorCode::Internal,
            format!(
                "mix {} cannot read the uninstall report: {error}",
                env!("CARGO_PKG_VERSION")
            ),
        )
    })?;

    emit(&rendered(json, &report, || {
        render::uninstall_report(&report)
    }))?;

    let still_there = left_behind(&report, endpoint, daemon, removing).await;

    // **And the process itself** (the T182b design, D8). The endpoint goes quiet before the daemon
    // has finished removing its home and exited, and until it has, its image is still mapped: the
    // Windows uninstaller deleting `mixengined.exe` straight after this returned is what found that.
    let lingering = finished_uninstall(&report)
        && daemon.is_some_and(|(pid, began)| !process_has_ended(pid, began));

    // **Why, before what.** A daemon that could not remove its home says so in a note outside it
    // (T182b), and that sentence is the one that tells a person what to close.
    if let Some((pid, _)) = daemon.filter(|_| !lingering) {
        print_the_daemons_note(pid);
    }

    for path in &still_there {
        report_left(path);
    }

    // T182a: the program's own files, read back off the disk once the daemon has gone, like the
    // home. `mix` itself is one of them and goes on running from memory, as the window does.
    for path in package_left(&report) {
        report_left(&path);
    }

    if let Some((pid, _)) = daemon.filter(|_| lingering) {
        report_left(&format!("the daemon (pid {pid}) is still running"));
    }

    Ok(
        match report.left_behind() || !still_there.is_empty() || lingering {
            true => ExitCode::FAILURE,
            false => ExitCode::SUCCESS,
        },
    )
}

/// The `.pkg`'s paths still on disk after a run that says it removed the package — T182a.
fn package_left(report: &UninstallReport) -> Vec<String> {
    let removed = report.items.iter().any(|item| {
        item.id == ResidueId::Package && matches!(item.outcome, Removal::Removed { .. })
    });
    if !removed {
        return Vec::new();
    }
    mixengine_platform::install::package_paths()
        .map(|paths| {
            paths
                .binaries
                .into_iter()
                .chain([paths.bundle])
                .filter(|path| path.symlink_metadata().is_ok())
                .map(|path| path.display().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Did the daemon say this uninstall finished, which is what ends it (T182b, D1)?
fn finished_uninstall(report: &UninstallReport) -> bool {
    !report.items.iter().any(|item| {
        matches!(
            item.outcome,
            Removal::Failed { .. } | Removal::Enqueued { .. }
        )
    })
}

/// Wait up to [`PROCESS_GONE`] for the process that began at `began` to have ended, and say whether
/// it did.
///
/// **By pid and start time**, so a pid the OS reused for something else in the meantime reads as
/// ended rather than as this daemon still running.
fn process_has_ended(pid: u32, began: mixengine_platform::process::StartTime) -> bool {
    let deadline = std::time::Instant::now() + PROCESS_GONE;

    loop {
        if has_ended(pid, began) {
            return true;
        }

        if std::time::Instant::now() >= deadline {
            return false;
        }

        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Has the process that began at `began` ended? Asked once.
fn has_ended(pid: u32, began: mixengine_platform::process::StartTime) -> bool {
    !mixengine_platform::process::started_at(pid)
        .ok()
        .flatten()
        .is_some_and(|now| now == began)
}

/// How many bytes the directories this plan removes hold — the home, the relocated directories and
/// the window's folders that are `Planned` (T182b).
///
/// **Without following a link**, since a removal does not either, and with every unreadable entry
/// counted as nothing: this is a figure for a person, and a wrong one is only a wrong figure.
fn bytes_going(plan: &UninstallReport) -> u64 {
    plan.items
        .iter()
        .filter(|item| matches!(item.outcome, Removal::Planned { .. }))
        .filter(|item| {
            matches!(
                item.id,
                ResidueId::Home
                    | ResidueId::RelocatedDirectory
                    | ResidueId::WindowData
                    | ResidueId::WindowCache
            )
        })
        .map(|item| bytes_under(std::path::Path::new(&item.location)))
        .sum()
}

/// The size of everything under `path`, or of `path` itself when it is a file.
fn bytes_under(path: &std::path::Path) -> u64 {
    let mut total = 0;
    let mut waiting = vec![path.to_path_buf()];

    while let Some(current) = waiting.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&current) else {
            continue;
        };

        if metadata.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&current) {
                waiting.extend(entries.filter_map(Result::ok).map(|entry| entry.path()));
            }
        } else {
            total += metadata.len();
        }
    }

    total
}

/// Print, and then remove, what the daemon `pid` said about a removal it could not finish.
fn print_the_daemons_note(pid: u32) {
    let note = mixengine_platform::tombstone::note_for(pid);

    let Ok(said) = std::fs::read_to_string(&note) else {
        return;
    };

    for line in said.lines().filter(|line| !line.trim().is_empty()) {
        let _ = writeln!(std::io::stderr(), "{line}");
    }

    let _ = std::fs::remove_file(&note);
}

/// `mix self-update` — roadmap task **T88**.
///
/// **The daemon does the update and this waits for it.** `mix` may depend on `mixengine-platform`
/// and `mixengine-proto` and on nothing else, and verifying a signature, unpacking an archive and
/// swapping files are all `mixengine-core`'s. What has to outlive the daemon is the *client*, and
/// what it has to do afterwards is exactly one thing — start the new one — which is
/// [`Autostart::run`].
///
/// The sequence, and which process performs each step:
///
/// | # | Step | Who |
/// | --- | --- | --- |
/// | 1 | take the lock, so two of these cannot interleave | `mix` |
/// | 2 | `update.check` → version, notes, size, what will be restarted | daemon |
/// | 3 | prompt: *install / skip this version / remind me later* | `mix` |
/// | 4 | `update.apply`: download, verify, smoke-test, stop, swap, answer, exit | daemon |
/// | 5 | wait for the endpoint to stop answering | `mix` |
/// | 6 | `Autostart::run` — start the new daemon and return when it listens | `mix` |
/// | 7 | start the services that were stopped; clean up `.old` | new daemon |
///
/// **Step 6 deliberately does not open a [`Client`] afterwards.** The new daemon may speak a newer
/// protocol than the `mix` still running from the old image, and a protocol-mismatch error at the
/// end of a successful update would be the worst possible last line. `--detach` exiting zero *is*
/// the readiness probe, which is what `autostart.rs` already documents.
/// What `mix uninstall` exits with when a process is in the way — the T182 design, D4.
const BLOCKED: u8 = 3;

/// What `mix uninstall` was asked to do: its six flags, on [`SelfUpdateAsk`]'s precedent.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "six independent command-line switches, each named where it is read"
)]
struct UninstallAsk {
    dry_run: bool,
    keep_home: bool,
    keep_relocated: bool,
    package: bool,
    relocated: bool,
    blocked: bool,
    yes: bool,
    no_wait: bool,
}

/// What `mix self-update` was asked to do: its three flags.
#[derive(Debug, Clone, Copy)]
struct SelfUpdateAsk {
    check: bool,
    yes: bool,
    finish: bool,
}

async fn self_update(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    asked: SelfUpdateAsk,
) -> Result<ExitCode, Error> {
    let SelfUpdateAsk { check, yes, finish } = asked;

    // **Held for the whole of this and not for the call**, because what must not interleave is the
    // swap and the relaunch: two of these racing would have the second find `.old` files written by
    // the first and a daemon that is in the middle of being replaced. `platform::lock` is the
    // mechanism the daemon already uses for its own single-instance guarantee.
    let _lock = match check {
        true => None,
        false => update_lock()?,
    };

    let mut client = Client::connect(endpoint, autostart).await?;

    // The second half of a `.pkg` update — T88f. Refused by the daemon, in a sentence, until
    // Installer.app has put the new binaries on disk.
    if finish {
        let applied: UpdateApplied = ask(
            &mut client,
            rpc::method::UPDATE_FINISH,
            encode(&UpdateFinish {}),
        )
        .await?;

        return relaunch(client, endpoint, autostart, json, &applied).await;
    }

    let status: UpdateStatus = ask(
        &mut client,
        rpc::method::UPDATE_CHECK,
        encode(&UpdateCheck { force: true }),
    )
    .await?;

    if check {
        emit(&rendered(json, &status, || render::update_status(&status)))?;
        return Ok(ExitCode::SUCCESS);
    }

    // **One document per run under `--json`, and the offer is not it.** The status is printed here
    // so a person can read what they are about to agree to; a caller reading JSON is not being asked
    // anything, and two objects on one stdout is not JSON at all — `mix uninstall`'s rule, and its
    // reason. What a `--json` run gets instead is whichever of the two below turns out to be the
    // answer.
    if !json {
        emit(&render::update_status(&status))?;
    }

    // **Neither of these is a failure.** A machine that is up to date and a copy of MixEngine that
    // `apt` installed are both perfectly healthy, and the reason has just been printed — or is about
    // to be, as the one document a `--json` caller gets.
    //
    // A copy the `.pkg` installed reads `managed` and carries an installer, and is not refused (T88f).
    let refused = !status.offered
        || (matches!(status.placement, UpdatePlacement::Managed { .. })
            && status.installer.is_none());

    if refused || status.available.is_none() {
        if json {
            emit(&rendered(json, &status, || render::update_status(&status)))?;
        }

        return Ok(ExitCode::SUCCESS);
    }

    let release = status.available.clone().expect("checked one line above");

    if !yes {
        let answer = chosen_update(&release.version, json)?;

        if let Some(decision) = answer.decision() {
            let answered: UpdateStatus = ask(
                &mut client,
                rpc::method::UPDATE_DECIDE,
                encode(&UpdateDecide {
                    version: release.version.clone(),
                    decision,
                }),
            )
            .await?;

            emit(&rendered(json, &answered, || {
                render::update_status(&answered)
            }))?;
            return Ok(ExitCode::SUCCESS);
        }
    }

    // The `.pkg` path: the daemon downloads, checks and opens it, and keeps running. What finishes
    // the update is `--finish`, once the person has been through Installer.app (T88f, D8).
    if status.installer.is_some() {
        let handed: UpdateHandedOver = ask(
            &mut client,
            rpc::method::UPDATE_HAND_OVER,
            encode(&UpdateHandOver {
                version: release.version.clone(),
            }),
        )
        .await?;

        emit(&rendered(json, &handed, || {
            render::update_handed_over(&handed)
        }))?;
        return Ok(ExitCode::SUCCESS);
    }

    let applied: UpdateApplied = ask(
        &mut client,
        rpc::method::UPDATE_APPLY,
        encode(&UpdateApply {
            version: release.version.clone(),
        }),
    )
    .await?;

    relaunch(client, endpoint, autostart, json, &applied).await
}

/// What follows an update's answer, whichever way the binaries were replaced: wait for the old
/// daemon to go, and start the new one.
async fn relaunch(
    client: Client,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    applied: &UpdateApplied,
) -> Result<ExitCode, Error> {
    // The daemon answered and is on its way out. From here the connection is worthless: it belongs
    // to a process that has already committed to exiting.
    drop(client);

    emit(&rendered(json, applied, || render::update_applied(applied)))?;

    // **Bounded.** A daemon that has answered `update.apply` has already stopped its services and
    // cancelled its token, so anything longer than this is a supervised process refusing to die —
    // which does not stop the relaunch. Past the timeout the new daemon is started anyway: the
    // endpoint is a socket it will rebind or a pipe it will re-create, and a user left with no
    // daemon at all is worse than a second one failing to start and saying so.
    if !Client::gone(endpoint, RELAUNCH_WAIT).await {
        let _ = writeln!(
            std::io::stderr(),
            "the old daemon has not stopped answering yet; starting the new one anyway"
        );
    }

    let Some(autostart) = autostart else {
        // `--no-autostart` on the one command whose whole ending is starting a daemon. Said rather
        // than silently skipped: the update happened, and the machine is one command short of
        // having a daemon again.
        let _ = writeln!(
            std::io::stderr(),
            "MixEngine {} is installed. --no-autostart was given, so start the daemon yourself: \
             mix status",
            applied.to
        );

        return Ok(ExitCode::SUCCESS);
    };

    if let Err(error) = autostart.run() {
        report_update_rollback(applied, &error);
        return Ok(ExitCode::FAILURE);
    }

    Ok(ExitCode::SUCCESS)
}

/// How long `mix self-update` waits for the daemon it just replaced to stop answering.
const RELAUNCH_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// The lock `mix self-update` and MixLab's updater both take — T187, spec D7.
const UPDATE_LOCK: &str = "update.lock";

/// Take the update lock beside the binaries, or say who has it.
///
/// **Beside the binaries, not in the home**, since T187. A lock belongs to what is being changed,
/// and what an update changes is the directory holding `mixengined`, which several homes can share
/// — and which MixLab's own updater, knowing nothing of homes, swaps too.
fn update_lock() -> Result<Option<mixengine_platform::lock::Lock>, Error> {
    let directory = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .ok_or_else(|| Error::new(ErrorCode::Io, "cannot tell which directory mix runs from"))?;
    update_lock_in(&directory)
}

/// [`update_lock`] in a directory the caller names, so a test needs no install.
///
/// **A directory this account cannot write holds no lock**, and the answer is `None`: nobody here
/// swaps it (a `.pkg`, a `.deb`), so there is nothing to keep two updates apart over.
fn update_lock_in(
    directory: &std::path::Path,
) -> Result<Option<mixengine_platform::lock::Lock>, Error> {
    let path = directory.join(UPDATE_LOCK);

    match mixengine_platform::lock::Lock::acquire(&path) {
        Ok(mixengine_platform::lock::Acquired::Held(lock)) => Ok(Some(lock)),
        Ok(mixengine_platform::lock::Acquired::Taken(holder)) => Err(Error::new(
            ErrorCode::PreconditionFailed,
            format!("another update is running ({holder})"),
        )
        .with_hint(
            "wait for it to finish, whether it is mix self-update or MixLab; two updates at once \
             would interleave their swaps",
        )),
        Err(mixengine_platform::Error::Io { source, .. })
            if matches!(
                source.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(crate::error::to_wire(&error)),
    }
}

/// What somebody answered to *there is a newer MixEngine*.
///
/// Three answers and not two, because *skip this version* and *remind me later* are different
/// decisions and a yes/no that meant both would be a prompt nobody could answer correctly —
/// `confirm::Choice`'s own reasoning, which T78 added for this shape of question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    /// Install it now.
    Install,
    /// Never offer this version again.
    Skip,
    /// Ask again in a few days.
    Later,
}

impl Choice {
    /// What `update.decide` is told, or [`None`] for the answer that decides nothing because it
    /// installs.
    fn decision(self) -> Option<UpdateDecision> {
        match self {
            Self::Install => None,
            Self::Skip => Some(UpdateDecision::Skip),
            Self::Later => Some(UpdateDecision::Later),
        }
    }
}

/// Ask, and read one line back.
///
/// # Errors
///
/// [`ErrorCode::PreconditionFailed`] when there is nobody to ask — `--json`, or a standard input at
/// end of file — naming the flag that says yes in advance. A script that could not be asked is told
/// what to do rather than defaulted into replacing its own binaries.
fn chosen_update(version: &str, json: bool) -> Result<Choice, Error> {
    if json {
        return Err(unanswered());
    }

    let question = format!("\ninstall MixEngine {version}? [i]nstall / [s]kip / [l]ater: ");

    match confirm::ask_update(&question) {
        Some(choice) => Ok(choice),
        None => Err(unanswered()),
    }
}

/// The one path that leaves a machine worse than it found it, reported so it can be undone by hand.
///
/// **`mix` does not attempt the rollback itself.** The files are the daemon's to place, `mix` may not
/// link `mixengine-core`, and a client that moved binaries around on a failure it does not
/// understand is a client doing something to the machine. What it can do honestly is name the two
/// files and the move that undoes the swap.
fn report_update_rollback(applied: &UpdateApplied, error: &Error) {
    let mut stderr = std::io::stderr();
    let suffix = std::env::consts::EXE_SUFFIX;

    let _ = writeln!(
        stderr,
        "\nMixEngine {} is installed, and the new daemon would not start: {error}",
        applied.to
    );
    let _ = writeln!(
        stderr,
        "the binaries it replaced are still in {}, each under its own name with .old on the end:",
        applied.directory
    );

    for name in &applied.replaced {
        let _ = writeln!(stderr, "  {name}{suffix}.old");
    }

    let _ = writeln!(
        stderr,
        "renaming each of those back over the file beside it puts this machine as it was."
    );
}

/// How long `mix uninstall` waits for the daemon it just ended.
///
/// It has a shutdown budget of its own out of `config.toml` and then removes several directories, so
/// this is generous rather than tight: what a short wait would buy is a false *"still there"* on a
/// slow machine, which is the one wrong answer this command must not give.
const GOING: std::time::Duration = std::time::Duration::from_secs(60);

/// How long a finished uninstall waits for the daemon's *process* to end once its endpoint has —
/// the T182b design, D8. Removing a home with runtimes in it is the slow part, and a machine with a
/// large `runtimes/` is exactly the one where giving up early would be a false *"still running"*.
const PROCESS_GONE: std::time::Duration = std::time::Duration::from_secs(120);

/// The paths the daemon said it was taking with it that are still on disk once it has gone.
///
/// **A process cannot measure the removal of the directory it is running out of**, so the daemon
/// names these and the client reads them back — which is what makes this command's exit code mean
/// *nothing is left behind* rather than *the daemon said so* (the T87 design, D9).
///
/// **And the paths are waited for, not read once.** The endpoint stops answering the moment the
/// daemon commits to going, which is a long way before it has stopped its services, checkpointed the
/// database, dropped the home lock and removed these directories. Reading at that moment reported
/// every path as left behind on a run where nothing was — measured on CI's Windows runner on
/// 2026-09-04, where the removal was complete a fraction of a second later and the command still
/// exited non-zero.
///
/// A daemon that is still there when the budget runs out has not removed them, and what this answers
/// is the honest thing: they are still there.
///
/// **A daemon whose process has ended will not remove anything more**, so once `daemon` has gone the
/// paths are read one last time and answered, rather than waited on for the rest of the budget
/// (T182b: a home held open by File Explorer kept the uninstaller waiting a minute for nothing).
///
/// **And it says that it is waiting** (T182b). Removing a home of a gigabyte takes a minute on
/// Windows, and a person watching the uninstaller saw nothing move for all of it: one line when the
/// wait begins, with how much is going, and one every `STILL` after.
async fn left_behind(
    report: &UninstallReport,
    endpoint: &Endpoint,
    daemon: Option<(u32, mixengine_platform::process::StartTime)>,
    removing: u64,
) -> Vec<String> {
    /// How often a line says the removal is still going.
    const STILL: std::time::Duration = std::time::Duration::from_secs(10);

    /// How often the paths are looked at while the daemon finishes.
    const STEP: std::time::Duration = std::time::Duration::from_millis(100);

    let going: Vec<&str> = report
        .items
        .iter()
        .filter(|item| matches!(item.outcome, Removal::OnExit { .. }))
        .map(|item| item.location.as_str())
        .collect();

    // T182, D1: a finished uninstall ends the daemon even when nothing of its own is going, and
    // this waits for that too — a script reading the exit code should not find the daemon still up.
    let finished = !report.items.iter().any(|item| {
        matches!(
            item.outcome,
            Removal::Failed { .. } | Removal::Enqueued { .. }
        )
    });

    if going.is_empty() {
        if finished && !Client::gone(endpoint, GOING).await {
            report_left("this home's daemon is still running after a finished uninstall");
        }

        return Vec::new();
    }

    let _ = writeln!(
        std::io::stderr(),
        "{}",
        render::uninstall_removing(going.len(), removing)
    );

    if !Client::gone(endpoint, GOING).await {
        report_left("this home's daemon is still running, so nothing of its own has been removed");
    }

    let began = tokio::time::Instant::now();
    let deadline = began + GOING;
    let mut said = began;

    loop {
        // Asked before the paths are read, so the reading after it is one the daemon cannot change.
        let ended = daemon.is_some_and(|(pid, began)| has_ended(pid, began));

        // A tombstone beside a path is that path half-removed (T182, D6), and counts as left.
        let mut left: Vec<String> = going
            .iter()
            .flat_map(|path| {
                let path = std::path::Path::new(path);

                path.exists()
                    .then(|| path.to_path_buf())
                    .into_iter()
                    .chain(mixengine_platform::tombstone::tombstones_beside(path))
                    .map(|left| left.display().to_string())
            })
            .collect();
        left.sort();
        left.dedup();

        if left.is_empty() || ended || tokio::time::Instant::now() + STEP >= deadline {
            return left;
        }

        if said.elapsed() >= STILL {
            said = tokio::time::Instant::now();
            let _ = writeln!(
                std::io::stderr(),
                "{}",
                render::uninstall_still_removing(began.elapsed())
            );
        }

        tokio::time::sleep(STEP).await;
    }
}

/// One line on stderr about something that is still on this machine.
///
/// Standard error, beside the report rather than inside it: stdout carries the daemon's answer, and
/// this is what the client found afterwards.
fn report_left(what: &str) {
    let _ = writeln!(std::io::stderr(), "still there: {what}");
}

/// Ask, once, in front of the plan that was just printed.
///
/// **`--json` never asks**, on `confirmed`'s rule and for its reason: a caller reading JSON has
/// nobody at the keyboard by construction, and answering for them either way is worse than refusing
/// — yes would remove a home nobody agreed to lose, and no would be a decline the caller could not
/// tell from an uninstall that happened.
fn agreed_to_uninstall(
    planned: &UninstallReport,
    keep_home: bool,
    keep_relocated: bool,
    json: bool,
) -> Result<bool, Error> {
    if json {
        return Err(unanswered());
    }

    // The relocated directories are only worth a clause of their own when there are some (T182, D2).
    let moved = planned.items.iter().any(|item| {
        item.id == ResidueId::RelocatedDirectory
            && !item.location.contains(mixengine_platform::tombstone::MARK)
    });

    let question = match (keep_home, moved, keep_relocated) {
        (true, false, _) => {
            "undo everything MixEngine has done to this machine, and keep this home?"
        }
        (false, false, _) => {
            "remove MixEngine from this machine, including this home and every database in it?"
        }
        (true, true, true) => {
            "undo everything MixEngine has done to this machine, and keep this home and the \
             directories moved out of it?"
        }
        (true, true, false) => {
            "undo everything MixEngine has done to this machine, keep this home, and remove the \
             directories moved out of it?"
        }
        (false, true, true) => {
            "remove MixEngine from this machine, including this home and every database in it, but \
             keep the directories moved out of it?"
        }
        (false, true, false) => {
            "remove MixEngine from this machine, including this home, the directories moved out of \
             it, and every database in them?"
        }
    };

    match confirm::ask(&format!("\n{question} [y/N] ")) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            // On stderr, beside the question it answers. Stdout carries what the command was asked
            // for, which was the plan above.
            let _ = writeln!(
                std::io::stderr(),
                "nothing was removed; {} row(s) are still where they were",
                planned.items.len()
            );

            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every command this binary offers is a command clap can build.**
    ///
    /// `debug_assert` is clap's own check for the mistakes a type system cannot catch — two
    /// arguments sharing an id, a positional after a variadic one — and it runs at *parse* time,
    /// which means without this test the first person to meet one is whoever typed the command.
    /// T77 met exactly that: `mix blueprint capture --name` collided with the `name` field of the
    /// flattened project argument, and every unit test in this crate passed while the command
    /// panicked on the first real run.
    #[test]
    fn every_command_is_one_clap_can_build() {
        use clap::CommandFactory as _;

        Args::command().debug_assert();
    }

    /// **T187, spec D7.** The lock is the file beside the binaries that MixLab's updater takes too,
    /// and a second taker is refused with the holder named.
    #[test]
    fn the_update_lock_is_the_file_beside_the_binaries() {
        let directory = tempfile::tempdir().unwrap();

        let held = update_lock_in(directory.path()).unwrap();
        assert!(held.is_some());
        assert!(directory.path().join(UPDATE_LOCK).exists());

        let refused = update_lock_in(directory.path()).unwrap_err();
        assert_eq!(refused.code, ErrorCode::PreconditionFailed);
        assert!(refused.message.contains(&std::process::id().to_string()));
    }
}
