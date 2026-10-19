//! The decisions one life of a service makes, each tested without a process.

use mixengine_proto::Millis;

use super::*;

fn conflict() -> StateReason {
    StateReason::PortInUse {
        port: 3306,
        pid: Some(42),
        program: Some("mysqld".to_owned()),
    }
}

fn refused() -> StateReason {
    StateReason::SuperuserRefused {
        said: "Access denied for user 'root'@'localhost'".to_owned(),
    }
}

fn crash_loop() -> StateReason {
    StateReason::CrashLoop {
        attempts: 3,
        window: Millis::from_secs(60),
        tail: Vec::new(),
    }
}

#[test]
fn a_conflict_outranks_a_refusal_and_a_refusal_a_timeout() {
    let after = Millis::from_secs(15);

    assert_eq!(
        timed_out_reason(Some(conflict()), Some(refused()), after),
        conflict(),
        "another program on the port is who the check was talking to"
    );
    assert_eq!(timed_out_reason(Some(conflict()), None, after), conflict());
    assert_eq!(timed_out_reason(None, Some(refused()), after), refused());
    assert_eq!(
        timed_out_reason(None, None, after),
        StateReason::ReadyTimeout { after }
    );
}

#[test]
fn a_conflict_replaces_an_exit_but_never_a_crash_loop() {
    let exited = StateReason::Exited { code: Some(1) };

    assert_eq!(
        given_up_reason(exited.clone(), Some(conflict())),
        conflict()
    );
    assert_eq!(given_up_reason(exited.clone(), None), exited);
    assert_eq!(
        given_up_reason(crash_loop(), Some(conflict())),
        crash_loop(),
        "a crash loop carries its own count and the lines the service printed"
    );
}

#[test]
fn a_restart_names_the_conflict_or_the_exit() {
    assert_eq!(restarting_reason(Some(conflict()), Some(1)), conflict());
    assert_eq!(
        restarting_reason(None, Some(1)),
        StateReason::Exited { code: Some(1) }
    );
}

#[test]
fn probe_action_covers_every_answer() {
    let busy = || {
        Err(mixengine_supervisor::Error::Spawn {
            program: "mariadb-admin".to_owned(),
            source: std::io::Error::other("the binary is being replaced"),
        })
    };
    let unsupported = Err(mixengine_supervisor::Error::UnsupportedCheck {
        check: "unix_socket",
        reason: "this system has no Unix sockets".to_owned(),
    });

    assert_eq!(
        probe_action(&Ok(Some(Verdict::Degraded)), false),
        ProbeAction::Degraded
    );
    assert_eq!(
        probe_action(&Ok(Some(Verdict::Recovered)), false),
        ProbeAction::Recovered
    );
    assert_eq!(probe_action(&Ok(None), false), ProbeAction::Nothing);
    assert_eq!(
        probe_action(&busy(), false),
        ProbeAction::RetryLater { say: true },
        "a fault that might pass is said once"
    );
    assert_eq!(
        probe_action(&busy(), true),
        ProbeAction::RetryLater { say: false },
        "and then only counted"
    );
    assert_eq!(
        probe_action(&unsupported, false),
        ProbeAction::StopProbing,
        "a probe that cannot be made here is never made again"
    );
}
