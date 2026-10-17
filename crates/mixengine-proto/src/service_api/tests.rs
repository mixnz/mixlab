use super::*;

fn service(id: &str) -> ServiceId {
    ServiceId::parse(id).expect("a valid service id")
}

/// T202a, D3. A failure written by a daemon from before the detail still reads, and the detail
/// the runner persisted travels with the reason.
#[test]
fn a_service_failure_reads_without_a_detail_and_carries_one() {
    let bare: ServiceFailure =
        serde_json::from_str(r#"{"service":"mariadb@main"}"#).expect("reads");
    assert_eq!(bare.detail, None);
    assert_eq!(bare.reason, None);

    let told = ServiceFailure {
        service: service("mariadb@main"),
        reason: Some(StateReason::SpawnFailed),
        detail: Some("the environment entry MYSQL_PWD: no credential is stored at x".to_owned()),
    };
    let json = serde_json::to_value(&told).expect("writes");
    assert_eq!(
        json["detail"],
        "the environment entry MYSQL_PWD: no credential is stored at x"
    );
    assert_eq!(
        serde_json::to_value(ServiceFailure {
            detail: None,
            ..told
        })
        .expect("writes")["detail"],
        serde_json::Value::Null
    );
}

/// **T182g.** A found instance that can be adopted says with what, and leaves out the two
/// fields that would only be there when it cannot.
#[test]
fn a_found_service_that_opens_carries_only_what_opens_it() {
    let found = ServiceFound {
        service: service("mariadb@main"),
        path: "/data/mariadb/main".to_owned(),
        made_by: None,
        opens_with: Some(PackageVersion::parse("11.4.5").expect("a version")),
        why_not: None,
    };

    let json = serde_json::to_value(&found).expect("encodes");
    assert_eq!(json["service"], "mariadb@main");
    assert_eq!(json["opens_with"], "11.4.5");
    assert!(
        json.get("made_by").is_none() && json.get("why_not").is_none(),
        "{json}"
    );

    let back: ServiceFound = serde_json::from_value(json).expect("decodes");
    assert_eq!(back, found);
}

/// Both answers travel tagged, and the front-end one carries which program it is — which is
/// what a client sends back to switch.
#[test]
fn a_role_says_what_a_service_is_for_and_names_the_program_when_it_matters() {
    let front_end = ServiceRole::FrontEnd {
        server: FrontEndServer::Nginx,
    };

    let encoded = serde_json::to_value(front_end).expect("a role encodes");
    assert_eq!(encoded["role"], "front_end");
    assert_eq!(encoded["server"], "nginx");
    assert_eq!(
        serde_json::from_value::<ServiceRole>(encoded).expect("and decodes"),
        front_end
    );

    let other = serde_json::to_value(ServiceRole::Other {}).expect("a role encodes");
    assert_eq!(other["role"], "other");
    assert_eq!(
        serde_json::from_value::<ServiceRole>(other).expect("and decodes"),
        ServiceRole::Other {}
    );
}

/// **The floor of protocol 1**, as the JSON a daemon from before **T97** actually sent — and
/// the guard on [ADR 0019] that keeps the next added member optional too.
///
/// Every member below is one this type was frozen with. [`ServiceSummary::role`] is absent,
/// which is what such a daemon puts on the wire, and this build reads it as [`None`] rather
/// than refusing the whole answer — which is the state a self-updating product spends every
/// upgrade in, its binaries replaced and its daemon not yet restarted.
///
/// [ADR 0019]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0019-an-added-response-member-is-optional.md
#[test]
fn a_summary_from_before_a_role_existed_still_reads() {
    let floor = r#"{
            "id": "mariadb@main",
            "state": "stopped",
            "supervised": false,
            "pid": null,
            "last_started_at": null,
            "last_exit_code": null,
            "depends_on": []
        }"#;

    let decoded: ServiceSummary = serde_json::from_str(floor).expect("a summary");

    assert_eq!(decoded.id.as_str(), "mariadb@main");
    assert_eq!(
        decoded.role, None,
        "an absent role is a daemon that predates the member, never a role nobody established"
    );
    assert_eq!(
        decoded.version, None,
        "an absent version is a daemon that predates T183"
    );
}

