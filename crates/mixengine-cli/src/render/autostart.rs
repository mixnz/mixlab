//! `mix autostart`: the login entry that starts the daemon.

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
mod tests {
    use mixengine_proto::AtBoot;

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
}
