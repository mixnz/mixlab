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
mod confirm;
mod docs;
mod error;
mod home;
mod render;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

use clap::{CommandFactory as _, Parser, Subcommand};
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    AnswerSubject, AutostartReport, BlueprintApplied, BlueprintApply, BlueprintApplyResponse,
    BlueprintCapture, BlueprintImport, BlueprintList, BlueprintPlan, BlueprintSummary,
    BundleReport, CaRotateReport, CaStatus, CaUninstallReport, CertIssue, CertIssueReport,
    CertStatusQuery, CertStatusReport, CleanupQuery, CleanupReport, DaemonShutdown, DaemonStatus,
    DatabaseAccount, DatabaseClientQuery, DatabaseClientReport, DatabaseCreate,
    DatabaseCredentials, DatabaseCredentialsQuery, DatabaseHandoff, DatabaseOpen,
    DiagnosticsBundle, DiskCategory, DiskUsage, DiskUsageQuery, Disposition, DoctorRepair,
    DoctorReport, DomainAdd, DomainRemove, DomainStatusQuery, DomainStatusReport, ElevationDrop,
    ElevationStatus, Error, ErrorCode, ExtensionAvailable, ExtensionCatalogue, ExtensionChange,
    ExtensionChoice, ExtensionConsent, ExtensionId, ExtensionInspect, ExtensionInspection,
    ExtensionInstall, ExtensionList, ExtensionOrigin, ExtensionPlan, ExtensionPlanRequest,
    ExtensionRemoval, ExtensionTarget, ExtensionUninstall, FrontEndReport, FrontEndServer,
    FrontEndSwitch, IdleReport, InstalledExtensions, JobFilter, JobId, JobList, JobOutcome,
    JobQuery, JobState, JobSummary, JobWait, LogFrame, MetricsFrame, MetricsHistory, Millis,
    MismatchAnswer, PackageCatalogue, PackageFilter, PackageInstall, PackageList, PackageRemoval,
    PackageTarget, PackageVersion, PathReport, PendingOpId, PlanAction, Priority, ProjectCreate,
    ProjectDetail, ProjectExport, ProjectList, ProjectQuery, ProjectRef, ProjectRemoval,
    ProjectUpdate, Reclaim, Remedy, Removal, RepairReport, Requirement, Requirements,
    ResetCredential, ResidueId, ResolvedRuntime, ResourceLimits, RouteTarget, RuntimeCatalogue,
    RuntimeFilter, RuntimeInstall, RuntimeKind, RuntimeList, RuntimeQuestion, RuntimeRemoval,
    RuntimeSummary, RuntimeTarget, RuntimeUninstall, SaveResources, SaveResourcesSet,
    ScaffoldConsent, ServiceAutostartSet, ServiceCreate, ServiceCreation, ServiceDelete, ServiceId,
    ServiceIdleSet, ServiceLimitsReport, ServiceLimitsSet, ServiceList, ServiceQuery,
    ServiceRemoval, ServiceRole, ServiceSummary, ServiceTarget, ServiceWalk, SignatureCheck,
    SiteCreate, SiteCreation, SiteDetail, SiteKind, SiteList, SiteListQuery, SiteQuery, SiteRef,
    SiteRemoval, SiteRoute, SiteShare, SiteSharing, SiteState, SiteUpdate, StorageReport,
    Timestamp, UninstallQuery, UninstallReport, UpdateApplied, UpdateApply, UpdateCheck,
    UpdateDecide, UpdateDecision, UpdateFinish, UpdateHandOver, UpdateHandedOver, UpdatePlacement,
    UpdateStatus, VersionAnswer, VersionConstraint, rpc,
};

use autostart::Autostart;
use client::Client;

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
    /// Defaults to the OS convention, exactly as `mixengined` resolves it — the two have to agree
    /// or they would be talking about different daemons.
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
    /// `--runtimes`, `--packages`, `--data` or `--logs` — and that choice is free only until the
    /// first runtime, package or service is installed, because from then on where they are is
    /// recorded against each of them rather than worked out.
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
    /// With no topic it lists them. It talks to no daemon and needs no home — the pages are
    /// compiled into this binary, which is what makes `mix docs install` answer on a machine where
    /// nothing starts. The same pages are published at <https://mixnz.github.io/mixlab/>, as
    /// HTML for a person and as plain Markdown for a program.
    Docs {
        /// Which topic. Omit it to list them.
        topic: Option<String>,

        /// Which language: `en` or `vi`. An unrecognised one is answered in English.
        #[arg(long, value_name = "CODE", env = "MIXENGINE_LANG")]
        lang: Option<String>,

        /// Print the whole command reference as Markdown, instead of a topic.
        ///
        /// This is what `docs/guide/en/cli.md` is generated from, by `packaging/docs.sh
        /// --reference` — so the reference cannot describe a flag this binary does not have. It is
        /// English only, because the definitions it is generated from are.
        ///
        /// It does not conflict with `--lang`: that flag carries `MIXENGINE_LANG`, and a variable
        /// somebody exported once should not be able to refuse a command.
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
    /// One reading and out by default. `--watch` opens the live stream, which is also what puts the
    /// daemon on its one-second rate — it samples once a minute when nobody is looking.
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
        /// shown, and then granted once — one prompt for the whole batch.
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
        /// what this machine is, any crash reports this home has recorded, and the tail of the log
        /// — with whatever was deliberately left out named beside them.
        #[arg(long, conflicts_with = "repair")]
        bundle: bool,

        /// Copy the archive here as well. Only with `--bundle`.
        #[arg(long, requires = "bundle", value_name = "FILE")]
        out: Option<PathBuf>,
    },

    /// Update MixEngine itself.
    ///
    /// Checks for a newer release and shows its version, its size and what changed before asking. On
    /// yes, the daemon downloads it, checks the signature, runs the new `mixengined` once to be sure
    /// this machine will start it, stops what it is supervising, replaces the binaries and exits —
    /// and this command starts the new daemon, which starts your services again.
    ///
    /// `mixengine-elevate` is never replaced here. It runs as root, and updating it needs an
    /// elevation prompt of its own.
    ///
    /// A copy of MixEngine that a package manager installed is not updated by this: it says so, and
    /// names the directory.
    ///
    /// On a Mac that installed MixLab from the .pkg, the next .pkg is downloaded, checked and opened
    /// in Installer.app instead, and nothing is stopped. When the installation is done,
    /// `mix self-update --finish` restarts MixEngine on the new version.
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
    /// Five categories — runtimes, data, logs, certs and cache — plus everything else. Each row says
    /// what would reclaim it: your databases never, a runtime only through `mix runtime uninstall`,
    /// the certificates only by losing HTTPS until they are issued again, and the logs and the cache
    /// by `mix cleanup`.
    Disk,

    /// Take back what is safe to lose: rotated log files and the download cache.
    ///
    /// Nothing else, whatever `mix disk` says the total is. Your databases, your installed runtimes,
    /// your certificates, the log files being written right now and this home's crash reports are
    /// all out of reach — this command matches file names, it does not sweep the home.
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
    /// Undoes everything MixEngine has written outside its own directory — the hosts block, the DNS
    /// routing, the port grant, the certificate authority, the firewall rules, the login entry, your
    /// PATH entry, the privileged helper and its audit log — and then removes the directory itself.
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

        /// With `--dry-run`: print only the relocated directories, one path per line.
        ///
        /// For a program to read — the Windows uninstaller shows them before it asks anything.
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

/// `mix cert …` — one subcommand per `cert.*` method, and nothing that is not one.
///
/// **`ca-status` and not `status`.** `docs/features/tls.md` gives the short name to the per-site
/// diagnostics with a live TLS handshake, which is roadmap task **T53**, and names this command's
/// siblings `ca-uninstall` and `ca-rotate`. Taking the short name here would mean renaming it later,
/// or giving one command two unrelated jobs.
#[derive(Debug, Subcommand)]
enum CertCommand {
    /// Say what this home's certificate authority is: its name, its fingerprint, how long it has.
    ///
    /// **Not whether this machine trusts it.** That is a question about the operating system's own
    /// certificate stores rather than about the authority, this build does not yet ask it, and
    /// nothing printed here implies an answer to it.
    // Roadmap task T49 is what will answer it, and `mix cert ca-install` is where that will live.
    // Kept out of the help text above on purpose: a task number means nothing to whoever typed
    // `--help`, and clap prints every line of a doc comment.

    /// Give a site the certificate its names need, or every HTTPS site one.
    ///
    /// Idempotent: a certificate that still covers the right names, has more than thirty days left
    /// and was signed by the authority this home has now is left exactly as it is.
    Issue {
        /// One site, by any of its domains. Every HTTPS site when this is left out.
        #[arg(long, value_name = "DOMAIN")]
        site: Option<String>,
    },
    /// Say whether each site's padlock is green, by asking the server rather than the disk.
    ///
    /// Opens a real TLS connection to this home's front end for every site and reports the
    /// certificate it presents — which is the only thing a browser ever sees, and the only way to
    /// notice a server still holding a certificate that was replaced underneath it.
    ///
    /// Reads only. Nothing is issued, nothing is installed and nothing is reloaded.
    Status {
        /// One site, by any of its domains. Every site when this is left out.
        #[arg(long, value_name = "DOMAIN")]
        site: Option<String>,
    },
    CaStatus,

    /// Replace this home's certificate authority with a new one.
    ///
    /// Destructive: every browser holding a cached chain under the old authority stops accepting
    /// it, and every site's certificate is reissued. Nothing is replaced unless this machine can be
    /// made to trust the new one — declining the prompt leaves this home exactly as it was.
    CaRotate {
        /// Answer the confirmation in advance, for a script with nobody at the keyboard.
        #[arg(long)]
        yes: bool,

        /// Start the work and print the job, rather than waiting for it to finish.
        #[arg(long = "no-wait")]
        no_wait: bool,
    },

    /// Take this home's certificate authority out of every store that trusts it.
    ///
    /// Leaves the certificate and its key on disk, and leaves every site's certificate alone —
    /// `mix doctor --repair` puts the trust back. Removing it from the system store needs an
    /// administrator; the browser databases do not.
    CaUninstall {
        /// Answer the confirmation in advance, for a script with nobody at the keyboard.
        #[arg(long)]
        yes: bool,

        /// Start the work and print the job, rather than waiting for it to finish.
        #[arg(long = "no-wait")]
        no_wait: bool,
    },
}

/// `mix project …` — one subcommand per `project.*` method, and nothing that is not one.
///
/// `import` is an **alias** on `create` rather than a seventh subcommand: both produce one row, and
/// what makes a create an import is the `mixengine.toml` already lying in the directory rather than
/// a different call. An alias is the same subcommand under a second name, so the rule above holds.
#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Register a directory as a project.
    ///
    /// With no `--name` and no `--pin`, whatever the `mixengine.toml` in that directory says is
    /// used — which is what adopting a colleague's checkout is.
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
    /// query — nothing in MixEngine records which database a project uses.
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
struct WhichProject {
    /// The project's name. Defaults to whichever project the current directory is in.
    #[arg(value_name = "PROJECT")]
    name: Option<String>,
}

/// `mix extension …` — one subcommand per `extension.*` method this build has.
#[derive(Debug, clap::Subcommand)]
enum ExtensionCommand {
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

/// `mix blueprint …` — one subcommand per `blueprint.*` method this build has.
///
/// `export` and `delete` are deliberately absent: a blueprint's rendering is already on disk at
/// `blueprints/<slug>.toml`, so exporting one is copying a file rather than a daemon method, and
/// nothing has asked to delete one.
#[derive(Debug, Subcommand)]
enum BlueprintCommand {
    /// Write down what a project is made of.
    Capture {
        /// What to file it under: lower-case letters, digits and hyphens.
        ///
        /// Positional rather than `--name`, because the flattened project argument is already
        /// called `name` and clap refuses two arguments under one id — found by running the command
        /// rather than by a test, which is why it is worth a sentence here.
        #[arg(value_name = "NAME")]
        name: String,

        /// Which project. Defaults to whichever project the current directory is in.
        #[arg(long, value_name = "PROJECT")]
        project: Option<String>,

        /// What it is for.
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,

        /// Replace the blueprint already filed under this name.
        #[arg(long)]
        overwrite: bool,
    },

    /// Take in a blueprint somebody else wrote.
    ///
    /// **What arrives without a signature the gallery key vouches for is untrusted for good** —
    /// nothing raises that afterwards, and it is what decides how loudly its `[scaffold]` command
    /// has to be agreed to before it runs.
    Import {
        /// The manifest to read.
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// What to file it under. Defaults to the file's own name, without `.toml`.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,

        /// The detached signature to check it against. Defaults to `<FILE>.minisig` if that exists.
        #[arg(long, value_name = "FILE")]
        signature: Option<PathBuf>,

        /// Replace the blueprint already filed under that name.
        #[arg(long)]
        overwrite: bool,
    },

    /// Every blueprint this home holds.
    List,

    /// What applying one would do.
    Apply {
        /// Which blueprint.
        #[arg(value_name = "BLUEPRINT")]
        blueprint: String,

        /// What the new project is called, and what `{project}` becomes.
        #[arg(long, value_name = "NAME")]
        project: String,

        /// Where it goes. Defaults to a directory named for the project, in the current one.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,

        /// Stop after planning, and print the plan.
        ///
        /// Sent as it is typed rather than insisted on here: whether this build can carry an apply
        /// out is the daemon's to say, and a client that refused to ask would be holding a rule of
        /// its own.
        #[arg(long)]
        dry_run: bool,

        /// Answer every version question by installing what the blueprint asks for.
        #[arg(long, conflicts_with = "use_installed")]
        install_missing: bool,

        /// Install a web server too, where this home has none.
        ///
        /// A home with no front end serves no site, and nothing installs one by itself. With this,
        /// a blueprint that declares a site plans the default web server as well — and a home that
        /// already has one, Caddy or nginx, is left alone.
        #[arg(long)]
        with_front_end: bool,

        /// Start the services this apply creates whenever MixEngine starts.
        ///
        /// Only what it creates: a server this home already had is left as its owner set it. Read
        /// and changed afterwards with `mix service autostart`.
        #[arg(long)]
        autostart: bool,

        /// Start the services this project needs once the apply is done.
        ///
        /// These are the project's sites, the database and pool they use, and the front end they
        /// are reached through, not every service of this home. They start after the permission
        /// prompt, so every site's name already resolves when it comes up.
        //
        // Roadmap task T125: which services those are is the daemon's answer and not this
        // command's. Until T125 the only set a client could ask for was every service this home
        // declares.
        #[arg(long)]
        start: bool,

        /// Answer every version question by using what this machine already has.
        #[arg(long)]
        use_installed: bool,

        /// Run the blueprint's own `[scaffold]` command without asking first.
        ///
        /// For a blueprint the gallery signed. An unsigned one takes the other flag, and neither
        /// covers the other: a script that runs somebody's unsigned command should say so on the
        /// line that does it.
        #[arg(long, conflicts_with = "run_untrusted_scaffold")]
        run_scaffold: bool,

        /// Run an **untrusted** blueprint's own `[scaffold]` command without asking first.
        ///
        /// Nothing vouches for what this runs. The command is still printed before it starts.
        #[arg(long)]
        run_untrusted_scaffold: bool,

        /// Spend the one elevation prompt at the end without asking first.
        #[arg(long)]
        grant: bool,

        /// Install what the blueprint's releases need of this machine first, without asking — the
        /// Microsoft Visual C++ Redistributable, on Windows. Windows still asks for approval. Its own
        /// flag rather than `--yes`: an apply asks several questions, and one flag answering all of
        /// them would answer ones nobody read.
        #[arg(long)]
        install_prerequisites: bool,

        /// Apply even though MixEngine judges this machine lacks something the releases need.
        #[arg(long)]
        ignore_requirements: bool,
    },
}

/// `mix database …` — one subcommand per `database.*` method this build has.
///
/// `list` and `drop` are deliberately absent: nothing has asked for either, and dropping a database
/// is a decision with data behind it rather than the other half of a pair.
#[derive(Debug, Subcommand)]
enum DatabaseCommand {
    /// Make a database and the account that reaches it.
    ///
    /// The instance is started if it is not running. Nothing prints the password: it is put in this
    /// machine's credential store, and what is printed is where.
    Create {
        /// Which instance: `mariadb@main`, `postgres@shop`.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// The database's name.
        #[arg(long, value_name = "NAME")]
        name: String,

        /// The account's name. The database's own when nobody says.
        #[arg(long, value_name = "ACCOUNT")]
        user: Option<String>,

        /// Choose the account's password instead of generating one.
        ///
        /// With a value, that is the password. Without one, `mix` prompts and reads one line from
        /// standard input — so this also works piped: `echo secret | mix database create … --password`.
        /// Not shown on any command line MixEngine itself runs afterwards: it goes into this
        /// machine's credential store the same way a generated password does.
        ///
        /// With an existing account of ours, this changes what is stored — and the server is
        /// realigned to it, the same way it already is when a password drifts.
        #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "VALUE")]
        password: Option<String>,
    },

