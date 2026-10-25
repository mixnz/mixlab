use mixengine_proto::{PackageVersion, Timestamp};

use super::*;

/// The line names both numbers and the ending, so nobody has to infer either.
///
/// **The `false` case is the one worth a test.** A database over its ceiling is warned about and
/// deliberately left alone, and a rendering that said only "watched" would read exactly like one
/// that was about to rescue it.
#[test]
fn a_watchdog_line_says_what_happens_at_the_end_of_it() {
    let restarted = super::watchdog_line(Some(MemoryWatchdog {
        after_minutes: 3,
        restarts: true,
    }));

    assert!(restarted.contains('3'), "{restarted}");
    assert!(restarted.contains("restarted after"), "{restarted}");

    let warned = super::watchdog_line(Some(MemoryWatchdog {
        after_minutes: 3,
        restarts: false,
    }));

    assert!(
        warned.contains("not restarted automatically"),
        "a service that is only warned about must not read as one that is rescued: {warned}"
    );

    assert!(
        super::watchdog_line(None).is_empty(),
        "nothing watching is no line at all, not an empty one"
    );
}

pub(in crate::render) fn id(value: &str) -> ServiceId {
    ServiceId::parse(value).expect("a valid service id")
}

/// A summary in the shape a given state implies: a running service has a process and a
/// supervisor, a stopped one has neither, and one with no row has nothing at all.
pub(in crate::render) fn summary(id_value: &str, state: Option<ServiceState>) -> ServiceSummary {
    let running = state == Some(ServiceState::Running);

    ServiceSummary {
        id: id(id_value),
        state,
        supervised: running,
        pid: running.then_some(4123),
        port: None,
        last_started_at: running.then_some(Timestamp(1_723_000_000_000)),
        last_exit_code: None,
        depends_on: Vec::new(),
        role: Some(mixengine_proto::ServiceRole::Other {}),
        port_movable: None,
        autostart: false,
        stopped_by: None,
        version: None,
        last_failure: None,
    }
}

/// A create that got the port it asked for says so, and explains nothing.
#[test]
fn a_service_created_on_the_port_it_wanted_is_reported_without_a_story() {
    let rendered = service_creation(&ServiceCreation {
        service: ServiceSummary {
            port: Some(3306),
            ..summary("mariadb@main", Some(ServiceState::Stopped))
        },
        moved_from: None,
    });

    assert!(rendered.contains("created mariadb@main"), "{rendered}");
    assert!(rendered.contains("port 3306"), "{rendered}");
    assert!(
        !rendered.contains("moved"),
        "nothing moved it, so nothing should say so: {rendered}"
    );
}

/// One that was moved names the port it wanted and the program that has it.
///
/// The whole point of the field: a developer whose `.env` says 3306 finds out here rather than
/// from a connection refused an hour later.
#[test]
fn a_service_moved_off_its_preferred_port_names_the_program_that_took_it() {
    let rendered = service_creation(&ServiceCreation {
        service: ServiceSummary {
            port: Some(3307),
            ..summary("mysql@main", Some(ServiceState::Stopped))
        },
        moved_from: Some(mixengine_proto::PortMoved {
            preferred: 3306,
            pid: Some(4242),
            program: Some("mysqld.exe".to_owned()),
        }),
    });

    assert!(rendered.contains("port 3307"), "{rendered}");
    assert!(rendered.contains("asked for 3306"), "{rendered}");
    assert!(rendered.contains("mysqld.exe has it"), "{rendered}");
}

/// A machine that will name neither the program nor the pid still says what happened.
///
/// Which is the ordinary case for a port another *MixEngine* service holds: the row has it and
/// there may be no process at all.
#[test]
fn a_move_with_nothing_to_name_still_says_the_port_was_taken() {
    let rendered = service_creation(&ServiceCreation {
        service: ServiceSummary {
            port: Some(3307),
            ..summary("mysql@main", Some(ServiceState::Stopped))
        },
        moved_from: Some(mixengine_proto::PortMoved {
            preferred: 3306,
            pid: None,
            program: None,
        }),
    });

    assert!(
        rendered.contains("another service or program on this machine has it"),
        "{rendered}"
    );
}