/// **T183.** A version goes out as the plain string it was installed as, and comes back equal.
#[test]
fn a_summary_carries_the_version_it_runs_and_omits_one_it_does_not_know() {
    let known = ServiceSummary {
        version: Some(PackageVersion::parse("5.7.44").expect("a version")),
        ..summary("mysql@main")
    };
    let encoded = serde_json::to_value(&known).unwrap();
    assert_eq!(encoded["version"], "5.7.44");
    assert_eq!(
        serde_json::from_value::<ServiceSummary>(encoded).unwrap(),
        known
    );

    let unknown = serde_json::to_value(summary("mysql@main")).unwrap();
    assert!(unknown.get("version").is_none(), "{unknown}");
}

/// **D4.** The shape every client has sent since T31a still parses, and gains one optional key.
#[test]
fn a_service_delete_from_before_force_existed_still_parses() {
    let asked: ServiceDelete =
        serde_json::from_value(serde_json::json!({"service": "mariadb@main"}))
            .expect("the old shape");

    assert!(!asked.force);
    assert_eq!(asked.target.service.as_str(), "mariadb@main");
}

/// One service, with nothing interesting about it.
fn summary(id: &str) -> ServiceSummary {
    ServiceSummary {
        id: service(id),
        state: Some(ServiceState::Stopped),
        supervised: false,
        pid: None,
        port: None,
        last_started_at: None,
        last_exit_code: None,
        depends_on: Vec::new(),
        role: Some(ServiceRole::Other {}),
        autostart: false,
        stopped_by: None,
        version: None,
        last_failure: None,
    }
}

