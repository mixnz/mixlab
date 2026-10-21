//! `mix uninstall`, and the lines it prints while it waits.

use super::size::size;
use mixengine_proto::{Removal, UninstallReport};

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

    // the daemon counted these; a person reading only the end is told the machine is not
    // bare on purpose.
    match report.kept_for_others {
        0 => {}
        1 => out.push_str("1 thing was kept because another home on this machine uses it\n"),
        many => out.push_str(&format!(
            "{many} things were kept because another home on this machine uses them\n"
        )),
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// ADR 0066: what another home still uses is counted by the daemon, and said once at the end.
    #[test]
    fn what_another_home_keeps_is_said_last() {
        let report = UninstallReport {
            items: Vec::new(),
            kept_for_others: 2,
        };

        assert!(
            uninstall_report(&report)
                .ends_with("2 things were kept because another home on this machine uses them\n"),
            "{}",
            uninstall_report(&report)
        );

        let none = UninstallReport {
            items: Vec::new(),
            kept_for_others: 0,
        };
        assert_eq!(uninstall_report(&none), "");
    }
}