/// A failed service with a note, the way the daemon reports one.
pub(in crate::render) fn failed_with(id_value: &str, detail: &str) -> ServiceSummary {
    ServiceSummary {
        last_failure: Some(mixengine_proto::ServiceFailureNote {
            at: Timestamp(1_760_000_000_000),
            reason: mixengine_proto::StateReason::SpawnFailed,
            detail: detail.to_owned(),
        }),
        ..summary(id_value, Some(ServiceState::Failed))
    }
}

/// The listing ends with why each failed service failed.
#[test]
fn a_failed_service_is_followed_by_why() {
    let rendered = service_list(&ServiceList {
        services: vec![
            failed_with("php-fpm@phpmyadmin", "no credential is stored at x"),
            summary("caddy", Some(ServiceState::Running)),
        ],
    });

    assert!(
        rendered.ends_with(
            "php-fpm@phpmyadmin failed: no credential is stored at x
"
        ),
        "{rendered}"
    );
    assert!(!rendered.contains("caddy failed"), "{rendered}");
}

#[test]
fn the_listing_is_a_table_whose_columns_line_up_whatever_the_names_are() {
    let list = ServiceList {
        services: vec![
            ServiceSummary {
                version: Some(PackageVersion::parse("11.4.3").expect("a version")),
                ..summary("mariadb@main", Some(ServiceState::Running))
            },
            ServiceSummary {
                depends_on: vec![id("mariadb@main")],
                ..summary("php", Some(ServiceState::Stopped))
            },
        ],
    };

    let rendered = service_list(&list);
    let lines: Vec<&str> = rendered.lines().collect();

    assert_eq!(
        lines[0],
        "SERVICE       VERSION  STATE    AUTOSTART  SUPERVISED  PID   DEPENDS ON"
    );
    assert_eq!(
        lines[1],
        "mariadb@main  11.4.3   running  no         yes         4123  —"
    );
    assert_eq!(
        lines[2],
        "php           —        stopped  no         no          —     mariadb@main"
    );
}

/// A status names the version beside the other facts, and a service with none prints no empty line
/// for it.
#[test]
fn a_status_names_the_version_the_service_runs() {
    let known = service_status(&ServiceSummary {
        version: Some(PackageVersion::parse("5.7.44").expect("a version")),
        ..summary("mysql@main", Some(ServiceState::Stopped))
    });
    assert!(known.contains("  version     5.7.44\n"), "{known}");

    let unknown = service_status(&summary("mysql@main", Some(ServiceState::Stopped)));
    assert!(!unknown.contains("version"), "{unknown}");
}

#[test]
fn a_home_with_no_declarations_says_so_rather_than_printing_a_bare_heading() {
    assert_eq!(
        service_list(&ServiceList {
            services: Vec::new()
        }),
        "no services are declared in this home\n"
    );
}

#[test]
fn a_service_that_was_never_created_is_told_apart_from_one_that_is_stopped() {
    let rendered = service_status(&summary("mailpit", None));

    assert!(rendered.starts_with("mailpit: not created"), "{rendered}");
    assert!(rendered.contains("has never been created"), "{rendered}");

    // The ordinary case says nothing extra, because a note on every status is a note nobody
    // reads — the same rule `mix status` follows for a daemon from another build.
    let stopped = service_status(&summary("mailpit", Some(ServiceState::Stopped)));
    assert!(!stopped.contains("note"), "{stopped}");
}

#[test]
fn a_start_time_that_outlived_its_run_is_labelled_as_history_rather_than_as_the_present() {
    let running = service_status(&summary("mariadb@main", Some(ServiceState::Running)));
    assert!(running.contains("started"), "{running}");
    assert!(!running.contains("last start"), "{running}");

    // The same field, and the daemon keeps it across a stop on purpose — so the rendering is
    // what has to stop claiming the service is in the run it names.
    let stopped = ServiceSummary {
        last_started_at: Some(Timestamp(1_723_000_000_000)),
        ..summary("mariadb@main", Some(ServiceState::Stopped))
    };

    let rendered = service_status(&stopped);
    assert!(rendered.contains("last start"), "{rendered}");
    assert!(!rendered.contains("started"), "{rendered}");
}

