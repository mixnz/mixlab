use super::*;

use crate::PROTOCOL_VERSION;

/// T182b, D1. The helper's version is its own, and every helper stamped before it (the product
/// versions, and the stray `0.1.0` from before the renumbering) compares older.
#[test]
fn the_helper_version_is_ahead_of_every_stamp_before_it() {
    let own = crate::PackageVersion::parse(HELPER_VERSION.to_owned()).expect("a version");

    for old in ["0.0.1", "0.0.7", "0.1.0"] {
        let old = crate::PackageVersion::parse(old.to_owned()).expect("a version");
        assert_eq!(
            old.cmp_precedence(&own),
            std::cmp::Ordering::Less,
            "{old:?}"
        );
    }
}

/// The daemon writes this; the helper reads it. A change to either side that the other did not
/// make shows up here first.
#[test]
fn a_request_round_trips() {
    let request = PrivilegedRequest {
        version: PROTOCOL_VERSION,
        home: PathBuf::from("/home/someone/.mixengine"),
        nonce: "b8f0…".to_owned(),
        ops: vec![serde_json::json!({ "op": "probe" })],
    };

    let encoded = serde_json::to_string(&request).unwrap();
    assert_eq!(
        serde_json::from_str::<PrivilegedRequest>(&encoded).unwrap(),
        request
    );
}

/// D3, the reading half: an operation this build has never heard of survives as an undecoded
/// value and does not take its neighbours down with it.
#[test]
fn an_unknown_operation_does_not_fail_the_envelope() {
    let text = r#"{
            "version": 1,
            "home": "/home/someone/.mixengine",
            "nonce": "n",
            "ops": [{ "op": "probe" }, { "op": "trust-ca-install", "der": [1, 2, 3] }]
        }"#;

    let request: PrivilegedRequest = serde_json::from_str(text).unwrap();

    assert_eq!(request.ops.len(), 2);
    assert!(serde_json::from_value::<PrivilegedOp>(request.ops[0].clone()).is_ok());
    assert!(serde_json::from_value::<PrivilegedOp>(request.ops[1].clone()).is_err());
}

/// D3, the intolerant half: a field inside an operation this build *does* know is fatal for that
/// operation. Silently ignoring it is how a weaker version of an operation gets applied.
#[test]
fn an_unknown_field_inside_a_known_operation_is_fatal() {
    let value = serde_json::json!({ "op": "probe", "and-also": "something new" });

    assert!(serde_json::from_value::<PrivilegedOp>(value).is_err());
}

/// A field the envelope does not know is a daemon speaking a protocol this build does not have.
#[test]
fn an_unknown_field_in_the_envelope_is_fatal() {
    let text = r#"{
            "version": 1, "home": "/h", "nonce": "n", "ops": [], "deadline": 5
        }"#;

    assert!(serde_json::from_str::<PrivilegedRequest>(text).is_err());
}

/// D5: the one operation that changes nothing is the one that does not need a token.
#[test]
fn probe_is_the_operation_that_needs_no_privilege() {
    assert!(!PrivilegedOp::Probe {}.requires_elevation());
}

/// `name()` is what goes in the audit log and in `supported_ops`; the tag is what goes on the
/// wire. Two spellings of one operation would make the log unreadable against the protocol.
#[test]
fn the_name_of_an_operation_is_its_wire_tag() {
    let encoded = serde_json::to_value(PrivilegedOp::Probe {}).unwrap();

    assert_eq!(encoded["op"], PrivilegedOp::Probe {}.name());
    assert!(PrivilegedOp::ALL.contains(&PrivilegedOp::Probe {}.name()));
    assert_eq!(PrivilegedOp::ALL.len(), 14, "ALL and the enum have drifted");
}

/// T182a, D2: the one operation that removes the program itself carries nothing, like the two
/// removals before it, needs a token, and has a key of its own.
#[test]
fn package_remove_carries_nothing_needs_a_token_and_has_its_own_key() {
    let op = PrivilegedOp::PackageRemove {};

    assert_eq!(op.name(), "package-remove");
    assert!(PrivilegedOp::ALL.contains(&op.name()));
    assert!(op.requires_elevation());
    assert_eq!(op.dedupe_key(), "package");
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        serde_json::json!({ "op": "package-remove" })
    );
    assert!(
        !op.describe().contains('/'),
        "no path in the sentence: {}",
        op.describe()
    );
}

