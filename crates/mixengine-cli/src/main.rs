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
mod by_hand;
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

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory as _, Parser, Subcommand};
use mixengine_proto::{Error, ServiceId};

use autostart::Autostart;
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
use commands::self_update::{SelfUpdateAsk, self_update};
use commands::service::{ServiceCommand, service};
use commands::site::{SiteCommand, site};
use commands::status::{status, storage};
use commands::uninstall::{UninstallAsk, uninstall};
use output::report;
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
}
