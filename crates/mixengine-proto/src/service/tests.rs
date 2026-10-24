use std::net::SocketAddr;

use super::*;
use crate::Millis;

/// The MySQL and Redis probes travel under names a client can read.
#[test]
fn the_protocol_probes_travel_by_their_own_names() {
    let addr: SocketAddr = "127.0.0.1:3306".parse().expect("an address");

    for (probe, tag) in [
        (HealthProbe::MysqlGreeting { addr }, "mysql_greeting"),
        (HealthProbe::RedisPing { addr }, "redis_ping"),
    ] {
        let json = serde_json::to_value(&probe).expect("serialised");
        assert_eq!(json["type"], tag, "{json}");

        let back: HealthProbe = serde_json::from_value(json).expect("read back");
        assert_eq!(back, probe);
    }
}

fn spec() -> ServiceSpecBuilder {
    ServiceSpec::builder(
        ServiceId::parse("mariadb@main").unwrap(),
        absolute("packages/mariadb/11.4.3/bin/mariadbd"),
    )
    .cwd(absolute("data/mariadb/main"))
    .ready(ReadyCheck::Tcp {
        addr: "127.0.0.1:3306".parse().unwrap(),
        timeout: Millis::from_secs(30),
    })
}

/// **The namespace is a published fact, not a private constant.** It is spelled out here because
/// another application reads credentials under it, and a word two programs agree on is a word one
/// of them must not be free to change quietly.
#[test]
fn the_keyring_namespace_is_the_one_word_both_applications_agree_on() {
    assert_eq!(KEYRING_SERVICE, "mixengine");
}

/// What a failed start is diagnosed against.
///
/// The recipe rendering a spec knows the number it wrote into the configuration, and nothing
/// downstream of it does: a `ReadyCheck` names an address only for the services that are checked
/// by connecting to one, and a database checked by running a query names none.
#[test]
fn a_spec_carries_the_ports_its_service_will_listen_on() {
    let declared = spec().ports([3306]).build().unwrap();
    assert_eq!(declared.ports(), [3306]);
}

/// A service that binds nothing is the ordinary case, not a spec that forgot to say.
#[test]
fn a_spec_declares_no_ports_by_default() {
    assert!(spec().build().unwrap().ports().is_empty());
}

/// A recipe's permission, and the default that is every other service.
///
/// **Asserted in both directions**, because the interesting half is the default: a spec that
/// says nothing must produce a service no watchdog will ever restart.
#[test]
fn a_spec_says_whether_a_watchdog_may_restart_it() {
    assert!(
        !spec().build().unwrap().restart_over_memory(),
        "a service is not restarted for its size unless its recipe asks"
    );

    assert!(
        spec()
            .restart_over_memory(true)
            .build()
            .unwrap()
            .restart_over_memory()
    );
}

#[test]
fn an_id_splits_into_a_name_and_an_optional_instance() {
    let instanced = ServiceId::parse("mariadb@main").unwrap();
    assert_eq!(instanced.name(), "mariadb");
    assert_eq!(instanced.instance(), Some("main"));

    let plain = ServiceId::parse("caddy").unwrap();
    assert_eq!(plain.name(), "caddy");
    assert_eq!(plain.instance(), None);
}

#[test]
fn the_ids_the_documentation_uses_are_all_valid() {
    for id in [
        "caddy",
        "mariadb@main",
        "php-fpm@8.3",
        "redis@main",
        "nginx",
    ] {
        assert!(ServiceId::parse(id).is_ok(), "{id} should parse");
    }
}

/// Every one of these would become a directory under `logs/services/`.
#[test]
fn an_id_cannot_be_something_that_is_not_a_directory_name() {
    for id in [
        "",
        "..",
        ".",
        "a/b",
        "a\\b",
        "MariaDB", // case would collide on a case-insensitive filesystem
        "-leading",
        "@main",
        "mariadb@",
        "mariadb@a@b",
        "php.fpm",    // a dot belongs to the instance, not the name
        "con",        // reserved on Windows
        "php-fpm@8.", // Windows strips the trailing dot, colliding with `php-fpm@8`
    ] {
        assert!(ServiceId::parse(id).is_err(), "{id:?} should be refused");
    }
}

