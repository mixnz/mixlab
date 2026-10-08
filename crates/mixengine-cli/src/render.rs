//! What `mix` puts on screen.
//!
//! The two renderings are deliberately not the same information twice at different widths. `--json`
//! is a contract: whatever the daemon answered, serialised, with the client's own identity beside it
//! so a captured file says which `mix` produced it (`docs/features/client-surface.md` calls this "copy
//! diagnostics"). The human one is a person's answer to "is it up, and which one am I talking to",
//! and leaves out anything they would not read.
//!
//! No colour, and no dependency for one. Nearly every line `mix` prints ends up pasted into a bug
//! report or an issue, and escape codes there are noise — the daemon makes the same call about its
//! own log file, which is coloured on stderr and never in `daemon.log`.

use std::time::SystemTime;
/// `*.test, *.localhost`, which is how a wildcard route reads in a sentence.
///
/// An empty list cannot reach this — the mode is `hosts_only` when nothing is routed — but it is
/// answered rather than left to render as nothing, because a status line that trails off is worse
/// than one that says the awkward thing.
fn patterns(tlds: &[String]) -> String {
    if tlds.is_empty() {
        return "no names".to_owned();
    }

    tlds.iter()
        .map(|tld| format!("*.{tld}"))
        .collect::<Vec<_>>()
        .join(", ")
}

use mixengine_proto::{
    Action, ApiAccess, ArtifactAvailability, AutostartMechanism, AutostartReport, BlueprintApplied,
    BlueprintList, BlueprintPlan, BlueprintSummary, BrowserDatabase, Browsers, BundleReport,
    CaRotateReport, CaState, CaStatus, CaUninstallReport, CertIssueReport, CertProblem, CertState,
    CertStatusReport, Cleanup, CleanupReport, CommandSource, DaemonShutdown, DaemonStatus,
    DaemonVersion, DatabaseAccount, DatabaseClientReport, DatabaseCredentials, DatabaseHandoff,
    DeclaredSiteState, DesktopClient, DiskUsage, Disposition, DnsMode, DoctorReport,
    DomainStatusReport, ElevationStatus, Enforcement, Execution, ExtensionCatalogue,
    ExtensionChange, ExtensionInspection, ExtensionKind, ExtensionList, ExtensionPlan,
    ExtensionRemoval, ExtensionSource, FilesystemReach, FrontEndOutcome, FrontEndReport,
    GrantOutcome, Handshake, IdleExemption, IdleProbe, IdleReport, IdleSource, InstalledExtensions,
    IssueOutcome, JobList, JobOutcome, JobState, JobSummary, Launch, Linkage, Made, MemoryMeasure,
    MemoryWatchdog, MetricsFrame, MetricsHistory, NetworkReach, OldVersion, Outcome,
    PROTOCOL_VERSION, PackageCatalogue, PackageList, PackageRelease, PackageRemoval,
    PackageVersion, PathReport, PinSource, PlanAction, PlanStep, PoolOutcome, Priority,
    ProjectDetail, ProjectExport, ProjectList, ProjectRemoval, RecipeAddition, Reclaim, Removal,
    RepairReport, Requirement, ResolvedRuntime, RotateOutcome, RuntimeCatalogue, RuntimeList,
    RuntimeRelease, RuntimeRemoval, RuntimeSource, RuntimeSummary, ServiceCreation, ServiceId,
    ServiceLimitsReport, ServiceList, ServiceRemoval, ServiceState, ServiceSummary, ServiceWalk,
    SignatureCheck, SiteDetail, SiteKind, SiteList, SiteOwner, SiteRemoval, SiteSharing,
    StateReason, StepResult, StorageChoice, StorageReport, Timestamp, Trust, UninstallOutcome,
    UninstallReport, Unusable, UpdateApplied, UpdateHandedOver, UpdatePlacement, UpdateStatus,
    UpgradeItem, UpgradeOutcome, UpgradePlan, Uptime, Verdict, WhenExceeded,
    privileged::ElevationOutcome,
};

/// `mix cert ca-status`, for a person.
///
/// **The trust line prints what the daemon said and never a word more.** T48 left it off entirely,
/// because there was no such fact in the answer and a line implying one would be the client
/// inventing something — `CLAUDE.md`'s "a client only renders what the daemon returns". T49a put the
/// fact in the answer, including the case where the daemon could not find out, which prints as
/// exactly that rather than as "no".
///
/// **And it says which store**, because "trusted" and "in this machine's own store, for every
/// account on it" are different claims and only the second is what happened.
/// `mix cert issue`, for a person — roadmap task **T50**.
///
/// **A line per site and never a count.** "3 certificates issued" is a number nobody can act on;
/// the domain is what somebody opens in a browser, and the reason a refusal gives is the only part
/// of this output that ever needs doing something about.
pub(crate) fn cert_issue(report: &CertIssueReport) -> String {
    if report.sites.is_empty() {
        return "  no site in this home declares HTTPS\n".to_owned();
    }

    report
        .sites
        .iter()
        .map(|site| match (&site.outcome, &site.state) {
            (IssueOutcome::Issued {}, CertState::Present { cert }) => format!(
                "  {}  issued: {} days, {} name(s)\n",
                site.domain,
                cert.days_left,
                cert.sans.len()
            ),
            (IssueOutcome::Reused {}, CertState::Present { cert }) => format!(
                "  {}  unchanged: {} days left\n",
                site.domain, cert.days_left
            ),
            // Roadmap task **T52**: a site that declares no HTTPS asked for nothing, and printing
            // it as "not issued" reads as a fault where there is none.
            (IssueOutcome::NotWanted { because }, _) => {
                format!("  {}  nothing to do: {because}\n", site.domain)
            }
            (IssueOutcome::Refused { because }, _) => {
                format!("  {}  not issued: {because}\n", site.domain)
            }
            // A written or reused certificate that does not read back is a state nothing should
            // produce, and printing it as success would hide exactly the case worth seeing.
            (_, state) => format!("  {}  unclear: {state:?}\n", site.domain),
        })
        .collect()
}

/// `mix cert status`, for a person — roadmap task **T53**.
///
/// **The command to run is written here and not by the daemon.** The answer carries a
/// [`CertProblem`], which is a name for a condition; turning that into `mix cert issue --site …` is
/// this client's job, because a graphical client renders a button for the same condition and would
/// have no use for a sentence telling its user to open a terminal.
///
/// **Two lines per site and not one**, unlike [`cert_issue`]: what is on the wire and what is on
/// the disk are two facts, and the whole point of this command is the case where they disagree.
pub(crate) fn cert_status(report: &CertStatusReport) -> String {
    if report.sites.is_empty() {
        return "  no site in this home
"
        .to_owned();
    }

    report
        .sites
        .iter()
        .map(|site| {
            let mut lines = format!(
                "  {}
",
                site.domain
            );

            lines.push_str(&match &site.handshake {
                Handshake::NotAsked {} => "    served over HTTP only
"
                .to_owned(),
                Handshake::NotServed { because } => {
                    format!(
                        "    not served over TLS: {because}
"
                    )
                }
                Handshake::Failed { because } => format!(
                    "    the handshake failed: {because}
"
                ),
                Handshake::Presented { cert, trust } => format!(
                    "    presented {}: {} days, {} name(s), {}
",
                    short(&cert.fingerprint),
                    cert.days_left,
                    cert.sans.len(),
                    match trust {
                        Verdict::Trusted {} => "trusted by this home's authority".to_owned(),
                        Verdict::Rejected { because } => format!("not trusted: {because}"),
                    }
                ),
            });

            if let Some(problem) = site.problem {
                lines.push_str(&format!(
                    "    {}
",
                    advice(&site.domain, problem)
                ));
            }

            lines
        })
        .collect()
}

/// The first sixteen characters of a fingerprint, which is what a person compares by eye.
///
/// The whole hash is in `--json` for anything that compares by machine.
fn short(fingerprint: &str) -> &str {
    &fingerprint[..fingerprint.len().min(16)]
}

/// What to do about a condition, in this client's own words.
fn advice(domain: &str, problem: CertProblem) -> String {
    match problem {
        CertProblem::NoCertificate | CertProblem::NamesDiffer | CertProblem::Expiring => {
            format!("run `mix cert issue --site {domain}`")
        }
        CertProblem::NotServed => {
            "start this home's front end; `mix service list` says which it is".to_owned()
        }
        CertProblem::ServedCertificateDiffers => {
            "the running server is holding an older certificate; restart this home's front end"
                .to_owned()
        }
        CertProblem::NotTrusted => {
            "this was not signed by this home's authority; `mix cert ca-status` says which \
             authority this home has"
                .to_owned()
        }
        // `CertProblem` is `#[non_exhaustive]`: a variant added by a newer daemon reaches an older
        // `mix`, and printing nothing at all would be worse than saying there is something to look
        // at.
        _ => "run `mix doctor`".to_owned(),
    }
}

pub(crate) fn ca_status(status: &CaStatus) -> String {
    let mut rendered = certificate(&status.state);

    rendered.push_str(&match &status.trust {
        Trust::Installed { store } => format!(
            "  trusted    yes, in {store}
"
        ),
        Trust::NotInstalled { because } => format!(
            "  trusted    no: {because}
"
        ),
        Trust::NoStore { because } => format!(
            "  trusted    n/a: {because}
"
        ),
        Trust::Unknown { because } => format!(
            "  trusted    unknown: {because}
"
        ),
    });

    // **A line per database, and never a summary count.** "2 of 3" is a number nobody can act on;
    // the path is what a person opens and the owner is what tells them which browser to restart.
    rendered.push_str(&match &status.browsers {
        Browsers::Reached { databases } if databases.is_empty() => {
            "  browsers   none found; Firefox and Chrome keep certificate databases of their own, \
             and this machine has none
"
            .to_owned()
        }
        Browsers::Reached { databases } => databases.iter().map(browser).collect::<String>(),
        Browsers::NoTool { because } => format!(
            "  browsers   not asked: {because}
"
        ),
        Browsers::NotSearched { because } => format!(
            "  browsers   n/a: {because}
"
        ),
        Browsers::Unknown { because } => format!(
            "  browsers   unknown: {because}
"
        ),
    });

    rendered
}

/// `mix cert ca-rotate` — roadmap task **T54**.
///
/// **What is left comes from the status, because that is the measurement**; the outcome supplies the
/// reason a measurement cannot give.
pub(crate) fn ca_rotate(report: &CaRotateReport) -> String {
    let mut rendered = match &report.outcome {
        RotateOutcome::Rotated {} => {
            let mut said = format!(
                "this home has a new certificate authority\n{} site certificate(s) were reissued under it\n",
                report.sites.len()
            );

            // **The one thing a rotation can leave behind and not otherwise mention.** A previous
            // authority that could not be read cannot be named for removal — T49a's D5 forbids
            // guessing — so the old certificate is still in the store, and saying nothing here
            // would read as a clean rotation.
            if report.previous.is_none() {
                said.push_str(
                    "the previous certificate was left in this machine's trust store: it could not\nbe read, and nothing is removed that cannot be named\n",
                );
            }

            said
        }

        RotateOutcome::NotCommitted { because } => format!(
            "nothing was changed: {because}\n\nrun `mix cert ca-status` to see what this machine holds now\n"
        ),

        RotateOutcome::NothingToRotate { because } => {
            format!("{because}\n\nrun `mix doctor --repair` to make one\n")
        }

        // `RotateOutcome` is `#[non_exhaustive]`: a variant a newer daemon knows reaches an older
        // `mix`, and printing nothing would be worse than saying the question was asked.
        _ => "this home's certificate authority was asked about\n".to_owned(),
    };

    rendered.push('\n');
    rendered.push_str(&ca_status(&report.status));
    rendered
}

/// `mix cert ca-uninstall` — roadmap task **T54**.
///
/// **What is left comes from the status, because that is the measurement**; the outcome supplies the
/// reason a measurement cannot give. So the two halves cannot disagree: there is only one reading.
pub(crate) fn ca_uninstall(report: &CaUninstallReport) -> String {
    let mut rendered = match &report.outcome {
        UninstallOutcome::Removed {} => {
            "this home's certificate authority was taken out of every store that held it
the certificate and its key are still on disk; `mix doctor --repair` puts the trust back
"
            .to_owned()
        }
        UninstallOutcome::PartlyRemoved { because } => format!(
            "some of it is still there: {because}

run `mix elevation grant` if a prompt was refused, or `mix cert ca-status` to look again
"
        ),
        UninstallOutcome::NothingToRemove { because } => format!("{because}\n"),
        // `UninstallOutcome` is `#[non_exhaustive]`: a variant a newer daemon knows reaches an older
        // `mix`, and printing nothing would be worse than saying the question was asked.
        _ => "this home's certificate authority was asked about\n".to_owned(),
    };

    rendered.push('\n');
    rendered.push_str(&ca_status(&report.status));
    rendered
}

/// One database's line.
fn browser(database: &BrowserDatabase) -> String {
    let verdict = if database.installed {
        "yes".to_owned()
    } else {
        match &database.because {
            Some(because) => format!("no: {because}"),
            None => "no".to_owned(),
        }
    };

    format!(
        "  browsers   {verdict}: {} ({})
",
        database.path, database.owner
    )
}

/// The authority itself, which is the half T48 built.
fn certificate(state: &CaState) -> String {
    match state {
        // Reachable, and worth a sentence rather than an empty screen: a start whose generation
        // failed warns into the daemon's log and carries on, so this is what the next question gets.
        CaState::Absent {} => "  authority  none; one is made when the daemon starts
"
        .to_owned(),

        CaState::Unusable { because } => {
            format!(
                "  authority  unusable: {}
",
                unusable(*because)
            )
        }

        CaState::Present { ca } => {
            let mut rendered = format!(
                "  authority  {}
",
                ca.subject
            );
            rendered.push_str(&format!(
                "  sha256     {}
",
                ca.fingerprint
            ));

            // Negative rather than clamped: an expired authority is a true state, and a screen that
            // said "in -3 days" — or silently "in 0 days" — would be hiding the one thing worth
            // acting on.
            rendered.push_str(&if ca.days_left < 0 {
                format!(
                    "  expired    {} days ago
",
                    ca.days_left.abs()
                )
            } else {
                format!(
                    "  expires    in {} days
",
                    ca.days_left
                )
            });

            rendered
        }
    }
}

/// Each way of being unusable, in a sentence. **Not what to do about it**: the reason is a fact
/// about this home, and the remedy is `mix cert ca-rotate`, which T54 builds.
fn unusable(because: Unusable) -> &'static str {
    match because {
        Unusable::KeyMissing => "the certificate is here and its private key is not",
        Unusable::CertificateMissing => "the private key is here and the certificate is not",
        Unusable::KeyUnreadable => "the private key is not one this build can read",
        Unusable::CertificateUnreadable => "the certificate is not a certificate",
        Unusable::KeyAndCertificateDisagree => {
            "the certificate and the private key are not each other's"
        }
    }
}

/// `mix status`, for a person.
pub(crate) fn status(status: &DaemonStatus) -> String {
    let mut rendered = format!(
        "mixengined {}: running (pid {}, up {})\n",
        status.version,
        status.pid,
        uptime(status.uptime)
    );

    // The home first, because it is the single most useful line when somebody is talking to a daemon
    // they did not expect to be talking to — which is the whole reason the field exists.
    for (label, value) in [
        ("home", status.home.as_str()),
        ("endpoint", status.endpoint.as_str()),
        ("database", status.database.as_str()),
        ("protocol", &status.protocol.0.to_string()),
    ] {
        rendered.push_str(&format!("  {label:9} {value}\n"));
    }

    // **The names line, and it is one line whichever mechanism is running** — roadmap task T44.
    // The mode alone is not the sentence somebody needs: what a hosts-only home loses is wildcards,
    // and what it wants to know is why, so both travel with it.
    if let Some(dns) = &status.dns {
        rendered.push_str(&match dns.mode {
            // The TLDs are named rather than counted, because from T45 on a home can have wildcards
            // for some of its names and not others — `.local` is never routed — and "wildcards work"
            // would be true and useless to somebody whose `.local` site had just stopped resolving.
            DnsMode::Dns => format!(
                "  names     DNS on {}, wildcards for {}\n",
                dns.listening.as_deref().unwrap_or("loopback"),
                patterns(&dns.wildcards)
            ),
            DnsMode::HostsOnly => format!(
                "  names     hosts file, no wildcards{}\n",
                dns.because
                    .as_deref()
                    .map(|because| format!(" ({because})"))
                    .unwrap_or_default()
            ),
        });
    }

    // Which store the passwords are in — roadmap task T194. Absent from a daemon that predates it,
    // and then no line is invented.
    if let Some(credentials) = &status.credentials {
        rendered.push_str(match credentials.store {
            mixengine_proto::CredentialStore::Os => "  passwords the system's credential store\n",
            mixengine_proto::CredentialStore::Home => {
                "  passwords a file in this home, readable by your account only\n"
            }
        });
    }

    if let Some(elevation) = &status.elevation {
        if elevation.elevated {
            rendered.push_str(
                "  note      this daemon holds an administrative token; every service it \
                 supervises inherits it\n",
            );
        }

        // Degraded is this number and nothing else — there is no flag on the wire and none here.
        if elevation.pending > 0 {
            rendered.push_str(&format!(
                "  waiting   {} for permission; `mix elevation status` says what they are\n",
                operations(elevation.pending)
            ));
        }
    }

    // **One line when there is an update, and nothing at all when there is not** — roadmap task
    // **T88**. Everything else about it is `update.status`, which is a screen; this is a status line,
    // and what makes it worth one is that nothing else in the product would ever mention it.
    if let Some(update) = &status.update {
        rendered.push_str(&format!(
            "  update    MixEngine {} is available; `mix self-update` shows what changed\n",
            update.version
        ));
    }

    // Same protocol, different builds: not an error — the handshake would have refused it if it
    // were — but the explanation for a `mix` that has a command the daemon answers `not_found` to,
    // and for whichever lines above that daemon was too old to fill in.
    //
    // **Reachable, from T88c on.** It was written for this skew and tested for it, and until
    // `elevation` and `dns` became optional the answer did not deserialise — so the one thing that
    // explained the situation was the one thing that could not be printed. See ADR 0019,
    // `docs/decisions/0019-an-added-response-member-is-optional.md`.
    //
    // One note and not two: a status somebody reads daily earns at most one, and in the only case
    // where both halves apply the second is the explanation of the first.
    let mut skew: Vec<String> = Vec::new();

    if status.version != env!("CARGO_PKG_VERSION") {
        skew.push(format!(
            "mix is {} and this daemon is {}; they speak the same protocol, so this is a daemon \
             that has not been restarted since the upgrade",
            env!("CARGO_PKG_VERSION"),
            status.version
        ));
    }

    // Named in the order the missing lines would have appeared, so the note reads as a gap in what
    // is above it rather than as a list of field names.
    let unreported: Vec<&str> = [
        (status.dns.is_none(), "how names resolve"),
        (status.elevation.is_none(), "what is waiting for permission"),
    ]
    .into_iter()
    .filter_map(|(missing, what)| missing.then_some(what))
    .collect();

    if !unreported.is_empty() {
        skew.push(format!("it did not report {}", unreported.join(", or ")));
    }

    if !skew.is_empty() {
        rendered.push_str(&format!("  note      {}\n", skew.join("; ")));
    }

    rendered
}

/// `mix self-update` and `mix self-update --check`, for a person — roadmap task **T88**.
///
/// **The consent prompt is this text plus one question.** `docs/features/updates.md` requires
/// that somebody sees the version, the size and the notes before they answer, and that they are told
/// what is about to be stopped — so all four are here, and the question that follows is one line.
///
/// **Every reason a release is not offered is the daemon's sentence, printed unchanged.** A client
/// that re-derived *"you skipped this one"* from a version string and a settings row would be
/// deciding something the daemon has already decided, which is the business-logic-in-a-client bug
/// `CLAUDE.md` forbids.
pub(crate) fn update_status(status: &UpdateStatus) -> String {
    let mut rendered = format!("MixEngine {}\n", status.current);

    // Installer.app has put the new binaries on disk and this daemon is still the old one — T88f.
    if let Some(installed) = &status.installed {
        rendered.push_str(&format!(
            "  update    {installed} is installed. finish it: mix self-update --finish\n"
        ));

        return rendered;
    }

    // A copy the `.pkg` installed reads `managed` on the wire, and is still offered its update: the
    // next `.pkg`, through Installer.app (T88f, D3). Only a copy with no installer is refused here.
    if let (UpdatePlacement::Managed { directory, because }, None) =
        (&status.placement, &status.installer)
    {
        rendered.push_str(&format!("  installed {directory}\n"));
        rendered.push_str(&format!("  update    not by MixEngine: {because}\n"));

        return rendered;
    }

    let Some(release) = &status.available else {
        rendered.push_str(match status.checked_at {
            Some(_) => "  update    nothing newer has been published\n",
            None => "  update    this daemon has not managed to read the update feed\n",
        });

        return rendered;
    };

    rendered.push_str(&format!(
        "  available {} ({}, {})\n",
        release.version,
        release.published_at,
        size(release.size)
    ));

    if !status.offered {
        // The whole of what a client does with a release it is not showing: say it exists, and say
        // the daemon's reason for not putting it in front of anybody.
        if let Some(because) = &status.because {
            rendered.push_str(&format!("  not now   {because}\n"));
        }

        return rendered;
    }

    if status.stale {
        // An offer from a cached document is a genuine offer — the signature was checked exactly as
        // it would have been on a fresh copy — and it is still worth saying which it was.
        rendered.push_str(
            "  note      read from the last feed this daemon verified, not a fresh one\n",
        );
    }

    if !status.will_restart.is_empty() {
        rendered.push_str(&format!(
            "  restarts  {} will be stopped and started again: {}\n",
            services(status.will_restart.len()),
            names(&status.will_restart)
        ));
    }

    if status.installer.is_some() {
        rendered.push_str("  installs  through Installer.app, which asks for your password\n");
    }

    if !release.notes.trim().is_empty() {
        rendered.push_str("\nwhat changed:\n");

        for line in release.notes.lines() {
            rendered.push_str(&format!("  {line}\n"));
        }
    }

    if let Some(url) = &release.notes_url {
        rendered.push_str(&format!("\n{url}\n"));
    }

    rendered
}

/// What `mix self-update` prints once the package is handed over — roadmap tasks **T88f** and
/// **T182b** (D5).
///
/// **The path and the command every time**, not only when opening failed. Over SSH, `open`
/// succeeds and Installer.app comes up on the Mac's own screen, which the person at this prompt may
/// not be looking at (the T88f readings, M4). Both are the daemon's, printed unchanged. A Linux
/// machine with no desktop session opens nothing, and the command is then the only way.
pub(crate) fn update_handed_over(handed: &UpdateHandedOver) -> String {
    if handed.opened {
        format!(
            "the installer is open. when it is done: mix self-update --finish\n  \
             package   {}\n  \
             or run    {}\n",
            handed.package, handed.command
        )
    } else {
        format!(
            "the update is downloaded. install it, then run: mix self-update --finish\n  \
             package   {}\n  \
             run       {}\n",
            handed.package, handed.command
        )
    }
}

/// What an update did, printed while the daemon that did it is exiting — roadmap task **T88**.
///
/// **`kept` is named rather than left out.** `mixengine-elevate` is deliberately not replaced, and
/// somebody comparing version numbers afterwards should find that stated rather than discover it.
pub(crate) fn update_applied(applied: &UpdateApplied) -> String {
    let mut rendered = format!("MixEngine {} → {}\n", applied.from, applied.to);

    rendered.push_str(&format!("  in        {}\n", applied.directory));
    rendered.push_str(&format!("  replaced  {}\n", list(&applied.replaced)));

    if !applied.kept.is_empty() {
        rendered.push_str(&format!(
            "  kept      {}; updating the privileged helper needs its own prompt\n",
            list(&applied.kept)
        ));
    }

    if !applied.restarting.is_empty() {
        rendered.push_str(&format!("  restarts  {}\n", names(&applied.restarting)));
    }

    rendered
}

/// A list of plain names, in the order the daemon gave them.
fn list(names: &[String]) -> String {
    match names.is_empty() {
        true => MISSING.to_owned(),
        false => names.join(", "),
    }
}

/// `mix status --json`.
///
/// An envelope rather than the daemon's answer alone. The daemon half is `DaemonStatus` verbatim, so
/// `mix status --json | jq .daemon.pid` reads the field by the name the API gives it; the client
/// half is the part no daemon can report, and version skew is the first thing anybody looks for in a
/// captured diagnostic.
///
/// **Verbatim includes what is not there.** A member an older daemon predates is absent from this
/// object rather than `null` or defaulted — `.daemon.dns` and `.daemon.elevation` from **T88c**,
/// `.daemon.update` from T88 — which is the honest encoding of a fact nobody reported, and what
/// `jq` should be asked about with `//` rather than indexed into.
pub(crate) fn status_json(status: &DaemonStatus) -> serde_json::Value {
    serde_json::json!({
        "client": client(),
        "daemon": status,
    })
}

/// `mix daemon stop`, for a person.
///
/// **The headline is the daemon and the detail is the services**, indented under it, because that is
/// what was asked for: `mix service stop` reports on services and this reports on a daemon that
/// happens to have stopped some. The walk itself is rendered by [`service_walk`] rather than a
/// second time here — a service that would not stop reads the same in both places, and two renderings
/// of one failure eventually disagree about it.
///
/// A daemon with nothing to stop says only the first line. `service_walk`'s "this home declares no
/// services" is the right sentence for a command that was *about* services and the wrong one here,
/// where nothing was asked about them.
///
/// **A shutdown that could not be ordered says so before anything else**, because the two answers
/// are otherwise the same one: an empty walk from a home with nothing to stop, and an empty walk
/// from a daemon that could not work out how to stop what it had. The second is the one a user has
/// to know about — everything went down at the same moment instead of dependents first — and the
/// only reason this can say it is that [`DaemonShutdown::unordered`] carries the reason. Rendered as
/// the daemon's own sentence, hint and all, rather than reworded here: the file to fix is named in
/// it, and `mix service list` will complain about that same file in those same words.
pub(crate) fn daemon_shutdown(shutdown: &DaemonShutdown) -> String {
    let mut rendered = String::from("mixengined is stopping\n");

    if let Some(why) = &shutdown.unordered {
        rendered.push_str(
            "  the services were not stopped in dependency order; mixengined could not work one \
             out, so all of them stopped at the same time\n",
        );

        // The wire error's own `Display`, which is the message and then the hint on a line of its
        // own; each line is indented under the headline the way the walk below is.
        for line in why.to_string().lines() {
            rendered.push_str(&format!("  {line}\n"));
        }
    }

    if shutdown.services.planned.is_empty() {
        return rendered;
    }

    for line in service_walk(Walked::Stop, &shutdown.services).lines() {
        rendered.push_str(&format!("  {line}\n"));
    }

    rendered
}

/// What a walk was aiming for, in the one place the three commands differ.
///
/// `service.start`, `service.stop` and `service.restart` answer the same [`ServiceWalk`], so the
/// only thing a rendering needs from the command is the verb — and having it as a type rather than a
/// string is what stops "stopped" being printed by the one that starts things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Walked {
    Start,
    Stop,
    Restart,

    /// A credential reset — roadmap task **T127**. The walk it reports is the *start* that puts
    /// back what the repair took down.
    ResetCredential,
}

impl Walked {
    /// What a service that got there did.
    const fn reached(self) -> &'static str {
        match self {
            Self::Start => "started",
            Self::Stop => "stopped",
            Self::Restart => "restarted",
            Self::ResetCredential => "started again",
        }
    }

    /// What the one that did not get there failed to do.
    const fn failed(self) -> &'static str {
        match self {
            Self::Start => "failed to start",
            Self::Stop => "failed to stop",
            Self::Restart => "failed to restart",
            Self::ResetCredential => "did not come back",
        }
    }

    /// The verb in the present, for a walk nobody is waiting for.
    const fn ongoing(self) -> &'static str {
        match self {
            Self::Start => "starting",
            Self::Stop => "stopping",
            Self::Restart => "restarting",
            Self::ResetCredential => "re-setting a credential",
        }
    }
}

/// `mix service list`, for a person.
///
/// A table because the question it answers is a comparison — which of these is up — and one block
/// per service would put the states four lines apart. `supervised` gets a column of its own rather
/// than being folded into the state: a row that says `running` with nothing supervising it is a
/// daemon that was killed, and merging the two would hide exactly the case worth seeing.
pub(crate) fn service_list(list: &ServiceList) -> String {
    if list.services.is_empty() {
        return "no services are declared in this home\n".to_owned();
    }

    let rows: Vec<[String; 7]> = list
        .services
        .iter()
        .map(|service| {
            [
                service.id.to_string(),
                service
                    .version
                    .as_ref()
                    .map_or_else(|| MISSING.to_owned(), |version| version.as_str().to_owned()),
                state(service),
                yes_no(service.autostart),
                yes_no(service.supervised),
                service
                    .pid
                    .map_or_else(|| MISSING.to_owned(), |pid| pid.to_string()),
                names(&service.depends_on),
            ]
        })
        .collect();

    // `AUTOSTART` beside `STATE` rather than out at the end — roadmap task T112. The two are the
    // question somebody scanning this table is actually asking: what is running, and what will be
    // running after the next login. `VERSION` beside the id (T183), because the id is a name
    // somebody chose and the version is what it actually is.
    let mut out = table(
        [
            "SERVICE",
            "VERSION",
            "STATE",
            "AUTOSTART",
            "SUPERVISED",
            "PID",
            "DEPENDS ON",
        ],
        &rows,
    );

    // **Why each failed service failed** — roadmap task **T200b**, D6. A line each, after the
    // table, because the sentence is too long for a column and is the reason anybody reads this.
    for service in &list.services {
        if let Some(note) = &service.last_failure {
            out.push_str(&format!(
                "{} failed: {}
",
                service.id, note.detail
            ));
        }
    }

    out
}

