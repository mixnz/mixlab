//! `mix autostart`: the login entry that starts the daemon.

use mixengine_proto::{AtBoot, AutostartMechanism, AutostartReport};

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
    /// `mix autostart enable --at-boot`.
    EnabledAtBoot,
    /// `mix autostart disable --at-boot`.
    DisabledAtBoot,
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

        (Autostarted::EnabledAtBoot, _, _) => match report.changed {
            true => "this home's daemon will now start at boot, before anyone logs in\n",
            false => "this home's daemon already started at boot\n",
        }
        .to_owned(),

        (Autostarted::DisabledAtBoot, _, _) => match (report.at_boot, report.changed) {
            (AtBoot::OnByAccount, _) => {
                "this account starts its services at boot on its own, and MixEngine leaves that \
                 alone\n"
            }
            (_, true) => {
                "this home's daemon no longer starts at boot; it still starts when you log in\n"
            }
            (_, false) => "this home's daemon did not start at boot\n",
        }
        .to_owned(),

        (Autostarted::Asked, true, true) => match report.at_boot {
            AtBoot::On | AtBoot::OnByAccount => {
                "this home's daemon starts at boot, before anyone logs in\n"
            }
            AtBoot::Off | AtBoot::Unsupported => "this home's daemon starts when you log in\n",
        }
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

    if let Some(line) = at_boot(report) {
        rendered.push_str(&format!("  {:<9} {}\n", "at boot", line));
    }

    if matches!(report.at_boot, AtBoot::On | AtBoot::OnByAccount) && report.for_this_home {
        rendered.push_str(
            "  services start with it only if their own autostart is on: see mix service list\n",
        );
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

    if autostarted == Autostarted::EnabledAtBoot && report.at_boot == AtBoot::On {
        rendered.push_str("it takes effect at the next boot\n");
    }

    rendered
}

/// Whether the daemon also starts before anyone logs in, and whose setting that is, or nothing
/// where the machine has no such thing.
fn at_boot(report: &AutostartReport) -> Option<&'static str> {
    match report.at_boot {
        AtBoot::Unsupported => None,
        AtBoot::Off if report.enabled && report.for_this_home => Some("no, only after you log in"),
        AtBoot::Off => Some("no"),
        AtBoot::On => {
            Some("yes, before anyone logs in (MixEngine turned on linger for this account)")
        }
        AtBoot::OnByAccount => Some(
            "yes, before anyone logs in (this account already lingers, and MixEngine leaves that \
             alone)",
        ),
    }
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
    use super::*;

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
            at_boot: AtBoot::Off,
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

    /// Found running M34: an entry that starts at boot was headed "starts when you log in".
    #[test]
    fn an_entry_that_starts_at_boot_is_headed_so() {
        for at_boot in [AtBoot::On, AtBoot::OnByAccount] {
            let report = AutostartReport {
                at_boot,
                ..registered(true)
            };

            let rendered = autostart_report(Autostarted::Asked, &report);

            assert!(
                rendered.starts_with("this home's daemon starts at boot, before anyone logs in"),
                "{rendered}"
            );
        }
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
            at_boot: AtBoot::Unsupported,
        };

        let rendered = autostart_report(Autostarted::Asked, &nothing);

        assert!(rendered.contains("no systemd user manager"), "{rendered}");
        assert!(rendered.contains("nothing to register"), "{rendered}");
    }

    #[test]
    fn the_boot_line_is_absent_where_there_is_no_boot_half() {
        let report = AutostartReport {
            at_boot: AtBoot::Unsupported,
            ..registered(true)
        };

        assert!(!autostart_report(Autostarted::Asked, &report).contains("at boot"));
    }

    #[test]
    fn each_boot_state_says_whose_it_is() {
        for (at_boot, says) in [
            (AtBoot::Off, "only after you log in"),
            (AtBoot::On, "MixEngine turned on linger"),
            (AtBoot::OnByAccount, "MixEngine leaves that alone"),
        ] {
            let report = AutostartReport {
                at_boot,
                ..registered(true)
            };
            let rendered = autostart_report(Autostarted::Asked, &report);
            assert!(rendered.contains(says), "{at_boot:?}: {rendered}");
        }
    }

    #[test]
    fn starting_at_boot_points_at_the_services_that_come_with_it() {
        let report = AutostartReport {
            at_boot: AtBoot::On,
            changed: true,
            ..registered(true)
        };
        let rendered = autostart_report(Autostarted::EnabledAtBoot, &report);

        assert!(
            rendered.starts_with("this home's daemon will now start at boot"),
            "{rendered}"
        );
        assert!(rendered.contains("mix service list"), "{rendered}");
        assert!(rendered.contains("next boot"), "{rendered}");
    }

    #[test]
    fn unticking_boot_against_the_account_s_own_linger_says_nothing_changed() {
        let report = AutostartReport {
            at_boot: AtBoot::OnByAccount,
            ..registered(true)
        };
        let rendered = autostart_report(Autostarted::DisabledAtBoot, &report);

        assert!(
            rendered.starts_with("this account starts its services at boot on its own"),
            "{rendered}"
        );
    }

    /// Nothing starts at login either, so "only after you log in" would be a promise.
    #[test]
    fn with_nothing_registered_the_boot_line_is_a_plain_no() {
        let report = AutostartReport {
            enabled: false,
            command: Vec::new(),
            ..registered(true)
        };
        let rendered = autostart_report(Autostarted::Asked, &report);

        assert!(rendered.contains("  at boot   no\n"), "{rendered}");
        assert!(!rendered.contains("only after you log in"), "{rendered}");
    }
}
