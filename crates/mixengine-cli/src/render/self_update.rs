//! `mix self-update` — the headless distribution's updater.

use super::service::names;
use super::size::size;
use super::table::MISSING;
use mixengine_proto::{UpdateApplied, UpdateHandedOver, UpdatePlacement, UpdateStatus};

/// `mix self-update` and `mix self-update --check`, for a person.
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

    // Installer.app has put the new binaries on disk and this daemon is still the old one.
    if let Some(installed) = &status.installed {
        rendered.push_str(&format!(
            "  update    {installed} is installed. finish it: mix self-update --finish\n"
        ));

        return rendered;
    }

    // A copy the `.pkg` installed reads `managed` on the wire, and is still offered its update: the
    // next `.pkg`, through Installer.app. Only a copy with no installer is refused here.
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

/// What `mix self-update` prints once the package is handed over.
///
/// **The path and the command every time**, not only when opening failed. Over SSH, `open` succeeds
/// and Installer.app comes up on the Mac's own screen, which the person at this prompt may not be
/// looking at (measured on a Mac). Both are the daemon's, printed unchanged. A Linux machine with
/// no desktop session opens nothing, and the command is then the only way.
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

/// What an update did, printed while the daemon that did it is exiting.
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

/// "1 service" / "3 services", so a sentence built from a count reads.
fn services(count: usize) -> String {
    match count {
        1 => "1 service".to_owned(),
        many => format!("{many} services"),
    }
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{ServiceId, Timestamp};

    use super::*;

    /// A release this account cannot install is refused in the daemon's own words, and the words are
    /// printed rather than replaced by a code this client would have to invent a sentence for.
    /// A status as a copy the `.pkg` installed reports it: `managed` on the wire, with the
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

    /// A copy the `.pkg` installed is offered the installer, not refused.
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
    /// (measured on a Mac).
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

    /// A Linux machine with no desktop session opens nothing: the command is printed as
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
}