/// The operation carries nothing, so its dedupe key is its name and two enqueues are one row
/// — the T85 design, D2. Asserted because this is the first operation since `Probe` with no
/// data in it at all, and `Probe`'s key is its *serialisation* rather than its name.
#[test]
fn installing_the_helper_is_one_pending_operation_however_often_it_is_asked_for() {
    let op = PrivilegedOp::HelperInstall {};

    assert_eq!(op.dedupe_key(), "helper-install");
    assert_eq!(op.dedupe_key(), PrivilegedOp::HelperInstall {}.dedupe_key());
    assert!(op.requires_elevation());
    assert!(PrivilegedOp::ALL.contains(&op.name()));
    assert_eq!(
        serde_json::to_value(&op).unwrap()["op"],
        "helper-install",
        "the wire tag and the name have drifted"
    );
}

/// D2, at the wire: there is no field a caller could aim this operation with, and a request
/// carrying one is refused rather than quietly accepted with the extra ignored.
#[test]
fn a_helper_install_carrying_a_path_is_refused_at_the_wire() {
    let value = serde_json::json!({ "op": "helper-install", "source": "/tmp/anything" });

    serde_json::from_value::<PrivilegedOp>(value)
        .expect_err("this operation has no field a caller could aim it with");
}

/// The response is read by a daemon that may be older than the helper that wrote it, so an
/// added field must not make it unreadable. The opposite rule to the request, deliberately.
#[test]
fn a_response_tolerates_a_field_the_reader_does_not_know() {
    let text = r#"{
            "version": 1, "elevate-version": "0.1.0", "nonce": "n", "elevated": true,
            "supported-ops": ["probe"], "audit-log": "/var/log/mixengine/elevate.log",
            "results": [{ "outcome": "applied", "detail": "…" }],
            "duration-ms": 4
        }"#;

    let response: PrivilegedResponse = serde_json::from_str(text).unwrap();

    assert_eq!(response.results.len(), 1);
    assert!(response.elevated);
}

#[test]
fn every_outcome_round_trips() {
    let outcomes = vec![
        OpOutcome::Applied {
            detail: "d".to_owned(),
        },
        OpOutcome::AlreadyDone,
        OpOutcome::Refused {
            reason: "r".to_owned(),
        },
        OpOutcome::Unsupported {
            reason: "u".to_owned(),
        },
        OpOutcome::Failed {
            message: "m".to_owned(),
        },
    ];

    let encoded = serde_json::to_string(&outcomes).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<OpOutcome>>(&encoded).unwrap(),
        outcomes
    );
}

