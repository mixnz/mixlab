use super::*;

/// A machine that has a store and does not hold it — the shape a fresh home answers with, and
/// the one every test below is about something other than.
fn untrusted() -> Trust {
    Trust::NotInstalled {
        because: "this machine does not hold MixEngine's authority".to_owned(),
    }
}

/// A machine MixEngine does not search for browser databases, which is what a Windows or macOS
/// answer looks like and what every test below is about something other than.
fn unsearched() -> Browsers {
    Browsers::NotSearched {
        because: "MixEngine does not search browser certificate databases here".to_owned(),
    }
}

/// The state travels as a word, and so does the reason it is unusable.
#[test]
fn a_state_travels_tagged_and_a_reason_travels_as_a_name() {
    let absent = serde_json::to_value(CaStatus {
        state: CaState::Absent {},
        trust: untrusted(),
        browsers: unsearched(),
    })
    .expect("the status encodes");

    assert_eq!(
        absent,
        serde_json::json!({
            "state": "absent",
            "trust": {
                "state": "not_installed",
                "because": "this machine does not hold MixEngine's authority",
            },
            // T49b's field, and it is nested for `trust`'s reason: this one spells its reason
            // `because` too, so a third flattened sentence would overwrite one of the first two.
            "browsers": {
                "state": "not_searched",
                "because": "MixEngine does not search browser certificate databases here",
            },
        })
    );

    let unusable = serde_json::to_value(CaStatus {
        state: CaState::Unusable {
            because: Unusable::KeyAndCertificateDisagree,
        },
        trust: Trust::NoStore {
            because: "no anchors directory here".to_owned(),
        },
        browsers: unsearched(),
    })
    .expect("the status encodes");

    assert_eq!(
        unusable,
        serde_json::json!({
            "state": "unusable",
            "because": "key_and_certificate_disagree",
            // **Nested, and this is the case that decided it.** Both halves spell their reason
            // `because`; flattened, an unusable authority on a machine with no store would lose
            // one of the two sentences and nothing would say which.
            "trust": { "state": "no_store", "because": "no anchors directory here" },
            "browsers": {
                "state": "not_searched",
                "because": "MixEngine does not search browser certificate databases here",
            },
        })
    );
}

/// There is no field a private key could travel in, and this is what keeps it so.
#[test]
fn nothing_in_a_status_can_carry_a_private_key() {
    let encoded = serde_json::to_string(&CaStatus {
        state: CaState::Present { ca: example() },
        trust: untrusted(),
        browsers: unsearched(),
    })
    .expect("the status encodes");

    assert!(
        !encoded.contains("PRIVATE"),
        "a status carried something that says PRIVATE: {encoded}"
    );

    // The field names are the whole of the guarantee. A future field called any of these is
    // what this test exists to make somebody argue for out loud.
    for forbidden in ["key_pem", "private_key", "key_der", "key_pkcs8"] {
        assert!(
            !encoded.contains(forbidden),
            "a status grew a {forbidden} field"
        );
    }
}

/// An expired authority is `Present` with a negative count, not `Unusable`.
#[test]
fn an_expired_authority_is_present_with_a_negative_count() {
    let encoded = serde_json::to_value(CaStatus {
        state: CaState::Present {
            ca: Ca {
                days_left: -7,
                ..example()
            },
        },
        trust: untrusted(),
        browsers: unsearched(),
    })
    .expect("the status encodes");

    assert_eq!(encoded["state"], "present");
    assert_eq!(encoded["ca"]["days_left"], -7);
}