/// The whole list, because a gap in it is invisible until a directory cannot be created — and
/// only on Windows. `lpt8` and `lpt9` were the gap.
#[test]
fn every_windows_device_name_is_refused() {
    let mut devices = vec![
        "con".to_owned(),
        "prn".to_owned(),
        "aux".to_owned(),
        "nul".to_owned(),
    ];
    for number in 0..=9 {
        devices.push(format!("com{number}"));
        devices.push(format!("lpt{number}"));
    }

    for device in devices {
        assert!(
            ServiceId::parse(device.as_str()).is_err(),
            "{device} is a device name on Windows and cannot be a directory"
        );
    }
}

/// The rule is about the directory name, which is the whole id — and Windows refuses `con`, not
/// every name that contains one. Checking each half separately turned `mariadb@aux` into an
/// error about a device nobody was naming.
#[test]
fn a_name_that_merely_contains_a_device_name_is_an_ordinary_id() {
    for id in ["mariadb@aux", "redis@com1", "lpt1@main", "console"] {
        assert!(ServiceId::parse(id).is_ok(), "{id} should parse");
    }
}

#[test]
fn an_id_that_is_not_a_directory_name_is_refused_on_the_way_in_too() {
    let error = serde_json::from_str::<ServiceId>(r#""../../etc""#)
        .unwrap_err()
        .to_string();

    assert!(error.contains("not a valid service id"), "{error}");
}

/// A built spec is read, never edited: the checks in `build` are only worth running if nothing
/// afterwards can undo them. This test is what fails if a field is ever made `pub` again — it
/// stops compiling rather than asserting.
#[test]
fn a_built_spec_is_read_through_accessors() {
    let built = spec()
        .arg("--defaults-file")
        .env("TZ", "UTC")
        .build()
        .unwrap();

    assert_eq!(built.id().as_str(), "mariadb@main");
    assert!(built.program().is_absolute());
    assert!(built.cwd().is_absolute());
    assert_eq!(built.args(), ["--defaults-file"]);
    assert_eq!(built.env()["TZ"], EnvValue::literal("UTC"));
    assert!(built.health().is_none());
    assert!(built.reload().is_none());
    assert!(built.idle().is_none());
    assert!(built.depends_on().is_empty());
    assert_eq!(built.restart(), RestartPolicy::default());
    assert_eq!(built.stop(), &StopBehaviour::default());
    assert_eq!(built.limits(), ResourceLimits::default());
    assert_eq!(built.logs(), LogPolicy::default());
    assert!(matches!(built.ready(), ReadyCheck::Tcp { .. }));
}

#[test]
fn a_spec_round_trips_through_json() {
    let built = spec()
        .args(["--defaults-file", "my.cnf"])
        .env("TZ", "UTC")
        .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb@main/root")
        .reload(ReloadBehaviour::Command {
            program: absolute("packages/caddy/2.11.4/caddy"),
            args: vec!["reload".to_owned()],
            patience: Millis::from_secs(30),
        })
        .depends_on(ServiceId::parse("caddy").unwrap())
        .build()
        .unwrap();

    let encoded = serde_json::to_string(&built).unwrap();
    assert_eq!(
        serde_json::from_str::<ServiceSpec>(&encoded).unwrap(),
        built
    );
}

/// A spec off the wire skips the builder, so the checks have to be reachable without one —
/// otherwise the loader that is supposed to run them would have to restate them, and the two
/// definitions of "usable" would drift.
#[test]
fn a_spec_that_skipped_the_builder_can_still_be_checked() {
    let built = spec().build().unwrap();
    assert!(built.validate().is_ok());

    let mut encoded = serde_json::to_value(&built).unwrap();
    encoded["program"] = serde_json::Value::String("mariadbd".to_owned());

    let deserialised = serde_json::from_value::<ServiceSpec>(encoded).unwrap();
    assert!(
        matches!(deserialised.validate(), Err(SpecError::Invalid { ref field, .. }) if field == "program")
    );
}

/// The point of [`EnvValue`]: the password is not in the spec, so it cannot leave with it.
#[test]
fn a_serialised_spec_names_a_credential_instead_of_carrying_one() {
    let built = spec()
        .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb@main/root")
        .build()
        .unwrap();

    let encoded = serde_json::to_value(&built).unwrap();
    let value = &encoded["env"]["MARIADB_ROOT_PASSWORD"];

    assert_eq!(value["from"], "keyring");
    assert_eq!(value["service"], "mixengine");
    assert_eq!(value["key"], "mariadb@main/root");
    assert!(value.get("value").is_none(), "{value}");
}

/// What an `extension.toml` author writes. The tagged form is still what the type *emits*, so
/// the wire and the database are unchanged.
#[test]
fn a_literal_environment_value_can_be_written_the_way_a_person_writes_one() {
    assert_eq!(
        serde_json::from_str::<EnvValue>(r#""UTC""#).unwrap(),
        EnvValue::literal("UTC")
    );
    assert_eq!(
        serde_json::from_str::<EnvValue>(r#"{"from":"literal","value":"UTC"}"#).unwrap(),
        EnvValue::literal("UTC")
    );
    assert_eq!(
        serde_json::to_string(&EnvValue::literal("UTC")).unwrap(),
        r#"{"from":"literal","value":"UTC"}"#
    );
}

/// The one mistake that would put a password in a spec. Ignoring the field would let it
/// round-trip out of a manifest looking like it had been accepted.
#[test]
fn a_keyring_entry_cannot_also_carry_the_credential() {
    let error = serde_json::from_str::<EnvValue>(
        r#"{"from":"keyring","service":"mixengine","key":"mariadb@main/root","value":"hunter2"}"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("password written into a spec"), "{error}");
    assert!(!error.contains("hunter2"), "{error}");
}

#[test]
fn an_environment_value_refuses_what_it_cannot_mean() {
    for json in [
        r#"{"from":"secret","service":"a","key":"b"}"#, // no such source
        r#"{"from":"keyring","service":"mixengine"}"#,  // no key
        r#"{"from":"literal"}"#,                        // no value
        r#"{"value":"UTC","service":"mixengine"}"#,     // a literal naming a keyring entry
        r#"{"from":"literal","value":"UTC","secret":"x"}"#, // should not look accepted
    ] {
        assert!(
            serde_json::from_str::<EnvValue>(json).is_err(),
            "{json} should be refused"
        );
    }
}

/// `Debug` reaches logs and bug reports, and there is nothing in a spec that must not.
#[test]
fn debug_shows_a_credential_by_name_and_has_no_value_to_show() {
    let built = spec()
        .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb@main/root")
        .build()
        .unwrap();

    let rendered = format!("{built:?}");
    assert!(rendered.contains("Keyring"), "{rendered}");
    assert!(rendered.contains("mariadb@main/root"), "{rendered}");
}

#[test]
fn a_spec_insists_on_the_two_fields_with_no_sensible_default() {
    let id = ServiceId::parse("caddy").unwrap();
    let program = absolute("packages/caddy/2.8.4/caddy");

    let no_cwd = ServiceSpec::builder(id.clone(), &program)
        .ready(ReadyCheck::PidAlive {
            settle: Millis::from_secs(1),
        })
        .build();
    assert!(matches!(no_cwd, Err(SpecError::Missing { ref field, .. }) if field == "cwd"));

    let no_ready = ServiceSpec::builder(id, &program)
        .cwd(absolute("etc/caddy"))
        .build();
    assert!(matches!(no_ready, Err(SpecError::Missing { ref field, .. }) if field == "ready"));
}

#[test]
fn a_spec_is_never_resolved_against_a_path() {
    let built = ServiceSpec::builder(ServiceId::parse("caddy").unwrap(), "caddy")
        .cwd(absolute("etc/caddy"))
        .ready(ReadyCheck::PidAlive {
            settle: Millis::from_secs(1),
        })
        .build();

    assert!(matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "program"));
}

/// `program` is not the only program a spec can spawn, and the other three go through the same
/// `Command`. A rule enforced on one of four is a rule with three ways around it.
#[test]
fn neither_is_a_command_a_stop_a_health_check_or_a_reload_runs() {
    let stop = spec()
        .stop(StopBehaviour::Command {
            program: "mariadb-admin".into(),
            args: vec!["shutdown".to_owned()],
            grace: Millis::from_secs(10),
        })
        .build();
    assert!(matches!(stop, Err(SpecError::Invalid { ref field, .. }) if field == "stop"));

    let health = spec()
        .health(HealthCheck {
            probe: HealthProbe::Command {
                program: "mariadb-admin".into(),
                args: vec!["ping".to_owned()],
            },
            interval: Millis::from_secs(10),
            timeout: Millis::from_secs(2),
            failures_before_degraded: 3,
            successes_before_running: 1,
        })
        .build();
    assert!(matches!(health, Err(SpecError::Invalid { ref field, .. }) if field == "health"));

    let reload = spec()
        .reload(ReloadBehaviour::Command {
            program: "caddy".into(),
            args: vec!["reload".to_owned()],
            patience: Millis::from_secs(30),
        })
        .build();
    assert!(matches!(reload, Err(SpecError::Invalid { ref field, .. }) if field == "reload"));
}

/// A ready check that runs a program is checked like every other place a spec names one.
///
/// The fourth of the four — `program`, `stop`, `health` and now `ready` — and the reason it is
/// asserted rather than assumed is `neither_is_a_command_a_stop_a_health_check_or_a_reload_runs`
/// above: a rule enforced on three of four is a rule with one way around it.
#[test]
fn a_relative_program_in_a_ready_check_is_refused_like_every_other_one() {
    let built = spec()
        .ready(ReadyCheck::Command {
            program: "mariadb-admin".into(),
            args: vec!["ping".to_owned()],
            timeout: Millis::from_secs(60),
        })
        .build();

    assert!(matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "ready"));
}

/// The deadline is read off whichever field carries it, including this variant's.
#[test]
fn a_command_ready_check_reports_its_own_deadline() {
    let check = ReadyCheck::Command {
        program: absolute("packages/mariadb/11.4.9/bin/mariadb-admin"),
        args: vec!["ping".to_owned()],
        timeout: Millis::from_secs(120),
    };

    assert_eq!(check.timeout(), Millis::from_secs(120));
}

/// A reload is abandoned when its patience runs out, and nothing else happens — so a patience of
/// zero is not a fast reload, it is a reload that is always reported as not done.
#[test]
fn a_reload_given_no_time_is_one_that_could_only_be_abandoned() {
    let built = spec()
        .reload(ReloadBehaviour::Command {
            program: absolute("packages/caddy/2.11.4/caddy"),
            args: vec!["reload".to_owned()],
            patience: Millis(0),
        })
        .build();

    assert!(matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "reload"));
}

/// An empty program is the field left out rather than a path, and says so.
#[test]
fn a_command_with_no_program_is_a_missing_field() {
    let built = spec()
        .stop(StopBehaviour::Command {
            program: PathBuf::new(),
            args: Vec::new(),
            grace: Millis::from_secs(10),
        })
        .build();

    assert!(matches!(built, Err(SpecError::Missing { ref field, .. }) if field == "stop"));
}

/// Every other duration of zero is refused; a grace period is the one where the default —
/// killing the process group — is destructive rather than merely wrong.
#[test]
fn a_graceful_stop_with_no_grace_would_be_a_kill_that_promised_otherwise() {
    for stop in [
        StopBehaviour::Signal { grace: Millis(0) },
        StopBehaviour::Command {
            program: absolute("packages/mariadb/11.4.3/bin/mariadb-admin"),
            args: vec!["shutdown".to_owned()],
            grace: Millis(0),
        },
    ] {
        let built = spec().stop(stop.clone()).build();

        assert!(
            matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "stop"),
            "{stop:?} should be refused"
        );
    }

    // `Kill` has no grace period to be zero, and is the honest way to say it.
    assert!(spec().stop(StopBehaviour::Kill).build().is_ok());
}

