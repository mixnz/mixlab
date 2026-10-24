use mixengine_platform::{
    AppControlState, Enforcement, Host as _, LimitMechanism, LimitSupport, MemoryMeasure,
    WhenExceeded,
};

use mixengine_proto::ProblemId;

use super::Outcome;

/// **A `Note` and never a `Problem`.** A `Problem` carries a `ProblemId`,
/// and a `ProblemId` is what `daemon.doctor_repair` matches on; automatically deleting a
/// firewall rule MixEngine did not create, and that somebody personally clicked Allow on, is not
/// a repair. So the condition is given no identity a repair could key off, and what it carries
/// is the sentence and the command.
#[test]
fn a_wide_rule_for_this_daemon_is_reported_as_a_note() {
    let outcome =
        super::foreign_rule_outcome(Ok(Some(2)), std::path::Path::new("C:/mixengined.exe"));

    let Outcome::Note { because } = outcome else {
        panic!("a rule Windows wrote is news, not a fault: {outcome:?}");
    };

    assert!(because.contains('2'), "{because}");
    assert!(because.contains("mixengined.exe"), "{because}");
    assert!(
        because.contains("delete rule"),
        "the command to run: {because}"
    );
}

#[test]
fn a_machine_holding_no_such_rule_is_ok() {
    assert!(matches!(
        super::foreign_rule_outcome(Ok(Some(0)), std::path::Path::new("/mixengined")),
        Outcome::Ok {}
    ));
}

/// macOS and Linux have no per-program inbound rule to enumerate, and that is an outcome rather
/// than silence — the rule every per-OS check in this report already follows.
#[test]
fn a_system_with_no_such_mechanism_is_skipped_rather_than_called_clean() {
    let outcome = super::foreign_rule_outcome(Ok(None), std::path::Path::new("/mixengined"));

    let Outcome::Skipped { because } = outcome else {
        panic!("a table that does not exist is not a clean bill of health: {outcome:?}");
    };

    assert!(because.contains("no per-program"), "{because}");
}

/// The mock is what makes any of this reachable: the rule in question is one *Windows* writes,
/// so no test can arrange it on the machine running the suite.
#[test]
fn the_mock_answers_what_a_test_arranged() {
    use mixengine_platform::Host as _;

    let host = mixengine_platform::mock::Host::with_firewall_rules("/mixengine", Some(3));

    assert_eq!(
        host.firewall_rules()
            .naming(std::path::Path::new("/mixengined"))
            .expect("the mock always answers"),
        Some(3)
    );
}

/// A controller this session was not given is a `Note`, carrying the sentence the platform wrote.
#[test]
fn a_controller_this_session_was_not_given_is_a_note() {
    let outcome = super::limit_outcome(&support(
        Enforcement::Unavailable {
            why: "this session does not delegate the cpu controller".to_owned(),
        },
        Enforcement::Hard {
            when: WhenExceeded::Killed,
        },
    ));

    match outcome {
        Outcome::Note { because } => {
            assert!(because.contains("cpu controller"), "{because}");
            assert!(
                !because.contains("memory"),
                "the lent one is not news: {because}"
            );
        }
        other => panic!("a machine without delegation is not broken: {other:?}"),
    }
}

/// **A field this system can never support says nothing at all**, which is the other half of the
/// same judgement: macOS having no hard memory cap is a fact about an operating system, not news
/// about this machine, and a line repeated on every run teaches people to skip the report.
#[test]
fn a_field_this_system_can_never_support_is_not_reported_as_news() {
    let outcome =
        super::limit_outcome(&support(Enforcement::Unsupported, Enforcement::Unsupported));

    assert!(matches!(outcome, Outcome::Ok {}), "{outcome:?}");
}

/// And a machine that lends both says so once, as `Ok`.
#[test]
fn a_machine_that_enforces_both_fields_is_ok() {
    let outcome = super::limit_outcome(&support(
        Enforcement::Hard {
            when: WhenExceeded::Killed,
        },
        Enforcement::Hard {
            when: WhenExceeded::Killed,
        },
    ));

    assert!(matches!(outcome, Outcome::Ok {}), "{outcome:?}");
}

/// Both fields missing are one sentence rather than two checks.
#[test]
fn two_missing_controllers_are_reported_together() {
    let outcome = super::limit_outcome(&support(
        Enforcement::Unavailable {
            why: "no cpu".to_owned(),
        },
        Enforcement::Unavailable {
            why: "no memory".to_owned(),
        },
    ));

    match outcome {
        Outcome::Note { because } => {
            assert!(
                because.contains("no cpu") && because.contains("no memory"),
                "{because}"
            );
        }
        other => panic!("{other:?}"),
    }
}

/// A support answer with the two interesting fields set and the rest held constant.
fn support(cpu: Enforcement, memory: Enforcement) -> LimitSupport {
    LimitSupport {
        mechanism: LimitMechanism::CgroupV2,
        cpu,
        memory,
        memory_measure: MemoryMeasure::ChargedPages,
        priority: true,
        cores: 8,
    }
}

/// **The failing arm of check 9, on every system.** What the real read answers is whatever the
/// machine running the suite happens to have reserved, and no test may change that — so without
/// the mock this branch would ship having never run.
#[test]
fn a_reserved_range_holding_port_80_is_found() {
    let host = mixengine_platform::mock::Host::with_reserved_ports("/mixengine", &[(60, 100)]);

    let ranges = host
        .reserved_ports()
        .reserved()
        .expect("the mock always answers");

    assert!(ranges.iter().any(|range| range.holds(80)), "{ranges:?}");
    assert!(!ranges.iter().any(|range| range.holds(443)), "{ranges:?}");
}

