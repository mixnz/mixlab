//! `mix status` and `mix storage`.

use super::elevation::operations;
use super::time::uptime;
use mixengine_proto::{
    DaemonStatus, DaemonVersion, DnsMode, PROTOCOL_VERSION, StorageChoice, StorageReport,
};

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

    // **The names line, and it is one line whichever mechanism is running.**
    // The mode alone is not the sentence somebody needs: what a hosts-only home loses is wildcards,
    // and what it wants to know is why, so both travel with it.
    if let Some(dns) = &status.dns {
        rendered.push_str(&match dns.mode {
            // The TLDs are named rather than counted, because a home can have wildcards for some of
            // its names and not others — `.local` is never routed — and "wildcards work" would be
            // true and useless to somebody whose `.local` site had just stopped resolving.
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

    // Which store the passwords are in. Absent from a daemon that predates it, and then no line is
    // invented.
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

    // **One line when there is an update, and nothing at all when there is not.** Everything else
    // about it is `update.status`, which is a screen; this is a status line, and what makes it
    // worth one is that nothing else in the product would ever mention it.
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
    // **Reachable.** It was written for this skew and tested for it, and until `elevation` and
    // `dns` became optional the answer did not deserialise
    // — so the one thing that explained the situation was the one thing that could not be printed.
    // See ADR 0019, `docs/decisions/0019-an-added-response-member-is-optional.md`.
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

/// `mix status --json`.
///
/// An envelope rather than the daemon's answer alone. The daemon half is `DaemonStatus` verbatim, so
/// `mix status --json | jq .daemon.pid` reads the field by the name the API gives it; the client
/// half is the part no daemon can report, and version skew is the first thing anybody looks for in a
/// captured diagnostic.
///
/// **Verbatim includes what is not there.** A member an older daemon predates is absent from this
/// object rather than `null` or defaulted — `.daemon.dns`, `.daemon.elevation` and `.daemon.update`
/// among them — which is the honest encoding of a fact nobody reported, and what `jq` should be
/// asked about with `//` rather than indexed into.
pub(crate) fn status_json(status: &DaemonStatus) -> serde_json::Value {
    serde_json::json!({
        "client": client(),
        "daemon": status,
    })
}

/// This build of `mix`, in the shape the daemon reports itself in.
fn client() -> serde_json::Value {
    serde_json::to_value(DaemonVersion {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: PROTOCOL_VERSION,
    })
    .expect("a DaemonVersion of two owned fields always serialises")
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

#[cfg(test)]
mod tests {
    use mixengine_proto::{Timestamp, Uptime};

    use super::*;

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

    /// `mix status` names the store when the daemon says, and invents nothing when it does not.
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

    /// `mix status` gains one line when an update is offered.
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

    /// A daemon from before `elevation` and `dns` existed.
    ///
    /// **The point is that this renders at all.** Until those two members became optional the
    /// answer did not deserialise, so the note below — written for exactly this skew, and tested
    /// above — could never reach anybody.
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
}
