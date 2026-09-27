//! Reading `config.toml`, and keeping the shipped template honest.

use std::path::PathBuf;

use mixengine_core::config::{
    self, Certs, Config, Crash, Daemon, Dns, LogFormat, LogLevel, Logging, Metrics, PathOverrides,
    RequestedPaths, Services, Sharing, Sites, TEMPLATE, Updates,
};
use tempfile::TempDir;

fn write(home: &TempDir, contents: &str) -> PathBuf {
    let path = home.path().join(config::FILE_NAME);
    std::fs::write(&path, contents).expect("the temporary home is writable");
    path
}

/// What a user actually sees: the error and everything under it.
///
/// The parse failure is the `#[source]` rather than part of the top-level message, so a test that
/// only looked at `to_string()` would miss the half that says which key is wrong.
fn reported(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        message.push('\n');
        message.push_str(&next.to_string());
        cause = next.source();
    }
    message
}

#[test]
fn a_missing_file_means_defaults() {
    let home = TempDir::new().unwrap();

    let config = config::load(&home.path().join(config::FILE_NAME)).unwrap();

    assert_eq!(config, Config::default());
    assert_eq!(config.log.level, LogLevel::Info);
    assert_eq!(config.log.format, LogFormat::Text);
    assert_eq!(config.daemon.ipc_path, None);
    assert_eq!(config.daemon.shutdown_grace_seconds, 10);
}

#[test]
fn a_shutdown_grace_of_zero_is_a_setting_and_not_an_absent_key() {
    // The one value a derived `Default` would have produced, which is why `Daemon` writes its own:
    // "kill everything at once" is a choice somebody can make, and it must not be what a user gets
    // for leaving the key out.
    let home = TempDir::new().unwrap();
    let path = write(&home, "[daemon]\nshutdown_grace_seconds = 0\n");

    let config = config::load(&path).unwrap();

    assert_eq!(config.daemon.shutdown_grace_seconds, 0);
    assert_ne!(
        config.daemon.shutdown_grace_seconds,
        Daemon::default().shutdown_grace_seconds
    );
}

#[test]
fn a_shutdown_grace_past_the_ceiling_is_refused_rather_than_lowered() {
    // One second over, so what is being checked is the boundary and not "a big number looks wrong".
    // The message has to carry both halves: the number that was refused, because a config file long
    // enough to hold a mistake is long enough that "somewhere in here" is not an answer, and the
    // number that would be accepted, because a ceiling nobody can read is a ceiling nobody can
    // satisfy.
    let home = TempDir::new().unwrap();
    let path = write(&home, "[daemon]\nshutdown_grace_seconds = 601\n");

    let error = config::load(&path).unwrap_err();
    let message = reported(&error);

    assert!(
        matches!(error, mixengine_core::Error::Config { .. }),
        "{error:?}"
    );
    assert!(message.contains("601"), "{message}");
    assert!(message.contains("600"), "{message}");
}

#[test]
fn a_shutdown_grace_of_u64_max_never_reaches_the_arithmetic_that_would_panic() {
    // The value from the report, and the reason there is a ceiling at all: the daemon turns this
    // into a `Duration` and adds it to an `Instant`, and that addition panics on overflow rather
    // than saturating — on the shutdown path, unwinding past the WAL checkpoint that a clean stop
    // exists to perform. Asserting the ceiling is named in the message also pins the claim about
    // `toml` this all rests on: TOML integers are decoded into whatever width holds them, so
    // `u64::MAX` arrives here intact rather than being refused earlier as too large for an `i64`.
    let home = TempDir::new().unwrap();
    let path = write(
        &home,
        &format!("[daemon]\nshutdown_grace_seconds = {}\n", u64::MAX),
    );

    let error = config::load(&path).unwrap_err();
    let message = reported(&error);

    assert!(
        matches!(error, mixengine_core::Error::Config { .. }),
        "{error:?}"
    );
    assert!(message.contains("18446744073709551615"), "{message}");
    assert!(message.contains("600"), "{message}");
}

