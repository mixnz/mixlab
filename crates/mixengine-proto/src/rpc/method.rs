/// Everything the daemon knows about itself and its home. See [`DaemonStatus`](crate::DaemonStatus).
pub const DAEMON_STATUS: &str = "daemon.status";

/// Build and protocol version alone — the cheap half of [`DAEMON_STATUS`], for the handshake a
/// client does before it decides whether it can talk to this daemon at all.
pub const DAEMON_VERSION: &str = "daemon.version";

/// Stop every supervised service, in reverse dependency order, and then stop.
///
/// Takes no parameters and answers [`DaemonShutdown`](crate::DaemonShutdown) — the *total*
/// budget it spends on that is `config.toml`'s, not a caller's, because it is a property of the
/// machine and not of whoever happens to be typing. The answer is written before the daemon
/// goes; the connection ending afterwards is the shutdown, not a failure.
pub const DAEMON_SHUTDOWN: &str = "daemon.shutdown";

/// Choose where this home keeps its passwords from the next start. Takes
/// [`CredentialStoreSet`](crate::CredentialStoreSet), answers
/// [`CredentialStoreChange`](crate::CredentialStoreChange).
///
/// **Refused while the current store holds anything for this home**, naming what it holds, and
/// on a release refused for the file anywhere but a Linux machine with no credential store
/// ([ADR 0059]). Nothing moves: the running daemon keeps its host, and the next start reads the
/// recorded choice.
///
/// [ADR 0059]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0059-a-linux-release-may-keep-a-homes-credentials-in-a-file.md
pub const DAEMON_SET_CREDENTIAL_STORE: &str = "daemon.set_credential_store";

/// Examine this machine and say what was found. Takes nothing, answers
/// [`DoctorReport`](crate::DoctorReport).
///
/// **A read in the strict sense**: no row is written, no file is written, nothing is enqueued,
/// and no elevation prompt can result from calling it. That is what makes it safe on a timer and
/// from a diagnostics bundle. Repairing what it finds is [`DAEMON_DOCTOR_REPAIR`].
pub const DAEMON_DOCTOR: &str = "daemon.doctor";

/// Repair what `daemon.doctor` found — everything this build can act on, in one call.
///
/// Takes [`DoctorRepair`](crate::DoctorRepair), answers [`RepairReport`](crate::RepairReport).
///
/// **The one method here that can raise an elevation prompt, and it raises at most one** — and
/// only when asked to. Repairs that need the helper enqueue; whether the queue is then flushed in
/// this same call is `grant`, which exists so that the rule every elevation follows survives:
/// what is about to be allowed is shown before it is allowed. A repair that lives inside
/// `MIXENGINE_HOME` prompts for nothing either way.
pub const DAEMON_DOCTOR_REPAIR: &str = "daemon.doctor_repair";

/// One diagnostics archive, written into this home and named in the answer.
///
/// Takes [`DiagnosticsBundle`](crate::DiagnosticsBundle), answers
/// [`BundleReport`](crate::BundleReport).
///
/// **Reads the machine and writes one file: the archive itself**, inside
/// `<root>/cache/diagnostics/`. No row, no queue, no prompt — which is what makes it safe to
/// offer beside a failure rather than only when things are calm. What it carries is a closed
/// list ([`Part`](crate::Part)) and what it refuses to carry is named in the answer.
pub const DAEMON_BUNDLE: &str = "daemon.bundle";

/// What an uninstall would take off this machine. Takes
/// [`UninstallQuery`](crate::UninstallQuery), answers
/// [`UninstallReport`](crate::UninstallReport).
///
/// **A read in the strict sense**, exactly as [`DAEMON_DOCTOR`] is: no row is written, nothing
/// is enqueued, and no elevation prompt can result from calling it. That is what makes it safe
/// to put in front of the one command that cannot be undone. Acting on what it found is
/// [`DAEMON_UNINSTALL`].
pub const DAEMON_UNINSTALL_PLAN: &str = "daemon.uninstall_plan";

/// Take MixEngine off this machine. Takes [`UninstallQuery`](crate::UninstallQuery), answers a
/// [`JobSummary`](crate::JobSummary) whose result is an
/// [`UninstallReport`](crate::UninstallReport).
///
/// **A job, because it can raise the elevation prompt** — what it then waits on is a person
/// reading a dialog, which has no deadline. It raises at most one, and only when `grant` says
/// so; the two-call path is [`DAEMON_UNINSTALL_PLAN`] followed by this.
///
/// **And unless `keep_home` is set it ends this daemon**, because a daemon whose home has been
/// removed has nothing left to serve. The job's result is written before the process goes.
pub const DAEMON_UNINSTALL: &str = "daemon.uninstall";

/// Where this home's disk has gone, category by category. Takes
/// [`DiskUsageQuery`](crate::DiskUsageQuery), answers [`DiskUsage`](crate::DiskUsage).
///
/// **A read in the strict sense**, exactly as [`DAEMON_DOCTOR`] and [`DAEMON_UNINSTALL_PLAN`]
/// are: no row is written, no file is written, nothing is enqueued, and no elevation prompt can
/// result from calling it. It is also a walk of tens of thousands of files, so the daemon keeps
/// the last reading for a minute — [`DiskUsageQuery::refresh`](crate::DiskUsageQuery::refresh)
/// forces a fresh one and [`DiskUsage::measured_at`](crate::DiskUsage::measured_at) says which
/// happened.
///
/// Acting on what it found is [`DAEMON_CLEANUP`], and only two of the five rows can be acted on
/// at all.
pub const DAEMON_DISK_USAGE: &str = "daemon.disk_usage";

/// Remove what is safe to lose. Takes [`CleanupQuery`](crate::CleanupQuery), answers a
/// [`JobSummary`](crate::JobSummary) whose result is a [`CleanupReport`](crate::CleanupReport).
///
/// **What it may remove is a closed list, never a walk of the home** — [`DAEMON_BUNDLE`]'s rule
/// applied to deletion. Rotated log files and `cache/`; nothing else, whatever
/// [`DAEMON_DISK_USAGE`]'s sum says. The live log files, `logs/crashes/`, `data/`, `runtimes/`
/// and `certs/` are out of reach by construction rather than by a check somebody has to
/// remember.
///
/// **A job, and one that refuses to run beside another.** `cache/downloads/` holds a resumable
/// download [`RUNTIME_INSTALL`] may be writing to and `cache/updates/` a payload
/// [`UPDATE_APPLY`] is about to run; unlinking either mid-flight breaks an install for a reason
/// nothing in its own log explains. So a second job running is `precondition_failed`, naming it.
pub const DAEMON_CLEANUP: &str = "daemon.cleanup";

