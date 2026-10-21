//! What leaves the queue on its own, and the firewall plans recorded.

use super::*;

/// **T179.** A site is created, deleted before anybody clicks Allow, and the row it queued goes
/// with it: the queue never describes a home that no longer declares the name, and a grant
/// cannot write one into the machine's hosts file.
#[tokio::test]
async fn a_hosts_entry_nobody_declares_any_more_leaves_the_queue() {
    let (home, elevation, events, _machine) =
        registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
    with_an_installed_helper(&home);
    let mut watching = events.subscribe();

    a_site_named(&elevation.store, "gone.local").await;
    elevation.require_hosts().await.unwrap();
    assert_eq!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        watching.next().await,
        Some(crate::api::events::Frame::Event(
            DaemonEvent::ElevationRequired { .. }
        ))
    ));

    sqlx::query("DELETE FROM sites")
        .execute(elevation.store.pool())
        .await
        .unwrap();
    elevation.require_hosts().await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty(),
        "the queue still holds a name this home no longer declares"
    );

    let Some(crate::api::events::Frame::Event(DaemonEvent::ElevationRequired { pending })) =
        watching.next().await
    else {
        panic!("a withdrawal publishes the queue that is left")
    };
    assert!(pending.is_empty(), "{pending:?}");
}

/// A machine this daemon could not read is a machine it knows nothing about, so the queue is
/// left exactly as it was (the T179 design, D2).
#[tokio::test]
async fn a_hosts_file_that_cannot_be_read_withdraws_nothing() {
    let (home, elevation, _events, _machine) = registry(mock::Host::unable_to_read_the_hosts_file(
        "/tmp/mixengine",
        "the file is not readable",
    ))
    .await;
    with_an_installed_helper(&home);

    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'hosts-apply', 1)",
    )
    .execute(elevation.store.pool())
    .await
    .unwrap();

    elevation.require_hosts().await.unwrap();

    assert_eq!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .len(),
        1,
        "a failed reading withdrew a row"
    );
}

/// **T179**, for the resolver: a machine that already routes what it should has nothing waiting
/// for it, whatever an earlier reading queued.
#[tokio::test]
async fn a_resolver_that_already_routes_withdraws_what_was_queued() {
    let (home, elevation, _events, _machine) = registry_resolving(
        mock::Host::with_resolver(
            "/tmp/mixengine",
            mixengine_platform::ResolverMethod::Nrpt,
            &mixengine_proto::domains::WIRED_TLDS,
        ),
        crate::dns::Dns::wired_for_tests(),
    )
    .await;
    with_an_installed_helper(&home);

    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'resolver', 1)",
    )
    .execute(elevation.store.pool())
    .await
    .unwrap();

    elevation.require_resolver().await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty(),
        "the resolver row outlived the machine agreeing"
    );
}

/// **T179**, for the trust store: a home with no authority has nothing to be trusted, so an
/// install queued for an authority that is gone goes with it (the design's Settled 1).
#[tokio::test]
async fn a_home_with_no_authority_withdraws_a_queued_trust_install() {
    let (home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;
    with_an_installed_helper(&home);

    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'trust-store', 1)",
    )
    .execute(elevation.store.pool())
    .await
    .unwrap();

    elevation.require_trust_store(None).await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// **T179**, for the port grant: a binary that already holds what it needs withdraws the grant
/// nobody answered. **Withdrawing is not revoking** — the machine keeps the capability.
#[tokio::test]
async fn a_binary_that_already_answers_withdraws_the_grant_nobody_answered() {
    let (home, elevation, _events, _machine) = registry(mock::Host::with_port_access(
        "/tmp/mixengine",
        mixengine_platform::PortAccessMethod::Capability,
    ))
    .await;
    with_an_installed_helper(&home);

    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'port-access', 1)",
    )
    .execute(elevation.store.pool())
    .await
    .unwrap();

    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy")))
        .await
        .unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// A probe that could not read the machine says nothing about what to stop asking for, so a
/// home with no front end leaves the queue alone (the T179 design, D2).
#[tokio::test]
async fn a_home_with_no_front_end_withdraws_nothing() {
    let (home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;
    with_an_installed_helper(&home);

    sqlx::query(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'port-access', 1)",
    )
    .execute(elevation.store.pool())
    .await
    .unwrap();

    elevation.require_port_access(None).await.unwrap();

    assert_eq!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .len(),
        1,
        "a home that cannot read the machine withdrew a row anyway"
    );
}

/// **T180.** A grant that carried a firewall plan records it, so the producer can tell a plan
/// this machine already holds from one it does not.
#[tokio::test]
async fn a_settled_firewall_plan_is_recorded() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    let plan = mixengine_proto::privileged::FirewallPlan {
        ports: vec![80, 443],
        label: "MixEngine — shared sites".to_owned(),
    };
    let waiting = vec![mixengine_proto::PendingOp {
        id: mixengine_proto::PendingOpId(7),
        op: PrivilegedOp::FirewallApply { plan: plan.clone() },
        description: "open two ports".to_owned(),
        requested_at: mixengine_proto::Timestamp(1),
        needs_the_screen: None,
    }];

    for outcome in [
        OpOutcome::Applied {
            detail: "two rules".to_owned(),
        },
        OpOutcome::AlreadyDone,
        OpOutcome::Unmanaged {
            reason: "this machine has no firewall to write".to_owned(),
            manual: "open the port yourself".to_owned(),
        },
    ] {
        mixengine_core::updates::records::clear(&elevation.store, FIREWALL_APPLIED)
            .await
            .unwrap();

        elevation
            .remember_the_firewall(&waiting, &[(mixengine_proto::PendingOpId(7), outcome)])
            .await;

        let recorded: Option<mixengine_proto::privileged::FirewallPlan> =
            mixengine_core::updates::records::get(&elevation.store, FIREWALL_APPLIED)
                .await
                .unwrap();
        assert_eq!(recorded.as_ref(), Some(&plan));
    }
}

/// An operation that was refused, unsupported or failed changed nothing, so it records nothing:
/// the next sharing change has to ask again.
#[tokio::test]
async fn a_firewall_plan_that_did_not_land_records_nothing() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    let plan = mixengine_proto::privileged::FirewallPlan {
        ports: vec![80],
        label: "MixEngine — shared sites".to_owned(),
    };
    let waiting = vec![mixengine_proto::PendingOp {
        id: mixengine_proto::PendingOpId(3),
        op: PrivilegedOp::FirewallApply { plan },
        description: "open one port".to_owned(),
        requested_at: mixengine_proto::Timestamp(1),
        needs_the_screen: None,
    }];

    for outcome in [
        OpOutcome::Refused {
            reason: "no".to_owned(),
        },
        OpOutcome::Unsupported {
            reason: "not here".to_owned(),
        },
        OpOutcome::Failed {
            message: "the tool said no".to_owned(),
        },
    ] {
        elevation
            .remember_the_firewall(
                &waiting,
                &[(mixengine_proto::PendingOpId(3), outcome.clone())],
            )
            .await;

        let recorded: Option<mixengine_proto::privileged::FirewallPlan> =
            mixengine_core::updates::records::get(&elevation.store, FIREWALL_APPLIED)
                .await
                .unwrap();
        assert_eq!(recorded, None, "{outcome:?} recorded a plan");
    }
}
