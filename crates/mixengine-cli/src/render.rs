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
pub(crate) mod extension;
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

use mixengine_proto::{AutostartMechanism, AutostartReport};

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