    /// Where this instance could be opened, and with what.
    ///
    /// MixLab, the window this MixEngine installed, when the install has one; nothing on the
    /// headless archive.
    ///
    /// Reads only: starts nothing, opens nothing. "No client" is an answer, not a failure.
    Client {
        /// Which instance: `mariadb@main`, `redis@main`.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,
    },

    /// The password MixEngine holds for one account.
    ///
    /// Reads only: starts nothing. Prints the password itself — the last line of the plain
    /// rendering is the value alone, so a script can read it with `tail -1`. This is the only
    /// `mix database` command whose whole purpose is to print a credential.
    Credentials {
        /// Which instance: `mariadb@main`, `postgres@shop`.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// The account to read. The server's administrator when nobody says.
        #[arg(long, value_name = "ACCOUNT")]
        user: Option<String>,
    },

    /// Open this instance in MixLab, the window this MixEngine installed.
    ///
    /// MixLab need not be running: a copy already open takes the connection as a new tab and this
    /// command says so, and one that is not open is started.
    ///
    /// The instance is started if it is not running. The account's password is read from this
    /// machine's credential store at that moment and handed to the client in its own environment —
    /// never printed, never put in an argument. Exits 1 on an install with no window — the headless
    /// archive — and says so.
    Open {
        /// Which instance.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// The account to sign in as. The server's administrator when nobody says.
        #[arg(long, value_name = "ACCOUNT")]
        user: Option<String>,

        /// A database to open at.
        #[arg(long, value_name = "NAME")]
        database: Option<String>,
    },
}

/// `mix domain …` — one subcommand per `domain.*` method, and nothing that is not one.
#[derive(Debug, Subcommand)]
enum DomainCommand {
    /// Give a site one more name.
    ///
    /// The new name is an alias: the site's primary domain is unchanged, because that is what its
    /// canonical URL and — from the HTTPS work — its certificate are named after.
    Add {
        /// The name to add.
        #[arg(value_name = "DOMAIN")]
        domain: String,

        /// Any of the site's existing domains.
        #[arg(long, value_name = "DOMAIN")]
        site: String,

        /// Accept `.local`, which belongs to mDNS and works until somebody plugs in a printer.
        #[arg(long = "i-know")]
        accept_risky_tld: bool,
    },

    /// Take one name away.
    ///
    /// Refused for a site's last domain and for its primary; `mix site update` reorders, and the
    /// first `--domain` it is given becomes the primary.
    Remove {
        /// The name to take away. It names its own site.
        #[arg(value_name = "DOMAIN")]
        domain: String,
    },

    /// What actually happens to a name, as four facts that can fail one at a time.
    Status {
        /// One name, or every name this home declares.
        #[arg(value_name = "DOMAIN")]
        domain: Option<String>,
    },
}

/// `mix site …` — one subcommand per `site.*` method, and nothing that is not one.
#[derive(Debug, Subcommand)]
enum SiteCommand {
    /// Declare a site under a project.
    ///
    /// With nothing but a project named, whatever the `[site]` and `[[services]]` in that
    /// project's `mixengine.toml` say is used — which is what adopting a colleague's site is.
    #[command(alias = "import")]
    Create {
        /// The project. Defaults to whichever project the current directory is in.
        #[arg(long, value_name = "PROJECT")]
        project: Option<String>,

        /// A domain. The first is the primary; repeat for aliases. Defaults to `<project>.test`.
        #[arg(long = "domain", value_name = "DOMAIN")]
        domains: Vec<String>,

        /// What is served, relative to the project's root. Defaults to the root itself.
        #[arg(long, value_name = "DIR")]
        doc_root: Option<String>,

        /// What serves it.
        #[arg(long, value_enum, value_name = "KIND")]
        kind: Option<SiteKindArg>,

        /// Where a `reverse-proxy` forwards to.
        #[arg(long, value_name = "URL", required_if_eq("kind", "reverse-proxy"))]
        upstream: Option<String>,

        /// The port a `node-app` listens on.
        #[arg(long, value_name = "PORT", required_if_eq("kind", "node-app"))]
        port: Option<u16>,

        /// The php-fpm pool a `php-fpm` site uses. Defaults to whatever this directory resolves to.
        #[arg(long, value_name = "SERVICE", value_parser = service_id)]
        pool: Option<ServiceId>,

        /// Forward a path prefix to an address: `/api=http://127.0.0.1:3003/xyz`. Repeatable.
        ///
        /// The upstream's path, when it has one, replaces the matched prefix.
        #[arg(long = "proxy", value_name = "PATH=URL")]
        proxy: Vec<String>,

        /// Answer a path prefix with a php-fpm pool: `/admin=php-fpm@8.3.33`, or `/admin` alone for
        /// whatever this project resolves to. Repeatable.
        #[arg(long = "php", value_name = "PATH[=POOL]")]
        php: Vec<String>,

        /// Serve a path prefix from a directory: `/assets=dist`. The prefix is stripped.
        /// Repeatable.
        #[arg(long = "files", value_name = "PATH=DIR")]
        files: Vec<String>,

        /// Remove every route this site has.
        #[arg(long, conflicts_with_all = ["proxy", "php", "files"])]
        no_routes: bool,

        /// A service the site declares, as `mariadb@main`. May be given more than once.
        #[arg(long = "service", value_name = "SERVICE", value_parser = service_id)]
        services: Vec<ServiceId>,

        /// Declare HTTPS for it. Phase 5 is what acts on this.
        #[arg(long)]
        https: Option<bool>,

        /// Redirect the plaintext address to the HTTPS one. Needs `--https true`.
        #[arg(long)]
        https_redirect: Option<bool>,

        /// Accept a `.local` domain, which belongs to mDNS.
        #[arg(long = "i-know")]
        accept_risky_tld: bool,
    },

    /// List the sites this home has been told about.
    List {
        /// Only this project's.
        #[arg(long, value_name = "PROJECT")]
        project: Option<String>,
    },

    /// Show one, with its domains, its pool and its services.
    Show {
        #[command(flatten)]
        site: WhichSite,
    },

    /// Change what a site is.
    ///
    /// `--domain` and `--service` **replace** rather than add to what the site had: giving neither
    /// changes neither.
    Update {
        #[command(flatten)]
        site: WhichSite,

        /// A domain. The first is the primary; repeat for aliases. Replaces the whole list.
        #[arg(long = "domain", value_name = "DOMAIN")]
        domains: Vec<String>,

        /// A new doc root.
        #[arg(long, value_name = "DIR")]
        doc_root: Option<String>,

        /// A new kind.
        #[arg(long, value_enum, value_name = "KIND")]
        kind: Option<SiteKindArg>,

        /// Where a `reverse-proxy` forwards to.
        #[arg(long, value_name = "URL", required_if_eq("kind", "reverse-proxy"))]
        upstream: Option<String>,

        /// The port a `node-app` listens on.
        #[arg(long, value_name = "PORT", required_if_eq("kind", "node-app"))]
        port: Option<u16>,

        /// The php-fpm pool.
        #[arg(long, value_name = "SERVICE", value_parser = service_id)]
        pool: Option<ServiceId>,

        /// Forward a path prefix to an address: `/api=http://127.0.0.1:3003/xyz`. Replaces the
        /// whole list, together with `--php` and `--files`.
        #[arg(long = "proxy", value_name = "PATH=URL")]
        proxy: Vec<String>,

        /// Answer a path prefix with a php-fpm pool: `/admin=php-fpm@8.3.33`, or `/admin` alone for
        /// whatever this project resolves to. Replaces the whole list.
        #[arg(long = "php", value_name = "PATH[=POOL]")]
        php: Vec<String>,

        /// Serve a path prefix from a directory: `/assets=dist`. The prefix is stripped. Replaces
        /// the whole list.
        #[arg(long = "files", value_name = "PATH=DIR")]
        files: Vec<String>,

        /// Remove every route this site has.
        #[arg(long, conflicts_with_all = ["proxy", "php", "files"])]
        no_routes: bool,

        /// A service the site declares. Replaces the whole list.
        #[arg(long = "service", value_name = "SERVICE", value_parser = service_id)]
        services: Vec<ServiceId>,

        /// Whether HTTPS is declared.
        #[arg(long)]
        https: Option<bool>,

        /// Redirect the plaintext address to the HTTPS one. Needs HTTPS enabled, before or with
        /// this same update.
        #[arg(long)]
        https_redirect: Option<bool>,

        /// Serve it, or stop serving it.
        #[arg(long, value_enum, value_name = "STATE")]
        state: Option<SiteStateArg>,

        /// Accept a `.local` domain.
        #[arg(long = "i-know")]
        accept_risky_tld: bool,
    },

    /// Let the local network reach this site, and print a QR code for it.
    ///
    /// This site only: every other site keeps answering on loopback alone. The certificate gains
    /// the LAN address, and one administrator prompt asks for the firewall rule.
    Share {
        #[command(flatten)]
        site: WhichSite,

        /// Which network to share on, by the name this machine gives it.
        ///
        /// Needed only where more than one is up — MixEngine refuses to choose rather than putting
        /// a site on a network you did not mean, and names the candidates when it does.
        #[arg(long, value_name = "NAME")]
        interface: Option<String>,

        /// How long to share for: `30s`, `90m`, `2h`, `1d`, or a bare number of seconds.
        ///
        /// Measured from when the share began, so asking for a length shorter than the site has
        /// already been shared for is refused rather than ending it on the spot. Off by default: a
        /// share with no `--for` lasts until you unshare it or this machine leaves the network.
        #[arg(long = "for", value_name = "LENGTH", value_parser = for_seconds)]
        r#for: Option<u64>,
    },

    /// Take it back off the local network.
    ///
    /// Removes the firewall rule, rebinds to loopback and reissues the certificate without the
    /// address. A site that is not shared is left as it is.
    Unshare {
        #[command(flatten)]
        site: WhichSite,
    },

    /// Serve this site.
    ///
    /// A flag and a re-render: the front end is told to read its configuration again. Nothing is
    /// started — a site is not a process, and the services it uses have states of their own.
    Start {
        #[command(flatten)]
        site: WhichSite,
    },

    /// Stop serving this site, keeping the declaration.
    Stop {
        #[command(flatten)]
        site: WhichSite,
    },

    /// Forget a site. The files are left exactly as they are.
    Delete {
        #[command(flatten)]
        site: WhichSite,
    },
}

/// Which site a command is about.
///
/// **The default is the directory you are in**, on [`WhichProject`]'s rule: with no argument `mix`
/// sends the working directory and the daemon walks up to the nearest registered project, then to
/// its site. A project holding several is refused there, naming them — which is a sentence this
/// client only prints.
#[derive(Debug, clap::Args)]
struct WhichSite {
    /// Any of the site's domains. Defaults to the site of whichever project you are in.
    #[arg(value_name = "DOMAIN")]
    domain: Option<String>,
}

/// What serves a site, as a person types it.
///
/// `SiteKindArg` rather than `Kind`: that name is already the runtime filter three commands take,
/// and one word meaning two things in one file is a rename waiting to go wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
enum SiteKindArg {
    /// PHP through a php-fpm pool.
    PhpFpm,
    /// Files, and nothing running.
    Static,
    /// Everything forwarded to an address you already have listening.
    ReverseProxy,
    /// A node process you run, on a port.
    NodeApp,
}

/// Whether the web server should serve a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
enum SiteStateArg {
    /// Serve it.
    Enabled,
    /// Declare it and do not serve it.
    Disabled,
}

/// `mix path …` — one subcommand per `path.*` method.
///
/// **None of the three takes an argument.** There is one directory this can be about, `<root>/bin`,
/// and the daemon is what knows where it is — a `--dir` here would be a command for putting
/// arbitrary directories on somebody's PATH, which is not a thing MixEngine does.
#[derive(Debug, Subcommand)]
enum PathCommand {
    /// Say whether a new terminal would find this home's commands.
    Status,

    /// Fill `<root>/bin` and put it on this user's PATH.
    ///
    /// Idempotent, and it says which of the two it did: a profile that already carries the line is
    /// left exactly as it is.
    Install,

    /// Take `<root>/bin` back off this user's PATH.
    ///
    /// The commands stay in the directory — they are inside the home, and removing the home is what
    /// removes them.
    Uninstall,

    /// Look for a tool you installed into a runtime, now.
    ///
    /// The daemon does this on its own every couple of seconds, so `npm install -g yarn` makes
    /// `yarn` a command without anybody asking. This is for the moment in between, and for a home
    /// whose `[bin] rescan_seconds` was slowed down.
    Rescan,
}

/// `mix autostart …` — one subcommand per `autostart.*` method.
///
/// **None of the three takes an argument**, for `mix path`'s reason above: there is one entry per
/// user and one home this can be about, and an argument would be a command for registering
/// arbitrary programs to run at somebody's login.
///
/// **`mix autostart` and not `mix daemon autostart`.** `daemon.*` is about the daemon that is
/// running; a logon task, a LaunchAgent and a systemd user unit outlive every daemon that ever
/// registered them.
#[derive(Debug, Subcommand)]
enum AutostartCommand {
    /// Say whether this home's daemon starts when you log in.
    Status,

    /// Register it.
    ///
    /// Does **not** start the daemon: there is one running, and it is the one answering this. What
    /// it changes is what happens at your next login. Idempotent, and it says which of the two it
    /// did.
    Enable,

    /// Remove it.
    ///
    /// Does **not** stop the daemon that is running — turning off "start at login" is not a request
    /// to lose the daemon you are using.
    Disable,
}

/// `mix elevation …` — one subcommand per `elevation.*` method, and nothing that is not one.
///
/// **There is no `mix elevation enqueue`, and there will not be.** What needs an administrator's
/// permission is decided by the operation that needs it — creating a site, issuing a certificate —
/// and a command that let a person put an arbitrary privileged operation in the queue would be a
/// client deciding what runs as root.
#[derive(Debug, Subcommand)]
enum ElevationCommand {
    /// Say what is waiting for permission, and what each of them will change.
    Status,

    /// Ask once, for everything that is waiting.
    ///
    /// One prompt covers the whole list. Saying no is a normal answer: the list stays, and this
    /// command can be run again later.
    //
    // One prompt for the whole queue: `docs/decisions/0005-on-demand-elevation.md` calls asking
    // inside a loop a defect.
    Grant {
        /// Say yes in advance, instead of being asked.
        ///
        /// What it skips is the question, never the screen: every operation and what it will change
        /// is printed either way. It exists for the caller that cannot be asked — a script, a CI
        /// step, anything with no terminal behind it — and for `--json`, which has no way to answer.
        #[arg(long)]
        yes: bool,

        /// Answer as soon as the prompt has been raised, without waiting for it.
        #[arg(long)]
        no_wait: bool,
    },