#[test]
fn a_shutdown_grace_under_the_ceiling_arrives_exactly_as_written() {
    // Including the ceiling itself: the bound is "no more than", and a user who reads the template
    // and types the largest number it names must not be told it is too large. 300 is the ordinary
    // case — a value somebody would plausibly set for a slow database, well clear of the bound and
    // untouched by it.
    for seconds in [300, 600] {
        let home = TempDir::new().unwrap();
        let path = write(
            &home,
            &format!("[daemon]\nshutdown_grace_seconds = {seconds}\n"),
        );

        let config = config::load(&path).unwrap();

        assert_eq!(config.daemon.shutdown_grace_seconds, seconds);
    }
}

#[test]
fn a_daemon_section_without_the_budget_still_gets_the_default() {
    // The section is present and the key is not, which is the one arrangement the checking of this
    // value could quietly break: a bound expressed as a field-level `default` would replace the
    // section's own, and the absent key would start meaning zero — "kill everything at once" — for
    // everybody who ever set an `ipc_path`.
    let home = TempDir::new().unwrap();
    let path = write(&home, "[daemon]\nipc_path = \"/tmp/mixengined.sock\"\n");

    let config = config::load(&path).unwrap();

    assert_eq!(
        config.daemon.shutdown_grace_seconds,
        Daemon::default().shutdown_grace_seconds
    );
    assert_eq!(config.daemon.shutdown_grace_seconds, 10);
}

#[test]
fn first_run_writes_the_template_and_still_reads_defaults() {
    let home = TempDir::new().unwrap();
    let path = home.path().join(config::FILE_NAME);

    let config = config::load_or_create(&path).unwrap();

    assert_eq!(config, Config::default());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
}

#[test]
fn an_existing_file_is_never_overwritten() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[log]\nlevel = \"debug\"\n");

    assert!(!config::write_template(&path).unwrap());

    let config = config::load_or_create(&path).unwrap();
    assert_eq!(config.log.level, LogLevel::Debug);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[log]\nlevel = \"debug\"\n"
    );
}

#[test]
fn every_setting_round_trips() {
    // Absolute means something different on each OS, so the fixture uses whatever "a second disk"
    // looks like here — the point is that a full path survives unchanged next to a relative one.
    let bulk = if cfg!(windows) {
        r"D:\bulk"
    } else {
        "/mnt/bulk"
    };
    let home = TempDir::new().unwrap();
    let path = write(
        &home,
        &format!(
            r#"
[log]
level = "trace"
format = "json"

[daemon]
ipc_path = "/run/user/1000/mixengined.sock"
shutdown_grace_seconds = 30

[dns]
enabled = false
port = 5300

[paths]
runtimes = "{bulk}/runtimes"
packages = "{bulk}/packages"
data = "{bulk}/data"
logs = "logs-elsewhere"
"#
        )
        .replace('\\', "/"),
    );

    let config = config::load(&path).unwrap();

    assert_eq!(
        config,
        Config {
            log: Logging {
                level: LogLevel::Trace,
                format: LogFormat::Json,
            },
            retired_bin: mixengine_core::config::RetiredBin::default(),
            daemon: Daemon {
                ipc_path: Some(PathBuf::from("/run/user/1000/mixengined.sock")),
                shutdown_grace_seconds: 30,
            },
            dns: Dns {
                enabled: false,
                port: Some(5300),
            },
            // None of the seven is named in the file this test writes, which is the assertion
            // that an absent section is the ordinary behaviour rather than an off switch.
            certs: Certs::default(),
            services: Services::default(),
            sharing: Sharing::default(),
            sites: Sites::default(),
            updates: Updates::default(),
            metrics: Metrics::default(),
            crash: Crash::default(),
            paths: PathOverrides {
                runtimes: Some(PathBuf::from(format!("{bulk}/runtimes").replace('\\', "/"))),
                packages: Some(PathBuf::from(format!("{bulk}/packages").replace('\\', "/"))),
                data: Some(PathBuf::from(format!("{bulk}/data").replace('\\', "/"))),
                logs: Some(PathBuf::from("logs-elsewhere")),
            },
        }
    );
}