/// What this home's certificate authority is: its subject, its fingerprint and how long it has
/// left.
///
/// Takes [`CaStatusQuery`](crate::CaStatusQuery), answers [`CaStatus`](crate::CaStatus).
///
/// **Never the private key.** `docs/architecture/security-model.md` says it is never copied,
/// exported by an RPC, or sent to a client, and [`CaStatus`](crate::CaStatus) has no field one
/// could travel in.
///
/// Whether an operating system *trusts* the certificate is a different question, about a
/// different subsystem: the operating system's trust store, which this method does not read.
pub const CERT_CA_STATUS: &str = "cert.ca_status";

/// What this daemon knows about updating itself, without going to the network. Takes nothing,
/// answers [`UpdateStatus`](crate::UpdateStatus).
///
/// The cheap one: it reports the last feed this daemon verified, whenever that was. What forces
/// a request is [`UPDATE_CHECK`].
pub const UPDATE_STATUS: &str = "update.status";

/// Read the published feed now. Takes [`UpdateCheck`](crate::UpdateCheck), answers
/// [`UpdateStatus`](crate::UpdateStatus).
///
/// **Goes to the network**, which is the whole of the difference from [`UPDATE_STATUS`]. A
/// failure is not an error here: the last document this daemon verified is answered instead,
/// with [`UpdateStatus::stale`](crate::UpdateStatus::stale) set.
pub const UPDATE_CHECK: &str = "update.check";

/// Remember *skip this version* or *remind me later*. Takes
/// [`UpdateDecide`](crate::UpdateDecide), answers the [`UpdateStatus`](crate::UpdateStatus) that
/// decision produced.
///
/// Answering with the new status rather than with nothing is what lets a client show the effect
/// of the answer it just sent without a second round trip.
pub const UPDATE_DECIDE: &str = "update.decide";

/// Install it. Takes [`UpdateApply`](crate::UpdateApply), answers
/// [`UpdateApplied`](crate::UpdateApplied) — **and then the daemon exits**.
///
/// The one method whose answer is followed by the connection closing on purpose, as
/// [`DAEMON_SHUTDOWN`] is: what the client does next is start the new daemon. The version is
/// taken rather than implied so that a check landing between the prompt and the answer cannot
/// install something nobody read the notes for.
///
/// Refused with `precondition_failed` when this copy of MixEngine was installed by a package
/// manager, before anything is downloaded.
pub const UPDATE_APPLY: &str = "update.apply";

/// Download and verify the next `.pkg`, and open it in Installer.app. Takes
/// [`UpdateHandOver`](crate::UpdateHandOver), answers
/// [`UpdateHandedOver`](crate::UpdateHandedOver). **The daemon keeps running**, and nothing is
/// stopped: a person who cancels the installer has lost nothing.
///
/// Refused with `precondition_failed` for a copy the `.pkg` did not install.
pub const UPDATE_HAND_OVER: &str = "update.hand_over";

/// Once the `.pkg` has installed, stop, remember what was running, answer and exit — the second
/// half of [`UPDATE_APPLY`], for a copy the `.pkg` installed. Takes
/// [`UpdateFinish`](crate::UpdateFinish), answers [`UpdateApplied`](crate::UpdateApplied) with
/// nothing in `replaced`.
///
/// Refused with `precondition_failed` until `update.status` carries `installed`.
pub const UPDATE_FINISH: &str = "update.finish";

/// Give a site the certificate its names need — or every HTTPS site one. Takes
/// [`CertIssue`](crate::CertIssue), answers [`CertIssueReport`](crate::CertIssueReport).
///
/// Idempotent: a certificate that still covers the right names, has more than thirty days left
/// and was signed by the authority this home has now is reused rather than replaced.
pub const CERT_ISSUE: &str = "cert.issue";

/// What this home's certificates are doing, per site, **including a live TLS handshake** against
/// the running front end. Takes [`CertStatusQuery`](crate::CertStatusQuery), answers
/// [`CertStatusReport`](crate::CertStatusReport).
///
/// The one question in this API that is not answered from a file: it opens a socket to this
/// home's own front end and reports the certificate that server presents, which is the only
/// thing a browser ever sees.
///
/// Reads only. Nothing is issued, nothing is installed and nothing is reloaded — a diagnostic
/// that repaired what it found would be unable to report the state it had just repaired.
pub const CERT_STATUS: &str = "cert.status";

/// Replace this home's certificate authority with a new one. Takes
/// [`CaRotateQuery`](crate::CaRotateQuery), answers a [`JobSummary`](crate::JobSummary) whose
/// result is a [`CaRotateReport`](crate::CaRotateReport).
///
/// The one destructive operation on certificates: every browser holding a cached chain under the
/// old authority stops accepting it.
///
/// **A job because it waits for a person.** One elevation prompt covers taking the old
/// certificate out and putting the new one in, and nothing is committed unless a fresh reading
/// of the store says this machine trusts the new authority. A declined prompt leaves this home
/// exactly as it was.
pub const CERT_CA_ROTATE: &str = "cert.ca_rotate";

/// Take this home's authority out of every store that trusts it. Takes
/// [`CaUninstallQuery`](crate::CaUninstallQuery), answers a [`JobSummary`](crate::JobSummary)
/// whose result is a [`CaUninstallReport`](crate::CaUninstallReport).
///
/// **Trust and never a file**: `certs/ca/` and every leaf are left where they are, because
/// removing trust is undone by `mix doctor --repair` and deleting a private key is undone by
/// nothing. Deleting is [`DAEMON_UNINSTALL`]'s.
pub const CERT_CA_UNINSTALL: &str = "cert.ca_uninstall";

/// Every version of every runtime the index offers **for this machine**, and whether each is
/// already here. Takes [`RuntimeFilter`](crate::RuntimeFilter), answers
/// [`RuntimeCatalogue`](crate::RuntimeCatalogue).
///
/// Reaches the network, and answers from the last verified index when it cannot — the
/// `stale` flag beside the list is what says which happened.
/// [`RuntimeFilter::refresh`](crate::RuntimeFilter::refresh) skips a still-fresh cache.
pub const RUNTIME_LIST_AVAILABLE: &str = "runtime.list_available";

/// Every runtime on this machine. Takes [`RuntimeFilter`](crate::RuntimeFilter), answers
/// [`RuntimeList`](crate::RuntimeList).
pub const RUNTIME_LIST_INSTALLED: &str = "runtime.list_installed";