/// `cpu_percent` is documented as a percentage *of one core*, so 101 is not a bigger cap — it
/// is a spec whose author meant something the field cannot express.
#[test]
fn a_cpu_cap_cannot_exceed_the_core_it_is_a_percentage_of() {
    for percent in [0, 101, 255] {
        let built = spec()
            .limits(ResourceLimits {
                cpu_percent: Some(percent),
                ..ResourceLimits::default()
            })
            .build();

        assert!(
            matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "limits"),
            "{percent}% should be refused"
        );
    }

    assert!(
        spec()
            .limits(ResourceLimits {
                cpu_percent: Some(100),
                ..ResourceLimits::default()
            })
            .build()
            .is_ok()
    );
}

#[test]
fn a_spec_cannot_depend_on_itself() {
    let built = spec()
        .depends_on(ServiceId::parse("mariadb@main").unwrap())
        .build();

    assert!(matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "depends_on"));
}

#[test]
fn a_spec_cannot_name_a_dependency_twice() {
    let built = spec()
        .depends_on(ServiceId::parse("caddy").unwrap())
        .depends_on(ServiceId::parse("caddy").unwrap())
        .build();

    assert!(matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "depends_on"));
}

#[test]
fn an_environment_variable_cannot_smuggle_a_second_one() {
    let built = spec().env("TZ=UTC\0PATH", "/usr/bin").build();

    assert!(matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "env"));
}