/// The services `mix site show` cannot start for this site, and why — roadmap task **T200b**, D6.
///
/// The site's pool and every service it is linked to, read against the listing. An empty string
/// when nothing it needs has failed, so the caller can append it whatever it holds.
pub(crate) fn site_failures(detail: &SiteDetail, services: &ServiceList) -> String {
    let pool = detail
        .pool
        .as_ref()
        .and_then(|pool| pool.resolved.as_ref().or(pool.declared.as_ref()));
    let needed = pool
        .into_iter()
        .chain(detail.services.iter().map(|link| &link.service));

    let mut out = String::new();

    for id in needed {
        let note = services
            .services
            .iter()
            .find(|service| &service.id == id)
            .and_then(|service| service.last_failure.as_ref());

        if let Some(note) = note {
            out.push_str(&format!(
                "{id} could not start: {}
",
                note.detail
            ));
        }
    }

    out
}

/// A boolean as a table cell.
fn yes_no(value: bool) -> String {
    match value {
        true => "yes".to_owned(),
        false => "no".to_owned(),
    }
}

/// `mix service autostart <service>`, for a person — roadmap task **T112**.
///
/// **Two sentences and not one word.** The setting alone would leave somebody reading `no` beside a
/// service that does start at every login, because a start plan pulls in what the flagged services
/// depend on — so the second line says that out loud rather than leaving it to the help text of a
/// command they have already run.
pub(crate) fn service_autostart(service: &ServiceSummary) -> String {
    let answer = match service.autostart {
        true => "starts with MixEngine",
        false => "does not start with MixEngine",
    };

    format!(
        // aligned on purpose: `note` sits in the label column the lines above it use.
        "{}: {answer}\n  note        anything a service that does start depends on is started too, \
         whether or not it is set here\n",
        service.id
    )
}

/// `mix service save-resources`, for a person.
pub(crate) fn save_resources(answer: mixengine_proto::SaveResources) -> String {
    match answer.on {
        true => "on: a service nobody is using is stopped, and started again when it is needed\n"
            .to_owned(),
        false => "off: nothing is stopped for being idle unless you gave it a time with \
                  `mix service idle`\n"
            .to_owned(),
    }
}

/// `mix service status <service>`, for a person.
pub(crate) fn service_status(service: &ServiceSummary) -> String {
    let mut rendered = format!("{}: {}\n", service.id, state(service));

    let mut field = |label: &str, value: &str| {
        rendered.push_str(&format!("  {label:11} {value}\n"));
    };

    if let Some(version) = &service.version {
        field("version", version.as_str());
    }
    field("supervised", if service.supervised { "yes" } else { "no" });
    field("autostart", if service.autostart { "yes" } else { "no" });

    if let Some(pid) = service.pid {
        field("pid", &pid.to_string());
    }
    if let Some(port) = service.port {
        field("port", &port.to_string());
    }
    if let Some(started) = service.last_started_at {
        // The label, not the value: `last_started_at` outlives the run it names, so a service that
        // has been stopped still has one — and `stopped` with `started 4m ago` under it reads as a
        // contradiction rather than as the history it is.
        let label = match in_the_run_it_names(service.state) {
            true => "started",
            false => "last start",
        };
        field(label, &ago(started, SystemTime::now()));
    }
    if let Some(code) = service.last_exit_code {
        field("last exit", &code.to_string());
    }
    if !service.depends_on.is_empty() {
        field("depends on", &names(&service.depends_on));
    }

    // The two states that need a sentence rather than a word, for the same reason `mix status`
    // explains a daemon from another build: neither is wrong, and neither is what a user assumes.
    if service.state.is_none() {
        field(
            "note",
            "this service is declared and has never been created, so there is nothing to start yet",
        );
    } else if !service.supervised && service.pid.is_some() {
        field(
            "note",
            "the row names a process and nothing in this daemon is supervising it; that is what a \
             daemon which was killed leaves behind",
        );
    }

    rendered
}

/// `mix service front-end`, for a person — roadmap task **T97**.
///
/// **What the daemon said, not what this client worked out.** The row is picked by
/// [`ServiceSummary::role`], which is the whole point of that member: no client anywhere maps a
/// package name to a meaning.
pub(crate) fn front_end(found: Option<&ServiceSummary>) -> String {
    let Some(service) = found else {
        return "this home has no front end, so nothing is serving its sites\n  \
                `mix service set-front-end caddy` makes one out of an installed package\n"
            .to_owned();
    };

    format!(
        "every site in this home is reached through {}\n{}",
        service.id,
        service_status(service)
    )
}

/// `mix service set-front-end`, for a person — roadmap task **T97**.
///
/// **The outcome leads**, on [`service_walk`]'s rule: three of the five endings leave the home where
/// it started, and somebody reading this needs to know which one happened before anything else.
pub(crate) fn front_end_report(report: &FrontEndReport) -> String {
    let named =
        |id: Option<&ServiceId>| id.map_or_else(|| "nothing".to_owned(), ToString::to_string);

    let mut rendered = match &report.outcome {
        FrontEndOutcome::Unchanged {} => format!(
            "{} is already this home's front end; nothing was stopped, deleted or created\n",
            named(report.was.as_ref())
        ),

        FrontEndOutcome::Switched { started } => {
            let mut said = format!(
                "every site in this home is now reached through {}, and was reached through {}\n",
                named(report.now.as_ref()),
                named(report.was.as_ref())
            );

            match started {
                Some(walk) if walk.failed.is_some() => said.push_str(
                    "  it was started and did not come up; `mix service logs` has what it \
                     printed\n",
                ),
                Some(_) => said.push_str("  started, because the one it replaced was running\n"),
                None => said.push_str(
                    "  left stopped, because the one it replaced was; `mix service start` brings \
                     it up\n",
                ),
            }

            said
        }

        FrontEndOutcome::NotGranted { because } => format!(
            "this home is still reached through {}, and nothing was changed\n  {because}\n  \
             `mix elevation grant` allows it, and then this command works\n",
            named(report.now.as_ref())
        ),

        FrontEndOutcome::RolledBack { because } => format!(
            "the switch did not happen and {} was put back\n  {because}\n",
            named(report.now.as_ref())
        ),

        FrontEndOutcome::Failed { because } => format!(
            "the switch failed and this home is now reached through {}\n  {because}\n",
            named(report.now.as_ref())
        ),
    };

    if !report.answering {
        rendered.push_str(
            "  it has not been allowed to answer on 80 and 443 on this machine, so it will not \
             start; `mix doctor` says what to do\n",
        );
    }

    if let Some(data) = &report.kept_data {
        rendered.push_str(&format!("  the data left where it was: {data}\n"));
    }

    if !report.not_carried.is_empty() {
        rendered.push_str("  what did not travel with the switch:\n");

        for left in &report.not_carried {
            rendered.push_str(&format!("    - {left}\n"));
        }
    }

    rendered
}

/// `mix service start|stop|restart`, for a person.
///
/// **The failure leads**, where everything that went right is a list underneath it. A walk of six
/// services that stopped at the fourth is read by somebody who wants the name of the one to fix,
/// and putting five lines of `started` above it is five lines between them and the answer.
pub(crate) fn service_walk(walked: Walked, walk: &ServiceWalk) -> String {
    if walk.planned.is_empty() {
        return format!(
            "nothing to {}: this home declares no services\n",
            match walked {
                Walked::Start => "start",
                Walked::Stop => "stop",
                Walked::Restart => "restart",
                Walked::ResetCredential => "repair",
            }
        );
    }

    if !walk.complete {
        return format!(
            "accepted; mixengined is {} {} in the background\n",
            walked.ongoing(),
            names(&walk.planned)
        );
    }

    let Some(failure) = &walk.failed else {
        return format!("{} {}\n", walked.reached(), names(&walk.reached));
    };

    // The runner's sentence first (T202a, D3), the state machine's reason when there is none, and
    // the admission when there is neither — a reason is `None` only when the failure was the
    // daemon's own, and inventing one would be worse than saying so.
    let mut rendered = match (&failure.detail, &failure.reason) {
        (Some(detail), _) => format!("{} {}: {detail}\n", failure.service, walked.failed()),
        (None, Some(reason)) => format!("{} {}: {reason}\n", failure.service, walked.failed()),
        (None, None) => format!(
            "{} {}: mixengined did not say why; logs/daemon.log has it\n",
            failure.service,
            walked.failed()
        ),
    };

    // The evidence, and the only part of a reason a client lays out itself: `StateReason`'s own
    // sentence says what happened, and these are the lines that show it. Two reasons carry any —
    // `CrashLoop`'s `tail`, and `SuperuserRefused`'s one line.
    match &failure.reason {
        Some(StateReason::CrashLoop { tail, .. }) => {
            for line in tail {
                rendered.push_str(&format!("    {line}\n"));
            }
        }

        // **And then the command that undoes it** — roadmap task **T127a**. A walk carries no hint
        // field and is not being given one for this: the daemon said what happened, and which of
        // `mix`'s own verbs repairs it is what this binary knows and the daemon does not.
        Some(StateReason::SuperuserRefused { said }) => {
            rendered.push_str(&format!("    {said}\n"));
            rendered.push_str(&format!(
                "    `mix service reset-credential {}` writes this home's password into the data \
                 directory and keeps every database in it\n",
                failure.service
            ));
        }

        // `StateReason` is `#[non_exhaustive]`, and every other reason is its own sentence and
        // nothing more.
        _ => {}
    }

    if !walk.reached.is_empty() {
        rendered.push_str(&format!(
            "  {:9} {}\n",
            walked.reached(),
            names(&walk.reached)
        ));
    }

    if !walk.blocked.is_empty() {
        rendered.push_str(&format!("  {:9} {}\n", "blocked", names(&walk.blocked)));
    }

    rendered
}

/// What is printed where a service has no value for something.
const MISSING: &str = "—";

/// What a summary says a service is doing, in one word.
fn state(service: &ServiceSummary) -> String {
    service
        .state
        .map_or_else(|| "not created".to_owned(), |state| state.to_string())
}

/// Whether the run `last_started_at` names is the one the service is still in.
///
/// Matched exhaustively on purpose, which is what [`ServiceState`] being a closed enum is for: a
/// state added later has to face this question rather than fall into a default. `Restarting` is on
/// the false side — a service waiting out a backoff has no process at all, so its last start is as
/// much history as a stopped one's.
const fn in_the_run_it_names(state: Option<ServiceState>) -> bool {
    match state {
        Some(
            ServiceState::Starting
            | ServiceState::Running
            | ServiceState::Degraded
            | ServiceState::Stopping,
        ) => true,
        Some(ServiceState::Stopped | ServiceState::Restarting | ServiceState::Failed) | None => {
            false
        }
    }
}

/// `mix runtime list`, for a person.
///
/// The default is a column rather than a mark beside the version, because the question somebody
/// scanning this asks is "which one does `php` mean" and a `*` is a footnote they have to look up.
pub(crate) fn runtime_list(list: &RuntimeList) -> String {
    if list.runtimes.is_empty() {
        return "no runtimes are installed; `mix runtime available` lists what can be\n".to_owned();
    }

    let now = SystemTime::now();
    let rows: Vec<[String; 5]> = list
        .runtimes
        .iter()
        .map(|runtime| {
            [
                runtime.kind.to_string(),
                runtime.version.to_string(),
                match runtime.default {
                    true => "yes".to_owned(),
                    false => MISSING.to_owned(),
                },
                size(runtime.bytes),
                ago(runtime.installed_at, now),
            ]
        })
        .collect();

    table(
        ["RUNTIME", "VERSION", "DEFAULT", "SIZE", "INSTALLED"],
        &rows,
    )
}

/// `mix runtime ext list`, for a person.
///
/// One row per extension: what it is called, whether it can be turned off, and who decided. The last
/// column is the one the command is usually run for — *on because the build says so* and *on because
/// you turned it on* are different answers to why xdebug is loaded.
pub(crate) fn extension_list(list: &ExtensionList) -> String {
    if list.extensions.is_empty() {
        return "this build declares no extensions; nothing to turn on or off\n".to_owned();
    }

    let rows: Vec<[String; 4]> = list
        .extensions
        .iter()
        .map(|extension| {
            [
                extension.name.clone(),
                match extension.linkage {
                    Linkage::Static => "compiled in".to_owned(),
                    Linkage::Shared => "module".to_owned(),
                    _ => MISSING.to_owned(),
                },
                match extension.enabled {
                    true => "on".to_owned(),
                    false => "off".to_owned(),
                },
                match extension.source {
                    ExtensionSource::BuildDefault => "this build".to_owned(),
                    ExtensionSource::User => "you".to_owned(),
                    _ => MISSING.to_owned(),
                },
            ]
        })
        .collect();

    table(["EXTENSION", "KIND", "STATE", "DECIDED BY"], &rows)
}

/// `mix runtime ext enable` and `disable`, for a person.
///
/// Says what it deliberately did *not* do to the pool, because the alternative is a client guessing
/// from the operating system it happens to be running on.
pub(crate) fn extension_change(change: &ExtensionChange) -> String {
    let state = match change.extension.enabled {
        true => "enabled",
        false => "disabled",
    };

    let pool = match change.pool {
        PoolOutcome::Reloaded => "its pool re-read its configuration",
        PoolOutcome::RestartRequired => {
            "the running pool is still using the previous set; restart it to pick this up"
        }
        PoolOutcome::PoolNotRunning => "its pool is not running and will read this when it starts",
        _ => "what its pool did is not something this build can describe",
    };

    format!("{} {state}; {pool}\n", change.extension.name)
}