    /// Forget an operation that is waiting, so it is never asked about again.
    Drop {
        /// Which one, as `mix elevation status` numbers them.
        op: Option<i64>,

        /// Forget all of them.
        ///
        /// Its own flag rather than "drop with nothing named": emptying the queue by typing less is
        /// exactly the mistake worth making impossible.
        #[arg(long, conflicts_with = "op")]
        all: bool,
    },
}

/// `mix runtime …` — one subcommand per `runtime.*` method, and nothing that is not one.
///
/// The two listings are two commands rather than one with a flag, because they answer two different
/// questions — what is here, and what could be — and the second one reaches the network while the
/// first reads a table. A `--available` on the first would hide that difference behind a flag.
#[derive(Debug, Subcommand)]
enum RuntimeCommand {
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

        /// Install what this version needs of the machine first without asking — the Microsoft
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

    /// Make one installed version the one its kind resolves to.
    Default {
        #[command(flatten)]
        runtime: Which,
    },

    /// Which extensions an installed build loads.
    ///
    /// Under `runtime` rather than as `mix php ext …`, which is what
    /// `docs/features/runtime-versions.md` wrote: a per-language command family for one language
    /// is a noun this CLI would then owe every other runtime.
    Ext {
        #[command(subcommand)]
        command: ExtCommand,
    },

    /// Say which installed version a directory uses, and why that one.
    ///
    /// The question `php -v` answers by running, asked without running anything — and the reason is
    /// the point of it: what a person wants when the version surprises them is which of the four
    /// sources decided it.
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
enum ExtCommand {
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
struct WhichPhp {
    /// The version, exactly as it is installed. Defaults to the one `php` resolves to here.
    #[arg(long = "php", value_name = "VERSION", value_parser = runtime_version)]
    version: Option<PackageVersion>,
}

/// `mix package …` — one subcommand per `package.*` method.
///
/// **Not runtimes.** PHP and Node are `mix runtime`, which has a default version and a shim behind
/// it; these are the servers a *service* is an instance of. What a package becomes once it is
/// installed is `mix service create`.
#[derive(Debug, Subcommand)]
enum PackageCommand {
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
    },

    /// Download and install one version.
    Install {
        #[command(flatten)]
        package: WhichPackage,

        /// Return once the daemon has accepted the install, rather than once it has finished.
        #[arg(long)]
        no_wait: bool,

        /// Install what this version needs of the machine first without asking — the Microsoft
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
    /// Refused while a service is an instance of it, naming the services — `mix service delete` is
    /// what frees it, and deleting a service keeps its data directory.
    Uninstall {
        #[command(flatten)]
        package: WhichPackage,
    },
}

/// Which package a listing is about, or every package.
#[derive(Debug, clap::Args)]
struct Named {
    /// Only this package. Every one of them when it is left out.
    #[arg(long, value_name = "PACKAGE")]
    package: Option<String>,
}

/// Which package a command acts on, which is the same question twice.
#[derive(Debug, clap::Args)]
struct WhichPackage {
    /// Which package, as `mix package available` lists it.
    #[arg(value_name = "PACKAGE")]
    package: String,

    /// Which version, exactly as `mix package available` lists it.
    #[arg(value_name = "VERSION", value_parser = runtime_version)]
    version: PackageVersion,
}

/// Which kind a listing is about, or every kind.
#[derive(Debug, clap::Args)]
struct Kind {
    /// Only this language. Every one of them when it is left out.
    #[arg(long, value_name = "RUNTIME", value_parser = runtime_kind)]
    kind: Option<RuntimeKind>,
}

/// Which runtime a command acts on, which is the same question three times.
#[derive(Debug, clap::Args)]
struct Which {
    /// Which language.
    #[arg(value_name = "RUNTIME", value_parser = runtime_kind)]
    kind: RuntimeKind,

    /// Which version, exactly as `mix runtime available` lists it.
    ///
    /// Required, and deliberately not a constraint like `8.3`, even now that the daemon can read
    /// one: choosing a version from a range is *resolution*, it answers with what is installed, and
    /// none of these three commands is asking that question — an install picking `8.3`'s newest
    /// would be picking between versions none of which are here yet. `mix runtime resolve` is where
    /// a range belongs.
    #[arg(value_name = "VERSION", value_parser = runtime_version)]
    version: PackageVersion,
}

/// `mix job …` — one subcommand per `job.*` method.
#[derive(Debug, Subcommand)]
enum JobCommand {
    /// List what this home has run, newest first.
    List {
        /// Only jobs in this state.
        #[arg(long, value_name = "STATE", value_parser = job_state)]
        state: Option<JobState>,

        /// At most this many.
        #[arg(long, short = 'n', value_name = "COUNT", default_value_t = 50)]
        limit: u32,
    },

    /// Describe one job.
    Status {
        /// The job, as `mix job list` numbers them.
        #[arg(value_name = "JOB")]
        job: i64,
    },

    /// Wait for a job to finish.
    ///
    /// **Answers when the job ends or when the wait runs out**, and the second is not an error: what
    /// comes back is the job as it stands. The exit status is what a script branches on — non-zero
    /// for a job that failed, and for one that has not finished yet.
    Wait {
        /// The job to wait for.
        #[arg(value_name = "JOB")]
        job: i64,

        /// How long to wait. The daemon caps what it grants.
        #[arg(long, value_name = "SECONDS", default_value_t = 30)]
        timeout: u64,
    },

    /// Ask a running job to stop.
    ///
    /// Cancellation is cooperative, so what comes back may still say `running`: the work ends when
    /// it next looks. Cancelling a job that has already ended is not an error.
    Cancel {
        /// The job to cancel.
        #[arg(value_name = "JOB")]
        job: i64,
    },

    /// What a job printed.
    ///
    /// Only a job that runs another program prints anything. Today that is an apply running a
    /// blueprint's `[scaffold]` command; other jobs report progress and a result, and show nothing
    /// here. The lines stay in memory while the daemon keeps the job, so read them while it runs.
    //
    // Roadmap task T78a.
    Logs {
        /// The job, as `mix job list` numbers them.
        #[arg(value_name = "JOB")]
        job: i64,

        /// Keep printing as the job prints.
        #[arg(long, short = 'f')]
        follow: bool,

        /// How many of the lines already printed to begin with.
        #[arg(long, short = 'n', value_name = "COUNT", default_value_t = 200)]
        lines: usize,
    },
}

/// `mix daemon …` — the daemon as a thing in itself, rather than as what answers about services.
///
/// `status` is deliberately not here and stays `mix status`: it is the first command anybody types,
/// and moving it would be renaming the one command that already exists to make room for a namespace.
#[derive(Debug, Subcommand)]
enum DaemonCommand {
    /// Stop the services this home is running, then stop the daemon.
    Stop,
}

/// `mix service …` — one subcommand per `service.*` method, and nothing that is not one.
#[derive(Debug, Subcommand)]
enum ServiceCommand {
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

        /// Stop it once nothing has used it for this long — `30m`, `2h`, `90m`.
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
    /// With no flag: read it. `mix autostart` is a different question — whether this *machine*
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
    /// separate argument for it: `mariadb@main` is an instance of `mariadb`, and a package that runs
    /// only once — Caddy — is named without an `@` at all.
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
    /// Takes the row and the configuration generated from it. **Never the data** — that is somebody's
    /// databases, and the answer names the directory that was left so nobody has to go looking.
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
    /// For a server that refuses the password MixEngine holds for it — `ERROR 1045`, or `password
    /// authentication failed`. A database keeps its own copy of that password inside its data
    /// directory and this machine's credential store holds the other; they are written together
    /// when the service first starts and can only come apart afterwards. Once they have, nothing
    /// can log in to put them back, because every way of changing the copy inside the directory
    /// needs the password that was lost.
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
}

/// What can be done to a service's limits.
#[derive(Debug, clap::Subcommand)]
enum LimitsCommand {
    /// Replace every limit on this service.
    ///
    /// **Every field, not only the ones named.** A flag left out is that field's default — uncapped,
    /// or ordinary priority — so `set --cpu 50` clears a memory ceiling that was there. That is
    /// deliberate: composing a partial change would mean reading the current value and merging it,
    /// which is business logic a client may not hold. What this does instead is print all three
    /// fields of the result, so a cleared limit is on the screen.
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
enum FrontEndServerArg {
    /// Caddy, which is what this project picks when there is a choice — ADR 0004.
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
enum PriorityArg {
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
struct Target {
    /// The service to act on. Every declared service when it is left out.
    ///
    /// Naming one does not mean acting on one — a plan is the transitive set — and what the daemon
    /// walked comes back in the answer.
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
struct StartTarget {
    #[command(flatten)]
    target: Target,