/// T40a's vocabulary, defined here so that task has a word to use — D11.
#[test]
fn a_declined_prompt_is_a_word_rather_than_a_number() {
    let encoded = serde_json::to_string(&ElevationOutcome::Declined).unwrap();

    assert_eq!(encoded, r#"{"outcome":"declined"}"#);
}

/// D7: a description is derived from the operation every time it is rendered, so it cannot
/// disagree with what will actually be applied. What is asserted here is that it says something
/// a person could act on — the wire tag repeated back is not that.
#[test]
fn every_operation_says_what_it_will_change() {
    for op in [
        PrivilegedOp::Probe {},
        PrivilegedOp::hosts_apply(None, [entry("127.0.0.1", "blog.test")]),
        PrivilegedOp::hosts_apply(None, []),
    ] {
        let described = op.describe();

        assert!(!described.is_empty());
        assert_ne!(described, op.name(), "a tag is not a description");
        assert!(
            described.chars().next().is_some_and(char::is_lowercase),
            "descriptions are rendered in a list and start mid-sentence: {described}"
        );
    }
}

/// D1: the operation carries the whole managed block, so two orderings of one change are one
/// operation and not two rows on the screen that asks a person to allow them.
#[test]
fn a_hosts_change_is_a_set_and_not_a_sequence() {
    let one = PrivilegedOp::hosts_apply(
        None,
        [
            entry("127.0.0.1", "api.blog.test"),
            entry("127.0.0.1", "blog.test"),
        ],
    );
    let other = PrivilegedOp::hosts_apply(
        None,
        [
            entry("127.0.0.1", "blog.test"),
            entry("127.0.0.1", "api.blog.test"),
            entry("127.0.0.1", "blog.test"),
        ],
    );

    assert_eq!(
        one, other,
        "order and repetition are not part of the request"
    );
}

/// D2: a whole-state operation deduplicates on its *kind*, so a newer state supersedes an older
/// one rather than queueing beside it. `Probe`'s key is unchanged, which is what makes the
/// column need no migration.
#[test]
fn a_whole_state_operation_deduplicates_on_its_kind() {
    let one = PrivilegedOp::hosts_apply(Some("aa".to_owned()), [entry("127.0.0.1", "blog.test")]);
    let other = PrivilegedOp::hosts_apply(Some("aa".to_owned()), [entry("127.0.0.1", "shop.test")]);

    assert_eq!(one.dedupe_key(), other.dedupe_key());
    assert_eq!(one.dedupe_key(), "hosts-apply:aa");
    assert_ne!(
        one, other,
        "the same key, and deliberately not the same operation"
    );

    assert_eq!(
        PrivilegedOp::Probe {}.dedupe_key(),
        serde_json::to_string(&PrivilegedOp::Probe {}).unwrap(),
        "Probe's key is still its serialisation, so no row in an existing home moves"
    );
}

/// It needs a token, unlike `Probe`, and the helper's one gate is what reads this.
#[test]
fn writing_the_hosts_file_needs_an_administrative_token() {
    assert!(
        PrivilegedOp::hosts_apply(None, [entry("127.0.0.1", "blog.test")]).requires_elevation()
    );
    assert_eq!(
        PrivilegedOp::hosts_apply(None, []).name(),
        "hosts-apply",
        "the tag is the audit log's word for it"
    );
}

/// The screen T64 renders exists to be read before somebody clicks Allow, so the description is
/// the domains themselves and not a count.
#[test]
fn a_hosts_change_describes_itself_by_naming_every_domain() {
    let described = PrivilegedOp::hosts_apply(
        None,
        [
            entry("127.0.0.1", "blog.test"),
            entry("127.0.0.1", "api.blog.test"),
        ],
    )
    .describe();

    assert!(described.contains("blog.test"), "{described}");
    assert!(described.contains("api.blog.test"), "{described}");
    assert!(described.contains("127.0.0.1"), "{described}");

    assert_eq!(
        PrivilegedOp::hosts_apply(Some("aa".to_owned()), []).describe(),
        "remove MixEngine's block from the hosts file"
    );
    assert!(
        PrivilegedOp::hosts_apply(None, [])
            .describe()
            .contains("before each home kept its own")
    );
}

/// The request is intolerant, and an operation that carries data is where that matters.
#[test]
fn a_hosts_entry_with_a_field_this_build_does_not_know_is_fatal() {
    let value = serde_json::json!({
        "op": "hosts-apply",
        "entries": [{ "address": "127.0.0.1", "domain": "blog.test", "comment": "hi" }]
    });

    assert!(serde_json::from_value::<PrivilegedOp>(value).is_err());
}

#[test]
fn a_hosts_change_round_trips() {
    let op = PrivilegedOp::hosts_apply(None, [entry("::1", "blog.test")]);

    let encoded = serde_json::to_string(&op).unwrap();
    assert_eq!(serde_json::from_str::<PrivilegedOp>(&encoded).unwrap(), op);
    assert!(encoded.contains(r#""op":"hosts-apply""#), "{encoded}");
}

/// A `HostEntry` for a test, from the two strings a reader recognises.
fn entry(address: &str, domain: &str) -> HostEntry {
    HostEntry {
        address: address.parse().expect("a literal address"),
        domain: domain.to_owned(),
    }
}

/// D12: they are two values of one question — *what port access should this machine have?* — so
/// the guarded upsert supersedes rather than queues, and execution order never has to be
/// reasoned about.
#[test]
fn granting_and_revoking_port_access_are_one_row() {
    let grant = PrivilegedOp::PortAccessGrant {
        plan: PortAccessPlan::Capability {
            binary: PathBuf::from("/home/someone/.mixengine/packages/caddy/caddy"),
            ports: vec![80, 443],
        },
    };
    let revoke = PrivilegedOp::PortAccessRevoke {
        target: PortAccessTarget::Redirect {},
    };

    assert_eq!(grant.dedupe_key(), "port-access");
    assert_eq!(revoke.dedupe_key(), "port-access");
    assert_ne!(
        grant, revoke,
        "the same key, and deliberately not the same operation"
    );
}

/// Both write outside the home, so both need the token — and the helper's one gate reads this.
#[test]
fn port_access_needs_an_administrative_token_in_both_directions() {
    assert!(
        PrivilegedOp::PortAccessGrant {
            plan: PortAccessPlan::Redirect {
                redirects: vec![PortRedirect {
                    answer: 80,
                    bind: 8080
                }],
            },
        }
        .requires_elevation()
    );
    assert!(
        PrivilegedOp::PortAccessRevoke {
            target: PortAccessTarget::Capability {
                binary: PathBuf::from("/x/caddy")
            },
        }
        .requires_elevation()
    );
}

/// The screen T64 renders is read before somebody clicks Allow. D11 leaves exactly one control
/// against a compromised daemon pointing a grant at a binary of its own choosing, and it is that
/// the whole path is printed here.
#[test]
fn a_port_access_change_describes_what_it_will_do_to_the_machine() {
    let capability = PrivilegedOp::PortAccessGrant {
        plan: PortAccessPlan::Capability {
            binary: PathBuf::from("/home/someone/.mixengine/packages/caddy/caddy"),
            ports: vec![80, 443],
        },
    }
    .describe();

    assert!(capability.contains("/home/someone"), "{capability}");
    assert!(capability.contains("80"), "{capability}");
    assert!(capability.contains("443"), "{capability}");

    let redirect = PrivilegedOp::PortAccessGrant {
        plan: PortAccessPlan::Redirect {
            redirects: vec![PortRedirect {
                answer: 80,
                bind: 8080,
            }],
        },
    }
    .describe();

    assert!(redirect.contains("8080"), "{redirect}");

    let taken = PrivilegedOp::PortAccessRevoke {
        target: PortAccessTarget::Capability {
            binary: PathBuf::from("/x/caddy"),
        },
    }
    .describe();

    assert!(taken.contains("/x/caddy"), "{taken}");
}

#[test]
fn both_port_access_operations_round_trip() {
    for op in [
        PrivilegedOp::PortAccessGrant {
            plan: PortAccessPlan::Capability {
                binary: PathBuf::from("/x/caddy"),
                ports: vec![80],
            },
        },
        PrivilegedOp::PortAccessRevoke {
            target: PortAccessTarget::Redirect {},
        },
    ] {
        let encoded = serde_json::to_string(&op).unwrap();

        assert_eq!(serde_json::from_str::<PrivilegedOp>(&encoded).unwrap(), op);
        assert!(PrivilegedOp::ALL.contains(&op.name()), "{}", op.name());
    }
}

/// D3's intolerant half, on the operation that carries the most data: a field this build does
/// not know, inside one it thinks it understands, is fatal — or a weaker grant gets applied and
/// nobody finds out.
#[test]
fn a_port_access_plan_with_a_field_this_build_does_not_know_is_fatal() {
    let value = serde_json::json!({
        "op": "port-access-grant",
        "plan": { "method": "capability", "binary": "/x/caddy", "ports": [80], "force": true }
    });

    assert!(serde_json::from_value::<PrivilegedOp>(value).is_err());
}

/// The T45 design, D3, written as a test so that putting the field back is a failure rather
/// than a review comment. The plan names TLDs and a port. It cannot name an address, because a
/// request that could name one is a request that could point this machine's name resolution
/// anywhere — with a valid signature and the user's own Allow click.
#[test]
fn a_resolver_plan_cannot_name_a_nameserver() {
    let plan = ResolverPlan::ResolverDirectory {
        tlds: vec!["test".to_owned()],
        port: 53_535,
    };

    let json = serde_json::to_string(&plan).expect("it serialises");

    assert!(!json.contains("127.0.0.1"), "{json}");
    assert!(!json.contains("nameserver"), "{json}");
    assert!(!json.contains("address"), "{json}");
}

/// D4: two answers to one question — what should this machine route? — so a revoke enqueued
/// behind a pending apply replaces it rather than queueing after it.
#[test]
fn both_directions_of_resolver_share_one_dedupe_key() {
    let apply = PrivilegedOp::ResolverApply {
        plan: ResolverPlan::Nrpt {
            tlds: vec!["test".to_owned()],
        },
    };
    let revoke = PrivilegedOp::ResolverRevoke {
        target: ResolverTarget::Nrpt {},
    };

    assert_eq!(apply.dedupe_key(), revoke.dedupe_key());
    assert_eq!(apply.dedupe_key(), "resolver");
    assert_ne!(
        apply.dedupe_key(),
        PrivilegedOp::hosts_apply(None, []).dedupe_key()
    );
}

/// Both need a token, and both are named in the audit log by their wire tag.
#[test]
fn resolver_operations_need_a_token_and_have_names() {
    let apply = PrivilegedOp::ResolverApply {
        plan: ResolverPlan::SystemdLink {
            tlds: vec!["test".to_owned()],
            port: 53_535,
        },
    };
    let revoke = PrivilegedOp::ResolverRevoke {
        target: ResolverTarget::SystemdLink {},
    };

    assert!(apply.requires_elevation());
    assert!(revoke.requires_elevation());
    assert_eq!(apply.name(), "resolver-apply");
    assert_eq!(revoke.name(), "resolver-revoke");
    assert!(PrivilegedOp::ALL.contains(&"resolver-apply"));
    assert!(PrivilegedOp::ALL.contains(&"resolver-revoke"));
}

/// T64's screen prints this before anything is raised, so it says the names and the files
/// rather than the mechanism's jargon.
#[test]
fn a_resolver_apply_describes_the_names_and_the_port() {
    let described = PrivilegedOp::ResolverApply {
        plan: ResolverPlan::ResolverDirectory {
            tlds: vec!["test".to_owned(), "internal".to_owned()],
            port: 53_535,
        },
    }
    .describe();

    assert!(described.contains("test"), "{described}");
    assert!(described.contains("internal"), "{described}");
    assert!(described.contains("53535"), "{described}");
    assert!(described.contains("/etc/resolver"), "{described}");
}

/// Windows has nowhere to put a port, so its description does not invent one.
#[test]
fn an_nrpt_plan_describes_no_port() {
    let described = PrivilegedOp::ResolverApply {
        plan: ResolverPlan::Nrpt {
            tlds: vec!["test".to_owned()],
        },
    }
    .describe();

    assert!(described.contains("test"), "{described}");
    assert!(!described.contains("port"), "{described}");
}

/// The intolerant half of the wire contract, as every other operation has it: an older helper
/// must refuse a field it does not understand rather than apply a weaker operation quietly.
#[test]
fn a_resolver_plan_with_an_unknown_field_is_refused() {
    let value = serde_json::json!({
        "op": "resolver-apply",
        "plan": { "method": "nrpt", "tlds": ["test"], "only-if": "something new" }
    });

    assert!(serde_json::from_value::<PrivilegedOp>(value).is_err());
}

/// D3: the plan carries the certificate itself and never a path to one.
#[test]
fn a_trust_plan_carries_the_certificate_and_not_a_path_to_it() {
    let encoded =
        serde_json::to_value(TrustPlan::SystemRoot { der: vec![1, 2, 3] }).expect("it encodes");

    assert_eq!(encoded["method"], "system-root");
    assert_eq!(encoded["der"], serde_json::json!([1, 2, 3]));
    assert!(
        encoded.get("path").is_none(),
        "a path is somebody else choosing which file root reads: {encoded}"
    );
}

/// D3's intolerant half: a helper that ignored a field inside a plan it thought it understood
/// would apply a weaker version of it and tell nobody.
#[test]
fn a_trust_plan_with_a_field_this_build_does_not_know_is_refused() {
    let value = serde_json::json!({ "method": "system-root", "der": [1], "and": "this" });

    assert!(serde_json::from_value::<TrustPlan>(value).is_err());
}

/// D5: there is no field a fingerprint could travel in, and that is this type's whole security
/// decision. A removal that named a certificate could name the root that validates Windows
/// Update; eight hex characters cannot describe one.
#[test]
fn a_trust_removal_names_an_authority_and_never_a_certificate() {
    let target = TrustTarget::SystemKeychain {
        key_id: "deadbeef".to_owned(),
    };

    let encoded = serde_json::to_string(&target).expect("it encodes");

    assert!(encoded.contains("deadbeef"), "{encoded}");
    assert!(
        !encoded.contains("fingerprint"),
        "a fingerprint field is what would let a compromised daemon name a corporate root: \
             {encoded}"
    );
}

/// D3: the operation the fixtures in `mixengine-elevate` and `mixengine-core` already spell.
#[test]
fn a_trust_install_is_tagged_the_way_the_existing_fixtures_spell_it() {
    let op = PrivilegedOp::TrustCaInstall {
        plan: TrustPlan::SystemRoot { der: vec![1, 2, 3] },
    };

    let encoded = serde_json::to_value(&op).expect("it encodes");

    assert_eq!(encoded["op"], "trust-ca-install");
    assert_eq!(encoded["plan"]["method"], "system-root");
    assert_eq!(encoded["plan"]["der"], serde_json::json!([1, 2, 3]));
}

/// Two values of one question, as the resolver pair is — D3, on T45's D4.
#[test]
fn installing_and_removing_supersede_each_other_in_the_queue() {
    let install = PrivilegedOp::TrustCaInstall {
        plan: TrustPlan::SystemRoot { der: vec![1] },
    };
    let remove = PrivilegedOp::TrustCaRemove {
        target: TrustTarget::SystemRoot {
            key_id: "deadbeef".to_owned(),
        },
    };

    assert_eq!(install.dedupe_key(), remove.dedupe_key());
    assert_eq!(install.dedupe_key(), "trust-store");
}

/// Or a daemon has to spend a prompt to discover what the installed helper can do.
#[test]
fn both_new_operations_are_in_the_reported_list() {
    assert!(PrivilegedOp::ALL.contains(&"trust-ca-install"));
    assert!(PrivilegedOp::ALL.contains(&"trust-ca-remove"));
}

/// Both need a token, because both change a store no ordinary account may write.
#[test]
fn trusting_and_untrusting_both_need_an_administrative_token() {
    assert!(
        PrivilegedOp::TrustCaInstall {
            plan: TrustPlan::SystemRoot { der: vec![1] },
        }
        .requires_elevation()
    );
    assert!(
        PrivilegedOp::TrustCaRemove {
            target: TrustTarget::SystemRoot {
                key_id: "deadbeef".to_owned(),
            },
        }
        .requires_elevation()
    );
}

/// The screen whose whole job is to say what is about to happen names the store, because
/// "trust a certificate" and "add a root for every account on this machine" are different
/// sentences and only the second is true.
#[test]
fn what_a_person_is_asked_to_allow_names_the_store() {
    let described = PrivilegedOp::TrustCaInstall {
        plan: TrustPlan::SystemKeychain { der: vec![1] },
    }
    .describe();

    assert!(described.contains("keychain"), "{described}");

    let described = PrivilegedOp::TrustCaRemove {
        target: TrustTarget::CaCertificates {
            key_id: "deadbeef".to_owned(),
        },
    }
    .describe();

    assert!(described.contains("deadbeef"), "{described}");
    assert!(described.contains("ca-certificates"), "{described}");
}

/// T87's two operations carry nothing, on `HelperInstall`'s rule: where each file is is a
/// constant compiled into the helper, so neither hands a compromised daemon a *delete this file
/// as root* primitive.
#[test]
fn the_two_removals_carry_no_field_and_refuse_one() {
    for (op, wire) in [
        (PrivilegedOp::HelperRemove {}, r#"{"op":"helper-remove"}"#),
        (
            PrivilegedOp::AuditLogRemove {},
            r#"{"op":"audit-log-remove"}"#,
        ),
    ] {
        assert_eq!(serde_json::to_string(&op).expect("it serialises"), wire);
        assert_eq!(
            serde_json::from_str::<PrivilegedOp>(wire).expect("it parses"),
            op
        );
    }

    serde_json::from_str::<PrivilegedOp>(r#"{"op":"helper-remove","path":"/tmp/x"}"#)
        .expect_err("a field this build does not know is an error, never a warning");
}

/// Two values of one question — is the helper where it belongs? — so a removal enqueued behind a
/// pending install replaces it rather than queueing after it. That is what stops an uninstall
/// having to drop the queue before it asks.
#[test]
fn installing_and_removing_the_helper_are_one_key() {
    assert_eq!(
        PrivilegedOp::HelperInstall {}.dedupe_key(),
        PrivilegedOp::HelperRemove {}.dedupe_key()
    );
}

/// T88a. The operation that makes an upgrade possible at all, and the one that carries a
/// signature requirement rather than a path.
#[test]
fn replacing_the_helper_is_an_operation_with_no_fields() {
    let op = PrivilegedOp::HelperReplace {};

    assert_eq!(op.name(), "helper-replace");
    assert!(op.requires_elevation());
    assert!(PrivilegedOp::ALL.contains(&op.name()));
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        serde_json::json!({ "op": "helper-replace" })
    );
    assert_eq!(
        serde_json::from_value::<PrivilegedOp>(serde_json::json!({ "op": "helper-replace" }))
            .unwrap(),
        op
    );
}

/// Three values of one question — which helper should this machine have — so a replacement
/// enqueued behind a pending install supersedes it rather than queueing after it.
#[test]
fn the_three_helper_operations_answer_one_question() {
    assert_eq!(
        PrivilegedOp::HelperReplace {}.dedupe_key(),
        PrivilegedOp::HelperInstall {}.dedupe_key()
    );
    assert_eq!(
        PrivilegedOp::HelperReplace {}.dedupe_key(),
        PrivilegedOp::HelperRemove {}.dedupe_key()
    );
}

/// The sentence somebody reads before clicking Allow has to say what makes this safe, because
/// what makes it safe is not the account it came from.
#[test]
fn replacing_the_helper_describes_the_check_that_makes_it_safe() {
    let described = PrivilegedOp::HelperReplace {}.describe();

    assert!(described.contains("signature"), "{described}");
    assert!(described.contains("replace"), "{described}");
    assert!(!described.contains('/'), "{described}");
    assert!(!described.contains('\\'), "{described}");
}

/// D3's intolerant half, on the operation that carries no fields: a `source` somebody added is
/// refused rather than dropped, because dropping it is how a weaker version of an operation
/// gets applied and nobody finds out.
#[test]
fn a_helper_replacement_with_a_field_is_unsupported() {
    let value = serde_json::json!({ "op": "helper-replace", "source": "/tmp/anything" });

    assert!(serde_json::from_value::<PrivilegedOp>(value).is_err());
}

/// And the audit log has no opposite: nothing installs it, the helper creates it on first run.
#[test]
fn the_audit_log_removal_is_a_key_of_its_own() {
    assert_eq!(PrivilegedOp::AuditLogRemove {}.dedupe_key(), "audit-log");
    assert_ne!(
        PrivilegedOp::AuditLogRemove {}.dedupe_key(),
        PrivilegedOp::HelperRemove {}.dedupe_key()
    );
}

/// Both reach inside a directory only an administrator can write, so there is nothing either
/// could do under an ordinary token but fail.
#[test]
fn both_removals_need_an_administrative_token() {
    assert!(PrivilegedOp::HelperRemove {}.requires_elevation());
    assert!(PrivilegedOp::AuditLogRemove {}.requires_elevation());
}

/// `ALL` is what the response reports as this build's vocabulary, and a name missing from it is
/// an operation a daemon would never learn it could ask for.
#[test]
fn every_operation_this_build_knows_is_named_in_all() {
    for name in ["helper-remove", "audit-log-remove"] {
        assert!(PrivilegedOp::ALL.contains(&name), "{name} is missing");
    }

    let mut unique = PrivilegedOp::ALL.to_vec();
    unique.sort_unstable();
    unique.dedup();

    assert_eq!(unique.len(), PrivilegedOp::ALL.len());
}

/// What a person reads before clicking Allow. No path in either sentence, because this crate has
/// none: which directory an OS keeps a helper or a log in is `mixengine_platform`'s answer.
#[test]
fn each_removal_describes_itself_without_naming_a_path() {
    for op in [
        PrivilegedOp::HelperRemove {},
        PrivilegedOp::AuditLogRemove {},
    ] {
        let sentence = op.describe();

        assert!(!sentence.is_empty());
        assert!(!sentence.contains('/'), "{sentence}");
        assert!(!sentence.contains('\\'), "{sentence}");
    }
}

#[test]
fn a_trusted_comment_is_read_as_a_stamp() {
    let stamp = HelperStamp::parse("mixengine-elevate 0.2.0 linux x86_64").expect("a stamp");

    assert_eq!(stamp.version, "0.2.0");
    assert_eq!(stamp.os, "linux");
    assert_eq!(stamp.arch, "x86_64");
}

/// Anything that is not this grammar is [`None`] rather than a stamp with empty fields: what
/// this value decides is whether a file is installed as root, and a partial reading is worse
/// than no reading at all.
#[test]
fn a_comment_that_is_not_the_grammar_is_not_a_stamp() {
    for comment in [
        "",
        "timestamp:1757030400	file:mixengine-elevate	hashed",
        "mixengine-elevate 0.2.0 linux",
        "mixengine-elevate 0.2.0 linux x86_64 extra",
        "mixengined 0.2.0 linux x86_64",
    ] {
        assert!(HelperStamp::parse(comment).is_none(), "{comment}");
    }
}

/// A signature that verifies says nothing about which machine the bytes are for, and installing
/// another architecture's helper as root is a machine that can no longer elevate anything.
#[test]
fn a_stamp_for_another_machine_is_not_for_this_host() {
    let host = HelperStamp {
        version: "0.2.0".to_owned(),
        os: HelperStamp::host_os().to_owned(),
        arch: HelperStamp::host_arch().to_owned(),
    };
    assert!(host.is_for_host());

    let elsewhere = HelperStamp {
        os: "plan9".to_owned(),
        ..host.clone()
    };
    assert!(!elsewhere.is_for_host());

    let other_arch = HelperStamp {
        arch: "s390x".to_owned(),
        ..host
    };
    assert!(!other_arch.is_for_host());
}

/// macOS publishes one universal helper under two architecture rows, so `universal` is a third
/// spelling of "this machine" there and of nothing anywhere else.
#[test]
fn universal_is_this_machine_only_on_macos() {
    let universal = HelperStamp {
        version: "0.2.0".to_owned(),
        os: HelperStamp::host_os().to_owned(),
        arch: "universal".to_owned(),
    };

    assert_eq!(universal.is_for_host(), cfg!(target_os = "macos"));
}

/// Both sides compose this from the request's own `home`, so it is one function and not two
/// spellings that agree until somebody edits one.
#[test]
fn the_candidate_sits_under_the_homes_run_directory() {
    let home = PathBuf::from("/srv/mixengine");
    let name = format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX);

    assert_eq!(helper_candidate_dir(&home), home.join("run").join("helper"));
    assert_eq!(
        helper_candidate(&home),
        helper_candidate_dir(&home).join(&name)
    );
    assert_eq!(
        helper_candidate_signature(&home),
        helper_candidate_dir(&home).join(format!("{name}.minisig"))
    );
}

/// T211g: this home's block, another home's and the legacy block are three questions, so three
/// rows — an uninstall asks for two of them in one batch.
#[test]
fn own_legacy_and_orphan_blocks_are_separate_queue_rows() {
    let own = PrivilegedOp::hosts_apply(Some("aa".to_owned()), []);
    let orphan = PrivilegedOp::hosts_apply(Some("bb".to_owned()), []);
    let legacy = PrivilegedOp::hosts_apply(None, []);

    assert_ne!(own.dedupe_key(), legacy.dedupe_key());
    assert_ne!(own.dedupe_key(), orphan.dedupe_key());
    assert_eq!(
        legacy.dedupe_key(),
        "hosts-apply",
        "the key rows queued before T211g already carry, so they need no migration"
    );
}

/// A row queued before T211g carries no `home`, and reads as the legacy block it was written for.
#[test]
fn a_hosts_apply_without_home_reads_as_the_legacy_block() {
    let value = serde_json::json!({ "op": "hosts-apply", "entries": [] });

    assert_eq!(
        serde_json::from_value::<PrivilegedOp>(value).unwrap(),
        PrivilegedOp::hosts_apply(None, [])
    );
    assert!(
        !serde_json::to_string(&PrivilegedOp::hosts_apply(None, []))
            .unwrap()
            .contains("home"),
        "the legacy block serialises as it always did"
    );
}