/// `mix package list`, for a person.
///
/// The last column is what a person opens this listing to find out when an uninstall was refused:
/// which services are instances of this version, and therefore what has to go first.
#[must_use]
pub(crate) fn package_list(list: &PackageList) -> String {
    if list.packages.is_empty() {
        return "no packages are installed; `mix package available` lists what can be
"
        .to_owned();
    }

    let now = SystemTime::now();
    let rows: Vec<[String; 5]> = list
        .packages
        .iter()
        .map(|package| {
            [
                package.package.clone(),
                package.version.to_string(),
                size(package.bytes),
                ago(package.installed_at, now),
                match package.services.is_empty() {
                    true => MISSING.to_owned(),
                    false => package
                        .services
                        .iter()
                        .map(|service| service.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                },
            ]
        })
        .collect();

    table(
        ["PACKAGE", "VERSION", "SIZE", "INSTALLED", "SERVICES"],
        &rows,
    )
}

/// Which rows of a catalogue to print — roadmap task **T193a**, D3.
///
/// **Filtering on what the daemon sent**, which is rendering: `line` and `newest_in_line` are the
/// daemon's answers, and nothing here compares two versions.
#[derive(Debug, Clone, Default)]
pub(crate) struct Lines {
    /// Every release, as before T193.
    pub(crate) all: bool,

    /// Every release of this one line.
    pub(crate) line: Option<String>,
}

impl Lines {
    /// Whether a release with these two members is printed.
    fn shows(&self, line: Option<&str>, newest: Option<bool>) -> bool {
        match (&self.line, line) {
            (Some(wanted), Some(have)) => wanted == have,
            (Some(_), None) => true,
            // A daemon from before lines marks nothing, and its list is printed whole.
            (None, _) => self.all || newest != Some(false),
        }
    }

    /// Whether the `MORE` column is printed: only on the one-row-per-line view of a daemon that
    /// said which line each row is in.
    fn counts(&self, any_line: bool) -> bool {
        !self.all && self.line.is_none() && any_line
    }
}

/// How many other releases share `line`, as a `MORE` cell. `lines` is every row of one name.
fn more(lines: &[Option<&str>], line: Option<&str>) -> String {
    let Some(line) = line else {
        return MISSING.to_owned();
    };

    match lines.iter().filter(|other| **other == Some(line)).count() {
        0 | 1 => MISSING.to_owned(),
        count => format!("+{}", count - 1),
    }
}

/// The lines above a table that name each update, with the command that applies it — D3.
fn update_lines<'a>(
    command: &str,
    updates: impl Iterator<Item = (&'a str, &'a PackageVersion, &'a PackageVersion)>,
) -> String {
    let mut said = String::new();
    for (name, from, to) in updates {
        said.push_str(&format!(
            "update: {name} {from} → {to}   mix {command} upgrade {name} {from}\n"
        ));
    }
    said
}

/// `mix package available`, for a person — one row per line unless `lines` says otherwise (T193a).
#[must_use]
pub(crate) fn package_catalogue(catalogue: &PackageCatalogue, lines: &Lines) -> String {
    let mut rendered = String::new();

    if catalogue.stale {
        rendered.push_str(
            "this list is from a cached index; mixengined could not reach the package index, so \
             versions published since then are missing\n",
        );
    }

    for gap in catalogue.unavailable.iter().flatten() {
        rendered.push_str(&format!(
            "{} is missing from this list, its part of the package index could not be read \
             ({}); `mix package available --refresh` tries again\n",
            gap.name, gap.reason
        ));
    }

    if catalogue.packages.is_empty() {
        rendered.push_str("the package index offers nothing this build can run on this machine\n");
        return rendered;
    }

    rendered.push_str(&update_lines(
        "package",
        catalogue
            .updates
            .iter()
            .flatten()
            .map(|update| (update.package.as_str(), &update.from, &update.to)),
    ));

    let shown: Vec<&PackageRelease> = catalogue
        .packages
        .iter()
        .filter(|release| lines.shows(release.line.as_deref(), release.newest_in_line))
        .collect();
    let counted = lines.counts(
        catalogue
            .packages
            .iter()
            .any(|release| release.line.is_some()),
    );

    let cells = |release: &PackageRelease| {
        [
            release.package.clone(),
            release.version.to_string(),
            release.channel.to_string(),
            size(release.bytes),
            match release.installed {
                true => "yes".to_owned(),
                false => MISSING.to_owned(),
            },
            release.eol.clone().unwrap_or_else(|| MISSING.to_owned()),
        ]
    };

    let emulated = emulation_column(shown.iter().map(|release| release.execution));
    let lacking = any_lacking(shown.iter().map(|release| release.needs.as_ref()));

    if let Some(note) = &emulated {
        rendered.push_str(note);
    }

    let mut headings = PACKAGE_HEADINGS.to_vec();
    if counted {
        headings.push(MORE_HEADING);
    }
    if emulated.is_some() {
        headings.push(RUNS_HEADING);
    }
    if lacking {
        headings.push(NEEDS_HEADING);
    }

    let rows: Vec<Vec<String>> = shown
        .iter()
        .map(|release| {
            let mut row = cells(release).to_vec();
            if counted {
                let same: Vec<Option<&str>> = catalogue
                    .packages
                    .iter()
                    .filter(|other| other.package == release.package)
                    .map(|other| other.line.as_deref())
                    .collect();
                row.push(more(&same, release.line.as_deref()));
            }
            if emulated.is_some() {
                row.push(runs(release.execution));
            }
            if lacking {
                row.push(needs(release.needs.as_ref()));
            }
            row
        })
        .collect();

    rendered.push_str(&table_of(&headings, &rows));
    rendered
}

/// The columns `mix package available` prints when nothing is emulated.
const PACKAGE_HEADINGS: [&str; 6] = ["PACKAGE", "VERSION", "CHANNEL", "SIZE", "INSTALLED", "EOL"];

/// The columns `mix runtime available` prints when nothing is emulated.
const RUNTIME_HEADINGS: [&str; 6] = ["RUNTIME", "VERSION", "CHANNEL", "SIZE", "INSTALLED", "EOL"];

/// The column that counts a line's other releases — roadmap task **T193a**.
const MORE_HEADING: &str = "MORE";

/// The seventh column's heading, on both listings.
const RUNS_HEADING: &str = "RUNS";

/// The column that says what a release lacks on this machine — roadmap task **T151**.
const NEEDS_HEADING: &str = "NEEDS";

/// What one release lacks, in the few words a cell has room for; blank for a release lacking nothing.
fn needs(needs: Option<&Vec<Requirement>>) -> String {
    needs
        .map(|needs| {
            needs
                .iter()
                .map(|requirement| requirement.need.label())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// Whether any release in a listing lacks something here — the one case its `NEEDS` column appears.
fn any_lacking<'a>(needs: impl Iterator<Item = Option<&'a Vec<Requirement>>>) -> bool {
    needs
        .into_iter()
        .any(|needs| needs.is_some_and(|needs| !needs.is_empty()))
}

/// What an install lacks here, one line each with what can be done — roadmap task **T151**.
pub(crate) fn requirements(unmet: &[Requirement]) -> String {
    let mut rendered = String::from("this machine lacks what it needs:\n");
    for requirement in unmet {
        rendered.push_str(&format!(
            "  - {}: {}\n",
            requirement.need, requirement.remedy
        ));
    }
    rendered
}

/// What an install may lack that refuses nothing — roadmap task **T27e**, its design's D16.
pub(crate) fn advisories(advisories: &[Requirement]) -> String {
    let named: Vec<String> = advisories
        .iter()
        .map(|requirement| requirement.need.label())
        .collect();

    format!(
        "warning: this machine's loader does not list {}; install them with this distribution's \
         package manager; the install goes on\n",
        named.join(", ")
    )
}

/// The note that goes above a listing with an emulated row in it, or [`None`] for one without —
/// roadmap task **T92**.
///
/// **A column rather than a line per row, and only when a row needs it.** On five of the six
/// targets MixEngine ships a build for, every release is native and a column reading `native` forty
/// times would be noise; on the sixth it is the answer to the question that machine's owner is
/// about to ask. The note is what makes the word mean something the first time somebody sees it,
/// and it goes above the table for the reason the staleness line does: it is true of the answer
/// rather than of any one row.
fn emulation_column(executions: impl Iterator<Item = Option<Execution>>) -> Option<String> {
    let emulated = executions.into_iter().any(|execution| {
        execution.is_some_and(|execution| matches!(execution, Execution::Emulated))
    });

    emulated.then(|| {
        "emulated; nothing is published for this machine's own architecture, so the x86_64 \
         build is installed and the operating system runs it\n"
            .to_owned()
    })
}

/// What the seventh column says about one release.
///
/// [`None`] is a daemon that predates the member rather than one that could not decide, per
/// [ADR 0019](../../../docs/decisions/0019-an-added-response-member-is-optional.md), so it reads
/// as the same dash every unstated value in these tables does.
fn runs(execution: Option<Execution>) -> String {
    execution.map_or_else(|| MISSING.to_owned(), |execution| execution.to_string())
}

/// `mix package uninstall`, for a person.
#[must_use]
pub(crate) fn package_removal(removal: &PackageRemoval) -> String {
    format!(
        "removed {} {}
",
        removal.removed.package, removal.removed.version
    )
}

/// `mix service create`, for a person.
///
/// **The second paragraph is the whole reason the answer is not just the service** — roadmap task
/// **T34c**. A recipe's preferred port is the number a person has in their `.env` and in their
/// muscle memory, and a service that was quietly given the next one along would be discovered as a
/// connection that is refused, hours later. So a move is stated at the moment it happens, with as
/// much of the program that took the port as this machine would give up.
#[must_use]
/// `mix service found`, for a person: one line per directory, what opens it or why nothing does.
pub(crate) fn service_found(found: &mixengine_proto::ServiceFoundList) -> String {
    if found.found.is_empty() {
        return "nothing an earlier install left is waiting to be adopted\n".to_owned();
    }

    let mut rendered = String::new();
    for row in &found.found {
        let state = match (&row.opens_with, &row.why_not) {
            (Some(version), _) => format!("opens with {version}"),
            (None, Some(why)) => why.clone(),
            (None, None) => "cannot be adopted".to_owned(),
        };
        rendered.push_str(&format!("{}  {state}\n  {}\n", row.service, row.path));
    }

    rendered
}

/// `mix service adopt`, for a person.
pub(crate) fn service_adopted(summary: &ServiceSummary) -> String {
    format!(
        "adopted {}, stopped\n  a new admin password was set where it keeps one; the databases and \
         accounts in it are as they were\n  `mix service start {}` starts it\n",
        summary.id, summary.id
    )
}

pub(crate) fn service_creation(creation: &ServiceCreation) -> String {
    let mut rendered = format!(
        "created {}
",
        creation.service.id
    );

    if let Some(port) = creation.service.port {
        rendered.push_str(&format!(
            "  it listens on port {port}
"
        ));
    }

    if let Some(moved) = &creation.moved_from {
        let holder = match (&moved.program, moved.pid) {
            (Some(program), _) => format!("{program} has it"),
            (None, Some(pid)) => format!("pid {pid} has it"),
            (None, None) => "another service or program on this machine has it".to_owned(),
        };

        rendered.push_str(&format!(
            "  it asked for {}; {holder}, so it was moved
",
            moved.preferred
        ));
    }

    rendered
}

/// `mix service delete`, for a person.
///
/// **The second line is the whole reason the answer is not just the service.** A delete keeps the
/// data directory, and a person who is not told which one it was has no way to find it later — or to
/// know that deleting the service did not delete their databases.
#[must_use]
pub(crate) fn service_removal(removal: &ServiceRemoval) -> String {
    let mut rendered = format!(
        "deleted {}
",
        removal.removed.id
    );

    match &removal.data_kept {
        Some(path) => rendered.push_str(&format!(
            "  its data is kept at {path}
"
        )),
        None => rendered.push_str(
            "  it had no data directory
",
        ),
    }

    rendered
}

/// `mix runtime available`, for a person.
///
/// **The staleness is a line above the table and not a column**, because it is true of the whole
/// answer: every row came out of the same document, and repeating "from a cached index" against each
/// of forty versions would say it forty times.
pub(crate) fn runtime_catalogue(catalogue: &RuntimeCatalogue, lines: &Lines) -> String {
    let mut rendered = String::new();

    if catalogue.stale {
        rendered.push_str(
            "this list is from a cached index; mixengined could not reach the package index, so \
             versions published since then are missing\n",
        );
    }

    for gap in catalogue.unavailable.iter().flatten() {
        rendered.push_str(&format!(
            "{} is missing from this list, its part of the package index could not be read \
             ({}); `mix runtime available --refresh` tries again\n",
            gap.name, gap.reason
        ));
    }

    if catalogue.runtimes.is_empty() {
        rendered.push_str("the package index offers nothing for this machine\n");
        return rendered;
    }

    rendered.push_str(&update_lines(
        "runtime",
        catalogue
            .updates
            .iter()
            .flatten()
            .map(|update| (update.kind.as_str(), &update.from, &update.to)),
    ));

    let shown: Vec<&RuntimeRelease> = catalogue
        .runtimes
        .iter()
        .filter(|release| lines.shows(release.line.as_deref(), release.newest_in_line))
        .collect();
    let counted = lines.counts(
        catalogue
            .runtimes
            .iter()
            .any(|release| release.line.is_some()),
    );

    let cells = |release: &RuntimeRelease| {
        [
            release.kind.to_string(),
            release.version.to_string(),
            release.channel.to_string(),
            size(release.bytes),
            match release.installed {
                true => "yes".to_owned(),
                false => MISSING.to_owned(),
            },
            release.eol.clone().unwrap_or_else(|| MISSING.to_owned()),
        ]
    };

    let emulated = emulation_column(shown.iter().map(|release| release.execution));
    let lacking = any_lacking(shown.iter().map(|release| release.needs.as_ref()));

    if let Some(note) = &emulated {
        rendered.push_str(note);
    }

    let mut headings = RUNTIME_HEADINGS.to_vec();
    if counted {
        headings.push(MORE_HEADING);
    }
    if emulated.is_some() {
        headings.push(RUNS_HEADING);
    }
    if lacking {
        headings.push(NEEDS_HEADING);
    }

    let rows: Vec<Vec<String>> = shown
        .iter()
        .map(|release| {
            let mut row = cells(release).to_vec();
            if counted {
                let same: Vec<Option<&str>> = catalogue
                    .runtimes
                    .iter()
                    .filter(|other| other.kind == release.kind)
                    .map(|other| other.line.as_deref())
                    .collect();
                row.push(more(&same, release.line.as_deref()));
            }
            if emulated.is_some() {
                row.push(runs(release.execution));
            }
            if lacking {
                row.push(needs(release.needs.as_ref()));
            }
            row
        })
        .collect();

    rendered.push_str(&table_of(&headings, &rows));
    rendered
}

/// What an update will do or did, for a person — roadmap tasks **T193b** and **T193c**, D7.
///
/// **One rendering for the plan and for the result**, as the type is one: each line gains how it
/// went once the job has run.
pub(crate) fn upgrade_plan(plan: &UpgradePlan) -> String {
    let mut said = format!("{} {} → {}", plan.subject, plan.from, plan.to);
    said.push_str(&match plan.to_installed {
        true => ", already installed\n".to_owned(),
        false => format!(", {} to download\n", size(plan.bytes)),
    });

    if plan.stale {
        said.push_str(
            "this is judged against a cached index; mixengined could not reach the package index\n",
        );
    }
    if !plan.needs.is_empty() {
        said.push_str(&requirements(&plan.needs));
    }

    for entry in &plan.entries {
        said.push_str(&format!(
            "  - {}{}\n",
            upgrade_item(plan, &entry.item),
            upgrade_outcome(&entry.outcome)
        ));
    }

    let (subject, from) = (&plan.subject, &plan.from);
    said.push_str(&match &plan.old {
        OldVersion::WillBeRemoved {} => {
            format!("{subject} {from} will be removed afterwards; --keep keeps it\n")
        }
        OldVersion::WillBeKept { because } => {
            format!("{subject} {from} will be kept: {}\n", because.join("; "))
        }
        OldVersion::Removed {} => format!("{subject} {from} was removed\n"),
        OldVersion::Kept { because } => {
            format!("{subject} {from} was kept: {}\n", because.join("; "))
        }
    });

    said
}

/// One entry of a plan, as a sentence.
fn upgrade_item(plan: &UpgradePlan, item: &UpgradeItem) -> String {
    let (subject, old, new) = (&plan.subject, &plan.from, &plan.to);
    match item {
        UpgradeItem::Default {} => format!("{new} becomes the default {subject}"),
        UpgradeItem::Site { site, from, to } => format!("{site} moves from {from} to {to}"),
        UpgradeItem::ExtensionPool {
            pool, moves: true, ..
        } => {
            format!("{pool} moves to {subject} {new}")
        }
        UpgradeItem::ExtensionPool {
            pool,
            moves: false,
            requires: Some(requires),
        } => format!("{pool} stays on {subject} {old}: it requires {requires}"),
        UpgradeItem::ExtensionPool {
            pool,
            moves: false,
            requires: None,
        } => format!("{pool} stays on {subject} {old}"),
        UpgradeItem::DroppedExtension { name } => {
            format!("{name} is not in {new}'s build, so it will be off")
        }
        UpgradeItem::Pin { project, from, to } => format!("{project}'s pin {from} becomes {to}"),
        UpgradeItem::Manifest {
            project,
            path,
            constraint,
        } => format!("{project} pins {subject} {constraint} in {path}, which only {old} answers"),
        UpgradeItem::Tool { name } => {
            format!("{name} is installed only in {old}; install it again under {new}")
        }
        UpgradeItem::Instance {
            service,
            restarts: true,
        } => format!("{service} is stopped, moved to {new} and started again"),
        UpgradeItem::Instance {
            service,
            restarts: false,
        } => format!("{service} moves to {new} and stays stopped"),
        UpgradeItem::FrontEndRestart { service } => format!(
            "{service} restarts, so no site answers for a moment; this machine may ask whether \
             {new} may answer on 80 and 443"
        ),
        UpgradeItem::NoDowngrade { service } => {
            format!("{service}: once MySQL {new} has opened its data, {old} cannot open it again")
        }
    }
}

/// How one entry went, as a suffix; nothing for a plan.
fn upgrade_outcome(outcome: &UpgradeOutcome) -> String {
    match outcome {
        UpgradeOutcome::Planned {} => String::new(),
        UpgradeOutcome::Done {} => " (done)".to_owned(),
        UpgradeOutcome::Skipped {} => " (not done)".to_owned(),
        UpgradeOutcome::Failed { because } => format!(" (failed: {because})"),
    }
}

/// One installed runtime, for a person: what `mix runtime default` answers and what a finished
/// install produced.
pub(crate) fn runtime_summary(runtime: &RuntimeSummary) -> String {
    let mut rendered = format!(
        "{} {}{}\n",
        runtime.kind,
        runtime.version,
        match runtime.default {
            true => ", the default for its kind",
            false => "",
        }
    );

    for (label, value) in [
        ("path", runtime.path.clone()),
        ("size", size(runtime.bytes)),
        ("installed", ago(runtime.installed_at, SystemTime::now())),
    ] {
        rendered.push_str(&format!("  {label:9} {value}\n"));
    }

    rendered
}

/// `mix runtime found` and `mix package found`, for a person: one version per line, where it is,
/// and why it is not listed.
pub(crate) fn on_disk<'a>(
    rows: impl Iterator<Item = (String, &'a String, &'a String)>,
    noun: &str,
) -> String {
    let mut rendered = String::new();
    for (name, path, why) in rows {
        rendered.push_str(&format!("{name}  {why}\n  {path}\n"));
    }

    if rendered.is_empty() {
        return "nothing on disk is waiting to be listed\n".to_owned();
    }

    rendered.push_str(&format!("`mix {noun} adopt` records one\n"));
    rendered
}

/// `mix home previous`, for a person.
pub(crate) fn home_previous(previous: &mixengine_proto::HomePrevious) -> String {
    let Some(copy) = &previous.copy else {
        return "no copy of an earlier install is in the folders this home keeps\n".to_owned();
    };

    let mut rendered = format!(
        "a copy of an earlier install: {} projects, {} sites, {} services, {} runtimes, {} packages\n  {}\n",
        copy.projects, copy.sites, copy.services, copy.runtimes, copy.packages, copy.path
    );
    rendered.push_str(match copy.newer {
        true => "  a newer MixEngine wrote it; install that version to restore it\n",
        false => "  `mix home restore` brings it back\n",
    });
    rendered
}

/// `mix home restore`, for a person.
pub(crate) fn home_restored(report: &mixengine_proto::HomeRestoreReport) -> String {
    let mut rendered = format!(
        "restored {} projects, {} sites, {} services, {} runtimes, {} packages\n",
        report.projects, report.sites, report.services, report.runtimes, report.packages
    );
    for line in &report.skipped {
        rendered.push_str(&format!("  skipped  {line}\n"));
    }
    for line in &report.problems {
        rendered.push_str(&format!("  problem  {line}\n"));
    }
    rendered.push_str("  restored databases are stopped, with a new admin password\n");
    rendered
}

/// `mix package adopt`, for a person: [`runtime_summary`]'s layout, for a package.
pub(crate) fn package_summary(package: &mixengine_proto::PackageSummary) -> String {
    let mut rendered = format!("{} {}\n", package.package, package.version);

    for (label, value) in [
        ("path", package.path.clone()),
        ("size", size(package.bytes)),
        ("installed", ago(package.installed_at, SystemTime::now())),
    ] {
        rendered.push_str(&format!("  {label:9} {value}\n"));
    }

    rendered
}

/// `mix runtime uninstall`, for a person.
///
/// The second line is the whole reason the answer is not just the runtime: a kind left with no
/// default is a kind whose shim resolves to nothing, and the person who caused it is the one who
/// should hear about it.
/// `mix runtime resolve`, for a person.
///
/// **The version is the first line and the reason is the last**, in that order because they are read
/// in that order: somebody who already knows which version they expect stops after the first line,
/// and somebody surprised by it reads on to find out which file did it. The path is between them
/// because it is what a person copies.
pub(crate) fn runtime_resolved(resolved: &ResolvedRuntime) -> String {
    let runtime = &resolved.runtime;

    let mut rendered = format!("{} {}\n", runtime.kind, runtime.version);
    rendered.push_str(&format!("  {:9} {}\n", "path", runtime.path));

    if let Some(constraint) = &resolved.constraint {
        rendered.push_str(&format!("  {:9} {constraint}\n", "asked for"));
    }

    let because = match &resolved.source {
        RuntimeSource::Explicit => "what you asked for on this command".to_owned(),
        RuntimeSource::Manifest { path } => path.clone(),
        RuntimeSource::Project { root } => format!("the project registered at {root}"),
        RuntimeSource::Default => format!(
            "the default for {}; nothing here pins a version",
            runtime.kind
        ),
    };
    rendered.push_str(&format!("  {:9} {because}\n", "chosen by"));

    rendered
}

pub(crate) fn runtime_removal(removal: &RuntimeRemoval) -> String {
    let mut rendered = format!(
        "removed {} {}\n",
        removal.removed.kind, removal.removed.version
    );

    if removal.default_cleared {
        rendered.push_str(&format!(
            "  it was the default for {}, and nothing was promoted in its place; \
             `mix runtime default {} <version>` chooses one\n",
            removal.removed.kind, removal.removed.kind
        ));
    }

    rendered
}

/// Which of the three `mix path` subcommands is being rendered.
///
/// The report they answer with is one type — the same sentence about the same directory — and what
/// differs is the first line, because "this is how things stand" and "this is what just happened"
/// are read differently even when the words after them are identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pathed {
    /// `mix path status`.
    Asked,
    /// `mix path install`.
    Installed,
    /// `mix path uninstall`.
    Uninstalled,
    /// `mix path rescan` — roadmap task **T131**.
    Rescanned,
}

/// `mix elevation status`, for a person.
///
/// **The list is the point, and it comes before any offer to raise a prompt.** T64 is what turns
/// this into the screen that explains every operation and what it will literally change *before*
/// somebody is asked to allow it; what is here already prints the operations' own descriptions,
/// because the daemon renders them and a client that composed its own would be composing the
/// sentence a person judges the change by.
pub(crate) fn elevation_status(status: &ElevationStatus) -> String {
    let mut rendered = waiting(status);

    if let Some(last) = &status.last {
        rendered.push_str(&format!("  last      {}\n", grant(last)));
    }

    if status.elevated {
        rendered.push_str(
            "  note      this daemon holds an administrative token; every service it supervises \
             inherits it\n",
        );
    }

    // T88a. The daemon composed this sentence, because what to do about an old helper differs by
    // *which* old helper it is — one that can replace itself is pointed at a command and one that
    // cannot is pointed at the installer — and choosing between those here would be a client
    // deciding what runs as root.
    if let Some(said) = status
        .installed_helper
        .as_ref()
        .and_then(|helper| helper.upgrade.as_ref())
    {
        rendered.push_str(&format!("  helper    {said}\n"));
    }

    match (&status.reason, status.pending.is_empty()) {
        // The reason is the answer, and on Linux it is a command to type. Printed whether or not
        // anything is waiting: a machine that cannot elevate is worth knowing about before the first
        // site is created rather than after.
        (Some(reason), _) => rendered.push_str(&format!("  cannot    {reason}\n")),

        (None, false) => rendered.push_str(
            "\n`mix elevation grant` asks once for all of them; `mix elevation drop` forgets one\n",
        ),

        (None, true) => {}
    }

    rendered
}

/// Everything that is waiting, and what each one will change: the part of the screen that is the
/// same whether it is being reported or being asked about.
///
/// The description is the operation's own — `PrivilegedOp::describe`, rendered by the daemon into
/// `PendingOp::description`. A client that composed its own sentence here would be composing the
/// one a person judges the change by, and it would be the sentence most able to disagree with what
/// is actually applied.
fn waiting(status: &ElevationStatus) -> String {
    let mut rendered = match status.pending.len() {
        0 => "nothing is waiting for permission\n".to_owned(),
        waiting => format!("{} for permission\n", operations(waiting)),
    };

    for pending in &status.pending {
        rendered.push_str(&format!(
            "  {:<4} {}: {}\n",
            pending.id,
            pending.op.name(),
            pending.description
        ));
    }

    rendered
}

/// The screen `mix elevation grant` shows **before** it raises anything — roadmap task **T64**.
///
/// The same list [`elevation_status`] prints, and then the one thing that screen cannot say: a
/// prompt is about to appear, it will appear once, and it will name this program. What is
/// deliberately absent is the advice [`elevation_status`] ends with — a person reading this is
/// already running `mix elevation grant`.
pub(crate) fn elevation_prompt(status: &ElevationStatus) -> String {
    let helper = status.helper.as_deref().unwrap_or("mixengine-elevate");

    format!(
        "{}\nyour operating system will ask once, for all of them, to allow\n  {helper}\n",
        waiting(status)
    )
}

/// What one grant did, in a line — and under it, what anything that did not happen had to say.
///
/// **The counts alone are not a diagnosis.** *0 applied, 1 still waiting* is true and there is
/// nothing to do with it: until 2026-09-16 the reason an operation gave lived in the audit log and
/// the daemon's log, neither of which is in front of the person who has just typed a password. One
/// of them was granted eight times against a sentence nobody had been shown.
fn grant(outcome: &GrantOutcome) -> String {
    let what = match &outcome.outcome {
        // A choice and not a failure — ADR 0005. The word carries that, and nothing here adds to it.
        ElevationOutcome::Declined => "declined".to_owned(),
        ElevationOutcome::Unavailable { reason } => format!("could not be raised: {reason}"),
        ElevationOutcome::Completed => format!(
            "{} applied, {} still waiting",
            outcome.applied, outcome.still_pending
        ),
    };

    let mut rendered = format!("job {}: {what}", outcome.job);

    // The daemon's own sentences, one per line, unchanged. Which operation each is about is already
    // in the sentence, because the daemon put it there.
    for problem in &outcome.problems {
        rendered.push_str(&format!("\n  {problem}"));
    }

    rendered
}

/// "1 operation is" / "3 operations are", so a sentence built from a count reads.
fn services(count: usize) -> String {
    match count {
        1 => "1 service".to_owned(),
        many => format!("{many} services"),
    }
}

/// The same, for the queue of privileged operations.
fn operations(count: usize) -> String {
    match count {
        1 => "1 operation is waiting".to_owned(),
        many => format!("{many} operations are waiting"),
    }
}

/// `mix path …`, for a person.
///
/// **The last line is the one that matters and it is about a shell that is not this one.** Nothing
/// `mix` can do changes the PATH of the terminal it was typed in — a child process cannot reach into
/// its parent's environment on any of the three systems — so an install that says nothing looks
/// exactly like one that did not work, to somebody who types `php` immediately afterwards and is
/// told there is no such command.
pub(crate) fn path_report(pathed: Pathed, report: &PathReport) -> String {
    let mut rendered = match (pathed, report.on_path) {
        (Pathed::Asked, true) => format!("{} is on this user's PATH\n", report.directory),
        (Pathed::Asked, false) => format!("{} is not on this user's PATH\n", report.directory),

        (Pathed::Installed, _) => match report.places.iter().any(|place| place.changed) {
            true => format!("{} is now on this user's PATH\n", report.directory),
            false => format!("{} was already on this user's PATH\n", report.directory),
        },

        (Pathed::Uninstalled, _) => match report.places.iter().any(|place| place.changed) {
            true => format!("{} is no longer on this user's PATH\n", report.directory),
            false => format!("{} was not on this user's PATH\n", report.directory),
        },

        // The PATH is not what a rescan is about, so it is not what the first line says: what
        // changed is the *contents* of the directory, and the listing below is where that shows.
        (Pathed::Rescanned, _) => format!("{} matches what is installed\n", report.directory),
    };

    for place in &report.places {
        rendered.push_str(&format!(
            "  {} {}\n",
            match place.present {
                true => "in ",
                false => "not in",
            },
            place.name
        ));
    }

    if report.places.is_empty() {
        rendered.push_str("  this machine has nowhere to keep a PATH that survives a reboot\n");
    }

    rendered.push_str(&format!(
        "  {} command{} in it: {}\n",
        report.commands.len(),
        match report.commands.len() {
            1 => "",
            _ => "s",
        },
        match report.commands.is_empty() {
            true => "none; `mix path install` fills the directory".to_owned(),
            false => report.commands.join(", "),
        }
    ));

    // **Where the ones that are not compiled in came from** — roadmap tasks T130 and T131. The
    // line above is a list of names; this is the half that tells somebody why `mysqldump` and
    // `yarn` are on their PATH, and which install would change if they uninstalled something.
    for origin in &report.origins {
        let said = match &origin.source {
            CommandSource::BuiltIn {} => continue,
            CommandSource::Client { package } => format!("a client of {package}"),
            CommandSource::Global { kind } => format!("installed into a {kind}"),
        };

        rendered.push_str(&format!("  {:<18} {said}\n", origin.command));
    }

    for conflict in &report.conflicts {
        rendered.push_str(&format!(
            "  {} runs {}'s, which {} also publishes\n",
            conflict.command,
            conflict.won,
            conflict.lost.join(" and ")
        ));
    }

    for stale in &report.stale {
        rendered.push_str(&format!(
            "  {stale} is in that directory and answers to nothing; it could not be removed\n"
        ));
    }

    if pathed != Pathed::Asked && report.places.iter().any(|place| place.changed) {
        rendered.push_str("open a new terminal for this to take effect\n");
    }

    rendered
}

/// `mix job list`, for a person.
pub(crate) fn job_list(list: &JobList) -> String {
    if list.jobs.is_empty() {
        return "this home has run no jobs\n".to_owned();
    }

    let now = SystemTime::now();
    let rows: Vec<[String; 5]> = list
        .jobs
        .iter()
        .map(|job| {
            [
                job.id.to_string(),
                job.kind.to_string(),
                job.state.to_string(),
                format!("{}%", job.percent),
                ago(job.started_at, now),
            ]
        })
        .collect();

    table(["JOB", "KIND", "STATE", "PROGRESS", "STARTED"], &rows)
}

/// One job, for a person: what `mix job status`, `wait` and `cancel` all answer with.
///
/// **A failed job's error is rendered as the daemon wrote it**, message and hint, rather than
/// summarised here — it is the same wire error the call would have been refused with had the work
/// been short enough to do inline, and rewording it would give one failure two spellings.
pub(crate) fn job_status(job: &JobSummary) -> String {
    let mut rendered = format!("job {}: {} ({})\n", job.id, job.state, job.kind);

    let mut field = |label: &str, value: &str| {
        rendered.push_str(&format!("  {label:9} {value}\n"));
    };

    if !job.message.is_empty() {
        field("doing", &format!("{} ({}%)", job.message, job.percent));
    }
    field("started", &ago(job.started_at, SystemTime::now()));

    match &job.outcome {
        Some(JobOutcome::Failed { error }) => {
            for line in error.to_string().lines() {
                rendered.push_str(&format!("  {line}\n"));
            }
        }

        // The result belongs to the method that produced the job, so this is the one place a
        // rendering has to branch on the kind rather than on the type. `runtime.install` is the only
        // producer there is; anything else prints nothing extra rather than guessing at a shape.
        Some(JobOutcome::Succeeded { result }) => {
            if let Ok(runtime) = serde_json::from_value::<RuntimeSummary>(result.clone()) {
                for line in runtime_summary(&runtime).lines() {
                    rendered.push_str(&format!("  {line}\n"));
                }
            }

            if let Ok(outcome) = serde_json::from_value::<GrantOutcome>(result.clone()) {
                rendered.push_str(&format!("  {}\n", grant(&outcome)));
            }
        }

        _ => {}
    }

    rendered
}

/// Whether a job that ended did what was asked, which is what an exit code is made of.
pub(crate) fn job_succeeded(job: &JobSummary) -> bool {
    job.state == JobState::Succeeded
}

/// `mix metrics` — what everything is costing right now.
///
/// **A dash where a CPU figure could not be taken, and never a zero.** A group measured for the
/// first time has no difference to report yet, and printing `0.0%` there would say a service is
/// idling in the second it is most expensive.
pub(crate) fn metrics_frame(frame: &MetricsFrame) -> String {
    if frame.samples.is_empty() {
        return "nothing could be measured
"
        .to_owned();
    }

    let rows: Vec<[String; 4]> = frame
        .samples
        .iter()
        .map(|sample| {
            [
                sample.subject.to_string(),
                percent(sample.cpu_percent, frame.cores),
                memory(sample.rss_bytes),
                sample.processes.to_string(),
            ]
        })
        .collect();

    table(["SUBJECT", "CPU", "MEMORY", "PROCESSES"], &rows)
}

/// `mix metrics --since` — the per-minute history, oldest first.
///
/// **`SAMPLES` is on the table rather than only in `--json`.** A minute made of one reading and one
/// made of sixty are both averages, and a person comparing two rows has to be able to see which is
/// which. A row is only ever missing because nothing was measured that minute — the service was
/// stopped, or the machine was asleep — so the gaps are part of the answer.
///
/// **The minute is printed as an age rather than as a clock time**, which is [`ago`]'s rule and this
/// workspace's: turning epoch milliseconds into 14:03 needs a civil calendar, and nothing here has
/// one. `--json` carries the millisecond, which is what a chart wants anyway.
pub(crate) fn metrics_history(history: &MetricsHistory, now: SystemTime) -> String {
    if history.minutes.is_empty() {
        return format!(
            "no readings in that window; this home keeps {} hours of them
",
            history.retention_hours
        );
    }

    let rows: Vec<[String; 6]> = history
        .minutes
        .iter()
        .map(|minute| {
            [
                ago(minute.minute, now),
                minute.subject.to_string(),
                percent(minute.cpu_avg, history.cores),
                percent(minute.cpu_peak, history.cores),
                memory(minute.rss_peak),
                minute.samples.to_string(),
            ]
        })
        .collect();

    table(
        [
            "MINUTE",
            "SUBJECT",
            "CPU AVG",
            "CPU PEAK",
            "MEMORY PEAK",
            "SAMPLES",
        ],
        &rows,
    )
}

/// A share of the whole machine with one decimal, as Task Manager shows it — roadmap task T190c —
/// or a dash where no figure was taken.
///
/// `cpu` is percent of one core and `cores` what one core is worth here; `0` is read as one. A
/// figure above zero that rounds to `0.0` prints `<0.1%`, so a running service never reads as idle.
fn percent(cpu: Option<f32>, cores: u32) -> String {
    cpu.map_or_else(
        || MISSING.to_owned(),
        |cpu| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a count of logical processors, far below f32's exact integers"
            )]
            let share = cpu / cores.max(1) as f32;

            if share > 0.0 && share < 0.05 {
                "<0.1%".to_owned()
            } else {
                format!("{share:.1}%")
            }
        },
    )
}

/// Resident bytes, at the scale a person reads memory in.
///
/// Mebibytes with one decimal, unlike [`size`]: a download is tens or hundreds of them and a service
/// is often under ten, where whole numbers would round php-fpm and Redis to the same figure.
fn memory(bytes: u64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "one decimal place of mebibytes")]
    let mib = bytes as f64 / (1u64 << 20) as f64;

    format!("{mib:.1} MiB")
}

/// A number of bytes, at the scale a download is read in.
///
/// Whole mebibytes, and never a fraction: what this number answers is "will this take a while and is
/// there room", and `41 MiB` answers it exactly as well as `40.7 MiB` while being a number a person
/// takes in at a glance. `--json` carries the byte count, unrounded.
fn size(bytes: u64) -> String {
    const MIB: u64 = 1 << 20;

    match bytes {
        0 => MISSING.to_owned(),
        // Anything smaller than a mebibyte would round to `0 MiB`, which reads as "nothing" for a
        // file that is really there.
        1..MIB => "< 1 MiB".to_owned(),
        _ => format!("{} MiB", bytes / MIB),
    }
}

/// What `mix uninstall` prints when it starts waiting for the directories to go — T182b.
pub(crate) fn uninstall_removing(directories: usize, bytes: u64) -> String {
    let what = match directories {
        1 => "1 folder".to_owned(),
        count => format!("{count} folders"),
    };

    match bytes {
        0 => format!("removing {what}, this can take a minute"),
        _ => format!("removing {what} ({}), this can take a minute", size(bytes)),
    }
}

/// And every so often while it waits, so a person can see it has not stopped.
pub(crate) fn uninstall_still_removing(waited: std::time::Duration) -> String {
    format!("still removing, {}s so far", waited.as_secs())
}

/// A list of services, in the order the daemon gave them.
fn names(services: &[ServiceId]) -> String {
    match services.is_empty() {
        true => MISSING.to_owned(),
        false => services
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// A listing with its headings, every column as wide as its widest cell.
///
/// Generic over the number of columns rather than written once per table: four commands here answer
/// with a listing now, and the alternative is four copies of the same width calculation drifting
/// apart in how they pad and where they trim.
fn table<const N: usize>(headings: [&str; N], rows: &[[String; N]]) -> String {
    let rows: Vec<Vec<String>> = rows.iter().map(|row| row.to_vec()).collect();
    table_of(&headings, &rows)
}

/// [`table`] for a column count decided at run time — roadmap task **T151**, where a listing may
/// grow `RUNS`, `NEEDS`, both or neither.
pub(crate) fn table_of(headings: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..headings.len())
        .map(|column| {
            rows.iter()
                .map(|row| row.get(column).map_or(0, |cell| cell.chars().count()))
                .chain(std::iter::once(headings[column].chars().count()))
                .max()
                .unwrap_or_default()
        })
        .collect();

    let mut rendered = String::new();
    let headings: Vec<String> = headings
        .iter()
        .map(|heading| (*heading).to_owned())
        .collect();

    for row in std::iter::once(&headings).chain(rows.iter()) {
        let line = row
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:width$}"))
            .collect::<Vec<_>>()
            .join("  ");

        // Trimmed, so a table's last column carries no trailing run of spaces into whatever a
        // person pastes it into.
        rendered.push_str(line.trim_end());
        rendered.push('\n');
    }

    rendered
}

/// How long ago something happened, from this machine's clock.
///
/// **The client's own clock, and it is the daemon's too**: the endpoint is a local socket, so there
/// is exactly one clock involved. `daemon.status` carries an `Uptime` because the daemon knows how
/// long it has been up; nothing carries a "now" for a service, and asking for one would be a round
/// trip to learn what `SystemTime::now` already says.
///
/// A moment in the future — a clock moved backwards between the start and this call — reads as
/// `just now` rather than as a negative age.
fn ago(Timestamp(happened): Timestamp, now: SystemTime) -> String {
    let Timestamp(now) = Timestamp::from_system_time(now);

    match u64::try_from(now.saturating_sub(happened) / 1_000) {
        Ok(0) | Err(_) => "just now".to_owned(),
        Ok(seconds) => format!("{} ago", units(seconds)),
    }
}

/// How long until something happens, from this machine's clock — roadmap task **T76**.
///
/// [`ago`]'s mirror, sharing [`units`] with it so that "in 1h 58m" and "1h 58m ago" round the same
/// way. A moment already gone reads as `any moment now` rather than as a negative wait: the loop
/// that ends a share runs on a period, so a deadline can be a few seconds past while the share is
/// still up, and that is a wait rather than a fault.
fn in_time(Timestamp(happens): Timestamp, now: SystemTime) -> String {
    let Timestamp(now) = Timestamp::from_system_time(now);

    match u64::try_from(happens.saturating_sub(now) / 1_000) {
        Ok(0) | Err(_) => "any moment now".to_owned(),
        Ok(seconds) => format!("in {}", units(seconds)),
    }
}

/// This build of `mix`, in the shape the daemon reports itself in.
fn client() -> serde_json::Value {
    serde_json::to_value(DaemonVersion {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: PROTOCOL_VERSION,
    })
    .expect("a DaemonVersion of two owned fields always serialises")
}

/// How long the daemon has been up, in the two units that matter at that scale.
///
/// Two and never three: "up 3d 4h" is what somebody wants from a status line, and "3d 4h 17m 6s" is
/// a number nobody reads. The exact figure is in `--json`, in seconds, unrounded.
fn uptime(Uptime(seconds): Uptime) -> String {
    units(seconds)
}

/// A number of seconds, at the scale a person reads.
///
/// Shared by [`uptime`] and [`ago`] rather than written twice: "up 13m 32s" and "started 13m 32s
/// ago" are the same rounding, and two copies of it would eventually round differently in one place.
fn units(seconds: u64) -> String {
    let (days, hours, minutes, seconds) = (
        seconds / 86_400,
        (seconds % 86_400) / 3_600,
        (seconds % 3_600) / 60,
        seconds % 60,
    );

    match (days, hours, minutes) {
        (0, 0, 0) => format!("{seconds}s"),
        (0, 0, _) => format!("{minutes}m {seconds}s"),
        (0, _, _) => format!("{hours}h {minutes}m"),
        _ => format!("{days}d {hours}h"),
    }
}

/// `mix project list` — every registered project, and whether it has a manifest.
pub(crate) fn project_list(list: &ProjectList) -> String {
    if list.projects.is_empty() {
        return "no projects are registered; `mix project create <dir>` adds one\n".to_owned();
    }

    let mut out = format!("{:<24}  {:<9}  {}\n", "PROJECT", "MANIFEST", "ROOT");

    for project in &list.projects {
        out.push_str(&format!(
            "{:<24}  {:<9}  {}\n",
            project.name,
            if project.manifest.is_some() {
                "yes"
            } else {
                "—"
            },
            project.root
        ));
    }

    out
}

/// `mix project show` — one project, and what each pin actually resolves to.
///
/// The **source** column is the whole value of the rendering: a pin read from the manifest outranks
/// the row, so a person looking at a version they did not expect is looking for which of the two is
/// in charge.
pub(crate) fn project_detail(detail: &ProjectDetail) -> String {
    let mut out = format!(
        "{}\n  root      {}\n  created   {}\n",
        detail.project.name, detail.project.root, detail.project.created_at
    );

    if let Some(manifest) = &detail.project.manifest {
        out.push_str(&format!("  manifest  {manifest}\n"));
    }

    if detail.pins.is_empty() {
        out.push_str("\nno runtimes are pinned\n");
    } else {
        out.push_str(&format!(
            "\n{:<8}  {:<10}  {:<10}  {}\n",
            "RUNTIME", "PINNED", "RESOLVES", "FROM"
        ));

        for pin in &detail.pins {
            let from = match &pin.source {
                PinSource::Registered => "this home".to_owned(),
                PinSource::Manifest { path } => path.clone(),
            };

            out.push_str(&format!(
                "{:<8}  {:<10}  {:<10}  {}\n",
                pin.kind.as_str(),
                pin.constraint.as_str(),
                pin.resolved.as_ref().map_or("—", PackageVersion::as_str),
                from
            ));
        }

        for hint in detail.pins.iter().filter_map(|pin| pin.hint.as_ref()) {
            out.push_str(&format!("\n{hint}\n"));
        }
    }

    // **T204, D7.** What mixengine.toml declares, and the command for each one not here yet.
    if !detail.declared_sites.is_empty() {
        out.push_str(&format!("\n{:<28}  {}\n", "IN MIXENGINE.TOML", "HERE"));

        for site in &detail.declared_sites {
            let here = match &site.state {
                DeclaredSiteState::Here => "yes".to_owned(),
                DeclaredSiteState::Missing => {
                    format!("no; `mix site create --from {}` adopts it", site.domain)
                }
                DeclaredSiteState::Elsewhere { owner } => format!("no; {owner} holds it"),
            };
            out.push_str(&format!("{:<28}  {here}\n", site.domain));
        }
    }

    out
}

/// `mix project delete` — and the directory it did not touch.
pub(crate) fn project_removal(removal: &ProjectRemoval) -> String {
    let mut out = format!("{} is no longer registered\n", removal.removed.name);
    out.push_str(&format!("  the directory is kept: {}\n", removal.root_kept));

    if let Some(manifest) = &removal.manifest_kept {
        out.push_str(&format!("  so is its manifest:   {manifest}\n"));
    }

    out
}

/// `mix project export` — which file, and whether it had to be made.
pub(crate) fn project_export(exported: &ProjectExport) -> String {
    let mut out = match exported.created {
        true => format!("wrote {}\n", exported.path),
        false => format!(
            "updated {}; everything else in it is untouched\n",
            exported.path
        ),
    };

    // **T204, D4.** What the file still declares that this home does not have, left as written.
    if !exported.sites_kept.is_empty() {
        let names = match exported.sites_kept.len() {
            1 => "that name",
            _ => "those names",
        };
        out.push_str(&format!(
            "kept {} in mixengine.toml, though no site here has {names}\n",
            exported.sites_kept.join(", ")
        ));
    }

    out
}