/// The Windows environment block is NUL-separated, so a NUL in a value ends the variable
/// early instead of being part of it.
#[test]
fn an_environment_value_cannot_end_itself_early() {
    let built = spec().env("PATH", "/usr/bin\0TZ=UTC").build();

    assert!(matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "env"));

    // A `=` in a value is ordinary — only the first one separates.
    assert!(spec().env("QUERY", "a=1&b=2").build().is_ok());
}

#[test]
fn a_backoff_that_shrinks_or_starts_at_zero_is_refused() {
    for backoff in [
        Backoff {
            initial: Millis(0),
            ..Backoff::default()
        },
        Backoff {
            initial: Millis::from_secs(60),
            max: Millis::from_secs(30),
            ..Backoff::default()
        },
        Backoff {
            multiplier_percent: 50,
            ..Backoff::default()
        },
        Backoff {
            jitter_percent: 150,
            ..Backoff::default()
        },
        // ±100% reaches zero, which is the retry loop `initial: Millis(0)` is refused for.
        Backoff {
            jitter_percent: 100,
            ..Backoff::default()
        },
    ] {
        let built = spec().restart(RestartPolicy::Always { backoff }).build();

        assert!(
            matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "restart"),
            "{backoff:?} should be refused"
        );
    }

    assert!(
        spec()
            .restart(RestartPolicy::Always {
                backoff: Backoff {
                    jitter_percent: 99,
                    ..Backoff::default()
                }
            })
            .build()
            .is_ok()
    );
}