#[test]
fn a_partial_file_fills_the_rest_in_from_the_defaults() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[log]\nformat = \"json\"\n");

    let config = config::load(&path).unwrap();

    assert_eq!(config.log.format, LogFormat::Json);
    assert_eq!(config.log.level, LogLevel::Info);
    assert_eq!(config.paths, PathOverrides::default());
}

#[test]
fn an_unknown_key_is_refused_and_the_message_lists_what_is_accepted() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[log]\nlevl = \"debug\"\n");

    let error = config::load(&path).unwrap_err();
    let message = reported(&error);

    assert!(
        matches!(error, mixengine_core::Error::Config { .. }),
        "{error:?}"
    );
    assert!(message.contains("levl"), "{message}");
    assert!(message.contains("level"), "{message}");
    assert!(message.contains("config.toml"), "{message}");
    // The line number matters: a config file long enough to make a typo in is long enough that
    // "somewhere in here" is not an answer.
    assert!(message.contains("line 2"), "{message}");
}

#[test]
fn an_unknown_section_is_refused_too() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[telemetry]\nenabled = true\n");

    let error = config::load(&path).unwrap_err();

    assert!(
        matches!(error, mixengine_core::Error::Config { .. }),
        "{error:?}"
    );
    assert!(reported(&error).contains("telemetry"), "{error}");
}

#[test]
fn a_value_outside_the_closed_set_is_refused() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[log]\nlevel = \"verbose\"\n");

    let error = config::load(&path).unwrap_err();

    assert!(
        matches!(error, mixengine_core::Error::Config { .. }),
        "{error:?}"
    );
    assert!(reported(&error).contains("verbose"), "{error}");
}

/// Paths that name no directory of their own.
///
/// `Path::join("")` gives the original path back, so an empty relocation would quietly make data/
/// *be* MIXENGINE_HOME — and a later "reset the data directory" would take the whole install with
/// it. Everything here says the same thing in a different way: `..` and `bulk/..` land on the home
/// or its parent, `/` on a whole filesystem.
///
/// **One list, walked by both doors into the rule** — roadmap task **T143**. `config::load` reads
/// it through serde and `config::set_paths` writes it through `check_relocation`; a second list
/// beside this one is how the two come to disagree.
const NAMES_NOTHING: [&str; 8] = ["", ".", "./", "..", "../", "bulk/..", "x/../..", "/"];

#[test]
fn a_relocation_that_names_nothing_is_refused() {
    for nowhere in NAMES_NOTHING {
        let home = TempDir::new().unwrap();
        let path = write(&home, &format!("[paths]\ndata = \"{nowhere}\"\n"));

        let error = config::load(&path).unwrap_err();

        assert!(
            matches!(error, mixengine_core::Error::Config { .. }),
            "data = {nowhere:?} was accepted: {error:?}"
        );
        assert!(reported(&error).contains("data"), "{error}");
    }
}

#[test]
fn a_relocation_beside_the_home_is_still_allowed() {
    // The rule above is "names nothing of its own", not "never climbs": a sibling directory is an
    // ordinary place to put a second disk's worth of data, and it contains no part of the home.
    let home = TempDir::new().unwrap();
    let path = write(&home, "[paths]\ndata = \"../mixengine-bulk\"\n");

    let config = config::load(&path).unwrap();

    assert_eq!(config.paths.data, Some(PathBuf::from("../mixengine-bulk")));
}

