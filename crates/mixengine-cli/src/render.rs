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
pub(crate) mod daemon;
pub(crate) mod elevation;
pub(crate) mod home;
pub(crate) mod job;
pub(crate) mod metrics;
pub(crate) mod package;
pub(crate) mod path;
pub(crate) mod prerequisites;
pub(crate) mod project;
pub(crate) mod runtime;
pub(crate) mod self_update;
pub(crate) mod service;
pub(crate) mod site;
mod size;
pub(crate) mod status;
mod table;
mod time;
pub(crate) mod uninstall;
pub(crate) mod upgrades;

use size::size;
use table::table;

use mixengine_proto::{
    Action, ApiAccess, ArtifactAvailability, AutostartMechanism, AutostartReport, BundleReport,
    Cleanup, CleanupReport, DatabaseAccount, DatabaseClientReport, DatabaseCredentials,
    DatabaseHandoff, DesktopClient, DiskUsage, DoctorReport, DomainStatusReport,
    ExtensionCatalogue, ExtensionInspection, ExtensionKind, ExtensionPlan, ExtensionRemoval,
    FilesystemReach, InstalledExtensions, Launch, Made, NetworkReach, Outcome, RecipeAddition,
    Reclaim, RepairReport,
};

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
///
/// [`uninstall_report`]: crate::render::uninstall::uninstall_report
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
///
/// [`Pathed`]: crate::render::path::Pathed
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
        CategoryUsage, Cleaned, DiskCategory, PackageVersion, PortWish, SecretAddress, ServiceId,
        Timestamp,
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