/// A create names the service and the version, and derives the package from the id — which is
/// what [`ServiceId::name`] has always said it is.
#[test]
fn a_create_takes_an_id_and_a_version_and_nothing_redundant() {
    let create: ServiceCreate = serde_json::from_str(r#"{"id":"mariadb@main","version":"11.4.2"}"#)
        .expect("the two required fields");

    assert_eq!(create.id.name(), "mariadb");
    assert_eq!(create.id.instance(), Some("main"));
    assert_eq!(create.version.as_str(), "11.4.2");
    assert_eq!(
        create.port, None,
        "a port nobody named is the recipe's own default"
    );
}

/// A delete says what it kept, because what it kept is somebody's databases.
#[test]
fn a_removal_names_the_data_it_did_not_touch() {
    let removal = ServiceRemoval {
        removed: summary("mariadb@main"),
        data_kept: Some("/home/me/.local/share/mixengine/data/mariadb/main".to_owned()),
    };

    let encoded = serde_json::to_value(&removal).expect("a removal encodes");

    assert_eq!(
        encoded["data_kept"],
        "/home/me/.local/share/mixengine/data/mariadb/main"
    );
    assert_eq!(
        serde_json::from_value::<ServiceRemoval>(encoded).expect("and decodes"),
        removal
    );
}

/// The shape a client that types nothing gets: everything, and wait for it.
#[test]
fn a_target_with_no_parameters_means_every_service_and_waiting() {
    let target: ServiceTarget = serde_json::from_str("{}").expect("every field has a default");

    assert_eq!(target, ServiceTarget::default());
    assert_eq!(target.service, None);
    assert!(target.wait, "a client that said nothing is waited for");
}

#[test]
fn not_waiting_is_the_one_thing_a_target_has_to_spell_out() {
    let target: ServiceTarget =
        serde_json::from_str(r#"{"service":"mariadb@main","wait":false}"#).expect("both fields");

    assert_eq!(target.service, Some(service("mariadb@main")));
    assert!(!target.wait);

    // The default is not written back out: `{"service":"…"}` and `{"service":"…","wait":true}`
    // are the same request, and only one of them is worth putting on the wire.
    assert_eq!(
        serde_json::to_string(&ServiceTarget {
            service: Some(service("mariadb@main")),
            project: None,
            wait: true,
        })
        .unwrap(),
        r#"{"service":"mariadb@main"}"#
    );
}

/// **T97.** The only thing a Settings screen has to send is which program it wants.
#[test]
fn a_switch_needs_only_the_server_and_raises_nothing_by_itself() {
    let switch: FrontEndSwitch =
        serde_json::from_str(r#"{"server":"nginx"}"#).expect("the one required field");

    assert_eq!(switch.server, FrontEndServer::Nginx);
    assert_eq!(
        switch.version, None,
        "a version nobody named is the newest installed"
    );
    assert!(
        !switch.grant,
        "what is about to be allowed is read before it is allowed"
    );
}

/// A misspelled member must not be read as *do not raise a prompt*, which is what a switch with
/// an unknown key and a permissive parser would silently be.
#[test]
fn a_switch_refuses_a_member_it_does_not_know() {
    serde_json::from_str::<FrontEndSwitch>(r#"{"server":"caddy","grants":true}"#)
        .expect_err("deny_unknown_fields");
}

/// Each ending travels tagged, and the two that carry nothing are still objects — the shape a
/// client switches on.
#[test]
fn every_ending_a_switch_can_have_round_trips_tagged() {
    for (outcome, tag) in [
        (FrontEndOutcome::Unchanged {}, "unchanged"),
        (FrontEndOutcome::Switched { started: None }, "switched"),
        (
            FrontEndOutcome::NotGranted {
                because: "nobody answered the prompt".to_owned(),
            },
            "not_granted",
        ),
        (
            FrontEndOutcome::RolledBack {
                because: "nginx -t refused the rendering".to_owned(),
            },
            "rolled_back",
        ),
        (
            FrontEndOutcome::Failed {
                because: "the old row could not be written back".to_owned(),
            },
            "failed",
        ),
    ] {
        let encoded = serde_json::to_value(&outcome).expect("an outcome encodes");
        assert_eq!(encoded["outcome"], tag);
        assert_eq!(
            serde_json::from_value::<FrontEndOutcome>(encoded).expect("and decodes"),
            outcome
        );
    }
}

/// The exit code, and the one ending that is deliberately not a failure: asking for the server a
/// home is already on is a question with an answer.
#[test]
fn only_a_switch_that_did_not_happen_is_something_the_caller_wanted_more_of() {
    let report = |outcome| FrontEndReport {
        was: Some(service("caddy")),
        now: Some(service("caddy")),
        outcome,
        answering: true,
        kept_data: None,
        not_carried: Vec::new(),
    };

    assert!(!report(FrontEndOutcome::Unchanged {}).wanted_more());
    assert!(!report(FrontEndOutcome::Switched { started: None }).wanted_more());

    for refused in [
        FrontEndOutcome::NotGranted {
            because: "no helper".to_owned(),
        },
        FrontEndOutcome::RolledBack {
            because: "it would not render".to_owned(),
        },
        FrontEndOutcome::Failed {
            because: "and would not go back".to_owned(),
        },
    ] {
        assert!(report(refused).wanted_more());
    }

    assert!(
        report(FrontEndOutcome::Switched { started: None }).moved(),
        "and only a switch moved the home"
    );
    assert!(!report(FrontEndOutcome::Unchanged {}).moved());
}

/// What a switch left behind is on the wire even when there is nothing to say, because an empty
/// list and an absent one are different answers — and a client renders the first as *nothing
/// was lost*.
#[test]
fn a_report_that_left_nothing_behind_still_says_so() {
    let report = FrontEndReport {
        was: None,
        now: Some(service("nginx")),
        outcome: FrontEndOutcome::Switched { started: None },
        answering: false,
        kept_data: None,
        not_carried: Vec::new(),
    };

    let encoded = serde_json::to_value(&report).expect("a report encodes");
    assert!(encoded.get("was").is_none(), "{encoded}");
    assert!(encoded.get("kept_data").is_none(), "{encoded}");
    assert_eq!(encoded["not_carried"], serde_json::json!([]));
    assert_eq!(
        serde_json::from_value::<FrontEndReport>(encoded).expect("and decodes"),
        report
    );
}

#[test]
fn a_status_without_a_service_does_not_decode() {
    serde_json::from_str::<ServiceQuery>("{}")
        .expect_err("a status with no subject is a mistyped list, not a list");
}

/// T167: who stopped a service travels as one word, and a daemon older than the member — or a
/// service that is not stopped — sends nothing.
#[test]
fn stopped_by_is_a_word_and_is_absent_when_not_said() {
    assert_eq!(serde_json::to_value(StoppedBy::Daemon).unwrap(), "daemon");
    assert_eq!(serde_json::to_value(StoppedBy::Person).unwrap(), "person");
    assert_eq!(serde_json::to_value(StoppedBy::Never).unwrap(), "never");

    let older = serde_json::json!({
        "id": "caddy", "state": "stopped", "supervised": false, "depends_on": []
    });
    let summary: ServiceSummary = serde_json::from_value(older).expect("an older daemon's row");
    assert_eq!(summary.stopped_by, None);
    assert!(
        serde_json::to_value(&summary)
            .unwrap()
            .get("stopped_by")
            .is_none(),
        "an absent reason is not written as null"
    );
}

#[test]
fn save_resources_is_one_boolean_each_way() {
    let asked: SaveResourcesSet = serde_json::from_str(r#"{"on":true}"#).unwrap();
    assert!(asked.on);
    assert_eq!(
        serde_json::to_value(SaveResources { on: false }).unwrap(),
        serde_json::json!({ "on": false })
    );
}

#[test]
fn a_summary_of_a_service_with_no_row_omits_the_state_rather_than_inventing_one() {
    let summary = ServiceSummary {
        id: service("mailpit"),
        state: None,
        supervised: false,
        pid: None,
        port: None,
        last_started_at: None,
        last_exit_code: None,
        depends_on: Vec::new(),
        role: None,
        autostart: false,
        stopped_by: None,
        version: None,
        last_failure: None,
    };

    let encoded = serde_json::to_value(&summary).unwrap();
    assert!(encoded.get("state").is_none(), "{encoded}");
    assert!(encoded.get("role").is_none(), "{encoded}");
    assert!(encoded.get("version").is_none(), "{encoded}");
    assert!(encoded.get("last_failure").is_none(), "{encoded}");
    assert_eq!(encoded["supervised"], false);
    assert_eq!(
        serde_json::from_value::<ServiceSummary>(encoded).unwrap(),
        summary
    );
}

#[test]
fn a_walk_that_stopped_carries_the_reason_the_transition_did() {
    let walk = ServiceWalk {
        planned: vec![service("mariadb@main"), service("php-fpm@8.3")],
        complete: true,
        reached: Vec::new(),
        failed: Some(ServiceFailure {
            service: service("mariadb@main"),
            detail: None,
            reason: Some(StateReason::ReadyTimeout {
                after: crate::Millis::from_secs(10),
            }),
        }),
        blocked: vec![service("php-fpm@8.3")],
    };

    let encoded = serde_json::to_value(&walk).unwrap();
    assert_eq!(encoded["failed"]["service"], "mariadb@main");
    assert_eq!(encoded["failed"]["reason"]["kind"], "ready_timeout");
    assert_eq!(
        serde_json::from_value::<ServiceWalk>(encoded).unwrap(),
        walk
    );
}

#[test]
fn a_walk_that_was_not_waited_for_says_so_rather_than_looking_like_one_that_did_nothing() {
    let accepted = ServiceWalk {
        planned: vec![service("mariadb@main")],
        complete: false,
        reached: Vec::new(),
        failed: None,
        blocked: Vec::new(),
    };

    let encoded = serde_json::to_value(&accepted).unwrap();
    assert_eq!(encoded["complete"], false);
    assert!(
        encoded.get("failed").is_none(),
        "nothing failed — nothing has happened yet: {encoded}"
    );
}