/// Windows compares environment variable names case-insensitively and Unix does not, so this
/// spec is two variables on one OS and one on the other — the same trap `ServiceId`'s
/// lowercase-only rule closes for directory names.
#[test]
fn two_environment_names_cannot_differ_only_by_case() {
    let built = spec()
        .env("PATH", "/usr/bin")
        .env("Path", "/usr/local/bin")
        .build();

    assert!(matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "env"));

    // Names that differ by more than case are two variables everywhere.
    assert!(
        spec()
            .env("PATH", "/usr/bin")
            .env("PATHEXT", ".EXE")
            .build()
            .is_ok()
    );
}

#[test]
fn health_probes_cannot_be_slower_than_the_gap_between_them() {
    let built = spec()
        .health(HealthCheck {
            probe: HealthProbe::Tcp {
                addr: "127.0.0.1:3306".parse().unwrap(),
            },
            interval: Millis::from_secs(5),
            timeout: Millis::from_secs(10),
            failures_before_degraded: 3,
            successes_before_running: 1,
        })
        .build();

    assert!(matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "health"));
}

#[test]
fn a_never_restart_policy_has_no_backoff_to_check() {
    assert_eq!(RestartPolicy::Never.backoff(), None);
    assert!(spec().restart(RestartPolicy::Never).build().is_ok());
}

/// `OnFailure` with no retries is `Never` with a crash-loop report attached, and a spec that can
/// say one thing two ways is a spec two pieces of code will read differently.
#[test]
fn a_restart_policy_that_never_retries_says_never() {
    let built = spec()
        .restart(RestartPolicy::OnFailure {
            max_retries: 0,
            window: Millis::from_secs(300),
            backoff: Backoff::default(),
        })
        .build();

    assert!(matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "restart"));
}

/// The defaults are the numbers `docs/architecture/process-supervision.md` publishes. If this
/// test needs editing, that document does too.
#[test]
fn the_defaults_are_the_documented_ones() {
    assert_eq!(
        Backoff::default(),
        Backoff {
            initial: Millis(500),
            max: Millis::from_secs(30),
            multiplier_percent: 200,
            jitter_percent: 20,
        }
    );
    assert_eq!(
        LogPolicy::default(),
        LogPolicy {
            max_file_bytes: 10 * 1024 * 1024,
            max_files: 5,
            ring_lines: 500,
        }
    );
    assert!(matches!(
        RestartPolicy::default(),
        RestartPolicy::OnFailure {
            max_retries: 5,
            window,
            ..
        } if window == Millis::from_secs(300)
    ));
}