/// Download and install one version. Takes [`RuntimeInstall`](crate::RuntimeInstall) and answers
/// a [`JobSummary`](crate::JobSummary) — **the first method in this API that returns a job**.
///
/// The call comes back as soon as the row is written and the work is spawned; progress arrives
/// as [`JobProgress`](crate::DaemonEvent::JobProgress) events, and
/// [`JOB_WAIT`] is what a script blocks on. What the finished job carries as its result is a
/// [`RuntimeSummary`](crate::RuntimeSummary).
pub const RUNTIME_INSTALL: &str = "runtime.install";

/// What one version lacks on this machine, and what can be done about each. Takes
/// [`RuntimeTarget`](crate::RuntimeTarget), answers [`Requirements`](crate::Requirements).
///
/// What a client asks **before** `runtime.install`, so it can ask the person once rather than
/// meet a refusal. The install judges again regardless.
pub const RUNTIME_REQUIREMENTS: &str = "runtime.requirements";

/// Remove one version from this machine. Takes [`RuntimeTarget`](crate::RuntimeTarget), answers
/// [`RuntimeRemoval`](crate::RuntimeRemoval).
///
/// Not a job, unlike its opposite: removing a directory is bounded by the disk rather than by
/// somebody's connection, and a call that answers in a moment should not make every client
/// learn a second protocol to hear the answer.
pub const RUNTIME_UNINSTALL: &str = "runtime.uninstall";

/// Make one installed version the one its kind resolves to. Takes
/// [`RuntimeTarget`](crate::RuntimeTarget), answers the
/// [`RuntimeSummary`](crate::RuntimeSummary) that is now the default.
pub const RUNTIME_SET_DEFAULT: &str = "runtime.set_default";

/// Record a version that is on disk without a row, such as one an earlier home left. Takes
/// [`RuntimeTarget`](crate::RuntimeTarget), answers the [`RuntimeSummary`](crate::RuntimeSummary)
/// now recorded, or the one already there.
///
/// Checked by its marker, or against the package index: every file the build provides is there,
/// and it runs. Nothing is downloaded and nothing on disk is removed.
pub const RUNTIME_ADOPT: &str = "runtime.adopt";

/// Every runtime directory on disk with no row, and why. Takes nothing, answers
/// [`RuntimeFoundList`](crate::RuntimeFoundList). Offline and a read: no index
/// is fetched and nothing is recorded; [`RUNTIME_ADOPT`] is what checks and records one.
pub const RUNTIME_FOUND: &str = "runtime.found";

/// What one installed version can load, and what it does load. Takes
/// [`RuntimeTarget`](crate::RuntimeTarget), answers
/// [`ExtensionList`](crate::ExtensionList).
///
/// Answers for every runtime and not only PHP: a kind whose artifacts declare no loadable
/// modules answers an empty list rather than a refusal, which is what keeps a client from having
/// to know which languages have extensions.
pub const RUNTIME_LIST_EXTENSIONS: &str = "runtime.list_extensions";

/// Turn one extension on or off for one installed version. Takes
/// [`ExtensionChoice`](crate::ExtensionChoice), answers
/// [`ExtensionChange`](crate::ExtensionChange).
///
/// **Version-wide**, deliberately: extensions load when a process starts, and one php-fpm pool
/// serves every site on that version — a per-site set would mean a pool per site.
///
/// Answers `unsupported_platform` for an extension this build compiles in, because what it would
/// take is a different build rather than a different setting, and `not_found` for a name it does
/// not ship.
pub const RUNTIME_SET_EXTENSION: &str = "runtime.set_extension";

/// Which installed version a directory uses, and why that one. Takes
/// [`RuntimeQuestion`](crate::RuntimeQuestion), answers
/// [`ResolvedRuntime`](crate::ResolvedRuntime).
///
/// **The one `runtime.*` method that decides rather than reports.** Everything above it names a
/// version; this one is handed a constraint at most, and works out which of the installed
/// versions answers it — from the caller's flag, from the `mixengine.toml` above the directory,
/// from the project registered there, or from the kind's default. It is a method rather than
/// something each client works out for itself because two clients disagreeing about which PHP a
/// directory uses is the one bug this whole product exists to prevent.
///
/// Answers `dependency_missing` when nothing installed satisfies it, with the install command
/// in the hint.
pub const RUNTIME_RESOLVE: &str = "runtime.resolve";

/// What updating one installed version within its line would do, changing nothing. Takes
/// [`RuntimeUpgradeQuery`](crate::RuntimeUpgradeQuery), answers
/// [`UpgradePlan`](crate::UpgradePlan).
pub const RUNTIME_UPGRADE_PLAN: &str = "runtime.upgrade_plan";

/// Update one installed version to a newer release of its line, moving everything that points
/// at it. Takes [`RuntimeUpgrade`](crate::RuntimeUpgrade), answers a
/// [`JobSummary`](crate::JobSummary) whose result is the [`UpgradePlan`](crate::UpgradePlan)
/// marked with what was done.
pub const RUNTIME_UPGRADE: &str = "runtime.upgrade";

/// Whether a new terminal will find the commands in `<root>/bin`. Takes no parameters, answers
/// [`PathReport`](crate::PathReport).
///
/// Reads the *persisted* PATH — the registry value on Windows, the shell profiles on both
/// others — and never the daemon's own environment, which is whatever started it.
pub const PATH_STATUS: &str = "path.status";

/// Fill `<root>/bin` and put it on this user's PATH. Takes no parameters, answers
/// [`PathReport`](crate::PathReport).
///
/// **Both halves, because either alone does nothing**: a PATH entry naming a directory with no
/// `php` in it is inert, and a directory full of shims nothing can find is a directory. Wholly
/// idempotent — a location that already carries the entry is left byte for byte alone and comes
/// back as `changed: false`.
///
/// **Nothing about it is elevated.** The user's own environment is user-writable on all three
/// systems, which is why this is an ordinary method and not a
/// `PrivilegedOp` — see `docs/architecture/platform-abstraction.md`.
pub const PATH_INSTALL: &str = "path.install";

/// Take `<root>/bin` back off this user's PATH. Takes no parameters, answers
/// [`PathReport`](crate::PathReport).
///
/// The shims are **left in place**: they are inside the home, they cost nothing there, and
/// removing what makes the home work in order to undo a line in a profile would be an
/// uninstall wearing a smaller command's name.
pub const PATH_UNINSTALL: &str = "path.uninstall";

