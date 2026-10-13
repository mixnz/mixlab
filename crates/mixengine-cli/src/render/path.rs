//! `mix path`: whether `<root>/bin` is on the PATH, and putting it there.

use mixengine_proto::{CommandSource, PathReport};

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