/// Every state survives the round trip a client actually makes.
///
/// **The half the encoding tests do not reach, and the half `flatten` is most likely to break.**
/// A `#[serde(flatten)]` field is deserialised through a buffering map rather than directly,
/// which is where it interacts badly with tagged enums — and the daemon only ever serialises,
/// so nothing else in this workspace would notice a status that encodes and cannot be read back.
#[test]
fn every_state_comes_back_as_what_went_out() {
    for state in [
        CaState::Absent {},
        CaState::Present { ca: example() },
        CaState::Unusable {
            because: Unusable::KeyMissing,
        },
        CaState::Unusable {
            because: Unusable::CertificateUnreadable,
        },
    ] {
        for trust in [
            Trust::Installed {
                store: "this machine's Trusted Root Certification Authorities".to_owned(),
            },
            Trust::NotInstalled {
                because: "the store does not hold it".to_owned(),
            },
            Trust::NoStore {
                because: "no anchors directory on this machine".to_owned(),
            },
            Trust::Unknown {
                because: "security exited 1".to_owned(),
            },
        ] {
            let sent = CaStatus {
                state: state.clone(),
                trust,
                browsers: unsearched(),
            };
            let wire = serde_json::to_string(&sent).expect("the status encodes");
            let received: CaStatus =
                serde_json::from_str(&wire).expect("a client can read what the daemon sent");

            assert_eq!(received, sent, "the status changed on the way: {wire}");
        }
    }
}

/// **A machine with browsers and one with none are different answers**, and both survive the
/// trip. The path and the owner both travel, because what a person does about a database that
/// lacks it is open that path and restart that browser.
#[test]
fn what_the_browsers_say_travels_beside_what_the_machine_says() {
    let sent = CaStatus {
        state: CaState::Present { ca: example() },
        trust: untrusted(),
        browsers: Browsers::Reached {
            databases: vec![BrowserDatabase {
                path: "/home/someone/.pki/nssdb".to_owned(),
                owner: "Chrome and Chromium".to_owned(),
                installed: false,
                because: Some("/home/someone/.pki/nssdb does not hold this authority".to_owned()),
            }],
        },
    };

    let wire = serde_json::to_value(&sent).expect("the status encodes");

    assert_eq!(wire["browsers"]["state"], "reached");
    assert_eq!(
        wire["browsers"]["databases"][0]["owner"],
        "Chrome and Chromium"
    );

    let received: CaStatus =
        serde_json::from_value(wire).expect("a client can read what the daemon sent");
    assert_eq!(received, sent);
}

/// **The three that name no database still say why**, and each is a different sentence: no
/// tool, not this system, and a scan that failed are three things a person would do three
/// different things about — install a package, nothing, and read a log.
#[test]
fn every_browsers_state_carries_its_own_reason() {
    for browsers in [
        Browsers::Reached { databases: vec![] },
        Browsers::NoTool {
            because: "certutil is not installed — it ships in libnss3-tools".to_owned(),
        },
        Browsers::NotSearched {
            because: "MixEngine does not search browser databases on Windows".to_owned(),
        },
        Browsers::Unknown {
            because: "certutil did not answer within 30 seconds".to_owned(),
        },
    ] {
        let sent = CaStatus {
            state: CaState::Absent {},
            trust: untrusted(),
            browsers,
        };
        let wire = serde_json::to_string(&sent).expect("the status encodes");
        let received: CaStatus =
            serde_json::from_str(&wire).expect("a client can read what the daemon sent");

        assert_eq!(received, sent, "the status changed on the way: {wire}");
    }
}

/// A misspelled option is refused rather than ignored.
#[test]
fn an_unknown_option_is_refused_rather_than_ignored() {
    let refused: Result<CaStatusQuery, _> =
        serde_json::from_value(serde_json::json!({ "verbose": true }));

    assert!(refused.is_err(), "an unknown field was accepted");
}

/// The three outcomes travel as words, and only the refusal carries a reason.
#[test]
fn an_issue_report_travels_tagged_and_only_a_refusal_says_why() {
    let report = CertIssueReport {
        sites: vec![
            SiteCertOutcome {
                domain: "blog.test".to_owned(),
                outcome: IssueOutcome::Issued {},
                state: CertState::Absent {},
            },
            SiteCertOutcome {
                domain: "shop.test".to_owned(),
                outcome: IssueOutcome::Refused {
                    because: "this home has no usable certificate authority".to_owned(),
                },
                state: CertState::Absent {},
            },
        ],
    };

    let wire = serde_json::to_value(&report).expect("it encodes");

    assert_eq!(wire["sites"][0]["outcome"]["outcome"], "issued");
    assert_eq!(wire["sites"][1]["outcome"]["outcome"], "refused");
    assert!(wire["sites"][0]["outcome"].get("because").is_none());

    let back: CertIssueReport =
        serde_json::from_value(wire).expect("a client can read what the daemon sent");
    assert_eq!(back, report);
}

