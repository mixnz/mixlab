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

pub(crate) mod blueprint;
pub(crate) mod catalogue;
pub(crate) mod cert;
pub(crate) mod elevation;
pub(crate) mod job;
pub(crate) mod prerequisites;
pub(crate) mod runtime;
pub(crate) mod self_update;
pub(crate) mod service;
mod size;
pub(crate) mod status;
mod table;
mod time;
pub(crate) mod upgrades;

use blueprint::next_steps_said;
use catalogue::{
    Lines, MORE_HEADING, NEEDS_HEADING, RUNS_HEADING, any_lacking, emulation_column, more, needs,
    runs, update_lines,
};
use service::{Walked, service_walk};
use size::size;
use table::{MISSING, table, table_of};
use time::{ago, in_time};

use mixengine_proto::{
    Action, ApiAccess, ArtifactAvailability, AutostartMechanism, AutostartReport, BundleReport,
    Cleanup, CleanupReport, CommandSource, DaemonShutdown, DatabaseAccount, DatabaseClientReport,
    DatabaseCredentials, DatabaseHandoff, DeclaredSiteState, DesktopClient, DiskUsage,
    DoctorReport, DomainStatusReport, ExtensionCatalogue, ExtensionInspection, ExtensionKind,
    ExtensionPlan, ExtensionRemoval, FilesystemReach, InstalledExtensions, Launch, Made,
    MetricsFrame, MetricsHistory, NetworkReach, Outcome, PackageCatalogue, PackageList,
    PackageRelease, PackageRemoval, PackageVersion, PathReport, PinSource, ProjectDetail,
    ProjectExport, ProjectList, ProjectRemoval, RecipeAddition, Reclaim, Removal, RepairReport,
    ServiceId, ServiceList, SiteDetail, SiteKind, SiteList, SiteOwner, SiteRemoval, SiteSharing,
    UninstallReport,
};
use std::time::SystemTime;

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
        PortWish, SecretAddress, ServiceState, ServiceWalk, Timestamp,
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
