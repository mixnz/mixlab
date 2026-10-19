//! What a health probe's answer does to the watch loop, tested without a process.

use super::*;

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