    /// Every service one project needs, instead of one or all.
    ///
    /// Its sites' databases and caches, the php-fpm pool they name, and the front end they are
    /// reached through — worked out by the daemon, which is the only thing that can: the set is
    /// `site_service_links`, and no client may derive it.
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
             {server} and starts it — no site is reachable while that happens.\n",
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
         MixEngine is already waiting for is asked for at the same time — `mix elevation status` \
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

/// A service id from the command line, refused here rather than at the daemon.
///
/// Not the client deciding anything — [`ServiceId::parse`] is the daemon's own rule, from the crate
/// that owns the vocabulary — it is only where the answer is cheapest: a typo should not start a
/// daemon and travel over a socket to be told it is a typo.
fn service_id(value: &str) -> Result<ServiceId, String> {
    ServiceId::parse(value).map_err(|error| error.to_string())
}

/// A duration from the command line, as a whole number of minutes.
///
/// **Refused rather than rounded when it is not one.** `services.idle_minutes` stores minutes, so
/// `--after 90s` would have to become either one minute or two, and a setting that quietly becomes
/// something else is worse than one that says it cannot. `Millis::parse` is the daemon's own
/// syntax — `30m`, `2h`, `500ms` — rather than a second reading of it here.
///
/// Zero is refused too, and it has its own flag: `--after 0m` reads as *stop it immediately* and
/// means the opposite, so `--never` is what a person types for that.
fn idle_after(value: &str) -> Result<u32, String> {
    let millis = mixengine_proto::Millis::parse(value)
        .ok_or_else(|| format!("{value:?} is not a duration — write it as `30m`, `2h` or `90m`"))?;

    if millis.is_zero() {
        return Err(
            "an idle policy of zero would stop the service on the next sweep; `--never` is how you              switch idle stopping off"
                .to_owned(),
        );
    }

    let minutes = millis.0 / 60_000;

    if minutes * 60_000 != millis.0 {
        return Err(format!(
            "{value:?} is not a whole number of minutes, and that is what MixEngine stores — write              it as minutes or hours"
        ));
    }

    u32::try_from(minutes).map_err(|_| format!("{value:?} is longer than MixEngine can store"))
}

/// A runtime kind from the command line, refused here for [`service_id`]'s reason.
///
/// The list is in the message because this is a closed set of four and a typo is the whole of what
/// can go wrong: `mix runtime install pph 8.3.33` should say what the four are rather than send
/// somebody to `--help`.
fn runtime_kind(value: &str) -> Result<RuntimeKind, String> {
    RuntimeKind::parse(value).ok_or_else(|| {
        format!(
            "{value:?} is not a runtime MixEngine manages — it knows {}",
            RuntimeKind::ALL.map(RuntimeKind::as_str).join(", ")
        )
    })
}

/// A version from the command line. [`PackageVersion::parse`] is the daemon's own rule.
fn runtime_version(value: &str) -> Result<PackageVersion, String> {
    PackageVersion::parse(value).map_err(|error| error.to_string())
}

/// A version *or a range* from the command line, which only `mix runtime resolve` takes.
fn version_constraint(value: &str) -> Result<VersionConstraint, String> {
    VersionConstraint::parse(value).map_err(|error| error.to_string())
}

/// A job state from the command line, for `mix job list --state`.
fn job_state(value: &str) -> Result<JobState, String> {
    JobState::parse(value).ok_or_else(|| {
        format!(
            "{value:?} is not a job state — a job is {}",
            JobState::ALL.map(JobState::as_str).join(", ")
        )
    })
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
            relocated,
            blocked,
            yes,
            no_wait,
        } => {
            let wanted = UninstallAsk {
                dry_run,
                keep_home,
                keep_relocated,
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

/// `mix project …`: one call, one rendering.
async fn project(
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

/// A name if one was typed, and this directory if none was.
///
/// **Not a default this client invents**: the path is sent as it stands and the daemon does the
/// walking, which is the same answer the shim gets.
/// `mix site …` — ask, and render what came back.
///
/// **Nothing is decided here.** No domain is validated, no doc root is made relative and no kind is
/// defaulted: all of that is the daemon's, and a `mix` that could refuse what the GUI could not
/// would be the first bug `CLAUDE.md` names.
/// `mix metrics` — roadmap task **T71**.
///
/// Three readings of one namespace and one command, because they are three tenses of one question:
/// what is it costing now (`metrics.snapshot`), what has it been costing (`metrics.history`), and
/// what is it costing from here on (`GET /metrics`).
///
/// **`--watch` is written out as it arrives**, on `mix service logs --follow`'s reasoning: a stream
/// has no last message, and a buffer that filled until it ended would print nothing at all. It is
/// also the only thing in this repository that opens that route, which is what keeps the daemon's
/// one-second rate exercised rather than merely implemented.
async fn metrics(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    watch: bool,
    since: Option<&str>,
    service: Option<&ServiceId>,
    json: bool,
) -> Result<ExitCode, Error> {
    let subject = service.map(|id| format!("service:{id}"));
    let mut client = Client::connect(endpoint, autostart).await?;

    if watch {
        return watch_metrics(&mut client, json).await;
    }

    if let Some(since) = since {
        let history: MetricsHistory = ask(
            &mut client,
            rpc::method::METRICS_HISTORY,
            encode(&serde_json::json!({
                "subject": subject,
                "since": since_moment(since)?,
            })),
        )
        .await?;

        emit(&rendered(json, &history, || {
            render::metrics_history(&history, SystemTime::now())
        }))?;

        return Ok(ExitCode::SUCCESS);
    }

    let frame: MetricsFrame = ask(
        &mut client,
        rpc::method::METRICS_SNAPSHOT,
        encode(&serde_json::json!({})),
    )
    .await?;

    // Narrowed here rather than by the daemon: `metrics.snapshot` takes no parameters, because one
    // reading measures every subject anyway and a filter on the wire would be a second way of asking
    // for the same pass.
    let frame = match subject {
        None => frame,
        Some(wanted) => MetricsFrame {
            samples: frame
                .samples
                .into_iter()
                .filter(|sample| sample.subject.to_string() == wanted)
                .collect(),
            ..frame
        },
    };

    emit(&rendered(json, &frame, || render::metrics_frame(&frame)))?;

    Ok(ExitCode::SUCCESS)
}

/// `mix metrics --watch`: print each reading as it arrives, until interrupted.
async fn watch_metrics(client: &mut Client, json: bool) -> Result<ExitCode, Error> {
    let mut stream = client.stream("/metrics").await?;

    while let Some(frame) = stream.next::<MetricsFrame>().await? {
        emit(&rendered(json, &frame, || {
            format!(
                "{}
",
                render::metrics_frame(&frame)
            )
        }))?;
    }

    Ok(ExitCode::SUCCESS)
}

/// How far back `--since 30m` reaches, as a moment this machine's clock names.
///
/// **Resolved here rather than sent as a duration**, because the API takes moments: a client that
/// sent "30m" would be asking the daemon to apply its own clock to a word, and the two clocks are
/// the same one — the endpoint is a local socket.
fn since_moment(value: &str) -> Result<Timestamp, Error> {
    let (count, unit) = value.split_at(
        value
            .find(|character: char| !character.is_ascii_digit())
            .ok_or_else(|| since_refusal(value))?,
    );

    let count: i64 = count.parse().map_err(|_| since_refusal(value))?;

    let millis = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return Err(since_refusal(value)),
    };

    let Timestamp(now) = Timestamp::from_system_time(SystemTime::now());

    Ok(Timestamp(
        now.saturating_sub(count.saturating_mul(millis).abs()),
    ))
}

/// What `--since` says when it cannot read what was typed.
fn since_refusal(value: &str) -> Error {
    Error::new(
        ErrorCode::InvalidArgument,
        format!("`--since {value}` is not a length of time"),
    )
    .with_hint("write it as a number and one of s, m, h, d — for example `--since 2h`")
}

/// `mix doctor` — roadmap task **T47a**.
async fn doctor(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let report: DoctorReport = ask(
        &mut client,
        rpc::method::DAEMON_DOCTOR,
        encode(&serde_json::json!({})),
    )
    .await?;

    emit(&rendered(json, &report, || render::doctor(&report)))?;

    // **The exit code is the report and not the call.** A doctor that exits 0 because it managed to
    // ask cannot be used in a script, and the shell is where the second question gets asked.
    Ok(if report.has_a_problem() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// `mix doctor --bundle` — roadmap task **T93**.
///
/// **This one exits zero when the archive was written**, unlike bare `mix doctor`, whose exit code
/// is the report. The deliverable of this command is the file: a bundle is taken *because*
/// something is wrong, so a non-zero exit every time would make the ordinary success read as a
/// failure to the person watching their terminal and to whatever wrapped it. What the exit code
/// answers here is "did I get the archive"; the answer to "is this machine well" is inside it,
/// where the person asking will be looking.
async fn bundle(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    out: Option<&Path>,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let report: BundleReport = ask(
        &mut client,
        rpc::method::DAEMON_BUNDLE,
        encode(&DiagnosticsBundle::default()),
    )
    .await?;

    // **The copy is the client's and never the daemon's.** A destination on the method would be a
    // way for any local caller to have the daemon write a file anywhere that daemon can reach — so
    // the archive lands in the home, and moving it out is done by whoever asked, with their own
    // permissions.
    if let Some(destination) = out {
        std::fs::copy(&report.path, destination).map_err(|source| {
            Error::new(
                ErrorCode::Io,
                format!(
                    "the bundle was written to {} but could not be copied to {}: {source}",
                    report.path,
                    destination.display()
                ),
            )
        })?;
    }

    emit(&rendered(json, &report, || render::bundle(&report, out)))?;

    Ok(ExitCode::SUCCESS)
}

/// `mix doctor --repair` — roadmap task **T47b**.
///
/// **Two calls, and T64 is the reason.** The first repairs what needs no privilege and *queues* what
/// does; then the batch is read, shown and answered before the second raises the prompt — which is
/// the rule `mix elevation grant` obeys, over the same queue, for the same reason. `--yes` collapses
/// the two into one call by saying so on the command line, which is a person answering in advance
/// rather than a client skipping the question.
async fn self_repair(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    yes: bool,
    no_wait: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let repaired: RepairReport = ask(
        &mut client,
        rpc::method::DAEMON_DOCTOR_REPAIR,
        encode(&DoctorRepair { grant: yes }),
    )
    .await?;

    emit(&rendered(json, &repaired, || render::repair(&repaired)))?;

    // **The exit code is the report and not the call**, as `mix doctor`'s is: everything found was
    // either repaired or queued, or it was not.
    let outcome = match repaired.left_something_undone() {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    };

    let started = match repaired.granting {
        // `--yes`: the daemon raised it because the person said so before it ran.
        Some(started) => started,

        None => {
            let waiting: ElevationStatus =
                ask(&mut client, rpc::method::ELEVATION_STATUS, None).await?;

            // Nothing to grant is the ordinary end of a repair: either nothing needed an
            // administrator, or a machine that cannot prompt at all left the queue where it was.
            if waiting.pending.is_empty() {
                return Ok(outcome);
            }

            if !confirmed(&waiting, json)? {
                // Saying no is an answer and not a failure — the same rule as `mix elevation grant`.
                // Nothing was dropped, so the same command works when the person is ready.
                return Ok(outcome);
            }

            ask(&mut client, rpc::method::ELEVATION_GRANT, None).await?
        }
    };

    if no_wait {
        emit(&rendered(json, &started, || render::job_status(&started)))?;
        return Ok(outcome);
    }

    let finished = follow(&mut client, started, json).await?;
    emit(&rendered(json, &finished, || render::job_status(&finished)))?;

    // A grant that failed is a machine still holding what it held, whatever the repairs did.
    Ok(match render::job_succeeded(&finished) {
        true => outcome,
        false => ExitCode::FAILURE,
    })
}

/// `mix disk` — roadmap task **T96**.
///
/// **`refresh: true`, always.** The daemon keeps a reading for a minute so that a dashboard
/// re-reading on every event does not walk `runtimes/` each time; somebody who typed a command is
/// asking about now, and a stale figure under a command they just ran would read as a command that
/// did nothing.
async fn disk(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let usage: DiskUsage = ask(
        &mut client,
        rpc::method::DAEMON_DISK_USAGE,
        encode(&DiskUsageQuery { refresh: true }),
    )
    .await?;

    emit(&rendered(json, &usage, || render::disk_usage(&usage)))?;

    Ok(ExitCode::SUCCESS)
}

/// `mix cleanup` — roadmap task **T96**.
///
/// **What is about to go is read and shown first**, on `mix uninstall`'s rule: `daemon.disk_usage`
/// is the plan half of this pair, and a command that deleted before anybody had seen the number
/// would be the *"measured and deleted in one breath"* T96 exists to refuse.
async fn cleanup(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    keep_logs: bool,
    keep_cache: bool,
    yes: bool,
    no_wait: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    if !yes {
        let usage: DiskUsage = ask(
            &mut client,
            rpc::method::DAEMON_DISK_USAGE,
            encode(&DiskUsageQuery { refresh: true }),
        )
        .await?;

        // Printed and not held back under `--json`, because the question below is refused there
        // anyway: what a person is about to allow is what they are shown.
        emit(&render::disk_usage(&usage))?;

        if !agreed_to_cleanup(&usage, keep_logs, keep_cache, json)? {
            // Saying no is an answer and not a failure — `mix uninstall`'s rule. Nothing went, so
            // the same command works when the person is ready.
            return Ok(ExitCode::SUCCESS);
        }
    }

    let started: JobSummary = ask(
        &mut client,
        rpc::method::DAEMON_CLEANUP,
        encode(&CleanupQuery {
            keep_logs,
            keep_cache,
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

    let report: CleanupReport = serde_json::from_value(result).map_err(|error| {
        Error::new(
            ErrorCode::Internal,
            format!(
                "mix {} cannot read the cleanup report: {error}",
                env!("CARGO_PKG_VERSION")
            ),
        )
    })?;

    emit(&rendered(json, &report, || render::cleanup_report(&report)))?;

    Ok(match report.left_behind() {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    })
}

/// Ask, once, in front of the table that was just printed — roadmap task **T96**.
///
/// **Nothing to take is a yes.** A command that asked *"remove nothing?"* would be one people learn
/// to answer without reading, which is the habit the question exists to prevent.
///
/// **`--json` never asks**, on [`agreed_to_uninstall`]'s rule and for its reason.
fn agreed_to_cleanup(
    usage: &DiskUsage,
    keep_logs: bool,
    keep_cache: bool,
    json: bool,
) -> Result<bool, Error> {
    let asked_for: u64 = usage
        .categories
        .iter()
        .filter(|category| match category.id {
            DiskCategory::Logs => !keep_logs,
            DiskCategory::Cache => !keep_cache,
            _ => false,
        })
        .map(|category| match category.reclaim {
            Reclaim::ByCleanup { bytes, .. } => bytes,
            _ => 0,
        })
        .sum();

    if asked_for == 0 {
        return Ok(true);
    }

    if json {
        return Err(unanswered());
    }

    match confirm::ask("\nremove them? nothing else is touched. [y/N] ") {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            // On stderr, beside the question it answers. Stdout carried the table above.
            let _ = writeln!(std::io::stderr(), "nothing was removed");

            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
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
            "wait for it to finish, whether it is mix self-update or MixLab; two updates at once              would interleave their swaps",
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

/// `mix domain` — roadmap task **T46**.
async fn domain(
    command: DomainCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        DomainCommand::Add {
            domain,
            site,
            accept_risky_tld,
        } => {
            let add = DomainAdd {
                site: SiteRef::Domain(site),
                domain,
                accept_risky_tld,
            };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::DOMAIN_ADD, encode(&add)).await?;
            emit(&rendered(json, &detail, || render::site_detail(&detail)))?;
        }

        DomainCommand::Remove { domain } => {
            let remove = DomainRemove { domain };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::DOMAIN_REMOVE, encode(&remove)).await?;
            emit(&rendered(json, &detail, || render::site_detail(&detail)))?;
        }

        DomainCommand::Status { domain } => {
            let query = DomainStatusQuery { domain };
            let report: DomainStatusReport =
                ask(&mut client, rpc::method::DOMAIN_DNS_STATUS, encode(&query)).await?;
            emit(&rendered(json, &report, || render::domain_status(&report)))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

async fn site(
    command: SiteCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        SiteCommand::Create {
            project,
            domains,
            doc_root,
            kind,
            upstream,
            port,
            pool,
            proxy,
            php,
            files,
            no_routes,
            services,
            https,
            https_redirect,
            accept_risky_tld,
        } => {
            let create = SiteCreate {
                project: whose(project)?,
                domains: (!domains.is_empty()).then_some(domains),
                doc_root,
                kind: site_kind(kind, upstream, port, pool)?,
                services: (!services.is_empty()).then_some(services),
                routes: site_routes(&proxy, &php, &files, no_routes)?,
                https,
                https_redirect,
                accept_risky_tld,
            };
            let creation: SiteCreation =
                ask(&mut client, rpc::method::SITE_CREATE, encode(&create)).await?;
            emit(&rendered(json, &creation, || {
                render::site_detail(&creation.site)
            }))?;
        }

        SiteCommand::List { project } => {
            let query = SiteListQuery {
                project: project.map(ProjectRef::Name),
            };
            let list: SiteList = ask(&mut client, rpc::method::SITE_LIST, encode(&query)).await?;
            emit(&rendered(json, &list, || render::site_list(&list)))?;
        }

        SiteCommand::Show { site } => {
            let query = SiteQuery {
                site: which_site(site)?,
            };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::SITE_SHOW, encode(&query)).await?;
            emit(&rendered(json, &detail, || render::site_detail(&detail)))?;
        }

        SiteCommand::Start { site } => {
            let query = SiteQuery {
                site: which_site(site)?,
            };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::SITE_START, encode(&query)).await?;
            emit(&rendered(json, &detail, || render::site_detail(&detail)))?;
        }

        SiteCommand::Stop { site } => {
            let query = SiteQuery {
                site: which_site(site)?,
            };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::SITE_STOP, encode(&query)).await?;
            emit(&rendered(json, &detail, || render::site_detail(&detail)))?;
        }

        SiteCommand::Update {
            site,
            domains,
            doc_root,
            kind,
            upstream,
            port,
            pool,
            proxy,
            php,
            files,
            no_routes,
            services,
            https,
            https_redirect,
            state,
            accept_risky_tld,
        } => {
            let update = SiteUpdate {
                site: which_site(site)?,
                domains: (!domains.is_empty()).then_some(domains),
                doc_root,
                kind: site_kind(kind, upstream, port, pool)?,
                services: (!services.is_empty()).then_some(services),
                routes: site_routes(&proxy, &php, &files, no_routes)?,
                https,
                https_redirect,
                state: state.map(|state| match state {
                    SiteStateArg::Enabled => SiteState::Enabled,
                    SiteStateArg::Disabled => SiteState::Disabled,
                }),
                accept_risky_tld,
            };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::SITE_UPDATE, encode(&update)).await?;
            emit(&rendered(json, &detail, || render::site_detail(&detail)))?;
        }

        SiteCommand::Delete { site } => {
            let query = SiteQuery {
                site: which_site(site)?,
            };
            let removal: SiteRemoval =
                ask(&mut client, rpc::method::SITE_DELETE, encode(&query)).await?;
            emit(&rendered(json, &removal, || render::site_removal(&removal)))?;
        }

        SiteCommand::Share {
            site,
            interface,
            r#for,
        } => {
            let request = SiteShare {
                site: which_site(site)?,
                interface,
                for_seconds: r#for,
            };
            let sharing: SiteSharing =
                ask(&mut client, rpc::method::SITE_SHARE, encode(&request)).await?;
            emit(&rendered(json, &sharing, || render::site_shared(&sharing)))?;
        }

        SiteCommand::Unshare { site } => {
            let query = SiteQuery {
                site: which_site(site)?,
            };
            ask::<()>(&mut client, rpc::method::SITE_UNSHARE, encode(&query)).await?;
            emit(&rendered(json, &(), || {
                "no longer shared on the local network\n".to_owned()
            }))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// The four flags a kind is spelled with on a command line, as the one value the API takes.
///
/// Assembly rather than logic: which flags a kind needs is decided by clap's `required_if_eq`, and
/// what a kind *means* is the daemon's. What this does is put a tagged enum back together out of
/// the flat arguments a shell can carry.
fn site_kind(
    kind: Option<SiteKindArg>,
    upstream: Option<String>,
    port: Option<u16>,
    pool: Option<ServiceId>,
) -> Result<Option<SiteKind>, Error> {
    let missing = |flag: &str, because: &str| {
        Error::new(ErrorCode::InvalidArgument, format!("{flag} {because}"))
    };

    Ok(match kind {
        // `--pool` on its own says php-fpm without saying it, which is the only kind a pool
        // belongs to; nothing named at all leaves the whole decision to the daemon.
        None => pool.map(|pool| SiteKind::PhpFpm { pool: Some(pool) }),
        Some(SiteKindArg::PhpFpm) => Some(SiteKind::PhpFpm { pool }),
        Some(SiteKindArg::Static) => Some(SiteKind::Static),
        Some(SiteKindArg::ReverseProxy) => Some(SiteKind::ReverseProxy {
            upstream: upstream.ok_or_else(|| missing("--upstream", "says where to forward to"))?,
        }),
        Some(SiteKindArg::NodeApp) => Some(SiteKind::NodeApp {
            port: port.ok_or_else(|| missing("--port", "says where the node process listens"))?,
        }),
    })
}

/// The three route flags as the one list the API takes — roadmap task **T135**.
///
/// **The whole list in one request, and never a read-modify-write.** A client that fetched a site's
/// routes, changed one and sent them back would be holding business logic and racing another writer;
/// these flags build the list a person typed and hand it over, which is the same thing `--domain`
/// and `--service` already do.
///
/// Assembly rather than logic, on [`site_kind`]'s rule: what a route *means* is the daemon's, and a
/// path it refuses comes back in the daemon's own words.
fn site_routes(
    proxy: &[String],
    php: &[String],
    files: &[String],
    no_routes: bool,
) -> Result<Option<Vec<SiteRoute>>, Error> {
    if no_routes {
        if !proxy.is_empty() || !php.is_empty() || !files.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "--no-routes and a route in the same command".to_owned(),
            )
            .with_hint("leave --no-routes off to declare routes; it is what empties the list"));
        }

        return Ok(Some(Vec::new()));
    }

    if proxy.is_empty() && php.is_empty() && files.is_empty() {
        return Ok(None);
    }

    // Split on the **first** `=`: a query is not part of an address, so a URL here has none, and a
    // directory that holds one is a directory this daemon will refuse by name.
    let split = |value: &str, flag: &str, what: &str| -> Result<(String, String), Error> {
        value
            .split_once('=')
            .map(|(path, rest)| (path.to_owned(), rest.to_owned()))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{value} is not a {flag} route"),
                )
                .with_hint(format!("<path>={what}, as in {flag} /api=…"))
            })
    };

    let mut routes = Vec::with_capacity(proxy.len() + php.len() + files.len());

    for value in proxy {
        let (path, upstream) = split(value, "--proxy", "an address")?;

        routes.push(SiteRoute {
            path,
            target: RouteTarget::Proxy { upstream },
        });
    }

    for value in php {
        // A pool is optional: `--php /admin` leaves the daemon to resolve the one this project
        // resolves to, exactly as a php-fpm site with no `--pool` does.
        let (path, pool) = match value.split_once('=') {
            Some((path, pool)) => {
                let pool = service_id(pool).map_err(|because| {
                    Error::new(ErrorCode::InvalidArgument, because)
                        .with_hint("a pool such as php-fpm@8.3.33, or no `=` at all to resolve one")
                })?;

                (path.to_owned(), Some(pool))
            }
            None => (value.clone(), None),
        };

        routes.push(SiteRoute {
            path,
            target: RouteTarget::PhpFpm { pool },
        });
    }

    for value in files {
        let (path, root) = split(value, "--files", "a directory")?;

        routes.push(SiteRoute {
            path,
            target: RouteTarget::Static { root },
        });
    }

    Ok(Some(routes))
}

/// Which site, defaulting to the directory this `mix` was run in.
fn which_site(site: WhichSite) -> Result<SiteRef, Error> {
    match site.domain {
        Some(domain) => Ok(SiteRef::Domain(domain)),
        None => Ok(SiteRef::Path(here(None)?.display().to_string())),
    }
}

/// Which project a `--project` names, defaulting to the directory this `mix` was run in.
fn whose(project: Option<String>) -> Result<ProjectRef, Error> {
    match project {
        Some(name) => Ok(ProjectRef::Name(name)),
        None => Ok(ProjectRef::Path(here(None)?.display().to_string())),
    }
}

/// `mix extension …` — read a manifest and say what installing it would produce.
async fn extension(
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
                render::extension_inspection(&inspection)
            }))?;
        }