/// Compare `<root>/bin` against what is installed, now. Takes no parameters, answers
/// [`PathReport`](crate::PathReport).
///
/// The daemon hears a tool somebody installed into a runtime through a watch on its bindir, so
/// this is for a bindir the system would not let it watch, and for a person who wants to know
/// *now* whether MixEngine agrees with their disk.
///
/// Idempotent and cheap: a pass that finds nothing changed copies nothing.
pub const PATH_RESCAN: &str = "path.rescan";

/// Whether this machine starts a daemon for this home at login. Takes no parameters, answers
/// [`AutostartReport`](crate::AutostartReport).
///
/// **Its own namespace and not `daemon.*`**, because `daemon.*` is about the daemon that is
/// running and this is about a setting on the machine that outlives every daemon which ever
/// registered it. `path.*` above is the precedent for a capability holding three methods of its
/// own.
///
/// Never fails for want of a mechanism: a machine that cannot start anything at login answers
/// [`AutostartMechanism::None`](crate::AutostartMechanism), because a status reports rather than
/// refuses.
pub const AUTOSTART_STATUS: &str = "autostart.status";

/// Register the entry. Takes no parameters, answers
/// [`AutostartReport`](crate::AutostartReport).
///
/// **Registers; it does not start.** Somebody who asked for "start it at login" did not ask for
/// "start it", and there is a daemon running already by the time this can be called.
///
/// Wholly idempotent — an entry that already says exactly this is left alone and comes back as
/// `changed: false`. Called from a second home it **replaces** the entry, because there is one
/// per user: see [`AutostartReport::for_this_home`](crate::AutostartReport).
///
/// **Nothing about it is elevated.** A logon task, a LaunchAgent and a systemd *user* unit all
/// belong to the account MixEngine runs as, which is why this is an ordinary method and not a
/// `PrivilegedOp` — see `docs/architecture/platform-abstraction.md`.
///
/// Answers `unsupported` on a machine with no mechanism at all, with the command to run by hand
/// in the hint.
pub const AUTOSTART_ENABLE: &str = "autostart.enable";

/// Take the entry away again. Takes no parameters, answers
/// [`AutostartReport`](crate::AutostartReport).
///
/// **Removes; it does not stop.** A person turning off "start at login" must not lose the daemon
/// they are using, so nothing here touches a running process.
pub const AUTOSTART_DISABLE: &str = "autostart.disable";

/// Also start at boot, before anyone logs in. Takes no parameters, answers
/// [`AutostartReport`](crate::AutostartReport).
///
/// Registers the login entry as [`AUTOSTART_ENABLE`] does — boot start without it would start
/// nothing — and then turns on the account's systemd linger, as the account and never elevated.
/// If the linger half is refused the entry stays registered and the hint names the command to run
/// with `sudo`. Answers `unsupported` where only an administrator could arrange it.
pub const AUTOSTART_ENABLE_AT_BOOT: &str = "autostart.enable_at_boot";

/// Stop starting at boot, and keep starting at login. Takes no parameters, answers
/// [`AutostartReport`](crate::AutostartReport).
///
/// Turns the account's linger off only if MixEngine turned it on; a linger the account set is left
/// alone and the answer says `changed: false`. Stops nothing that is running.
pub const AUTOSTART_DISABLE_AT_BOOT: &str = "autostart.disable_at_boot";

/// Every declared service and what it is doing. See [`ServiceList`](crate::ServiceList).
pub const SERVICE_LIST: &str = "service.list";

/// One of them, by id — the same [`ServiceSummary`](crate::ServiceSummary) a list is made of.
/// Takes [`ServiceQuery`](crate::ServiceQuery).
pub const SERVICE_STATUS: &str = "service.status";

/// Start a service and everything it depends on. Takes
/// [`ServiceTarget`](crate::ServiceTarget), answers [`ServiceWalk`](crate::ServiceWalk).
pub const SERVICE_START: &str = "service.start";

/// Stop a service and everything that depends on it. Same types as [`SERVICE_START`], and the
/// opposite walk rather than the same one reversed.
pub const SERVICE_STOP: &str = "service.stop";

/// Take a service down and put back exactly what went down with it. Same types again.
pub const SERVICE_RESTART: &str = "service.restart";

/// Re-set one database's superuser credential inside its own data directory. Takes
/// [`ResetCredential`](crate::ResetCredential), answers [`ServiceWalk`](crate::ServiceWalk).
///
/// The repair for a server that refuses the password this home holds for it. A database keeps
/// its own copy of that password inside its data directory and the keyring holds the other; they
/// are written together by the first run and can only come apart one way. Once they have, no
/// client can put them back, because every way of changing the one inside the directory needs
/// the credential that was lost.
///
/// So this stops the service and everything that depends on it, writes the credential this home
/// holds into the data directory through the recipe's own offline step — a server that listens
/// on nothing — and starts back what went down. **Every database in the directory is kept.**
///
/// Also the name of the job it performs the work in, which is where the log of it lives.
pub const SERVICE_RESET_CREDENTIAL: &str = "service.reset_credential";

/// Service data an earlier home left under `data/`, with no service row. Takes nothing, answers
/// [`ServiceFoundList`](crate::ServiceFoundList): each directory, the version that made it, and
/// the installed version that would open it or why none does. A read: nothing is written.
pub const SERVICE_FOUND: &str = "service.found";

/// A copy of an earlier home's state in a folder this home keeps. Takes nothing, answers
/// [`HomePrevious`](crate::HomePrevious): what the newest copy holds,
/// while this home has no project, site or package's service of its own yet. A read.
pub const HOME_PREVIOUS: &str = "home.previous";

/// Restore that copy into this home. Takes nothing, answers
/// [`HomeRestoreReport`](crate::HomeRestoreReport). Projects, sites, domains, routes and
/// services come back; every database gets a new admin password and is left stopped; the hosts
/// file and the certificates of HTTPS sites are asked for as a site create asks for them.
pub const HOME_RESTORE: &str = "home.restore";

/// Turn one found data directory back into a service. Takes [`ServiceAdopt`](crate::ServiceAdopt),
/// answers the [`ServiceSummary`](crate::ServiceSummary) of the service, left stopped.
///
/// `service.create` with that id, whose data directory is the one found, then — for a database
/// — `service.reset_credential`, which writes a new admin password into it. Every database and
/// account inside is kept. An id that already has a row answers that row.
pub const SERVICE_ADOPT: &str = "service.adopt";

/// Every service package on this machine. Takes [`PackageFilter`](crate::PackageFilter),
/// answers [`PackageList`](crate::PackageList).
///
/// A *package* here is a server, a database or a cache — never a runtime. Those have
/// [`RUNTIME_LIST_INSTALLED`], a default version and a shim that reads it.
pub const PACKAGE_LIST: &str = "package.list";