/// And the ordinary arm, so the assertion above is a comparison rather than a coincidence.
#[test]
fn a_machine_that_reserves_nothing_holds_nothing() {
    let host = mixengine_platform::mock::Host::with_reserved_ports("/mixengine", &[]);

    assert!(
        host.reserved_ports()
            .reserved()
            .expect("the mock always answers")
            .is_empty()
    );
}
/// The one decision in check 11, with the database and the registry taken out of it.
///
/// A row is stranded when it *claims* a supervisor — its state says so, or it names a pid — and
/// this daemon holds no runner for it. Both halves are in the table: a claimed row that is held
/// is not stranded, and an unheld row that claims nothing is not either.
#[test]
fn a_row_is_stranded_only_when_it_claims_a_supervisor_nobody_is() {
    let held: std::collections::BTreeSet<&str> = ["fakeservice@held"].into_iter().collect();

    let stranded = super::stranded(
        [
            ("fakeservice@held", true),
            ("fakeservice@lost", true),
            ("fakeservice@quiet", false),
        ]
        .into_iter(),
        &held,
    );

    assert_eq!(stranded, vec!["fakeservice@lost".to_owned()]);
}

/// And the quiet case, so the assertion above is a comparison rather than a coincidence: a
/// daemon supervising everything it has rows for strands nothing.
#[test]
fn a_daemon_supervising_what_it_has_rows_for_strands_nothing() {
    let held: std::collections::BTreeSet<&str> = ["fakeservice@main"].into_iter().collect();

    assert!(super::stranded([("fakeservice@main", true)].into_iter(), &held).is_empty());
}

/// The ordinary Windows machine, and every machine that never had the feature.
#[test]
fn a_machine_refusing_nothing_is_ok() {
    let outcome = super::app_control_outcome(Ok(AppControlState::Off));

    assert!(matches!(outcome, Outcome::Ok {}), "{outcome:?}");
}

/// **Enforcing is a `Problem` and not a `Note`**, even on a machine where everything currently
/// runs: the judgement is per file, and the next image MixEngine loads is a runtime archive whose
/// hash has never existed anywhere.
#[test]
fn an_enforcing_machine_is_a_problem_with_a_name() {
    let outcome = super::app_control_outcome(Ok(AppControlState::Enforced));

    let Outcome::Problem { id, because } = outcome else {
        panic!("a machine that will refuse the next runtime is not well: {outcome:?}");
    };

    assert_eq!(id, ProblemId::ApplicationControlEnforced);
    assert!(because.contains("Smart App Control"), "{because}");
}

/// A decision that has not been made is not a fault.
#[test]
fn a_machine_still_evaluating_is_a_note() {
    let outcome = super::app_control_outcome(Ok(AppControlState::Evaluation));

    assert!(matches!(outcome, Outcome::Note { .. }), "{outcome:?}");
}

/// **A build with no name for a state must not guess which named one it resembles**, and the
/// number goes into the report so whoever reads it can look the state up.
#[test]
fn a_state_with_no_name_is_skipped_and_keeps_its_number() {
    let outcome = super::app_control_outcome(Ok(AppControlState::Unknown { value: 7 }));

    let Outcome::Skipped { because } = outcome else {
        panic!("a state this build cannot read is not a verdict: {outcome:?}");
    };

    assert!(
        because.contains('7'),
        "the number is missing from {because}"
    );
}

/// macOS and Linux: a check that ran and says why it had nothing to examine.
#[test]
fn a_system_with_no_such_policy_is_skipped_rather_than_called_clean() {
    let outcome = super::app_control_outcome(Err(mixengine_platform::Error::UnsupportedPlatform {
        capability: "AppControl",
        reason: "no such thing here".to_owned(),
    }));

    let Outcome::Skipped { because } = outcome else {
        panic!("a system with no such policy is not a clean bill of health: {outcome:?}");
    };

    assert!(because.contains("no such thing here"), "{because}");
}

/// Found running M34: a machine whose programs skip `systemd-resolved` is a note naming why, never
/// a problem `--repair` would answer with a grant that changes nothing.
#[test]
fn a_resolver_programs_never_ask_is_a_note() {
    let state = mixengine_platform::ResolverState {
        method: mixengine_platform::ResolverMethod::SystemdLink,
        wired: Vec::new(),
        missing: Some("programs on this machine do not ask systemd-resolved: …".to_owned()),
        bypassed: Some("programs on this machine do not ask systemd-resolved: …".to_owned()),
        declared: vec!["test".to_owned()],
    };

    let Outcome::Note { because } = super::resolver_outcome(Ok(state), 3) else {
        panic!("the person's own DNS setup is news, not a fault");
    };

    assert!(
        because.starts_with("names under .test, .localhost and .internal come from the hosts file"),
        "{because}"
    );
    assert!(because.contains("do not ask systemd-resolved"), "{because}");
    assert!(
        because.contains("sudo apt install libnss-resolve"),
        "the way out, measured on the machine that found this: {because}"
    );
}

#[test]
fn a_resolver_routing_less_than_asked_is_still_a_problem() {
    let state = mixengine_platform::ResolverState {
        method: mixengine_platform::ResolverMethod::SystemdLink,
        wired: vec!["test".to_owned()],
        missing: Some("not all of them".to_owned()),
        bypassed: None,
        declared: vec!["test".to_owned()],
    };

    assert!(matches!(
        super::resolver_outcome(Ok(state), 3),
        Outcome::Problem {
            id: ProblemId::ResolverNotWired,
            ..
        }
    ));
}
