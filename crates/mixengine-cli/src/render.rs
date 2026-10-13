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
pub(crate) mod database;
pub(crate) mod disk;
pub(crate) mod doctor;
pub(crate) mod domain;
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

use table::table;

use mixengine_proto::{
    ApiAccess, ArtifactAvailability, AutostartMechanism, AutostartReport, ExtensionCatalogue,
    ExtensionInspection, ExtensionKind, ExtensionPlan, ExtensionRemoval, FilesystemReach,
    InstalledExtensions, NetworkReach, RecipeAddition,
};

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
    use mixengine_proto::{PackageVersion, PortWish, ServiceId};

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