/// `mix site list` — every site, and what serves it.
pub(crate) fn site_list(list: &SiteList) -> String {
    if list.sites.is_empty() {
        return "no sites are declared; `mix site create` adds one\n".to_owned();
    }

    // **A count and not the routes themselves** — roadmap task **T135**. What a listing is for is
    // "does this site have more behind it"; which prefix answers what is `mix site show`'s, where
    // there is room to print the address beside it.
    let mut out = format!(
        "{:<28}  {:<14}  {:<7}  {:<9}  {}\n",
        "DOMAIN", "KIND", "ROUTES", "STATE", "OWNER"
    );

    for site in &list.sites {
        let routes = match site.routes.len() {
            0 => "—".to_owned(),
            count => count.to_string(),
        };

        out.push_str(&format!(
            "{:<28}  {:<14}  {routes:<7}  {:<9}  {}\n",
            site.domain,
            kind_word(&site.kind),
            site.state.as_str(),
            owner_word(&site.owner)
        ));
    }

    out
}

/// Who a site belongs to, as a column reads it — roadmap task **T81b**.
fn owner_word(owner: &SiteOwner) -> String {
    match owner {
        SiteOwner::Project { name } => name.clone(),
        SiteOwner::Extension { id } => format!("extension {id}"),
    }
}

/// What a route's target is printed as — roadmap task **T135**.
///
/// The target *and* its address, because a path on its own answers nothing a person came to find
/// out: `/api  →  http://127.0.0.1:3003/xyz` is the line, and the arrow is what makes the direction
/// readable at a glance.
fn route_target_word(target: &mixengine_proto::RouteTarget) -> String {
    match target {
        mixengine_proto::RouteTarget::Proxy { upstream } => format!("→ {upstream}"),
        mixengine_proto::RouteTarget::PhpFpm { pool } => format!(
            "php-fpm {}",
            pool.as_ref()
                .map_or("(the service it named is gone)", ServiceId::as_str)
        ),
        mixengine_proto::RouteTarget::Static { root } => format!("files in {root}"),
    }
}

/// The word a person typed for a kind, which is the word the wire uses.
fn kind_word(kind: &SiteKind) -> &'static str {
    match kind {
        SiteKind::PhpFpm { .. } => "php-fpm",
        SiteKind::Static => "static",
        SiteKind::ReverseProxy { .. } => "reverse-proxy",
        SiteKind::NodeApp { .. } => "node-app",
    }
}

/// `daemon.doctor`, as a person reads it — roadmap task **T47a**.
///
/// **Every check gets a line, including the ones that found nothing.** A doctor that printed only
/// faults would leave a person unsure it looked, which is the whole reason the report carries what
/// was examined rather than only what was wrong.
///
/// The word in the margin is the outcome and the indented line under it is the daemon's own
/// sentence — the daemon's `because` never carries advice (T47a design, D3), so the one line telling
/// a person what to do about a `PROBLEM` is this client's own, appended once for the whole report
/// rather than repeated under every line that earned it.
pub(crate) fn doctor(report: &DoctorReport) -> String {
    let mut out = String::new();

    for check in &report.checks {
        let (mark, because) = match &check.outcome {
            Outcome::Ok {} => ("ok     ", None),
            Outcome::Note { because } => ("note   ", Some(because)),
            Outcome::Problem { because, .. } => ("PROBLEM", Some(because)),
            Outcome::Skipped { because } => ("skipped", Some(because)),
        };

        out.push_str(&format!("{mark}  {}\n", check.name));

        if let Some(because) = because {
            out.push_str(&format!("         {because}\n"));
        }
    }

    if report.has_a_problem() {
        out.push_str("\nrun `mix doctor --repair` to fix what it can\n");
    }

    out
}

/// `daemon.doctor_repair`, as a person reads it — roadmap task **T47b**.
///
/// **The same three margins as [`doctor`]**, so the two read as one tool: what was done, what is
/// waiting, and what nothing could be done about. A `PROBLEM` here means the same thing it means
/// there — something is wrong and this build cannot fix it.
///
/// A repair that found nothing prints a sentence rather than nothing at all, for `doctor`'s reason
/// one document along: silence cannot be told apart from a command that did not run.
pub(crate) fn repair(report: &RepairReport) -> String {
    if report.actions.is_empty() {
        return "nothing to repair\n".to_owned();
    }

    let mut out = String::new();

    for action in &report.actions {
        let (mark, sentence) = match &action.outcome {
            Action::Repaired { what } => ("repaired", what),
            Action::Enqueued { what } => ("waiting ", what),
            Action::Untouched { because } => ("PROBLEM ", because),
        };

        out.push_str(&format!("{mark}  {}\n", action.name));
        out.push_str(&format!("          {sentence}\n"));
    }

    out
}

/// `mix uninstall`, as a person reads it — roadmap task **T87**.
///
/// **Every row, in the daemon's order, whatever it answered.** A rendering that hid the `absent`
/// rows would leave a person unable to tell *"there was no resolver wiring"* from *"the resolver
/// wiring was not looked at"*, on the one command whose whole promise is that nothing is left
/// behind.
///
/// **And the place is printed under every row, including the absent ones**, because the place is what
/// somebody goes and checks afterwards. A row saying "nothing there" without saying *where* is a row
/// nobody can verify.
pub(crate) fn uninstall_report(report: &UninstallReport) -> String {
    let mut out = String::new();

    for item in &report.items {
        let (mark, sentence) = match &item.outcome {
            Removal::Absent {} => ("nothing  ", None),
            Removal::Planned { how } => ("would    ", Some(how)),
            Removal::Removed { what } => ("removed  ", Some(what)),
            Removal::Enqueued { what } => ("waiting  ", Some(what)),
            Removal::OnExit { what } => ("going    ", Some(what)),
            Removal::OnRestart { what } => ("restart  ", Some(what)),
            Removal::Kept { because } => ("kept     ", Some(because)),
            Removal::Failed { because } => ("LEFT     ", Some(because)),
            Removal::Blocked { by } => ("BLOCKED  ", Some(by)),
        };

        out.push_str(&format!("{mark}{}\n", item.what));
        out.push_str(&format!("         {}\n", item.location));

        if let Some(sentence) = sentence {
            out.push_str(&format!("         {sentence}\n"));
        }
    }

    out
}

/// `daemon.disk_usage`, as a person reads it — roadmap task **T96**.
///
/// **Every row carries what would take it back**, because the question somebody has when they look
/// at a disk table is not *how big* but *what can I do about it* — and four of the five rows have a
/// different answer. The line at the end is the only number that leads to a command.
pub(crate) fn disk_usage(usage: &DiskUsage) -> String {
    let mut rows: Vec<[String; 3]> = usage
        .categories
        .iter()
        .map(|category| {
            [
                category.id.as_str().to_owned(),
                size(category.bytes),
                reclaim(&category.reclaim),
            ]
        })
        .collect();

    // Not a sixth category: the five are what the API answers, and this is the arithmetic that keeps
    // the table adding up to what the file manager says about the same directory.
    rows.push([
        "other".to_owned(),
        size(usage.other_bytes),
        "packages, generated config, the database".to_owned(),
    ]);

    // The summary line is not a row — it would read as a sixth category — so it lines its own label
    // up against the ones above rather than against a width written down twice.
    let label = rows
        .iter()
        .map(|row| row[0].chars().count())
        .max()
        .unwrap_or_default();

    let mut out = table(["", "size", "reclaimed by"], &rows);

    out.push_str(&format!(
        "\n{:<label$}  {}\n",
        "total",
        size(usage.total_bytes())
    ));

    for category in &usage.categories {
        if let Some(note) = &category.unreadable {
            out.push_str(&format!("{}: {note}\n", category.id.as_str()));
        }
    }

    let reclaimable = usage.reclaimable_bytes();
    out.push_str(&match reclaimable {
        0 => "\nthere is nothing `mix cleanup` would take back\n".to_owned(),
        _ => format!("\n`mix cleanup` would take back {}\n", size(reclaimable)),
    });

    out
}

/// What would reclaim one row, in a phrase.
fn reclaim(reclaim: &Reclaim) -> String {
    match reclaim {
        Reclaim::Never { because } | Reclaim::AtACost { because } => because.clone(),
        Reclaim::ByMethod { method, because } => format!("{method}: {because}"),
        Reclaim::ByCleanup { bytes, files } => {
            format!("`mix cleanup`: {} in {files} file(s)", size(*bytes))
        }
    }
}

/// `daemon.cleanup`, as a person reads it — roadmap task **T96**.
///
/// **Every row, whatever it answered**, on [`uninstall_report`]'s rule: a report that printed only
/// what it removed would leave somebody unable to tell *"there were no rotated log files"* from
/// *"the logs were not looked at"*.
pub(crate) fn cleanup_report(report: &CleanupReport) -> String {
    let mut out = String::new();

    for item in &report.items {
        let (mark, sentence) = match &item.outcome {
            Cleanup::Empty {} => ("nothing  ", None),
            Cleanup::Reclaimed { files, bytes } => (
                "took     ",
                Some(format!("{} in {files} file(s)", size(*bytes))),
            ),
            Cleanup::Partial {
                files,
                bytes,
                left_behind,
                because,
            } => (
                "LEFT     ",
                Some(format!(
                    "{} in {files} file(s); {left_behind} file(s) would not go: {because}",
                    size(*bytes)
                )),
            ),
            Cleanup::Kept { because } => ("kept     ", Some(because.clone())),
            Cleanup::Failed { because } => ("LEFT     ", Some(because.clone())),
        };

        out.push_str(&format!("{mark}{}\n", item.id.as_str()));
        out.push_str(&format!("         {}\n", item.location));

        if let Some(sentence) = sentence {
            out.push_str(&format!("         {sentence}\n"));
        }
    }

    out.push_str(&format!("\ntook back {}\n", size(report.reclaimed_bytes())));

    out
}

/// `daemon.bundle`, as a person reads it — roadmap task **T93**.
///
/// **The omissions are printed and not summarised.** They are the half a person will not otherwise
/// know to ask about, and a bundle whose gaps live only in a JSON field is a bundle whose gaps get
/// discovered by whoever opens it three days later, looking for the file that is not there.
pub(crate) fn bundle(report: &BundleReport, copied_to: Option<&std::path::Path>) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "wrote  {}
",
        report.path
    ));
    out.push_str(&format!(
        "       {}, {} file(s)
",
        size(report.bytes),
        report.members.len()
    ));

    if let Some(destination) = copied_to {
        out.push_str(&format!(
            "copied {}
",
            destination.display()
        ));
    }

    if !report.omitted.is_empty() {
        out.push_str(
            "
not included
",
        );
        for left in &report.omitted {
            out.push_str(&format!(
                "       {}: {}
",
                left.name, left.because
            ));
        }
    }

    out
}

/// `domain.dns_status`, as a person reads it — roadmap task **T46**.
///
/// **One column per fact, because the four fail independently.** A single "works / does not" column
/// would be exactly the derivation the report exists to prevent; the sentence under a failing row is
/// the thing a person acts on, and it is the daemon's sentence rather than this client's.
pub(crate) fn domain_status(report: &DomainStatusReport) -> String {
    if report.domains.is_empty() {
        return "no domains declared
"
        .to_owned();
    }

    let width = report
        .domains
        .iter()
        .map(|row| row.domain.len())
        .max()
        .unwrap_or_default();

    let mut out = String::new();

    for row in &report.domains {
        let resolved = if row.resolves_to.is_empty() {
            "does not resolve".to_owned()
        } else {
            row.resolves_to
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };

        out.push_str(&format!(
            "{:width$}  {}  {}  {}  {}
",
            row.domain,
            if row.site.is_some() {
                "declared"
            } else {
                "unknown "
            },
            if row.hosts_entry { "hosts" } else { "     " },
            if row.wildcard { "wildcard" } else { "        " },
            resolved,
        ));

        if let Some(because) = &row.because {
            out.push_str(&format!(
                "{:width$}  {because}
",
                ""
            ));
        }
    }

    out
}

/// The `https` line's value, with the redirect folded in rather than given a row of its own —
/// roadmap task **T98**. A redirect is never reachable without HTTPS already being on, so it is not
/// a fact worth asking a reader to check two lines to get.
fn https_word(https: bool, redirect: bool) -> String {
    match (https, redirect) {
        (true, true) => "yes (redirect)".to_owned(),
        (true, false) => "yes".to_owned(),
        (false, _) => "no".to_owned(),
    }
}

/// `mix site show` — one site, and the two answers about its pool.
///
/// The **pool** lines are the whole value of the rendering: a site's pool is frozen at create while
/// the shell in the same directory keeps following the default, so somebody looking at a PHP
/// version they did not expect is looking for which of the two is in charge.
pub(crate) fn site_detail(detail: &SiteDetail) -> String {
    let mut out = format!(
        "{}\n  owner     {}\n  kind      {}\n  root      {}\n  doc root  {}{}\n  https     {}\n  \
         state     {}\n",
        detail.site.domain,
        owner_word(&detail.site.owner),
        kind_word(&detail.site.kind),
        detail.root,
        detail.doc_root_full,
        if detail.doc_root_exists {
            ""
        } else {
            "  (not there yet)"
        },
        https_word(detail.site.https, detail.site.https_redirect),
        detail.site.state.as_str()
    );

    match &detail.site.kind {
        SiteKind::ReverseProxy { upstream } => {
            out.push_str(&format!("  upstream  {upstream}\n"));
        }
        SiteKind::NodeApp { port } => out.push_str(&format!("  port      {port}\n")),
        SiteKind::PhpFpm { .. } | SiteKind::Static => {}
    }

    if let Some(pool) = &detail.pool {
        out.push_str(&format!(
            "  pool      {}\n",
            pool.declared
                .as_ref()
                .map_or("(the service it named is gone)", ServiceId::as_str)
        ));

        if pool.declared != pool.resolved {
            out.push_str(&format!(
                "  resolves  {}; this directory resolves to a different PHP than the site was \
                 declared with\n",
                pool.resolved.as_ref().map_or("—", ServiceId::as_str)
            ));
        }
    }

    if let Some(sharing) = &detail.site.sharing {
        out.push_str(&format!(
            "  shared    {} on {}\n",
            sharing.url, sharing.interface
        ));

        if let Some(until) = sharing.until {
            out.push_str(&format!(
                "  ends      {}\n",
                in_time(until, SystemTime::now())
            ));
        }
    }

    if detail.domains.len() > 1 {
        out.push_str(&format!("\naliases: {}\n", detail.domains[1..].join(", ")));
    }

    // **In match order, which is what the daemon answers with** — roadmap task **T135**. Not the
    // order somebody typed: the front end resolves an overlap by specificity, and a listing showing
    // declaration order would be showing something no server does.
    if !detail.site.routes.is_empty() {
        out.push_str(&format!("\n{:<24}  {}\n", "PATH", "ANSWERED BY"));

        for route in &detail.site.routes {
            out.push_str(&format!(
                "{:<24}  {}\n",
                route.path,
                route_target_word(&route.target)
            ));
        }
    }

    if !detail.services.is_empty() {
        out.push_str(&format!("\n{:<24}  {}\n", "SERVICE", "STATE"));

        for link in &detail.services {
            out.push_str(&format!(
                "{:<24}  {}\n",
                link.service.as_str(),
                link.state.as_str()
            ));
        }
    }

    out
}

/// `mix site delete` — what was freed, and what was not touched.
pub(crate) fn site_removal(removal: &SiteRemoval) -> String {
    let mut out = format!("{} is no longer declared\n", removal.removed.domain);

    out.push_str(&format!(
        "  the files are kept:    {}\n",
        removal.doc_root_kept
    ));
    out.push_str(&format!(
        "  free for another site: {}\n",
        removal.domains_released.join(", ")
    ));

    out
}

/// When a service is stopped for being unused, and what is holding it open right now.
///
/// **Four answers, and never fewer.** A service that stays running does so for one of four reasons
/// that look identical from outside — nothing idles it, somebody switched idling off for it,
/// something running depends on it, or a project is being kept warm. Two of those are settings and
/// two are not, so a rendering that showed only the policy would send half of the people who read it
/// to change something that was never the cause. This is `mix domain status`' rule from T46, applied
/// to a smaller question.
pub(crate) fn service_idle(report: &IdleReport) -> String {
    let mut rendered = format!("{}\n", report.service);

    let policy = match &report.policy {
        Some(policy) => format!("after {}", policy.after),
        None => "never".to_owned(),
    };

    let source = match report.source {
        IdleSource::Row => "set for this service",
        IdleSource::Never => "switched off for this service",
        IdleSource::Recipe => "the default for this kind of service",
        // The state of every service in this build, and it is worth spelling out rather than
        // leaving as a blank: nothing is wrong, the feature simply has no default yet.
        IdleSource::Unset => "no default yet; nothing idles this",
        // Asked for, and nothing to measure it with. The line above still says "never", which is
        // what happens; this says why, which is what a person can act on.
        IdleSource::Unmeasurable => "asked for, but this service has nothing to measure",
        // A newer daemon distinguishing something this build does not. The policy line above is
        // still true, so what is lost is the provenance and not the answer.
        _ => "for a reason this version of `mix` does not know",
    };

    rendered.push_str(&idle_line("idle stop", &policy, source));

    if let Some(policy) = &report.policy {
        rendered.push_str(&idle_line("measured by", &probe(&policy.probe), ""));
    }

    for exemption in &report.exempt {
        let held = match exemption {
            IdleExemption::DependentRunning { service } => {
                format!("{service} is running and depends on it")
            }
            IdleExemption::ProjectKeptWarm { project } => {
                format!("the project {project} is being kept warm")
            }
            // A newer daemon knows a reason this build does not. Named as one rather than dropped:
            // what the reader needs is that *something* holds it open, and a blank line would say
            // the opposite.
            _ => "something this version of `mix` does not know about".to_owned(),
        };

        rendered.push_str(&idle_line("held open by", &held, ""));
    }

    rendered
}

/// How a probe is described to somebody who did not choose it.
///
/// A probe comes from the recipe rather than from the person reading this, so it is written as what
/// is being watched and not as the variant's name.
fn probe(probe: &IdleProbe) -> String {
    match probe {
        IdleProbe::Connections { port } => format!("connections to port {port}"),
        IdleProbe::HttpCounter { url, field } => format!("`{field}` at {url}"),
        IdleProbe::FastCgiStatus { socket, path } => {
            format!("`{path}` at {}", socket.display())
        }
        _ => "something this version of `mix` does not know about".to_owned(),
    }
}

/// One line of [`service_idle`], laid out as [`limit_line`] lays its own out.
fn idle_line(field: &str, value: &str, note: &str) -> String {
    format!("  {field:<13} {value:<24} {note}\n")
        .trim_end()
        .to_owned()
        + "\n"
}

/// What a service may take, and what this machine will actually do about each of it.
///
/// **The number and the verdict on one line, always.** A ceiling of 512 MB means one thing where it
/// is a commit charge enforced by a failed allocation and another where it is charged pages enforced
/// by the OOM killer — and a third where it is stored and enforced by nothing at all. Printing the
/// number alone would be telling a third of the truth.
///
/// **And every field, always, including the ones that are unset.** This is where `service.set_limits`
/// pays for taking the whole value rather than a patch: `mix service limits web set --cpu 50` clears
/// a memory ceiling that was there, and the only thing that keeps that from being a surprise is that
/// the cleared field is on the screen a line below the one that was set.
pub(crate) fn service_limits(report: &ServiceLimitsReport) -> String {
    let mut rendered = format!(
        "{}
",
        report.service
    );

    rendered.push_str(&limit_line(
        "cpu",
        &report.limits.cpu_percent.map_or_else(
            || "uncapped".to_owned(),
            |percent| format!("{percent}% of one core"),
        ),
        &enforcement(
            &report.support.cpu,
            report.support.memory_measure,
            false,
            report.limits.cpu_percent.is_some(),
        ),
    ));

    rendered.push_str(&limit_line(
        "memory",
        &report
            .limits
            .memory_mb
            .map_or_else(|| "uncapped".to_owned(), |mb| format!("{mb} MB")),
        &enforcement(
            &report.support.memory,
            report.support.memory_measure,
            true,
            report.limits.memory_mb.is_some(),
        ),
    ));

    rendered.push_str(&watchdog_line(report.watchdog));

    rendered.push_str(&limit_line(
        "priority",
        match report.limits.priority {
            Priority::Normal => "normal",
            Priority::Background => "background",
        },
        match report.support.priority {
            true => "enforced",
            false => "not enforced here",
        },
    ));

    rendered.push_str(&format!(
        "
cpu is a percentage of one core; this machine has {} of them
",
        report.support.cores
    ));

    rendered
}

/// The line about what is watching a ceiling this machine cannot hold — task **T71a**.
///
/// **Both numbers and the ending, or nothing at all.** A client that printed only the restart would
/// say nothing about the services most worth saying something about: a database over its ceiling is
/// warned about and deliberately left alone, and a person who saw no line would think nothing was
/// watching. Empty for [`None`], which is a machine that enforces the ceiling itself or a service
/// that declared none — in both cases there is no loop to describe.
fn watchdog_line(watchdog: Option<MemoryWatchdog>) -> String {
    let Some(watchdog) = watchdog else {
        return String::new();
    };

    let minutes = watchdog.after_minutes;

    let ending = if watchdog.restarts {
        format!("restarted after {minutes} minutes over it")
    } else {
        "warned about; this service is not restarted automatically".to_owned()
    };

    format!("  {:<9} {:<18} {ending}\n", "watchdog", "checked a minute")
}

// One field: what was asked for, and what happens to it here.
fn limit_line(field: &str, asked: &str, verdict: &str) -> String {
    format!("  {field:<9} {asked:<18} {verdict}\n")
}

/// What this machine does with one field, in words rather than in an enum's name.
///
/// **The tense depends on whether a ceiling is actually set**, and getting that wrong is a real way
/// to mislead: a field nobody has capped that reads *"enforced — at the ceiling, the service is
/// killed"* names a ceiling that does not exist. So an uncapped field is written conditionally, which
/// also makes this line useful *before* somebody sets one — it is where they find out what the
/// number would mean here.
///
/// `measured` is only true for the memory line: it is what the *number* counts, and a CPU percentage
/// counts the same thing everywhere.
fn enforcement(
    enforcement: &Enforcement,
    measure: MemoryMeasure,
    measured: bool,
    capped: bool,
) -> String {
    match enforcement {
        Enforcement::Hard { when } => {
            let ending = match when {
                WhenExceeded::AllocationFails => "the next allocation fails",
                WhenExceeded::Killed => "the service is killed",
            };

            let counts = counted(measure, measured);

            match capped {
                true => format!("enforced:{counts} at the ceiling, {ending}"),
                false => format!("would be enforced:{counts} at a ceiling, {ending}"),
            }
        }

        // The permanent fact: this operating system has no such mechanism, and none is coming.
        Enforcement::Unsupported => match capped {
            true => "stored, not enforced; this system has no such limit".to_owned(),
            false => "this system has no such limit".to_owned(),
        },

        // The fixable one, in the platform's own words, because they were written for this line.
        Enforcement::Unavailable { why } => match capped {
            true => format!("stored, not enforced: {why}"),
            false => format!("could not be enforced: {why}"),
        },

        // **Watched rather than capped** — roadmap task T71a. Deliberately not the word "enforced":
        // the service may go over this number and keep running. What happens after it does is per
        // service rather than per machine, so it is the `watchdog` line below this one and not this
        // sentence. The `why` is carried where the platform gave one, which is a machine somebody
        // could start differently; macOS gives none and none is printed.
        Enforcement::Advisory { why } => {
            let counts = counted(measure, measured);

            let opening = match capped {
                true => format!("watched, not capped:{counts}"),
                false => format!("would be watched, not capped:{counts}"),
            };

            match why {
                Some(why) => format!("{opening} {why}"),
                None => format!("{opening} this system has no hard cap to give"),
            }
        }

        // A variant this build of the client has never heard of. The rest of the line is still true,
        // and saying so beats printing a word that was invented after this binary was compiled.
        _ => "this client does not know what this machine does with it".to_owned(),
    }
}

/// What the memory number counts here, as a clause to drop into a longer sentence.
///
/// Empty for every field but memory: a CPU percentage counts the same thing everywhere, and the
/// clause would be noise on the line that carries it.
fn counted(measure: MemoryMeasure, measured: bool) -> String {
    if !measured {
        return String::new();
    }

    match measure {
        MemoryMeasure::Commit => " counts committed memory;".to_owned(),
        MemoryMeasure::ChargedPages => " counts resident memory and page cache;".to_owned(),

        // Named as an overestimate on the line itself, because it is one: shared pages are counted
        // once per process, so a pool and its workers add up to more than they occupy.
        MemoryMeasure::Resident => {
            " counts resident memory, shared pages once per process;".to_owned()
        }

        _ => String::new(),
    }
}

/// What `mix site share` prints: where the site is, and a code a phone can point at.
///
/// **The QR is drawn here and the URL is answered by the daemon** - the T74 design, D10. A terminal
/// is one client's rendering of one string; a graphical client draws its own from the same string,
/// and the daemon knows about neither.
pub(crate) fn site_shared(sharing: &SiteSharing) -> String {
    let mut out = format!(
        "shared on the local network\n  url        {}\n  interface  {} ({})\n",
        sharing.url, sharing.interface, sharing.address
    );

    // **The name is printed, and whether anything answers for it is printed beside it** - roadmap
    // task T75. A name that resolves nowhere looks exactly like one that does until somebody types
    // it into a phone, so a home that could not bind UDP 5353 says so here rather than there.
    if let Some(name) = sharing.name.as_deref() {
        match sharing.advertised {
            true => out.push_str(&format!("  name       {name}\n")),
            false => out.push_str(&format!(
                "  name       {name} (not being advertised: this home could not answer mDNS)\n"
            )),
        }
    }

    out.push_str(&format!("  authority  {}\n", sharing.ca_url));

    // **Printed only when there is one** — roadmap task T76. A share with no deadline is the
    // ordinary case, and a line reading "ends  never" is a line every reader has to skip.
    if let Some(until) = sharing.until {
        out.push_str(&format!(
            "  ends       {}\n",
            in_time(until, SystemTime::now())
        ));
    }

    // **The QR carries the address and not the name** - the T75 design, D11. Android's resolver
    // does not answer `.local` for a browser, so a code carrying the name would be a broken URL for
    // a large share of the phones this feature exists for.
    if let Some(code) = qr(&sharing.url) {
        out.push('\n');
        out.push_str(&code);
    }

    out.push_str(
        "\nover http: a phone does not trust this home's certificate authority until it has \
         installed it. Open the authority URL on the device, then, on iOS, turn it on under \
         Settings > General > About > Certificate Trust Settings\n",
    );

    out
}

/// The URL as a QR code, in half-height blocks, or [`None`] where it will not encode.
///
/// **Never an error.** An address and a port is thirty characters at most, so nothing a home
/// produces comes close to the limit - but a code that would not fit is still no reason to fail a
/// share that already worked. The URL above it is the answer; the code is the convenience.
fn qr(url: &str) -> Option<String> {
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;

    Some(
        code.render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .build(),
    )
}

/// `mix database create` — what was made, and where the credential is.
///
/// **It says where the password lives and never what it is** — the T77a design, D11. Two lines,
/// aligned on the noun, in the words this file uses everywhere else rather than a glyph.
pub(crate) fn database_created(
    account: &DatabaseAccount,
    store: Option<mixengine_proto::CredentialStore>,
) -> String {
    let word = |made: Made| match made {
        Made::Created => "created",
        Made::Existing => "already existed",
    };

    format!(
        "database {} {} on {}\naccount  {} {}, password in {}",
        account.database,
        word(account.made.database),
        account.service,
        account.user,
        word(account.made.user),
        where_kept(&account.secret, store),
    )
}

/// `mix database credentials` — the address, and the password itself, last and alone.
///
/// **The last line is the value and nothing else** — roadmap task **T77b**'s D3 — so that
/// `mix database credentials mariadb@main --user blog | tail -1` is the password, for a script
/// writing a project's `.env`.
pub(crate) fn database_credentials(
    answer: &DatabaseCredentials,
    store: Option<mixengine_proto::CredentialStore>,
) -> String {
    // The value at the start of its own line, not indented under the two above: `tail -1` takes
    // the whole line, and a client handed `  value` is handed a different password.
    format!(
        "password for {} on {}\n  stored in {}\n{}",
        answer.user,
        answer.service,
        where_kept(&answer.secret, store),
        answer.password,
    )
}

/// `mix daemon credential-store` — what was recorded, and when it applies (T194).
pub(crate) fn credential_store_change(change: mixengine_proto::CredentialStoreChange) -> String {
    let word = |store| match store {
        mixengine_proto::CredentialStore::Os => "the system's credential store",
        mixengine_proto::CredentialStore::Home => "a file in this home",
    };

    if change.recorded == change.running {
        format!(
            "this home keeps its passwords in {}\n",
            word(change.recorded)
        )
    } else {
        format!(
            "this home keeps its passwords in {} from the next start; run `mix daemon stop`, and \
             the next `mix` command starts it there\n",
            word(change.recorded)
        )
    }
}

/// Where a credential is, in words — roadmap task T194, D5. A home that keeps its passwords in a
/// file says so; any other answer, an older daemon's included, names the store's namespace as it
/// always has.
fn where_kept(
    secret: &mixengine_proto::SecretAddress,
    store: Option<mixengine_proto::CredentialStore>,
) -> String {
    match store {
        Some(mixengine_proto::CredentialStore::Home) => {
            format!("this home's credentials file at {}", secret.key)
        }
        _ => format!("the {} credentials at {}", secret.service, secret.key),
    }
}

/// `mix database client`, for a person — roadmap task **T83**.
///
/// Two lines: what the service speaks, and where it could be opened. A service no client opens is
/// said in those words rather than left as a blank, since a blank reads as "nothing installed".
pub(crate) fn database_client(report: &DatabaseClientReport) -> String {
    let protocol = match report.protocol {
        Some(protocol) => format!("{protocol} protocol"),
        None => "not a database a desktop client opens".to_owned(),
    };

    let mut out = format!(
        "{}  {protocol}\n{}",
        report.service,
        desktop_client(&report.client)
    );

    // **Where the credential is, without opening anything** — roadmap task **T84**, the design's
    // D6. An address is a name, and printing it is what stops the next question being *"and where
    // would I find the password?"*.
    if let Some(at) = &report.secret {
        out.push_str(&format!(
            "  its administrator's password is in the {} credentials at {}\n",
            at.service, at.key
        ));
    }

    out
}