/// Every enum in this module carries its discriminator inside the object, the way
/// `DaemonEvent` does — one handler in the GUI, and an unknown variant is an object an older
/// client can recognise and ignore.
#[test]
fn every_choice_is_internally_tagged() {
    let ready = serde_json::to_value(ReadyCheck::PidAlive {
        settle: Millis::from_secs(2),
    })
    .unwrap();
    assert_eq!(ready["type"], "pid_alive");
    assert_eq!(ready["settle"], 2000);

    let stop = serde_json::to_value(StopBehaviour::Kill).unwrap();
    assert_eq!(stop["type"], "kill");

    let probe = serde_json::to_value(IdleProbe::Connections { port: 3306 }).unwrap();
    assert_eq!(probe["type"], "connections");

    let probe = serde_json::to_value(IdleProbe::FastCgiStatus {
        socket: std::path::PathBuf::from("/run/php-fpm-8.3.sock"),
        path: "/mixengine-status".to_owned(),
    })
    .unwrap();
    assert_eq!(probe["type"], "fast_cgi_status");
}
/// A reload by signal is a spec like any other, and its patience is checked like a command's.
///
/// Zero is not "wait as long as it takes" and not "do not wait": it is a window closed before
/// anything could have happened in it, which would report every reload as not done while every
/// reload was in fact happening.
#[test]
fn a_signal_reload_needs_a_window_to_happen_in() {
    let built = spec()
        .reload(ReloadBehaviour::Signal {
            signal: ReloadSignal::Usr2,
            patience: Millis(0),
        })
        .build();

    assert!(
        matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "reload"),
        "{built:?}"
    );

    spec()
        .reload(ReloadBehaviour::Signal {
            signal: ReloadSignal::Usr2,
            patience: Millis(5_000),
        })
        .build()
        .expect("a signal and a window to send it in");
}

/// Zero is not how "uncapped" is spelled, and a service allowed to allocate nothing cannot run.
#[test]
fn a_zero_memory_cap_is_refused_because_none_is_how_uncapped_is_spelled() {
    let limits = ResourceLimits {
        memory_mb: Some(0),
        ..ResourceLimits::default()
    };

    assert!(limits.validate().is_err());
}

/// The ordinary state of a service nobody has capped.
#[test]
fn an_absent_memory_cap_is_accepted() {
    assert!(ResourceLimits::default().validate().is_ok());
}

#[test]
fn a_real_memory_cap_is_accepted() {
    let limits = ResourceLimits {
        memory_mb: Some(512),
        ..ResourceLimits::default()
    };

    assert!(limits.validate().is_ok());
}

/// **`cpu_percent` is deliberately not checked here**, however large it is.
///
/// The rule about it is measured against the number of cores this machine has, and this crate
/// has no way to ask. That refusal is the daemon's — see [`ResourceLimits::validate`].
#[test]
fn a_cpu_percent_this_crate_cannot_judge_is_not_refused_here() {
    let limits = ResourceLimits {
        cpu_percent: Some(255),
        ..ResourceLimits::default()
    };

    assert!(limits.validate().is_ok());
}

/// The check runs where every producer of a spec meets it.
#[test]
fn a_spec_carrying_a_zero_memory_cap_does_not_build() {
    let built = spec()
        .limits(ResourceLimits {
            memory_mb: Some(0),
            ..ResourceLimits::default()
        })
        .build();

    assert!(
        matches!(built, Err(SpecError::Invalid { ref field, .. }) if field == "limits"),
        "{built:?}"
    );
}
/// Four answers out of two inputs, and the two that look alike are the point.
///
/// `Never` and `Unset` are both a service that will not be stopped; `Recipe` and `Row` are both
/// one that will. What separates each pair is which side said so, and a client renders the two
/// sentences differently — "switched off for this service" against "nothing idles this yet".
#[test]
fn an_idle_source_needs_both_the_column_and_the_recipe_to_be_decided() {
    let thirty = Some(Millis::from_secs(30 * 60));
    let policy = IdlePolicy {
        after: Millis::from_secs(30 * 60),
        probe: IdleProbe::Connections { port: 3306 },
    };
    let some = Some(&policy);

    assert_eq!(IdleSource::of(Some(0), None, None), IdleSource::Never);
    assert_eq!(
        IdleSource::of(Some(0), thirty, None),
        IdleSource::Never,
        "zero outranks a recipe default, which is the whole reason zero is stored"
    );

    assert_eq!(IdleSource::of(Some(45), None, some), IdleSource::Row);
    assert_eq!(IdleSource::of(Some(45), thirty, some), IdleSource::Row);

    assert_eq!(IdleSource::of(None, thirty, some), IdleSource::Recipe);
    assert_eq!(
        IdleSource::of(None, None, None),
        IdleSource::Unset,
        "no setting and no recipe default is no policy at all"
    );

    // The state a report without it calls "never": asked for, and nothing to measure with.
    assert_eq!(
        IdleSource::of(Some(45), None, None),
        IdleSource::Unmeasurable,
        "a duration asked for that produced no policy is a recipe with no probe, not a refusal"
    );
    assert_eq!(IdleSource::of(None, thirty, None), IdleSource::Unmeasurable);
}