/// Every version of every package this build can run, as the index offers them **for this
/// machine**, and whether each is already here. Takes
/// [`PackageFilter`](crate::PackageFilter), answers
/// [`PackageCatalogue`](crate::PackageCatalogue).
///
/// Reaches the network, and answers from the last verified index when it cannot — the `stale`
/// flag beside the list is what says which happened.
/// [`PackageFilter::refresh`](crate::PackageFilter::refresh) skips a still-fresh cache.
///
/// **Names only what this build has a recipe for.** An index entry MixEngine cannot configure is
/// a download ending in a directory nothing can use, so it is not offered at all.
pub const PACKAGE_LIST_AVAILABLE: &str = "package.list_available";

/// Download and unpack one version of one package. Takes
/// [`PackageInstall`](crate::PackageInstall), answers a [`JobSummary`](crate::JobSummary).
///
/// A job for [`RUNTIME_INSTALL`]'s reason: an install is tens of megabytes over somebody's
/// connection, and a long operation returns a job rather than holding a call open. What the
/// finished job carries as its result is a [`PackageSummary`](crate::PackageSummary) — the same
/// sentence [`PACKAGE_LIST`] answers with.
pub const PACKAGE_INSTALL: &str = "package.install";

/// [`RUNTIME_REQUIREMENTS`] for a service package. Takes [`PackageTarget`](crate::PackageTarget),
/// answers [`Requirements`](crate::Requirements).
pub const PACKAGE_REQUIREMENTS: &str = "package.requirements";

/// Remove one version of one package. Takes [`PackageTarget`](crate::PackageTarget), answers
/// [`PackageRemoval`](crate::PackageRemoval).
///
/// **Refused while any service is an instance of it**, naming them: `services.package_id` is
/// `ON DELETE RESTRICT`, and what a person does about it is [`SERVICE_DELETE`].
pub const PACKAGE_UNINSTALL: &str = "package.uninstall";

/// Record a version that is on disk without a row. Takes
/// [`PackageTarget`](crate::PackageTarget), answers the
/// [`PackageSummary`](crate::PackageSummary) now recorded, or the one already there. Checked
/// as [`RUNTIME_ADOPT`] checks a runtime.
pub const PACKAGE_ADOPT: &str = "package.adopt";

/// Every package directory on disk with no row, and why. Takes nothing, answers
/// [`PackageFoundList`](crate::PackageFoundList), as [`RUNTIME_FOUND`] does for runtimes.
pub const PACKAGE_FOUND: &str = "package.found";

/// What updating one installed package version within its line would do, changing nothing.
/// Takes [`PackageUpgradeQuery`](crate::PackageUpgradeQuery), answers
/// [`UpgradePlan`](crate::UpgradePlan).
pub const PACKAGE_UPGRADE_PLAN: &str = "package.upgrade_plan";

/// Update one installed package version to a newer release of its line, moving its instances
/// one at a time. Takes [`PackageUpgrade`](crate::PackageUpgrade), answers a
/// [`JobSummary`](crate::JobSummary) whose result is the [`UpgradePlan`](crate::UpgradePlan)
/// marked with what was done.
pub const PACKAGE_UPGRADE: &str = "package.upgrade";

/// Create a service from an installed package. Takes
/// [`ServiceCreate`](crate::ServiceCreate), answers the
/// [`ServiceSummary`](crate::ServiceSummary) the new row became.
///
/// **The configuration is rendered before the answer**, so a service that could not be
/// configured is one that was never created — the config generator fails a whole declared set on
/// one bad row, and a row left behind would take every later `service.*` call down with it.
pub const SERVICE_CREATE: &str = "service.create";

/// Delete a service. Takes [`ServiceQuery`](crate::ServiceQuery), answers
/// [`ServiceRemoval`](crate::ServiceRemoval).
///
/// Takes the row and `etc/<service-id>/` with it and **never the data directory**, which the
/// answer names instead. The id is required for [`SERVICE_STATUS`]'s reason: a delete with no
/// subject is not a delete of everything.
pub const SERVICE_DELETE: &str = "service.delete";

/// Change which program every site on this machine is reached through. Takes
/// [`FrontEndSwitch`](crate::FrontEndSwitch), answers a [`JobSummary`](crate::JobSummary) whose
/// result is a [`FrontEndReport`](crate::FrontEndReport).
///
/// **A job and not a setting**, which is
/// [ADR 0026](https://github.com/mixnz/mixlab/blob/master/docs/decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md):
/// the row, the rendered sites and — on Linux — the port-80 grant all have to move together, and
/// on a machine where nobody grants the honest outcome is that the home **stays on the front end
/// it had**. That outcome is described rather than left as a home whose sites are rendered for a
/// server that cannot answer.
///
/// **There is no method for reading it back**, deliberately: [`SERVICE_LIST`] already answers,
/// through [`ServiceSummary::role`](crate::ServiceSummary).
pub const SERVICE_SET_FRONT_END: &str = "service.set_front_end";

/// What a service may take, and what this machine will actually enforce of it. Takes
/// [`ServiceTarget`](crate::ServiceTarget), answers
/// [`ServiceLimitsReport`](crate::ServiceLimitsReport).
///
/// **The declared limits and the machine's support come back together**, because neither is
/// worth reading alone: `512` means nothing until what it is measured as, and what happens at
/// the ceiling, are beside it.
pub const SERVICE_LIMITS: &str = "service.limits";

/// Replace what a service may take. Takes [`ServiceLimitsSet`](crate::ServiceLimitsSet), answers
/// [`ServiceLimitsReport`](crate::ServiceLimitsReport).
///
/// **The whole value, never a delta** — the hosts block's rule, applied to a much smaller thing
/// for its reason: a patch needs a three-way value per field, which puts an enum
/// into every reader of limits including the ones that only display them.
///
/// **Applied immediately.** A running service is re-capped before this answers, so there is no
/// state in which a limit is set and not in effect — and a service already over a newly lowered
/// memory ceiling can be killed by this call, which is the correct behaviour for the thing being
/// asked for.
pub const SERVICE_SET_LIMITS: &str = "service.set_limits";

/// When this service is stopped for being unused, and what is holding it open right now. Takes
/// [`ServiceTarget`](crate::ServiceTarget), answers [`IdleReport`](crate::IdleReport).
///
/// **The policy, where it came from, and what exempts it — all three.** *Why is this still
/// running?* has four answers that look identical from outside: no policy, a policy switched
/// off here, something running that depends on it, and a project being kept warm. A report that
/// collapsed them into one `Option` would send a person to change a setting that was never the
/// cause — the rule [`DnsStatus`](crate::DnsStatus) follows, at a smaller scale.
pub const SERVICE_IDLE: &str = "service.idle";