/// `mix database open`, for a person — roadmap task **T83**.
///
/// **The password is never printed**, and neither is anything that looks like one: what is printed
/// is where it was read from and that it went to one process. A client that did not open prints
/// its state in the same words `client` uses, so the two commands agree.
pub(crate) fn database_opened(handoff: &DatabaseHandoff) -> String {
    match (&handoff.client, handoff.launched) {
        (DesktopClient::Installed { name, .. }, Some(launched)) => {
            let account = match &handoff.user {
                Some(user) => format!(" as {user}"),
                None => String::new(),
            };
            let how = match launched {
                Launch::Running { pid } => format!("pid {pid}"),
                Launch::HandedOn => "handed to the copy already running".to_owned(),
            };
            let secret = match &handoff.secret {
                Some(at) => format!(
                    "  password read from the {} credentials at {} and handed to that process \
                     alone\n",
                    at.service, at.key
                ),
                None => String::new(),
            };

            format!(
                "opened {} in {name}{account} ({how})\n{secret}",
                handoff.service
            )
        }
        (client, _) => desktop_client(client),
    }
}

/// The client's state, in the words both commands print.
fn desktop_client(client: &DesktopClient) -> String {
    match client {
        DesktopClient::Installed { name, program } => {
            format!("  {name} installed at {program}\n")
        }
        DesktopClient::NoClient => "  this install has no MixLab window to open a database in\n  \
                                    MixLab comes with MixEngine's installers; the headless \
                                    archive has none\n"
            .to_owned(),
    }
}

/// `mix blueprint capture` — what was written down, and where to read it.
pub(crate) fn blueprint_captured(summary: &BlueprintSummary) -> String {
    format!(
        "captured {} from this project
  {}
",
        summary.slug, summary.file
    )
}

/// `mix blueprint import` — what was taken in, and whether anything vouched for it.
///
/// **The trust is said on the way in**, roadmap task **T78a**: it is decided here once and never
/// again, so this line is the only moment a person is told what they now have.
///
/// **And which kind of untrusted it is** — roadmap task **T79b**. A file that came with nothing and
/// a file whose signature did not verify are both untrusted and are not the same event: only the
/// second is what the gallery key exists to catch, and it used to arrive here as the first one's
/// sentence.
pub(crate) fn blueprint_imported(summary: &BlueprintSummary) -> String {
    let vouched = match (summary.trusted, summary.signature) {
        (true, _) => "signed by the gallery key",

        // True of all three things the verifier folds together — a manifest edited after it was
        // signed, a signature from another key, and a file that is not a signature at all. Saying
        // "the bytes changed" would accuse the second and third of the first.
        (false, Some(SignatureCheck::Rejected)) => {
            "untrusted: a signature came with it, and it is not the gallery's"
        }

        (false, Some(SignatureCheck::Missing)) => {
            "untrusted: nothing came with it to vouch for it, and nothing will"
        }

        // A row written before T79b, or one whose reason this build cannot read: the sentence this
        // line has always had, which says the true half of what is known.
        (false, _) => "untrusted: nothing vouches for it, and nothing will",
    };

    format!(
        "imported {}: {vouched}
  {}
",
        summary.slug, summary.file
    )
}

/// `mix extension inspect` — what a manifest declares, and what installing it here would produce.
///
/// **Three things a person reads off this**, in this order: what it is, what it would run, and what
/// it asked for. The last is the one a line could mislead about, so it says *asked for* — a port
/// here is a wish, and allocation is not something T80 does at all.
///
/// The permission lines say which of them are boundaries. `network` and `filesystem` are enforced
/// by the manifest format itself; `services` is a declaration, and reads as one, because an
/// extension runs as this account and could ignore any token it was handed (ADR 0014).
pub(crate) fn extension_inspection(inspection: &ExtensionInspection) -> String {
    let mut out = format!(
        "{} {}: {}\n  {}\n",
        inspection.id,
        inspection.version,
        inspection.name,
        match inspection.kind {
            ExtensionKind::Service => "a program MixEngine would supervise",
            ExtensionKind::WebApp => "source MixEngine would serve on an internal domain",
            ExtensionKind::Recipe => "configuration MixEngine would merge into what it generates",
        }
    );

    if !inspection.description.is_empty() {
        out.push_str(&format!("  {}\n", inspection.description));
    }
    if let Some(homepage) = &inspection.homepage {
        out.push_str(&format!("  {homepage}\n"));
    }

    out.push_str(&format!(
        "\nreaches      {}\n",
        match inspection.permissions.network {
            NetworkReach::Loopback => "this machine only, on 127.0.0.1",
            NetworkReach::Lan =>
                "every interface, on 0.0.0.0, reachable from other machines on this network",
        }
    ));

    if !inspection.permissions.filesystem.is_empty() {
        let paths: Vec<&str> = inspection
            .permissions
            .filesystem
            .iter()
            .map(|reach| match reach {
                FilesystemReach::OwnData => "its own installation and data directories",
                FilesystemReach::ProjectRootsRead => {
                    "reading project roots (declared; this build grants nothing for it)"
                }
            })
            .collect();
        out.push_str(&format!("paths        {}\n", paths.join(", ")));
    }

    if !inspection.permissions.services.is_empty() {
        let calls: Vec<&str> = inspection
            .permissions
            .services
            .iter()
            .map(|access| match access {
                ApiAccess::Read => "read",
                ApiAccess::Write => "change",
            })
            .collect();
        out.push_str(&format!(
            "api          says it would {} what MixEngine knows about services; a declaration \
             shown to you, not a permission MixEngine enforces\n",
            calls.join(" and ")
        ));
    }

    out.push_str(&match &inspection.artifact {
        ArtifactAvailability::Published { url, .. } => format!("artifact     {url}\n"),
        ArtifactAvailability::OtherTargets { targets } => format!(
            "artifact     none for this machine; published for {}\n",
            targets.join(", ")
        ),
        ArtifactAvailability::NotRequired => "artifact     none; it downloads nothing\n".to_owned(),
    });

    out.push_str(&format!(
        "install dir  {}\ndata dir     {}\n",
        inspection.install_dir, inspection.data_dir
    ));

    if let Some(spec) = &inspection.runs {
        out.push_str(&format!(
            "\nit would run\n  program  {}\n  cwd      {}\n",
            spec.program().display(),
            spec.cwd().display()
        ));
        if !spec.args().is_empty() {
            out.push_str(&format!("  args     {}\n", spec.args().join(" ")));
        }
    }

    if let Some(site) = &inspection.serves {
        out.push_str(&format!(
            "\nit would serve\n  root     {}\n  domain   {}\n  runtime  {} {}\n",
            site.root, site.domain, site.runtime, site.requires
        ));
    }

    if !inspection.ports.is_empty() {
        out.push_str("\nports asked for, and not held; allocation happens at install\n");
        for port in &inspection.ports {
            out.push_str(&format!("  {:<10} {}\n", port.name, port.wanted));
        }
    }

    if !inspection.extends.is_empty() {
        out.push_str("\nit would also add\n");
        for addition in &inspection.extends {
            out.push_str(&match addition {
                RecipeAddition::PhpIni { key, value } => format!("  php.ini  {key} = {value}\n"),
                // **The server is named rather than folded away** — roadmap task T81c. A home
                // running the other front end renders nothing for this entry, and a reader who
                // cannot see which one it is for cannot tell that from a line that took effect.
                RecipeAddition::FrontEnd { server, fragment } => {
                    format!("  frontend ({}) {fragment}\n", server.package())
                }
            });
        }
    }

    out
}

/// `mix blueprint list` — every blueprint this home holds.
pub(crate) fn blueprint_list(list: &BlueprintList) -> String {
    if list.blueprints.is_empty() {
        return "no blueprints have been captured; `mix blueprint capture --name <name>` writes one
"
        .to_owned();
    }

    let mut out = format!(
        "{:<24}  {:<9}  {:<12}  {}
",
        "BLUEPRINT", "SOURCE", "TRUST", "DESCRIPTION"
    );

    for blueprint in &list.blueprints {
        out.push_str(&format!(
            "{:<24}  {:<9}  {:<12}  {}
",
            blueprint.slug,
            blueprint.source.as_str(),
            // A word rather than a colour, because `--json` carries the same fact and a listing
            // that only said it in ANSI would say it to nobody in a pipe.
            //
            // Three words rather than two — roadmap task **T79b**. `mismatched` is the one worth
            // scanning a table for: somebody signed that file, and this is not what they signed.
            match (blueprint.trusted, blueprint.signature) {
                (true, _) => "signed",
                (false, Some(SignatureCheck::Missing)) => "unsigned",
                (false, Some(SignatureCheck::Rejected)) => "mismatched",
                (false, _) => "untrusted",
            },
            match blueprint.description.is_empty() {
                true => "—",
                false => blueprint.description.as_str(),
            }
        ));
    }

    out
}

/// `mix blueprint apply --dry-run` — every action, in the order it would happen.
///
/// **Words rather than a column of glyphs.** Nothing else in this file marks a line with `✓` or
/// `✗`, and a non-ASCII status column is one more thing to be wrong on a Windows console — while
/// the words are the vocabulary [`Disposition`] already has.
///
/// The elevation sentence is gathered to the end and said **once** (the T77 design, D11): what a
/// person needs to know before they start is that they will be asked for a password, not which of
/// six lines asks for it.
pub(crate) fn blueprint_plan(plan: &BlueprintPlan) -> String {
    let mut out = format!(
        "Plan: {} into project {} at {}

",
        plan.blueprint, plan.project, plan.root
    );

    for step in &plan.steps {
        out.push_str(&format!(
            "  {:<11} {}
",
            disposition_word(&step.disposition),
            step_said(step)
        ));
    }

    if plan.steps.iter().any(|step| step.elevates) {
        out.push_str(
            "
applying this asks for elevation once, to write the hosts file
",
        );
    }

    out
}

/// `mix blueprint apply` — what the apply did, step by step.
///
/// **Every step, including the ones that needed nothing**: a second apply whose every line says
/// *already true* is what tells a person the first one finished, and a rendering that hid them would
/// hide exactly that.
///
/// What did **not** run is gathered to the end and said in full, because a `[scaffold]` command is
/// the one line somebody has to act on themselves.
pub(crate) fn blueprint_applied(applied: &BlueprintApplied) -> String {
    let mut out = format!(
        "Applied {} as {} at {}\n\n",
        applied.blueprint, applied.project, applied.root
    );

    for step in &applied.steps {
        out.push_str(&format!(
            "  {:<11} {}\n",
            match &step.result {
                StepResult::Done { .. } => "done",
                StepResult::AlreadyTrue => "already",
                StepResult::NotRun { .. } => "not run",
                // **A step that ran and did not succeed** — roadmap task **T78a**. Told apart from
                // one nothing attempted, because what a person does next differs: a failure is
                // theirs to read, and a skip is theirs to decide about.
                StepResult::Failed { .. } => "failed",
                _ => "unknown",
            },
            action_said(&step.action)
        ));

        // **T202, D2.** What differed from the plan, under the step it belongs to: fourteen
        // spaces, which is the two of the indent plus the eleven of the status column plus its
        // trailing space, so the sentence lines up with the action it is about.
        if let StepResult::Done { note: Some(note) } = &step.result {
            out.push_str(&format!("              {note}\n"));
        }
    }

    for step in &applied.steps {
        match &step.result {
            StepResult::NotRun { why } | StepResult::Failed { why } => {
                out.push_str(&format!("\n{why}\n"));
            }

            _ => {}
        }
    }

    out
}

/// Whether any step of an apply ran and failed — roadmap task **T78a**.
///
/// **What the exit status is read off.** The job succeeded: the apply did everything it was asked
/// and the report is complete, and the command's own exit code is the command's news (the T78a
/// design, D7). A shell still has to hear it, and this is where it does.
pub(crate) fn blueprint_had_a_failed_step(applied: &BlueprintApplied) -> bool {
    applied
        .steps
        .iter()
        .any(|step| matches!(step.result, StepResult::Failed { .. }))
}

/// The one word a disposition is printed as.
fn disposition_word(disposition: &Disposition) -> &'static str {
    match disposition {
        Disposition::Satisfied => "installed",
        Disposition::Create => "create",
        Disposition::Choice { .. } => "asks",
        Disposition::Confirm { .. } => "confirm",
        Disposition::Blocked { .. } => "blocked",
        Disposition::Unsupported { .. } => "unsupported",
        // A disposition a later build added. Printed as something rather than hidden, because a
        // step nobody can see is a step nobody can decide about.
        _ => "unknown",
    }
}

/// What one step says about itself, including the reason where it has one.
fn step_said(step: &PlanStep) -> String {
    let said = action_said(&step.action);

    match &step.disposition {
        Disposition::Choice { installed, .. } => {
            format!("{said}; {} is installed", installed.as_str())
        }
        Disposition::Blocked { reason } | Disposition::Unsupported { reason } => {
            format!("{said}: {reason}")
        }
        _ => said,
    }
}