/// A request naming no site is the every-site form, and it is the default rather than a
/// separate method.
#[test]
fn an_issue_request_may_name_no_site_at_all() {
    let every: CertIssue = serde_json::from_value(serde_json::json!({}))
        .expect("an empty object is the every-site form");

    assert_eq!(every.site, None);

    let one: CertIssue =
        serde_json::from_value(serde_json::json!({ "site": { "domain": "blog.test" } }))
            .expect("a named site");

    assert_eq!(one.site, Some(SiteRef::Domain("blog.test".to_owned())));
}

/// **A site's certificate carries no key either**, which is the same guarantee `CaStatus` makes
/// and the reason `SiteCert` has no `certificate_pem` to hide one behind.
#[test]
fn nothing_in_a_site_certificate_can_carry_a_private_key() {
    let encoded = serde_json::to_string(&CertState::Present {
        cert: SiteCert {
            subject: "CN=blog.test".to_owned(),
            sans: vec!["blog.test".to_owned()],
            issuer: "CN=MixEngine Local CA 0123abcd".to_owned(),
            fingerprint: "ab".repeat(32),
            not_before: Timestamp(0),
            not_after: Timestamp(1),
            days_left: 90,
        },
    })
    .expect("the state encodes");

    assert!(!encoded.contains("PRIVATE"), "{encoded}");
    for forbidden in ["key_pem", "private_key", "key_der", "key_pkcs8"] {
        assert!(
            !encoded.contains(forbidden),
            "a site certificate grew a {forbidden} field"
        );
    }
}