#[test]
fn a_relocation_that_does_not_say_which_drive_is_refused() {
    // Windows only: `C:\home\MixEngine`.join("/bulk") is `C:\bulk` — neither inside the home nor
    // where the user was pointing, and nothing in the config file hints at it. On Unix the same
    // string is plainly absolute and means what it says.
    let home = TempDir::new().unwrap();
    let path = write(&home, "[paths]\ndata = \"/bulk\"\n");

    let loaded = config::load(&path);

    if cfg!(windows) {
        let error = loaded.unwrap_err();
        assert!(
            matches!(error, mixengine_core::Error::Config { .. }),
            "{error:?}"
        );
        assert!(reported(&error).contains("drive"), "{error}");
    } else {
        assert_eq!(
            loaded.unwrap().paths.data,
            Some(PathBuf::from("/bulk")),
            "an absolute path is the ordinary case on Unix"
        );
    }
}

#[test]
fn a_relocation_that_names_a_drive_but_not_its_root_is_refused() {
    // The mirror of the case above, and the more dangerous one: a path carrying a drive prefix
    // replaces everything it is joined to, so `C:\home\MixEngine`.join("C:bulk") is plain `C:bulk`
    // — resolved against drive C's *current directory*, which the config file never mentions.
    let home = TempDir::new().unwrap();
    let path = write(&home, "[paths]\ndata = \"C:bulk\"\n");

    let loaded = config::load(&path);

    if cfg!(windows) {
        let error = loaded.unwrap_err();
        assert!(
            matches!(error, mixengine_core::Error::Config { .. }),
            "{error:?}"
        );
        assert!(reported(&error).contains("drive"), "{error}");
    } else {
        assert_eq!(
            loaded.unwrap().paths.data,
            Some(PathBuf::from("C:bulk")),
            "Unix has no drive prefixes — this is a directory whose name contains a colon"
        );
    }
}

#[test]
fn a_network_share_is_a_directory_like_any_other() {
    // Windows folds the whole of `\\server\share` into the path's prefix, so counting components
    // finds nothing there and the "names no directory" rule would refuse a perfectly addressable
    // place. A share root is not a drive root: it is somewhere data can actually live.
    let home = TempDir::new().unwrap();
    let path = write(&home, "[paths]\ndata = '//server/share'\n");

    let config = config::load(&path).unwrap();

    assert_eq!(config.paths.data, Some(PathBuf::from("//server/share")));
}

#[test]
fn an_empty_ipc_path_is_refused() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[daemon]\nipc_path = \"\"\n");

    let error = config::load(&path).unwrap_err();

    assert!(
        matches!(error, mixengine_core::Error::Config { .. }),
        "{error:?}"
    );
}

#[test]
fn the_template_as_shipped_changes_nothing() {
    let config: Config = toml::from_str(TEMPLATE).unwrap();

    assert_eq!(config, Config::default());
}

/// Every template a release has shipped, byte for byte, named by the first release that shipped it.
/// A release that changes the template adds its own here.
const RELEASED_TEMPLATES: &[(&str, &str)] = &[
    ("v0.0.1", include_str!("released-templates/v0.0.1.toml")),
    ("v0.0.7", include_str!("released-templates/v0.0.7.toml")),
];

/// **`config.toml` is written once and never rewritten**, so a home keeps the template of the
/// release it first ran — and whatever a later build drops from the template must still read
/// there. T185b dropped `[bin]`, and every home first run by 0.0.7 to 0.0.9 stopped starting, the
/// uninstaller with it, since it asks the daemon what it would remove.
#[test]
fn a_home_holding_any_released_template_still_starts() {
    for (release, template) in RELEASED_TEMPLATES {
        let home = TempDir::new().unwrap();
        let path = write(&home, template);

        if let Err(error) = config::load(&path) {
            panic!(
                "a home first run by {release} no longer reads its own config.toml:\n{}",
                reported(&error)
            );
        }
    }
}

