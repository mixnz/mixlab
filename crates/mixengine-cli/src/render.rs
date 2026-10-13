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

pub(crate) mod catalogue;
pub(crate) mod elevation;
pub(crate) mod prerequisites;
pub(crate) mod runtime;
pub(crate) mod service;
mod size;
mod table;
mod time;
pub(crate) mod upgrades;

use catalogue::{
    Lines, MORE_HEADING, NEEDS_HEADING, RUNS_HEADING, any_lacking, emulation_column, more, needs,
    runs, update_lines,
};
use elevation::{grant, operations};
use runtime::runtime_summary;
use service::{Walked, names, service_walk};
use size::size;
use table::{MISSING, table, table_of};
use time::{ago, in_time, uptime};

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
    DomainStatusReport, ExtensionCatalogue, ExtensionInspection, ExtensionKind, ExtensionPlan,
    ExtensionRemoval, FilesystemReach, GrantOutcome, Handshake, InstalledExtensions, IssueOutcome,
    JobList, JobOutcome, JobState, JobSummary, Launch, Made, MetricsFrame, MetricsHistory,
    NetworkReach, Outcome, PROTOCOL_VERSION, PackageCatalogue, PackageList, PackageRelease,
    PackageRemoval, PackageVersion, PathReport, PinSource, PlanAction, PlanStep, ProjectDetail,
    ProjectExport, ProjectList, ProjectRemoval, RecipeAddition, Reclaim, Removal, RepairReport,
    RotateOutcome, RuntimeSummary, ServiceId, ServiceList, SignatureCheck, SiteDetail, SiteKind,
    SiteList, SiteOwner, SiteRemoval, SiteSharing, StepResult, StorageChoice, StorageReport, Trust,
    UninstallOutcome, UninstallReport, Unusable, UpdateApplied, UpdateHandedOver, UpdatePlacement,
    UpdateStatus, Verdict,
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
///
/// [`service_walk`]: crate::render::service::service_walk
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
                // **Gone reads as gone** — roadmap task **T206d**, D4.
                match package.is_missing() {
                    true => "missing".to_owned(),
                    false => ago(package.installed_at, now),
                },
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

    let mut said = table(
        ["PACKAGE", "VERSION", "SIZE", "INSTALLED", "SERVICES"],
        &rows,
    );
    if let Some(gone) = list.packages.iter().find(|package| package.is_missing()) {
        let (name, version) = (&gone.package, &gone.version);
        said.push_str(&format!(
            "{name} {version}'s folder is gone: `mix package install {name} {version}` puts it \
             back; `mix package uninstall {name} {version}` forgets it\n"
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

/// `mix package uninstall`, for a person.
#[must_use]
pub(crate) fn package_removal(removal: &PackageRemoval) -> String {
    format!(
        "removed {} {}
",
        removal.removed.package, removal.removed.version
    )
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
///
/// [`runtime_summary`]: crate::render::runtime::runtime_summary
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

/// "1 operation is" / "3 operations are", so a sentence built from a count reads.
fn services(count: usize) -> String {
    match count {
        1 => "1 service".to_owned(),
        many => format!("{many} services"),
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
///
/// [`ago`]: crate::render::time::ago
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
///
/// [`size`]: crate::render::size::size
fn memory(bytes: u64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "one decimal place of mebibytes")]
    let mib = bytes as f64 / (1u64 << 20) as f64;

    format!("{mib:.1} MiB")
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

/// This build of `mix`, in the shape the daemon reports itself in.
fn client() -> serde_json::Value {
    serde_json::to_value(DaemonVersion {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: PROTOCOL_VERSION,
    })
    .expect("a DaemonVersion of two owned fields always serialises")
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

    // **T205, D6.** What the project's blueprint says is left to do.
    if let Some(steps) = &detail.next_steps {
        out.push_str(&next_steps_said(steps));
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

    let actions: Vec<&PlanAction> = plan.steps.iter().map(|step| &step.action).collect();

    for (position, step) in plan.steps.iter().enumerate() {
        out.push_str(&format!(
            "  {:<11} {}
",
            disposition_word(&step.disposition),
            with_site(step_said(step), &actions, position)
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

    let actions: Vec<&PlanAction> = applied.steps.iter().map(|step| &step.action).collect();

    for (position, step) in applied.steps.iter().enumerate() {
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
            with_site(action_said(&step.action), &actions, position)
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

    if let Some(steps) = &applied.next_steps {
        out.push_str(&next_steps_said(steps));
    }

    out
}

/// A `site` line with the site it makes — roadmap task **T204a**, D7.
///
/// Read off the step after it, as the daemon's executor reads it: a site's names are the `domain`
/// steps straight after it, so a plan with two sites reads as two rather than as one long run of
/// names under two identical lines.
fn with_site(said: String, actions: &[&PlanAction], position: usize) -> String {
    match (actions.get(position), actions.get(position + 1)) {
        (Some(PlanAction::CreateSite { .. }), Some(PlanAction::AddDomain { domain, .. })) => {
            said.replacen("site ", &format!("site {domain}: "), 1)
        }
        _ => said,
    }
}

/// The steps a blueprint left, one line each — roadmap task **T205**, D6.
fn next_steps_said(steps: &mixengine_proto::NextSteps) -> String {
    use mixengine_proto::NextStepKind;

    let mut out = String::from("\nNext, in the project directory:\n");

    for step in &steps.steps {
        let what = match step.kind {
            NextStepKind::Open => format!("open {}", step.path.as_deref().unwrap_or("/")),
            _ => step.run.clone().unwrap_or_default(),
        };

        // **Which site it is for**, when the blueprint has several — roadmap task **T204a**, D7.
        let site = step.site.as_ref().map(|site| format!("({site})"));

        let mut tail = Vec::new();
        if let Some(site) = &site {
            tail.push(site.as_str());
        }
        if step.kind == NextStepKind::Serve {
            tail.push("(keeps running)");
        }
        if step.optional {
            tail.push("(optional)");
        }
        if let Some(note) = step.note.as_deref() {
            tail.push(note);
        }

        match tail.is_empty() {
            true => out.push_str(&format!("  {what}\n")),
            false => out.push_str(&format!("  {what}   {}\n", tail.join(" "))),
        }
    }

    // **D12**: the same file the scaffold came from, so the same caution.
    if !steps.trusted {
        out.push_str(
            "These come from a blueprint nobody vouches for; read them before running them.\n",
        );
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
        PlanAction::FetchArchive { url, .. } => format!("download and unpack {url}"),
        PlanAction::WriteDotenv { key, path } => {
            format!("write {key} to {path}, with the database password")
        }
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
        CategoryUsage, Cleaned, DiskCategory, MetricsMinute, MetricsSample, MetricsSubject,
        PortWish, RuntimeKind, SecretAddress, ServiceState, ServiceWalk, StepOutcome, Timestamp,
        Uptime, VersionConstraint,
    };

    use super::service::tests::{failed_with, id, summary};
    use super::*;

    /// The same for a package — T206d, D4.
    #[test]
    fn a_package_whose_folder_is_gone_reads_missing_and_names_the_way_back() {
        let package = mixengine_proto::PackageSummary {
            package: "msys2".to_owned(),
            version: mixengine_proto::PackageVersion::parse("2026.10.08").expect("a version"),
            path: "/h/packages/msys2/2026.10.08".to_owned(),
            installed_at: Timestamp(1_760_000_000_000),
            bytes: 1,
            services: Vec::new(),
            missing: Some(true),
        };
        let said = package_list(&mixengine_proto::PackageList {
            packages: vec![package],
        });
        assert!(said.contains("missing"), "{said}");
        assert!(
            said.contains("`mix package install msys2 2026.10.08` puts it back"),
            "{said}"
        );
        assert!(
            said.contains("`mix package uninstall msys2 2026.10.08` forgets it"),
            "{said}"
        );
    }

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

    /// **What is left to do ends the report** — roadmap task **T205**, D6. A `serve` step says it
    /// keeps running, an optional one says so, and an `open` step names its path.
    #[test]
    fn an_apply_with_steps_ends_with_what_to_run() {
        let step = |kind, run: Option<&str>, path: Option<&str>, note: Option<&str>, optional| {
            mixengine_proto::NextStep {
                kind,
                run: run.map(str::to_owned),
                path: path.map(str::to_owned),
                note: note.map(str::to_owned),
                optional,
                credentials: false,
                site: None,
            }
        };
        let applied = BlueprintApplied {
            blueprint: "nextjs".to_owned(),
            project: "shop".to_owned(),
            root: "/tmp/shop".to_owned(),
            steps: Vec::new(),
            next_steps: Some(mixengine_proto::NextSteps {
                trusted: true,
                steps: vec![
                    step(
                        mixengine_proto::NextStepKind::Once,
                        Some("npm install"),
                        None,
                        None,
                        false,
                    ),
                    step(
                        mixengine_proto::NextStepKind::Serve,
                        Some("npm run dev"),
                        None,
                        Some("port 3000"),
                        false,
                    ),
                    step(
                        mixengine_proto::NextStepKind::Open,
                        None,
                        Some("/cp"),
                        None,
                        true,
                    ),
                ],
            }),
            database: None,
        };

        let said = blueprint_applied(&applied);
        assert!(said.contains("Next, in the project directory:"), "{said}");
        assert!(said.contains("  npm install\n"), "{said}");
        assert!(
            said.contains("  npm run dev   (keeps running) port 3000\n"),
            "{said}"
        );
        assert!(said.contains("  open /cp   (optional)\n"), "{said}");
        assert!(!said.contains("nobody vouches"), "{said}");
    }

    /// **Untrusted steps say so** — T205, D12.
    #[test]
    fn untrusted_steps_are_read_before_they_are_run() {
        let said = next_steps_said(&mixengine_proto::NextSteps {
            trusted: false,
            steps: vec![mixengine_proto::NextStep {
                kind: mixengine_proto::NextStepKind::Once,
                run: Some("npm install".to_owned()),
                path: None,
                note: None,
                optional: false,
                credentials: false,
                site: None,
            }],
        });
        assert!(said.contains("nobody vouches"), "{said}");
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
            next_steps: None,
            database: None,
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
            next_steps: None,
            database: None,
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
            next_steps: None,
            database: None,
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

    /// **T204a, D7.** A plan with two sites says which site each `site` line makes.
    #[test]
    fn a_plan_with_two_sites_names_each_one() {
        let site = || PlanStep {
            action: PlanAction::CreateSite {
                kind: mixengine_proto::SiteKind::Static,
                doc_root: String::new(),
                https: true,
                routes: Vec::new(),
                services: None,
            },
            disposition: Disposition::Create,
            elevates: false,
        };
        let name = |domain: &str| PlanStep {
            action: PlanAction::AddDomain {
                domain: domain.to_owned(),
                primary: true,
            },
            disposition: Disposition::Create,
            elevates: true,
        };
        let mut plan = a_plan();
        plan.steps = vec![site(), name("shop.test"), site(), name("docs.shop.test")];

        let rendered = super::blueprint_plan(&plan);
        assert!(rendered.contains("site shop.test"), "{rendered}");
        assert!(rendered.contains("site docs.shop.test"), "{rendered}");
    }

    /// **T204a, D7.** A step that belongs to one site says which.
    #[test]
    fn a_step_with_a_site_says_it() {
        let steps = mixengine_proto::NextSteps {
            trusted: true,
            steps: vec![mixengine_proto::NextStep {
                kind: mixengine_proto::NextStepKind::Serve,
                run: Some("npm run dev".to_owned()),
                path: None,
                note: None,
                optional: false,
                credentials: false,
                site: Some("vite.shop.test".to_owned()),
            }],
        };

        assert!(super::next_steps_said(&steps).contains("npm run dev   (vite.shop.test)"));
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

    /// **A share with no deadline prints no line about one.** Every line in this block is one a
    /// reader has to take in, and "ends never" is one they would have to learn to skip.
    #[test]
    fn a_share_with_no_deadline_says_nothing_about_one() {
        assert!(!super::site_shared(&a_share(true)).contains("ends"));
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
            next_steps: None,
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