/// What one action says about itself, whatever became of it.
///
/// Split from [`step_said`] so that a plan and the report of an apply say the same words about the
/// same action — two renderings would be two vocabularies for one list.
fn action_said(action: &PlanAction) -> String {
    match action {
        PlanAction::RegisterProject { name, root, pins } => {
            // The pins are on this line rather than folded into the runtime steps below, because
            // they are what the *project* will ask for from now on — which is a different fact from
            // what is being installed, and the one a person is really applying a blueprint for.
            let asks = match pins.is_empty() {
                true => String::new(),
                false => format!(
                    ", asking for {}",
                    pins.iter()
                        .map(|(kind, wanted)| format!("{} {}", kind.as_str(), wanted.as_str()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };

            format!("project {name} at {root}{asks}")
        }
        PlanAction::InstallRuntime { kind, wanted } => {
            format!("{} {}", kind.as_str(), wanted.as_str())
        }
        PlanAction::InstallPackage { package, wanted } => match wanted {
            Some(wanted) => format!("{package} {}", wanted.as_str()),
            None => package.clone(),
        },
        PlanAction::EnsureService {
            package,
            instance,
            version,
            dedicated,
        } => format!(
            "{package} {}@{instance}{}",
            version
                .as_ref()
                .map(|version| format!("{} ", version.as_str()))
                .unwrap_or_default(),
            match dedicated {
                true => ", this project's own",
                false => ", reusing the shared instance",
            }
        ),
        PlanAction::CreateDatabase { database, user, .. } => {
            format!("database {database}, user {user}")
        }
        PlanAction::CreateSite {
            kind,
            doc_root,
            https,
            ..
        } => format!(
            "site {} at {}{}",
            site_kind_word(kind),
            match doc_root.is_empty() {
                true => "the project root",
                false => doc_root.as_str(),
            },
            match https {
                true => ", https",
                false => "",
            }
        ),
        PlanAction::AddDomain { domain, primary } => match primary {
            true => format!("domain {domain}"),
            false => format!("domain {domain}, an alias"),
        },
        PlanAction::IssueCertificate { domains } => {
            format!("certificate for {}", domains.join(", "))
        }
        // **The line says how far this reaches.** Extension choices belong to an installed runtime,
        // so this changes the PHP every project on this machine runs — which belongs here, at the
        // moment somebody is deciding, rather than in documentation.
        PlanAction::SetPhpExtension { runtime, name } => match runtime {
            Some(runtime) => format!(
                "php extension {name}: changes PHP {} for every project here",
                runtime.as_str()
            ),
            // Nothing installed answers yet: the runtime step installs the PHP this lands on.
            None => format!(
                "php extension {name} on the PHP this installs, for every project that uses it"
            ),
        },
        PlanAction::RunScaffold { command } => format!("run `{command}`"),
        _ => "something this build cannot describe".to_owned(),
    }
}

/// The word a site kind is printed as in a plan.
fn site_kind_word(kind: &SiteKind) -> &'static str {
    match kind {
        SiteKind::PhpFpm { .. } => "php-fpm",
        SiteKind::Static => "static",
        SiteKind::ReverseProxy { .. } => "reverse-proxy",
        SiteKind::NodeApp { .. } => "node-app",
    }
}

/// `mix extension list` — roadmap task **T81**.
///
/// **The `TRUST` column is T79b's, one table across.** A blueprint says `signed` / `unsigned` /
/// `mismatched`; an extension has two answers, because the registry's signature covers the whole
/// document — an entry either arrived inside something the compiled-in key vouched for, or the
/// document was refused before anything was installed. What is left is `--path`, which nothing
/// vouches for and which stays marked for as long as it is installed.
pub(crate) fn installed_extensions(list: &InstalledExtensions) -> String {
    if list.extensions.is_empty() {
        return "nothing is installed; `mix extension available` lists what could be\n".to_owned();
    }

    let rows: Vec<[String; 8]> = list
        .extensions
        .iter()
        .map(|one| {
            [
                one.id.to_string(),
                one.version.to_string(),
                one.kind.as_str().to_owned(),
                match one.signed {
                    true => "signed".to_owned(),
                    false => "unsigned".to_owned(),
                },
                one.service
                    .as_ref()
                    .map_or_else(|| "—".to_owned(), ToString::to_string),
                // A web-app's site or a service's page: where it opens (T200a, D3).
                one.site
                    .clone()
                    .or_else(|| one.ui.clone())
                    .unwrap_or_else(|| "—".to_owned()),
                match one.ports.is_empty() {
                    true => "—".to_owned(),
                    false => one
                        .ports
                        .iter()
                        .map(|port| format!("{}={}", port.name, port.wanted))
                        .collect::<Vec<_>>()
                        .join(" "),
                },
                // Last, so a long sentence pushes nothing else out of line — T200, D5.
                one.description.clone().unwrap_or_default(),
            ]
        })
        .collect();

    table(
        [
            "ID",
            "VERSION",
            "KIND",
            "TRUST",
            "SERVICE",
            "OPENS AT",
            "PORTS",
            "DESCRIPTION",
        ],
        &rows,
    )
}

/// `mix extension available`.
///
/// **Empty and old are two answers, not one.** A registry that lists nothing — which is what
/// **T81a** publishes until the first manifests land — is a complete answer, and telling that
/// person to update sends them after a listing no version of MixEngine would show them. The
/// sentence about what this build cannot read belongs to the other empty: one where every entry
/// there is an entry this build had to drop.
pub(crate) fn extension_catalogue(catalogue: &ExtensionCatalogue) -> String {
    let mut out = match (catalogue.extensions.is_empty(), catalogue.unreadable) {
        (true, 0) => "the registry lists no extensions yet\n".to_owned(),
        (true, _) => "the registry lists nothing this build can read\n".to_owned(),
        (false, _) => {
            let rows: Vec<[String; 5]> = catalogue
                .extensions
                .iter()
                .map(|one| {
                    [
                        one.id.to_string(),
                        one.version.to_string(),
                        one.kind.as_str().to_owned(),
                        match one.installed {
                            true => "yes".to_owned(),
                            false => match &one.artifact {
                                ArtifactAvailability::OtherTargets { .. } => {
                                    "not for this machine".to_owned()
                                }
                                _ => "no".to_owned(),
                            },
                        },
                        one.description.clone(),
                    ]
                })
                .collect();

            table(["ID", "VERSION", "KIND", "INSTALLED", "DESCRIPTION"], &rows)
        }
    };

    if catalogue.stale {
        out.push_str(
            "\nthis is the last registry MixEngine could verify; the published one could not be \
             reached\n",
        );
    }

    // **Said rather than swallowed** — the T81 design's D4. An extension missing from a listing is
    // one somebody goes looking for in the wrong place.
    if catalogue.unreadable > 0 {
        out.push_str(&format!(
            "\n{} {} this build cannot read; update MixEngine to see {}\n",
            catalogue.unreadable,
            match catalogue.unreadable {
                1 => "entry",
                _ => "entries",
            },
            match catalogue.unreadable {
                1 => "it",
                _ => "them",
            }
        ));
    }

    out
}

/// `mix extension plan`, which is also the question `install` asks before it installs anything.
pub(crate) fn extension_plan(plan: &ExtensionPlan) -> String {
    let mut out = format!(
        "{} {}: {}\n  {}\n",
        plan.id,
        plan.version,
        plan.name,
        match plan.kind {
            ExtensionKind::Service => "a program MixEngine would supervise",
            ExtensionKind::WebApp => "source MixEngine would serve on an internal domain",
            ExtensionKind::Recipe => "configuration MixEngine would merge into what it generates",
        }
    );

    if !plan.description.is_empty() {
        out.push_str(&format!("  {}\n", plan.description));
    }
    if let Some(homepage) = &plan.homepage {
        out.push_str(&format!("  {homepage}\n"));
    }

    out.push_str(&match plan.signed {
        true => "\nsigned       by the key this build trusts\n".to_owned(),
        false => {
            "\nUNSIGNED     nothing vouches for this: it was read from a directory\n".to_owned()
        }
    });

    out.push_str(&format!(
        "reaches      {}\n",
        match plan.permissions.network {
            NetworkReach::Loopback => "this machine only, on 127.0.0.1",
            NetworkReach::Lan =>
                "every interface, on 0.0.0.0, reachable from other machines on this network",
        }
    ));

    if !plan.permissions.filesystem.is_empty() {
        let paths: Vec<&str> = plan
            .permissions
            .filesystem
            .iter()
            .map(|reach| match reach {
                FilesystemReach::OwnData => "its own installation and data directories",
                FilesystemReach::ProjectRootsRead => {
                    "reading project roots (declared; this build grants nothing for it)"
                }
            })
            .collect();
        out.push_str(&format!("paths        {}\n", paths.join(", ")));
    }

    if !plan.permissions.services.is_empty() {
        let calls: Vec<&str> = plan
            .permissions
            .services
            .iter()
            .map(|access| match access {
                ApiAccess::Read => "read",
                ApiAccess::Write => "change",
            })
            .collect();
        out.push_str(&format!(
            "api          says it would {} what MixEngine knows about services; a declaration \
             shown to you, not a permission MixEngine enforces\n",
            calls.join(" and ")
        ));
    }

    if !plan.ports.is_empty() {
        out.push_str(&format!(
            "ports        {}\n",
            plan.ports
                .iter()
                .map(|port| format!("{} (wants {})", port.name, port.wanted))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if let Some(site) = &plan.site {
        out.push_str(&format!(
            "site         https://{}, on {}\n",
            site.domain, site.pool
        ));

        // **Which server this would open onto, before anybody agrees to it** — roadmap task **T82**.
        // An administrative interface onto a database is a thing to be shown the database.
        if let Some(database) = &site.database {
            out.push_str(&format!("database     {database}\n"));
        }

        // **And what it is really being granted** — roadmap task **T82a**, its design's D2. Handing
        // an application a database superuser's password is the most consequential thing an
        // extension can be given, so it is said in full: which account, where the password comes
        // from, and that nothing writes it down.
        if let Some(user) = &site.signs_in {
            out.push_str(&format!(
                "signs in     as {user}, in a php-fpm pool of its own; that pool reads the \
                 password from this machine's keyring when it starts, and nothing writes it to \
                 disk\n"
            ));
        }
    }

    out.push_str(&format!(
        "install dir  {}\ndata dir     {}\n",
        plan.install_dir, plan.data_dir
    ));

    out
}

/// `mix extension uninstall`.
pub(crate) fn extension_removal(removal: &ExtensionRemoval) -> String {
    let mut out = format!("{} was uninstalled\n", removal.id);

    if let Some(service) = &removal.service {
        out.push_str(&format!("  its service {service} went with it\n"));
    }

    // The pool a `web-app` was served on — roadmap task **T82a**. Named for the same reason the
    // service above is: a process that went is a thing to say, not a thing to leave out.
    if let Some(pool) = &removal.pool {
        out.push_str(&format!("  its pool {pool} went with it\n"));
    }

    if let Some(site) = &removal.site {
        out.push_str(&format!("  released {site}\n"));
    }

    match &removal.data_dir_kept {
        Some(path) => out.push_str(&format!(
            "  its data was kept at {path}\n  `mix extension uninstall {} --delete-data` removes \
             that too\n",
            removal.id
        )),
        None => out.push_str("  its data directory was deleted\n"),
    }

    out
}

/// Which of the three `mix autostart` commands is being rendered.
///
/// The report they answer with is one type, and what differs is the first line: "this is how things
/// stand" and "this is what just happened" are read differently even when the words after them are
/// identical. [`Pathed`]'s reasoning, and beside it for the family resemblance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Autostarted {
    /// `mix autostart status`.
    Asked,
    /// `mix autostart enable`.
    Enabled,
    /// `mix autostart disable`.
    Disabled,
}

/// `mix autostart …`, for a person — roadmap task **T85b**.
///
/// **An entry that is registered for another home is never reported as "set up".** There is one
/// entry per user, so a second home enabling replaces it, and somebody reading "this home starts at
/// login" while the entry names a directory they deleted last week would have no way to tell. The
/// daemon decides which case it is; this only says it.
pub(crate) fn autostart_report(autostarted: Autostarted, report: &AutostartReport) -> String {
    let mut rendered = match (autostarted, report.enabled, report.for_this_home) {
        (_, true, false) => "an autostart entry is registered, but for another home
"
        .to_owned(),

        (Autostarted::Asked, true, true) => "this home's daemon starts when you log in
"
        .to_owned(),
        (Autostarted::Asked, false, _) => "this home's daemon does not start when you log in
"
        .to_owned(),

        (Autostarted::Enabled, _, _) => match report.changed {
            true => "this home's daemon will now start when you log in
"
            .to_owned(),
            false => "this home's daemon already started when you log in
"
            .to_owned(),
        },

        (Autostarted::Disabled, _, _) => match report.changed {
            true => "this home's daemon no longer starts when you log in
"
            .to_owned(),
            false => "this home's daemon did not start when you log in
"
            .to_owned(),
        },
    };

    rendered.push_str(&format!(
        "  {:<9} {}
",
        mechanism(report.mechanism),
        report.location
    ));

    if !report.command.is_empty() {
        rendered.push_str(&format!(
            "  {:<9} {}
",
            "starts",
            report.command.join(" ")
        ));
    }

    if report.mechanism == AutostartMechanism::None {
        rendered.push_str(
            "  this machine has no way to start something at login that MixEngine will write, so \
             there is nothing to register
",
        );
    }

    if autostarted == Autostarted::Enabled && report.enabled && report.for_this_home {
        rendered.push_str(
            "it takes effect at your next login
",
        );
    }

    rendered
}

/// What this machine starts things with, as a person would name it.
fn mechanism(mechanism: AutostartMechanism) -> &'static str {
    match mechanism {
        AutostartMechanism::LogonTask => "task",
        AutostartMechanism::LaunchAgent => "agent",
        AutostartMechanism::SystemdUser => "unit",
        AutostartMechanism::None => "nowhere",
    }
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{
        CategoryUsage, Cleaned, DiskCategory, MetricsMinute, MetricsSample, MetricsSubject, Need,
        PortWish, RedistributableArch, Remedy, RuntimeKind, SecretAddress, ServiceState,
        StepOutcome, Timestamp, VersionConstraint,
    };

    use super::*;

    /// **T200, D5.** The listing says what each extension is for, last, where a long sentence
    /// pushes nothing else out of line.
    #[test]
    fn an_installed_extension_is_listed_with_what_it_is_for() {
        let list = InstalledExtensions {
            extensions: vec![mixengine_proto::ExtensionSummary {
                id: mixengine_proto::ExtensionId::parse("mailpit").expect("an id"),
                name: "Mailpit".to_owned(),
                version: PackageVersion::parse("1.31.0".to_owned()).expect("a version"),
                kind: ExtensionKind::Service,
                signed: true,
                service: None,
                ports: Vec::new(),
                site: None,
                description: Some("Local SMTP capture and web UI".to_owned()),
                ui: Some("http://127.0.0.1:8025/".to_owned()),
            }],
        };

        let rendered = installed_extensions(&list);

        assert!(rendered.contains("DESCRIPTION"), "{rendered}");
        // **T200a, D3.** A service's page sits where a web-app's site does.
        assert!(rendered.contains("OPENS AT"), "{rendered}");
        assert!(rendered.contains("http://127.0.0.1:8025/"), "{rendered}");
        assert!(!rendered.contains(" SITE "), "{rendered}");
        assert!(
            rendered.contains("Local SMTP capture and web UI"),
            "{rendered}"
        );
    }

    /// One offered release, at whatever execution the daemon reported — roadmap task **T92**.
    fn offered(version: &str, execution: Option<Execution>) -> RuntimeRelease {
        RuntimeRelease {
            kind: RuntimeKind::Php,
            version: PackageVersion::parse(version.to_owned()).expect("a version"),
            channel: mixengine_proto::PackageChannel::Stable,
            eol: None,
            bytes: 34_718_139,
            installed: false,
            execution,
            needs: None,
            line: None,
            newest_in_line: None,
        }
    }

    fn in_line(release: RuntimeRelease, line: &str, newest: bool) -> RuntimeRelease {
        RuntimeRelease {
            line: Some(line.to_owned()),
            newest_in_line: Some(newest),
            ..release
        }
    }

    fn by_line() -> RuntimeCatalogue {
        RuntimeCatalogue {
            runtimes: vec![
                in_line(offered("8.4.25", None), "8.4", true),
                in_line(offered("8.4.24", None), "8.4", false),
                in_line(offered("8.4.23", None), "8.4", false),
                in_line(offered("8.3.30", None), "8.3", true),
            ],
            stale: false,
            unavailable: None,
            updates: Some(vec![mixengine_proto::RuntimeUpdate {
                kind: RuntimeKind::Php,
                from: PackageVersion::parse("8.4.24").expect("a version"),
                to: PackageVersion::parse("8.4.25").expect("a version"),
                to_installed: false,
                needs: Some(Vec::new()),
            }]),
        }
    }

    const EVERY_LINE: Lines = Lines {
        all: false,
        line: None,
    };

    /// **T193a, D3.** One row stands for each line, with a count of the rest.
    #[test]
    fn one_row_stands_for_each_line_with_a_count_of_the_rest() {
        let rendered = runtime_catalogue(&by_line(), &EVERY_LINE);

        assert!(rendered.contains("8.4.25"), "{rendered}");
        assert!(rendered.contains("8.3.30"), "{rendered}");
        assert!(
            !rendered.contains("8.4.23"),
            "an older patch is not a row: {rendered}"
        );
        assert!(rendered.contains("MORE"), "{rendered}");
        assert!(
            rendered.contains("+2"),
            "two other 8.4 releases: {rendered}"
        );
    }

    #[test]
    fn all_prints_every_release() {
        let rendered = runtime_catalogue(
            &by_line(),
            &Lines {
                all: true,
                line: None,
            },
        );
        assert!(rendered.contains("8.4.23"), "{rendered}");
        assert!(!rendered.contains("MORE"), "{rendered}");
    }

    #[test]
    fn naming_a_line_prints_every_release_of_it_and_nothing_else() {
        let rendered = runtime_catalogue(
            &by_line(),
            &Lines {
                all: false,
                line: Some("8.4".to_owned()),
            },
        );
        assert!(rendered.contains("8.4.23"), "{rendered}");
        assert!(!rendered.contains("8.3.30"), "{rendered}");
    }

    #[test]
    fn an_update_is_named_above_the_table_with_the_command_that_applies_it() {
        let rendered = runtime_catalogue(&by_line(), &EVERY_LINE);
        assert!(rendered.contains("php 8.4.24 → 8.4.25"), "{rendered}");
        assert!(
            rendered.contains("mix runtime upgrade php 8.4.24"),
            "{rendered}"
        );
    }

    /// **A daemon from before T193 sends no lines**, and its list is printed as it always was
    /// rather than as nothing — the plan's review focus 1.
    #[test]
    fn an_old_daemon_s_catalogue_is_printed_flat() {
        let rendered = runtime_catalogue(
            &RuntimeCatalogue {
                runtimes: vec![offered("8.4.25", None), offered("8.4.24", None)],
                stale: false,
                updates: None,
                unavailable: None,
            },
            &EVERY_LINE,
        );
        assert!(
            rendered.contains("8.4.25") && rendered.contains("8.4.24"),
            "{rendered}"
        );
        assert!(!rendered.contains("MORE"), "{rendered}");
    }

    fn lacking(release: RuntimeRelease, need: Need, remedy: Remedy) -> RuntimeRelease {
        RuntimeRelease {
            needs: Some(vec![Requirement { need, remedy }]),
            ..release
        }
    }

    /// **T196.** A kind the index could not be read for is said above the table, where the
    /// staleness line is said, and a list that read everything says nothing.
    #[test]
    fn a_kind_the_index_could_not_be_read_for_is_said_above_the_table() {
        let mut catalogue = RuntimeCatalogue {
            runtimes: vec![offered("8.3.33", Some(Execution::Native))],
            stale: false,
            updates: None,
            unavailable: Some(vec![mixengine_proto::CatalogueGap {
                name: "node".to_owned(),
                reason: "it does not hash to what the signed root says".to_owned(),
            }]),
        };

        let rendered = runtime_catalogue(&catalogue, &EVERY_LINE);
        let first = rendered.lines().next().expect("a line above the table");
        assert!(
            first.starts_with("node is missing from this list"),
            "{rendered}"
        );
        assert!(first.contains("does not hash"), "{rendered}");
        assert!(
            first.ends_with("`mix runtime available --refresh` tries again"),
            "a line ends with the command that fixes it: {rendered}"
        );

        for silent in [None, Some(Vec::new())] {
            catalogue.unavailable = silent;
            assert!(
                !runtime_catalogue(&catalogue, &EVERY_LINE).contains("missing from this list"),
                "nothing unread, nothing said"
            );
        }
    }

    /// **T151.** The column appears only when a row lacks something, on `RUNS`' reasoning.
    #[test]
    fn a_needs_column_appears_only_when_a_row_lacks_something() {
        let plain = RuntimeCatalogue {
            runtimes: vec![offered("8.3.33", Some(Execution::Native))],
            stale: false,
            updates: None,
            unavailable: None,
        };
        assert!(!runtime_catalogue(&plain, &EVERY_LINE).contains("NEEDS"));

        let lacking_one = RuntimeCatalogue {
            runtimes: vec![
                lacking(
                    offered("8.4.24", Some(Execution::Native)),
                    Need::VisualCpp {
                        year: "2022".to_owned(),
                        arch: RedistributableArch::X64,
                        found: None,
                    },
                    Remedy::InstallVisualCpp {
                        arch: RedistributableArch::X64,
                    },
                ),
                offered("8.3.33", Some(Execution::Native)),
            ],
            stale: false,
            updates: None,
            unavailable: None,
        };
        let rendered = runtime_catalogue(&lacking_one, &EVERY_LINE);
        assert!(rendered.contains("NEEDS"), "{rendered}");
        assert!(rendered.contains("Visual C++ 2022 (x64)"), "{rendered}");
    }

    #[test]
    fn what_is_lacking_is_said_with_what_can_be_done() {
        let rendered = requirements(&[Requirement {
            need: Need::Macos {
                at_least: "14.0".to_owned(),
                found: "13.6".to_owned(),
            },
            remedy: Remedy::ChooseVersion {
                version: PackageVersion::parse("8.3.33").unwrap(),
            },
        }]);

        assert!(
            rendered.contains("macOS 14.0 or newer, and this Mac runs 13.6"),
            "{rendered}"
        );
        assert!(
            rendered.contains("8.3.33 is the newest release that runs here"),
            "{rendered}"
        );
    }

    /// **A warning names the libraries and says the install goes on** — roadmap task **T27e**, D16.
    #[test]
    fn a_warning_names_the_libraries_and_says_the_install_goes_on() {
        let said = advisories(&[Requirement {
            need: Need::SharedLibrary {
                soname: "libasound.so.2".to_owned(),
            },
            remedy: Remedy::InstallFromDistribution,
        }]);

        assert!(said.starts_with("warning: "), "{said}");
        assert!(said.contains("libasound.so.2"), "{said}");
        assert!(said.contains("goes on"), "{said}");
    }

    /// The column exists only where it says something — roadmap task **T92**.
    ///
    /// Five of the six targets MixEngine ships a build for see this rendering, and a column reading
    /// `native` against every row would be noise on all five.
    #[test]
    fn a_catalogue_of_native_releases_has_no_column_about_it() {
        let rendered = runtime_catalogue(
            &RuntimeCatalogue {
                runtimes: vec![offered("8.3.33", Some(Execution::Native))],
                stale: false,
                updates: None,
                unavailable: None,
            },
            &EVERY_LINE,
        );

        assert!(!rendered.contains("RUNS"), "no column: {rendered}");
        assert!(!rendered.contains("emulated"), "and no note: {rendered}");
    }

    /// A daemon from before the member reports nothing, which is not a claim that anything is
    /// emulated — [ADR 0019](../../../docs/decisions/0019-an-added-response-member-is-optional.md).
    #[test]
    fn a_daemon_that_reports_no_execution_brings_no_column_either() {
        let rendered = runtime_catalogue(
            &RuntimeCatalogue {
                runtimes: vec![offered("8.3.33", None)],
                stale: false,
                updates: None,
                unavailable: None,
            },
            &EVERY_LINE,
        );

        assert!(!rendered.contains("RUNS"), "{rendered}");
    }

    #[test]
    fn one_emulated_release_brings_the_column_and_the_sentence_that_explains_it() {
        let rendered = runtime_catalogue(
            &RuntimeCatalogue {
                runtimes: vec![
                    offered("8.3.33", Some(Execution::Emulated)),
                    offered("8.4.24", Some(Execution::Native)),
                ],
                stale: false,
                updates: None,
                unavailable: None,
            },
            &EVERY_LINE,
        );

        assert!(rendered.contains("RUNS"), "the column: {rendered}");
        assert!(rendered.contains("emulated"), "the word: {rendered}");
        assert!(
            rendered.contains("native"),
            "and its opposite, so the column reads: {rendered}"
        );
        assert!(
            rendered
                .lines()
                .next()
                .is_some_and(|line| line.starts_with("emulated;")),
            "the note comes before the table: {rendered}"
        );
    }

    /// **T81b.** A listing says who owns each site, and an extension's site says so in words a
    /// person can act on.
    #[test]
    fn a_site_listing_names_the_owner() {
        let list = SiteList {
            sites: vec![
                mixengine_proto::SiteSummary {
                    domain: "blog.test".to_owned(),
                    owner: SiteOwner::Project {
                        name: "blog".to_owned(),
                    },
                    kind: SiteKind::Static,
                    doc_root: String::new(),
                    https: true,
                    https_redirect: false,
                    state: mixengine_proto::SiteState::Enabled,
                    routes: Vec::new(),
                    sharing: None,
                },
                mixengine_proto::SiteSummary {
                    domain: "phpmyadmin.mixengine.test".to_owned(),
                    owner: SiteOwner::Extension {
                        id: mixengine_proto::ExtensionId::parse("phpmyadmin").expect("an id"),
                    },
                    kind: SiteKind::PhpFpm { pool: None },
                    doc_root: "app".to_owned(),
                    https: true,
                    https_redirect: false,
                    state: mixengine_proto::SiteState::Enabled,
                    routes: Vec::new(),
                    sharing: None,
                },
            ],
        };

        let rendered = site_list(&list);

        assert!(rendered.contains("OWNER"), "{rendered}");
        assert!(rendered.contains("  blog\n"), "{rendered}");
        assert!(rendered.contains("extension phpmyadmin"), "{rendered}");
    }

    /// **T81b.** A plan for a web-app says which name it takes and which pool it runs on — and
    /// since **T82a**, which account it would sign itself in as, because a database superuser's
    /// password is the one thing on this screen somebody must not agree to by accident.
    #[test]
    fn an_extension_plan_names_the_site_it_would_take() {
        let plan = ExtensionPlan {
            id: mixengine_proto::ExtensionId::parse("phpmyadmin").expect("an id"),
            name: "phpMyAdmin".to_owned(),
            version: PackageVersion::parse("5.2.1").expect("a version"),
            kind: ExtensionKind::WebApp,
            description: String::new(),
            homepage: None,
            signed: true,
            permissions: mixengine_proto::ExtensionPermissions::default(),
            ports: Vec::new(),
            install_dir: "/x".to_owned(),
            data_dir: "/y".to_owned(),
            site: Some(mixengine_proto::PlannedSite {
                domain: "phpmyadmin.mixengine.test".to_owned(),
                pool: ServiceId::parse("php-fpm@phpmyadmin").expect("an id"),
                database: Some(ServiceId::parse("mariadb@main").expect("an id")),
                signs_in: Some("root".to_owned()),
            }),
        };

        let rendered = extension_plan(&plan);

        assert!(
            rendered
                .contains("site         https://phpmyadmin.mixengine.test, on php-fpm@phpmyadmin"),
            "{rendered}"
        );
        assert!(
            rendered.contains("database     mariadb@main"),
            "which server it would open onto is shown before anybody agrees: {rendered}"
        );
        assert!(
            rendered.contains("signs in     as root"),
            "the account, and the sentence about where its password comes from: {rendered}"
        );
        assert!(
            rendered.contains("keyring"),
            "a person agreeing to this is told the password is never written down: {rendered}"
        );
    }

    /// An inspection of the Mailpit fixture, as the daemon would answer it.
    fn mailpit_inspection() -> ExtensionInspection {
        let spec = mixengine_proto::ServiceSpec::builder(
            ServiceId::parse("mailpit").expect("an id"),
            if cfg!(windows) {
                r"C:\home\.mixengine\extensions\mailpit\mailpit"
            } else {
                "/home/dev/.mixengine/extensions/mailpit/mailpit"
            },
        )
        .cwd(if cfg!(windows) {
            r"C:\home\.mixengine\extensions\mailpit\data"
        } else {
            "/home/dev/.mixengine/extensions/mailpit/data"
        })
        .args(["--listen", "127.0.0.1:8025", "--smtp", "127.0.0.1:1025"])
        .ready(mixengine_proto::ReadyCheck::Tcp {
            addr: "127.0.0.1:8025".parse().expect("an address"),
            timeout: mixengine_proto::Millis::from_secs(10),
        })
        .build()
        .expect("a spec");

        ExtensionInspection {
            id: mixengine_proto::ExtensionId::parse("mailpit").expect("an id"),
            name: "Mailpit".to_owned(),
            version: PackageVersion::parse("1.20.0").expect("a version"),
            kind: ExtensionKind::Service,
            description: "Local SMTP capture and web UI".to_owned(),
            homepage: Some("https://mailpit.axllent.org".to_owned()),
            permissions: mixengine_proto::ExtensionPermissions {
                services: std::collections::BTreeSet::new(),
                network: NetworkReach::Loopback,
                filesystem: [FilesystemReach::OwnData].into_iter().collect(),
            },
            artifact: ArtifactAvailability::Published {
                url: "https://example.invalid/mailpit.zip".to_owned(),
                sha256: "0".repeat(64),
            },
            ports: vec![
                PortWish {
                    name: "ui_port".to_owned(),
                    wanted: 8025,
                },
                PortWish {
                    name: "smtp_port".to_owned(),
                    wanted: 1025,
                },
            ],
            install_dir: "/home/dev/.mixengine/extensions/mailpit".to_owned(),
            data_dir: "/home/dev/.mixengine/extensions/mailpit/data".to_owned(),
            runs: Some(spec),
            serves: None,
            extends: vec![RecipeAddition::PhpIni {
                key: "sendmail_path".to_owned(),
                value: "/home/dev/.mixengine/extensions/mailpit/mailpit sendmail".to_owned(),
            }],
        }
    }

    /// A person reads three things off this: what it is, what it would run, and what it asked
    /// for. The last is the one a line could mislead about.
    #[test]
    fn an_inspection_says_what_would_run_and_what_was_only_asked_for() {
        let rendered = extension_inspection(&mailpit_inspection());

        assert!(rendered.contains("mailpit"));
        assert!(rendered.contains("127.0.0.1:8025"));
        assert!(rendered.contains("asked for"));
        assert!(!rendered.contains("reserved"));
    }

    /// **`services` is a disclosure**, and the line says so rather than reading as a grant.
    #[test]
    fn the_permission_lines_do_not_read_as_grants() {
        let mut inspection = mailpit_inspection();
        inspection.permissions.services.insert(ApiAccess::Read);

        let rendered = extension_inspection(&inspection);

        assert!(rendered.contains("says it would"));
        assert!(rendered.contains("not a permission MixEngine enforces"));
    }

    /// `lan` renders `0.0.0.0`, which reads as alarming without the sentence beside it.
    #[test]
    fn every_interface_is_explained() {
        let mut inspection = mailpit_inspection();
        inspection.permissions.network = NetworkReach::Lan;

        let rendered = extension_inspection(&inspection);

        assert!(rendered.contains("reachable from other machines"));
    }

    /// A catalogue with nothing in it, which is what a freshly published registry answers.
    fn an_empty_catalogue(unreadable: usize) -> ExtensionCatalogue {
        ExtensionCatalogue {
            extensions: Vec::new(),
            unreadable,
            stale: false,
        }
    }

    /// Empty and old are different answers. The registry published for **T81a** lists nothing at
    /// all, and telling that person their build is too old sends them to update something that
    /// would not change the listing.
    #[test]
    fn an_empty_registry_does_not_read_as_a_build_too_old() {
        let rendered = extension_catalogue(&an_empty_catalogue(0));

        assert!(rendered.contains("no extensions"));
        assert!(!rendered.contains("this build"));
        assert!(!rendered.contains("update MixEngine"));
    }

    /// The other empty: every entry there is one this build cannot read, and that person *is* the
    /// one who should update.
    #[test]
    fn a_listing_this_build_cannot_read_still_says_to_update() {
        let rendered = extension_catalogue(&an_empty_catalogue(2));

        assert!(rendered.contains("this build can read"));
        assert!(rendered.contains("2 entries this build cannot read"));
        assert!(rendered.contains("update MixEngine"));
    }

    /// A plan with one of everything the renderer has a branch for.
    fn a_plan() -> BlueprintPlan {
        BlueprintPlan {
            blueprint: "laravel-php82".to_owned(),
            project: "shop".to_owned(),
            root: "/home/dev/shop".to_owned(),
            steps: vec![
                PlanStep {
                    action: PlanAction::InstallRuntime {
                        kind: mixengine_proto::RuntimeKind::Php,
                        wanted: mixengine_proto::VersionConstraint::parse("8.2.23")
                            .expect("a constraint"),
                    },
                    disposition: Disposition::Satisfied,
                    elevates: false,
                },
                PlanStep {
                    action: PlanAction::SetPhpExtension {
                        runtime: Some(PackageVersion::parse("8.2.23").expect("a version")),
                        name: "xdebug".to_owned(),
                    },
                    disposition: Disposition::Create,
                    elevates: false,
                },
                PlanStep {
                    action: PlanAction::AddDomain {
                        domain: "shop.test".to_owned(),
                        primary: true,
                    },
                    disposition: Disposition::Blocked {
                        reason: "shop.test is already answered by blog.test".to_owned(),
                    },
                    elevates: true,
                },
                PlanStep {
                    action: PlanAction::IssueCertificate {
                        domains: vec!["shop.test".to_owned()],
                    },
                    disposition: Disposition::Create,
                    elevates: true,
                },
            ],
            source: mixengine_proto::BlueprintSource::Captured,
            trusted: true,
            signature: None,
        }
    }

    /// **Every step, and the one that did not run said in full** — roadmap task T78. A scaffold
    /// command nobody ran is the one line a person has to act on themselves, so it is not folded
    /// into a count.
    #[test]
    fn an_apply_prints_every_step_and_spells_out_what_did_not_run() {
        let applied = BlueprintApplied {
            blueprint: "blog-stack".to_owned(),
            project: "shop".to_owned(),
            root: "/tmp/shop".to_owned(),
            steps: vec![
                StepOutcome {
                    action: PlanAction::RegisterProject {
                        name: "shop".to_owned(),
                        root: "/tmp/shop".to_owned(),
                        pins: std::collections::BTreeMap::new(),
                    },
                    result: StepResult::Done { note: None },
                },
                StepOutcome {
                    action: PlanAction::InstallRuntime {
                        kind: RuntimeKind::Php,
                        wanted: VersionConstraint::parse("8.2.23").expect("a constraint"),
                    },
                    result: StepResult::AlreadyTrue,
                },
                StepOutcome {
                    action: PlanAction::RunScaffold {
                        command: "composer install".to_owned(),
                    },
                    result: StepResult::NotRun {
                        why: "`composer install` was not run: nobody agreed to it".to_owned(),
                    },
                },
            ],
        };

        let rendered = super::blueprint_applied(&applied);

        assert!(rendered.contains("done"), "{rendered}");
        assert!(rendered.contains("already"), "{rendered}");
        assert!(rendered.contains("composer install"), "{rendered}");
        assert!(rendered.contains("nobody agreed to it"), "{rendered}");
    }

    /// **A step that ran and failed reads as that, not as one that was skipped** — roadmap task
    /// **T78a**, its design's D7. What a person does next differs between the two, and the exit
    /// status differs with it.
    #[test]
    fn a_failed_step_prints_its_exit_rather_than_a_skip() {
        let applied = BlueprintApplied {
            blueprint: "borrowed".to_owned(),
            project: "shop".to_owned(),
            root: "/tmp/shop".to_owned(),
            steps: vec![StepOutcome {
                action: PlanAction::RunScaffold {
                    command: "composer install".to_owned(),
                },
                result: StepResult::Failed {
                    why: "`composer install` exited with 1".to_owned(),
                },
            }],
        };

        let rendered = super::blueprint_applied(&applied);

        assert!(rendered.contains("failed"), "{rendered}");
        assert!(!rendered.contains("not run"), "{rendered}");
        assert!(rendered.contains("exited with 1"), "{rendered}");
        assert!(super::blueprint_had_a_failed_step(&applied));
    }

    /// T202, D2. What differed from the plan sits under the step it belongs to, indented past the
    /// status column, so the account a project ended up with is found where the step is.
    #[test]
    fn a_done_step_with_a_note_prints_it_under_the_step() {
        let applied = BlueprintApplied {
            blueprint: "laravel".to_owned(),
            project: "shop".to_owned(),
            root: "/tmp/shop".to_owned(),
            steps: vec![StepOutcome {
                action: PlanAction::CreateDatabase {
                    package: "mariadb".to_owned(),
                    instance: "main".to_owned(),
                    database: "shop".to_owned(),
                    user: "shop".to_owned(),
                },
                result: StepResult::Done {
                    note: Some(
                        "the account shop is somebody else's, so this project's is shop-2"
                            .to_owned(),
                    ),
                },
            }],
        };

        let rendered = super::blueprint_applied(&applied);

        let lines: Vec<&str> = rendered.lines().collect();
        let step = lines
            .iter()
            .position(|line| line.starts_with("  done"))
            .expect("the step line");
        assert_eq!(
            lines[step + 1],
            "              the account shop is somebody else's, so this project's is shop-2"
        );
    }

    /// An import says which of the two things a person now has, because it is the only moment they
    /// are told: the trust is decided there and never again.
    #[test]
    fn an_import_says_whether_anything_vouched_for_it() {
        let untrusted = super::blueprint_imported(&BlueprintSummary {
            slug: "borrowed".to_owned(),
            name: "borrowed".to_owned(),
            description: String::new(),
            created_at: "2026-09-01T00:00:00Z".to_owned(),
            source: mixengine_proto::BlueprintSource::Imported,
            trusted: false,
            signature: Some(mixengine_proto::SignatureCheck::Missing),
            file: "/home/dev/.mixengine/blueprints/borrowed.toml".to_owned(),
        });

        assert!(untrusted.contains("untrusted"), "{untrusted}");
        assert!(untrusted.contains("nothing will"), "{untrusted}");
    }

    /// **Which kind of untrusted** — roadmap task **T79b**. A file nobody signed and a file whose
    /// signature did not verify are both untrusted and are not the same event, and the second is
    /// the one worth reading twice.
    #[test]
    fn an_import_says_which_kind_of_untrusted_it_is() {
        let summary = |signature| BlueprintSummary {
            slug: "borrowed".to_owned(),
            name: "borrowed".to_owned(),
            description: String::new(),
            created_at: "2026-09-01T00:00:00Z".to_owned(),
            source: mixengine_proto::BlueprintSource::Imported,
            trusted: false,
            signature,
            file: "/home/dev/.mixengine/blueprints/borrowed.toml".to_owned(),
        };

        let missing = super::blueprint_imported(&summary(Some(SignatureCheck::Missing)));
        assert!(missing.contains("nothing came with it"), "{missing}");

        // True of all three things the verifier folds together — edited after signing, signed by
        // another key, and not a signature at all. "The bytes changed" would accuse the last two of
        // the first.
        let rejected = super::blueprint_imported(&summary(Some(SignatureCheck::Rejected)));
        assert!(rejected.contains("not the gallery's"), "{rejected}");
        assert!(!rejected.contains("nothing came with it"), "{rejected}");

        // A row older than this task kept no reason, and keeps the sentence it has always had.
        let older = super::blueprint_imported(&summary(None));
        assert!(older.contains("nothing vouches for it"), "{older}");
    }

    /// The listing says it in one word, because a table is where somebody scans six blueprints at
    /// once — and `--json` carries the same three facts for anything that is not a person.
    #[test]
    fn the_listing_says_which_kind_of_untrusted_each_one_is() {
        let row = |slug: &str, trusted, signature| BlueprintSummary {
            slug: slug.to_owned(),
            name: slug.to_owned(),
            description: String::new(),
            created_at: "2026-09-01T00:00:00Z".to_owned(),
            source: mixengine_proto::BlueprintSource::Imported,
            trusted,
            signature,
            file: format!("/home/dev/.mixengine/blueprints/{slug}.toml"),
        };

        let listed = super::blueprint_list(&BlueprintList {
            blueprints: vec![
                row("good", true, Some(SignatureCheck::Verified)),
                row("bare", false, Some(SignatureCheck::Missing)),
                row("stale", false, Some(SignatureCheck::Rejected)),
                row("older", false, None),
            ],
        });

        let line = |slug: &str| {
            listed
                .lines()
                .find(|line| line.starts_with(slug))
                .unwrap_or_else(|| panic!("no row for {slug}: {listed}"))
                .to_owned()
        };

        assert!(line("good").contains("signed"), "{listed}");
        assert!(line("bare").contains("unsigned"), "{listed}");
        assert!(line("stale").contains("mismatched"), "{listed}");
        assert!(line("older").contains("untrusted"), "{listed}");
    }

    /// Words, not glyphs — and the reason a step is blocked travels with the step, because a person
    /// reading "blocked" without it has to go looking.
    #[test]
    fn a_plan_prints_one_line_per_step_and_says_who_holds_a_taken_domain() {
        let rendered = super::blueprint_plan(&a_plan());

        assert!(rendered.contains("installed"), "{rendered}");
        assert!(rendered.contains("blocked"), "{rendered}");
        assert!(
            rendered.contains("already answered by blog.test"),
            "{rendered}"
        );
        assert!(
            !rendered.contains('\u{2713}') && !rendered.contains('\u{2717}'),
            "a status glyph crept into a file that has never had one:\n{rendered}"
        );
    }

    /// **D11**: said once, at the end, so a person knows before they start rather than four lines
    /// in.
    #[test]
    fn a_plan_that_would_ask_for_a_password_says_so_once() {
        let rendered = super::blueprint_plan(&a_plan());

        assert_eq!(rendered.matches("elevation").count(), 1, "{rendered}");
    }

    /// Enabling an extension changes the PHP every project on this machine runs, and that belongs
    /// on the line somebody is deciding from.
    #[test]
    fn enabling_an_extension_says_that_it_reaches_past_this_project() {
        let rendered = super::blueprint_plan(&a_plan());

        assert!(rendered.contains("every project here"), "{rendered}");
    }

    /// An empty home says what to type next, the way every other empty listing here does.
    #[test]
    fn a_home_with_no_blueprints_says_how_to_make_one() {
        let rendered = super::blueprint_list(&BlueprintList {
            blueprints: Vec::new(),
        });

        assert!(rendered.contains("mix blueprint capture"), "{rendered}");
    }

    /// A share as the daemon answers one, advertised or not — roadmap tasks **T74** and **T75**.
    fn a_share(advertised: bool) -> SiteSharing {
        SiteSharing {
            interface: "Wi-Fi".to_owned(),
            address: "192.168.1.10".to_owned(),
            url: "http://192.168.1.10".to_owned(),
            name: Some("blog-mixengine.local".to_owned()),
            advertised,
            ca_url: "http://192.168.1.10/__mixengine/ca.crt".to_owned(),
            since: Timestamp(1_700_000_000_000),
            until: None,
        }
    }

    /// What `mix site share` prints once the phone has a name and somewhere to get the authority —
    /// roadmap task **T75**.
    ///
    /// **The QR stays on the address.** Android's resolver does not answer `.local` for a browser,
    /// so a code carrying the name would be a broken URL for a large share of phones — the T75
    /// design, D11.
    #[test]
    fn a_shared_site_prints_its_name_and_where_to_get_the_authority() {
        let rendered = super::site_shared(&a_share(true));

        assert!(rendered.contains("blog-mixengine.local"), "{rendered}");
        assert!(rendered.contains("/__mixengine/ca.crt"), "{rendered}");
        assert!(
            rendered.contains("Certificate Trust Settings"),
            "{rendered}"
        );
    }

    /// **A deadline is printed as a wait and not as a timestamp** — roadmap task **T76**. Somebody
    /// who has just typed `--for 2h` wants to know it took, and "in 2h" is the answer to that; an
    /// instant in milliseconds is a number they would have to convert.
    #[test]
    fn a_share_with_a_deadline_says_when_it_ends() {
        let Timestamp(millis) = Timestamp::from_system_time(SystemTime::now());

        let sharing = SiteSharing {
            until: Some(Timestamp(millis + 7_200_000)),
            ..a_share(true)
        };

        let rendered = super::site_shared(&sharing);

        // **The wait and not the figure.** `site_shared` reads its own clock, so the minutes have
        // already moved by the time this line runs; asserting `in 2h 0m` would be asserting that
        // no time passed between two statements, which is the shape of a test that fails once a
        // month on a loaded machine and is then marked flaky rather than read.
        assert!(rendered.contains("  ends       in "), "{rendered}");
    }

    /// The figure itself, where the clock is an argument and not a reading — the same rounding
    /// `ago` and `uptime` use, so all three say `1h 59m` about the same span.
    #[test]
    fn a_wait_is_rounded_the_way_an_age_is() {
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);

        assert_eq!(in_time(Timestamp(1_000_000 + 7_200_000), now), "in 2h 0m");
        assert_eq!(in_time(Timestamp(1_000_000 + 90_000), now), "in 1m 30s");
    }

    /// **A share with no deadline prints no line about one.** Every line in this block is one a
    /// reader has to take in, and "ends never" is one they would have to learn to skip.
    #[test]
    fn a_share_with_no_deadline_says_nothing_about_one() {
        assert!(!super::site_shared(&a_share(true)).contains("ends"));
    }

    /// A deadline the loop has not caught up with yet is a wait, not a negative number: the sweep
    /// runs on a period, so an instant a few seconds past is the ordinary state.
    #[test]
    fn a_deadline_already_gone_reads_as_a_wait() {
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(10);

        assert_eq!(in_time(Timestamp(1_000), now), "any moment now");
    }

    /// **A name nothing is answering for is said, not hidden.** The site still works by address,
    /// and a name printed as though it resolved is the slowest kind of wrong.
    #[test]
    fn a_name_that_is_not_advertised_says_so() {
        let rendered = super::site_shared(&a_share(false));

        assert!(rendered.contains("not being advertised"), "{rendered}");
    }

    /// What `mix site share` prints — roadmap task **T74**.
    ///
    /// **The URL is above the code, and both are there.** A QR is unreadable to anyone reading a
    /// transcript, piping the output, or working over a connection that mangles block characters,
    /// so the string a person could type by hand is never replaced by a picture of it.
    #[test]
    fn a_shared_site_prints_its_url_the_interface_and_a_code() {
        let rendered = super::site_shared(&a_share(true));

        assert!(rendered.contains("http://192.168.1.10"), "{rendered}");
        assert!(rendered.contains("Wi-Fi"), "{rendered}");

        // The code itself: half-height blocks, and enough of them to be a QR rather than a stray
        // character in a sentence.
        assert!(
            rendered.matches(['█', '▀', '▄']).count() > 100,
            "{rendered}"
        );

        // And why it is http, which is the question the URL raises for a site declaring HTTPS.
        assert!(rendered.contains("certificate authority"), "{rendered}");
    }

    /// The line names both numbers and the ending, so nobody has to infer either — task **T71a**.
    ///
    /// **The `false` case is the one worth a test.** A database over its ceiling is warned about and
    /// deliberately left alone, and a rendering that said only "watched" would read exactly like one
    /// that was about to rescue it.
    #[test]
    fn a_watchdog_line_says_what_happens_at_the_end_of_it() {
        let restarted = super::watchdog_line(Some(MemoryWatchdog {
            after_minutes: 3,
            restarts: true,
        }));

        assert!(restarted.contains('3'), "{restarted}");
        assert!(restarted.contains("restarted after"), "{restarted}");

        let warned = super::watchdog_line(Some(MemoryWatchdog {
            after_minutes: 3,
            restarts: false,
        }));

        assert!(
            warned.contains("not restarted automatically"),
            "a service that is only warned about must not read as one that is rescued: {warned}"
        );

        assert!(
            super::watchdog_line(None).is_empty(),
            "nothing watching is no line at all, not an empty one"
        );
    }

    fn sample(subject: MetricsSubject, cpu: Option<f32>, rss: u64) -> MetricsSample {
        MetricsSample {
            subject,
            cpu_percent: cpu,
            rss_bytes: rss,
            processes: 1,
        }
    }

    #[test]
    fn a_subject_with_no_cpu_figure_renders_a_dash_and_never_a_zero() {
        let rendered = metrics_frame(&MetricsFrame {
            at: Timestamp(60_000),
            samples: vec![sample(MetricsSubject::Daemon, None, 41_943_040)],
            cores: 1,
        });

        assert!(rendered.contains("40.0 MiB"), "{rendered}");
        assert!(rendered.contains(MISSING), "{rendered}");
        assert!(
            !rendered.contains("0.0%"),
            "a figure that could not be taken is not a service using no CPU: {rendered}"
        );
    }

    /// **CPU is a share of the machine with one decimal, as Task Manager shows it** — roadmap task
    /// T190c. The frame's figure is percent of one core; `cores` is what one core is worth here.
    #[test]
    fn cpu_is_a_share_of_the_machine() {
        let rendered = metrics_frame(&MetricsFrame {
            at: Timestamp(60_000),
            samples: vec![
                sample(MetricsSubject::Daemon, Some(150.0), 1),
                sample(
                    MetricsSubject::Service(ServiceId::parse("redis@main").expect("an id")),
                    Some(0.53),
                    1,
                ),
            ],
            cores: 12,
        });

        assert!(rendered.contains("12.5%"), "{rendered}");
        assert!(
            rendered.contains("<0.1%"),
            "a running service never reads as idle: {rendered}"
        );
    }

    #[test]
    fn a_frame_that_measured_nothing_says_so_rather_than_printing_an_empty_table() {
        let rendered = metrics_frame(&MetricsFrame {
            at: Timestamp(60_000),
            samples: Vec::new(),
            cores: 1,
        });

        assert_eq!(
            rendered,
            "nothing could be measured
"
        );
    }

    #[test]
    fn a_history_row_shows_how_many_readings_it_is_made_of() {
        let now = SystemTime::now();
        let Timestamp(millis) = Timestamp::from_system_time(now);

        let rendered = metrics_history(
            &MetricsHistory {
                minutes: vec![MetricsMinute {
                    subject: MetricsSubject::Daemon,
                    minute: Timestamp(millis - 120_000),
                    cpu_avg: Some(1.5),
                    cpu_peak: Some(9.5),
                    rss_avg: 41_943_040,
                    rss_peak: 62_914_560,
                    samples: 60,
                }],
                retention_hours: 24,
                cores: 4,
            },
            now,
        );

        assert!(rendered.contains("SAMPLES"), "{rendered}");
        assert!(rendered.contains("60"), "{rendered}");
        assert!(
            rendered.contains("0.4%") && rendered.contains("2.4%"),
            "1.5 and 9.5 percent of one core on four cores (T190c): {rendered}"
        );
        assert!(
            rendered.contains("ago"),
            "a minute is printed as an age: {rendered}"
        );
    }

    #[test]
    fn an_empty_history_says_how_long_this_home_keeps_one() {
        let rendered = metrics_history(
            &MetricsHistory {
                minutes: Vec::new(),
                retention_hours: 24,
                cores: 1,
            },
            SystemTime::now(),
        );

        assert!(rendered.contains("24 hours"), "{rendered}");
    }

    fn example() -> DaemonStatus {
        DaemonStatus {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            protocol: PROTOCOL_VERSION,
            pid: 4123,
            home: "/home/dev/.local/share/mixengine".to_owned(),
            endpoint: "/home/dev/.local/share/mixengine/run/mixengined.sock".to_owned(),
            database: "/home/dev/.local/share/mixengine/mixengine.db".to_owned(),
            started_at: Timestamp(1_723_000_000_500),
            uptime: Uptime(812),
            elevation: Some(mixengine_proto::ElevationSummary {
                elevated: false,
                can_prompt: true,
                pending: 0,
            }),
            dns: Some(mixengine_proto::DnsStatus {
                mode: DnsMode::HostsOnly,
                listening: None,
                wildcards: Vec::new(),
                because: Some("[dns] enabled = false in config.toml".to_owned()),
            }),
            update: None,
            credentials: None,
        }
    }

    /// T194: `mix status` names the store when the daemon says, and invents nothing when it does
    /// not — Review Focus 5.
    #[test]
    fn status_names_the_credential_store_when_the_daemon_says() {
        let told = DaemonStatus {
            credentials: Some(mixengine_proto::CredentialsStatus {
                store: mixengine_proto::CredentialStore::Home,
                choosable: true,
            }),
            ..example()
        };

        let rendered = status(&told);
        assert!(
            rendered.contains("  passwords a file in this home"),
            "{rendered}"
        );

        let rendered = status(&example());
        assert!(!rendered.contains("  passwords "), "{rendered}");
    }

    /// `mix status` gains one line when an update is offered — roadmap task **T88**.
    #[test]
    fn status_names_the_version_that_is_waiting() {
        let offered = DaemonStatus {
            update: Some(mixengine_proto::UpdateOffer {
                version: "0.2.0".to_owned(),
                published_at: "2026-09-05T09:12:00Z".to_owned(),
            }),
            ..example()
        };

        let rendered = status(&offered);

        assert!(rendered.contains("0.2.0"), "{rendered}");
        assert!(rendered.contains("mix self-update"), "{rendered}");
    }

    /// And prints nothing at all when there is not, which is every daemon that has not checked.
    #[test]
    fn status_says_nothing_about_updates_when_there_is_nothing_to_say() {
        let rendered = status(&example());

        assert!(!rendered.contains("update"), "{rendered}");
    }

    /// A release this account cannot install is refused in the daemon's own words, and the words are
    /// printed rather than replaced by a code this client would have to invent a sentence for.
    /// A status as a copy the `.pkg` installed reports it — T88f: `managed` on the wire, with the
    /// installer beside it, and a release offered.
    fn pkg_status() -> UpdateStatus {
        UpdateStatus {
            current: "0.0.8".to_owned(),
            available: Some(mixengine_proto::UpdateRelease {
                version: "0.0.9".to_owned(),
                published_at: "2026-09-24T00:00:00Z".to_owned(),
                notes: "feat(updates): hand a .pkg to Installer.app".to_owned(),
                notes_url: None,
                size: 68_638_683,
            }),
            offered: true,
            because: None,
            checked_at: Some(Timestamp(1_790_183_317_000)),
            stale: false,
            placement: UpdatePlacement::Managed {
                directory: "/usr/local/bin".to_owned(),
                because: "the .pkg installed this copy".to_owned(),
            },
            will_restart: Vec::new(),
            installer: Some(mixengine_proto::UpdateInstaller {
                kind: "pkg".to_owned(),
                size: 68_638_683,
            }),
            installed: None,
        }
    }

    /// A copy the `.pkg` installed is offered the installer, not refused (T88f, D8).
    #[test]
    fn a_pkg_copy_is_offered_the_installer_rather_than_refused() {
        let rendered = update_status(&pkg_status());

        assert!(!rendered.contains("not by MixEngine"), "{rendered}");
        assert!(rendered.contains("0.0.9"), "{rendered}");
        assert!(rendered.contains("Installer.app"), "{rendered}");
    }

    /// Installed and not yet restarted: the one thing to say is how to finish.
    #[test]
    fn an_installed_version_says_to_finish() {
        let rendered = update_status(&UpdateStatus {
            installed: Some("0.0.9".to_owned()),
            ..pkg_status()
        });

        assert!(rendered.contains("mix self-update --finish"), "{rendered}");
    }

    /// The path and the command, every time: over SSH, Installer.app opens on the Mac's own screen
    /// (the T88f readings, M4).
    #[test]
    fn a_handover_prints_the_package_and_the_command_every_time() {
        let package = "/Users/x/Library/Application Support/MixEngine/cache/updates/0.0.9/\
                       mixlab-0.0.9-macos-universal.pkg";
        let rendered = update_handed_over(&mixengine_proto::UpdateHandedOver {
            version: "0.0.9".to_owned(),
            package: package.to_owned(),
            command: format!("sudo installer -pkg '{package}' -target /"),
            opened: true,
        });

        assert!(rendered.contains(package), "{rendered}");
        assert!(rendered.contains("sudo installer -pkg"), "{rendered}");
        assert!(rendered.contains("mix self-update --finish"), "{rendered}");
    }

    /// T182b. The wait for the folders to go says how much is going, and then that it is still going.
    #[test]
    fn the_wait_for_a_removal_says_what_it_is_waiting_for() {
        assert_eq!(
            uninstall_removing(3, 1_073_741_824),
            "removing 3 folders (1024 MiB), this can take a minute"
        );
        assert_eq!(
            uninstall_removing(1, 0),
            "removing 1 folder, this can take a minute"
        );
        assert_eq!(
            uninstall_still_removing(std::time::Duration::from_millis(20_400)),
            "still removing, 20s so far"
        );
    }

    /// T182b, D5. A Linux machine with no desktop session opens nothing: the command is printed as
    /// the step to take, and nothing claims an installer is open.
    #[test]
    fn a_handover_with_nothing_opened_prints_the_command_as_the_way() {
        let package = "/home/x/.local/share/mixengine/cache/updates/0.0.9/\
                       mixengine-headless_0.0.9-1_amd64.deb";
        let rendered = update_handed_over(&mixengine_proto::UpdateHandedOver {
            version: "0.0.9".to_owned(),
            package: package.to_owned(),
            command: format!("sudo apt install '{package}'"),
            opened: false,
        });

        assert!(
            rendered.contains(&format!("sudo apt install '{package}'")),
            "{rendered}"
        );
        assert!(rendered.contains("mix self-update --finish"), "{rendered}");
        assert!(!rendered.contains("is open"), "{rendered}");
    }

    #[test]
    fn a_managed_install_prints_the_daemons_own_reason() {
        let rendered = update_status(&UpdateStatus {
            current: "0.1.0".to_owned(),
            available: None,
            offered: false,
            because: None,
            checked_at: None,
            stale: false,
            placement: UpdatePlacement::Managed {
                directory: "/usr/bin".to_owned(),
                because: "this account cannot write to /usr/bin".to_owned(),
            },
            will_restart: Vec::new(),
            installer: None,
            installed: None,
        });

        assert!(rendered.contains("/usr/bin"), "{rendered}");
        assert!(rendered.contains("not by MixEngine"), "{rendered}");
    }

    /// The consent prompt's four facts: the version, the size, what changed, and what stops.
    #[test]
    fn an_offer_carries_everything_somebody_needs_to_answer_it() {
        let rendered = update_status(&UpdateStatus {
            current: "0.1.0".to_owned(),
            available: Some(mixengine_proto::UpdateRelease {
                version: "0.2.0".to_owned(),
                published_at: "2026-09-05T09:12:00Z".to_owned(),
                notes: "feat(cli): mix self-update".to_owned(),
                notes_url: Some("https://example.invalid/v0.2.0".to_owned()),
                size: 15 << 20,
            }),
            offered: true,
            because: None,
            checked_at: Some(Timestamp(1_757_000_000_000)),
            stale: false,
            placement: UpdatePlacement::SelfUpdatable {
                directory: "/home/dev/.local/bin".to_owned(),
            },
            will_restart: vec![
                ServiceId::parse("mariadb").expect("a service id"),
                ServiceId::parse("caddy").expect("a service id"),
            ],
            installer: None,
            installed: None,
        });

        assert!(rendered.contains("0.2.0"), "{rendered}");
        assert!(rendered.contains("15 MiB"), "{rendered}");
        assert!(rendered.contains("mix self-update"), "{rendered}");
        assert!(rendered.contains("2 services"), "{rendered}");
        assert!(rendered.contains("mariadb"), "{rendered}");
        assert!(
            rendered.contains("https://example.invalid/v0.2.0"),
            "{rendered}"
        );
    }

    /// What was replaced, and — the line that stops somebody thinking the update was partial — what
    /// deliberately was not.
    #[test]
    fn an_applied_update_says_the_helper_was_kept_and_why() {
        let rendered = update_applied(&UpdateApplied {
            from: "0.1.0".to_owned(),
            to: "0.2.0".to_owned(),
            directory: "/home/dev/.local/bin".to_owned(),
            replaced: vec!["mix".to_owned(), "mixengined".to_owned()],
            kept: vec!["mixengine-elevate".to_owned()],
            restarting: vec![ServiceId::parse("mariadb").expect("a service id")],
        });

        assert!(rendered.contains("0.1.0 → 0.2.0"), "{rendered}");
        assert!(rendered.contains("mixengine-elevate"), "{rendered}");
        assert!(rendered.contains("own prompt"), "{rendered}");
    }

    #[test]
    fn the_human_rendering_leads_with_whether_it_is_up_and_which_home_it_is() {
        let rendered = status(&example());
        let mut lines = rendered.lines();

        // Read from the same constant `example()` builds the status with, rather than written out:
        // the fixture has always been version-agnostic and this assertion was not, so a version bump
        // failed a test that is about a *heading* and not about a number.
        let heading = format!(
            "mixengined {}: running (pid 4123, up 13m 32s)",
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(lines.next(), Some(heading.as_str()));
        assert_eq!(
            lines.next(),
            Some("  home      /home/dev/.local/share/mixengine")
        );
    }

    #[test]
    fn a_daemon_from_another_build_is_explained_rather_than_left_to_be_noticed() {
        // 0.0.0 because no build is ever numbered that: a real version here became the workspace's
        // own at the v0.0.9 bump, and the daemon stopped being from another build.
        let mut older = example();
        older.version = "0.0.0".to_owned();

        let rendered = status(&older);
        assert!(rendered.contains("has not been restarted"), "{rendered}");
        assert!(rendered.contains("0.0.0"), "{rendered}");

        // And the ordinary case says nothing, because a note on every line of a status somebody
        // reads daily is a note nobody reads.
        assert!(!status(&example()).contains("note"));
    }

    /// A daemon from before `elevation` and `dns` existed — roadmap task **T88c**.
    ///
    /// **The point is that this renders at all.** Until T88c the answer did not deserialise, so the
    /// note below — written for exactly this skew, and tested above — could never reach anybody.
    ///
    /// What is *not* printed matters as much: no `names` line invented from a default, which would
    /// state that `api.blog.test` does not resolve on the word of a daemon that said nothing.
    #[test]
    fn a_daemon_that_reported_neither_names_nor_elevation_says_so_and_invents_nothing() {
        let older = DaemonStatus {
            // Never a build's own version; see the test above.
            version: "0.0.0".to_owned(),
            elevation: None,
            dns: None,
            ..example()
        };

        let rendered = status(&older);

        // The lines that would have carried them are absent rather than guessed at. Matched on the
        // label column — two spaces and the label — because the note below says "how names resolve"
        // and "what is waiting for permission", and a bare `contains("names ")` would find those.
        assert!(!rendered.contains("  names "), "{rendered}");
        assert!(!rendered.contains("  waiting "), "{rendered}");
        assert!(!rendered.contains("hosts file"), "{rendered}");

        // And the one note says both halves: which daemon this is, and what it did not say.
        assert!(rendered.contains("has not been restarted"), "{rendered}");
        assert!(rendered.contains("did not report"), "{rendered}");
        assert!(rendered.contains("how names resolve"), "{rendered}");
        assert!(
            rendered.contains("what is waiting for permission"),
            "{rendered}"
        );
        assert_eq!(
            rendered
                .lines()
                .filter(|line| line.contains("note "))
                .count(),
            1,
            "{rendered}"
        );
    }

    #[test]
    fn the_json_is_the_daemons_answer_untouched_under_one_key() {
        let status = example();
        let encoded = status_json(&status);

        assert_eq!(encoded["daemon"], serde_json::to_value(&status).unwrap());
        assert_eq!(encoded["client"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(encoded["client"]["protocol"], 1);
        // Unrounded, in seconds: the rendering above is for a person and this is for a program.
        assert_eq!(encoded["daemon"]["uptime"], 812);
    }

    fn id(value: &str) -> ServiceId {
        ServiceId::parse(value).expect("a valid service id")
    }

    /// A summary in the shape a given state implies: a running service has a process and a
    /// supervisor, a stopped one has neither, and one with no row has nothing at all.
    fn summary(id_value: &str, state: Option<ServiceState>) -> ServiceSummary {
        let running = state == Some(ServiceState::Running);

        ServiceSummary {
            id: id(id_value),
            state,
            supervised: running,
            pid: running.then_some(4123),
            port: None,
            last_started_at: running.then_some(Timestamp(1_723_000_000_000)),
            last_exit_code: None,
            depends_on: Vec::new(),
            role: Some(mixengine_proto::ServiceRole::Other {}),
            autostart: false,
            stopped_by: None,
            version: None,
            last_failure: None,
        }
    }

    /// A create that got the port it asked for says so, and explains nothing.
    #[test]
    fn a_service_created_on_the_port_it_wanted_is_reported_without_a_story() {
        let rendered = service_creation(&ServiceCreation {
            service: ServiceSummary {
                port: Some(3306),
                ..summary("mariadb@main", Some(ServiceState::Stopped))
            },
            moved_from: None,
        });

        assert!(rendered.contains("created mariadb@main"), "{rendered}");
        assert!(rendered.contains("port 3306"), "{rendered}");
        assert!(
            !rendered.contains("moved"),
            "nothing moved it, so nothing should say so: {rendered}"
        );
    }

    /// One that was moved names the port it wanted and the program that has it.
    ///
    /// The whole point of the field: a developer whose `.env` says 3306 finds out here rather than
    /// from a connection refused an hour later.
    #[test]
    fn a_service_moved_off_its_preferred_port_names_the_program_that_took_it() {
        let rendered = service_creation(&ServiceCreation {
            service: ServiceSummary {
                port: Some(3307),
                ..summary("mysql@main", Some(ServiceState::Stopped))
            },
            moved_from: Some(mixengine_proto::PortMoved {
                preferred: 3306,
                pid: Some(4242),
                program: Some("mysqld.exe".to_owned()),
            }),
        });

        assert!(rendered.contains("port 3307"), "{rendered}");
        assert!(rendered.contains("asked for 3306"), "{rendered}");
        assert!(rendered.contains("mysqld.exe has it"), "{rendered}");
    }

    /// A machine that will name neither the program nor the pid still says what happened.
    ///
    /// Which is the ordinary case for a port another *MixEngine* service holds: the row has it and
    /// there may be no process at all.
    #[test]
    fn a_move_with_nothing_to_name_still_says_the_port_was_taken() {
        let rendered = service_creation(&ServiceCreation {
            service: ServiceSummary {
                port: Some(3307),
                ..summary("mysql@main", Some(ServiceState::Stopped))
            },
            moved_from: Some(mixengine_proto::PortMoved {
                preferred: 3306,
                pid: None,
                program: None,
            }),
        });

        assert!(
            rendered.contains("another service or program on this machine has it"),
            "{rendered}"
        );
    }

    /// A failed service with a note, the way the daemon reports one (T200b).
    fn failed_with(id_value: &str, detail: &str) -> ServiceSummary {
        ServiceSummary {
            last_failure: Some(mixengine_proto::ServiceFailureNote {
                at: Timestamp(1_760_000_000_000),
                reason: mixengine_proto::StateReason::SpawnFailed,
                detail: detail.to_owned(),
            }),
            ..summary(id_value, Some(ServiceState::Failed))
        }
    }

    /// **T200b, D6.** The listing ends with why each failed service failed.
    #[test]
    fn a_failed_service_is_followed_by_why() {
        let rendered = service_list(&ServiceList {
            services: vec![
                failed_with("php-fpm@phpmyadmin", "no credential is stored at x"),
                summary("caddy", Some(ServiceState::Running)),
            ],
        });

        assert!(
            rendered.ends_with(
                "php-fpm@phpmyadmin failed: no credential is stored at x
"
            ),
            "{rendered}"
        );
        assert!(!rendered.contains("caddy failed"), "{rendered}");
    }

    /// **T200b, D6.** `mix site show` names the service its site cannot start, and why.
    #[test]
    fn a_site_whose_pool_failed_says_which_and_why() {
        let detail: SiteDetail = serde_json::from_value(serde_json::json!({
            "site": {
                "domain": "phpmyadmin.mixengine.test",
                "owner": {"type": "extension", "id": "phpmyadmin"},
                "kind": {"kind": "php-fpm", "pool": "php-fpm@phpmyadmin"},
                "doc_root": "",
                "https": true,
                "https_redirect": false,
                "state": "enabled"
            },
            "root": "/x",
            "doc_root_full": "/x",
            "doc_root_exists": true,
            "domains": ["phpmyadmin.mixengine.test"],
            "pool": {"declared": "php-fpm@phpmyadmin", "resolved": "php-fpm@phpmyadmin"},
            "services": [{"service": "mariadb@main", "state": "running"}]
        }))
        .expect("a site detail");

        let services = ServiceList {
            services: vec![
                failed_with("php-fpm@phpmyadmin", "no credential is stored at x"),
                summary("mariadb@main", Some(ServiceState::Running)),
            ],
        };

        assert_eq!(
            site_failures(&detail, &services),
            "php-fpm@phpmyadmin could not start: no credential is stored at x
"
        );
        assert_eq!(
            site_failures(
                &detail,
                &ServiceList {
                    services: vec![summary("php-fpm@phpmyadmin", Some(ServiceState::Running))]
                }
            ),
            ""
        );
    }

    #[test]
    fn the_listing_is_a_table_whose_columns_line_up_whatever_the_names_are() {
        let list = ServiceList {
            services: vec![
                ServiceSummary {
                    version: Some(PackageVersion::parse("11.4.3").expect("a version")),
                    ..summary("mariadb@main", Some(ServiceState::Running))
                },
                ServiceSummary {
                    depends_on: vec![id("mariadb@main")],
                    ..summary("php", Some(ServiceState::Stopped))
                },
            ],
        };

        let rendered = service_list(&list);
        let lines: Vec<&str> = rendered.lines().collect();

        assert_eq!(
            lines[0],
            "SERVICE       VERSION  STATE    AUTOSTART  SUPERVISED  PID   DEPENDS ON"
        );
        assert_eq!(
            lines[1],
            "mariadb@main  11.4.3   running  no         yes         4123  —"
        );
        assert_eq!(
            lines[2],
            "php           —        stopped  no         no          —     mariadb@main"
        );
    }

    /// **T183.** A status names the version beside the other facts, and a service with none
    /// prints no empty line for it.
    #[test]
    fn a_status_names_the_version_the_service_runs() {
        let known = service_status(&ServiceSummary {
            version: Some(PackageVersion::parse("5.7.44").expect("a version")),
            ..summary("mysql@main", Some(ServiceState::Stopped))
        });
        assert!(known.contains("  version     5.7.44\n"), "{known}");

        let unknown = service_status(&summary("mysql@main", Some(ServiceState::Stopped)));
        assert!(!unknown.contains("version"), "{unknown}");
    }

    #[test]
    fn a_home_with_no_declarations_says_so_rather_than_printing_a_bare_heading() {
        assert_eq!(
            service_list(&ServiceList {
                services: Vec::new()
            }),
            "no services are declared in this home\n"
        );
    }

    #[test]
    fn a_service_that_was_never_created_is_told_apart_from_one_that_is_stopped() {
        let rendered = service_status(&summary("mailpit", None));

        assert!(rendered.starts_with("mailpit: not created"), "{rendered}");
        assert!(rendered.contains("has never been created"), "{rendered}");

        // The ordinary case says nothing extra, because a note on every status is a note nobody
        // reads — the same rule `mix status` follows for a daemon from another build.
        let stopped = service_status(&summary("mailpit", Some(ServiceState::Stopped)));
        assert!(!stopped.contains("note"), "{stopped}");
    }

    #[test]
    fn a_start_time_that_outlived_its_run_is_labelled_as_history_rather_than_as_the_present() {
        let running = service_status(&summary("mariadb@main", Some(ServiceState::Running)));
        assert!(running.contains("started"), "{running}");
        assert!(!running.contains("last start"), "{running}");

        // The same field, and the daemon keeps it across a stop on purpose — so the rendering is
        // what has to stop claiming the service is in the run it names.
        let stopped = ServiceSummary {
            last_started_at: Some(Timestamp(1_723_000_000_000)),
            ..summary("mariadb@main", Some(ServiceState::Stopped))
        };

        let rendered = service_status(&stopped);
        assert!(rendered.contains("last start"), "{rendered}");
        assert!(!rendered.contains("started"), "{rendered}");
    }

    #[test]
    fn a_row_naming_a_process_nothing_is_supervising_is_pointed_at_rather_than_smoothed_over() {
        let orphan = ServiceSummary {
            supervised: false,
            ..summary("mariadb@main", Some(ServiceState::Running))
        };

        let rendered = service_status(&orphan);
        assert!(rendered.contains("supervised  no"), "{rendered}");
        assert!(rendered.contains("daemon which was killed"), "{rendered}");
    }

    #[test]
    fn a_walk_that_reached_everything_is_one_line() {
        let walk = ServiceWalk {
            planned: vec![id("mariadb@main"), id("php-fpm@8.3")],
            complete: true,
            reached: vec![id("mariadb@main"), id("php-fpm@8.3")],
            failed: None,
            blocked: Vec::new(),
        };

        assert_eq!(
            service_walk(Walked::Start, &walk),
            "started mariadb@main, php-fpm@8.3\n"
        );
        assert_eq!(
            service_walk(Walked::Stop, &walk),
            "stopped mariadb@main, php-fpm@8.3\n"
        );
    }

    /// T202a, D3. A walk's failure prints the runner's detail before the reason's own sentence.
    #[test]
    fn a_walks_failure_prints_the_runners_detail_first() {
        let walk = ServiceWalk {
            planned: vec![id("db")],
            complete: true,
            reached: vec![],
            failed: Some(mixengine_proto::ServiceFailure {
                service: id("db"),
                reason: Some(StateReason::SpawnFailed),
                detail: Some(
                    "the environment entry MYSQL_PWD: no credential is stored at mixengine/h/db/root"
                        .to_owned(),
                ),
            }),
            blocked: vec![],
        };

        let rendered = service_walk(Walked::Start, &walk);

        assert!(
            rendered.contains("db failed to start: the environment entry MYSQL_PWD"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("could not be started at all"),
            "{rendered}"
        );
    }

    #[test]
    fn a_walk_that_stopped_leads_with_the_service_to_fix_and_shows_what_it_took_down() {
        let walk = ServiceWalk {
            planned: vec![id("db"), id("web"), id("worker")],
            complete: true,
            reached: vec![id("db")],
            failed: Some(mixengine_proto::ServiceFailure {
                service: id("web"),
                detail: None,
                reason: Some(StateReason::CrashLoop {
                    attempts: 5,
                    window: mixengine_proto::Millis::from_secs(300),
                    tail: vec!["Address already in use".to_owned()],
                }),
            }),
            blocked: vec![id("worker")],
        };

        let rendered = service_walk(Walked::Start, &walk);
        let lines: Vec<&str> = rendered.lines().collect();

        // The name of the thing to fix is the first thing on the screen, and the evidence is
        // directly under it — five lines of `started` above both would be five lines in the way.
        assert_eq!(lines[0], "web failed to start: 5 failed starts within 5m");
        assert_eq!(lines[1], "    Address already in use");
        assert_eq!(lines[2], "  started   db");
        assert_eq!(lines[3], "  blocked   worker");
    }

    /// **A refused superuser is shown with the command that repairs it** — roadmap task **T127a**.
    ///
    /// `mix` naming one of `mix`'s own verbs is not business logic in a client: the daemon said what
    /// happened, and which command undoes it is the thing only this binary knows. A `ServiceWalk`
    /// carries no hint field, so this is where it can be said at all.
    #[test]
    fn a_refused_superuser_is_shown_with_the_repair_under_it() {
        let walk = ServiceWalk {
            planned: vec![id("postgres@main")],
            complete: true,
            reached: Vec::new(),
            failed: Some(mixengine_proto::ServiceFailure {
                service: id("postgres@main"),
                detail: None,
                reason: Some(StateReason::SuperuserRefused {
                    said: "FATAL:  password authentication failed for user \"postgres\"".to_owned(),
                }),
            }),
            blocked: Vec::new(),
        };

        let rendered = service_walk(Walked::Start, &walk);
        let lines: Vec<&str> = rendered.lines().collect();

        assert_eq!(
            lines[0],
            "postgres@main failed to start: it refuses the superuser password this home holds"
        );
        assert_eq!(
            lines[1],
            "    FATAL:  password authentication failed for user \"postgres\""
        );
        assert!(
            lines[2].contains("mix service reset-credential postgres@main"),
            "{rendered}"
        );
    }

    #[test]
    fn a_walk_nobody_waited_for_says_it_is_still_happening() {
        let accepted = ServiceWalk {
            planned: vec![id("db")],
            complete: false,
            reached: Vec::new(),
            failed: None,
            blocked: Vec::new(),
        };

        assert_eq!(
            service_walk(Walked::Restart, &accepted),
            "accepted; mixengined is restarting db in the background\n"
        );
    }

    #[test]
    fn a_shutdown_says_what_happened_to_the_daemon_and_puts_the_services_under_it() {
        let shutdown = DaemonShutdown {
            services: ServiceWalk {
                planned: vec![id("web"), id("db")],
                complete: true,
                reached: vec![id("web"), id("db")],
                failed: None,
                blocked: Vec::new(),
            },
            unordered: None,
        };

        assert_eq!(
            daemon_shutdown(&shutdown),
            "mixengined is stopping\n  stopped web, db\n"
        );
    }

    #[test]
    fn a_shutdown_with_nothing_to_stop_is_one_line_about_the_daemon() {
        // And specifically not `service_walk`'s "this home declares no services", which answers a
        // question about services that nobody asked here.
        let quiet = DaemonShutdown {
            services: ServiceWalk {
                planned: Vec::new(),
                complete: true,
                reached: Vec::new(),
                failed: None,
                blocked: Vec::new(),
            },
            unordered: None,
        };

        assert_eq!(daemon_shutdown(&quiet), "mixengined is stopping\n");
    }

    /// The same empty walk, and the opposite thing to say about it — which is the whole reason
    /// `unordered` is on the wire at all.
    #[test]
    fn a_shutdown_that_could_not_be_ordered_is_told_apart_from_one_with_nothing_to_stop() {
        let skipped = DaemonShutdown {
            services: ServiceWalk {
                planned: Vec::new(),
                complete: true,
                reached: Vec::new(),
                failed: None,
                blocked: Vec::new(),
            },
            unordered: Some(
                mixengine_proto::Error::new(
                    mixengine_proto::ErrorCode::Internal,
                    "cannot read the declarations in /home/dev/extensions/mailpit/extension.toml",
                )
                .with_hint("`logs/daemon.log` has the detail a report needs"),
            ),
        };

        let rendered = daemon_shutdown(&skipped);
        let lines: Vec<&str> = rendered.lines().collect();

        // The daemon still went, and that is still the headline: what follows is why the stop was
        // not the one the user was promised.
        assert_eq!(lines[0], "mixengined is stopping");
        assert!(
            lines[1].contains("were not stopped in dependency order"),
            "{rendered}"
        );
        assert_eq!(
            lines[2],
            "  cannot read the declarations in /home/dev/extensions/mailpit/extension.toml",
            "the daemon's own sentence, which names the file to fix: {rendered}"
        );
        assert_eq!(
            lines[3], "  hint: `logs/daemon.log` has the detail a report needs",
            "{rendered}"
        );
    }

    #[test]
    fn a_service_that_would_not_stop_is_named_although_the_daemon_stopped_anyway() {
        // T18's one failure: a survivor adopted from a previous daemon that will not die. The daemon
        // goes regardless — refusing would leave a user with no way out — so the report is the whole
        // of what tells them the port is still held.
        let refused = DaemonShutdown {
            services: ServiceWalk {
                planned: vec![id("db")],
                complete: true,
                reached: Vec::new(),
                failed: Some(mixengine_proto::ServiceFailure {
                    service: id("db"),
                    detail: None,
                    reason: None,
                }),
                blocked: Vec::new(),
            },
            unordered: None,
        };

        let rendered = daemon_shutdown(&refused);
        assert!(
            rendered.starts_with("mixengined is stopping\n"),
            "{rendered}"
        );
        assert!(rendered.contains("db failed to stop"), "{rendered}");
    }

    #[test]
    fn an_age_is_rounded_the_way_an_uptime_is_and_never_reads_as_negative() {
        let started = Timestamp(1_723_000_000_000);
        let now = |offset: i64| {
            std::time::UNIX_EPOCH
                + std::time::Duration::from_millis((started.0 + offset).unsigned_abs())
        };

        assert_eq!(ago(started, now(812_000)), "13m 32s ago");
        assert_eq!(ago(started, now(500)), "just now");

        // A clock that went backwards between the start and this call. Rendering "-4s ago" would
        // make a user doubt the service rather than the clock.
        assert_eq!(ago(started, now(-4_000)), "just now");
    }

    #[test]
    fn uptime_stops_at_two_units_whichever_two_they_are() {
        assert_eq!(uptime(Uptime(0)), "0s");
        assert_eq!(uptime(Uptime(59)), "59s");
        assert_eq!(uptime(Uptime(60)), "1m 0s");
        assert_eq!(uptime(Uptime(3_599)), "59m 59s");
        assert_eq!(uptime(Uptime(3_600)), "1h 0m");
        assert_eq!(uptime(Uptime(86_399)), "23h 59m");
        assert_eq!(uptime(Uptime(86_400)), "1d 0h");
        assert_eq!(uptime(Uptime(9_000_000)), "104d 4h");
    }

    fn a_pending_probe(id: i64) -> mixengine_proto::PendingOp {
        let op = mixengine_proto::privileged::PrivilegedOp::Probe {};

        mixengine_proto::PendingOp {
            id: mixengine_proto::PendingOpId(id),
            description: op.describe(),
            op,
            requested_at: mixengine_proto::Timestamp(1_760_000_000_000),
        }
    }

    /// The screen T64 will build on: every operation, and what each will literally change, before
    /// anybody raises a prompt.
    #[test]
    fn a_pending_list_says_what_each_operation_will_change() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: vec![a_pending_probe(1), a_pending_probe(2)],
            last: None,
        });

        assert!(rendered.contains("2 operations are waiting"), "{rendered}");
        assert!(rendered.contains("mix elevation grant"), "{rendered}");
        assert!(
            rendered.contains(&mixengine_proto::privileged::PrivilegedOp::Probe {}.describe()),
            "{rendered}"
        );
    }

    /// T64's screen: the same list, and one sentence about the prompt that is about to be raised.
    ///
    /// The assertion that matters is the negative one. `mix elevation status` ends by telling a
    /// person to run `mix elevation grant`; this is printed *by* that command, so repeating the
    /// advice would be telling somebody to run what they are already running.
    #[test]
    fn the_screen_before_a_prompt_is_the_list_and_what_the_prompt_will_be() {
        let rendered = elevation_prompt(&ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: vec![a_pending_probe(1), a_pending_probe(2)],
            last: None,
        });

        assert!(rendered.contains("2 operations are waiting"), "{rendered}");
        assert!(
            rendered.contains(&mixengine_proto::privileged::PrivilegedOp::Probe {}.describe()),
            "{rendered}"
        );
        assert!(rendered.contains("once"), "{rendered}");
        assert!(!rendered.contains("mix elevation grant"), "{rendered}");
    }

    /// T88a. The sentence about an old helper comes from the daemon, so what this asserts is that
    /// it reaches the screen — not what it says.
    #[test]
    fn an_old_privileged_helper_is_reported_where_the_queue_is() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: Some(mixengine_proto::InstalledHelper {
                version: "0.1.0".to_owned(),
                protocol: 1,
                supported_ops: vec!["probe".to_owned()],
                upgrade: Some("run this release's installer".to_owned()),
            }),
            pending: Vec::new(),
            last: None,
        });

        assert!(rendered.contains("helper"), "{rendered}");
        assert!(
            rendered.contains("run this release's installer"),
            "{rendered}"
        );
    }

    /// And a helper this build is happy with says nothing at all, which is the ordinary machine.
    #[test]
    fn a_current_privileged_helper_is_not_mentioned() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: Some(mixengine_proto::InstalledHelper {
                version: "0.2.0".to_owned(),
                protocol: 1,
                supported_ops: vec!["probe".to_owned(), "helper-replace".to_owned()],
                upgrade: None,
            }),
            pending: Vec::new(),
            last: None,
        });

        assert!(!rendered.contains("helper"), "{rendered}");
    }

    /// A machine that cannot prompt has to print the reason, because on Linux the reason is the
    /// command a person is meant to type.
    #[test]
    fn a_machine_that_cannot_prompt_prints_what_to_do_instead() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: false,
            reason: Some(
                "no polkit agent; run: pkexec /opt/mixengine/mixengine-elevate /…".to_owned(),
            ),
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: vec![a_pending_probe(1)],
            last: None,
        });

        assert!(rendered.contains("pkexec"), "{rendered}");
        assert!(
            !rendered.contains("mix elevation grant"),
            "offering a command that cannot work: {rendered}"
        );
    }

    /// A decline is a normal outcome, so it reads as one — and the list stays, which is the whole of
    /// the degraded mode a person sees.
    #[test]
    fn a_declined_grant_reads_as_a_choice_rather_than_a_failure() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: vec![a_pending_probe(1)],
            last: Some(mixengine_proto::GrantOutcome {
                job: mixengine_proto::JobId(4),
                at: mixengine_proto::Timestamp(1_760_000_000_000),
                outcome: mixengine_proto::privileged::ElevationOutcome::Declined,
                applied: 0,
                still_pending: 1,
                problems: Vec::new(),
            }),
        });

        assert!(rendered.contains("declined"), "{rendered}");
        assert!(!rendered.to_lowercase().contains("error"), "{rendered}");
    }

    /// Whichever mechanism is running, `mix status` says which — and a home on the hosts file is
    /// told what it is missing rather than left to discover it on the first subdomain.
    #[test]
    fn the_status_line_names_the_mechanism_this_home_resolves_through() {
        let hosts_only = status(&example());
        assert!(hosts_only.contains("names     hosts file"), "{hosts_only}");
        assert!(hosts_only.contains("no wildcards"), "{hosts_only}");
        assert!(hosts_only.contains("[dns] enabled"), "{hosts_only}");

        let on_dns = DaemonStatus {
            dns: Some(mixengine_proto::DnsStatus {
                mode: DnsMode::Dns,
                listening: Some("127.0.0.1:53535".to_owned()),
                wildcards: vec!["test".to_owned(), "localhost".to_owned()],
                because: None,
            }),
            ..example()
        };

        let rendered = status(&on_dns);
        assert!(
            rendered.contains("names     DNS on 127.0.0.1:53535"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wildcards for *.test, *.localhost"),
            "{rendered}"
        );
    }

    /// `mix status` says it in one line, without a second round trip and without deciding for
    /// itself what degraded means.
    #[test]
    fn the_status_line_says_how_many_are_waiting_and_whether_the_daemon_is_elevated() {
        let waiting = DaemonStatus {
            elevation: Some(mixengine_proto::ElevationSummary {
                elevated: true,
                can_prompt: true,
                pending: 3,
            }),
            ..example()
        };

        let rendered = status(&waiting);

        assert!(rendered.contains("3 operations are waiting"), "{rendered}");
        assert!(rendered.contains("administrative token"), "{rendered}");
    }
    /// A report for `fakeservice@main` with `source` and nothing else varied.
    fn idle_report(source: mixengine_proto::IdleSource) -> IdleReport {
        IdleReport {
            service: mixengine_proto::ServiceId::parse("fakeservice@main").expect("an id"),
            policy: None,
            source,
            exempt: Vec::new(),
        }
    }

    /// The three ways a service is never idle-stopped read as three different sentences.
    ///
    /// **The one that matters is `Unmeasurable`**, which was found by a failing test rather than
    /// designed: a php-fpm pool on a Unix socket has no port to count, so a person who has just
    /// typed `--after 30m` would otherwise be told only "never" — the outcome without the reason,
    /// which is an invitation to type it again.
    #[test]
    fn the_three_ways_of_never_idling_are_three_different_sentences() {
        let unset = service_idle(&idle_report(mixengine_proto::IdleSource::Unset));
        let never = service_idle(&idle_report(mixengine_proto::IdleSource::Never));
        let unmeasurable = service_idle(&idle_report(mixengine_proto::IdleSource::Unmeasurable));

        for rendered in [&unset, &never, &unmeasurable] {
            assert!(rendered.contains("never"), "{rendered}");
        }

        assert!(unset.contains("no default yet"), "{unset}");
        assert!(never.contains("switched off"), "{never}");
        assert!(
            unmeasurable.contains("nothing to measure"),
            "asked for and unmeasurable is not the same answer as nobody asking: {unmeasurable}"
        );
    }

    /// An exemption names the thing a person would have to go and change.
    #[test]
    fn an_exemption_names_what_is_holding_the_service_open() {
        let report = IdleReport {
            exempt: vec![
                IdleExemption::DependentRunning {
                    service: mixengine_proto::ServiceId::parse("php-fpm@8.3").expect("an id"),
                },
                IdleExemption::ProjectKeptWarm {
                    project: "shop".to_owned(),
                },
            ],
            ..idle_report(mixengine_proto::IdleSource::Row)
        };

        let rendered = service_idle(&report);

        assert!(rendered.contains("php-fpm@8.3"), "{rendered}");
        assert!(rendered.contains("shop"), "{rendered}");
    }

    /// It says where the credential is and never what it is — the T77a design, D11. The test is the
    /// guard: a renderer that grew a password would put one in a terminal's scrollback.
    #[test]
    fn a_created_database_says_where_the_password_lives() {
        let rendered = database_created(&blog_account(), None);

        assert!(
            rendered.contains("database blog created on mariadb@main"),
            "{rendered}"
        );
        assert!(
            rendered.contains("account  blog already existed"),
            "{rendered}"
        );
        assert!(rendered.contains("mariadb@main/blog"), "{rendered}");
        assert!(
            !rendered.to_ascii_lowercase().contains("password is"),
            "{rendered}"
        );
    }

    fn blog_account() -> DatabaseAccount {
        DatabaseAccount {
            service: ServiceId::parse("mariadb@main").expect("an id"),
            database: "blog".to_owned(),
            user: "blog".to_owned(),
            secret: SecretAddress::of("mariadb@main/blog"),
            made: mixengine_proto::Provisioned {
                database: Made::Created,
                user: Made::Existing,
            },
        }
    }

    /// T77b, D3: the last line is the password and nothing else, so `| tail -1` hands a script the
    /// value a client accepts. It carried the block's two-space indent until the T202 walk piped it
    /// into `mariadb -p` and was refused.
    #[test]
    fn the_last_line_of_the_credentials_is_the_password_alone() {
        let answer = DatabaseCredentials {
            service: ServiceId::parse("mariadb@main").expect("an id"),
            user: "blog".to_owned(),
            secret: SecretAddress::of("mariadb@main/blog"),
            password: "s3cr3t-value".to_owned(),
        };

        let rendered = database_credentials(&answer, None);

        assert_eq!(
            rendered.lines().last(),
            Some("s3cr3t-value"),
            "{rendered:?}"
        );
    }

    /// T194, D5: a home that keeps its passwords in a file says so where it says where one is.
    #[test]
    fn a_database_in_a_file_store_says_so() {
        let home = Some(mixengine_proto::CredentialStore::Home);

        let created = database_created(&blog_account(), home);
        assert!(
            created.contains("this home's credentials file at mariadb@main/blog"),
            "{created}"
        );

        let answer = DatabaseCredentials {
            service: ServiceId::parse("mariadb@main").expect("an id"),
            user: "blog".to_owned(),
            secret: SecretAddress::of("mariadb@main/blog"),
            password: "p".to_owned(),
        };
        let told = database_credentials(&answer, home);
        assert!(
            told.contains("this home's credentials file at mariadb@main/blog"),
            "{told}"
        );
        let untold = database_credentials(&answer, None);
        assert!(untold.contains("mixengine credentials"), "{untold}");
    }

    /// T96. Every row says what would take it back, so a person reading the table never has to know
    /// which of five directories is safe to empty.
    #[test]
    fn the_disk_table_says_what_would_reclaim_each_row() {
        let usage = DiskUsage {
            root: "/home/a/.mixengine".to_owned(),
            measured_at: Timestamp::from_system_time(std::time::UNIX_EPOCH),
            categories: vec![
                CategoryUsage {
                    id: DiskCategory::Runtimes,
                    location: "/home/a/.mixengine/runtimes".to_owned(),
                    bytes: 700 << 20,
                    files: 40_000,
                    reclaim: Reclaim::ByMethod {
                        method: "runtime.uninstall".to_owned(),
                        because: "one at a time".to_owned(),
                    },
                    unreadable: None,
                },
                CategoryUsage {
                    id: DiskCategory::Cache,
                    location: "/home/a/.mixengine/cache".to_owned(),
                    bytes: 90 << 20,
                    files: 12,
                    reclaim: Reclaim::ByCleanup {
                        bytes: 90 << 20,
                        files: 12,
                    },
                    unreadable: None,
                },
            ],
            other_bytes: 10 << 20,
        };

        let rendered = disk_usage(&usage);

        assert!(rendered.contains("runtimes"), "{rendered}");
        assert!(rendered.contains("700 MiB"), "{rendered}");
        assert!(rendered.contains("runtime.uninstall"), "{rendered}");
        assert!(rendered.contains("other"), "{rendered}");
        assert!(rendered.contains("10 MiB"), "{rendered}");
        assert!(rendered.contains("mix cleanup"), "{rendered}");
        assert!(rendered.contains("90 MiB"), "{rendered}");

        // The summary line sits under the column it summarises, whatever the longest label is.
        assert!(rendered.contains("\ntotal     800 MiB\n"), "{rendered}");
    }

    /// T96. A category that could only be read in part says so in the table, not only in the JSON: a
    /// figure that is a floor and reads as a total is the one thing this must not do.
    #[test]
    fn an_unreadable_category_says_so_in_the_table() {
        let usage = DiskUsage {
            root: "/home/a/.mixengine".to_owned(),
            measured_at: Timestamp::from_system_time(std::time::UNIX_EPOCH),
            categories: vec![CategoryUsage {
                id: DiskCategory::Data,
                location: "/mnt/bulk/data".to_owned(),
                bytes: 0,
                files: 0,
                reclaim: Reclaim::Never {
                    because: "these are your databases".to_owned(),
                },
                unreadable: Some("1 entry could not be read (permission denied)".to_owned()),
            }],
            other_bytes: 0,
        };

        let rendered = disk_usage(&usage);

        assert!(rendered.contains("could not be read"), "{rendered}");
        assert!(
            rendered.contains("there is nothing `mix cleanup` would take back"),
            "{rendered}"
        );
    }

    /// T96. A row appears whatever it answered, so a person can tell "there were no rotated logs"
    /// from "the logs were not looked at" — `uninstall_report`'s rule, and its reason.
    #[test]
    fn a_cleanup_prints_every_row_and_ends_on_the_total() {
        let report = CleanupReport {
            items: vec![
                Cleaned {
                    id: DiskCategory::Logs,
                    location: "/home/a/.mixengine/logs".to_owned(),
                    outcome: Cleanup::Kept {
                        because: "you asked for the logs to be left".to_owned(),
                    },
                },
                Cleaned {
                    id: DiskCategory::Cache,
                    location: "/home/a/.mixengine/cache".to_owned(),
                    outcome: Cleanup::Reclaimed {
                        files: 12,
                        bytes: 90 << 20,
                    },
                },
            ],
        };

        let rendered = cleanup_report(&report);

        assert!(rendered.contains("kept"), "{rendered}");
        assert!(
            rendered.contains("you asked for the logs to be left"),
            "{rendered}"
        );
        assert!(rendered.contains("90 MiB"), "{rendered}");
        assert!(rendered.contains("took back 90 MiB"), "{rendered}");
    }

    /// T96. A file that would not go is named, because a total that quietly excluded it would be a
    /// cleanup somebody runs twice wondering why the number never moves.
    #[test]
    fn a_partial_cleanup_says_what_would_not_go() {
        let report = CleanupReport {
            items: vec![Cleaned {
                id: DiskCategory::Logs,
                location: "/l".to_owned(),
                outcome: Cleanup::Partial {
                    files: 1,
                    bytes: 1 << 20,
                    left_behind: 2,
                    because: "/l/daemon.log.1: the file is open".to_owned(),
                },
            }],
        };

        let rendered = cleanup_report(&report);

        assert!(rendered.contains("LEFT"), "{rendered}");
        assert!(rendered.contains("2 file(s)"), "{rendered}");
        assert!(rendered.contains("the file is open"), "{rendered}");
    }

    /// **T204, D4.** An export names the entries it left, and says nothing when there are none.
    #[test]
    fn an_export_names_the_site_entries_it_kept() {
        let mut exported = ProjectExport {
            path: "/srv/blog/mixengine.toml".to_owned(),
            created: false,
            sites_kept: Vec::new(),
        };
        assert!(!project_export(&exported).contains("kept"));

        exported.sites_kept = vec!["old.test".to_owned()];
        assert!(
            project_export(&exported)
                .contains("kept old.test in mixengine.toml, though no site here has that name"),
            "{}",
            project_export(&exported)
        );
    }

    /// **T204, D7.** The declared sites print even with no pins, each with what to do.
    #[test]
    fn a_project_lists_the_sites_its_manifest_declares() {
        let detail = ProjectDetail {
            project: mixengine_proto::ProjectSummary {
                name: "shop".to_owned(),
                root: "/srv/shop".to_owned(),
                created_at: "2026-10-08T00:00:00Z".to_owned(),
                manifest: Some("/srv/shop/mixengine.toml".to_owned()),
                keep_warm: false,
            },
            pins: Vec::new(),
            declared_sites: vec![
                mixengine_proto::DeclaredSite {
                    domain: "web.test".into(),
                    aliases: Vec::new(),
                    state: mixengine_proto::DeclaredSiteState::Here,
                },
                mixengine_proto::DeclaredSite {
                    domain: "api.test".into(),
                    aliases: Vec::new(),
                    state: mixengine_proto::DeclaredSiteState::Missing,
                },
                mixengine_proto::DeclaredSite {
                    domain: "taken.test".into(),
                    aliases: Vec::new(),
                    state: mixengine_proto::DeclaredSiteState::Elsewhere {
                        owner: "blog".into(),
                    },
                },
            ],
        };

        let out = project_detail(&detail);

        assert!(out.contains("no runtimes are pinned"), "{out}");
        assert!(out.contains("mix site create --from api.test"), "{out}");
        assert!(out.contains("blog holds it"), "{out}");
    }
}