#[test]
fn a_row_naming_a_process_nothing_is_supervising_is_pointed_at_rather_than_smoothed_over() {
    let orphan = ServiceSummary {
        supervised: false,
        ..summary("mariadb@main", Some(ServiceState::Running))
    };

    let rendered = service_status(&orphan);
    assert!(rendered.contains("supervised  no"), "{rendered}");
    assert!(rendered.contains("daemon which was killed"), "{rendered}");
}

#[test]
fn a_walk_that_reached_everything_is_one_line() {
    let walk = ServiceWalk {
        planned: vec![id("mariadb@main"), id("php-fpm@8.3")],
        complete: true,
        reached: vec![id("mariadb@main"), id("php-fpm@8.3")],
        failed: None,
        blocked: Vec::new(),
    };

    assert_eq!(
        service_walk(Walked::Start, &walk),
        "started mariadb@main, php-fpm@8.3\n"
    );
    assert_eq!(
        service_walk(Walked::Stop, &walk),
        "stopped mariadb@main, php-fpm@8.3\n"
    );
}

/// A walk's failure prints the runner's detail before the reason's own sentence.
#[test]
fn a_walks_failure_prints_the_runners_detail_first() {
    let walk = ServiceWalk {
        planned: vec![id("db")],
        complete: true,
        reached: vec![],
        failed: Some(mixengine_proto::ServiceFailure {
            service: id("db"),
            reason: Some(StateReason::SpawnFailed),
            detail: Some(
                "the environment entry MYSQL_PWD: no credential is stored at mixengine/h/db/root"
                    .to_owned(),
            ),
        }),
        blocked: vec![],
    };

    let rendered = service_walk(Walked::Start, &walk);

    assert!(
        rendered.contains("db failed to start: the environment entry MYSQL_PWD"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("could not be started at all"),
        "{rendered}"
    );
}

#[test]
fn a_walk_that_stopped_leads_with_the_service_to_fix_and_shows_what_it_took_down() {
    let walk = ServiceWalk {
        planned: vec![id("db"), id("web"), id("worker")],
        complete: true,
        reached: vec![id("db")],
        failed: Some(mixengine_proto::ServiceFailure {
            service: id("web"),
            detail: None,
            reason: Some(StateReason::CrashLoop {
                attempts: 5,
                window: mixengine_proto::Millis::from_secs(300),
                tail: vec!["Address already in use".to_owned()],
            }),
        }),
        blocked: vec![id("worker")],
    };

    let rendered = service_walk(Walked::Start, &walk);
    let lines: Vec<&str> = rendered.lines().collect();

    // The name of the thing to fix is the first thing on the screen, and the evidence is
    // directly under it — five lines of `started` above both would be five lines in the way.
    assert_eq!(lines[0], "web failed to start: 5 failed starts within 5m");
    assert_eq!(lines[1], "    Address already in use");
    assert_eq!(lines[2], "  started   db");
    assert_eq!(lines[3], "  blocked   worker");
}

/// **A refused superuser is shown with the command that repairs it.**
///
/// `mix` naming one of `mix`'s own verbs is not business logic in a client: the daemon said what
/// happened, and which command undoes it is the thing only this binary knows. A `ServiceWalk`
/// carries no hint field, so this is where it can be said at all.
#[test]
fn a_refused_superuser_is_shown_with_the_repair_under_it() {
    let walk = ServiceWalk {
        planned: vec![id("postgres@main")],
        complete: true,
        reached: Vec::new(),
        failed: Some(mixengine_proto::ServiceFailure {
            service: id("postgres@main"),
            detail: None,
            reason: Some(StateReason::SuperuserRefused {
                said: "FATAL:  password authentication failed for user \"postgres\"".to_owned(),
            }),
        }),
        blocked: Vec::new(),
    };

    let rendered = service_walk(Walked::Start, &walk);
    let lines: Vec<&str> = rendered.lines().collect();

    assert_eq!(
        lines[0],
        "postgres@main failed to start: it refuses the superuser password this home holds"
    );
    assert_eq!(
        lines[1],
        "    FATAL:  password authentication failed for user \"postgres\""
    );
    assert!(
        lines[2].contains("mix service reset-credential postgres@main"),
        "{rendered}"
    );
}

#[test]
fn a_walk_nobody_waited_for_says_it_is_still_happening() {
    let accepted = ServiceWalk {
        planned: vec![id("db")],
        complete: false,
        reached: Vec::new(),
        failed: None,
        blocked: Vec::new(),
    };

    assert_eq!(
        service_walk(Walked::Restart, &accepted),
        "accepted; mixengined is restarting db in the background\n"
    );
}

/// A report for `fakeservice@main` with `source` and nothing else varied.
fn idle_report(source: mixengine_proto::IdleSource) -> IdleReport {
    IdleReport {
        service: mixengine_proto::ServiceId::parse("fakeservice@main").expect("an id"),
        policy: None,
        source,
        exempt: Vec::new(),
    }
}

/// The three ways a service is never idle-stopped read as three different sentences.
///
/// **The one that matters is `Unmeasurable`**, which was found by a failing test rather than
/// designed: a php-fpm pool on a Unix socket has no port to count, so a person who has just
/// typed `--after 30m` would otherwise be told only "never" — the outcome without the reason,
/// which is an invitation to type it again.
#[test]
fn the_three_ways_of_never_idling_are_three_different_sentences() {
    let unset = service_idle(&idle_report(mixengine_proto::IdleSource::Unset));
    let never = service_idle(&idle_report(mixengine_proto::IdleSource::Never));
    let unmeasurable = service_idle(&idle_report(mixengine_proto::IdleSource::Unmeasurable));

    for rendered in [&unset, &never, &unmeasurable] {
        assert!(rendered.contains("never"), "{rendered}");
    }

    assert!(unset.contains("no default yet"), "{unset}");
    assert!(never.contains("switched off"), "{never}");
    assert!(
        unmeasurable.contains("nothing to measure"),
        "asked for and unmeasurable is not the same answer as nobody asking: {unmeasurable}"
    );
}

/// An exemption names the thing a person would have to go and change.
#[test]
fn an_exemption_names_what_is_holding_the_service_open() {
    let report = IdleReport {
        exempt: vec![
            IdleExemption::DependentRunning {
                service: mixengine_proto::ServiceId::parse("php-fpm@8.3").expect("an id"),
            },
            IdleExemption::ProjectKeptWarm {
                project: "shop".to_owned(),
            },
        ],
        ..idle_report(mixengine_proto::IdleSource::Row)
    };

    let rendered = service_idle(&report);

    assert!(rendered.contains("php-fpm@8.3"), "{rendered}");
    assert!(rendered.contains("shop"), "{rendered}");
}

/// A move says where from and where to, whether it restarted, and lists what MixEngine left alone.
#[test]
fn service_set_lists_what_was_not_followed() {
    let restarted = super::service_set(&mixengine_proto::ServiceSetResult {
        service: summary("mysql@8", Some(ServiceState::Running)),
        port: Some(mixengine_proto::PortChange {
            from: 3307,
            to: 3306,
        }),
        restarted: true,
        not_followed: vec!["a project's .env that names 3307".to_owned()],
    });

    assert!(
        restarted.starts_with("mysql@8 moved from 3307 to 3306, and was restarted."),
        "{restarted}"
    );
    assert!(
        restarted.contains("Not changed by MixEngine:"),
        "{restarted}"
    );
    assert!(
        restarted.contains("  - a project's .env that names 3307"),
        "{restarted}"
    );

    let stopped = super::service_set(&mixengine_proto::ServiceSetResult {
        service: summary("mysql@8", Some(ServiceState::Stopped)),
        port: Some(mixengine_proto::PortChange {
            from: 3307,
            to: 3306,
        }),
        restarted: false,
        not_followed: Vec::new(),
    });

    assert!(
        stopped.starts_with(
            "mysql@8 moved from 3307 to 3306. It is stopped, and will listen there when it starts."
        ),
        "{stopped}"
    );

    let unchanged = super::service_set(&mixengine_proto::ServiceSetResult {
        service: summary("mysql@8", Some(ServiceState::Stopped)),
        port: Some(mixengine_proto::PortChange {
            from: 3306,
            to: 3306,
        }),
        restarted: false,
        not_followed: Vec::new(),
    });

    assert_eq!(unchanged, "mysql@8 is already on 3306.\n");
}