        ExtensionCommand::List => {
            let list: InstalledExtensions =
                ask(&mut client, rpc::method::EXTENSION_LIST, encode(&())).await?;

            emit(&rendered(json, &list, || {
                render::installed_extensions(&list)
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
                render::extension_catalogue(&catalogue)
            }))?;
        }

        ExtensionCommand::Plan { id, path } => {
            let asked = ExtensionPlanRequest {
                source: origin(id, path)?,
            };
            let plan: ExtensionPlan =
                ask(&mut client, rpc::method::EXTENSION_PLAN, encode(&asked)).await?;

            emit(&rendered(json, &plan, || render::extension_plan(&plan)))?;
        }

        ExtensionCommand::Install {
            id,
            path,
            yes,
            no_wait,
        } => {
            let source = origin(id, path)?;

            // **The plan is read before anything is installed, and the consent names it** — the
            // T81 design's D2 and D9. Two calls rather than one because that is what makes the
            // question answerable: the daemon has no keyboard, and the permissions a person is
            // agreeing to arrive with the listing rather than with the artifact.
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
                emit(&rendered(json, &started, || render::job_status(&started)))?;
                return Ok(ExitCode::SUCCESS);
            }

            let finished = follow(&mut client, started, json).await?;
            emit(&rendered(json, &finished, || render::job_status(&finished)))?;

            let succeeded = render::job_succeeded(&finished);

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
                render::extension_removal(&removal)
            }))?;
        }

        ExtensionCommand::Start { id } => {
            let asked = ExtensionTarget {
                id: extension_id(&id)?,
            };
            let walk: ServiceWalk =
                ask(&mut client, rpc::method::EXTENSION_START, encode(&asked)).await?;

            emit(&rendered(json, &walk, || {
                render::service_walk(render::Walked::Start, &walk)
            }))?;
        }

        ExtensionCommand::Stop { id } => {
            let asked = ExtensionTarget {
                id: extension_id(&id)?,
            };
            let walk: ServiceWalk =
                ask(&mut client, rpc::method::EXTENSION_STOP, encode(&asked)).await?;

            emit(&rendered(json, &walk, || {
                render::service_walk(render::Walked::Stop, &walk)
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
        render::extension_plan(plan)
    )) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            let _ = writeln!(std::io::stderr(), "nothing was installed");
            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}

/// `mix blueprint …` — capture one, list them, see what applying one would do.
async fn blueprint(
    command: BlueprintCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        BlueprintCommand::Capture {
            name,
            project,
            description,
            overwrite,
        } => {
            let capture = BlueprintCapture {
                project: which(WhichProject { name: project })?,
                name,
                description,
                overwrite,
            };
            let summary: BlueprintSummary = ask(
                &mut client,
                rpc::method::BLUEPRINT_CAPTURE,
                encode(&capture),
            )
            .await?;
            emit(&rendered(json, &summary, || {
                render::blueprint_captured(&summary)
            }))?;
        }

        BlueprintCommand::Import {
            file,
            name,
            signature,
            overwrite,
        } => {
            // Resolved here, because the daemon has no idea what directory this process is in and a
            // relative path sent as it was typed would be read against the wrong one.
            let import = BlueprintImport {
                path: here(Some(file))?.display().to_string(),
                signature: match signature {
                    Some(path) => Some(here(Some(path))?.display().to_string()),
                    None => None,
                },
                name,
                overwrite,
            };

            let summary: BlueprintSummary =
                ask(&mut client, rpc::method::BLUEPRINT_IMPORT, encode(&import)).await?;

            emit(&rendered(json, &summary, || {
                render::blueprint_imported(&summary)
            }))?;
        }

        BlueprintCommand::List => {
            let list: BlueprintList = ask(&mut client, rpc::method::BLUEPRINT_LIST, None).await?;
            emit(&rendered(json, &list, || render::blueprint_list(&list)))?;
        }

        BlueprintCommand::Apply {
            blueprint,
            project,
            path,
            dry_run,
            install_missing,
            use_installed,
            with_front_end,
            // Bound under another name: `autostart` in this function is already the daemon
            // autostarter every command carries — `mix autostart` is about the daemon and this flag
            // is about the services an apply creates.
            autostart: services_autostart,
            start,
            run_scaffold,
            run_untrusted_scaffold,
            grant,
            install_prerequisites,
            ignore_requirements,
        } => {
            // **Where it goes, and who names it** — roadmap task **T120a**. A path somebody typed
            // is sent as typed; with none, the daemon is told to make one *under* this directory,
            // and it names that directory with the project's handle.
            //
            // The naming is deliberately not done here. This binary may not depend on
            // `mixengine-core` (`mixengine-proto/tests/workspace_layering.rs`), so it cannot call
            // `domains::slug` — and restating the charset would be a second copy of the rule T120
            // spent a task consolidating, in the client the ban on business logic is about. A
            // client knows where it is standing; the daemon knows what things are called.
            let (root, root_is_parent) = match path {
                Some(path) => (here(Some(path))?, false),
                None => (here(None)?, true),
            };

            let mut apply = BlueprintApply {
                blueprint,
                project,
                root: root.display().to_string(),
                root_is_parent,
                dry_run: true,
                answers: Vec::new(),
                // Filled in below, once the plan says whether there is a command to agree to and
                // who wrote it — roadmap task **T78a**.
                scaffold: None,
                // Carried on the dry run as well as on the real one, which is what keeps the
                // feature's own acceptance criterion true: `--dry-run` prints the actions the real
                // run performs, so a flag that changed the plan may not be added afterwards.
                front_end: with_front_end,
                autostart: services_autostart,
                // Answered below, once the plan says what its releases lack — T152.
                install_prerequisites: false,
                ignore_requirements,
            };

            // **The plan comes first either way** (the T78 design, D6). A dry run stops here; a real
            // apply needs it because the questions are in it, and a daemon has no keyboard to ask
            // them with.
            let planned: BlueprintApplyResponse =
                ask(&mut client, rpc::method::BLUEPRINT_APPLY, encode(&apply)).await?;

            let BlueprintApplyResponse::Planned { plan, needs } = &planned else {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "the daemon answered a dry run with something other than a plan",
                ));
            };

            emit(&rendered(json, plan, || render::blueprint_plan(plan)))?;

            let needs = needs.as_deref().unwrap_or_default();
            if !json && !needs.is_empty() {
                emit(&render::requirements(needs))?;
            }

            if dry_run {
                return Ok(ExitCode::SUCCESS);
            }

            let Some(answers) = answered(plan, install_missing, use_installed, json)? else {
                // Cancelling is an answer and not a failure: nothing was asked of the machine, and
                // the same command works when the person has decided.
                return Ok(ExitCode::SUCCESS);
            };

            // **The command is shown and agreed to, every apply** — roadmap task **T78a**. The
            // consent carries the command it was given about, so a blueprint that changed between
            // this plan and the apply below cannot be run under it.
            let consent = agreed_to_scaffold(plan, run_scaffold, run_untrusted_scaffold, json)?;

            // **Asked once for the whole plan** (T152, D7), after the version and scaffold questions
            // so every question the apply raises is answered before any of it starts.
            let Some(agreed) = agreed_to_prerequisites(needs, install_prerequisites, json)? else {
                return Ok(ExitCode::SUCCESS);
            };
            apply.install_prerequisites = agreed;

            apply.dry_run = false;
            apply.answers = answers;
            apply.scaffold = consent;

            let started: BlueprintApplyResponse =
                ask(&mut client, rpc::method::BLUEPRINT_APPLY, encode(&apply)).await?;

            let BlueprintApplyResponse::Started { job } = started else {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "the daemon answered an apply with something other than a job",
                ));
            };

            // **A second connection, because two streams cannot share one** — roadmap task
            // **T78a**. The job's own progress comes back on this client's `job.wait`, and what the
            // blueprint's command prints comes down `GET /logs/job/<id>` for as long as it runs. A
            // log that cannot be opened is not worth failing an apply over: the work goes on and the
            // outcome still says what happened.
            let watching = watch_job_log(endpoint, autostart, job.id, json).await;

            let finished = follow(&mut client, job, json).await?;

            if let Some(watching) = watching {
                watching.abort();
            }
            emit(&rendered(json, &finished, || render::job_status(&finished)))?;

            let mut a_step_failed = false;

            if let Some(JobOutcome::Succeeded { result }) = &finished.outcome
                && let Ok(applied) = serde_json::from_value::<BlueprintApplied>(result.clone())
            {
                emit(&rendered(json, &applied, || {
                    render::blueprint_applied(&applied)
                }))?;

                a_step_failed = render::blueprint_had_a_failed_step(&applied);
            }

            if !render::job_succeeded(&finished) {
                return Ok(ExitCode::FAILURE);
            }

            // **The job succeeded and a step did not** — roadmap task **T78a**, its design's D7. The
            // apply applied everything it was asked to; what failed is the blueprint's own command,
            // and a shell that chained on this has to hear it.
            if a_step_failed {
                return Ok(ExitCode::FAILURE);
            }

            // **The client is what spends the prompt** (D10): the apply queued the hosts entries and
            // the daemon never raises a dialog on its own initiative, so the last thing this command
            // does is offer the one prompt that makes the new site reachable.
            let spent = granted(&mut client, grant, json).await?;

            if !start || spent != ExitCode::SUCCESS {
                return Ok(spent);
            }

            // **After the elevation and not inside the job** — roadmap task **T117**. An apply never
            // raises a prompt; it queues what needs one and the client spends it above. A front end
            // started before that would serve the new site at a name this machine does not resolve
            // and with a certificate no store trusts — a browser error at the end of a progress bar.
            //
            // **This project's services, not this home's** — roadmap task **T125**. Until then the
            // target was empty, meaning *every service this home declares*, and a home with four
            // PHP versions and three databases started all of them to put one site up. What the
            // apply needs is a set no client may derive — so the daemon derives it, from the sites
            // this project now has, and `--start` asks for it by name.
            let walk: ServiceWalk = ask(
                &mut client,
                rpc::method::SERVICE_START,
                encode(&ServiceTarget {
                    service: None,
                    project: Some(ProjectRef::Name(apply.project.clone())),
                    wait: true,
                }),
            )
            .await?;

            emit(&rendered(json, &walk, || {
                render::service_walk(render::Walked::Start, &walk)
            }))?;