/// And with every key it documented uncommented, which is the file of somebody who changed one.
#[test]
fn every_key_a_released_template_documented_still_reads() {
    for (release, template) in RELEASED_TEMPLATES {
        let uncommented: String = template
            .lines()
            .map(|line| {
                line.strip_prefix('#')
                    .filter(|rest| is_key_line(rest))
                    .unwrap_or(line)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let home = TempDir::new().unwrap();
        let path = write(&home, &uncommented);

        if let Err(error) = config::load(&path) {
            panic!(
                "a key {release} documented is now refused:\n{}",
                reported(&error)
            );
        }
    }
}

#[test]
fn every_key_the_template_documents_is_a_real_key() {
    // `deny_unknown_fields` turns this into a spell-checker for the template: uncomment every key
    // line and a documented key that no longer exists, or was renamed, fails to parse.
    let uncommented: String = TEMPLATE
        .lines()
        .map(|line| {
            line.strip_prefix('#')
                .filter(|rest| is_key_line(rest))
                .unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let config: Config = toml::from_str(&uncommented).unwrap_or_else(|error| {
        panic!("the template documents a key that does not exist: {error}")
    });

    // The values shown for `[log]` are claimed to be the defaults, so they have to be.
    assert_eq!(config.log, Logging::default());
    // And so is the one shown for the shutdown budget, which the template states in so many words.
    assert_eq!(
        config.daemon.shutdown_grace_seconds,
        Daemon::default().shutdown_grace_seconds
    );
    // And the one shown for the DNS server, which the template also states as the default.
    assert_eq!(config.dns.enabled, Dns::default().enabled);
    // And the renewal period, which the template also states as the default.
    assert_eq!(
        config.certs.renew_check_seconds,
        Certs::default().renew_check_seconds
    );
    // The rest have no default to show and carry an example instead — which must still be parsed
    // as the right type, not silently ignored.
    assert!(config.daemon.ipc_path.is_some());
    // The port depends on the machine, so the template shows an example rather than a default.
    assert!(config.dns.port.is_some());
    assert!(config.paths.runtimes.is_some());
    assert!(config.paths.packages.is_some());
    assert!(config.paths.data.is_some());
    assert!(config.paths.logs.is_some());
}

/// A commented-out setting (`#level = "info"`), as opposed to prose (`# How much to log`).
fn is_key_line(rest: &str) -> bool {
    rest.starts_with(|character: char| character.is_ascii_lowercase()) && rest.contains(" = ")
}

#[test]
fn an_absent_certs_section_checks_hourly() {
    let home = TempDir::new().unwrap();

    let config = config::load(&home.path().join(config::FILE_NAME)).unwrap();

    assert_eq!(config.certs.renew_check_seconds, 3600);
    assert_eq!(config.certs, Certs::default());
}

/// **Zero is a loop with no pause in it, not a setting**, which is what makes it different from the
/// shutdown budget's zero — that one means "kill everything at once" and is a real answer somebody
/// might want.
#[test]
fn a_renewal_period_of_zero_is_refused_rather_than_corrected() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[certs]\nrenew_check_seconds = 0\n");

    let error = config::load(&path).unwrap_err();
    let message = reported(&error);

    assert!(
        matches!(error, mixengine_core::Error::Config { .. }),
        "{error:?}"
    );
    assert!(message.contains("3600"), "{message}");
}

/// A second is legal, and it is what the daemon's own renewal suite sets. A period no test can move
/// would leave the loop the one part of T52 that nothing ever runs — which is exactly how T51 nearly
/// shipped an nginx TLS port no machine could bind.
#[test]
fn a_renewal_period_of_one_second_is_accepted() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[certs]\nrenew_check_seconds = 1\n");

    let config = config::load(&path).unwrap();

    assert_eq!(config.certs.renew_check_seconds, 1);
}

/// **A share that ends by itself has a period, and zero is refused** — roadmap task **T76**, on the
/// reasoning `renew_check_seconds` states: zero is not a short pause, it is none.
#[test]
fn a_sharing_check_of_zero_is_refused_rather_than_corrected() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[sharing]\ncheck_seconds = 0\n");

    let error = config::load(&path).unwrap_err();
    let message = reported(&error);

    assert!(
        matches!(error, mixengine_core::Error::Config { .. }),
        "{error:?}"
    );
    assert!(message.contains("30"), "{message}");
}

/// A second is legal, and it is what the daemon's own revoke suite sets — the same argument that
/// makes `renew_check_seconds` a key rather than a constant.
#[test]
fn a_sharing_check_of_one_second_is_accepted() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[sharing]\ncheck_seconds = 1\n");

    let config = config::load(&path).unwrap();

    assert_eq!(config.sharing.check_seconds, 1);
}