/// A misspelled option is refused rather than silently defaulted — the rule T40 established for
/// every parameterless method since, and it matters most on the two that take something away.
#[test]
fn neither_destructive_method_takes_an_option() {
    serde_json::from_str::<CaRotateQuery>("{}").expect("no options is the only shape");
    serde_json::from_str::<CaUninstallQuery>("{}").expect("no options is the only shape");
    serde_json::from_str::<CaRotateQuery>(r#"{"force":true}"#)
        .expect_err("a misspelled option is told, not defaulted");
    serde_json::from_str::<CaUninstallQuery>(r#"{"all":true}"#)
        .expect_err("a misspelled option is told, not defaulted");
}

/// A client matches on a word rather than working out which fields arrived.
#[test]
fn each_outcome_is_tagged_by_a_word() {
    let rotated = serde_json::to_value(RotateOutcome::Rotated {}).expect("it encodes");
    assert_eq!(rotated["outcome"], "rotated");

    let refused = serde_json::to_value(RotateOutcome::NotCommitted {
        because: "the store does not hold the new authority".to_owned(),
    })
    .expect("it encodes");
    assert_eq!(refused["outcome"], "not_committed");
    assert_eq!(
        refused["because"],
        "the store does not hold the new authority"
    );

    assert_eq!(
        serde_json::to_value(UninstallOutcome::Removed {}).expect("it encodes")["outcome"],
        "removed"
    );
    assert_eq!(
        serde_json::to_value(UninstallOutcome::PartlyRemoved {
            because: "the store still holds it".to_owned(),
        })
        .expect("it encodes")["outcome"],
        "partly_removed"
    );
}

/// **The security-model rule, enforced by there being nowhere to put one.** A rotation report
/// carries the authority that was replaced, and [`Ca`] has only ever had the public half.
#[test]
fn a_rotation_report_has_no_field_a_private_key_could_travel_in() {
    let report = CaRotateReport {
        outcome: RotateOutcome::Rotated {},
        previous: Some(example()),
        status: CaStatus {
            state: CaState::Present { ca: example() },
            trust: Trust::Installed {
                store: "the System keychain".to_owned(),
            },
            browsers: Browsers::Unknown {
                because: "nothing was asked".to_owned(),
            },
        },
        sites: Vec::new(),
    };

    let encoded = serde_json::to_string(&report).expect("it encodes");

    assert!(
        !encoded.contains("PRIVATE KEY"),
        "no private key travels: {encoded}"
    );
    for forbidden in ["key_pem", "private_key", "key_der", "key_pkcs8"] {
        assert!(
            !encoded.contains(forbidden),
            "a rotation report grew a {forbidden} field"
        );
    }
}

/// A rotation that did not commit reports the authority this home still has, and no sites.
#[test]
fn a_rotation_that_did_not_commit_says_why_and_names_nothing_reissued() {
    let report = CaRotateReport {
        outcome: RotateOutcome::NotCommitted {
            because: "this machine trusted the old authority and does not trust the new one"
                .to_owned(),
        },
        previous: Some(example()),
        status: CaStatus {
            state: CaState::Present { ca: example() },
            trust: Trust::Installed {
                store: "the System keychain".to_owned(),
            },
            browsers: Browsers::Unknown {
                because: "nothing was asked".to_owned(),
            },
        },
        sites: Vec::new(),
    };

    let value = serde_json::to_value(&report).expect("it encodes");

    assert_eq!(
        value["outcome"], "not_committed",
        "the tag is at the top level rather than nested in a field of its own name: {value}"
    );
    assert_eq!(
        value["status"]["ca"]["key_id"], "0123abcd",
        "the authority reported is the one this home still has: {value}"
    );
    assert!(
        value["sites"].as_array().expect("a list").is_empty(),
        "nothing was reissued: {value}"
    );
}

/// **The half the encoding tests do not reach, and the half `flatten` is most likely to break.**
///
/// `mix` reads these back out of a job's outcome, so unlike every other type in this file they
/// are deserialised in the product and not only in a test. A flattened field is deserialised
/// through a buffering map rather than directly, which is exactly where it interacts badly with
/// tagged enums — the reasoning `every_state_comes_back_as_what_went_out` already applies to
/// [`CaStatus`].
#[test]
fn both_reports_come_back_as_what_went_out() {
    let status = CaStatus {
        state: CaState::Present { ca: example() },
        trust: Trust::Installed {
            store: "the System keychain".to_owned(),
        },
        browsers: Browsers::NoTool {
            because: "certutil is not installed".to_owned(),
        },
    };

    for outcome in [
        RotateOutcome::Rotated {},
        RotateOutcome::NotCommitted {
            because: "the store does not hold the new authority".to_owned(),
        },
        RotateOutcome::NothingToRotate {
            because: "this home has no certificate authority".to_owned(),
        },
    ] {
        let report = CaRotateReport {
            outcome,
            previous: Some(example()),
            status: status.clone(),
            sites: Vec::new(),
        };

        let encoded = serde_json::to_string(&report).expect("it encodes");
        let back: CaRotateReport =
            serde_json::from_str(&encoded).expect("a rotation report reads back");

        assert_eq!(back, report, "{encoded}");
    }

    for outcome in [
        UninstallOutcome::Removed {},
        UninstallOutcome::PartlyRemoved {
            because: "the store still holds it".to_owned(),
        },
        UninstallOutcome::NothingToRemove {
            because: "this home has no certificate authority".to_owned(),
        },
    ] {
        let report = CaUninstallReport {
            outcome,
            status: status.clone(),
        };

        let encoded = serde_json::to_string(&report).expect("it encodes");
        let back: CaUninstallReport =
            serde_json::from_str(&encoded).expect("a removal report reads back");

        assert_eq!(back, report, "{encoded}");
    }
}

fn example() -> Ca {
    Ca {
        subject: "CN=MixEngine Local CA 0123abcd".to_owned(),
        fingerprint: "ab".repeat(32),
        key_id: "0123abcd".to_owned(),
        not_before: Timestamp(0),
        not_after: Timestamp(1),
        days_left: 3_650,
        certificate_pem: "-----BEGIN CERTIFICATE-----\n".to_owned(),
    }
}