            return Ok(match walk.failed {
                None => ExitCode::SUCCESS,
                Some(_) => ExitCode::FAILURE,
            });
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// `mix database …` — make a database and the account that reaches it, hand it to a client, or
/// read what password is stored for one.
///
/// **`create`, `client` and `open` never print a password**: what comes back is the address the
/// credential is stored under, because a password on a terminal is a password in scrollback, in a
/// tmux buffer and in a CI log. Handing one to a program that needs it is `open` (roadmap task
/// **T83**), and the daemon puts it in that program's environment alone. `credentials` is the one
/// exception — its whole purpose is to print one, for a project's `.env` — roadmap task **T77b**.
async fn database(
    command: DatabaseCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        DatabaseCommand::Create {
            service,
            name,
            user,
            password,
        } => {
            let password = match password.as_deref() {
                None => None,
                Some(value) if !value.is_empty() => Some(value.to_owned()),
                Some(_) => match crate::confirm::read_password(&format!(
                    "password for {} on {service}: ",
                    user.as_deref().unwrap_or(&name)
                )) {
                    Some(line) => Some(line),
                    None => {
                        eprintln!("nobody to ask — pass `--password <value>` or pipe one line in");
                        return Ok(ExitCode::from(1));
                    }
                },
            };

            let create = DatabaseCreate {
                service,
                database: name,
                user,
                password,
            };
            let account: DatabaseAccount =
                ask(&mut client, rpc::method::DATABASE_CREATE, encode(&create)).await?;

            emit(&rendered(json, &account, || {
                render::database_created(&account)
            }))?;
        }

        DatabaseCommand::Client { service } => {
            let report: DatabaseClientReport = ask(
                &mut client,
                rpc::method::DATABASE_CLIENT,
                encode(&DatabaseClientQuery { service }),
            )
            .await?;

            emit(&rendered(json, &report, || {
                render::database_client(&report)
            }))?;
        }

        DatabaseCommand::Open {
            service,
            user,
            database,
        } => {
            let handoff: DatabaseHandoff = ask(
                &mut client,
                rpc::method::DATABASE_OPEN,
                encode(&DatabaseOpen {
                    service,
                    user,
                    database,
                }),
            )
            .await?;

            emit(&rendered(json, &handoff, || {
                render::database_opened(&handoff)
            }))?;

            // A client that did not open is exit 1 for `mix service start`'s reason: `mix database
            // open db && …` is a sentence about a client having opened. The answer is a state and
            // was printed as one; the code is what a script reads.
            if handoff.launched.is_none() {
                return Ok(ExitCode::from(1));
            }
        }

        DatabaseCommand::Credentials { service, user } => {
            let answer: DatabaseCredentials = ask(
                &mut client,
                rpc::method::DATABASE_CREDENTIALS,
                encode(&DatabaseCredentialsQuery { service, user }),
            )
            .await?;

            emit(&rendered(json, &answer, || {
                render::database_credentials(&answer)
            }))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn which(project: WhichProject) -> Result<ProjectRef, Error> {
    match project.name {
        Some(name) => Ok(ProjectRef::Name(name)),
        None => Ok(ProjectRef::Path(here(None)?.display().to_string())),
    }
}

/// A directory argument, or the one this process is in.
fn here(given: Option<PathBuf>) -> Result<PathBuf, Error> {
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

/// `php=^8.3` — one pin, as a person types it.
fn pin(value: &str) -> Result<(RuntimeKind, VersionConstraint), String> {
    let (kind, constraint) = value
        .split_once('=')
        .ok_or_else(|| format!("`{value}` is not `<runtime>=<version>`"))?;

    Ok((runtime_kind(kind)?, version_constraint(constraint)?))
}

/// `mix package …`: one call, one rendering — except the install, which follows a job.
async fn package(
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
        } => {
            let filter = PackageFilter { package, refresh };
            let catalogue: PackageCatalogue = ask(
                &mut client,
                rpc::method::PACKAGE_LIST_AVAILABLE,
                encode(&filter),
            )
            .await?;
            emit(&rendered(json, &catalogue, || {
                render::package_catalogue(&catalogue)
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
                emit(&rendered(json, &started, || render::job_status(&started)))?;
                return Ok(ExitCode::SUCCESS);
            }

            let finished = follow(&mut client, started, json).await?;
            emit(&rendered(json, &finished, || render::job_status(&finished)))?;

            return Ok(match render::job_succeeded(&finished) {
                true => ExitCode::SUCCESS,
                false => ExitCode::FAILURE,
            });
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

/// `mix path …`: one call, one rendering.
///
/// No exit code of its own — unlike `mix service start`, every one of these either did what it said
/// or failed outright, and there is no partial answer for a status to describe. A `bin/` with a
/// leftover in it is reported in the rendering and is not a failure: the commands that should be
/// there are there.
async fn path(
    command: PathCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let (method, pathed) = match command {
        PathCommand::Status => (rpc::method::PATH_STATUS, render::Pathed::Asked),
        PathCommand::Install => (rpc::method::PATH_INSTALL, render::Pathed::Installed),
        PathCommand::Uninstall => (rpc::method::PATH_UNINSTALL, render::Pathed::Uninstalled),
        PathCommand::Rescan => (rpc::method::PATH_RESCAN, render::Pathed::Rescanned),
    };

    let report: PathReport = ask(&mut client, method, None).await?;
    emit(&rendered(json, &report, || {
        render::path_report(pathed, &report)
    }))?;

    Ok(ExitCode::SUCCESS)
}

/// `mix autostart …`: one call, one rendering — roadmap task **T85b**.
///
/// No exit code of its own, on `mix path`'s rule: each of these either did what it said or failed
/// outright, and a machine with no mechanism at all is something to report rather than a failure of
/// the command.
///
/// **The `autostart` parameter is not what this command is about.** It is the client's own — the
/// thing that starts a daemon when none answers, in [`crate::autostart`] — and every command in this
/// file takes it under that name. The two meet here and nowhere else, which is why this function is
/// `autostart_entry` and the parameter is left alone.
async fn autostart_entry(
    command: AutostartCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    let (method, rendered_as) = match command {
        AutostartCommand::Status => (rpc::method::AUTOSTART_STATUS, render::Autostarted::Asked),
        AutostartCommand::Enable => (rpc::method::AUTOSTART_ENABLE, render::Autostarted::Enabled),
        AutostartCommand::Disable => (
            rpc::method::AUTOSTART_DISABLE,
            render::Autostarted::Disabled,
        ),
    };

    let report: AutostartReport = ask(&mut client, method, None).await?;
    emit(&rendered(json, &report, || {
        render::autostart_report(rendered_as, &report)
    }))?;

    Ok(ExitCode::SUCCESS)
}

/// `mix cert …`: one call, one rendering.
///
/// **Every state exits zero, including `absent` and `unusable`.** This reports; `mix doctor` is what
/// carries a verdict. A reporting command with a failing exit is one nobody can put in front of an
/// `&&` without thinking about it, and there is nothing here a person asked to change.
async fn cert(
    command: CertCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        CertCommand::CaStatus => {
            let status: CaStatus = ask(&mut client, rpc::method::CERT_CA_STATUS, None).await?;
            emit(&rendered(json, &status, || render::ca_status(&status)))?;

            Ok(ExitCode::SUCCESS)
        }

        CertCommand::Status { site } => {
            let request = CertStatusQuery {
                site: site.map(SiteRef::Domain),
            };
            let report: CertStatusReport =
                ask(&mut client, rpc::method::CERT_STATUS, encode(&request)).await?;
            emit(&rendered(json, &report, || render::cert_status(&report)))?;

            Ok(ExitCode::SUCCESS)
        }

        CertCommand::Issue { site } => {
            let request = CertIssue {
                site: site.map(SiteRef::Domain),
            };
            let report: CertIssueReport =
                ask(&mut client, rpc::method::CERT_ISSUE, encode(&request)).await?;
            emit(&rendered(json, &report, || render::cert_issue(&report)))?;

            Ok(ExitCode::SUCCESS)
        }

        CertCommand::CaRotate { yes, no_wait } => {
            if !yes
                && !agreed(
                    &mut client,
                    "this will replace this home's certificate authority. Every site's certificate \
                     is reissued under the new one, and every browser holding a cached chain under \
                     the old one stops accepting it until it re-reads the store.",
                    json,
                )
                .await?
            {
                return Ok(ExitCode::SUCCESS);
            }

            let started: JobSummary = ask(&mut client, rpc::method::CERT_CA_ROTATE, None).await?;

            job_answering(&mut client, started, no_wait, json, |result| {
                serde_json::from_value::<CaRotateReport>(result)
                    .ok()
                    .map(|report| render::ca_rotate(&report))
            })
            .await
        }

        CertCommand::CaUninstall { yes, no_wait } => {
            if !yes
                && !agreed(
                    &mut client,
                    "this will take this home's certificate authority out of every store on this \
                     machine that trusts it. The certificate and its key stay on disk, and \
                     `mix doctor --repair` puts the trust back.",
                    json,
                )
                .await?
            {
                return Ok(ExitCode::SUCCESS);
            }

            let started: JobSummary =
                ask(&mut client, rpc::method::CERT_CA_UNINSTALL, None).await?;

            job_answering(&mut client, started, no_wait, json, |result| {
                serde_json::from_value::<CaUninstallReport>(result)
                    .ok()
                    .map(|report| render::ca_uninstall(&report))
            })
            .await
        }
    }
}

/// Say what a command is about to change, name anything else already queued, and ask.
///
/// **T64's rule, adapted to a command that queues its own work.** `mix elevation grant` can read the
/// batch before it asks, because the batch is already there; `cert.ca_uninstall` builds its own
/// batch inside the job, so what this puts in front of a person is the command's own sentence — plus
/// whatever was *already* waiting, because one grant spends one prompt on all of it and a person who
/// typed a certificate command should not discover afterwards that their hosts file moved.
async fn agreed(client: &mut Client, what: &str, json: bool) -> Result<bool, Error> {
    if json {
        return Err(unanswered());
    }

    let waiting: ElevationStatus = ask(client, rpc::method::ELEVATION_STATUS, None).await?;

    let also = match waiting.pending.is_empty() {
        true => String::new(),
        false => format!(
            "\n\nthis machine is also holding these, and one prompt covers them all:\n{}",
            render::elevation_prompt(&waiting)
        ),
    };

    match confirm::ask(&format!("{what}{also}\n\ncontinue? [y/N] ")) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            let _ = writeln!(std::io::stderr(), "nothing was changed");
            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}

/// Follow a job to its end and render whatever it produced, or print the job when it produced
/// nothing this command knows how to read.
///
/// **The result is decoded here and not in [`render::job_status`].** That function tries several
/// types in turn against one `serde_json::Value`, and T54's two reports are similar enough — an
/// `outcome` and a `status` apiece — that adding them to that chain is how a rotation gets rendered
/// as a removal. A command knows its own type.
async fn job_answering(
    client: &mut Client,
    started: JobSummary,
    no_wait: bool,
    json: bool,
    render_result: impl Fn(serde_json::Value) -> Option<String>,
) -> Result<ExitCode, Error> {
    if no_wait {
        emit(&rendered(json, &started, || render::job_status(&started)))?;
        return Ok(ExitCode::SUCCESS);
    }

    let finished = follow(client, started, json).await?;

    match finished.outcome.clone() {
        Some(JobOutcome::Succeeded { result }) => match render_result(result.clone()) {
            Some(said) => emit(&rendered(json, &result, || said))?,
            None => emit(&rendered(json, &finished, || render::job_status(&finished)))?,
        },
        _ => emit(&rendered(json, &finished, || render::job_status(&finished)))?,
    }

    Ok(match render::job_succeeded(&finished) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    })
}

/// `mix elevation …`: one call, one rendering — except the grant, which is one call and a wait.
async fn elevation(
    command: ElevationCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        ElevationCommand::Status => {
            let status: ElevationStatus =
                ask(&mut client, rpc::method::ELEVATION_STATUS, None).await?;
            emit(&rendered(json, &status, || {
                render::elevation_status(&status)
            }))?;

            Ok(ExitCode::SUCCESS)
        }

        ElevationCommand::Grant { yes, no_wait } => {
            // Roadmap task T64: what is about to be allowed is read before it is allowed. The
            // ordering is a property of the API rather than of this function — the daemon never
            // raises a prompt on its own initiative, so there is a moment between knowing the batch
            // and asking for it, and this is what happens in that moment.
            let waiting: ElevationStatus =
                ask(&mut client, rpc::method::ELEVATION_STATUS, None).await?;

            // An empty queue is `elevation.grant`'s own refusal to make, and it is left to it. What
            // is skipped is the question: there is nothing to put in front of somebody.
            if !waiting.pending.is_empty() && !yes && !confirmed(&waiting, json)? {
                // Saying no is an answer and not a failure — `docs/decisions/0005-on-demand-
                // elevation.md`. Nothing was written and nothing was dropped, so the same command
                // works when the person is ready.
                return Ok(ExitCode::SUCCESS);
            }

            let started: JobSummary = ask(&mut client, rpc::method::ELEVATION_GRANT, None).await?;

            if no_wait {
                emit(&rendered(json, &started, || render::job_status(&started)))?;
                return Ok(ExitCode::SUCCESS);
            }

            let finished = follow(&mut client, started, json).await?;
            emit(&rendered(json, &finished, || render::job_status(&finished)))?;

            Ok(match render::job_succeeded(&finished) {
                true => ExitCode::SUCCESS,
                false => ExitCode::FAILURE,
            })
        }

        ElevationCommand::Drop { op, all } => {
            // Neither named is a person who has not decided which. Refused here rather than turned
            // into "all", which is the reading that empties a queue somebody meant to prune.
            if op.is_none() && !all {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "name an operation to forget, or pass --all to forget every one of them",
                ));
            }

            let asked = ElevationDrop {
                op: op.map(PendingOpId),
            };
            let left: ElevationStatus =
                ask(&mut client, rpc::method::ELEVATION_DROP, encode(&asked)).await?;

            emit(&rendered(json, &left, || render::elevation_status(&left)))?;

            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Show what a grant would allow, ask about it, and answer whether to go on — roadmap task **T64**.
///
/// The screen is printed whichever way this ends, because it is the point: a person is being asked
/// to give an administrator's permission to a batch of operations, and the batch is what they are
/// judging. The question comes after it, never instead of it.
///
/// # Errors
///
/// When there is nobody to ask — `--json`, or a standard input at end of file. Both are refused
/// rather than assumed either way: yes would raise a dialog on a machine nobody is sitting at, and
/// no would be a decline the caller could not tell from a grant that happened.
fn confirmed(waiting: &ElevationStatus, json: bool) -> Result<bool, Error> {
    if json {
        return Err(unanswered());
    }

    match confirm::ask(&format!(
        "{}\ncontinue? [y/N] ",
        render::elevation_prompt(waiting)
    )) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            // On stderr, beside the question it answers. Stdout carries what a command was asked
            // for, and this run was asked for a grant that is not going to happen.
            let _ = writeln!(
                std::io::stderr(),
                "nothing was asked for; run `mix elevation grant` again when you are ready"
            );

            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}

/// There was nobody to put the question to.
/// The answers to a plan's version questions, or [`None`] when somebody cancelled.
///
/// **One question per mismatch, and the flags answer them all** (the T78 design, D6). A person
/// answers them one at a time; a script passes `--install-missing` or `--use-installed` and is never
/// asked. Standard input at end of file with a question outstanding is a refusal naming the two
/// flags, on `confirm.rs`' standing rule: what must not happen is a prompt nobody is there to see.
fn answered(
    plan: &BlueprintPlan,
    install_missing: bool,
    use_installed: bool,
    json: bool,
) -> Result<Option<Vec<VersionAnswer>>, Error> {
    let mut answers = Vec::new();

    for step in &plan.steps {
        let Disposition::Choice { installed, wanted } = &step.disposition else {
            continue;
        };

        let Some(subject) = subject_of(&step.action) else {
            continue;
        };

        let answer = match (install_missing, use_installed) {
            (true, _) => MismatchAnswer::Install,
            (_, true) => MismatchAnswer::UseInstalled,

            (false, false) => {
                // A `--json` run has nobody at a keyboard by construction, and a question it cannot
                // ask is one it must not answer on somebody's behalf.
                if json {
                    return Err(unanswerable_question());
                }

                match confirm::choose(&format!(
                    "{subject} {} is not installed. [i]nstall it, [u]se the installed {}, or \
                     [c]ancel? ",
                    wanted.as_str(),
                    installed.as_str()
                )) {
                    confirm::Choice::Install => MismatchAnswer::Install,
                    confirm::Choice::UseInstalled => MismatchAnswer::UseInstalled,

                    confirm::Choice::Cancel => {
                        let _ = writeln!(
                            std::io::stderr(),
                            "nothing was applied; run the same command again when you have decided"
                        );

                        return Ok(None);
                    }

                    confirm::Choice::Unanswerable => return Err(unanswerable_question()),
                }
            }
        };

        answers.push(VersionAnswer { subject, answer });
    }

    Ok(Some(answers))
}

/// What a version question is about, where the action is one that can raise one.
fn subject_of(action: &PlanAction) -> Option<AnswerSubject> {
    match action {
        PlanAction::InstallRuntime { kind, .. } => Some(AnswerSubject::Runtime { kind: *kind }),

        PlanAction::EnsureService {
            package, instance, ..
        } => service_id(&format!("{package}@{instance}"))
            .or_else(|_| service_id(package))
            .ok()
            .map(|id| AnswerSubject::Service { id }),

        _ => None,
    }
}

/// Spend the one elevation prompt an apply queued, having asked first unless told not to.
///
/// **A client is the only thing allowed to raise one** — `docs/architecture/daemon-and-ipc.md`'s
/// rule, which the daemon's own elevation queue is built around. An apply enqueues; this is where
/// somebody says yes.
async fn granted(client: &mut Client, grant: bool, json: bool) -> Result<ExitCode, Error> {
    let waiting: ElevationStatus = ask(client, rpc::method::ELEVATION_STATUS, None).await?;

    // Nothing waiting is the ordinary end of an apply on a machine that already had the names in
    // its hosts file — or one that cannot prompt at all, which left the queue where it was.
    if waiting.pending.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }

    // **An apply that worked is not a failure because nobody was there to say yes.** The names are
    // in the daemon's queue and one command spends them, so what a `--json` run and a closed
    // standard input get is that sentence rather than an error about a question nobody heard.
    if !grant && (json || !asked_to_grant(&waiting)) {
        let _ = writeln!(
            std::io::stderr(),
            "{} still waiting; `mix elevation grant` writes them",
            waiting.pending.len()
        );

        return Ok(ExitCode::SUCCESS);
    }

    let started: JobSummary = ask(client, rpc::method::ELEVATION_GRANT, None).await?;
    let finished = follow(client, started, json).await?;
    emit(&rendered(json, &finished, || render::job_status(&finished)))?;

    Ok(match render::job_succeeded(&finished) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    })
}

/// Whether somebody at the keyboard said yes to spending the prompt.
///
/// Anything but a typed yes — including a closed standard input — is *not now*, which is a state the
/// same command gets out of and never an error: the queue is untouched either way.
fn asked_to_grant(waiting: &ElevationStatus) -> bool {
    matches!(
        confirm::ask(&format!(
            "{}\ngrant now? [y/N] ",
            render::elevation_prompt(waiting)
        )),
        confirm::Answer::Yes
    )
}

/// The refusal for a version question nothing could answer.
fn unanswerable_question() -> Error {
    Error::new(
        ErrorCode::InvalidArgument,
        "this blueprint asks for a version this machine does not have, and nothing answered the \
         question",
    )
    .with_hint(
        "`--install-missing` installs what it asks for; `--use-installed` takes what is here",
    )
}

fn unanswered() -> Error {
    Error::new(
        ErrorCode::InvalidArgument,
        "nothing answered the question, so nothing was asked of the operating system either",
    )
    .with_hint("pass `--yes` to answer in advance")
}

/// What somebody said in advance about a machine that lacks something — roadmap task **T151**.
#[derive(Debug, Clone, Copy)]
struct Agreement {
    /// `--yes`: install what MixEngine can install, without asking.
    yes: bool,

    /// `--ignore-requirements`: do not judge at all.
    ignore_requirements: bool,
}

/// Whether to install what the machine lacks first, given what the daemon judged.
///
/// `Ok(Some(agreed))` goes on to the install; `Ok(None)` is a person who answered no. **Only an
/// installable lack is asked about**: one no installer fixes is the daemon's to refuse, in its own
/// sentence, and asking about it here would be a second wording of one refusal.
fn agreed_to_prerequisites(
    unmet: &[Requirement],
    yes: bool,
    json: bool,
) -> Result<Option<bool>, Error> {
    // **Said before anything is decided, and it decides nothing** — roadmap task **T27e**, D16: a
    // library the distribution provides is the person's to install, and the runtime installs
    // meanwhile. On `--json` it is left out, because the same list is in what the daemon answered.
    let advisories: Vec<Requirement> = unmet
        .iter()
        .filter(|requirement| matches!(requirement.remedy, Remedy::InstallFromDistribution))
        .cloned()
        .collect();
    if !advisories.is_empty() && !json {
        let _ = write!(std::io::stderr(), "{}", render::advisories(&advisories));
    }

    let installable = unmet
        .iter()
        .any(|requirement| matches!(requirement.remedy, Remedy::InstallVisualCpp { .. }));
    let blocked = unmet.iter().any(|requirement| {
        !matches!(
            requirement.remedy,
            Remedy::InstallVisualCpp { .. } | Remedy::InstallFromDistribution
        )
    });

    if !installable || blocked {
        return Ok(Some(false));
    }
    if yes {
        return Ok(Some(true));
    }
    if json {
        return Err(unanswered());
    }

    match confirm::ask(&format!(
        "{}install it first? Windows may ask for administrator approval. [y/N] ",
        render::requirements(unmet)
    )) {
        confirm::Answer::Yes => Ok(Some(true)),
        confirm::Answer::No => {
            let _ = writeln!(std::io::stderr(), "nothing was installed");
            Ok(None)
        }
        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}

/// Ask the daemon what `target` lacks, then [`agreed_to_prerequisites`].
///
/// **A question the daemon cannot answer asks nothing.** A version the index does not publish, or an
/// index that cannot be read, is the install's to report — as a job that fails in its own words — and
/// refusing it here first would be the same failure said twice, the second time more vaguely.
async fn prerequisites_agreed(
    client: &mut Client,
    method: &str,
    target: Option<serde_json::Value>,
    agreement: Agreement,
    json: bool,
) -> Result<Option<bool>, Error> {
    if agreement.ignore_requirements {
        return Ok(Some(false));
    }

    match ask::<Requirements>(client, method, target).await {
        Ok(judged) => agreed_to_prerequisites(&judged.unmet, agreement.yes, json),
        Err(_) => Ok(Some(false)),
    }
}

/// `mix runtime …`: one call, one rendering — except the install, which is one call and a wait.
async fn runtime(
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
            emit(&rendered(json, &list, || render::runtime_list(&list)))?;
        }

        RuntimeCommand::Available {
            filter: Kind { kind },
            refresh,
        } => {
            let filter = RuntimeFilter { kind, refresh };
            let catalogue: RuntimeCatalogue = ask(
                &mut client,
                rpc::method::RUNTIME_LIST_AVAILABLE,
                encode(&filter),
            )
            .await?;
            emit(&rendered(json, &catalogue, || {
                render::runtime_catalogue(&catalogue)
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

        RuntimeCommand::Uninstall { runtime, force } => {
            let asked = RuntimeUninstall {
                target: target(runtime),
                force,
            };
            let removal: RuntimeRemoval =
                ask(&mut client, rpc::method::RUNTIME_UNINSTALL, encode(&asked)).await?;
            emit(&rendered(json, &removal, || {
                render::runtime_removal(&removal)
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
                render::runtime_summary(&summary)
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
                    emit(&rendered(json, &list, || render::extension_list(&list)))?;
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
                        render::extension_change(&change)
                    }))?;
                }
            }
        }

        RuntimeCommand::Resolve { kind, version, cwd } => {
            let question = question(kind, version, cwd)?;
            let resolved: ResolvedRuntime =
                ask(&mut client, rpc::method::RUNTIME_RESOLVE, encode(&question)).await?;
            emit(&rendered(json, &resolved, || {
                render::runtime_resolved(&resolved)
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
/// **What the machine lacks is asked about first** (T151), once, before anything is started.
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

    let asked = RuntimeInstall {
        target,
        install_prerequisites,
        ignore_requirements: agreement.ignore_requirements,
    };
    let started: JobSummary = ask(client, rpc::method::RUNTIME_INSTALL, encode(&asked)).await?;

    if no_wait {
        emit(&rendered(json, &started, || render::job_status(&started)))?;
        return Ok(ExitCode::SUCCESS);
    }

    let finished = follow(client, started, json).await?;

    emit(&rendered(json, &finished, || render::job_status(&finished)))?;

    Ok(match render::job_succeeded(&finished) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    })
}

/// Poll a job until it ends, saying on stderr what it is doing as that changes.
///
/// A short timeout rather than the default thirty seconds: what is being waited for is a progress
/// report, and a wait that only answers when the job *ends* would leave a person watching nothing
/// for the length of a download. The daemon caps what it grants either way, so the cost of asking
/// often is one round trip a second on a socket that is not a network.
async fn follow(client: &mut Client, started: JobSummary, json: bool) -> Result<JobSummary, Error> {
    /// How long each `job.wait` asks for.
    const POLL: Millis = Millis(1_000);

    let mut job = started;
    let mut said = String::new();

    while !job.state.is_finished() {
        // Only in the human rendering, and only when it changed: a `--json` run emits one object,
        // and a progress line repeated once a second would be noise in a terminal and a log.
        if !json && job.message != said {
            said = job.message.clone();
            report_progress(job.percent, &said);
        }

        job = ask(
            client,
            rpc::method::JOB_WAIT,
            encode(&JobWait {
                job: job.id,
                timeout: POLL,
            }),
        )
        .await?;
    }

    Ok(job)
}

/// Agreement to run the blueprint's own command, or [`None`] where there is nothing to agree to.
///
/// Roadmap task **T78a**, its design's D4 and D15. **The command is printed exactly as it will run**
/// — a confirmation that paraphrased would be a confirmation to something else — and the flag that
/// answers it depends on who wrote the blueprint: `--run-scaffold` for one the gallery signed, and
/// `--run-untrusted-scaffold` for one nobody vouches for. Neither covers the other, so a script that
/// runs somebody's unsigned command says so on the line that does it.
///
/// Declining is not a failure. The apply goes ahead without the command and the step comes back
/// saying it was left, which is a project a person can use and one line they can run themselves.
///
/// # Errors
///
/// `invalid_argument` when there is a question and nothing to answer it with: a `--json` run, or a
/// standard input that is closed — [`answered`]'s `Unanswerable` rule, unchanged.
fn agreed_to_scaffold(
    plan: &BlueprintPlan,
    run_scaffold: bool,
    run_untrusted_scaffold: bool,
    json: bool,
) -> Result<Option<ScaffoldConsent>, Error> {
    let Some(command) = plan.steps.iter().find_map(|step| match &step.action {
        PlanAction::RunScaffold { command } => Some(command.clone()),
        _ => None,
    }) else {
        return Ok(None);
    };

    let untrusted = !plan.trusted;

    let given = match untrusted {
        true => run_untrusted_scaffold,
        false => run_scaffold,
    };

    if given {
        return Ok(Some(ScaffoldConsent { command, untrusted }));
    }

    // The flag for the other kind of blueprint is not an answer about this one, and saying so is
    // more use than running the command or silently skipping it.
    let wrong_flag = match untrusted {
        true => run_scaffold,
        false => run_untrusted_scaffold,
    };

    if wrong_flag {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            match untrusted {
                true => format!(
                    "nothing vouches for {}, so `--run-scaffold` does not answer for it",
                    plan.blueprint
                ),
                false => format!(
                    "{} is signed, so `--run-untrusted-scaffold` does not answer for it",
                    plan.blueprint
                ),
            },
        )
        .with_hint(match untrusted {
            true => "`--run-untrusted-scaffold` runs it anyway",
            false => "`--run-scaffold` runs it",
        }));
    }

    // **Nobody to ask is not a refusal here, and that is a departure from `answered`.** A version
    // question has no safe default — the two answers leave different machines — so a `--json` run
    // with one outstanding is refused. This question does have one: not running somebody else's
    // command leaves a project that works and a line saying what was left, and there is no flag for
    // *no*, so refusing would make "apply this without its command" impossible from a script.
    if json {
        return Ok(unasked(&command, untrusted));
    }

    let vouched = vouching(untrusted, plan.signature);

    match confirm::ask(&format!(
        "{vouched} It wants to run, in the new project's directory:\n\n    {command}\n\nRun it? \
         [y/N] "
    )) {
        confirm::Answer::Yes => Ok(Some(ScaffoldConsent { command, untrusted })),

        // Declining leaves the step as a sentence and applies everything else, which is what the
        // daemon does with an apply that carries no consent at all.
        confirm::Answer::No => Ok(None),

        confirm::Answer::Unanswerable => Ok(unasked(&command, untrusted)),
    }
}

/// What a person is told about the blueprint whose command they are being asked to run.
///
/// **Which kind of untrusted, and not merely that it is** — roadmap task **T79b**. This is the
/// moment the reason is worth most: somebody is about to run a stranger's command, and "a signature
/// came with this and it is not the gallery's" is a different thing to weigh than "nobody signed
/// it". It changes what is *said* and nothing about what is allowed — one flag still answers for
/// both kinds (the T79b design's D8).
///
/// A function of its own because the question it belongs to is only asked on a real apply, with a
/// keyboard in front of it: a sentence reachable no other way is a sentence nothing can check.
fn vouching(untrusted: bool, signature: Option<SignatureCheck>) -> &'static str {
    match (untrusted, signature) {
        (false, _) => "This blueprint is signed.",

        // True of all three things the verifier folds together, which is why it does not say the
        // bytes changed: a colleague's own key and a corrupt `.minisig` land here too.
        (true, Some(SignatureCheck::Rejected)) => {
            "A signature came with this blueprint and it is not the gallery's."
        }

        // Nothing came with it, or the row is older than the reason.
        (true, _) => "Nothing vouches for this blueprint.",
    }
}

/// Say on stderr that a command was left unrun because there was nobody to ask, and leave it.
///
/// Roadmap task **T78a**. The apply goes ahead: what it makes is a project somebody can use, and
/// the step's own outcome says the command is still theirs to run.
fn unasked(command: &str, untrusted: bool) -> Option<ScaffoldConsent> {
    let flag = match untrusted {
        true => "--run-untrusted-scaffold",
        false => "--run-scaffold",
    };

    let _ = writeln!(
        std::io::stderr(),
        "mix: `{command}` was not run — nothing here could be asked. `{flag}` agrees to it."
    );

    None
}

/// Print what a job's own command says, for as long as it says anything.
///
/// Roadmap task **T78a**. A task rather than part of the wait, because the two are different
/// streams: progress comes back on `job.wait` and output comes down `GET /logs/job/<id>`, and one
/// connection cannot carry both. [`None`] when the daemon cannot be reached a second time, which is
/// a log this command does without rather than an apply it refuses to run.
async fn watch_job_log(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    job: JobId,
    json: bool,
) -> Option<tokio::task::JoinHandle<()>> {
    let mut client = Client::connect(endpoint, autostart).await.ok()?;
    let path = format!("logs/job/{}", job.0);

    Some(tokio::spawn(async move {
        let _ = logs(&mut client, &path, 0, true, json).await;
    }))
}

/// Say where a job has got to, where it will not be mistaken for the command's answer.
fn report_progress(percent: u8, message: &str) {
    if message.is_empty() {
        return;
    }

    // Nothing to do about a stderr that will not take it — the answer the user asked for is still
    // going out on stdout. `writeln!` rather than `eprintln!`, which panics when stderr is closed.
    let _ = writeln!(std::io::stderr(), "  {percent:>3}%  {message}");
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
                    "a version (`8.3.33`), a series (`8.3`) or a caret (`^8.3`) — the same forms \
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

/// `mix job …`: one call, one rendering, and an exit status that means what a shell expects.
///
/// **A job that failed is an answer and not an error**, which is why this returns an [`ExitCode`]:
/// what happened is on stdout in both renderings, and what changes is the status — so
/// `mix job wait 3 && …` stops where a person reading the output would.
async fn job(
    command: JobCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    // **Only `wait` answers with the job's own outcome.** The other three did what they were asked
    // the moment the daemon answered — a status was reported, a cancellation was requested — and a
    // non-zero exit for `mix job status` on a job that failed yesterday would make asking about a
    // failure a failure.
    let (method, params, verdict) = match command {
        JobCommand::List { state, limit } => {
            let list: JobList = ask(
                &mut client,
                rpc::method::JOB_LIST,
                encode(&JobFilter { state, limit }),
            )
            .await?;
            emit(&rendered(json, &list, || render::job_list(&list)))?;
            return Ok(ExitCode::SUCCESS);
        }

        JobCommand::Status { job } => (
            rpc::method::JOB_STATUS,
            encode(&JobQuery { job: JobId(job) }),
            false,
        ),
        JobCommand::Cancel { job } => (
            rpc::method::JOB_CANCEL,
            encode(&JobQuery { job: JobId(job) }),
            false,
        ),

        // **The one job command that is not a call** — roadmap task **T78a**: output travels on its
        // own stream, never on the event stream and never as a method's answer (ADR 0009).
        JobCommand::Logs { job, follow, lines } => {
            return logs(&mut client, &format!("logs/job/{job}"), lines, follow, json).await;
        }
        JobCommand::Wait { job, timeout } => (
            rpc::method::JOB_WAIT,
            encode(&JobWait {
                job: JobId(job),
                timeout: Millis::from_secs(timeout),
            }),
            true,
        ),
    };

    let job: JobSummary = ask(&mut client, method, params).await?;
    emit(&rendered(json, &job, || render::job_status(&job)))?;

    // A wait that ran out is not a success either: it is what a script blocks on, and exiting zero
    // there would carry it past a download that is still running.
    Ok(match !verdict || render::job_succeeded(&job) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    })
}

/// `mix daemon stop`: stop the services, then the daemon.
///
/// **The answer arrives before the daemon goes**, which is what makes an exit code possible here at
/// all: the walk it carries says whether everything this home was running actually stopped, and a
/// service that refused is worth a non-zero status even though the daemon stopped regardless. What
/// happens to the connection a moment later is not this command's business — the response has been
/// read by then.
///
/// **A stop that could not be ordered is the same kind of non-zero**, and for the same reason rather
/// than by analogy: the exit code here has never meant "the daemon stopped" — it means "what was
/// asked for happened" — and what `mix daemon stop` asks for is every service stopped in dependency
/// order. A daemon that could not work one out stopped them all at the same moment instead, which is
/// the arrangement the ordering exists to prevent, and exiting `0` would carry a
/// `mix daemon stop && …` past it in silence. Both halves are still on stdout in both renderings;
/// only the status changes.
async fn daemon_stop(endpoint: &Endpoint, json: bool) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, None).await?;
    let shutdown: DaemonShutdown = ask(&mut client, rpc::method::DAEMON_SHUTDOWN, None).await?;

    emit(&rendered(json, &shutdown, || {
        render::daemon_shutdown(&shutdown)
    }))?;

    Ok(match (&shutdown.services.failed, &shutdown.unordered) {
        (None, None) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    })
}

/// `mix storage`: where this home's growing directories are — roadmap task **T145**.
///
/// **Neither asynchronous nor connected**, which is what makes it the odd one in the match above.
/// Every other command sends a request; this one runs `mixengined --storage` and reads the document
/// it printed. It has to: whether the choice is still free is a count of rows, `mix` links neither
/// `mixengine-core` nor `sqlx`, and there may be no daemon — a person asking this is usually asking
/// it *before* the first start.
///
/// `--json` hands back what the daemon printed, unchanged. Re-serialising a document this client
/// only parsed in order to print it would be a second chance to render it differently.
fn storage(root: &Path, json: bool) -> Result<ExitCode, Error> {
    let answered = autostart::ask(root, &["--storage"])?;

    if json {
        println!("{answered}");
        return Ok(ExitCode::SUCCESS);
    }

    let report: StorageReport = serde_json::from_str(&answered).map_err(|source| {
        Error::new(
            ErrorCode::Internal,
            format!("mixengined described this home in a way this `mix` cannot read: {source}"),
        )
        .with_hint(
            "the two binaries are from different releases — reinstall MixEngine so that \
                    `mix` and `mixengined` come from one",
        )
    })?;

    print!("{}", render::storage(&report));

    Ok(ExitCode::SUCCESS)
}

/// `mix status`: what the daemon says about itself.
async fn status(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;
    let status: DaemonStatus = ask(&mut client, rpc::method::DAEMON_STATUS, None).await?;

    match json {
        // The newline is here rather than in `render`, which builds a document and does not know
        // whether it is the last thing on the stream. The human rendering ends in one already.
        true => emit(&format!("{}\n", render::status_json(&status)))?,
        false => emit(&render::status(&status))?,
    }

    Ok(ExitCode::SUCCESS)
}

/// `mix service …`: one call, one rendering, and an exit code that means what a shell expects.
///
/// **A walk that failed is an answer and not an error**, which is why this returns an
/// [`ExitCode`] rather than reporting through [`report`]: a plan of six services where the fourth
/// fails leaves three running, one failed and two never tried, and all of that goes to stdout in
/// both renderings. What the failure changes is the exit status, so `mix service start db && …`
/// stops where a person reading the output would.
async fn service(
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

/// `mix service logs`: what a service has printed, and what it prints next.
///
/// **Written out as it arrives rather than collected**, which is the whole difference between this
/// and every other command here: a `--follow` never has a last message, and a buffer that filled
/// until the stream ended would print nothing for as long as the service kept running.
///
/// **The text goes out exactly as the service wrote it.** No timestamp, no `[stderr]`, nothing of
/// MixEngine's — for the same reason `current.log` carries none: this is piped into `grep` by
/// somebody who greps MariaDB's log the same way, and a prefix of ours would break every one of
/// those to restate what `--json` already carries. What the human rendering does add is the one
/// thing that is not output: a gap, on stderr, where the daemon or this client fell behind and lines
/// were lost. Silence there would make a log with a hole in it look complete.
/// `subject` is the route's two segments — `service/<id>` or `job/<id>` (roadmap task **T78a**) —
/// because what differs between the two is the path and nothing else this function does.
async fn logs(
    client: &mut Client,
    subject: &str,
    lines: usize,
    follow: bool,
    json: bool,
) -> Result<ExitCode, Error> {
    let path = format!("/{subject}?tail={lines}&follow={}", u8::from(follow));
    let mut stream = client.stream(&path).await?;

    while let Some(frame) = stream.next::<LogFrame>().await? {
        match (json, &frame) {
            // Verbatim, one object per line: a script filtering on `stream` or ordering by `at`
            // needs what the human rendering deliberately drops.
            (true, _) => emit(&format!(
                "{}\n",
                serde_json::to_string(&frame).expect("a proto type always serialises")
            ))?,

            (false, LogFrame::Line(line)) => emit(&format!("{}\n", line.text))?,
            (false, LogFrame::Historic { text }) => emit(&format!("{text}\n"))?,

            (false, LogFrame::Gap { missed }) => {
                report_gap(*missed);
            }

            // A variant from a later daemon. Ignored rather than refused, which is what the wire
            // types are `non_exhaustive` for.
            (false, _) => {}
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Say on stderr that lines were lost, so that a redirected log stays exactly the log.
fn report_gap(missed: u64) {
    let mut stderr = std::io::stderr();

    // Nothing to do about a stderr that will not take it, and nothing worth failing the command
    // over: the output the user asked for is still going out.
    let _ = writeln!(
        stderr,
        "mix: {missed} lines were dropped — this client fell behind the service"
    );
}

/// Call a method and decode what it answered.
///
/// **Decoded rather than passed through as the [`Value`](serde_json::Value) it arrived as, even for
/// `--json`.** The handshake has already established that this daemon speaks our protocol, so a
/// field this build cannot read is a bug worth reporting as one — and `--json` promising a
/// `ServiceWalk` means it has to be a `ServiceWalk` that goes out.
async fn ask<T: serde::de::DeserializeOwned>(
    client: &mut Client,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<T, Error> {
    let result = client.call(method, params).await?;

    serde_json::from_value(result).map_err(|error| {
        Error::new(
            ErrorCode::Internal,
            format!(
                "mix {} cannot read the answer to {method} from mixengined {}: {error}",
                env!("CARGO_PKG_VERSION"),
                client.daemon().version
            ),
        )
    })
}

/// The parameters of a call, as the wire carries them.
///
/// `expect` and not a failure path: every params type here is `mixengine-proto`'s and made of
/// strings, booleans and options, none of which can fail to serialise.
fn encode(params: &impl serde::Serialize) -> Option<serde_json::Value> {
    Some(serde_json::to_value(params).expect("a proto params type always serialises"))
}

/// One of the two renderings of an answer, ready to be written.
///
/// The `--json` half is the daemon's answer **verbatim**, unlike `mix status`, whose envelope exists
/// so a captured diagnostic says which `mix` produced it. A script asking about services wants
/// `.services[]` and `.failed.reason.kind` where the API names them, and the daemon's build is one
/// `mix status` away.
fn rendered(json: bool, answer: &impl serde::Serialize, human: impl FnOnce() -> String) -> String {
    match json {
        true => format!(
            "{}\n",
            serde_json::to_string(answer).expect("a proto answer type always serialises")
        ),
        false => human(),
    }
}

/// Put the command's answer on stdout.
///
/// `write!` and not `print!`, for the reason [`report`] gives for stderr — the macro panics when the
/// write fails — but the two failures it can meet are not the same failure and are not answered the
/// same way. A reader that went away, `mix status | head -1`, is not this program's problem and is
/// what every well-behaved tool exits quietly on. Anything else — a full disk, a handle closed
/// before the process started — is a command that did not deliver its answer, and says so in the
/// same wire error every other failure here uses.
///
/// Flushed explicitly, because the lock is a `LineWriter`: a rendering that reaches the buffer and
/// no further would otherwise fail on drop, where the error is discarded and this run would have
/// exited zero having printed nothing.
fn emit(rendered: &str) -> Result<(), Error> {
    let mut stdout = std::io::stdout().lock();

    // T182e: a reader that cannot decode UTF-8 — the Windows uninstaller, through `nsExec` — asks
    // for ASCII look-alikes of the typography this crate's sentences use.
    let rendered = match std::env::var_os(PLAIN_TEXT) {
        Some(_) => plain_text(rendered),
        None => rendered.to_owned(),
    };

    stdout
        .write_all(rendered.as_bytes())
        .and_then(|()| stdout.flush())
        .or_else(|source| match source.kind() {
            std::io::ErrorKind::BrokenPipe => Ok(()),
            _ => Err(Error::new(
                ErrorCode::Io,
                format!("cannot write to stdout: {source}"),
            )),
        })
}

/// Set, to anything, by a caller that reads `mix`'s output in a legacy code page — T182e. The
/// Windows uninstaller does, since `nsExec` decodes what it captures as ANSI.
const PLAIN_TEXT: &str = "MIXENGINE_PLAIN_TEXT";

/// `text` with the typographic characters `mix` and the daemon write — dashes, curly quotes, the
/// ellipsis — spelled in ASCII, and everything else, a person's name in a path included, as it was.
fn plain_text(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\u{2014}' | '\u{2013}' | '\u{2012}' | '\u{2212}' => plain.push('-'),
            '\u{2018}' | '\u{2019}' => plain.push('\''),
            '\u{201C}' | '\u{201D}' => plain.push('"'),
            '\u{2026}' => plain.push_str("..."),
            other => plain.push(other),
        }
    }
    plain
}

/// Put a failure where the person or the program running `mix` will find it.
///
/// **stderr, in both renderings.** stdout carries the command's answer and nothing else, so a script
/// that redirects it into a file gets either a status object or an empty file — never an error
/// object where a status was meant to be.
fn report(error: &Error, json: bool) {
    let mut stderr = std::io::stderr().lock();

    // The `Display` in `mixengine-proto` is the human rendering: the message, and the hint on a
    // line of its own the way `cargo` prints one. A wire error is three owned strings and cannot
    // fail to serialise, so the fallback is a formality rather than a case.
    let rendered = match json {
        true => serde_json::to_string(error).unwrap_or_else(|_| format!("error: {error}")),
        false => format!("error: {error}"),
    };

    // `writeln!` and not `eprintln!`, which panics if stderr is closed — `mix status 2>&-` in a
    // pipeline that has already gone away is not worth a panic message about a panic.
    let _ = writeln!(stderr, "{rendered}");
}

/// `2h` as a number of seconds, for `--for` — roadmap task **T76**.
///
/// **Hand-written rather than a dependency.** Four suffixes and a bare number of seconds is the
/// whole grammar this flag needs, and every entry in this workspace's `Cargo.toml` carries a
/// paragraph justifying itself; a parser for `2h` cannot write one.
///
/// # Errors
///
/// The sentence a person reads, for anything that is not a positive length of time. Zero is refused
/// rather than read as "no limit": a share that ends when it begins is not a share, and a flag
/// silently ignored is the other way to be wrong.
fn for_seconds(text: &str) -> Result<u64, String> {
    let refusal = || format!("`{text}` is not a length of time — try `30s`, `90m`, `2h` or `1d`");

    let split = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);

    let value: u64 = digits.parse().map_err(|_| refusal())?;

    let multiplier = match unit {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return Err(refusal()),
    };

    match value.checked_mul(multiplier) {
        Some(0) | None => Err(refusal()),
        Some(seconds) => Ok(seconds),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T182e. The uninstaller reads `mix` through `nsExec`, which decodes in the ANSI code page, so
    /// under `MIXENGINE_PLAIN_TEXT` the typographic characters become their ASCII look-alikes — the
    /// firewall row read `MixEngine â€” shared sites` in the uninstaller's log.
    #[test]
    fn plain_text_spells_typography_in_ascii() {
        assert_eq!(
            plain_text("MixEngine — shared sites, 1–2, ‘a’ “b” …"),
            "MixEngine - shared sites, 1-2, 'a' \"b\" ..."
        );
    }

    /// And leaves everything else alone — a path with a non-English name is still that path.
    #[test]
    fn plain_text_leaves_other_characters_alone() {
        assert_eq!(plain_text(r"C:\Users\Nguyễn\x"), r"C:\Users\Nguyễn\x");
    }

    /// **T151.** Nothing is asked, and nothing refused here, unless MixEngine could install it.
    #[test]
    fn only_an_installable_lack_is_asked_about() {
        use mixengine_proto::{Need, RedistributableArch};

        let installable = Requirement {
            need: Need::VisualCpp {
                year: "2019".to_owned(),
                arch: RedistributableArch::X64,
                found: None,
            },
            remedy: Remedy::InstallVisualCpp {
                arch: RedistributableArch::X64,
            },
        };
        let escapable = Requirement {
            need: Need::Glibc {
                at_least: "2.34".to_owned(),
                found: "2.31".to_owned(),
            },
            remedy: Remedy::ChooseVersion {
                version: PackageVersion::parse("8.3.33").unwrap(),
            },
        };

        assert_eq!(
            agreed_to_prerequisites(&[], false, true).unwrap(),
            Some(false)
        );
        assert_eq!(
            agreed_to_prerequisites(&[escapable], false, true).unwrap(),
            Some(false)
        );
        assert_eq!(
            agreed_to_prerequisites(std::slice::from_ref(&installable), true, true).unwrap(),
            Some(true)
        );
        assert!(
            agreed_to_prerequisites(&[installable], false, true).is_err(),
            "--json needs --yes"
        );

        // **A library the distribution provides asks nothing** — roadmap task T27e, D16.
        let advisory = Requirement {
            need: Need::SharedLibrary {
                soname: "libasound.so.2".to_owned(),
            },
            remedy: Remedy::InstallFromDistribution,
        };
        assert_eq!(
            agreed_to_prerequisites(&[advisory], false, true).unwrap(),
            Some(false),
            "it goes on, and nothing was agreed to"
        );
    }

    /// **T135, D12.** Three flags build the whole list in one request — no read-modify-write in the
    /// client, so no race and no business logic here.
    #[test]
    fn three_flags_build_one_route_list() {
        let routes = site_routes(
            &["/api=http://127.0.0.1:3003/xyz".to_owned()],
            &["/admin=php-fpm@8.3.33".to_owned(), "/old".to_owned()],
            &["/assets=dist".to_owned()],
            false,
        )
        .expect("a list")
        .expect("some routes");

        assert_eq!(routes.len(), 4);
        assert_eq!(routes[0].path, "/api");
        assert_eq!(
            routes[0].target,
            RouteTarget::Proxy {
                upstream: "http://127.0.0.1:3003/xyz".to_owned()
            }
        );
        assert_eq!(
            routes[2].target,
            RouteTarget::PhpFpm { pool: None },
            "a `--php` with no pool leaves the resolution to the daemon, as a site does"
        );
        assert_eq!(
            routes[3].target,
            RouteTarget::Static {
                root: "dist".to_owned()
            }
        );

        // Nothing typed leaves the list alone; `--no-routes` empties it. Two different requests.
        assert_eq!(site_routes(&[], &[], &[], false).expect("nothing"), None);
        assert_eq!(
            site_routes(&[], &[], &[], true).expect("cleared"),
            Some(Vec::new())
        );

        // `--no-routes` beside a route is refused rather than ordered. clap refuses it first; this
        // keeps the function honest for any caller that is not clap.
        assert!(site_routes(&["/a=http://h:1".to_owned()], &[], &[], true).is_err());

        // A value with no `=` is a proxy route with nowhere to go.
        assert!(site_routes(&["/a".to_owned()], &[], &[], false).is_err());
    }

    #[test]
    fn a_length_of_time_is_read_from_its_suffix() {
        assert_eq!(for_seconds("30s"), Ok(30));
        assert_eq!(for_seconds("30"), Ok(30));
        assert_eq!(for_seconds("90m"), Ok(5_400));
        assert_eq!(for_seconds("2h"), Ok(7_200));
        assert_eq!(for_seconds("1d"), Ok(86_400));
    }

    /// **Zero is refused rather than treated as "no limit".** A share that ends the instant it
    /// begins is not a share, and silently ignoring the flag would be the other way to be wrong.
    #[test]
    fn a_length_of_time_of_zero_or_a_word_is_refused() {
        assert!(for_seconds("0").is_err());
        assert!(for_seconds("0h").is_err());
        assert!(for_seconds("soon").is_err());
        assert!(for_seconds("2 hours").is_err());
        assert!(for_seconds("").is_err());
        assert!(for_seconds("-1").is_err());
    }

    /// A length large enough to overflow is a refusal and not a wrap.
    #[test]
    fn a_length_too_large_to_hold_is_refused() {
        assert!(for_seconds(&format!("{}d", u64::MAX)).is_err());
    }

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

    /// **The question names which kind of untrusted it is** — roadmap task **T79b**. It is asked
    /// only on a real apply, so this is where the three sentences are checked.
    #[test]
    fn the_scaffold_question_says_what_is_known_about_the_blueprint() {
        assert!(super::vouching(false, Some(SignatureCheck::Verified)).contains("is signed"));

        let rejected = super::vouching(true, Some(SignatureCheck::Rejected));
        assert!(rejected.contains("not the gallery's"), "{rejected}");

        let missing = super::vouching(true, Some(SignatureCheck::Missing));
        assert!(missing.contains("Nothing vouches"), "{missing}");

        // A row older than this task, or one whose reason this build cannot read: the sentence it
        // has always had, which says the true half of what is known.
        assert_eq!(super::vouching(true, None), missing);
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