/// A home with no `[sharing]` section still ends a share it should.
#[test]
fn a_home_with_no_sharing_section_checks_on_the_default_period() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "");

    let config = config::load(&path).unwrap();

    assert_eq!(config.sharing, Sharing::default());
    assert_eq!(config.sharing.check_seconds, 30);
}

/// Recording is on unless the file says otherwise — the T91 design's reading of "opt-in", which is
/// spent on transmission and not on a file in the user's own home.
#[test]
fn crash_reports_are_recorded_unless_the_file_says_otherwise() {
    let home = TempDir::new().unwrap();

    let config = config::load(&home.path().join(config::FILE_NAME)).unwrap();
    assert_eq!(config.crash, Crash::default());
    assert!(config.crash.enabled);

    let path = write(&home, "[crash]\nenabled = false\n");
    let config = config::load(&path).unwrap();
    assert!(!config.crash.enabled);
}

// ---------------------------------------------------------------------------------------------
// Writing `[paths]` — roadmap task T143.
// ---------------------------------------------------------------------------------------------

/// What a caller asks for, with only `data` set.
///
/// **Relative, and on all three systems deliberately.** `/bulk/data` is an ordinary absolute path on
/// macOS and Linux and a *drive-less root* on Windows, which `[paths]` refuses outright — so a test
/// written with one passes on two systems and fails on the third for a reason that has nothing to do
/// with what it is testing. Measured: CI went red on exactly this. A relative value is what `[paths]`
/// documents first and is the same string everywhere; the absolute case is covered by
/// `mixengine-daemon`'s suite, which builds one from a `TempDir`.
fn asking_for_data(directory: &str) -> RequestedPaths {
    RequestedPaths {
        data: Some(PathBuf::from(directory)),
        ..RequestedPaths::default()
    }
}

/// The section is created when the file has none, and only the key asked for is written.
#[test]
fn a_key_is_written_into_a_file_that_had_no_paths_section() {
    let home = TempDir::new().unwrap();
    let path = write(&home, "[log]\nlevel = \"warn\"\n");

    let written = config::set_paths(&path, &asking_for_data("bulk/data")).unwrap();

    assert_eq!(written, vec!["data"]);

    let config = config::load(&path).unwrap();
    assert_eq!(config.paths.data, Some(PathBuf::from("bulk/data")));
    assert_eq!(config.paths.runtimes, None);
    assert_eq!(config.paths.packages, None);
    assert_eq!(config.paths.logs, None);

    // The rest of the file is still the user's.
    assert_eq!(config.log.level, LogLevel::Warn);
}

/// **The reason this is a `toml_edit` document and not a re-serialised `Config`.** The template is
/// sixty lines of commented explanation of the very keys being written, and a writer that threw
/// them away would leave the next reader a file that documents nothing.
#[test]
fn every_comment_in_the_template_survives_a_write() {
    let home = TempDir::new().unwrap();
    let path = write(&home, TEMPLATE);

    let commented = |text: &str| {
        text.lines()
            .filter(|line| line.trim_start().starts_with('#'))
            .count()
    };

    let before = commented(TEMPLATE);
    config::set_paths(&path, &asking_for_data("bulk/data")).unwrap();
    let after = std::fs::read_to_string(&path).unwrap();

    assert_eq!(commented(&after), before, "{after}");
    assert!(
        after.contains("#runtimes = \"bulk/runtimes\""),
        "the commented examples stay where the template put them: {after}"
    );
    assert_eq!(
        config::load(&path).unwrap().paths.data,
        Some(PathBuf::from("bulk/data"))
    );
}