#[cfg(test)]
mod autostart_tests {
    use mixengine_proto::{AutostartMechanism, AutostartReport};

    use super::{Autostarted, autostart_report};

    fn registered(for_this_home: bool) -> AutostartReport {
        AutostartReport {
            mechanism: AutostartMechanism::SystemdUser,
            location: "/home/me/.config/systemd/user/mixengined.service".to_owned(),
            enabled: true,
            changed: false,
            command: vec![
                "/usr/bin/mixengined".to_owned(),
                "--home".to_owned(),
                match for_this_home {
                    true => "/home/me/.local/share/mixengine".to_owned(),
                    false => "/home/me/other".to_owned(),
                },
            ],
            for_this_home,
        }
    }

    /// The half-state the whole `for_this_home` field exists for.
    #[test]
    fn an_entry_for_another_home_is_not_reported_as_set_up() {
        let rendered = autostart_report(Autostarted::Asked, &registered(false));

        assert!(rendered.contains("another home"), "{rendered}");
        assert!(rendered.contains("/home/me/other"), "{rendered}");
        assert!(
            !rendered.contains("this home's daemon starts"),
            "{rendered}"
        );
    }

    #[test]
    fn an_entry_for_this_home_reads_as_set_up_and_names_what_it_starts() {
        let rendered = autostart_report(Autostarted::Asked, &registered(true));

        assert!(rendered.contains("starts when you log in"), "{rendered}");
        assert!(
            rendered.contains("/usr/bin/mixengined --home"),
            "{rendered}"
        );
    }