/// Replace how long this service may look idle before it is stopped. Takes
/// [`ServiceIdleSet`](crate::ServiceIdleSet), answers [`IdleReport`](crate::IdleReport).
///
/// **Three states rather than two**, and the third is what makes a later default safe: an
/// absent value restores the recipe's, `0` switches idle-stopping off for this service, and a
/// number is minutes. See [`IdleSource`](crate::IdleSource).
///
/// **Not applied to anything immediately**, unlike `service.set_limits`: what this changes is
/// when a future sweep will act, and a running service is neither stopped nor reprieved by the
/// call itself.
pub const SERVICE_SET_IDLE: &str = "service.set_idle";

/// Change a setting of a service that exists. Takes [`ServiceSet`](crate::ServiceSet), answers
/// [`ServiceSetResult`](crate::ServiceSetResult).
///
/// **Today the setting is the port**, and the call is the whole move: the port is checked the way
/// an allocated one is at create, the row changes, the configuration is rendered again, and a
/// running service is restarted — a port is one of the changes a reload cannot apply. A stopped
/// one stays stopped. When the restart fails on the new port, the service is put back on the old
/// one and the failure is the answer.
pub const SERVICE_SET: &str = "service.set";

/// Replace whether this service starts when the daemon does. Takes
/// [`ServiceAutostartSet`](crate::ServiceAutostartSet), answers
/// [`ServiceSummary`](crate::ServiceSummary).
///
/// **Nothing is started or stopped by the call**, on [`SERVICE_SET_IDLE`]'s reasoning: what this
/// changes is what the walk at the *next* daemon start covers, and starting the service now
/// would answer a different question than the one asked.
///
/// **What the walk covers is not only what carries the flag.** It is built as a start plan over
/// the flagged services, so a pool that is set brings up the database it depends on whether or
/// not that database is set — the same rule `service.start` on one service already follows.
pub const SERVICE_SET_AUTOSTART: &str = "service.set_autostart";

/// Whether this home stops services nobody is using. Takes nothing, answers
/// [`SaveResources`](crate::SaveResources).
pub const SERVICE_SAVE_RESOURCES: &str = "service.save_resources";

/// Turn the home's resource saving on or off. Takes
/// [`SaveResourcesSet`](crate::SaveResourcesSet), answers
/// [`SaveResources`](crate::SaveResources).
///
/// **Nothing is stopped or started by the call**, on [`SERVICE_SET_IDLE`]'s reasoning: the next
/// idle sweep reads the switch, and a service already past its saving policy is stopped by that
/// sweep, not by this. Off is the default ([ADR 0041]).
///
/// [ADR 0041]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0041-mixengine-stops-nothing-a-person-did-not-ask-it-to.md
pub const SERVICE_SET_SAVE_RESOURCES: &str = "service.set_save_resources";

/// Make a database and the account that reaches it, on one running instance. Takes
/// [`DatabaseCreate`](crate::DatabaseCreate), answers
/// [`DatabaseAccount`](crate::DatabaseAccount).
///
/// **Answers inline rather than with a job**, unlike every other method that can take minutes:
/// making a database is milliseconds, and what can be slow is getting there — the instance may
/// be stopped, and starting it may perform a first run. [`SERVICE_START`] already carries
/// exactly that cost this way, and a job here would be a second progress vocabulary for work
/// that is instant.
pub const DATABASE_CREATE: &str = "database.create";

/// Where one database instance could be opened, and with what. Takes
/// [`DatabaseClientQuery`](crate::DatabaseClientQuery), answers
/// [`DatabaseClientReport`](crate::DatabaseClientReport).
///
/// **Reads only**: starts nothing, launches nothing, touches no credential. It exists so a
/// client can draw the affordance from data rather than probing the filesystem for an
/// application — `docs/features/client-surface.md`.
pub const DATABASE_CLIENT: &str = "database.client";

/// Hand one instance to the installed desktop database client. Takes
/// [`DatabaseOpen`](crate::DatabaseOpen), answers [`DatabaseHandoff`](crate::DatabaseHandoff).
///
/// Starts the instance if it is stopped (on [`DATABASE_CREATE`]'s road), reads the account's
/// password from the OS keyring at that moment and puts it in the started process's environment
/// — never in an argument, the URL, a log or this answer. "Not installed" is a state in the
/// answer, not an error.
pub const DATABASE_OPEN: &str = "database.open";

/// The password MixEngine holds for one account. Takes
/// [`DatabaseCredentialsQuery`](crate::DatabaseCredentialsQuery), answers
/// [`DatabaseCredentials`](crate::DatabaseCredentials).
///
/// **Reads only**, like [`DATABASE_CLIENT`]: no instance is started, no statement runs. It
/// exists so a password already held can reach a project's `.env`, which no other method can do
/// — every other `database.*` answer names the address and never the value.
pub const DATABASE_CREDENTIALS: &str = "database.credentials";

/// The long operations this daemon has run, newest first. Takes
/// [`JobFilter`](crate::JobFilter), answers [`JobList`](crate::JobList).
pub const JOB_LIST: &str = "job.list";

/// One of them, by id — the same [`JobSummary`](crate::JobSummary) a list is made of. Takes
/// [`JobQuery`](crate::JobQuery).
pub const JOB_STATUS: &str = "job.status";

/// Block until a job finishes, or until the caller's timeout runs out.
///
/// Takes [`JobWait`](crate::JobWait) and answers the same [`JobSummary`](crate::JobSummary) as
/// [`JOB_STATUS`]. **The one method in this API that waits on purpose** — see
/// [`JobWait`](crate::JobWait) for why that does not contradict the rule it is an exception to,
/// and why a wait that runs out is an answer rather than an error.
pub const JOB_WAIT: &str = "job.wait";

/// Ask a running job to stop. Takes [`JobQuery`](crate::JobQuery), answers the
/// [`JobSummary`](crate::JobSummary) as it stands.
///
/// **Asking is all it does.** Cancellation is cooperative, so the summary that comes back may
/// still say `running` — the work ends when it next looks at the token, and
/// [`JobFinished`](crate::DaemonEvent::JobFinished) is what says it did. Cancelling a job that
/// has already ended changes nothing and is not an error: the caller wanted it stopped and it is.
pub const JOB_CANCEL: &str = "job.cancel";