/// A second write replaces the value rather than adding a second key of the same name — which TOML
/// refuses to parse at all, so this failing would be a file nothing can read.
#[test]
fn writing_the_same_key_twice_replaces_it() {
    let home = TempDir::new().unwrap();
    let path = write(&home, TEMPLATE);

    config::set_paths(&path, &asking_for_data("bulk/first")).unwrap();
    config::set_paths(&path, &asking_for_data("bulk/second")).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.matches("\ndata = ").count(),
        1,
        "two keys of one name is a file TOML will not read: {text}"
    );
    assert_eq!(
        config::load(&path).unwrap().paths.data,
        Some(PathBuf::from("bulk/second"))
    );
}

/// A comment somebody wrote themselves is still attached to what they wrote it about.
#[test]
fn a_comment_a_person_added_stays_where_they_put_it() {
    let home = TempDir::new().unwrap();
    let path = write(
        &home,
        "# the big disk arrived 2026-09-16\n[paths]\nruntimes = \"bulk/runtimes\"\n",
    );

    config::set_paths(&path, &asking_for_data("bulk/data")).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("# the big disk arrived 2026-09-16\n[paths]"),
        "{text}"
    );

    // And the key that was already there is untouched by a request that did not mention it.
    let config = config::load(&path).unwrap();
    assert_eq!(config.paths.runtimes, Some(PathBuf::from("bulk/runtimes")));
    assert_eq!(config.paths.data, Some(PathBuf::from("bulk/data")));
}

/// **The same rule through the other door.** Every value `config::load` refuses, `set_paths`
/// refuses — and nothing is written when it does, so a refused request leaves the file as it was.
#[test]
fn the_writer_refuses_exactly_what_the_reader_refuses() {
    for nowhere in NAMES_NOTHING {
        let home = TempDir::new().unwrap();
        let path = write(&home, TEMPLATE);

        let error = config::set_paths(&path, &asking_for_data(nowhere)).unwrap_err();

        assert!(
            matches!(error, mixengine_core::Error::ConfigEdit { .. }),
            "data = {nowhere:?} was written: {error:?}"
        );
        assert!(reported(&error).contains("data"), "{error}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            TEMPLATE,
            "a refused value left the file changed"
        );
    }
}

/// Nothing asked for is nothing written, and the file is not even opened for writing.
#[test]
fn an_empty_request_writes_nothing() {
    let home = TempDir::new().unwrap();
    let path = write(&home, TEMPLATE);

    assert!(RequestedPaths::default().is_empty());
    assert!(
        config::set_paths(&path, &RequestedPaths::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
}

/// **The silent no-op.** A launchd plist carries its flags for the life of the plist, so asking for
/// what the file already says has to be a request that changes nothing.
#[test]
fn asking_for_what_the_file_already_says_differs_in_nothing() {
    let held = PathOverrides {
        data: Some(PathBuf::from("bulk/data")),
        ..PathOverrides::default()
    };

    assert!(
        asking_for_data("bulk/data")
            .differing_from(&held)
            .is_empty()
    );
    assert_eq!(
        asking_for_data("bulk/elsewhere").differing_from(&held),
        vec!["data"]
    );
    assert_eq!(
        asking_for_data("bulk/data").differing_from(&PathOverrides::default()),
        vec!["data"],
        "a key the file does not set at all is a key this request changes"
    );
    assert!(
        RequestedPaths::default().differing_from(&held).is_empty(),
        "a key nobody mentioned is not a key anybody changed"
    );
}