    #[test]
    fn an_enable_that_wrote_nothing_does_not_claim_to_have_written() {
        let rendered = autostart_report(Autostarted::Enabled, &registered(true));

        assert!(rendered.contains("already"), "{rendered}");
    }

    #[test]
    fn an_enable_that_wrote_says_when_it_takes_effect() {
        let mut report = registered(true);
        report.changed = true;

        let rendered = autostart_report(Autostarted::Enabled, &report);

        assert!(rendered.contains("will now start"), "{rendered}");
        assert!(rendered.contains("next login"), "{rendered}");
    }

    #[test]
    fn a_machine_with_no_mechanism_says_there_is_nothing_to_register() {
        let nothing = AutostartReport {
            mechanism: AutostartMechanism::None,
            location: "no systemd user manager on this machine".to_owned(),
            enabled: false,
            changed: false,
            command: Vec::new(),
            for_this_home: false,
        };

        let rendered = autostart_report(Autostarted::Asked, &nothing);

        assert!(rendered.contains("no systemd user manager"), "{rendered}");
        assert!(rendered.contains("nothing to register"), "{rendered}");
    }
}

/// `mix storage` — where this home's four growing directories are, and whether that can change.
///
/// **The relocated ones are marked and the rest are not**, rather than a column saying "default" on
/// four lines out of four on almost every machine. What a person is looking for here is which of
/// them is somewhere else.
pub(crate) fn storage(report: &StorageReport) -> String {
    let mut rendered = format!("  home       {}\n", report.root);

    let paths = &report.paths;
    for (name, directory) in [
        ("runtimes", &paths.runtimes),
        ("packages", &paths.packages),
        ("data", &paths.data),
        ("logs", &paths.logs),
    ] {
        rendered.push_str(&format!(
            "  {name:<9}  {}{}\n",
            directory.path,
            if directory.relocated { " (moved)" } else { "" }
        ));
    }

    rendered.push_str(&match &report.changeable {
        StorageChoice::Free => "\n  nothing is installed yet, so these may still be moved: start \
                                the daemon with --runtimes, --packages, --data or --logs\n"
            .to_owned(),

        // The daemon's own sentence, for the reason every other rendering here uses one: what is
        // installed is a fact it measured, and a client restating it would be a second answer.
        StorageChoice::Taken { explanation, .. } => format!(
            "\n  {explanation}, so these can no longer be moved by a flag; moving them means \
             moving the files and rewriting what the database records about them\n"
        ),
    });

    rendered
}

#[cfg(test)]
mod grant_problems {
    use mixengine_proto::UpgradeEntry;

    use super::*;

    /// **A grant that did nothing says why** — roadmap task **T147**.
    ///
    /// The regression this pins is not a crash. It is a person granting the same operation eight
    /// times because the only thing they were shown was *0 applied, 1 still waiting*, while the
    /// sentence that would have stopped them sat in a log file.
    #[test]
    fn what_did_not_happen_is_printed_under_the_line() {
        let outcome = GrantOutcome {
            job: mixengine_proto::JobId(8),
            at: mixengine_proto::Timestamp(1_760_000_000_000),
            outcome: mixengine_proto::privileged::ElevationOutcome::Completed,
            applied: 0,
            still_pending: 1,
            problems: vec![
                "helper-install: cannot read /Volumes/SSD/mixengine-elevate: Operation not \
                 permitted (os error 1)"
                    .to_owned(),
            ],
        };

        let rendered = grant(&outcome);

        assert!(
            rendered.contains("0 applied, 1 still waiting"),
            "{rendered}"
        );
        assert!(rendered.contains("helper-install"), "{rendered}");
        assert!(rendered.contains("Operation not permitted"), "{rendered}");
    }

    /// A grant with nothing to report is the line it always was, with nothing appended.
    #[test]
    fn a_grant_that_did_everything_gains_no_lines() {
        let outcome = GrantOutcome {
            job: mixengine_proto::JobId(1),
            at: mixengine_proto::Timestamp(1_760_000_000_000),
            outcome: mixengine_proto::privileged::ElevationOutcome::Completed,
            applied: 2,
            still_pending: 0,
            problems: Vec::new(),
        };

        assert_eq!(grant(&outcome), "job #1: 2 applied, 0 still waiting");
    }
    fn a_plan(old: OldVersion, outcome: UpgradeOutcome) -> UpgradePlan {
        UpgradePlan {
            subject: "php".to_owned(),
            from: PackageVersion::parse("8.4.24").expect("a fixture"),
            to: PackageVersion::parse("8.4.25").expect("a fixture"),
            to_installed: false,
            bytes: 31_457_280,
            stale: false,
            needs: Vec::new(),
            entries: vec![
                UpgradeEntry {
                    item: UpgradeItem::Default {},
                    outcome: outcome.clone(),
                },
                UpgradeEntry {
                    item: UpgradeItem::Site {
                        site: "blog.test".to_owned(),
                        from: ServiceId::parse("php-fpm@8.4.24").expect("a fixture"),
                        to: ServiceId::parse("php-fpm@8.4.25").expect("a fixture"),
                    },
                    outcome,
                },
            ],
            old,
        }
    }

    #[test]
    fn a_plan_says_what_moves_and_what_becomes_of_the_old_version() {
        let rendered = upgrade_plan(&a_plan(
            OldVersion::WillBeRemoved {},
            UpgradeOutcome::Planned {},
        ));

        assert!(rendered.starts_with("php 8.4.24 → 8.4.25"), "{rendered}");
        assert!(rendered.contains("to download"), "{rendered}");
        assert!(
            rendered.contains("blog.test moves from php-fpm@8.4.24 to php-fpm@8.4.25"),
            "{rendered}"
        );
        assert!(
            rendered.contains("8.4.25 becomes the default php"),
            "{rendered}"
        );
        assert!(
            rendered.contains("php 8.4.24 will be removed"),
            "{rendered}"
        );
        assert!(rendered.contains("--keep"), "{rendered}");
    }

    #[test]
    fn a_plan_kept_for_a_reason_names_the_reason() {
        let rendered = upgrade_plan(&a_plan(
            OldVersion::WillBeKept {
                because: vec!["blog pins php 8.4.24 in /work/blog/mixengine.toml".to_owned()],
            },
            UpgradeOutcome::Planned {},
        ));
        assert!(rendered.contains("will be kept"), "{rendered}");
        assert!(rendered.contains("/work/blog/mixengine.toml"), "{rendered}");
    }

    #[test]
    fn a_finished_update_marks_each_line() {
        let rendered = upgrade_plan(&a_plan(
            OldVersion::Kept {
                because: vec!["the front end refused the sites".to_owned()],
            },
            UpgradeOutcome::Skipped {},
        ));
        assert!(rendered.contains("(not done)"), "{rendered}");
        assert!(
            rendered.contains("php 8.4.24 was kept: the front end refused the sites"),
            "{rendered}"
        );
    }
}