/// Every subject's reading, taken now. Takes no parameters, answers
/// [`MetricsFrame`](crate::MetricsFrame).
///
/// **A reading rather than the last one taken.** With nobody watching, this daemon samples once
/// a minute, so serving the cached tick would answer a person with a number up to a minute old
/// and would not mention a service that started ten seconds ago. A reading younger than a second
/// is reused instead, which is what stops a script looping on this from driving the machine at a
/// rate it never opened `GET /metrics` to ask for.
pub const METRICS_SNAPSHOT: &str = "metrics.snapshot";

/// The 24-hour history, one row per subject per minute. Takes
/// [`MetricsHistoryQuery`](crate::MetricsHistoryQuery), answers
/// [`MetricsHistory`](crate::MetricsHistory).
///
/// **There is no `metrics.subscribe` beside these two**, and the live stream is `GET /metrics`
/// rather than a method for [ADR 0009](https://github.com/mixnz/mixlab/blob/master/docs/decisions/0009-logs-travel-on-their-own-stream.md)'s
/// reason: a JSON-RPC call cannot stream, the event bus is 1024 messages shared by every client,
/// and a subscription that had to be ended by a second call would keep sampling for a client
/// that crashed. An open connection is the subscription; closing it is the end of it.
pub const METRICS_HISTORY: &str = "metrics.history";

/// What is waiting for permission, whether this machine can raise a prompt, and what the last
/// grant did. Takes no parameters, answers [`ElevationStatus`](crate::ElevationStatus).
///
/// The screen, where [`DaemonStatus`](crate::DaemonStatus) carries the status line: this is what
/// each operation *is* and what it will change, and that does not belong in the call every
/// client makes on connect.
pub const ELEVATION_STATUS: &str = "elevation.status";

/// Spend one prompt on everything that is waiting. Takes an optional
/// [`ElevationGrant`](crate::ElevationGrant) — none is a dialog, as before — and answers a
/// [`JobSummary`](crate::JobSummary).
///
/// **A job, because what it waits on is a person reading a dialog** and there is no clock on
/// that — the exception `job.wait` is bounded by declared ready timeouts does not transfer.
/// A second grant while one is in flight is `conflict`, naming the job already running: two
/// concurrent grants are two prompts for one queue, which is the defect the batch exists to
/// prevent.
pub const ELEVATION_GRANT: &str = "elevation.grant";

/// Forget one pending operation, or all of them. Takes
/// [`ElevationDrop`](crate::ElevationDrop), answers the
/// [`ElevationStatus`](crate::ElevationStatus) the queue is left in.
///
/// The other way out of a degraded mode, and the reason a decline is not a trap: an operation
/// nobody intends to allow can be taken off the list instead of being asked about forever.
pub const ELEVATION_DROP: &str = "elevation.drop";

/// Register a directory as a project. Takes [`ProjectCreate`](crate::ProjectCreate), answers
/// the [`ProjectDetail`](crate::ProjectDetail) the new row became.
///
/// **The import too.** `name` and `pins` fall through to the `mixengine.toml` lying at the
/// root, so a create that names only a directory is how a colleague's checkout is adopted —
/// see `docs/features/runtime-versions.md` for what that file may say.
pub const PROJECT_CREATE: &str = "project.create";

/// Every registered project. Takes no parameters, answers
/// [`ProjectList`](crate::ProjectList).
pub const PROJECT_LIST: &str = "project.list";

/// One of them, with its pins in effective order and whether each resolves today. Takes
/// [`ProjectQuery`](crate::ProjectQuery), answers [`ProjectDetail`](crate::ProjectDetail).
pub const PROJECT_SHOW: &str = "project.show";

/// Change a project's name, root or pins. Takes [`ProjectUpdate`](crate::ProjectUpdate),
/// answers the [`ProjectDetail`](crate::ProjectDetail) it now is.
///
/// `pins` **replaces** rather than merges: absent means unchanged, `{}` clears them.
pub const PROJECT_UPDATE: &str = "project.update";

/// Forget a project. Takes [`ProjectQuery`](crate::ProjectQuery), answers
/// [`ProjectRemoval`](crate::ProjectRemoval).
///
/// **The directory is kept and named**, on `service.delete`'s reasoning: nothing about
/// unregistering a project says anything about wanting somebody's repository gone.
pub const PROJECT_DELETE: &str = "project.delete";

/// Write the project into `<root>/mixengine.toml`. Takes
/// [`ProjectQuery`](crate::ProjectQuery), answers [`ProjectExport`](crate::ProjectExport).
///
/// **Merges rather than rewrites**: comments, key order and a hand-written `[site]` survive,
/// because the file's whole purpose is to be read by a person.
pub const PROJECT_EXPORT: &str = "project.export";

/// Create a site under a project. Takes [`SiteCreate`](crate::SiteCreate), answers
/// [`SiteCreation`](crate::SiteCreation).
///
/// **The import too**, on [`PROJECT_CREATE`]'s reasoning: with nothing but a project named,
/// the domains, doc root, kind and services come from the `[site]` and `[[services]]` in that
/// project's manifest.
pub const SITE_CREATE: &str = "site.create";

/// Every site, or one project's. Takes [`SiteListQuery`](crate::SiteListQuery), answers
/// [`SiteList`](crate::SiteList).
pub const SITE_LIST: &str = "site.list";

/// One site, with its domains, its pool and its services. Takes
/// [`SiteQuery`](crate::SiteQuery), answers [`SiteDetail`](crate::SiteDetail).
pub const SITE_SHOW: &str = "site.show";

/// Change what a site is. Takes [`SiteUpdate`](crate::SiteUpdate), answers the
/// [`SiteDetail`](crate::SiteDetail) it now is.
///
/// `domains` and `services` **replace** rather than merge.
pub const SITE_UPDATE: &str = "site.update";

/// Serve this site. Takes [`SiteQuery`](crate::SiteQuery), answers the
/// [`SiteDetail`](crate::SiteDetail) it now is.
///
/// **A flag and a walk, never a process.** It sets the state, re-renders the front end's
/// configuration and hands it the reload; it does not start the front end, the pool, or anything
/// else — `starting`, `running` and `failed` belong to the services a site *uses*, which have
/// seven states of their own.
///
/// [`SITE_UPDATE`] can already carry a state, so this is reachable by other means. It exists
/// because "start this site" is the sentence a person says, and a client renders what the daemon
/// returns rather than composing an update to express a verb.
pub const SITE_START: &str = "site.start";

/// Stop serving this site, keeping the declaration. Takes [`SiteQuery`](crate::SiteQuery),
/// answers [`SiteDetail`](crate::SiteDetail).
///
/// The site's rendered file is removed on the same walk, which is what makes the front end stop
/// answering for the name rather than going on serving a site nothing declares.
pub const SITE_STOP: &str = "site.stop";

/// Let the local network reach this site. Takes [`SiteShare`](crate::SiteShare), answers
/// [`SiteSharing`](crate::SiteSharing).
///
/// **Per site, and never global.** What it changes is this one site's listeners, this one site's
/// certificate, and the machine's firewall rules; every other site is untouched, and so is the
/// front end's own bind address.
///
/// Raises at most one elevation prompt, which is the only one in normal day-to-day use.
pub const SITE_SHARE: &str = "site.share";

/// Take it back off the local network. Takes [`SiteQuery`](crate::SiteQuery), answers nothing.
///
/// **The reverse in every particular**: the firewall rule goes first, then the listener, then
/// the certificate name. Unsharing a site that is not shared is the state the caller asked for
/// rather than an error.
pub const SITE_UNSHARE: &str = "site.unshare";

/// Delete a site. Takes [`SiteQuery`](crate::SiteQuery), answers
/// [`SiteRemoval`](crate::SiteRemoval).
///
/// **The doc root is kept and named**, on [`PROJECT_DELETE`]'s reasoning: the files were never
/// ours.
pub const SITE_DELETE: &str = "site.delete";

/// Write down what a project is made of. Takes
/// [`BlueprintCapture`](crate::BlueprintCapture), answers
/// [`BlueprintSummary`](crate::BlueprintSummary).
pub const BLUEPRINT_CAPTURE: &str = "blueprint.capture";

/// Take in a blueprint somebody else wrote. Takes
/// [`BlueprintImport`](crate::BlueprintImport), answers the
/// [`BlueprintSummary`](crate::BlueprintSummary) of the row it wrote.
///
/// **The only method that can produce an untrusted blueprint.** Whether the file arrived with a
/// signature the gallery key vouches for is decided here, once, and becomes the summary's
/// `trusted` — which is what later decides whether its `[scaffold]` command may be offered at
/// all. Nothing raises it afterwards.
pub const BLUEPRINT_IMPORT: &str = "blueprint.import";

/// Every blueprint this home holds. Takes nothing, answers
/// [`BlueprintList`](crate::BlueprintList).
pub const BLUEPRINT_LIST: &str = "blueprint.list";

/// What applying one would do, and doing it. Takes
/// [`BlueprintApply`](crate::BlueprintApply), answers
/// [`BlueprintApplyResponse`](crate::BlueprintApplyResponse): the plan for `dry_run: true`, and
/// the job carrying it out for `dry_run: false`.
///
/// One method rather than a separate `blueprint.plan`, because the plan a person reads and the
/// plan the daemon carries out have to be the same list — and a tagged answer is what lets that
/// survive execution arriving.
///
/// **Refused before it becomes a job**: a plan holding a step that cannot be done, a version
/// question nobody answered, and an answer to a question this plan does not ask.
pub const BLUEPRINT_APPLY: &str = "blueprint.apply";

/// Read an `extension.toml` and say what installing it here would produce. Takes
/// [`ExtensionInspect`](crate::ExtensionInspect), answers
/// [`ExtensionInspection`](crate::ExtensionInspection).
///
/// **Read-only**: it installs nothing.
pub const EXTENSION_INSPECT: &str = "extension.inspect";

/// What this home has installed. Takes nothing, answers
/// [`ExtensionList`](crate::ExtensionList).
pub const EXTENSION_LIST: &str = "extension.list";

/// What the signed registry publishes. Takes
/// [`ExtensionAvailable`](crate::ExtensionAvailable), answers
/// [`ExtensionCatalogue`](crate::ExtensionCatalogue), which says how many entries this build
/// could not read rather than leaving them out in silence.
pub const EXTENSION_AVAILABLE: &str = "extension.available";

/// What installing something would do here, and what a person is agreeing to. Takes
/// [`ExtensionPlanRequest`](crate::ExtensionPlanRequest), answers
/// [`ExtensionPlan`](crate::ExtensionPlan).
///
/// **Read-only, and asked before anything is fetched** — which is what the registry carrying
/// manifests rather than pointers to them buys.
pub const EXTENSION_PLAN: &str = "extension.plan";

/// Install one. Takes [`ExtensionInstall`](crate::ExtensionInstall) — including the consent
/// naming what was shown — and answers a [`JobSummary`](crate::JobSummary).
pub const EXTENSION_INSTALL: &str = "extension.install";

/// Remove one. Takes [`ExtensionUninstall`](crate::ExtensionUninstall), answers
/// [`ExtensionRemoval`](crate::ExtensionRemoval), which says where the kept data directory is.
pub const EXTENSION_UNINSTALL: &str = "extension.uninstall";

/// Start the service an extension runs as. Takes
/// [`ExtensionTarget`](crate::ExtensionTarget), answers
/// [`ServiceWalk`](crate::ServiceWalk).
///
/// **The same row `service.start` acts on.** This exists because an extension is what somebody
/// installed and its `ServiceId` is an implementation detail of that; it adds no supervision of
/// its own.
pub const EXTENSION_START: &str = "extension.start";

/// Stop it. Takes [`ExtensionTarget`](crate::ExtensionTarget), answers
/// [`ServiceWalk`](crate::ServiceWalk).
pub const EXTENSION_STOP: &str = "extension.stop";

/// Give a site one more name. Takes [`DomainAdd`](crate::DomainAdd), answers the
/// [`SiteDetail`](crate::SiteDetail) it now is.
///
/// **Never makes the new name primary** — [`SITE_UPDATE`] reorders, and the head of that list is
/// the primary. Reachable through [`SITE_UPDATE`] as [`SITE_START`] is, and here for the same
/// reason plus one: composing the replacement means a client reading the list, appending to it
/// and sending it back, which is a read-modify-write that drops whatever another client added in
/// between.
pub const DOMAIN_ADD: &str = "domain.add";

/// Take one name away. Takes [`DomainRemove`](crate::DomainRemove), answers the
/// [`SiteDetail`](crate::SiteDetail) it now is.
///
/// Refused for a site's last domain and for its primary, each by name.
pub const DOMAIN_REMOVE: &str = "domain.remove";

/// What actually happens to a name. Takes [`DomainStatusQuery`](crate::DomainStatusQuery),
/// answers [`DomainStatusReport`](crate::DomainStatusReport).
///
/// Four facts rather than a verdict, because they fail independently — see
/// [`DomainStatus`](crate::DomainStatus).
pub const DOMAIN_DNS_STATUS: &str = "domain.dns_status";
